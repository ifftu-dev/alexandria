//! Detection of the unsupported pre-profile single-vault layout.
//!
//! Before per-profile storage, an install kept one `alexandria.db`, one vault
//! directory, one iroh store, one plugin directory and one video cache
//! directly under the app data directory. That layout is no longer supported
//! and is never migrated, converted or deleted. Startup reports what it finds
//! and continues into onboarding for a fresh profile, so the old files stay
//! exactly where they are and can still be copied out by hand.

use std::path::{Path, PathBuf};

use super::manager::PROFILES_DIRNAME;

/// Reported when pre-profile data is found. The data is left untouched.
pub const UNSUPPORTED_LEGACY_LAYOUT: &str = "unsupported pre-profile data found in the app data \
     directory; it was left untouched and no profile was created from it";

/// Entries that only exist in the pre-profile layout.
const LEGACY_ENTRIES: [&str; 8] = [
    "alexandria.db",
    "alexandria.db-wal",
    "alexandria.db-shm",
    "stronghold",
    "vault",
    "iroh",
    "plugins",
    "videocache",
];

/// Entries that identify the layout. A stray `plugins/` or `iroh/` directory
/// alone is not pre-profile data.
const IDENTIFYING_ENTRIES: [&str; 3] = ["alexandria.db", "stronghold", "vault"];

/// List the pre-profile entries present under `app_data_dir`.
///
/// Returns an empty list once any profile exists, so an established install
/// never reports. Nothing is read, moved or removed.
pub fn detect(app_data_dir: &Path) -> Vec<PathBuf> {
    if has_profiles(app_data_dir) {
        return Vec::new();
    }
    if !IDENTIFYING_ENTRIES
        .iter()
        .any(|entry| app_data_dir.join(entry).exists())
    {
        return Vec::new();
    }
    LEGACY_ENTRIES
        .iter()
        .map(|entry| app_data_dir.join(entry))
        .filter(|path| path.exists())
        .collect()
}

/// Human-readable report naming every entry that was left in place.
pub fn report(entries: &[PathBuf]) -> String {
    let list = entries
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!("{UNSUPPORTED_LEGACY_LAYOUT}: {list}")
}

fn has_profiles(app_data_dir: &Path) -> bool {
    let root = app_data_dir.join(PROFILES_DIRNAME);
    if !root.exists() {
        return false;
    }
    std::fs::read_dir(&root)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, b"legacy").unwrap();
    }

    fn make_legacy(app_dir: &Path) {
        touch(&app_dir.join("alexandria.db"));
        touch(&app_dir.join("alexandria.db-wal"));
        std::fs::create_dir_all(app_dir.join("stronghold")).unwrap();
        std::fs::create_dir_all(app_dir.join("iroh")).unwrap();
        std::fs::create_dir_all(app_dir.join("videocache")).unwrap();
    }

    #[test]
    fn legacy_install_is_reported_and_left_untouched() {
        let dir = TempDir::new().unwrap();
        make_legacy(dir.path());

        let found = detect(dir.path());

        assert_eq!(found.len(), 5, "{found:?}");
        assert!(report(&found).contains("alexandria.db"));
        for entry in [
            "alexandria.db",
            "alexandria.db-wal",
            "stronghold",
            "iroh",
            "videocache",
        ] {
            assert!(dir.path().join(entry).exists(), "{entry} was touched");
        }
        assert!(!dir.path().join(PROFILES_DIRNAME).exists());
    }

    #[test]
    fn established_profile_install_reports_nothing() {
        let dir = TempDir::new().unwrap();
        make_legacy(dir.path());
        std::fs::create_dir_all(dir.path().join(PROFILES_DIRNAME).join("some-profile")).unwrap();

        assert!(detect(dir.path()).is_empty());
    }

    #[test]
    fn fresh_install_reports_nothing() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("iroh-staging")).unwrap();
        std::fs::create_dir_all(dir.path().join("plugins")).unwrap();

        assert!(detect(dir.path()).is_empty());
    }
}
