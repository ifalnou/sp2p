use std::process::{Command, Child};
use std::time::Duration;
use tempfile::TempDir;
use std::fs;
use std::path::PathBuf;

struct AppInstance {
    child: Child,
    dir: TempDir,
}

impl AppInstance {
    fn new(name: &str, port: u16, network: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();

        // Ensure directories exist
        let inbox_dir = dir.path().join("inbox");
        let send_dir = dir.path().join("send");
        fs::create_dir_all(&inbox_dir).unwrap();
        fs::create_dir_all(&send_dir).unwrap();

        let exe = env!("CARGO_BIN_EXE_sp2p");
        let child = Command::new(exe)
            .arg("--name")
            .arg(name)
            .arg("--port")
            .arg(port.to_string())
            .arg("--network")
            .arg(network)
            .arg("--dir")
            .arg(dir.path().to_str().unwrap())
            .arg("--no-upnp")
            .arg("--no-tray")
            .spawn()
            .unwrap();

        AppInstance {
            child,
            dir,
        }
    }

    fn inbox_path(&self) -> PathBuf {
        self.dir.path().join("inbox")
    }

    fn send_path(&self) -> PathBuf {
        self.dir.path().join("send")
    }

    fn create_inbox(&self, name: &str) {
        fs::create_dir_all(self.inbox_path().join(name)).unwrap();
    }

    fn remove_inbox(&self, name: &str) {
        fs::remove_dir_all(self.inbox_path().join(name)).unwrap();
    }

    fn send_file(&self, inbox_name: &str, file_name: &str, content: &str) {
        let target_dir = self.send_path().join(inbox_name);
        let full_path = target_dir.join(file_name);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(full_path, content).unwrap();
    }
}

impl Drop for AppInstance {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn test_end_to_end_file_transfer() {
    let app1 = AppInstance::new("E2E 1", 10081, "net_e2e");
    let app2 = AppInstance::new("E2E 2", 10082, "net_e2e");

    // Wait for apps to startup
    std::thread::sleep(Duration::from_secs(2));

    // App 2 creates an inbox "test_inbox"
    app2.create_inbox("test_inbox");

    // Wait for App 1 to discover the new inbox.
    // Broadcasts happen every 5 seconds. We wait 6 seconds to be safe.
    std::thread::sleep(Duration::from_secs(6));

    // App 1 sends a file to "test_inbox"
    app1.send_file("test_inbox", "hello.txt", "Hello World From App 1");

    // Wait for file transfer to complete (plus debounce delay of 0.5s in watcher)
    std::thread::sleep(Duration::from_secs(3));

    // Check if App 2 received the file
    let received_file = app2.inbox_path().join("test_inbox").join("hello.txt");
    assert!(received_file.exists(), "File was not received by app2");

    let content = fs::read_to_string(&received_file).unwrap();
    assert_eq!(content, "Hello World From App 1", "File content mismatch");

    // Now let's test deleting an inbox.
    app2.remove_inbox("test_inbox");

    // Wait for 6 seconds to allow broadcast packet indicating deletion to arrive
    std::thread::sleep(Duration::from_secs(6));

    // App 1 sends a file to the removed "test_inbox"
    app1.send_file("test_inbox", "deleted.txt", "Should Not Arrive");

    std::thread::sleep(Duration::from_secs(3));

    // Check if App 2 received the file (it shouldn't have, or at least the inbox doesn't exist)
    let bad_file = app2.inbox_path().join("test_inbox").join("deleted.txt");
    assert!(!bad_file.exists(), "File was received by app2 despite inbox being deleted");
}

#[test]
fn test_nested_folder_transfer() {
    let app1 = AppInstance::new("Nested 1", 10083, "net_nested");
    let app2 = AppInstance::new("Nested 2", 10084, "net_nested");

    // Wait for apps to startup
    std::thread::sleep(Duration::from_secs(2));

    // App 2 creates an inbox "media_inbox"
    app2.create_inbox("media_inbox");

    // Wait for App 1 to discover the new inbox.
    std::thread::sleep(Duration::from_secs(6));

    // App 1 sends a nested file to "media_inbox"
    app1.send_file("media_inbox", "photos/summer/pic.jpg", "JPEG Data");

    // Wait for file transfer to complete
    std::thread::sleep(Duration::from_secs(3));

    // Check if App 2 received the nested file
    let received_file = app2.inbox_path().join("media_inbox").join("photos/summer/pic.jpg");
    assert!(received_file.exists(), "Nested file was not received by app2");

    let content = fs::read_to_string(&received_file).unwrap();
    assert_eq!(content, "JPEG Data", "File content mismatch for nested file");
}

#[test]
fn test_delayed_transfer() {
    let app1 = AppInstance::new("Delayed 1", 10085, "net_delayed");

    // Give app1 time to startup
    std::thread::sleep(Duration::from_secs(2));

    // App 1 sends a file to "offline_inbox", but app2 isn't running yet!
    app1.send_file("offline_inbox", "delayed.txt", "Better late than never");

    // Wait 2 seconds to ensure app1 processed the file system event and saw no peers
    std::thread::sleep(Duration::from_secs(2));

    // Now start app2
    let app2 = AppInstance::new("Delayed 2", 10086, "net_delayed");

    // Have app2 create the inbox we're waiting for
    app2.create_inbox("offline_inbox");

    // Wait for App 1 to discover app2 and the new inbox, and for the delayed transfer to execute.
    // Broadcasts happen every 5 seconds, so wait 8 seconds to be safe.
    std::thread::sleep(Duration::from_secs(8));

    // Check if App 2 received the file that was waiting in app1's send directory
    let received_file = app2.inbox_path().join("offline_inbox").join("delayed.txt");
    assert!(received_file.exists(), "Delayed file was not received by app2");

    let content = fs::read_to_string(&received_file).unwrap();
    assert_eq!(content, "Better late than never", "File content mismatch for delayed file");
}

#[test]
fn test_multiple_peers_same_inbox() {
    let app1 = AppInstance::new("Alice", 10087, "net_multi");
    let app2 = AppInstance::new("Bob", 10088, "net_multi");
    let app3 = AppInstance::new("Charlie", 10089, "net_multi");

    std::thread::sleep(Duration::from_secs(2));

    // Apps 2 and 3 both create the *same* inbox
    app2.create_inbox("team_inbox");
    app3.create_inbox("team_inbox");

    // Wait for App 1 to discover both peers for "team_inbox"
    std::thread::sleep(Duration::from_secs(8));

    // App 1 sends a file to "team_inbox"
    app1.send_file("team_inbox", "broadcast.txt", "Hello to everyone");

    // Wait for transfers
    std::thread::sleep(Duration::from_secs(4));

    // Check if App 2 received the file
    let received_file2 = app2.inbox_path().join("team_inbox").join("broadcast.txt");
    assert!(received_file2.exists(), "File was not received by app2");
    assert_eq!(fs::read_to_string(&received_file2).unwrap(), "Hello to everyone");

    // Check if App 3 received the file
    let received_file3 = app3.inbox_path().join("team_inbox").join("broadcast.txt");
    assert!(received_file3.exists(), "File was not received by app3");
    assert_eq!(fs::read_to_string(&received_file3).unwrap(), "Hello to everyone");
}
