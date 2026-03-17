use std::path::PathBuf;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio::io::AsyncWriteExt;
use tokio::fs::File;
use tracing::info;

pub async fn send_file(addr: SocketAddr, inbox_name: String, file_path: PathBuf) -> std::io::Result<()> {
    let file_name = match file_path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n.to_string(),
        None => return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid file name")),
    };

    let mut file = File::open(&file_path).await?;
    let metadata = file.metadata().await?;
    let file_size = metadata.len();

    info!("Connecting to {} to send {} ({} bytes)", addr, file_name, file_size);
    let mut stream = TcpStream::connect(addr).await?;

    // Protocol:
    // 1. Inbox name length + bytes
    // 2. File name length + bytes
    // 3. File size (u64)
    // 4. File data

    write_string(&mut stream, &inbox_name).await?;
    write_string(&mut stream, &file_name).await?;
    stream.write_u64(file_size).await?;

    // Use tokio::io::copy for fast streaming
    tokio::io::copy(&mut file, &mut stream).await?;

    info!("Successfully sent {}", file_name);
    Ok(())
}

async fn write_string<W: tokio::io::AsyncWrite + Unpin>(writer: &mut W, s: &str) -> std::io::Result<()> {
    let bytes = s.as_bytes();
    writer.write_u32(bytes.len() as u32).await?;
    writer.write_all(bytes).await?;
    Ok(())
}
