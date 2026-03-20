use notify::{Event, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::collections::HashSet;
use tokio::sync::mpsc;
use tracing::{error, info, debug, warn};
use std::fs;

use crate::core::router::Router;
use crate::transfer::client::send_file;

pub struct LocalInboxes {
    // Thread-safe list of the user's currently available inboxes
    pub folders: Arc<RwLock<HashSet<String>>>,
}

impl LocalInboxes {
    pub fn new() -> Self {
        Self {
            folders: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Read the inbox dir initially
    pub fn scan_initial(&self, inbox_dir: &std::path::Path) {
        let mut set = self.folders.write().unwrap();
        set.clear();
        if let Ok(entries) = fs::read_dir(inbox_dir) {
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type() {
                    if file_type.is_dir() {
                        if let Ok(name) = entry.file_name().into_string() {
                            set.insert(name);
                        }
                    }
                }
            }
        }
        info!("Initial local inboxes: {:?}", *set);
    }
}

/// Parses a file path under the send directory into (inbox_name, relative_file_path).
/// Returns None if the path doesn't represent a valid sendable file.
fn parse_send_file_path(path: &std::path::Path, send_dir: &std::path::Path) -> Option<(String, String)> {
    let relative = path.strip_prefix(send_dir).ok()?;
    let mut components = relative.components();
    let inbox_name = components.next()?.as_os_str().to_str()?.to_string();
    let rel_file_path: PathBuf = components.collect();
    if rel_file_path.as_os_str().is_empty() {
        return None;
    }
    Some((inbox_name, rel_file_path.to_string_lossy().replace("\\", "/")))
}

/// Recursively collects all file paths under a directory.
fn collect_files_recursive(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        if let Ok(entries) = fs::read_dir(&current) {
            for entry in entries.flatten() {
                if let Ok(ft) = entry.file_type() {
                    let path = entry.path();
                    if ft.is_dir() {
                        stack.push(path);
                    } else if ft.is_file() {
                        files.push(path);
                    }
                }
            }
        }
    }
    files
}

/// Watches the `inbox` folder for new subdirectories created/deleted by the user
pub fn watch_inbox_directory(inbox_dir: PathBuf, local_inboxes: Arc<LocalInboxes>) {
    let (tx, mut rx) = mpsc::unbounded_channel();

    let inbox_dir_clone = inbox_dir.clone();
    // Setup file watcher using notify
    std::thread::spawn(move || {
        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        }).expect("Failed to create file watcher");

        watcher.watch(&inbox_dir_clone, RecursiveMode::NonRecursive).expect("Failed to watch inbox dir");

        // Block thread to keep watcher alive
        loop {
            std::thread::park();
        }
    });

    // Handle events asynchronously
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            // For now, on any structural change inside `inbox/`, we rescan the whole folder.
            // This is simple, resilient to weird edge cases, and fast enough for a single folder.
            if event.kind.is_create() || event.kind.is_remove() || event.kind.is_modify() {
                if let Some(path) = event.paths.first() {
                    // Because we watch `inbox_dir` NonRecursive, any event here relates to it.
                    // Instead of strict equality parent checking, just verify it starts with `inbox_dir`
                    // or is `inbox_dir` itself.
                    if path.starts_with(&inbox_dir) || path == &inbox_dir {
                        local_inboxes.scan_initial(&inbox_dir);
                    }
                }
            }
        }
    });
}

/// Scans the `send/{inbox_name}` directory for any files that might have been dropped while the peer was offline
fn rescan_send_directory_for_inbox(
    inbox_name: &str,
    send_dir: &std::path::Path,
    router: &Router,
    in_progress: &Arc<std::sync::Mutex<HashSet<PathBuf>>>,
    crypto_key: &Arc<[u8; 32]>
) {
    let inbox_dir = send_dir.join(inbox_name);
    if !inbox_dir.exists() {
        return;
    }

    for path in collect_files_recursive(&inbox_dir) {
        if let Some((inbox, rel_path)) = parse_send_file_path(&path, send_dir) {
            let mut set = in_progress.lock().unwrap();
            if !set.contains(&path) {
                set.insert(path.clone());
                drop(set);
                handle_new_file_to_send(
                    path, inbox, rel_path, router.clone(),
                    in_progress.clone(), send_dir.to_path_buf(), crypto_key.clone(),
                );
            }
        }
    }
}

/// Watches the `send` directory. If a file is placed here, we wait for writer lock release and send it.
pub fn watch_send_directory(send_dir: PathBuf, router: Router, crypto_key: Arc<[u8; 32]>) {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut router_rx = router.subscribe_new_inboxes();

    let send_dir_clone_for_watcher = send_dir.clone();
    // Setup file watcher using notify for the `send` dir recursively
    std::thread::spawn(move || {
        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        }).expect("Failed to create sender file watcher");

        watcher.watch(&send_dir_clone_for_watcher, RecursiveMode::Recursive).expect("Failed to watch send dir");

        loop {
            std::thread::park();
        }
    });

    let send_dir_clone = send_dir.clone();

    // We use a deduplication set to avoid processing the same file multiple times
    // due to multiple creation/modification events fired by the OS.
    let in_progress = Arc::new(std::sync::Mutex::new(HashSet::new()));

    tokio::spawn(async move {
        // Run an initial rescan over all current peers known just in case
        // Some might have already been discovered before this watcher spawned
        // But since we just started, usually it's empty. We let router events trigger rescans.

        loop {
            tokio::select! {
                Some(event) = rx.recv() => {
                    if event.kind.is_create() || event.kind.is_modify() {
                        let all_paths: Vec<PathBuf> = event.paths.into_iter().flat_map(|p| {
                            if p.is_dir() {
                                collect_files_recursive(&p)
                            } else if p.is_file() {
                                vec![p]
                            } else {
                                vec![]
                            }
                        }).collect();

                        for path in all_paths {
                            if let Some((inbox_name, rel_file_path)) = parse_send_file_path(&path, &send_dir_clone) {
                                let mut set = in_progress.lock().unwrap();
                                if !set.contains(&path) {
                                    set.insert(path.clone());
                                    drop(set);
                                    handle_new_file_to_send(
                                        path, inbox_name, rel_file_path, router.clone(),
                                        in_progress.clone(), send_dir_clone.clone(), crypto_key.clone(),
                                    );
                                }
                            }
                        }
                    }
                }
                Ok(new_inbox) = router_rx.recv() => {
                    debug!("Rescanning send directory for newly discovered inbox: {}", new_inbox);
                    rescan_send_directory_for_inbox(
                        &new_inbox,
                        &send_dir_clone,
                        &router,
                        &in_progress,
                        &crypto_key
                    );
                }
            }
        }
    });
}

fn handle_new_file_to_send(path: PathBuf, inbox_name: String, relative_file_path: String, router: Router, in_progress: Arc<std::sync::Mutex<HashSet<PathBuf>>>, send_dir: PathBuf, crypto_key: Arc<[u8; 32]>) {
    tokio::spawn(async move {
        // Debounce: Wait until the file is no longer exclusively locked by another process
        let mut retries = 0;
        loop {
            match std::fs::OpenOptions::new().write(true).open(&path) {
                Ok(_) => break, // Got write access, meaning another app released it
                Err(_) => {
                    retries += 1;
                    if retries > 30 {
                        warn!("Timeout waiting for file unlock: {:?}", path);
                        in_progress.lock().unwrap().remove(&path);
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }
            }
        }

        // Wait an extra tiny bit after unlock to be fully sure the file is entirely written
        // Some programs release lock then re-acquire, a 500ms delay helps smoothing this edge case.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Look up targets for the inbox
        let targets = router.get_peers_for_inbox(&inbox_name);

        if targets.is_empty() {
            debug!("No targets found for inbox '{}'. Ignoring file for now.", inbox_name);
            in_progress.lock().unwrap().remove(&path);
            return;
        }

        if targets.len() > 1 {
            info!("Inbox '{}' is claimed by {} multiple peers! Broadcasting file to all of them.", inbox_name, targets.len());
        }

        info!("Initiating transfer of {:?} to inbox '{}'", path, inbox_name);

        let mut all_success = true;
        for target in targets {
            if let Err(e) = send_file(target, inbox_name.clone(), path.clone(), relative_file_path.clone(), crypto_key.clone()).await {
                error!("Failed to send to {}: {}", target, e);
                all_success = false;
            }
        }

        if all_success {
            // Cleanup: Once sending attempt is complete AND SUCCESSFUL, we delete the file
            if let Err(e) = tokio::fs::remove_file(&path).await {
                error!("Failed to clean up sent file {:?}: {}", path, e);
            } else {
                info!("Cleaned up {:?}", path);

                // Clean up empty parent directories up to the `send_dir` root
                let mut current_dir = path.parent().map(PathBuf::from);
                while let Some(dir) = current_dir {
                    // Do not delete folders outside or equal to our send_dir root, NOR the actual inbox folder inside it
                    if dir == send_dir || dir.parent() == Some(send_dir.as_path()) {
                        break;
                    }

                    // Attempt to remove the directory. This safely fails if the dir is not empty.
                    if std::fs::remove_dir(&dir).is_err() {
                        break;
                    }

                    current_dir = dir.parent().map(PathBuf::from);
                }
            }
        } else {
            error!("Transfer failed for {:?}, retaining file for future attempt", path);
        }

        // Finally, remove from in-progress tracking
        in_progress.lock().unwrap().remove(&path);
    });
}

