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
            if event.kind.is_create() || event.kind.is_remove() {
                // Determine the folder that changed from paths
                if let Some(path) = event.paths.first() {
                    if let Some(parent) = path.parent() {
                        if parent == inbox_dir {
                            local_inboxes.scan_initial(&inbox_dir);
                        }
                    }
                }
            }
        }
    });
}

/// Watches the `send` directory. If a file is placed here, we wait for writer lock release and send it.
pub fn watch_send_directory(send_dir: PathBuf, router: Router) {
    let (tx, mut rx) = mpsc::unbounded_channel();

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
        while let Some(event) = rx.recv().await {
            if event.kind.is_create() || event.kind.is_modify() {
                for path in event.paths {
                    if !path.is_file() {
                        continue;
                    }

                    // Only react if it's placed inside a subfolder representing an inbox name
                    // e.g., send/bob/document.txt
                    if let Some(parent) = path.parent() {
                        if parent != send_dir_clone {
                            if let Some(inbox_os_str) = parent.file_name() {
                                if let Some(inbox_name) = inbox_os_str.to_str() {
                                    let mut set = in_progress.lock().unwrap();
                                    if !set.contains(&path) {
                                        set.insert(path.clone());
                                        handle_new_file_to_send(
                                            path.clone(),
                                            inbox_name.to_string(),
                                            router.clone(),
                                            in_progress.clone()
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    });
}

fn handle_new_file_to_send(path: PathBuf, inbox_name: String, router: Router, in_progress: Arc<std::sync::Mutex<HashSet<PathBuf>>>) {
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

        for target in targets {
            if let Err(e) = send_file(target, inbox_name.clone(), path.clone()).await {
                error!("Failed to send to {}: {}", target, e);
            }
        }

        // Cleanup: Once sending attempt is complete, we delete the file
        if let Err(e) = tokio::fs::remove_file(&path).await {
            error!("Failed to clean up sent file {:?}: {}", path, e);
        } else {
            info!("Cleaned up {:?}", path);
        }

        // Finally, remove from in-progress tracking
        in_progress.lock().unwrap().remove(&path);
    });
}

