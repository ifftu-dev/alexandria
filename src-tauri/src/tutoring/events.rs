//! Where a call's events go: the webview in the app, a recorder in tests.

use serde::Serialize;
use serde_json::Value;

/// A sink for the `tutoring:*` events a call raises.
pub trait CallEvents: Send + Sync + 'static {
    fn emit(&self, event: &str, payload: Value);
}

/// Serialize and emit; a payload that cannot serialize is a bug, logged once.
pub(super) fn emit<T: Serialize>(sink: &dyn CallEvents, event: &str, payload: T) {
    match serde_json::to_value(payload) {
        Ok(value) => sink.emit(event, value),
        Err(error) => log::error!("tutoring: event {event} failed to serialize: {error}"),
    }
}

impl<E: CallEvents> CallEvents for std::sync::Arc<E> {
    fn emit(&self, event: &str, payload: Value) {
        (**self).emit(event, payload)
    }
}

impl CallEvents for tauri::AppHandle {
    fn emit(&self, event: &str, payload: Value) {
        let _ = tauri::Emitter::emit(self, event, payload);
    }
}

/// Keeps every event, and wakes anyone waiting for one.
#[cfg(test)]
pub struct RecordedEvents {
    log: std::sync::Mutex<Vec<(String, Value)>>,
    changed: tokio::sync::Notify,
}

#[cfg(test)]
impl Default for RecordedEvents {
    fn default() -> Self {
        Self {
            log: Default::default(),
            changed: tokio::sync::Notify::new(),
        }
    }
}

#[cfg(test)]
impl RecordedEvents {
    /// Every payload emitted under `event`, oldest first.
    pub fn payloads(&self, event: &str) -> Vec<Value> {
        self.log
            .lock()
            .expect("events poisoned")
            .iter()
            .filter(|(name, _)| name == event)
            .map(|(_, payload)| payload.clone())
            .collect()
    }

    /// The first payload under `event` that satisfies `matches`, waiting up
    /// to `timeout` for it to arrive.
    pub async fn wait_for(
        &self,
        event: &str,
        timeout: std::time::Duration,
        mut matches: impl FnMut(&Value) -> bool,
    ) -> Option<Value> {
        tokio::time::timeout(timeout, async {
            loop {
                // Register for the wake-up before looking, so an event that
                // lands between the look and the wait is not missed.
                let notified = self.changed.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if let Some(found) = self.payloads(event).into_iter().find(|p| matches(p)) {
                    return found;
                }
                notified.await;
            }
        })
        .await
        .ok()
    }
}

#[cfg(test)]
impl CallEvents for RecordedEvents {
    fn emit(&self, event: &str, payload: Value) {
        self.log
            .lock()
            .expect("events poisoned")
            .push((event.to_string(), payload));
        self.changed.notify_waiters();
    }
}
