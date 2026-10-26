//! Global AI chat: profile-scoped threads, explicit context, and streamed
//! answers from the learner's own model connections.
//!
//! See `docs/ai-chat-spec.md`. Requests go from Rust straight to the configured
//! endpoint; the webview never talks to a provider and never sees a key.

pub mod action;
pub mod artifact;
pub mod bank_guard;
pub mod context;
pub mod goal;
pub mod memory;
pub mod practice;
pub mod run;
pub mod store;
pub mod turn;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::SecondsFormat;
use tokio::sync::oneshot;

/// A moment as stored: RFC 3339 in UTC with millisecond precision, so
/// timestamps compare as text in the order they happened.
pub(crate) fn timestamp(at: chrono::DateTime<chrono::Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub(crate) fn now() -> String {
    timestamp(chrono::Utc::now())
}

/// Most answers generating at once across all threads of a profile.
pub const MAX_CONCURRENT_RUNS: usize = 2;

/// Live generation tasks, so they can be stopped and so a run marked as
/// streaming in the database can be told apart from one whose task is gone.
#[derive(Default)]
pub struct ChatRuntime {
    /// A run stays registered until its task ends; the sender is taken when
    /// it is asked to stop, so a stopping run still counts as live.
    runs: Mutex<HashMap<String, Option<oneshot::Sender<()>>>>,
}

/// Removes its run from the registry when the task ends for any reason,
/// including being dropped by a profile lock.
pub struct RunGuard {
    runtime: Arc<ChatRuntime>,
    run_id: String,
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        if let Ok(mut runs) = self.runtime.runs.lock() {
            runs.remove(&self.run_id);
        }
    }
}

impl ChatRuntime {
    /// Reserves a slot for a run. Fails when the profile is already running
    /// the maximum number of answers.
    pub fn register(
        self: &Arc<Self>,
        run_id: &str,
    ) -> Result<(RunGuard, oneshot::Receiver<()>), &'static str> {
        let mut runs = self.runs.lock().map_err(|_| "chat runtime unavailable")?;
        if runs.len() >= MAX_CONCURRENT_RUNS {
            return Err("too_many_runs: wait for another answer to finish");
        }
        let (tx, rx) = oneshot::channel();
        runs.insert(run_id.to_string(), Some(tx));
        Ok((
            RunGuard {
                runtime: self.clone(),
                run_id: run_id.to_string(),
            },
            rx,
        ))
    }

    pub fn has_capacity(&self) -> bool {
        self.runs
            .lock()
            .map(|r| r.len() < MAX_CONCURRENT_RUNS)
            .unwrap_or(false)
    }

    pub fn is_live(&self, run_id: &str) -> bool {
        self.runs
            .lock()
            .map(|r| r.contains_key(run_id))
            .unwrap_or(false)
    }

    /// Asks a run to stop. Stopping ends the local request; it cannot promise
    /// the provider stopped computing or billing.
    pub fn cancel(&self, run_id: &str) -> bool {
        let sender = self
            .runs
            .lock()
            .ok()
            .and_then(|mut r| r.get_mut(run_id).and_then(Option::take));
        match sender {
            Some(tx) => {
                let _ = tx.send(());
                true
            }
            None => false,
        }
    }

    /// Stops every run: an assessment is starting, or the profile is locking.
    pub fn cancel_all(&self) {
        let senders: Vec<_> = match self.runs.lock() {
            Ok(mut runs) => runs.values_mut().filter_map(Option::take).collect(),
            Err(_) => return,
        };
        for tx in senders {
            let _ = tx.send(());
        }
    }
}

/// Stable error code for a provider failure, stored with the message and
/// translated by the UI. Codes never carry provider text.
pub fn error_code(error: &alexandria_ai::Error) -> String {
    use alexandria_ai::Error as E;
    match error {
        E::Invalid(_) => "invalid".into(),
        E::Unreachable => "unreachable".into(),
        E::Timeout => "timeout".into(),
        E::Interrupted => "interrupted".into(),
        E::Http {
            status: 401 | 403, ..
        } => "auth".into(),
        E::Http {
            status: 429,
            retry_after: Some(s),
        } => format!("rate_limited:{s}"),
        E::Http { status: 429, .. } => "rate_limited".into(),
        E::Http { status: 404, .. } => "model_unavailable".into(),
        E::Http { status, .. } => format!("http:{status}"),
        E::Malformed => "malformed".into(),
        E::TooLarge => "too_large".into(),
        E::Empty => "empty".into(),
        E::Provider => "provider".into(),
        E::Unsupported(_) => "unsupported".into(),
        E::Cancelled => "cancelled".into(),
        E::UsageLimit => "usage_limit".into(),
        E::NotEligible => "plan_ineligible".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_bounds_runs_and_forgets_dropped_tasks() {
        let runtime = Arc::new(ChatRuntime::default());
        let (a, _rx_a) = runtime.register("a").unwrap();
        let (_b, mut rx_b) = runtime.register("b").unwrap();
        assert!(runtime.register("c").is_err());
        assert!(!runtime.has_capacity());
        drop(a);
        assert!(!runtime.is_live("a"));
        assert!(runtime.has_capacity());
        assert!(runtime.cancel("b"));
        assert!(rx_b.try_recv().is_ok());
        assert!(!runtime.cancel("b"));
        // Still live until its task ends, so recovery cannot race the finish.
        assert!(runtime.is_live("b"));
    }

    #[test]
    fn cancel_all_signals_every_run() {
        let runtime = Arc::new(ChatRuntime::default());
        let (_a, mut rx_a) = runtime.register("a").unwrap();
        let (_b, mut rx_b) = runtime.register("b").unwrap();
        runtime.cancel_all();
        assert!(rx_a.try_recv().is_ok() && rx_b.try_recv().is_ok());
    }

    #[test]
    fn error_codes_carry_no_provider_text() {
        let code = error_code(&alexandria_ai::Error::Http {
            status: 429,
            retry_after: Some(7),
        });
        assert_eq!(code, "rate_limited:7");
        assert_eq!(
            error_code(&alexandria_ai::Error::Invalid("secret detail".into())),
            "invalid"
        );
    }
}
