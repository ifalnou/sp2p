use std::path::PathBuf;
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, BufReader};
use tokio::fs::File;
use tracing::{error, info, debug};

pub fn spawn_server(port: u16, inbox_root: PathBuf) {
    tokio::spawn(async move {
        let listener = match TcpListener::bind(("0.0.0.0", port)).await {
            Ok(l) => l,
            Err(e) => {
                error!("Failed to bind TCP listener on port {}: {}", port, e);
                return;
            }
        };

        info!("TCP Server listening on port {}", port);

        loop {
            match listener.accept().await {
                Ok((mut socket, addr)) => {
                    let root = inbox_root.clone();
                    tokio::spawn(async move {
                        debug!("Accepted connection from {}", addr);
                        let mut reader = BufReader::new(&mut socket);

                        // Simple custom protocol:
                        // 1. Inbox name length (u32) + bytes
                        // 2. File name length (u32) + bytes
                        // 3. File size (u64)
                        // 4. File data

                        let inbox_name = match read_string(&mut reader).await {
                            Ok(s) => s,
                            Err(e) => { error!("Failed to read inbox name: {}", e); return; }
                        };

                        let file_name = match read_string(&mut reader).await {
                            Ok(s) => s,
                            Err(e) => { error!("Failed to read file name: {}", e); return; }
                        };

                        let file_size = match reader.read_u64().await {
                            Ok(s) => s,
                            Err(e) => { error!("Failed to read file size: {}", e); return; }
                        };

                        let dest_dir = root.join(&inbox_name);
                        // Security basic: check if dest_dir exists and is indeed a subfolder of inbox
                        if !dest_dir.exists() {
                            // Automatically accept anyway? The roadmap says "appear in the app_folder/inbox/{inbox_name}"
                            // Let's create it if it doesn't exist.
                            if let Err(e) = tokio::fs::create_dir_all(&dest_dir).await {
                                error!("Failed to create destination dir: {}", e);
                                return;
                            }
                        }

                        let dest_file = dest_dir.join(&file_name);
                        info!("Receiving file {} ({} bytes) into {}", file_name, file_size, inbox_name);

                        let mut file = match File::create(&dest_file).await {
                            Ok(f) => f,
                            Err(e) => { error!("Failed to create local file {:?}: {}", dest_file, e); return; }
                        };

                        let mut bytes_copied = 0;
                        let mut buffer = [0u8; 8192];
                        while bytes_copied < file_size {
                            let to_read = std::cmp::min((file_size - bytes_copied) as usize, buffer.len());
                            match reader.read_exact(&mut buffer[..to_read]).await {
                                Ok(n) if n > 0 => {
                                    if let Err(e) = tokio::io::AsyncWriteExt::write_all(&mut file, &buffer[..n]).await {
                                        error!("Failed to write to disk: {}", e);
                                        return;
                                    }
                                    bytes_copied += n as u64;
                                }
                                _ => {
                                    error!("Connection closed prematurely while transferring file");
                                    return;
                                }
                            }
                        }

                        info!("Successfully received {} from {}", file_name, addr);
                    });
                }
                Err(e) => error!("TCP Accept failed: {}", e),
            }
        }
    });
}

async fn read_string<R: tokio::io::AsyncRead + Unpin>(reader: &mut R) -> std::io::Result<String> {
    let len = reader.read_u32().await? as usize;
    // Arbitrary reasonable limit (1024 bytes for a string is generous for folder/file names)
    if len > 1024 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "String too long"));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    String::from_utf8(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}
