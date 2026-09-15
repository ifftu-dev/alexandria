//! `ProfileManager` — listing, creating, deleting, renaming profiles.
//!
//! The manager owns the public sidecar (`profiles_index.json`) and the
//! root directory layout under `<app_data>/profiles/`. It does not hold
//! per-profile cryptographic material — that lives in the keystore and
//! is loaded only when the corresponding profile is unlocked.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::network_profile::embedded_preprod;

use super::index::{Avatar, IndexError, ProfileIndex, ProfileSummary};

/// Root directory for all per-profile data (relative to app_data_dir).
pub const PROFILES_DIRNAME: &str = "profiles";

/// Stable opaque identifier for a profile. Wraps a UUIDv4 string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProfileId(String);

impl ProfileId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Parse from a string. Validates that it is a UUID to keep the
    /// disk layout predictable and prevent path traversal via crafted ids.
    pub fn parse(s: &str) -> Result<Self, ProfileError> {
        uuid::Uuid::parse_str(s).map_err(|_| ProfileError::InvalidId(s.to_string()))?;
        Ok(Self(s.to_string()))
    }
}

impl Default for ProfileId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ProfileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Resolved on-disk paths for a single profile. Cheap to recompute from
/// the profile id — held here so callers can stop juggling joins.
#[derive(Debug, Clone)]
pub struct ProfilePaths {
    pub id: ProfileId,
    pub root: PathBuf,
    pub vault_dir: PathBuf,
    pub db_path: PathBuf,
    pub iroh_dir: PathBuf,
    pub plugins_dir: PathBuf,
    pub video_cache_dir: PathBuf,
}

impl ProfilePaths {
    pub fn for_id(app_data_dir: &Path, id: &ProfileId) -> Self {
        let root = app_data_dir.join(PROFILES_DIRNAME).join(id.as_str());
        Self {
            id: id.clone(),
            vault_dir: root.join("vault"),
            db_path: root.join("alexandria.db"),
            iroh_dir: root.join("iroh"),
            plugins_dir: root.join("plugins"),
            video_cache_dir: root.join("videocache"),
            root,
        }
    }

    /// Create all per-profile subdirectories. Idempotent.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.vault_dir)?;
        std::fs::create_dir_all(&self.iroh_dir)?;
        std::fs::create_dir_all(&self.plugins_dir)?;
        std::fs::create_dir_all(&self.video_cache_dir)?;
        Ok(())
    }

    /// Remove transient plaintext media while retaining the cache directory.
    ///
    /// Directory symlinks are unlinked rather than followed. This matters
    /// because the directory is reachable through Tauri's asset protocol and
    /// must not expose a locked profile's materialized content.
    pub fn clear_video_cache(&self) -> std::io::Result<()> {
        let entries = match std::fs::read_dir(&self.video_cache_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return std::fs::create_dir_all(&self.video_cache_dir);
            }
            Err(error) => return Err(error),
        };

        for entry in entries {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() && !file_type.is_symlink() {
                std::fs::remove_dir_all(entry.path())?;
            } else {
                std::fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }
}

#[derive(Error, Debug)]
pub enum ProfileError {
    #[error("profile not found: {0}")]
    NotFound(String),
    #[error("invalid profile id: {0}")]
    InvalidId(String),
    #[error("display name must be 1-64 characters")]
    InvalidDisplayName,
    #[error("profile {profile_id} belongs to network {actual}; this build requires {expected}")]
    NetworkMismatch {
        profile_id: String,
        expected: String,
        actual: String,
    },
    #[error("embedded network profile is invalid: {0}")]
    NetworkProfile(String),
    #[error("profile index error: {0}")]
    Index(#[from] IndexError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Load the public profile index and refuse any profile bound to a network
/// other than the one embedded in this build. The app and CLI share this gate
/// so neither can select a profile the other would refuse to open.
pub fn load_network_index(app_data_dir: &Path) -> Result<ProfileIndex, ProfileError> {
    let index = ProfileIndex::load(app_data_dir)?;
    let expected_network = &embedded_preprod()
        .map_err(|error| ProfileError::NetworkProfile(error.to_string()))?
        .network_id;
    if let Some(profile) = index
        .profiles
        .iter()
        .find(|profile| &profile.network_id != expected_network)
    {
        return Err(ProfileError::NetworkMismatch {
            profile_id: profile.id.to_string(),
            expected: expected_network.clone(),
            actual: profile.network_id.clone(),
        });
    }
    Ok(index)
}

/// Coordinates the profile index + per-profile directories. Safe to
/// share across threads; the index is guarded by an internal mutex.
pub struct ProfileManager {
    app_data_dir: PathBuf,
    index: Mutex<ProfileIndex>,
}

impl ProfileManager {
    /// Open the manager rooted at `app_data_dir`. Loads (or initializes)
    /// the sidecar index.
    pub fn open(app_data_dir: &Path) -> Result<Self, ProfileError> {
        std::fs::create_dir_all(app_data_dir.join(PROFILES_DIRNAME))?;
        let index = load_network_index(app_data_dir)?;
        Ok(Self {
            app_data_dir: app_data_dir.to_path_buf(),
            index: Mutex::new(index),
        })
    }

    pub fn app_data_dir(&self) -> &Path {
        &self.app_data_dir
    }

    /// Snapshot the current list of profiles. Sorted by `last_unlocked_at`
    /// descending (most recently used first), then by `created_at`.
    pub fn list(&self) -> Vec<ProfileSummary> {
        let guard = self.index.lock().expect("profile index mutex poisoned");
        let mut out = guard.profiles.clone();
        out.sort_by(|a, b| {
            let a_key = a.last_unlocked_at.unwrap_or(a.created_at);
            let b_key = b.last_unlocked_at.unwrap_or(b.created_at);
            b_key.cmp(&a_key)
        });
        out
    }

    pub fn count(&self) -> usize {
        self.index
            .lock()
            .expect("profile index mutex poisoned")
            .profiles
            .len()
    }

    pub fn get(&self, id: &ProfileId) -> Option<ProfileSummary> {
        self.index
            .lock()
            .expect("profile index mutex poisoned")
            .get(id)
            .cloned()
    }

    /// Resolve paths for a profile id. Does not check whether the
    /// directory actually exists on disk.
    pub fn paths_for(&self, id: &ProfileId) -> ProfilePaths {
        ProfilePaths::for_id(&self.app_data_dir, id)
    }

    /// Reserve a new profile slot: assigns an id, creates directories,
    /// adds an index entry, persists the index. Does **not** create the
    /// vault or database — the caller (typically `unlock_profile` on a
    /// fresh profile) must do that and then call `touch_unlocked`.
    pub fn create(&self, display_name: &str, avatar: Avatar) -> Result<ProfilePaths, ProfileError> {
        let network_id = &embedded_preprod()
            .expect("embedded network profile is valid")
            .network_id;
        self.create_on_network(display_name, avatar, network_id)
    }

    pub fn create_on_network(
        &self,
        display_name: &str,
        avatar: Avatar,
        network_id: &str,
    ) -> Result<ProfilePaths, ProfileError> {
        let display_name = display_name.trim();
        if display_name.is_empty() || display_name.chars().count() > 64 {
            return Err(ProfileError::InvalidDisplayName);
        }
        let expected_network = &embedded_preprod()
            .expect("embedded network profile is valid")
            .network_id;
        if network_id != expected_network {
            return Err(ProfileError::NetworkMismatch {
                profile_id: "new profile".to_string(),
                expected: expected_network.clone(),
                actual: network_id.to_string(),
            });
        }

        let id = ProfileId::new();
        let paths = ProfilePaths::for_id(&self.app_data_dir, &id);
        paths.ensure_dirs()?;

        let summary = ProfileSummary {
            id: id.clone(),
            network_id: network_id.to_string(),
            display_name: display_name.to_string(),
            avatar,
            color: pick_color(&id),
            created_at: Utc::now(),
            last_unlocked_at: None,
        };

        {
            let mut guard = self.index.lock().expect("profile index mutex poisoned");
            guard.upsert(summary);
            guard.save(&self.app_data_dir)?;
        }

        log::info!("created profile {id}");
        Ok(paths)
    }

    /// Rename a profile. Display-name validation matches `create`.
    pub fn rename(&self, id: &ProfileId, new_name: &str) -> Result<(), ProfileError> {
        let new_name = new_name.trim();
        if new_name.is_empty() || new_name.chars().count() > 64 {
            return Err(ProfileError::InvalidDisplayName);
        }
        let mut guard = self.index.lock().expect("profile index mutex poisoned");
        let entry = guard
            .get_mut(id)
            .ok_or_else(|| ProfileError::NotFound(id.to_string()))?;
        entry.display_name = new_name.to_string();
        guard.save(&self.app_data_dir)?;
        Ok(())
    }

    /// Update the avatar on an existing profile. Persists the index.
    pub fn set_avatar(&self, id: &ProfileId, avatar: Avatar) -> Result<(), ProfileError> {
        let mut guard = self.index.lock().expect("profile index mutex poisoned");
        let entry = guard
            .get_mut(id)
            .ok_or_else(|| ProfileError::NotFound(id.to_string()))?;
        entry.avatar = avatar;
        guard.save(&self.app_data_dir)?;
        Ok(())
    }

    /// Stamp `last_unlocked_at` to now. Called after a successful unlock.
    pub fn touch_unlocked(&self, id: &ProfileId) -> Result<(), ProfileError> {
        let mut guard = self.index.lock().expect("profile index mutex poisoned");
        let entry = guard
            .get_mut(id)
            .ok_or_else(|| ProfileError::NotFound(id.to_string()))?;
        entry.last_unlocked_at = Some(Utc::now());
        guard.save(&self.app_data_dir)?;
        Ok(())
    }

    /// Remove the profile from the index AND delete its directory from
    /// disk. Best-effort overwrite — relies on password protection rather
    /// than cryptographic erasure. The caller MUST ensure the profile is
    /// not currently the active (unlocked) one.
    pub fn delete(&self, id: &ProfileId) -> Result<(), ProfileError> {
        let paths = ProfilePaths::for_id(&self.app_data_dir, id);
        if paths.root.exists() {
            std::fs::remove_dir_all(&paths.root)?;
        }

        let mut guard = self.index.lock().expect("profile index mutex poisoned");
        if guard.remove(id).is_none() {
            return Err(ProfileError::NotFound(id.to_string()));
        }
        guard.save(&self.app_data_dir)?;
        log::info!("deleted profile {id}");
        Ok(())
    }
}

/// Pick a stable accent color for a profile based on its id. Deterministic so
/// renaming the profile does not change the tile color.
fn pick_color(id: &ProfileId) -> String {
    const PALETTE: &[&str] = &[
        "#7c3aed", // violet-600
        "#0ea5e9", // sky-500
        "#10b981", // emerald-500
        "#f59e0b", // amber-500
        "#ef4444", // red-500
        "#ec4899", // pink-500
        "#14b8a6", // teal-500
        "#8b5cf6", // violet-500
    ];
    let bytes = id.as_str().as_bytes();
    let sum: usize = bytes.iter().map(|b| *b as usize).sum();
    PALETTE[sum % PALETTE.len()].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn manager() -> (TempDir, ProfileManager) {
        let tmp = TempDir::new().unwrap();
        let m = ProfileManager::open(tmp.path()).unwrap();
        (tmp, m)
    }

    #[test]
    fn fresh_manager_has_no_profiles() {
        let (_tmp, m) = manager();
        assert_eq!(m.count(), 0);
        assert!(m.list().is_empty());
    }

    #[test]
    fn create_profile_persists_index_and_dirs() {
        let (tmp, m) = manager();
        let paths = m.create("Pratyush", Avatar::default()).unwrap();
        assert!(paths.root.exists());
        assert!(paths.vault_dir.exists());
        assert!(paths.iroh_dir.exists());
        assert!(paths.plugins_dir.exists());
        assert!(paths.video_cache_dir.exists());
        assert_eq!(m.count(), 1);
        assert_eq!(m.get(&paths.id).unwrap().network_id, "preprod");

        // Reopen — index must survive.
        let m2 = ProfileManager::open(tmp.path()).unwrap();
        assert_eq!(m2.count(), 1);
        assert_eq!(m2.list()[0].display_name, "Pratyush");
    }

    #[test]
    fn create_rejects_another_network() {
        let (_tmp, m) = manager();
        let error = m
            .create_on_network("Alice", Avatar::default(), "other-network")
            .unwrap_err();
        assert!(matches!(error, ProfileError::NetworkMismatch { .. }));
        assert_eq!(m.count(), 0);
    }

    #[test]
    fn open_rejects_a_profile_from_another_network() {
        let (tmp, m) = manager();
        let paths = m.create("Alice", Avatar::default()).unwrap();
        drop(m);

        let index_path = tmp.path().join(super::super::index::INDEX_FILENAME);
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
        value["profiles"][0]["network_id"] = serde_json::json!("other-network");
        std::fs::write(&index_path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        let error = match ProfileManager::open(tmp.path()) {
            Ok(_) => panic!("cross-network profile should be rejected"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            ProfileError::NetworkMismatch {
                actual,
                expected,
                profile_id
            } if actual == "other-network" && expected == "preprod" && profile_id == paths.id.to_string()
        ));
    }

    #[test]
    fn rename_round_trip() {
        let (_tmp, m) = manager();
        let paths = m.create("Old name", Avatar::default()).unwrap();
        m.rename(&paths.id, "New name").unwrap();
        assert_eq!(m.get(&paths.id).unwrap().display_name, "New name");
    }

    #[test]
    fn rename_rejects_blank_or_too_long() {
        let (_tmp, m) = manager();
        let paths = m.create("Alice", Avatar::default()).unwrap();
        assert!(matches!(
            m.rename(&paths.id, ""),
            Err(ProfileError::InvalidDisplayName)
        ));
        assert!(matches!(
            m.rename(&paths.id, &"x".repeat(65)),
            Err(ProfileError::InvalidDisplayName)
        ));
    }

    #[test]
    fn delete_removes_dir_and_entry() {
        let (_tmp, m) = manager();
        let paths = m.create("Bob", Avatar::default()).unwrap();
        assert!(paths.root.exists());
        m.delete(&paths.id).unwrap();
        assert!(!paths.root.exists());
        assert_eq!(m.count(), 0);
    }

    #[test]
    fn delete_unknown_profile_errors() {
        let (_tmp, m) = manager();
        let err = m.delete(&ProfileId::new()).unwrap_err();
        assert!(matches!(err, ProfileError::NotFound(_)));
    }

    #[test]
    fn list_is_sorted_by_last_unlock_desc() {
        let (_tmp, m) = manager();
        let a = m.create("A", Avatar::default()).unwrap();
        let b = m.create("B", Avatar::default()).unwrap();
        let c = m.create("C", Avatar::default()).unwrap();

        // Touch in reverse order so c is most recent.
        m.touch_unlocked(&a.id).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        m.touch_unlocked(&b.id).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        m.touch_unlocked(&c.id).unwrap();

        let sorted: Vec<_> = m.list().into_iter().map(|p| p.display_name).collect();
        assert_eq!(sorted, vec!["C", "B", "A"]);
    }

    #[test]
    fn paths_for_id_is_deterministic() {
        let (tmp, m) = manager();
        let id = ProfileId::new();
        let p1 = m.paths_for(&id);
        let p2 = ProfilePaths::for_id(tmp.path(), &id);
        assert_eq!(p1.root, p2.root);
        assert_eq!(p1.db_path, p2.db_path);
    }

    #[test]
    fn profile_id_rejects_non_uuid_strings() {
        assert!(ProfileId::parse("../../etc/passwd").is_err());
        assert!(ProfileId::parse("not-a-uuid").is_err());
        let id = ProfileId::new();
        assert!(ProfileId::parse(id.as_str()).is_ok());
    }
}
