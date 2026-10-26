//! The app's Studio state for the active profile: the Optional AI checks
//! client, assistant grants and their broker, and running Studio jobs.
//!
//! `AppState` owns one. Unlocking a profile activates it; locking or
//! switching invalidates it, which blocks Studio work, drops the client and
//! grants, and aborts every job. The Studio, decisions and assistant broker
//! commands read and update it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

pub struct StudioRuntime {
    /// The Optional AI checks client: profile epoch, settings revision, when
    /// its addresses were pinned, and the client.
    pub decisions: Mutex<Option<(u64, i64, std::time::Instant, alexandria_decisions::Client)>>,
    pub grants: Mutex<alexandria_studio::grants::Grants>,
    pub broker_available: AtomicBool,
    pub broker_gate: tokio::sync::RwLock<()>,
    pub epoch: AtomicU64,
    pub blocked: AtomicBool,
    pub jobs: Mutex<HashMap<String, (i64, tauri::async_runtime::JoinHandle<()>)>>,
}

impl Default for StudioRuntime {
    fn default() -> Self {
        Self {
            decisions: Mutex::new(None),
            grants: Mutex::new(alexandria_studio::grants::Grants::default()),
            broker_available: AtomicBool::new(false),
            broker_gate: tokio::sync::RwLock::new(()),
            epoch: AtomicU64::new(0),
            blocked: AtomicBool::new(true),
            jobs: Mutex::new(HashMap::new()),
        }
    }
}

impl StudioRuntime {
    pub fn activate(&self) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
        self.blocked.store(false, Ordering::SeqCst);
    }
    pub fn invalidate(&self) {
        if let Ok(mut client) = self.decisions.lock() {
            *client = None;
        }
        if let Ok(mut grants) = self.grants.lock() {
            grants.clear();
        }
        self.blocked.store(true, Ordering::SeqCst);
        self.epoch.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut jobs) = self.jobs.lock() {
            for (_, (_, job)) in jobs.drain() {
                job.abort();
            }
        }
    }
}
