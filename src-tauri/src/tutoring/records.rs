//! Where a call's rows live.
//!
//! The call manager records the sessions it creates and joins and checks a
//! room's kind before joining, but it is a per-device singleton and the
//! database belongs to the unlocked profile. This seam hands it the profile's
//! database for the length of one command; tests hand it an in-memory one.

use std::future::Future;

use crate::db::executor::{DatabaseExecutor, DatabaseWorkload};
use crate::db::Database;
use crate::profile::scope::ProfileLease;

/// Runs database work for the call manager on the current profile.
pub trait SessionRecords: Send + Sync {
    fn run<T: Send + 'static>(
        &self,
        label: &'static str,
        work: impl FnOnce(&Database) -> Result<T, String> + Send + 'static,
    ) -> impl Future<Output = Result<T, String>> + Send;
}

/// The unlocked profile's database, through the executor and lease a
/// command holds.
pub struct ProfileRecords {
    pub(crate) executor: DatabaseExecutor,
    pub(crate) lease: ProfileLease,
}

impl SessionRecords for ProfileRecords {
    async fn run<T: Send + 'static>(
        &self,
        label: &'static str,
        work: impl FnOnce(&Database) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        self.executor
            .execute(DatabaseWorkload::Learner, self.lease.clone(), label, work)
            .await
    }
}

/// An in-memory database for tests; several managers may share one, as two
/// calls on one device would.
#[cfg(test)]
#[derive(Clone)]
pub struct MemoryRecords(pub std::sync::Arc<std::sync::Mutex<Database>>);

#[cfg(test)]
impl MemoryRecords {
    pub fn migrated() -> Self {
        let db = Database::open_in_memory().expect("in-memory database");
        db.run_migrations().expect("migrations");
        Self(std::sync::Arc::new(std::sync::Mutex::new(db)))
    }

    pub fn with<T>(&self, read: impl FnOnce(&Database) -> T) -> T {
        read(&self.0.lock().expect("records poisoned"))
    }
}

#[cfg(test)]
impl SessionRecords for MemoryRecords {
    async fn run<T: Send + 'static>(
        &self,
        _label: &'static str,
        work: impl FnOnce(&Database) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        self.with(work)
    }
}
