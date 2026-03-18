use std::path::PathBuf;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::fs::File;
use tracing::info;
use std::sync::Arc;
use snow::TransportState;
use crate::core::crypto::{noise_client_handshake, write_noise};

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

    // Send file data in chunks
    // Use a 4MB buffer to reduce disk read overhead
    let mut file_buf = vec![0u8; 4 * 1024 * 1024];
    let mut writer = tokio::io::BufWriter::with_capacity(4 * 1024 * 1024, &mut stream);

    loop {
        let n = file.read(&mut file_buf).await?;
        if n == 0 {
            break;
        }
        write_noise(&mut writer, &mut noise, &file_buf[..n]).await?;
    }

    writer.flush().await?;

    info!("Successfully sent {}", relative_file_path);
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
