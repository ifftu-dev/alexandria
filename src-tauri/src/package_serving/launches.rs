//! Open launches: which path prefix serves which package's files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rand::RngCore;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

use crate::domain::package_launch::OpenedPackageLaunch;

/// One open launch.
#[derive(Debug, Clone)]
pub(super) struct OpenLaunch {
    pub package_hash: String,
    /// The package's extracted files, canonical.
    pub root: PathBuf,
}

/// The launches open in this profile. Held in the app state, read by the
/// scheme handler on every request, and emptied when the profile stops.
#[derive(Debug, Default)]
pub struct Launches {
    open: Mutex<HashMap<String, OpenLaunch>>,
    extracting: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

impl Launches {
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds a new prefix to a package whose files are at `root`. The prefix
    /// is 128 random bits; nothing else can name the launch.
    pub fn open(&self, package_hash: &str, root: &Path) -> std::io::Result<OpenedPackageLaunch> {
        let root = std::fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "package files are not a directory",
            ));
        }
        let mut bytes = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        let prefix: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        self.open
            .lock()
            .expect("launch registry lock poisoned")
            .insert(
                prefix.clone(),
                OpenLaunch {
                    package_hash: package_hash.to_string(),
                    root,
                },
            );
        Ok(OpenedPackageLaunch {
            base_url: format!("{}/{prefix}/", super::origin()),
            prefix,
            package_hash: package_hash.to_string(),
        })
    }

    /// Retires a launch's prefix. Returns whether it was open.
    pub fn close(&self, prefix: &str) -> bool {
        self.open
            .lock()
            .expect("launch registry lock poisoned")
            .remove(prefix)
            .is_some()
    }

    /// Retires every launch, as the profile stops.
    pub fn close_all(&self) {
        self.open
            .lock()
            .expect("launch registry lock poisoned")
            .clear();
        self.extracting
            .lock()
            .expect("launch registry lock poisoned")
            .clear();
    }

    /// Whether any open launch serves this package. Dropping a package's
    /// extracted files waits until this is false.
    pub fn is_package_open(&self, package_hash: &str) -> bool {
        self.open
            .lock()
            .expect("launch registry lock poisoned")
            .values()
            .any(|launch| launch.package_hash == package_hash)
    }

    pub(super) fn get(&self, prefix: &str) -> Option<OpenLaunch> {
        self.open
            .lock()
            .expect("launch registry lock poisoned")
            .get(prefix)
            .cloned()
    }

    /// Held while a launch makes sure its package is extracted, so two
    /// launches of one package never extract it at the same time.
    pub(super) async fn lock_extraction(&self, package_hash: &str) -> OwnedMutexGuard<()> {
        let lock = self
            .extracting
            .lock()
            .expect("launch registry lock poisoned")
            .entry(package_hash.to_string())
            .or_default()
            .clone();
        lock.lock_owned().await
    }
}
