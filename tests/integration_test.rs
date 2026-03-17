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
    fn new(port: u16) -> Self {
        let dir = tempfile::tempdir().unwrap();
        
        // Ensure directories exist
        let inbox_dir = dir.path().join("inbox");
        let send_dir = dir.path().join("send");
        fs::create_dir_all(&inbox_dir).unwrap();
        fs::create_dir_all(&send_dir).unwrap();

        let exe = env!("CARGO_BIN_EXE_sp2p");
        let child = Command::new(exe)
            .arg("--port")
            .arg(port.to_string())
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
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join(file_name), content).unwrap();
    }
}

impl Drop for AppInstance {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[test]
fn test_end_to_end_file_transfer() {
    let app1 = AppInstance::new(10081);
    let app2 = AppInstance::new(10082);

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
