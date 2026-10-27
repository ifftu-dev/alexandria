use std::path::{Path, PathBuf};

use argon2::Argon2;
use iota_stronghold::{KeyProvider, SnapshotPath, Stronghold};
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

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
    /// Argon2id(password, salt), computed once when the vault is created or
    /// opened. It is the snapshot key and the root every subkey is expanded
    /// from, so unlocking runs the memory-hard KDF once, not once per key.
    /// Cleared on lock().
    master: Zeroizing<[u8; 32]>,
    /// The password — kept for `check_password`. Cleared on lock().
    password: Zeroizing<String>,
}

impl std::fmt::Debug for Keystore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keystore")
            .field("state", &"<redacted>")
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
        let master = derive_key(password, &salt)?;
        let key_provider = key_provider(&master)?;

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
            master,
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
        let master = derive_key(password, &salt)?;
        let key_provider = key_provider(&master)?;

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
            master,
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
    /// Builds a fresh `KeyProvider` from the master key each time, since
    /// `KeyProvider` is not `Clone`.
    fn save(&self) -> Result<(), KeystoreError> {
        let key_provider = key_provider(&self.master)?;
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
    pub fn lock(mut self) -> Result<(), KeystoreError> {
        self.clear_secrets()?;
        // `self.password` is `Zeroizing<String>` — dropped and zeroed here.
        log::info!("Keystore locked");
        Ok(())
    }

    pub(crate) fn clear_secrets(&mut self) -> Result<(), KeystoreError> {
        // Zeroize first: a failed Stronghold clear leaves this keystore in
        // shared state, and it must not keep the password alive there.
        self.password.zeroize();
        self.master.zeroize();
        self.stronghold
            .clear()
            .map_err(|e| KeystoreError::Stronghold(format!("{e:?}")))
    }

    /// Get the vault directory path.
    pub fn vault_dir(&self) -> &Path {
        &self.vault_dir
    }

    /// Derive a 32-byte database encryption key.
    ///
    /// Uses HKDF-SHA256 over the Argon2id master key to produce a
    /// purpose-specific subkey.
    pub fn derive_db_key(&self) -> [u8; 32] {
        derive_subkey(&self.master, b"alexandria-db-key")
    }

    /// Derive a 32-byte key for iroh node secret encryption.
    pub fn derive_node_key(&self) -> [u8; 32] {
        derive_subkey(&self.master, b"alexandria-iroh-node-key")
    }

    /// Derive a 32-byte key for content blob encryption.
    pub fn derive_content_key(&self) -> [u8; 32] {
        derive_subkey(&self.master, b"alexandria-content-key")
    }
}

/// Derive the 32-byte master key from password + salt using Argon2id.
///
/// Argon2id is a memory-hard KDF that resists GPU/ASIC brute-force attacks.
/// Parameters: 64 MB memory, 3 iterations, 4 lanes. The 32-byte output is
/// Stronghold's snapshot key and the input to every subkey.
fn derive_key(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, KeystoreError> {
    let params = argon2::Params::new(
        64 * 1024, // m_cost: 64 MB memory
        3,         // t_cost: 3 iterations
        4,         // p_cost: 4 parallel lanes
        Some(32),  // output length: 32 bytes
    )
    .map_err(|e| KeystoreError::Memory(format!("Argon2 params error: {e}")))?;

    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let mut key = Zeroizing::new([0u8; 32]);
    argon2
        .hash_password_into(password.as_bytes(), salt, key.as_mut_slice())
        .map_err(|e| KeystoreError::Memory(format!("Argon2 hash failed: {e}")))?;
    Ok(key)
}

/// A Stronghold key provider for the master key.
fn key_provider(master: &[u8; 32]) -> Result<KeyProvider, KeystoreError> {
    KeyProvider::try_from(Zeroizing::new(master.to_vec()))
        .map_err(|e| KeystoreError::Memory(format!("{e:?}")))
}

/// Expand a purpose-specific 32-byte subkey from the master key with
/// HKDF-SHA256, so several independent keys (DB, node, content) come from
/// one Argon2id run.
fn derive_subkey(master: &[u8; 32], info: &[u8]) -> [u8; 32] {
    use hkdf::Hkdf;
    use sha2::Sha256;

    let hk = Hkdf::<Sha256>::new(None, master);
    let mut subkey = [0u8; 32];
    hk.expand(info, &mut subkey).expect("hkdf expand");
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

/// Read the salt file and verify its integrity HMAC.
///
/// The file is salt + HMAC, 64 bytes. The pre-HMAC 32-byte format is not
/// supported: it is refused rather than accepted without verification.
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
            // The HMAC is keyed by the entered password, so a mismatch is
            // indistinguishable from an incorrect password at unlock time.
            // Treat this the same as a wrong password instead of surfacing a
            // misleading corruption message to the user.
            return Err(KeystoreError::IncorrectPassword);
        }
        Ok(salt.to_vec())
    } else {
        Err(KeystoreError::Stronghold(format!(
            "unsupported salt file: {} bytes, expected {} (salt + integrity HMAC)",
            data.len(),
            SALT_LEN + HMAC_LEN
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn clear_secrets_erases_password_and_vault_memory_and_is_repeatable() {
        let directory = tempfile::TempDir::new().expect("temporary vault");
        let mut keystore =
            Keystore::create(directory.path(), "testpassword").expect("create vault");
        keystore
            .store_mnemonic("test mnemonic")
            .expect("store mnemonic");
        keystore.clear_secrets().expect("clear secrets");
        assert!(keystore.password.is_empty());
        assert_eq!(*keystore.master, [0u8; 32], "the master key is wiped too");
        assert!(keystore.retrieve_mnemonic().is_err());
        keystore.clear_secrets().expect("repeat cleanup");
    }

    /// Subkeys are expanded from one cached Argon2id result. They must stay
    /// exactly what re-running Argon2id per key produced before, or every
    /// existing database, node key, and content blob would stop opening.
    #[test]
    fn subkeys_match_a_fresh_argon2_run_and_survive_reopening() {
        use hkdf::Hkdf;
        use sha2::Sha256;

        let directory = tempfile::TempDir::new().expect("temporary vault");
        let created = Keystore::create(directory.path(), "testpassword").expect("create vault");
        created
            .store_mnemonic("test mnemonic")
            .expect("store mnemonic");
        let keys = (
            created.derive_db_key(),
            created.derive_node_key(),
            created.derive_content_key(),
        );

        let salt = read_and_verify_salt(directory.path(), "testpassword").expect("salt");
        let params = argon2::Params::new(64 * 1024, 3, 4, Some(32)).unwrap();
        let mut master = [0u8; 32];
        argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
            .hash_password_into(b"testpassword", &salt, &mut master)
            .unwrap();
        let expand = |info: &[u8]| {
            let mut out = [0u8; 32];
            Hkdf::<Sha256>::new(None, &master)
                .expand(info, &mut out)
                .unwrap();
            out
        };
        assert_eq!(keys.0, expand(b"alexandria-db-key"));
        assert_eq!(keys.1, expand(b"alexandria-iroh-node-key"));
        assert_eq!(keys.2, expand(b"alexandria-content-key"));
        assert_ne!(keys.0, keys.1);

        drop(created);
        let reopened = Keystore::open(directory.path(), "testpassword").expect("reopen");
        assert_eq!(reopened.derive_db_key(), keys.0);
        assert_eq!(reopened.derive_node_key(), keys.1);
        assert_eq!(reopened.derive_content_key(), keys.2);
        assert_eq!(reopened.retrieve_mnemonic().unwrap(), "test mnemonic");
    }

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
        let ks = Keystore::create(&dir, "testpassword").expect("create failed");
        assert!(Keystore::exists(&dir));
        let debug = format!("{ks:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains(dir.to_string_lossy().as_ref()));
        assert!(!debug.contains("testpassword"));
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
        assert!(
            matches!(result.unwrap_err(), KeystoreError::IncorrectPassword),
            "expected IncorrectPassword"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unsupported_legacy_salt_file_is_refused() {
        let dir = tempfile::TempDir::new().expect("temporary vault");
        let _keystore = Keystore::create(dir.path(), "testpassword").expect("create failed");
        let salt_path = dir.path().join(SALT_FILENAME);
        let current = fs::read(&salt_path).expect("read salt");
        fs::write(&salt_path, &current[..SALT_LEN]).expect("write pre-HMAC salt");

        let error = Keystore::open(dir.path(), "testpassword").expect_err("must refuse");

        assert!(
            format!("{error}").contains("unsupported salt file"),
            "{error}"
        );
        assert_eq!(
            fs::read(&salt_path).expect("salt still readable").len(),
            SALT_LEN,
            "the refused file must be left as it was"
        );
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
