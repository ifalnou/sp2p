use std::path::PathBuf;
use std::net::SocketAddr;
use std::io::SeekFrom;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt, AsyncSeekExt};
use tokio::fs::File;
use tracing::info;
use std::sync::Arc;
use snow::TransportState;
use crate::core::crypto::{noise_client_handshake, write_noise, read_noise};
use crate::core::state::{GLOBAL_STATE, ActiveTransfer, TransferHistoryItem};
use std::time::SystemTime;

pub async fn send_file(addr: SocketAddr, inbox_name: String, file_path: PathBuf, relative_file_path: String, crypto_key: Arc<[u8; 32]>) -> std::io::Result<()> {
    let mut file = File::open(&file_path).await?;
    let metadata = file.metadata().await?;
    let file_size = metadata.len();

    info!("Connecting to {} to send {} ({} bytes)", addr, relative_file_path, file_size);
    let mut stream = TcpStream::connect(addr).await?;

    // Perform Noise handshake
    let mut noise = noise_client_handshake(&mut stream, &crypto_key).await?;

    // Protocol:
    // 1. Inbox name length + bytes
    // 2. File name length + bytes
    // 3. File size (u64)
    // 4. File data

    write_encrypted_string(&mut stream, &mut noise, &inbox_name).await?;
    write_encrypted_string(&mut stream, &mut noise, &relative_file_path).await?;

    // Encrypt file size (8 bytes)
    let size_bytes = file_size.to_be_bytes();
    write_noise(&mut stream, &mut noise, &size_bytes).await?;

    // Read resume offset from Server
    let offset_bytes = read_noise(&mut stream, &mut noise).await?;
    if offset_bytes.len() != 8 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid offset length from server"));
    }
    let mut ob = [0u8; 8];
    ob.copy_from_slice(&offset_bytes);
    let resume_offset = u64::from_be_bytes(ob);

    if resume_offset > file_size {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Server provided invalid resume offset"));
    }

    if resume_offset > 0 {
        info!("Resuming transfer of {} from offset {}", relative_file_path, resume_offset);
        file.seek(SeekFrom::Start(resume_offset)).await?;
    }

    let transfer_id = format!("{}-{}", addr, relative_file_path);
    {
        let mut state = GLOBAL_STATE.write().unwrap();
        state.active_transfers.push(ActiveTransfer {
            id: transfer_id.clone(),
            filename: relative_file_path.clone(),
            bytes_transferred: resume_offset,
            total_bytes: file_size,
            is_sending: true,
        });
    }

    // Send file data in chunks
    // Use a 4MB buffer to reduce disk read overhead
    let mut file_buf = vec![0u8; 4 * 1024 * 1024];
    let mut writer = tokio::io::BufWriter::with_capacity(4 * 1024 * 1024, &mut stream);

    let mut hasher = blake3::Hasher::new();
    let mut remaining = file_size - resume_offset;

    while remaining > 0 {
        let to_read = std::cmp::min(remaining, file_buf.len() as u64) as usize;
        let n = file.read(&mut file_buf[..to_read]).await?;
        if n == 0 {
            break;
        }
        write_noise(&mut writer, &mut noise, &file_buf[..n]).await?;
        hasher.update(&file_buf[..n]);
        remaining -= n as u64;

        if let Ok(mut state) = GLOBAL_STATE.write() {
            if let Some(t) = state.active_transfers.iter_mut().find(|t| t.id == transfer_id) {
                t.bytes_transferred = file_size - remaining;
            }
        }
    }

    if remaining > 0 {
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "File shrank during transfer"));
    }

    // Send cryptographic hash of the transferred payload
    let hash = hasher.finalize();
    write_noise(&mut writer, &mut noise, hash.as_bytes()).await?;

    writer.flush().await?;
    drop(writer);

    // Wait for ACK
    let ack = read_noise(&mut stream, &mut noise).await?;
    if ack.len() != 1 || ack[0] != 1 {
        // Record failure
        if let Ok(mut state) = GLOBAL_STATE.write() {
            state.active_transfers.retain(|t| t.id != transfer_id);
            state.history.push(TransferHistoryItem {
                filename: relative_file_path.clone(),
                is_sending: true,
                success: false,
                timestamp: SystemTime::now(),
            });
        }
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "File transfer rejected by server (hash mismatch?)"));
    }

    info!("Sent file {} successfully", relative_file_path);

    // Record success
    if let Ok(mut state) = GLOBAL_STATE.write() {
        state.active_transfers.retain(|t| t.id != transfer_id);
        state.history.push(TransferHistoryItem {
            filename: relative_file_path.clone(),
            is_sending: true,
            success: true,
            timestamp: SystemTime::now(),
        });
    }

    Ok(())
}

async fn write_encrypted_string<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    noise: &mut TransportState,
    s: &str
) -> std::io::Result<()> {
    let bytes = s.as_bytes();
    // Prefix string with its own length, then encrypt everything as a block
    let mut payload = Vec::with_capacity(4 + bytes.len());
    payload.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    payload.extend_from_slice(bytes);

    write_noise(writer, noise, &payload).await
}
