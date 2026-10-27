//! Fan-out from the native platforms to the JS listeners registered with
//! `register_listener`.
//!
//! Tauri's `Plugin::trigger` reaches listeners held by the plugin object that
//! Tauri instantiated through its plugin dispatch. This plugin does not use
//! that dispatch — the iOS commands go through direct `@_cdecl` FFI (see
//! `commands.rs`), and the Android commands run in a Kotlin plugin that Rust
//! forwards to — so `trigger` on either side has no listeners to reach.
//!
//! Instead the platforms call into [`emit`] (Android through the JNI symbol in
//! `commands.rs`, iOS through the `@_cdecl` function in
//! `ios/Sources/MobilePushPlugin.swift`), which sends the payload to every
//! `Channel` registered against that event name.
//!
//! A listener arrives as the `Channel` the JS side created for
//! `addPluginListener`: Tauri deserializes it from the `__CHANNEL__:<id>`
//! string and [`emit`] sends the platform's JSON payload through it, exactly as
//! `Plugin::trigger` would have.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use parking_lot::Mutex;
use serde_json::Value;
use tauri::ipc::Channel;

/// Alexandria addition: listener admission and sticky handoff share one lock.
/// Platforms call from plain JNI/FFI functions, outside the plugin instance.
/// All callbacks and channel sends run after this lock has been released.
#[derive(Default)]
struct Registry {
    channels: HashMap<String, Vec<Channel<Value>>>,
    rust_listeners: HashMap<String, Vec<RustListener>>,
    pending: HashMap<String, Value>,
}

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(|| Mutex::new(Registry::default()));

/// A Rust-side listener. Alexandria addition: the host app handles push
/// events in Rust (recording them, showing a local notification, routing a
/// tap) rather than in the webview, so the platforms need a Rust audience as
/// well as the JS channels above.
pub type RustListener = Arc<dyn Fn(&Value) + Send + Sync + 'static>;

/// Registers a listener for `event`, delivering a sticky payload if one is
/// waiting.
pub fn register(event: String, channel: Channel<Value>) {
    log::info!(
        "[mobile-push] listener registered for event `{event}` (channel {})",
        channel.id()
    );
    let pending = {
        let mut registry = REGISTRY.lock();
        registry
            .channels
            .entry(event.clone())
            .or_default()
            .push(channel.clone());
        registry.pending.remove(&event)
    };
    if let Some(pending) = pending {
        log::debug!("[mobile-push] replaying the pending `{event}` payload");
        if let Err(e) = channel.send(pending) {
            log::warn!("[mobile-push] could not replay `{event}`: {e}");
        }
    }
}

/// Registers a Rust listener for `event`. Alexandria addition.
///
/// Takes a sticky payload that is waiting, for the same reason [`register`]
/// does: a tap that launched the app is reported before the host's setup hook
/// has run, and the subscriber that setup installs is the one meant to act
/// on it. Rust listeners are never removed; the host installs them once, for
/// the life of the process.
pub fn subscribe<F>(event: &str, listener: F)
where
    F: Fn(&Value) + Send + Sync + 'static,
{
    let listener: RustListener = Arc::new(listener);
    let pending = {
        let mut registry = REGISTRY.lock();
        registry
            .rust_listeners
            .entry(event.to_string())
            .or_default()
            .push(listener.clone());
        registry.pending.remove(event)
    };
    if let Some(pending) = pending {
        log::debug!("[mobile-push] replaying the pending `{event}` payload to a Rust listener");
        listener(&pending);
    }
}

/// Drops the listener for `event` with the given channel id.
///
/// `PluginListener::unregister` calls the `remove_listener` command with the
/// channel id it was given at registration.
pub fn remove(event: &str, channel_id: u32) {
    let mut registry = REGISTRY.lock();
    let map = &mut registry.channels;
    let Some(channels) = map.get_mut(event) else {
        return;
    };
    channels.retain(|c| c.id() != channel_id);
    if channels.is_empty() {
        map.remove(event);
    }
}

/// Drops every listener and pending payload.
///
/// Listeners outlive the webview that registered them, so they are cleared
/// when the app exits rather than left behind.
pub fn clear() {
    *REGISTRY.lock() = Registry::default();
}

/// Delivers a platform event to every listener registered for `event`.
///
/// `payload` is a JSON document produced by the platform side. A payload that
/// does not parse is logged and dropped: a malformed notification must not
/// take down the thread that produced it. A listener whose channel fails to
/// send (its webview is gone) is dropped, so a reload does not accumulate dead
/// channels.
///
/// With `sticky`, a payload that arrives before any listener exists is kept
/// and replayed to the first one to register. That is for events which are
/// about a single past occurrence the user acted on — a notification tap —
/// where a late listener still needs it. Events that arrive continuously
/// (a received notification, a rotated token) are not sticky: replaying a stale
/// one on the next launch would act on a moment that has passed.
///
/// Only the mobile platforms produce events, so this is unused (and not
/// compiled) on the desktop targets.
#[cfg(any(mobile, test))]
pub fn emit(event: &str, payload: &str, sticky: bool) {
    let data: Value = match serde_json::from_str(payload) {
        Ok(data) => data,
        Err(e) => {
            log::error!("[mobile-push] dropping unparseable `{event}` payload: {e}");
            return;
        }
    };

    // Choose delivery or buffering atomically with listener admission. A
    // subscriber either takes this pending payload or appears in this snapshot.
    let (rust_listeners, channels) = {
        let mut registry = REGISTRY.lock();
        let rust_listeners = registry
            .rust_listeners
            .get(event)
            .cloned()
            .unwrap_or_default();
        let channels = registry.channels.get(event).cloned().unwrap_or_default();
        if rust_listeners.is_empty() && channels.is_empty() {
            if sticky {
                log::debug!("[mobile-push] holding `{event}` until a listener registers");
                registry.pending.insert(event.to_string(), data);
            }
            return;
        }
        (rust_listeners, channels)
    };

    // Callbacks may subscribe or emit again; channel sends may need the main
    // thread. Neither can run under the registry lock.
    for listener in &rust_listeners {
        listener(&data);
    }

    let mut delivered = 0usize;
    let mut dead = Vec::new();
    for channel in &channels {
        match channel.send(data.clone()) {
            Ok(()) => delivered += 1,
            Err(e) => {
                log::debug!(
                    "[mobile-push] dropping `{event}` listener {}: {e}",
                    channel.id()
                );
                dead.push(channel.id());
            }
        }
    }

    if !dead.is_empty() {
        let mut registry = REGISTRY.lock();
        let map = &mut registry.channels;
        if let Some(channels) = map.get_mut(event) {
            channels.retain(|c| !dead.contains(&c.id()));
            if channels.is_empty() {
                map.remove(event);
            }
        }
    }

    log::debug!("[mobile-push] `{event}` delivered to {delivered} listener(s)");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// The registry is process-global, so tests that share it must not run
    /// concurrently: one test's `clear` would otherwise wipe another's
    /// listeners. Each test holds this for its duration.
    fn serial() -> parking_lot::MutexGuard<'static, ()> {
        static LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
        let guard = LOCK.lock();
        clear();
        guard
    }

    /// A channel that records what it receives, plus the recording buffer.
    fn recorder() -> (Channel<Value>, Arc<Mutex<Vec<Value>>>) {
        let received: Arc<Mutex<Vec<Value>>> = Arc::default();
        let sink = received.clone();
        let channel = Channel::<Value>::new(move |body| {
            sink.lock().push(body.deserialize::<Value>().unwrap());
            Ok(())
        });
        (channel, received)
    }

    /// A channel whose send always fails, as one bound to a closed webview does.
    fn broken() -> Channel<Value> {
        Channel::<Value>::new(|_| Err(tauri::Error::WebviewNotFound))
    }

    #[test]
    fn delivers_only_to_listeners_of_that_event() {
        let _serial = serial();
        let (received, got) = recorder();
        let (tapped, tap_got) = recorder();
        register("notification-received".into(), received);
        register("notification-tapped".into(), tapped);

        emit("notification-received", r#"{"title":"hi"}"#, false);

        assert_eq!(*got.lock(), vec![serde_json::json!({"title": "hi"})]);
        assert!(tap_got.lock().is_empty());
    }

    #[test]
    fn delivers_to_every_listener_of_an_event() {
        let _serial = serial();
        let (first, first_got) = recorder();
        let (second, second_got) = recorder();
        register("token-received".into(), first);
        register("token-received".into(), second);

        emit("token-received", r#"{"token":"abc"}"#, false);

        assert_eq!(first_got.lock().len(), 1);
        assert_eq!(second_got.lock().len(), 1);
    }

    #[test]
    fn an_event_without_listeners_is_not_an_error() {
        let _serial = serial();
        emit("notification-tapped", r#"{"data":{}}"#, false);
    }

    #[test]
    fn malformed_payload_is_dropped_without_delivering() {
        let _serial = serial();
        let (channel, got) = recorder();
        let id = channel.id();
        register("notification-received".into(), channel);

        emit("notification-received", "not json", false);

        assert!(got.lock().is_empty());
        // Still registered: a bad payload says nothing about the listener.
        assert_eq!(
            REGISTRY
                .lock()
                .channels
                .get("notification-received")
                .unwrap()
                .len(),
            1
        );
        remove("notification-received", id);
    }

    #[test]
    fn remove_drops_only_that_channel() {
        let _serial = serial();
        let (first, first_got) = recorder();
        let (second, second_got) = recorder();
        let first_id = first.id();
        register("notification-received".into(), first);
        register("notification-received".into(), second);

        remove("notification-received", first_id);
        emit("notification-received", r#"{"n":1}"#, false);

        assert!(first_got.lock().is_empty());
        assert_eq!(second_got.lock().len(), 1);
    }

    #[test]
    fn a_listener_that_cannot_send_is_dropped() {
        let _serial = serial();
        register("notification-tapped".into(), broken());
        let (live, got) = recorder();
        register("notification-tapped".into(), live);

        emit("notification-tapped", r#"{"n":1}"#, false);
        assert_eq!(got.lock().len(), 1);
        // The dead channel is gone, the live one remains.
        assert_eq!(
            REGISTRY
                .lock()
                .channels
                .get("notification-tapped")
                .unwrap()
                .len(),
            1
        );

        emit("notification-tapped", r#"{"n":2}"#, false);
        assert_eq!(got.lock().len(), 2);
    }

    #[test]
    fn a_sticky_event_arriving_early_reaches_the_first_listener() {
        let _serial = serial();
        // The cold-start ordering: the platform reports the tap before any
        // JavaScript has run, so registration is what delivers it.
        emit("notification-tapped", r#"{"data":{"roomId":"r"}}"#, true);
        assert!(REGISTRY.lock().pending.contains_key("notification-tapped"));

        let (channel, got) = recorder();
        register("notification-tapped".into(), channel);

        assert_eq!(
            *got.lock(),
            vec![serde_json::json!({"data": {"roomId": "r"}})]
        );
        // Delivered once, not held for every later listener.
        assert!(!REGISTRY.lock().pending.contains_key("notification-tapped"));
    }

    #[test]
    fn a_rust_subscriber_receives_events_and_takes_a_pending_tap() {
        let _serial = serial();
        emit("notification-tapped", r#"{"data":{"route":"/x"}}"#, true);
        let got: Arc<Mutex<Vec<Value>>> = Arc::default();
        let sink = got.clone();
        subscribe("notification-tapped", move |payload| {
            sink.lock().push(payload.clone())
        });
        // The sticky tap reached the subscriber at registration and is gone.
        assert_eq!(got.lock().len(), 1);
        assert!(!REGISTRY.lock().pending.contains_key("notification-tapped"));

        emit("notification-tapped", r#"{"data":{"route":"/y"}}"#, true);
        assert_eq!(got.lock().len(), 2);
        // Delivered, so not held for a JS listener that never comes.
        assert!(!REGISTRY.lock().pending.contains_key("notification-tapped"));
    }

    #[test]
    fn a_tap_racing_cold_replay_reaches_the_subscriber_once_without_blocking() {
        let _serial = serial();
        emit("notification-tapped", r#"{"tap":"cold"}"#, true);
        let got: Arc<Mutex<Vec<Value>>> = Arc::default();
        let sink = got.clone();
        let start = Arc::new(std::sync::Barrier::new(2));
        let native_start = start.clone();
        let (finished, completion) = std::sync::mpsc::channel();
        let completion = Mutex::new(completion);
        let replay_completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let replay_result = replay_completed.clone();
        let native = std::thread::spawn(move || {
            native_start.wait();
            emit("notification-tapped", r#"{"tap":"racing"}"#, true);
            finished.send(()).unwrap();
        });

        subscribe("notification-tapped", move |payload| {
            sink.lock().push(payload.clone());
            if payload["tap"] == "cold" {
                // Keep the initial replay callback active until another native
                // tap is emitted. Registration must already be effective, and
                // neither replay nor live delivery may hold the registry lock.
                start.wait();
                replay_result.store(
                    completion
                        .lock()
                        .recv_timeout(std::time::Duration::from_secs(5))
                        .is_ok(),
                    std::sync::atomic::Ordering::SeqCst,
                );
            }
        });
        native.join().unwrap();

        // Public subscription observes any stranded tap without inspecting
        // the registry. A second subscriber must not replay either tap.
        let late: Arc<Mutex<Vec<Value>>> = Arc::default();
        let late_sink = late.clone();
        subscribe("notification-tapped", move |payload| {
            late_sink.lock().push(payload.clone());
        });
        assert_eq!(
            *got.lock(),
            vec![
                serde_json::json!({"tap": "cold"}),
                serde_json::json!({"tap": "racing"}),
            ]
        );
        assert!(
            late.lock().is_empty(),
            "a tap was stranded for a later listener"
        );
        assert!(
            replay_completed.load(std::sync::atomic::Ordering::SeqCst),
            "native emission blocked behind the replay callback"
        );
    }

    #[test]
    fn a_non_sticky_event_arriving_early_is_not_replayed() {
        let _serial = serial();
        emit("token-received", r#"{"token":"abc"}"#, false);

        let (channel, got) = recorder();
        register("token-received".into(), channel);

        assert!(got.lock().is_empty());
    }
}
