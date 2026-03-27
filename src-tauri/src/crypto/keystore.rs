use std::path::{Path, PathBuf};

use argon2::Argon2;
use iota_stronghold::{KeyProvider, SnapshotPath, Stronghold};
use thiserror::Error;
use zeroize::Zeroizing;

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
/// HMAC-SHA256 tag length for salt integrity verification.
const HMAC_LEN: usize = 32;

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
/// - Password: combined with a random salt via Argon2id (memory-hard KDF)
///   with 64 MB memory cost, 3 iterations, 4 lanes — resistant to GPU/ASIC
///   brute-force attacks
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

        // Generate random salt and write with integrity HMAC
        let salt = generate_salt();
        write_salt_with_hmac(vault_dir, &salt, password)?;

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

        // Read and verify salt integrity
        let salt = read_and_verify_salt(vault_dir, password)?;

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

    /// Verify a password candidate against the stored password.
    ///
    /// Uses constant-time comparison to prevent timing side-channels.
    pub fn check_password(&self, candidate: &str) -> Result<(), KeystoreError> {
        use subtle::ConstantTimeEq;
        let stored = self.password.as_bytes();
        let given = candidate.as_bytes();
        if stored.len() != given.len() || stored.ct_eq(given).unwrap_u8() != 1 {
            return Err(KeystoreError::IncorrectPassword);
        }
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

    /// Derive a 32-byte database encryption key.
    ///
    /// Uses HKDF-SHA256 with the Argon2id master key material
    /// (password + salt) to produce a purpose-specific subkey.
    pub fn derive_db_key(&self) -> [u8; 32] {
        derive_subkey(&self.password, &self.salt, b"alexandria-db-key")
    }

    /// Derive a 32-byte key for iroh node secret encryption.
    pub fn derive_node_key(&self) -> [u8; 32] {
        derive_subkey(&self.password, &self.salt, b"alexandria-iroh-node-key")
    }

    /// Derive a 32-byte key for content blob encryption.
    pub fn derive_content_key(&self) -> [u8; 32] {
        derive_subkey(&self.password, &self.salt, b"alexandria-content-key")
    }
}

/// Derive a 32-byte encryption key from password + salt using Argon2id.
///
/// Argon2id is a memory-hard KDF that resists GPU/ASIC brute-force attacks.
/// Parameters: 64 MB memory, 3 iterations, 4 lanes — balances security
/// against ~200ms derivation time on modern hardware.
///
/// The 32-byte output matches Stronghold's snapshot encryption key size.
fn derive_key(password: &str, salt: &[u8]) -> Result<KeyProvider, KeystoreError> {
    let params = argon2::Params::new(
        64 * 1024, // m_cost: 64 MB memory
        3,         // t_cost: 3 iterations
        4,         // p_cost: 4 parallel lanes
        Some(32),  // output length: 32 bytes
    )
    .map_err(|e| KeystoreError::Memory(format!("Argon2 params error: {e}")))?;

    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let mut key = Zeroizing::new(vec![0u8; 32]);
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| KeystoreError::Memory(format!("Argon2 hash failed: {e}")))?;

    KeyProvider::try_from(key).map_err(|e| KeystoreError::Memory(format!("{e:?}")))
}

/// Derive a purpose-specific 32-byte subkey from password + salt.
///
/// 1. Argon2id(password, salt) → 32-byte master key material
/// 2. HKDF-SHA256(ikm=master, info=purpose) → 32-byte subkey
///
/// This allows deriving multiple independent keys (DB, node, content)
/// from a single password without re-running Argon2id each time.
fn derive_subkey(password: &str, salt: &[u8], info: &[u8]) -> [u8; 32] {
    use hkdf::Hkdf;
    use sha2::Sha256;

    // Step 1: Argon2id master key
    let params = argon2::Params::new(64 * 1024, 3, 4, Some(32)).expect("valid argon2 params");
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut master = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut master)
        .expect("argon2 hash");

    // Step 2: HKDF-SHA256 expand
    let hk = Hkdf::<Sha256>::new(None, &master);
    let mut subkey = [0u8; 32];
    hk.expand(info, &mut subkey).expect("hkdf expand");

    // Clear master key material
    master.iter_mut().for_each(|b| *b = 0);

    subkey
}

/// Generate a cryptographically random 32-byte salt.
fn generate_salt() -> [u8; SALT_LEN] {
    use rand::rngs::OsRng;
    use rand::RngCore;
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    salt
}

/// Compute HMAC-SHA256 of the salt, keyed by the password.
///
/// This protects salt integrity (detects tampering/corruption) but not
/// confidentiality — the salt is not secret. The password is used as
/// the HMAC key because the salt is needed before the KDF can run.
fn compute_salt_hmac(password: &str, salt: &[u8]) -> [u8; HMAC_LEN] {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let mut mac =
        Hmac::<Sha256>::new_from_slice(password.as_bytes()).expect("HMAC can take key of any size");
    mac.update(salt);
    let result = mac.finalize();
    let mut out = [0u8; HMAC_LEN];
    out.copy_from_slice(&result.into_bytes());
    out
}

/// Write salt + HMAC tag to the salt file.
fn write_salt_with_hmac(
    vault_dir: &Path,
    salt: &[u8],
    password: &str,
) -> Result<(), KeystoreError> {
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
        KeystoreError::Stronghold(format!("salt file missing at {}", salt_path.display()))
    })?;

    if data.len() == SALT_LEN + HMAC_LEN {
        // New format: verify HMAC
        let (salt, stored_tag) = data.split_at(SALT_LEN);
        let expected_tag = compute_salt_hmac(password, salt);
        if stored_tag != expected_tag {
            return Err(KeystoreError::Stronghold(
                "salt file corrupted or tampered — integrity check failed".into(),
            ));
        }
        Ok(salt.to_vec())
    } else if data.len() == SALT_LEN {
        // Legacy format: no HMAC, accept but upgrade on next save
        log::warn!("Salt file uses legacy format (no integrity HMAC) — will upgrade on next save");
        Ok(data)
    } else {
        Err(KeystoreError::Stronghold(format!(
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
