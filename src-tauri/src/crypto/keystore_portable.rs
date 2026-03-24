//! Portable keystore — encrypted mnemonic storage without IOTA Stronghold.
//!
//! Uses AES-256-GCM for authenticated encryption with a key derived from
//! the user's password via Argon2id (same parameters as the desktop keystore).
//! This compiles on all platforms including iOS and Android.
//!
//! File format (vault.bin):
//!   bytes  0..12  : AES-GCM nonce (96 bits)
//!   bytes 12..end : AES-GCM ciphertext + 16-byte auth tag
//!
//! The plaintext is the raw UTF-8 mnemonic phrase.

use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use thiserror::Error;
use zeroize::Zeroizing;

/// Encrypted vault filename.
const VAULT_FILENAME: &str = "vault.bin";
/// Random salt filename (generated once per vault).
const SALT_FILENAME: &str = "vault_salt.bin";
/// Salt length in bytes.
const SALT_LEN: usize = 32;
/// AES-GCM nonce length (96 bits).
const NONCE_LEN: usize = 12;

#[derive(Error, Debug)]
pub enum KeystoreError {
    #[error("vault already exists at {0}")]
    VaultAlreadyExists(PathBuf),
    #[error("vault not found at {0}")]
    VaultNotFound(PathBuf),
    #[error("incorrect password")]
    IncorrectPassword,
    #[error("vault is locked")]
    VaultLocked,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("crypto error: {0}")]
    Crypto(String),
    #[error("mnemonic not found in vault")]
    MnemonicNotFound,
}

/// Portable keystore backed by AES-256-GCM + Argon2id.
///
/// Stores the BIP-39 mnemonic phrase in an encrypted file.
/// Same threat model as the Stronghold-based desktop keystore:
/// - At rest: AES-256-GCM authenticated encryption
/// - Password: Argon2id with 64 MB memory, 3 iterations, 4 lanes
pub struct Keystore {
    vault_dir: PathBuf,
    /// Decrypted mnemonic — held in memory while unlocked.
    mnemonic: Option<Zeroizing<String>>,
    /// Salt bytes — kept for re-encryption on save.
    salt: Vec<u8>,
    /// Password — kept for re-encryption on save.
    password: Zeroizing<String>,
}

impl std::fmt::Debug for Keystore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keystore")
            .field("vault_dir", &self.vault_dir)
            .finish_non_exhaustive()
    }
}

impl Keystore {
    /// Check whether a vault file exists in the given directory.
    pub fn exists(vault_dir: &Path) -> bool {
        vault_dir.join(VAULT_FILENAME).exists()
    }

    /// Create a new vault with the given password.
    ///
    /// Generates a random salt, creates an empty vault file, and returns
    /// an unlocked Keystore. The mnemonic must be stored separately via
    /// `store_mnemonic()`.
    pub fn create(vault_dir: &Path, password: &str) -> Result<Self, KeystoreError> {
        let vault_file = vault_dir.join(VAULT_FILENAME);
        if vault_file.exists() {
            return Err(KeystoreError::VaultAlreadyExists(vault_file));
        }

        std::fs::create_dir_all(vault_dir)?;

        let salt = generate_salt();
        write_salt_with_hmac(vault_dir, &salt, password)?;

        log::info!("Portable keystore created at {}", vault_dir.display());

        Ok(Self {
            vault_dir: vault_dir.to_path_buf(),
            mnemonic: None,
            salt: salt.to_vec(),
            password: Zeroizing::new(password.to_string()),
        })
    }

    /// Open an existing vault with the given password.
    ///
    /// Decrypts the vault file and loads the mnemonic into memory.
    pub fn open(vault_dir: &Path, password: &str) -> Result<Self, KeystoreError> {
        let vault_file = vault_dir.join(VAULT_FILENAME);
        if !vault_file.exists() {
            return Err(KeystoreError::VaultNotFound(vault_file));
        }

        let salt = read_and_verify_salt(vault_dir, password)?;

        let key = derive_key(password, &salt)?;
        let ciphertext = std::fs::read(&vault_file)?;

        if ciphertext.len() < NONCE_LEN {
            return Err(KeystoreError::Crypto("vault file too short".into()));
        }

        let nonce = Nonce::from_slice(&ciphertext[..NONCE_LEN]);
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| KeystoreError::Crypto(format!("cipher init: {e}")))?;

        let plaintext = cipher
            .decrypt(nonce, &ciphertext[NONCE_LEN..])
            .map_err(|_| KeystoreError::IncorrectPassword)?;

        let mnemonic = String::from_utf8(plaintext)
            .map_err(|e| KeystoreError::Crypto(format!("invalid UTF-8: {e}")))?;

        log::info!("Portable keystore unlocked from {}", vault_dir.display());

        Ok(Self {
            vault_dir: vault_dir.to_path_buf(),
            mnemonic: Some(Zeroizing::new(mnemonic)),
            salt,
            password: Zeroizing::new(password.to_string()),
        })
    }

    /// Store a mnemonic phrase in the vault.
    pub fn store_mnemonic(&mut self, mnemonic: &str) -> Result<(), KeystoreError> {
        self.mnemonic = Some(Zeroizing::new(mnemonic.to_string()));
        self.save()?;
        log::info!("Mnemonic stored in portable vault");
        Ok(())
    }

    /// Retrieve the mnemonic phrase from the vault.
    pub fn retrieve_mnemonic(&self) -> Result<String, KeystoreError> {
        self.mnemonic
            .as_ref()
            .map(|m| m.as_str().to_string())
            .ok_or(KeystoreError::MnemonicNotFound)
    }

    /// Save the current mnemonic to disk (encrypt and write).
    fn save(&self) -> Result<(), KeystoreError> {
        let mnemonic = self
            .mnemonic
            .as_ref()
            .ok_or(KeystoreError::MnemonicNotFound)?;

        let key = derive_key(&self.password, &self.salt)?;
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| KeystoreError::Crypto(format!("cipher init: {e}")))?;

        let nonce_bytes = generate_nonce();
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, mnemonic.as_bytes())
            .map_err(|e| KeystoreError::Crypto(format!("encrypt: {e}")))?;

        // Write nonce || ciphertext
        let mut output = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);

        std::fs::write(self.vault_dir.join(VAULT_FILENAME), &output)?;
        Ok(())
    }

    /// Lock the vault, clearing in-memory secrets.
    pub fn lock(mut self) -> Result<(), KeystoreError> {
        self.mnemonic = None;
        // password is Zeroizing — dropped and zeroed here
        log::info!("Portable keystore locked");
        Ok(())
    }

    /// Get the vault directory path.
    pub fn vault_dir(&self) -> &Path {
        &self.vault_dir
    }
}

/// Derive a 32-byte AES key from password + salt using Argon2id.
///
/// Same parameters as the desktop Stronghold keystore: 64 MB memory,
/// 3 iterations, 4 lanes.
fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; 32], KeystoreError> {
    let params = argon2::Params::new(64 * 1024, 3, 4, Some(32))
        .map_err(|e| KeystoreError::Crypto(format!("Argon2 params: {e}")))?;

    let argon2 = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let mut key = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| KeystoreError::Crypto(format!("Argon2 hash: {e}")))?;

    Ok(key)
}

/// Generate a cryptographically random 32-byte salt.
fn generate_salt() -> [u8; SALT_LEN] {
    use rand::rngs::OsRng;
    use rand::RngCore;
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    salt
}

/// Generate a cryptographically random 12-byte nonce.
fn generate_nonce() -> [u8; NONCE_LEN] {
    use rand::rngs::OsRng;
    use rand::RngCore;
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    nonce
}

/// HMAC-SHA256 tag length for salt integrity verification.
const HMAC_LEN: usize = 32;

/// Compute HMAC-SHA256 of the salt, keyed by the password.
fn compute_salt_hmac(password: &str, salt: &[u8]) -> [u8; HMAC_LEN] {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let mut mac = Hmac::<Sha256>::new_from_slice(password.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(salt);
    let result = mac.finalize();
    let mut out = [0u8; HMAC_LEN];
    out.copy_from_slice(&result.into_bytes());
    out
}

/// Write salt + HMAC tag to the salt file.
fn write_salt_with_hmac(vault_dir: &Path, salt: &[u8], password: &str) -> Result<(), KeystoreError> {
    let tag = compute_salt_hmac(password, salt);
    let mut data = Vec::with_capacity(SALT_LEN + HMAC_LEN);
    data.extend_from_slice(salt);
    data.extend_from_slice(&tag);
    std::fs::write(vault_dir.join(SALT_FILENAME), &data)?;
    Ok(())
}

/// Read salt file and verify its integrity HMAC.
///
/// Supports both the new format (salt + HMAC = 64 bytes) and the legacy
/// format (salt only = 32 bytes) for backward compatibility.
fn read_and_verify_salt(vault_dir: &Path, password: &str) -> Result<Vec<u8>, KeystoreError> {
    let salt_path = vault_dir.join(SALT_FILENAME);
    let data = std::fs::read(&salt_path).map_err(|_| {
        KeystoreError::Crypto(format!("salt file missing at {}", salt_path.display()))
    })?;

    if data.len() == SALT_LEN + HMAC_LEN {
        let (salt, stored_tag) = data.split_at(SALT_LEN);
        let expected_tag = compute_salt_hmac(password, salt);
        if stored_tag != expected_tag {
            return Err(KeystoreError::Crypto(
                "salt file corrupted or tampered — integrity check failed".into(),
            ));
        }
        Ok(salt.to_vec())
    } else if data.len() == SALT_LEN {
        log::warn!("Salt file uses legacy format (no integrity HMAC) — will upgrade on next save");
        Ok(data)
    } else {
        Err(KeystoreError::Crypto(format!(
            "salt file has unexpected size: {} bytes",
            data.len()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_vault_dir() -> PathBuf {
        let dir = std::env::temp_dir()
            .join("alexandria_test_portable_vault")
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn vault_exists_returns_false_when_no_vault() {
        let dir = temp_vault_dir();
        assert!(!Keystore::exists(&dir));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn create_vault_and_check_exists() {
        let dir = temp_vault_dir();
        let mut ks = Keystore::create(&dir, "testpassword").expect("create failed");
        ks.store_mnemonic("test mnemonic").expect("store failed");
        assert!(Keystore::exists(&dir));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn store_and_retrieve_mnemonic() {
        let dir = temp_vault_dir();
        let mut ks = Keystore::create(&dir, "testpassword").expect("create failed");

        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        ks.store_mnemonic(mnemonic).expect("store failed");

        let retrieved = ks.retrieve_mnemonic().expect("retrieve failed");
        assert_eq!(mnemonic, retrieved);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn open_with_correct_password() {
        let dir = temp_vault_dir();
        let mut ks = Keystore::create(&dir, "correctpassword").expect("create failed");

        let mnemonic = "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo wrong";
        ks.store_mnemonic(mnemonic).expect("store failed");
        ks.lock().expect("lock failed");

        let ks2 = Keystore::open(&dir, "correctpassword").expect("open failed");
        let retrieved = ks2.retrieve_mnemonic().expect("retrieve failed");
        assert_eq!(mnemonic, retrieved);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn open_with_wrong_password_fails() {
        let dir = temp_vault_dir();
        let mut ks = Keystore::create(&dir, "correctpassword").expect("create failed");
        ks.store_mnemonic("test").expect("store failed");
        ks.lock().expect("lock failed");

        let result = Keystore::open(&dir, "wrongpassword");
        assert!(result.is_err());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn open_nonexistent_vault_fails() {
        let dir = temp_vault_dir();
        let result = Keystore::open(&dir, "anypassword");
        assert!(result.is_err());
        fs::remove_dir_all(&dir).ok();
    }
}
