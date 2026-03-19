use notify_rust::Notification;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time;

use tracing::{debug, error};

pub struct FileReceivedEvent {
    pub inbox_name: String,
    pub relative_path: String,
}

struct PendingNotification {
    root_folder: String, // the topmost folder inside the inbox, or the filename if it's just a file at the root
    file_count: usize,
    last_update: time::Instant,
}

pub fn spawn_notification_debouncer() -> mpsc::UnboundedSender<FileReceivedEvent> {
    let (tx, mut rx) = mpsc::unbounded_channel::<FileReceivedEvent>();

    tokio::spawn(async move {
        // Map from inbox_name to a Map of root objects -> pending notifications
        let mut grouped_notifications: HashMap<String, HashMap<String, PendingNotification>> = HashMap::new();
        let timeout = Duration::from_secs(10);
        let mut interval = time::interval(Duration::from_secs(2));

        loop {
            tokio::select! {
                Some(event) = rx.recv() => {
                    // Extract root path
                    let root_item = event.relative_path
                        .split(&['/', '\\'][..])
                        .next()
                        .unwrap_or(&event.relative_path)
                        .to_string();

                    let inbox_group = grouped_notifications.entry(event.inbox_name.clone()).or_default();
                    let pending = inbox_group.entry(root_item.clone()).or_insert(PendingNotification {
                        root_folder: root_item,
                        file_count: 0,
                        last_update: time::Instant::now(),
                    });

                    pending.file_count += 1;
                    pending.last_update = time::Instant::now();
                    debug!("Notification debouncer received file in '{}'. Tracking {} files for this root.", event.inbox_name, pending.file_count);
                }
                _ = interval.tick() => {
                    let now = time::Instant::now();

                    for (inbox, root_group) in grouped_notifications.iter_mut() {
                        let mut to_remove = Vec::new();
                        for (root_name, pending) in root_group.iter() {
                            if now.duration_since(pending.last_update) >= timeout {
                                // Flush notification
                                if pending.file_count == 1 {
                                    send_toast(&format!("Received item in '{}'", inbox), &format!("'{}' has been received.", pending.root_folder));
                                } else {
                                    send_toast(&format!("Received folder in '{}'", inbox), &format!("'{}' and {} other files have been received.", pending.root_folder, pending.file_count - 1));
                                }
                                to_remove.push(root_name.clone());
                            }
                        }

                        for r in to_remove {
                            root_group.remove(&r);
                        }
                    }
                }
            }
        }
    });

    tx
}

fn send_toast(summary: &str, body: &str) {
    if let Err(e) = Notification::new()
        .summary(summary)
        .body(body)
        .appname("sp2p")
        .show()
    {
        error!("Failed to send desktop notification: {}", e);
    }
}
