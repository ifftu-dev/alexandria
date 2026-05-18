//! Public sidecar `profiles_index.json` — read by the picker before any
//! vault is unlocked. Only safe-to-disclose fields live here.

use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::manager::ProfileId;

/// Current schema version. Bump when adding non-backward-compatible fields.
pub const INDEX_VERSION: u32 = 1;

/// Filename of the public sidecar.
pub const INDEX_FILENAME: &str = "profiles_index.json";

/// Avatar representation. Kept small + serializable to keep the index lean.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Avatar {
    /// A single emoji character (e.g. "🦊"). Default for newly-created profiles.
    Emoji(String),
    /// Reference to a local image at `<profile_dir>/avatar.png`. The string
    /// is a content hash (blake3-hex) so picker can invalidate cache when
    /// the file changes; the bytes themselves are not in the index.
    Image { hash: String },
    /// Deterministic identicon derived from the profile id. No bytes stored.
    Identicon,
}

impl Default for Avatar {
    fn default() -> Self {
        Avatar::Emoji("🙂".to_string())
    }
}

/// Public summary of a single profile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileSummary {
    pub id: ProfileId,
    pub display_name: String,
    #[serde(default)]
    pub avatar: Avatar,
    /// Hex CSS color, e.g. "#7c3aed". Used as accent on the picker tile.
    #[serde(default = "default_color")]
    pub color: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub last_unlocked_at: Option<DateTime<Utc>>,
}

fn default_color() -> String {
    "#7c3aed".to_string()
}

/// Top-level sidecar contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileIndex {
    pub version: u32,
    pub profiles: Vec<ProfileSummary>,
}

impl Default for ProfileIndex {
    fn default() -> Self {
        Self {
            version: INDEX_VERSION,
            profiles: Vec::new(),
        }
    }
}

#[derive(Error, Debug)]
pub enum IndexError {
    #[error("IO error reading profiles index: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid profiles index JSON: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("unsupported profiles index version: {0}")]
    UnsupportedVersion(u32),
}

impl ProfileIndex {
    /// Load the index from `<app_data>/profiles_index.json`. Returns an
    /// empty index if the file does not exist.
    pub fn load(app_data_dir: &Path) -> Result<Self, IndexError> {
        let path = app_data_dir.join(INDEX_FILENAME);
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = std::fs::read(&path)?;
        let index: ProfileIndex = serde_json::from_slice(&bytes)?;
        if index.version > INDEX_VERSION {
            return Err(IndexError::UnsupportedVersion(index.version));
        }
        Ok(index)
    }

    /// Atomically persist the index. Writes to a temp file then renames,
    /// so a crash mid-write cannot corrupt the existing index.
    pub fn save(&self, app_data_dir: &Path) -> Result<(), IndexError> {
        std::fs::create_dir_all(app_data_dir)?;
        let path = app_data_dir.join(INDEX_FILENAME);
        let tmp = app_data_dir.join(format!("{INDEX_FILENAME}.tmp"));

        let bytes = serde_json::to_vec_pretty(self)?;
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Find a profile by id.
    pub fn get(&self, id: &ProfileId) -> Option<&ProfileSummary> {
        self.profiles.iter().find(|p| &p.id == id)
    }

    /// Find a mutable reference by id.
    pub fn get_mut(&mut self, id: &ProfileId) -> Option<&mut ProfileSummary> {
        self.profiles.iter_mut().find(|p| &p.id == id)
    }

    /// Remove a profile entry. Returns the removed entry if it existed.
    pub fn remove(&mut self, id: &ProfileId) -> Option<ProfileSummary> {
        let pos = self.profiles.iter().position(|p| &p.id == id)?;
        Some(self.profiles.remove(pos))
    }

    /// Insert or update a profile entry.
    pub fn upsert(&mut self, summary: ProfileSummary) {
        if let Some(existing) = self.get_mut(&summary.id) {
            *existing = summary;
        } else {
            self.profiles.push(summary);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::manager::ProfileId;
    use tempfile::TempDir;

    fn sample(id: ProfileId, name: &str) -> ProfileSummary {
        ProfileSummary {
            id,
            display_name: name.to_string(),
            avatar: Avatar::default(),
            color: default_color(),
            created_at: Utc::now(),
            last_unlocked_at: None,
        }
    }

    #[test]
    fn load_missing_index_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let index = ProfileIndex::load(tmp.path()).unwrap();
        assert_eq!(index.version, INDEX_VERSION);
        assert!(index.profiles.is_empty());
    }

    #[test]
    fn save_then_load_round_trip() {
        let tmp = TempDir::new().unwrap();
        let id = ProfileId::new();
        let mut index = ProfileIndex::default();
        index.upsert(sample(id.clone(), "Pratyush"));
        index.save(tmp.path()).unwrap();

        let loaded = ProfileIndex::load(tmp.path()).unwrap();
        assert_eq!(loaded.profiles.len(), 1);
        assert_eq!(loaded.profiles[0].display_name, "Pratyush");
        assert_eq!(loaded.profiles[0].id, id);
    }

    #[test]
    fn upsert_replaces_existing_entry() {
        let id = ProfileId::new();
        let mut index = ProfileIndex::default();
        index.upsert(sample(id.clone(), "Alice"));
        index.upsert(sample(id.clone(), "Alice (renamed)"));
        assert_eq!(index.profiles.len(), 1);
        assert_eq!(index.profiles[0].display_name, "Alice (renamed)");
    }

    #[test]
    fn remove_returns_entry_when_present() {
        let id = ProfileId::new();
        let mut index = ProfileIndex::default();
        index.upsert(sample(id.clone(), "Bob"));
        let removed = index.remove(&id).expect("entry present");
        assert_eq!(removed.display_name, "Bob");
        assert!(index.profiles.is_empty());
    }

    #[test]
    fn rejects_future_version() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(INDEX_FILENAME);
        std::fs::write(&path, r#"{"version":999,"profiles":[]}"#).unwrap();
        let err = ProfileIndex::load(tmp.path()).unwrap_err();
        assert!(matches!(err, IndexError::UnsupportedVersion(999)));
    }

    #[test]
    fn save_is_atomic_via_tempfile() {
        let tmp = TempDir::new().unwrap();
        let index = ProfileIndex::default();
        index.save(tmp.path()).unwrap();
        // The .tmp file must not survive the rename.
        assert!(!tmp.path().join(format!("{INDEX_FILENAME}.tmp")).exists());
        assert!(tmp.path().join(INDEX_FILENAME).exists());
    }
}
