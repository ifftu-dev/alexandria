use anyhow::{bail, Context, Result};
use std::env;
use std::path::{Path, PathBuf};

const SALT_FILENAME: &str = "vault_salt.bin";
const SALT_LEN: usize = 32;
const HMAC_LEN: usize = 32;

const APP_IDENTIFIER: &str = "org.alexandria.node";

/// Project context — resolved paths for the Alexandria project.
///
/// `root` is optional because most of this CLI has nothing to do with a source
/// checkout. The vault, credential, database, and TUI commands operate on the
/// installed app's data directory, which is resolved from platform paths and
/// exists whether or not the repository does. Only the commands that shell out
/// to `cargo`/`xcrun` — `run`, `clean build`, and parts of `doctor` — need a
/// checkout, and they ask for it explicitly via [`require_root`].
///
/// Making this mandatory would break every command for anyone who installed
/// the CLI from the app bundle and ran it from their home directory.
#[derive(Debug, Clone)]
pub struct ProjectContext {
    /// Root of the project (contains package.json + src-tauri/), when the
    /// command was run from inside a checkout.
    pub root: Option<PathBuf>,
    /// App data directory (~/Library/Application Support/org.alexandria.node/)
    pub app_data_dir: PathBuf,
    /// Which profile's data the vault/database commands operate on.
    pub profile: ProfileSelection,
}

/// Where a profile's files live.
///
/// The app stores each profile under `profiles/<uuid>/` with its vault in
/// `vault/`. Installs predating that migration keep a single flat layout with
/// the desktop vault in `stronghold/`, so both are resolved here — a CLI that
/// only understood one of them would silently look at the wrong place.
#[derive(Debug, Clone)]
pub enum ProfileSelection {
    /// A profile from `profiles_index.json`.
    Profile {
        id: String,
        display_name: String,
        root: PathBuf,
    },
    /// Pre-migration single-vault layout, directly under the app data dir.
    Legacy,
}

impl ProfileSelection {
    fn root(&self, app_data_dir: &Path) -> PathBuf {
        match self {
            ProfileSelection::Profile { root, .. } => root.clone(),
            ProfileSelection::Legacy => app_data_dir.to_path_buf(),
        }
    }

    /// Human label for status output.
    pub fn label(&self) -> String {
        match self {
            ProfileSelection::Profile {
                display_name, id, ..
            } => {
                format!("{display_name} ({})", &id[..id.len().min(8)])
            }
            ProfileSelection::Legacy => "legacy single-vault layout".to_string(),
        }
    }
}

impl ProjectContext {
    /// Resolve paths. Never fails for missing a checkout — see the type docs.
    ///
    /// `wanted` selects a profile by id (or id prefix) or display name. With
    /// none given, the most recently unlocked profile is used, which is the
    /// one the app itself would open.
    pub fn detect(wanted: Option<&str>) -> Result<Self> {
        let cwd = env::current_dir().context("Failed to get current directory")?;
        let app_data_dir = Self::resolve_app_data_dir();
        let profile = resolve_profile(&app_data_dir, wanted)?;
        Ok(Self {
            root: find_project_root(&cwd),
            app_data_dir,
            profile,
        })
    }

    /// Directory holding the selected profile's files.
    pub fn profile_root(&self) -> PathBuf {
        self.profile.root(&self.app_data_dir)
    }

    /// The checkout root, or a message explaining that this command needs one.
    pub fn require_root(&self) -> Result<&Path> {
        self.root.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "This command needs an Alexandria source checkout, and none was found \n\
                 above the current directory.\n\
                 Run it from inside the project (looking for src-tauri/tauri.conf.json).\n\n\
                 Commands that work anywhere: credentials, vc, vp, role-assessment, db, \n\
                 tui, doctor, path, clean data."
            )
        })
    }

    /// The `src-tauri` directory, when there is a checkout.
    pub fn tauri_dir(&self) -> Option<PathBuf> {
        self.root.as_ref().map(|r| r.join("src-tauri"))
    }

    /// The `src-tauri` directory, or the same explanation as [`require_root`].
    pub fn require_tauri_dir(&self) -> Result<PathBuf> {
        Ok(self.require_root()?.join("src-tauri"))
    }

    /// Get the SQLite database path
    pub fn db_path(&self) -> PathBuf {
        self.profile_root().join("alexandria.db")
    }

    /// Get the vault directory.
    ///
    /// Profiles always use `vault/`. The pre-migration layout used
    /// `stronghold/` on desktop and `vault/` on mobile, so both are accepted
    /// there, preferring whichever actually exists.
    pub fn vault_dir(&self) -> PathBuf {
        match &self.profile {
            ProfileSelection::Profile { .. } => self.profile_root().join("vault"),
            ProfileSelection::Legacy => {
                let stronghold = self.app_data_dir.join("stronghold");
                if stronghold.join(SALT_FILENAME).exists() {
                    stronghold
                } else {
                    self.app_data_dir.join("vault")
                }
            }
        }
    }

    /// Get the iroh data directory
    pub fn iroh_dir(&self) -> PathBuf {
        self.profile_root().join("iroh")
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

// ── Profile resolution ──────────────────────────────────────────────

/// Pick which profile's data to operate on.
///
/// Deserializes the app's own `profiles_index.json` through `app_lib`, so the
/// CLI cannot drift from the format the app writes.
fn resolve_profile(app_data_dir: &Path, wanted: Option<&str>) -> Result<ProfileSelection> {
    use app_lib::profile::index::{ProfileIndex, INDEX_FILENAME};
    use app_lib::profile::manager::PROFILES_DIRNAME;

    let index_path = app_data_dir.join(INDEX_FILENAME);
    let Ok(raw) = std::fs::read_to_string(&index_path) else {
        // No index: either a pre-migration install or the app has never run.
        // Either way the legacy paths are the only ones that could exist.
        if let Some(name) = wanted {
            bail!(
                "No profiles found at {} — cannot select `{name}`.\n\
                 Launch the app and create a profile first.",
                index_path.display()
            );
        }
        return Ok(ProfileSelection::Legacy);
    };

    let index: ProfileIndex = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", index_path.display()))?;

    if index.profiles.is_empty() {
        return Ok(ProfileSelection::Legacy);
    }

    let chosen = match wanted {
        Some(name) => {
            let needle = name.to_lowercase();
            let matches: Vec<_> = index
                .profiles
                .iter()
                .filter(|p| {
                    p.id.as_str() == name
                        || p.id.as_str().starts_with(name)
                        || p.display_name.to_lowercase() == needle
                })
                .collect();
            match matches.as_slice() {
                [one] => *one,
                [] => bail!("{}", no_such_profile(name, &index)),
                many => bail!(
                    "`{name}` matches {} profiles. Use the full id:\n{}",
                    many.len(),
                    list_profiles(&index)
                ),
            }
        }
        // Most recently unlocked is the one the app itself would open.
        None => index
            .profiles
            .iter()
            .max_by_key(|p| p.last_unlocked_at.unwrap_or(p.created_at))
            .expect("non-empty checked above"),
    };

    Ok(ProfileSelection::Profile {
        id: chosen.id.as_str().to_string(),
        display_name: chosen.display_name.clone(),
        root: app_data_dir.join(PROFILES_DIRNAME).join(chosen.id.as_str()),
    })
}

fn list_profiles(index: &app_lib::profile::index::ProfileIndex) -> String {
    index
        .profiles
        .iter()
        .map(|p| format!("  {}  {}", p.id.as_str(), p.display_name))
        .collect::<Vec<_>>()
        .join("\n")
}

fn no_such_profile(name: &str, index: &app_lib::profile::index::ProfileIndex) -> String {
    format!(
        "No profile matches `{name}`.\n\nAvailable profiles:\n{}",
        list_profiles(index)
    )
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

/// Walk up from the given path looking for a directory containing
/// src-tauri/tauri.conf.json. `None` when there is no checkout above `start` —
/// the normal case for a CLI installed from the app bundle.
fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join("src-tauri/tauri.conf.json").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_with_no_checkout_above_it_has_no_root() {
        // The CLI is installed from the app bundle and run from anywhere, so
        // this is the common case, not an error. `/` can never contain a
        // checkout above it.
        assert_eq!(find_project_root(Path::new("/")), None);
    }

    #[test]
    fn detect_succeeds_without_a_checkout() {
        // Regression: `detect()` used to bail when no checkout was found,
        // which made every command — including the vault and credential ones
        // that never touch a checkout — fail for an installed CLI.
        let ctx = ProjectContext {
            root: None,
            app_data_dir: PathBuf::from("/tmp/appdata"),
            profile: ProfileSelection::Legacy,
        };
        assert_eq!(ctx.db_path(), PathBuf::from("/tmp/appdata/alexandria.db"));
        assert_eq!(ctx.iroh_dir(), PathBuf::from("/tmp/appdata/iroh"));
    }

    #[test]
    fn a_profile_puts_every_path_under_its_own_directory() {
        // Regression: the CLI used the pre-migration flat layout, so it looked
        // for a vault at <app-data>/stronghold while the app had moved
        // everything to profiles/<uuid>/ with the vault in vault/. Every
        // database and credential command failed with "No vault found".
        let ctx = ProjectContext {
            root: None,
            app_data_dir: PathBuf::from("/tmp/appdata"),
            profile: ProfileSelection::Profile {
                id: "52fce5a0-8b4e-4db8-af45-db96d3f3e647".into(),
                display_name: "Test".into(),
                root: PathBuf::from("/tmp/appdata/profiles/52fce5a0-8b4e-4db8-af45-db96d3f3e647"),
            },
        };
        let root = PathBuf::from("/tmp/appdata/profiles/52fce5a0-8b4e-4db8-af45-db96d3f3e647");
        assert_eq!(ctx.db_path(), root.join("alexandria.db"));
        assert_eq!(
            ctx.vault_dir(),
            root.join("vault"),
            "profiles use vault/, not stronghold/"
        );
        assert_eq!(ctx.iroh_dir(), root.join("iroh"));
    }

    #[test]
    fn legacy_prefers_stronghold_when_it_holds_the_salt() {
        // Desktop installs predating the migration keep the vault in
        // stronghold/; mobile used vault/. Pick whichever actually has a salt
        // file rather than guessing from the platform.
        let base = std::env::temp_dir().join("alexandria-ctx-legacy-test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("stronghold")).unwrap();
        std::fs::write(base.join("stronghold").join(SALT_FILENAME), b"x").unwrap();

        let ctx = ProjectContext {
            root: None,
            app_data_dir: base.clone(),
            profile: ProfileSelection::Legacy,
        };
        assert_eq!(ctx.vault_dir(), base.join("stronghold"));

        // With no stronghold salt, fall back to the mobile-style vault/.
        std::fs::remove_file(base.join("stronghold").join(SALT_FILENAME)).unwrap();
        assert_eq!(ctx.vault_dir(), base.join("vault"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn resolve_profile_picks_the_most_recently_unlocked() {
        let base = std::env::temp_dir().join("alexandria-ctx-index-test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(
            base.join("profiles_index.json"),
            r#"{"version":1,"profiles":[
                {"id":"11111111-1111-4111-8111-111111111111","display_name":"Older",
                 "created_at":"2026-01-01T00:00:00Z","last_unlocked_at":"2026-01-02T00:00:00Z"},
                {"id":"22222222-2222-4222-8222-222222222222","display_name":"Newer",
                 "created_at":"2026-01-01T00:00:00Z","last_unlocked_at":"2026-06-01T00:00:00Z"}
            ]}"#,
        )
        .unwrap();

        // No selection: the app opens the most recently unlocked, so we do too.
        match resolve_profile(&base, None).unwrap() {
            ProfileSelection::Profile { display_name, .. } => assert_eq!(display_name, "Newer"),
            other => panic!("expected a profile, got {other:?}"),
        }

        // By display name, case-insensitively.
        match resolve_profile(&base, Some("older")).unwrap() {
            ProfileSelection::Profile { display_name, .. } => assert_eq!(display_name, "Older"),
            other => panic!("expected a profile, got {other:?}"),
        }

        // By id prefix.
        match resolve_profile(&base, Some("11111111")).unwrap() {
            ProfileSelection::Profile { display_name, .. } => assert_eq!(display_name, "Older"),
            other => panic!("expected a profile, got {other:?}"),
        }

        // An unknown name lists what is available rather than just refusing.
        let err = resolve_profile(&base, Some("nope"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("Older") && err.contains("Newer"), "got: {err}");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn resolve_profile_falls_back_to_legacy_with_no_index() {
        let base = std::env::temp_dir().join("alexandria-ctx-noindex-test");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        assert!(matches!(
            resolve_profile(&base, None).unwrap(),
            ProfileSelection::Legacy
        ));
        // Asking for a named profile when there is no index is an error, not a
        // silent fall back to the wrong data.
        assert!(resolve_profile(&base, Some("whatever")).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn require_root_explains_itself_and_names_the_commands_that_work() {
        let ctx = ProjectContext {
            root: None,
            app_data_dir: PathBuf::from("/tmp/appdata"),
            profile: ProfileSelection::Legacy,
        };
        assert!(ctx.tauri_dir().is_none());

        let err = ctx.require_root().unwrap_err().to_string();
        assert!(err.contains("source checkout"), "got: {err}");
        // The message has to point somewhere useful, not just refuse.
        assert!(err.contains("credentials"), "got: {err}");
        assert!(err.contains("tui"), "got: {err}");
    }

    #[test]
    fn require_root_returns_the_root_when_there_is_one() {
        let ctx = ProjectContext {
            root: Some(PathBuf::from("/repo")),
            app_data_dir: PathBuf::from("/tmp/appdata"),
            profile: ProfileSelection::Legacy,
        };
        assert_eq!(ctx.require_root().unwrap(), Path::new("/repo"));
        assert_eq!(
            ctx.require_tauri_dir().unwrap(),
            PathBuf::from("/repo/src-tauri")
        );
    }
}
