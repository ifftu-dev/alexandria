use std::path::{Path, PathBuf};

use hmac::{Hmac, Mac};
use iota_stronghold::{KeyProvider, SnapshotPath, Stronghold};
use sha2::Sha512;
use thiserror::Error;
use zeroize::Zeroizing;

type HmacSha512 = Hmac<Sha512>;

/// Store record key for the mnemonic.
const MNEMONIC_KEY: &[u8] = b"mnemonic";
/// Stronghold client path.
const CLIENT_PATH: &[u8] = b"alexandria_client";

/// Stronghold snapshot filename.
const SNAPSHOT_FILENAME: &str = "alexandria.stronghold";
/// Random salt filename (generated once per vault).
const SALT_FILENAME: &str = "vault_salt.bin";
/// Salt length in bytes.
const SALT_LEN: usize = 32;

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
    #[error("stronghold error: {0}")]
    Stronghold(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("memory error: {0}")]
    Memory(String),
    #[error("mnemonic not found in vault")]
    MnemonicNotFound,
}

/// Secure keystore backed by IOTA Stronghold.
///
/// Stores the BIP-39 mnemonic phrase in an encrypted vault file.
/// The vault is encrypted with a key derived from the user's password
/// combined with a random per-device salt.
///
/// Threat model:
/// - At rest: vault file is encrypted via Stronghold's snapshot mechanism
/// - In memory: Stronghold uses memory guards and fragmented storage
/// - Password: combined with a random salt via HMAC-SHA512 to derive the key
///
/// Future: biometric / OS keychain unlock will replace password entry.
pub struct Keystore {
    stronghold: Stronghold,
    snapshot_path: SnapshotPath,
    vault_dir: PathBuf,
    /// The salt bytes — kept in memory so we can re-derive the key for saves.
    salt: Vec<u8>,
    /// The password — kept in memory for the session so we can commit.
    /// Cleared on lock().
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
    /// Check whether a Stronghold vault file exists in the given directory.
    pub fn exists(vault_dir: &Path) -> bool {
        vault_dir.join(SNAPSHOT_FILENAME).exists()
    }

    /// Create a new vault with the given password.
    ///
    /// Generates a random salt, derives the encryption key, creates the
    /// Stronghold snapshot file, and returns an unlocked Keystore.
    ///
    /// Fails if a vault already exists in `vault_dir`.
    pub fn create(vault_dir: &Path, password: &str) -> Result<Self, KeystoreError> {
        let snapshot_file = vault_dir.join(SNAPSHOT_FILENAME);
        if snapshot_file.exists() {
            return Err(KeystoreError::VaultAlreadyExists(snapshot_file));
        }

        // Ensure directory exists
        std::fs::create_dir_all(vault_dir)?;

        // Generate random salt
        let salt = generate_salt();
        std::fs::write(vault_dir.join(SALT_FILENAME), &salt)?;

        // Derive encryption key from password + salt
        let key_provider = derive_key(password, &salt)?;

        // Create Stronghold and client
        let stronghold = Stronghold::default();
        let snapshot_path = SnapshotPath::from_path(&snapshot_file);

        stronghold
            .create_client(CLIENT_PATH)
            .map_err(|e| KeystoreError::Stronghold(format!("{e:?}")))?;

        // Persist the empty vault to disk (establishes the snapshot file)
        stronghold
            .commit_with_keyprovider(&snapshot_path, &key_provider)
            .map_err(|e| KeystoreError::Stronghold(format!("{e:?}")))?;

        log::info!("Keystore created at {}", snapshot_file.display());

        Ok(Self {
            stronghold,
            snapshot_path,
            vault_dir: vault_dir.to_path_buf(),
            salt: salt.to_vec(),
            password: Zeroizing::new(password.to_string()),
        })
    }

    /// Open an existing vault with the given password.
    ///
    /// Loads the snapshot file, decrypts with the derived key, and
    /// returns an unlocked Keystore.
    pub fn open(vault_dir: &Path, password: &str) -> Result<Self, KeystoreError> {
        let snapshot_file = vault_dir.join(SNAPSHOT_FILENAME);
        if !snapshot_file.exists() {
            return Err(KeystoreError::VaultNotFound(snapshot_file));
        }

        // Read salt
        let salt_path = vault_dir.join(SALT_FILENAME);
        let salt = std::fs::read(&salt_path).map_err(|_| {
            KeystoreError::Stronghold(format!("salt file missing at {}", salt_path.display()))
        })?;

        // Derive key
        let key_provider = derive_key(password, &salt)?;

        // Load snapshot
        let stronghold = Stronghold::default();
        let snapshot_path = SnapshotPath::from_path(&snapshot_file);

        stronghold
            .load_client_from_snapshot(CLIENT_PATH, &key_provider, &snapshot_path)
            .map_err(|e| {
                let msg = format!("{e:?}");
                // Stronghold returns various errors for wrong password;
                // the most common is a snapshot decryption failure.
                if msg.contains("Decrypt")
                    || msg.contains("decrypt")
                    || msg.contains("IntegrityError")
                    || msg.contains("integrity")
                    || msg.contains("InvalidData")
                {
                    KeystoreError::IncorrectPassword
                } else {
                    KeystoreError::Stronghold(msg)
                }
            })?;

        log::info!("Keystore unlocked from {}", snapshot_file.display());

        Ok(Self {
            stronghold,
            snapshot_path,
            vault_dir: vault_dir.to_path_buf(),
            salt,
            password: Zeroizing::new(password.to_string()),
        })
    }

    /// Store a mnemonic phrase in the vault's encrypted store.
    ///
    /// Uses the client store (key-value, readable back) rather than the
    /// vault (write-only, procedure-only). Both are encrypted at rest
    /// in the snapshot file.
    pub fn store_mnemonic(&self, mnemonic: &str) -> Result<(), KeystoreError> {
        let client = self
            .stronghold
            .get_client(CLIENT_PATH)
            .map_err(|e| KeystoreError::Stronghold(format!("{e:?}")))?;

        client
            .store()
            .insert(MNEMONIC_KEY.to_vec(), mnemonic.as_bytes().to_vec(), None)
            .map_err(|e| KeystoreError::Stronghold(format!("{e:?}")))?;

        // Write client state into snapshot data
        self.stronghold
            .write_client(CLIENT_PATH)
            .map_err(|e| KeystoreError::Stronghold(format!("{e:?}")))?;

        // Persist to disk
        self.save()?;

        log::info!("Mnemonic stored in vault");
        Ok(())
    }

    /// Retrieve the mnemonic phrase from the vault.
    ///
    /// Returns the plaintext mnemonic. The caller is responsible for
    /// handling it securely (display briefly, then discard).
    pub fn retrieve_mnemonic(&self) -> Result<String, KeystoreError> {
        let client = self
            .stronghold
            .get_client(CLIENT_PATH)
            .map_err(|e| KeystoreError::Stronghold(format!("{e:?}")))?;

        let data = client
            .store()
            .get(MNEMONIC_KEY)
            .map_err(|e| KeystoreError::Stronghold(format!("{e:?}")))?;

        match data {
            Some(bytes) => {
                let mnemonic = String::from_utf8(bytes)
                    .map_err(|e| KeystoreError::Stronghold(format!("invalid UTF-8: {e}")))?;
                Ok(mnemonic)
            }
            None => Err(KeystoreError::MnemonicNotFound),
        }
    }

    /// Save the current vault state to disk.
    ///
    /// Re-derives the key from the stored password + salt each time,
    /// since `KeyProvider` is not `Clone`.
    fn save(&self) -> Result<(), KeystoreError> {
        let key_provider = derive_key(&self.password, &self.salt)?;
        self.stronghold
            .commit_with_keyprovider(&self.snapshot_path, &key_provider)
            .map_err(|e| KeystoreError::Stronghold(format!("{e:?}")))?;
        Ok(())
    }

    /// Clear in-memory secrets (lock the vault).
    ///
    /// After this call, the Keystore is consumed. A new `open()` call
    /// is required to access secrets again.
    pub fn lock(self) -> Result<(), KeystoreError> {
        self.stronghold
            .clear()
            .map_err(|e| KeystoreError::Stronghold(format!("{e:?}")))?;
        // `self.password` is `Zeroizing<String>` — dropped and zeroed here.
        log::info!("Keystore locked");
        Ok(())
    }

    /// Get the vault directory path.
    pub fn vault_dir(&self) -> &Path {
        &self.vault_dir
    }
}

/// Derive a 32-byte encryption key from password + salt using HMAC-SHA512.
///
/// The result is truncated to 32 bytes (256 bits) which is the key size
/// Stronghold uses for snapshot encryption. We use HMAC-SHA512 because:
/// - We already have the `hmac` and `sha2` crates
/// - Combined with a random 32-byte salt, this provides strong at-rest security
/// - Future: upgrade to argon2id for memory-hard KDF (brute-force resistance)
fn derive_key(password: &str, salt: &[u8]) -> Result<KeyProvider, KeystoreError> {
    let mut mac = HmacSha512::new_from_slice(salt)
        .map_err(|e| KeystoreError::Memory(format!("HMAC init failed: {e}")))?;
    mac.update(password.as_bytes());
    let result = mac.finalize().into_bytes();

    // Take first 32 bytes as the key
    let key_bytes = Zeroizing::new(result[..32].to_vec());

    KeyProvider::try_from(key_bytes).map_err(|e| KeystoreError::Memory(format!("{e:?}")))
}

/// Generate a cryptographically random 32-byte salt.
fn generate_salt() -> [u8; SALT_LEN] {
    use rand::RngCore;
    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    salt
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_vault_dir() -> PathBuf {
        let dir = std::env::temp_dir()
            .join("alexandria_test_vault")
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
        let _ks = Keystore::create(&dir, "testpassword").expect("create failed");
        assert!(Keystore::exists(&dir));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn create_vault_twice_fails() {
        let dir = temp_vault_dir();
        let _ks = Keystore::create(&dir, "testpassword").expect("create failed");
        let err = Keystore::create(&dir, "testpassword");
        assert!(err.is_err());
        assert!(
            matches!(err.unwrap_err(), KeystoreError::VaultAlreadyExists(_)),
            "expected VaultAlreadyExists"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn store_and_retrieve_mnemonic() {
        let dir = temp_vault_dir();
        let ks = Keystore::create(&dir, "testpassword").expect("create failed");

        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        ks.store_mnemonic(mnemonic).expect("store failed");

        let retrieved = ks.retrieve_mnemonic().expect("retrieve failed");
        assert_eq!(mnemonic, retrieved);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn open_with_correct_password() {
        let dir = temp_vault_dir();
        let ks = Keystore::create(&dir, "correctpassword").expect("create failed");

        let mnemonic = "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo wrong";
        ks.store_mnemonic(mnemonic).expect("store failed");

        // Lock and drop
        ks.lock().expect("lock failed");

        // Re-open with correct password
        let ks2 = Keystore::open(&dir, "correctpassword").expect("open failed");
        let retrieved = ks2.retrieve_mnemonic().expect("retrieve failed");
        assert_eq!(mnemonic, retrieved);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn open_with_wrong_password_fails() {
        let dir = temp_vault_dir();
        let ks = Keystore::create(&dir, "correctpassword").expect("create failed");
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
        assert!(
            matches!(result.unwrap_err(), KeystoreError::VaultNotFound(_)),
            "expected VaultNotFound"
        );
        fs::remove_dir_all(&dir).ok();
    }
}
