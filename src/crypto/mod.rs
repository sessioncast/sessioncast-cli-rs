use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use rand::RngCore;

const IV_LEN: usize = 12;
const TAG_LEN: usize = 16;

/// Encrypt data using AES-256-GCM
/// Format: [iv(12) | tag(16) | ciphertext]
pub fn encrypt(data: &[u8], key: &[u8]) -> anyhow::Result<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| anyhow::anyhow!("Invalid key: {}", e))?;
    
    // Generate random IV
    let mut iv = [0u8; IV_LEN];
    rand::thread_rng().fill_bytes(&mut iv);
    let nonce = Nonce::from_slice(&iv);

    // Encrypt
    let ciphertext = cipher.encrypt(nonce, data)
        .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

    // Combine: iv + ciphertext (tag is included in ciphertext by aes-gcm)
    let mut result = Vec::with_capacity(IV_LEN + ciphertext.len());
    result.extend_from_slice(&iv);
    result.extend_from_slice(&ciphertext);

    Ok(result)
}

/// Decrypt data encrypted with AES-256-GCM
/// Format: [iv(12) | tag(16) | ciphertext]
pub fn decrypt(data: &[u8], key: &[u8]) -> anyhow::Result<Vec<u8>> {
    if data.len() < IV_LEN {
        anyhow::bail!("Data too short for decryption");
    }

    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| anyhow::anyhow!("Invalid key: {}", e))?;

    let iv = &data[..IV_LEN];
    let ciphertext = &data[IV_LEN..];

    let nonce = Nonce::from_slice(iv);
    let plaintext = cipher.decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;

    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let key = b"01234567890123456789012345678901"; // 32 bytes
        let data = b"Hello, World!";

        let encrypted = encrypt(data, key).unwrap();
        assert!(encrypted.len() > data.len());

        let decrypted = decrypt(&encrypted, key).unwrap();
        assert_eq!(decrypted, data);
    }
}
