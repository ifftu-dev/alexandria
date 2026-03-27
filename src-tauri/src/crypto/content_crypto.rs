//! Transparent content encryption for iroh blob storage.
//!
//! Encrypts content before storing in the FsStore and decrypts after
//! retrieval. Uses AES-256-GCM with a per-blob random nonce.
//!
//! File format: `version(1) || nonce(12) || ciphertext(N + 16 auth tag)`
//! Version 0x01 = AES-256-GCM.

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use thiserror::Error;

const VERSION_AES_GCM: u8 = 0x01;

#[derive(Error, Debug)]
pub enum ContentCryptoError {
    #[error("encryption failed: {0}")]
    Encrypt(String),
    #[error("decryption failed: {0}")]
    Decrypt(String),
    #[error("unsupported version: {0}")]
    UnsupportedVersion(u8),
    #[error("data too short")]
    TooShort,
}

/// Encrypt plaintext content using AES-256-GCM.
///
/// Returns `version(1) || nonce(12) || ciphertext`.
pub fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, ContentCryptoError> {
    use rand::RngCore;

    let cipher = Aes256Gcm::new(key.into());
    let mut nonce_bytes = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| ContentCryptoError::Encrypt(e.to_string()))?;

    let mut out = Vec::with_capacity(1 + 12 + ciphertext.len());
    out.push(VERSION_AES_GCM);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt content encrypted by [`encrypt`].
///
/// If the data doesn't start with a known version byte, returns `None`
/// to indicate legacy unencrypted content (caller should use raw bytes).
pub fn decrypt(key: &[u8; 32], data: &[u8]) -> Result<Option<Vec<u8>>, ContentCryptoError> {
    if data.is_empty() {
        return Ok(Some(Vec::new()));
    }

    match data[0] {
        VERSION_AES_GCM => {
            // version(1) + nonce(12) + at least auth tag(16)
            if data.len() < 1 + 12 + 16 {
                return Err(ContentCryptoError::TooShort);
            }
            let nonce = Nonce::from_slice(&data[1..13]);
            let cipher = Aes256Gcm::new(key.into());
            let plaintext = cipher
                .decrypt(nonce, &data[13..])
                .map_err(|_| ContentCryptoError::Decrypt("AES-GCM auth failed".into()))?;
            Ok(Some(plaintext))
        }
        // Not a known version byte — likely legacy unencrypted content
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = [42u8; 32];
        let plaintext = b"Hello, Alexandria!";

        let encrypted = encrypt(&key, plaintext).unwrap();
        assert_ne!(&encrypted, plaintext);
        assert_eq!(encrypted[0], VERSION_AES_GCM);

        let decrypted = decrypt(&key, &encrypted).unwrap().unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_key_fails() {
        let key = [42u8; 32];
        let wrong_key = [99u8; 32];
        let plaintext = b"secret";

        let encrypted = encrypt(&key, plaintext).unwrap();
        let result = decrypt(&wrong_key, &encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn legacy_unencrypted_returns_none() {
        let key = [42u8; 32];
        let legacy = b"plain text content without version byte";

        // First byte isn't VERSION_AES_GCM, so returns None
        let result = decrypt(&key, legacy).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn empty_content_roundtrip() {
        let key = [42u8; 32];
        let encrypted = encrypt(&key, b"").unwrap();
        let decrypted = decrypt(&key, &encrypted).unwrap().unwrap();
        assert!(decrypted.is_empty());
    }

    #[test]
    fn each_encryption_produces_different_ciphertext() {
        let key = [42u8; 32];
        let plaintext = b"same input";

        let enc1 = encrypt(&key, plaintext).unwrap();
        let enc2 = encrypt(&key, plaintext).unwrap();
        assert_ne!(enc1, enc2, "different nonces should produce different ciphertext");

        // But both decrypt to the same plaintext
        assert_eq!(
            decrypt(&key, &enc1).unwrap().unwrap(),
            decrypt(&key, &enc2).unwrap().unwrap()
        );
    }
}
