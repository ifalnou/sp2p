use argon2::{Argon2, Params};
use chacha20poly1305::{
    aead::{Aead, KeyInit, OsRng},
    AeadCore, ChaCha20Poly1305, Key, Nonce,
};
use snow::{Builder, TransportState};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Derives a 32-byte symmetric key from a password string using Argon2id.
///
/// Note: We use a static salt here because the password acts as a Pre-Shared Key (PSK)
/// for the group. For actual password storage a random salt must be used, but for
/// deriving a deterministic PSK across a network, the salt is deterministic.
pub fn derive_key(password: &str) -> [u8; 32] {
    let mut key = [0u8; 32];

    // We use a hardcoded domain-separation salt so all peers derive the exact same key.
    let salt_bytes = b"sp2p-network-static-salt";

    // Default Argon2id parameters
    let params = Params::default();
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let password_bytes = password.as_bytes();

    argon2.hash_password_into(password_bytes, salt_bytes, &mut key)
        .expect("Argon2 hash failed");

    key
}

/// Encrypts a UDP payload. Returns the combined nonce + ciphertext.
pub fn encrypt_udp(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, &'static str> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng); // 96-bits; 12 bytes

    let ciphertext = cipher.encrypt(&nonce, plaintext).map_err(|_| "Encryption failed")?;

    let mut combined = Vec::with_capacity(nonce.len() + ciphertext.len());
    combined.extend_from_slice(&nonce);
    combined.extend_from_slice(&ciphertext);
    Ok(combined)
}

/// Decrypts a UDP payload. Expects the first 12 bytes to be the nonce.
pub fn decrypt_udp(key: &[u8; 32], combined: &[u8]) -> Result<Vec<u8>, &'static str> {
    if combined.len() < 12 {
        return Err("Payload too short");
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));

    cipher.decrypt(nonce, ciphertext).map_err(|_| "Decryption failed")
}

pub const NOISE_PATTERN: &str = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s";

/// Initiates a Noise handshake as an initiator (Client).
pub async fn noise_client_handshake<S: AsyncReadExt + AsyncWriteExt + Unpin>(
    stream: &mut S,
    psk: &[u8; 32],
) -> std::io::Result<TransportState> {
    let builder = Builder::new(NOISE_PATTERN.parse().unwrap());
    let mut noise = builder.psk(0, psk).build_initiator().unwrap();

    let mut buf = vec![0u8; 65535];

    // -> e
    let len = noise.write_message(&[], &mut buf).unwrap();
    stream.write_u16(len as u16).await?;
    stream.write_all(&buf[..len]).await?;

    // <- e, ee
    let len = stream.read_u16().await? as usize;
    stream.read_exact(&mut buf[..len]).await?;
    noise.read_message(&buf[..len], &mut []).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    Ok(noise.into_transport_mode().unwrap())
}

/// Initiates a Noise handshake as a responder (Server).
pub async fn noise_server_handshake<S: AsyncReadExt + AsyncWriteExt + Unpin>(
    stream: &mut S,
    psk: &[u8; 32],
) -> std::io::Result<TransportState> {
    let builder = Builder::new(NOISE_PATTERN.parse().unwrap());
    let mut noise = builder.psk(0, psk).build_responder().unwrap();

    let mut buf = vec![0u8; 65535];

    // -> e
    let len = stream.read_u16().await? as usize;
    stream.read_exact(&mut buf[..len]).await?;
    noise.read_message(&buf[..len], &mut []).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // <- e, ee
    let len = noise.write_message(&[], &mut buf).unwrap();
    stream.write_u16(len as u16).await?;
    stream.write_all(&buf[..len]).await?;

    Ok(noise.into_transport_mode().unwrap())
}

/// Write encrypted data over the stream.
pub async fn write_noise<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    noise: &mut TransportState,
    data: &[u8],
) -> std::io::Result<()> {
    // Noise max payload is 65535 bytes minus 16 byte MAC, so ~65519 payload max
    let mut buf = [0u8; 65535];
    for chunk in data.chunks(65000) {
        let len = noise.write_message(chunk, &mut buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writer.write_u16(len as u16).await?;
        writer.write_all(&buf[..len]).await?;
    }
    Ok(())
}

/// Read encrypted data from the stream.
pub async fn read_noise<R: AsyncReadExt + Unpin>(
    reader: &mut R,
    noise: &mut TransportState,
) -> std::io::Result<Vec<u8>> {
    let len = reader.read_u16().await? as usize;
    let mut cipher_buf = vec![0u8; len];
    reader.read_exact(&mut cipher_buf).await?;

    let mut plain_buf = [0u8; 65535];
    let plain_len = noise.read_message(&cipher_buf, &mut plain_buf)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    Ok(plain_buf[..plain_len].to_vec())
}
