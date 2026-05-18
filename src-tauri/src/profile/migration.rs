//! Auto-migration from the pre-multi-user single-vault layout into the
//! per-profile layout. Runs at most once per device: on the first launch
//! after upgrade, if legacy paths exist and `profiles/` does not, the
//! legacy data is moved (atomically per-entry) into `profiles/<new-uuid>/`
//! and registered with the [`ProfileManager`].
//!
//! Layout transformation:
//!
//! ```text
//! BEFORE                                AFTER
//! <app_data>/                           <app_data>/
//!   stronghold/        ──▶                profiles/<uuid>/vault/
//!   vault/             ──▶                profiles/<uuid>/vault/
//!   alexandria.db      ──▶                profiles/<uuid>/alexandria.db
//!   alexandria.db-wal  ──▶                profiles/<uuid>/alexandria.db-wal
//!   alexandria.db-shm  ──▶                profiles/<uuid>/alexandria.db-shm
//!   iroh/              ──▶                profiles/<uuid>/iroh/
//!   plugins/           ──▶                profiles/<uuid>/plugins/
//!   videocache/        ──▶                profiles/<uuid>/videocache/
//! ```
//!
//! Each move is `std::fs::rename`, which is atomic on the same filesystem.
//! If any move fails partway, [`MigrationReport::Failed`] is returned with
//! the entries that did succeed; the caller may choose to roll back by
//! moving them back, or leave them in place for manual inspection.

use std::path::{Path, PathBuf};

use super::index::Avatar;
use super::manager::{ProfileError, ProfileId, ProfileManager, ProfilePaths, PROFILES_DIRNAME};

/// Default display name assigned to migrated single-vault profiles.
pub const MIGRATED_DEFAULT_NAME: &str = "My Profile";

/// Set of legacy paths the migrator looks for. Any subset may exist.
#[derive(Debug, Clone)]
pub struct LegacyLayout {
    pub app_data_dir: PathBuf,
}

impl LegacyLayout {
    pub fn at(app_data_dir: &Path) -> Self {
        Self {
            app_data_dir: app_data_dir.to_path_buf(),
        }
    }

    /// Desktop vault dir.
    pub fn stronghold_dir(&self) -> PathBuf {
        self.app_data_dir.join("stronghold")
    }

    /// Mobile vault dir.
    pub fn vault_dir(&self) -> PathBuf {
        self.app_data_dir.join("vault")
    }

    pub fn db_path(&self) -> PathBuf {
        self.app_data_dir.join("alexandria.db")
    }

    pub fn db_wal(&self) -> PathBuf {
        self.app_data_dir.join("alexandria.db-wal")
    }

    pub fn db_shm(&self) -> PathBuf {
        self.app_data_dir.join("alexandria.db-shm")
    }

    pub fn iroh_dir(&self) -> PathBuf {
        self.app_data_dir.join("iroh")
    }

    pub fn plugins_dir(&self) -> PathBuf {
        self.app_data_dir.join("plugins")
    }

    pub fn video_cache_dir(&self) -> PathBuf {
        self.app_data_dir.join("videocache")
    }

    pub fn profiles_root(&self) -> PathBuf {
        self.app_data_dir.join(PROFILES_DIRNAME)
    }

    /// Returns `true` if there is anything worth migrating — i.e. at
    /// least the legacy DB or a vault dir exists.
    pub fn has_legacy_data(&self) -> bool {
        self.db_path().exists() || self.stronghold_dir().exists() || self.vault_dir().exists()
    }

    /// Returns `true` if the new layout has any profiles. Migration is
    /// skipped when this is true to avoid overwriting an established
    /// per-profile install.
    pub fn has_profiles_dir(&self) -> bool {
        let root = self.profiles_root();
        // Treat an empty profiles/ dir as "no profiles" — the manager
        // creates the root on open and we don't want that to block the
        // first-launch migration.
        if !root.exists() {
            return false;
        }
        std::fs::read_dir(&root)
            .map(|mut iter| iter.next().is_some())
            .unwrap_or(false)
    }
}

/// Outcome of running the migrator.
#[derive(Debug)]
pub enum MigrationReport {
    /// No legacy data was found; nothing to do.
    NoLegacyData,
    /// `profiles/` already exists with at least one entry; migration skipped.
    AlreadyMigrated,
    /// Migration completed; returns the freshly-created profile's paths.
    Migrated { id: ProfileId, paths: ProfilePaths },
    /// Migration failed partway. `moved` lists entries that succeeded
    /// before the failure; the caller may roll them back if desired.
    Failed {
        error: MigrationError,
        moved: Vec<(PathBuf, PathBuf)>,
    },
}

#[derive(thiserror::Error, Debug)]
pub enum MigrationError {
    #[error("failed to move {from} -> {to}: {source}")]
    Rename {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("profile manager error: {0}")]
    Profile(#[from] ProfileError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Run the migrator. Idempotent: if there is no legacy data or the new
/// layout is already populated, this returns immediately without changes.
pub fn migrate_if_needed(
    manager: &ProfileManager,
    layout: &LegacyLayout,
) -> Result<MigrationReport, ProfileError> {
    if layout.has_profiles_dir() {
        log::debug!("migration skipped: profiles dir already populated");
        return Ok(MigrationReport::AlreadyMigrated);
    }
    if !layout.has_legacy_data() {
        log::debug!("migration skipped: no legacy data");
        return Ok(MigrationReport::NoLegacyData);
    }

    let id = ProfileId::new();
    let target = ProfilePaths::for_id(&layout.app_data_dir, &id);

    // We do *not* call ensure_dirs() here — std::fs::rename of an
    // existing legacy dir into the target path will create it. We only
    // create the top-level profiles/<id>/ wrapper.
    std::fs::create_dir_all(&target.root).map_err(ProfileError::Io)?;

    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();

    // List of (source, destination, optional). The DB-related sidecar
    // files (-wal, -shm) are optional — SQLCipher may not have created
    // them yet. Vault dirs are platform-conditional and only one will
    // exist on any given install.
    let plan: Vec<(PathBuf, PathBuf, bool)> = vec![
        (layout.stronghold_dir(), target.vault_dir.clone(), true),
        (layout.vault_dir(), target.vault_dir.clone(), true),
        (layout.db_path(), target.db_path.clone(), true),
        (layout.db_wal(), target.root.join("alexandria.db-wal"), true),
        (layout.db_shm(), target.root.join("alexandria.db-shm"), true),
        (layout.iroh_dir(), target.iroh_dir.clone(), true),
        (layout.plugins_dir(), target.plugins_dir.clone(), true),
        (
            layout.video_cache_dir(),
            target.video_cache_dir.clone(),
            true,
        ),
    ];

    for (from, to, optional) in plan {
        if !from.exists() {
            if optional {
                continue;
            }
            return Ok(MigrationReport::Failed {
                error: MigrationError::Rename {
                    from: from.clone(),
                    to: to.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "required path missing",
                    ),
                },
                moved,
            });
        }

        if let Err(source) = std::fs::rename(&from, &to) {
            log::error!(
                "migration failed at {} -> {}: {source}",
                from.display(),
                to.display()
            );
            return Ok(MigrationReport::Failed {
                error: MigrationError::Rename { from, to, source },
                moved,
            });
        }
        log::info!("migrated {} -> {}", from.display(), to.display());
        moved.push((from, to));
    }

    // Make sure expected subdirs exist even if the legacy install did
    // not have them (e.g. plugins/ never created because no plugins
    // were installed).
    target.ensure_dirs().map_err(ProfileError::Io)?;

    let paths = manager.adopt_existing(id.clone(), MIGRATED_DEFAULT_NAME, Avatar::default())?;
    Ok(MigrationReport::Migrated { id, paths })
}

/// Best-effort rollback of a `Failed` report. Moves each `(target, source)`
/// pair back to its origin. Returns the first error encountered, if any.
pub fn rollback(moved: &[(PathBuf, PathBuf)]) -> std::io::Result<()> {
    for (from, to) in moved.iter().rev() {
        if to.exists() && !from.exists() {
            std::fs::rename(to, from)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, b"x").unwrap();
    }

    fn make_legacy(app_dir: &Path) {
        // Simulate desktop legacy install with database + vault + iroh.
        touch(&app_dir.join("alexandria.db"));
        touch(&app_dir.join("alexandria.db-wal"));
        std::fs::create_dir_all(app_dir.join("stronghold")).unwrap();
        touch(&app_dir.join("stronghold/alexandria.stronghold"));
        std::fs::create_dir_all(app_dir.join("iroh")).unwrap();
        touch(&app_dir.join("iroh/blob.bin"));
        std::fs::create_dir_all(app_dir.join("plugins")).unwrap();
        std::fs::create_dir_all(app_dir.join("videocache")).unwrap();
    }

    #[test]
    fn no_legacy_data_reports_skip() {
        let tmp = TempDir::new().unwrap();
        let mgr = ProfileManager::open(tmp.path()).unwrap();
        let layout = LegacyLayout::at(tmp.path());
        let report = migrate_if_needed(&mgr, &layout).unwrap();
        assert!(matches!(report, MigrationReport::NoLegacyData));
    }

    #[test]
    fn full_legacy_install_migrates_successfully() {
        let tmp = TempDir::new().unwrap();
        make_legacy(tmp.path());

        let mgr = ProfileManager::open(tmp.path()).unwrap();
        let layout = LegacyLayout::at(tmp.path());
        let report = migrate_if_needed(&mgr, &layout).unwrap();

        let paths = match report {
            MigrationReport::Migrated { paths, .. } => paths,
            other => panic!("expected Migrated, got {other:?}"),
        };

        assert!(paths.db_path.exists());
        assert!(paths.root.join("alexandria.db-wal").exists());
        assert!(paths.vault_dir.exists());
        assert!(paths.vault_dir.join("alexandria.stronghold").exists());
        assert!(paths.iroh_dir.exists());

        // Legacy paths must be gone.
        assert!(!tmp.path().join("alexandria.db").exists());
        assert!(!tmp.path().join("stronghold").exists());

        // Manager picked up the new entry.
        assert_eq!(mgr.count(), 1);
        let summary = &mgr.list()[0];
        assert_eq!(summary.display_name, MIGRATED_DEFAULT_NAME);
    }

    #[test]
    fn second_run_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        make_legacy(tmp.path());

        let mgr = ProfileManager::open(tmp.path()).unwrap();
        let layout = LegacyLayout::at(tmp.path());
        let _ = migrate_if_needed(&mgr, &layout).unwrap();
        let second = migrate_if_needed(&mgr, &layout).unwrap();
        assert!(matches!(second, MigrationReport::AlreadyMigrated));
    }

    #[test]
    fn populated_profiles_dir_blocks_migration() {
        let tmp = TempDir::new().unwrap();
        make_legacy(tmp.path());
        // Pre-populate profiles/ as if multi-user was already active.
        let fake_id = uuid::Uuid::new_v4().to_string();
        std::fs::create_dir_all(tmp.path().join("profiles").join(&fake_id)).unwrap();

        let mgr = ProfileManager::open(tmp.path()).unwrap();
        let layout = LegacyLayout::at(tmp.path());
        let report = migrate_if_needed(&mgr, &layout).unwrap();
        assert!(matches!(report, MigrationReport::AlreadyMigrated));

        // Legacy DB must remain untouched.
        assert!(tmp.path().join("alexandria.db").exists());
    }

    #[test]
    fn partial_legacy_install_still_migrates() {
        // Only a DB and a vault — no iroh/plugins/videocache present yet.
        let tmp = TempDir::new().unwrap();
        touch(&tmp.path().join("alexandria.db"));
        std::fs::create_dir_all(tmp.path().join("stronghold")).unwrap();
        touch(&tmp.path().join("stronghold/alexandria.stronghold"));

        let mgr = ProfileManager::open(tmp.path()).unwrap();
        let layout = LegacyLayout::at(tmp.path());
        let report = migrate_if_needed(&mgr, &layout).unwrap();
        let paths = match report {
            MigrationReport::Migrated { paths, .. } => paths,
            other => panic!("expected Migrated, got {other:?}"),
        };
        assert!(paths.db_path.exists());
        assert!(paths.vault_dir.exists());
        // Ensure ensure_dirs() filled in the missing subdirectories.
        assert!(paths.iroh_dir.exists());
        assert!(paths.plugins_dir.exists());
        assert!(paths.video_cache_dir.exists());
    }
}
