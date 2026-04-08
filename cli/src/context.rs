use anyhow::{bail, Context, Result};
use std::env;
use std::path::{Path, PathBuf};

const SALT_FILENAME: &str = "vault_salt.bin";
const SALT_LEN: usize = 32;
const HMAC_LEN: usize = 32;

const APP_IDENTIFIER: &str = "org.alexandria.node";

/// Project context — resolved paths for the Alexandria project
#[derive(Debug, Clone)]
pub struct ProjectContext {
    /// Root of the project (contains package.json + src-tauri/)
    pub root: PathBuf,
    /// src-tauri directory
    pub tauri_dir: PathBuf,
    /// App data directory (~/Library/Application Support/org.alexandria.node/)
    pub app_data_dir: PathBuf,
}

impl ProjectContext {
    /// Detect the project root by walking up from CWD looking for src-tauri/tauri.conf.json
    pub fn detect() -> Result<Self> {
        let cwd = env::current_dir().context("Failed to get current directory")?;
        let root = find_project_root(&cwd)?;
        let tauri_dir = root.join("src-tauri");
        let app_data_dir = Self::resolve_app_data_dir();

        Ok(Self {
            root,
            tauri_dir,
            app_data_dir,
        })
    }

    /// Get the SQLite database path
    pub fn db_path(&self) -> PathBuf {
        self.app_data_dir.join("alexandria.db")
    }

    /// Get the vault directory (stronghold/ on desktop, vault/ on mobile)
    pub fn vault_dir(&self) -> PathBuf {
        self.app_data_dir.join("stronghold")
    }

    /// Get the iroh data directory
    pub fn iroh_dir(&self) -> PathBuf {
        self.app_data_dir.join("iroh")
    }

    /// Check if app data directory exists
    pub fn has_app_data(&self) -> bool {
        self.app_data_dir.exists()
    }

    /// Check if the database exists
    pub fn has_db(&self) -> bool {
        self.db_path().exists()
    }

    /// Check if the vault exists
    pub fn has_vault(&self) -> bool {
        self.vault_dir().join(SALT_FILENAME).exists()
    }

    /// Derive the 32-byte database encryption key from a password.
    ///
    /// Reads the vault salt from disk and runs Argon2id + HKDF-SHA256,
    /// matching the key derivation in the Tauri app.
    pub fn derive_db_key(&self, password: &str) -> Result<[u8; 32]> {
        let salt = self.read_vault_salt(password)?;
        Ok(derive_subkey(password, &salt, b"alexandria-db-key"))
    }

    /// Read and verify the vault salt file.
    fn read_vault_salt(&self, password: &str) -> Result<Vec<u8>> {
        let salt_path = self.vault_dir().join(SALT_FILENAME);
        let data = std::fs::read(&salt_path)
            .with_context(|| format!("vault salt not found at {}", salt_path.display()))?;

        if data.len() == SALT_LEN + HMAC_LEN {
            let (salt, stored_tag) = data.split_at(SALT_LEN);
            let expected_tag = compute_salt_hmac(password, salt);
            if stored_tag != expected_tag {
                bail!("Incorrect vault password");
            }
            Ok(salt.to_vec())
        } else if data.len() == SALT_LEN {
            Ok(data)
        } else {
            bail!(
                "Corrupt vault salt file ({} bytes, expected {})",
                data.len(),
                SALT_LEN + HMAC_LEN
            );
        }
    }

    /// Resolve the app data directory based on platform
    fn resolve_app_data_dir() -> PathBuf {
        if cfg!(target_os = "macos") {
            dirs::home_dir()
                .unwrap_or_default()
                .join("Library/Application Support")
                .join(APP_IDENTIFIER)
        } else if cfg!(target_os = "linux") {
            dirs::data_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".local/share"))
                .join(APP_IDENTIFIER)
        } else if cfg!(target_os = "windows") {
            dirs::data_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("AppData/Roaming"))
                .join(APP_IDENTIFIER)
        } else {
            dirs::home_dir()
                .unwrap_or_default()
                .join(format!(".{}", APP_IDENTIFIER))
        }
    }
}

// ── Key derivation (mirrors src-tauri/src/crypto/keystore.rs) ───────

/// Argon2id → HKDF-SHA256 subkey derivation.
fn derive_subkey(password: &str, salt: &[u8], info: &[u8]) -> [u8; 32] {
    use argon2::Argon2;
    use hkdf::Hkdf;
    use sha2::Sha256;

    let params = argon2::Params::new(64 * 1024, 3, 4, Some(32)).expect("valid argon2 params");
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut master = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut master)
        .expect("argon2 hash");

    let hk = Hkdf::<Sha256>::new(None, &master);
    let mut subkey = [0u8; 32];
    hk.expand(info, &mut subkey).expect("hkdf expand");

    master.iter_mut().for_each(|b| *b = 0);
    subkey
}

/// HMAC-SHA256 of the salt, keyed by password (for integrity check).
fn compute_salt_hmac(password: &str, salt: &[u8]) -> [u8; 32] {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let mut mac =
        Hmac::<Sha256>::new_from_slice(password.as_bytes()).expect("HMAC accepts any key length");
    mac.update(salt);
    let result = mac.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result.into_bytes());
    out
}

/// Walk up from the given path looking for a directory containing src-tauri/tauri.conf.json
fn find_project_root(start: &Path) -> Result<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join("src-tauri/tauri.conf.json").exists() {
            return Ok(current);
        }
        if !current.pop() {
            bail!(
                "Could not find Alexandria project root.\n\
                 Run this command from within the project directory.\n\
                 (Looking for src-tauri/tauri.conf.json)"
            );
        }
    }
}
