use std::path::PathBuf;
use tokio::net::TcpListener;
use tokio::io::{BufReader, AsyncWriteExt};
use tokio::io::AsyncSeekExt;
use crate::core::crypto::write_noise;
use tracing::{error, info, debug};
use std::sync::Arc;
use snow::TransportState;
use crate::core::crypto::{noise_server_handshake, read_noise};

pub fn spawn_server(port: u16, inbox_root: PathBuf, crypto_key: Arc<[u8; 32]>) {
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
                    let psk = crypto_key.clone();

                    tokio::spawn(async move {
                        debug!("Accepted connection from {}", addr);

                        // Handshake
                        let mut noise = match noise_server_handshake(&mut socket, &psk).await {
                            Ok(n) => n,
                            Err(e) => {
                                error!("Noise handshake failed from {}: {}", addr, e);
                                return;
                            }
                        };

                        let mut reader = BufReader::new(&mut socket);

                        let inbox_name = match read_encrypted_string(&mut reader, &mut noise).await {
                            Ok(s) => s,
                            Err(e) => { error!("Failed to read inbox name: {}", e); return; }
                        };

                        let file_name = match read_encrypted_string(&mut reader, &mut noise).await {
                            Ok(s) => s,
                            Err(e) => { error!("Failed to read file name: {}", e); return; }
                        };

                        // Security check: prevent path traversal attacks
                        if file_name.contains("..") {
                            error!("Invalid file path (path traversal attempt): {}", file_name);
                            return;
                        }

                        let file_size_bytes = match read_noise(&mut reader, &mut noise).await {
                            Ok(b) => b,
                            Err(e) => { error!("Failed to read file size block: {}", e); return; }
                        };

                        if file_size_bytes.len() != 8 {
                            error!("Invalid file size block length");
                            return;
                        }

                        let mut fb = [0u8; 8];
                        fb.copy_from_slice(&file_size_bytes[0..8]);
                        let file_size = u64::from_be_bytes(fb);

                        let dest_dir = root.join(&inbox_name);
                        // Security basic: check if dest_dir exists and is indeed a subfolder of inbox
                        if !dest_dir.exists() {
                            // Let's create it if it doesn't exist.
                            if let Err(e) = tokio::fs::create_dir_all(&dest_dir).await {
                                error!("Failed to create destination dir: {}", e);
                                return;
                            }
                        }

                        // Use the file_name (which can be a relative path) to build the dest_file
                        // Handle windows vs backslashes correctly
                        #[cfg(windows)]
                        let safe_file_name = file_name.replace("/", "\\");
                        #[cfg(not(windows))]
                        let safe_file_name = file_name.clone();

                        let dest_file = dest_dir.join(&safe_file_name);

                        if let Some(parent) = dest_file.parent() {
                            if !parent.exists() {
                                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                                    error!("Failed to create destination directories: {}", e);
                                    return;
                                }
                            }
                        }

                        let mut existing_size = 0u64;
                        if dest_file.exists() {
                            if let Ok(meta) = tokio::fs::metadata(&dest_file).await {
                                let len = meta.len();
                                if len <= file_size {
                                    existing_size = len;
                                }
                            }
                        }

                        if existing_size > 0 {
                            info!("Resuming {} from {} bytes (total {} bytes)", file_name, existing_size, file_size);
                        } else {
                            info!("Receiving file {} ({} bytes) into {}", file_name, file_size, inbox_name);
                        }

                        let mut file = match tokio::fs::OpenOptions::new()
                            .create(true)
                            .write(true)
                            .open(&dest_file).await {
                            Ok(f) => f,
                            Err(e) => { error!("Failed to open/create local file {:?}: {}", dest_file, e); return; }
                        };

                        if existing_size == 0 {
                            if let Err(e) = file.set_len(0).await {
                                error!("Failed to truncate file: {}", e); return;
                            }
                        }
                        if let Err(e) = file.seek(tokio::io::SeekFrom::Start(existing_size)).await {
                            error!("Failed to seek local file: {}", e); return;
                        }

                        // Send offset back to client
                        if let Err(e) = write_noise(reader.get_mut(), &mut noise, &existing_size.to_be_bytes()).await {
                            error!("Failed to send resume offset: {}", e); return;
                        }

                        let mut file_writer = tokio::io::BufWriter::with_capacity(4 * 1024 * 1024, file);
                        let mut hasher = blake3::Hasher::new();
                        let expected_bytes = file_size - existing_size;
                        let mut bytes_copied = 0u64;

                        while bytes_copied < expected_bytes {
                            match read_noise(&mut reader, &mut noise).await {
                                Ok(data) => {
                                    if let Err(e) = file_writer.write_all(&data).await {
                                        error!("Failed to write to disk: {}", e);
                                        return;
                                    }
                                    hasher.update(&data);
                                    bytes_copied += data.len() as u64;
                                    if bytes_copied > expected_bytes {
                                        error!("Received more bytes than expected");
                                        return;
                                    }
                                }
                                Err(e) => {
                                    error!("Connection closed or corrupted transfer: {}", e);
                                    return;
                                }
                            }
                        }

                        let client_hash = match read_noise(&mut reader, &mut noise).await {
                            Ok(h) => h,
                            Err(e) => { error!("Failed to read hash block: {}", e); return; }
                        };

                        let my_hash = hasher.finalize();
                        if client_hash != my_hash.as_bytes() {
                            error!("Payload hash mismatch for {}! Corrupted transfer.", file_name);
                            let _ = write_noise(reader.get_mut(), &mut noise, &[0]).await;
                            return;
                        }

                        if let Err(e) = file_writer.flush().await {
                            error!("Failed to flush file to disk: {}", e);
                            return;
                        }

                        // Send ACK
                        let _ = write_noise(reader.get_mut(), &mut noise, &[1]).await;

                        info!("Successfully received {} from {}", file_name, addr);
                    });
                }
                Err(e) => error!("TCP Accept failed: {}", e),
            }
        }
    });
}

async fn read_encrypted_string<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
    noise: &mut TransportState,
) -> std::io::Result<String> {
    let plain_bytes = read_noise(reader, noise).await?;

    if plain_bytes.len() < 4 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "String block too short"));
    }

    let mut len_buf = [0u8; 4];
    len_buf.copy_from_slice(&plain_bytes[0..4]);
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > 1024 || len > (plain_bytes.len() - 4) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "String too long or invalid length"));
    }

    String::from_utf8(plain_bytes[4..4+len].to_vec())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}
