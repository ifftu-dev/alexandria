// SPDX-License-Identifier: AGPL-3.0-or-later
//! Remote push: registering this device with Alexandria Cloud, and acting on
//! what the platform delivers.
//!
//! # Why Cloud, and why only mobile
//!
//! A phone cannot keep a peer connection open while asleep, and the only way
//! to wake it is the platform's push service. Alexandria Cloud is the party
//! that already knows the events worth waking a phone for — it holds the
//! interview invitations, offers and requests — and already authenticates the
//! learner by DID proof, so the token goes there. The desktop app is a
//! long-running node and gets the same notifications from its own
//! background pass, so it registers nothing.
//!
//! # What Cloud learns
//!
//! A DID it already serves, a platform, a token the platform issued, and a
//! random device id. The token is what Firebase Cloud Messaging can be asked
//! to wake — Google delivers to Android itself and relays to Apple for iOS —
//! and it is not tied to anything else about the person. A push payload
//! carries a kind, an id and a route — never a name — so Google and Apple
//! learn only that an Alexandria event of some kind reached this device.
//!
//! # Several profiles on one phone
//!
//! Each profile registers the same platform token under its own DID and its
//! own device id, so Cloud reaches whichever profile the event is for; the
//! payload names no profile. A tap opens the app, the app asks for a profile
//! if none is unlocked, and the route is replayed once one is.

use ed25519_dalek::SigningKey;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_notification::{NotificationExt, PermissionState};

use super::{cloud_copy, kind, NewNotification};
use crate::commands::holder_pull::{cloud_origin, is_loopback, nonce, proof};
use crate::db::executor::DatabaseWorkload;
use crate::settings::{registry::keys, SettingsStore};
use crate::AppState;

/// Where this device stands with push, as the settings page shows it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PushStatus {
    /// This platform can receive remote pushes at all.
    pub supported: bool,
    /// A registration with the configured Cloud is on record for this profile.
    pub registered: bool,
    pub platform: Option<String>,
    pub cloud_origin: Option<String>,
    /// Why the last attempt did not register, if it did not.
    pub last_error: Option<String>,
}

/// The last failure, kept so the settings page can say why rather than
/// just "not registered".
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn set_last_error(error: Option<String>) {
    if let Ok(mut slot) = LAST_ERROR.lock() {
        *slot = error;
    }
}

fn last_error() -> Option<String> {
    LAST_ERROR.lock().ok().and_then(|slot| slot.clone())
}

/// The push service this build's token belongs to: Firebase Cloud Messaging
/// on both phones. On iOS the plugin hands Apple's device token to the
/// Firebase SDK and reports the FCM token it gets back, so Cloud addresses
/// an iPhone and an Android phone the same way. Desktop has none.
pub fn platform() -> Option<&'static str> {
    if cfg!(any(target_os = "ios", target_os = "android")) {
        Some("fcm")
    } else {
        None
    }
}

/// This profile's registration with one Cloud origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub cloud_origin: String,
    pub device_id: String,
    pub platform: String,
    pub token: String,
    pub registered_at: String,
}

pub fn load_registration(conn: &Connection, origin: &str) -> Result<Option<Registration>, String> {
    conn.query_row(
        "SELECT cloud_origin, device_id, platform, token, registered_at \
         FROM push_registrations WHERE cloud_origin = ?1",
        params![origin],
        |row| {
            Ok(Registration {
                cloud_origin: row.get(0)?,
                device_id: row.get(1)?,
                platform: row.get(2)?,
                token: row.get(3)?,
                registered_at: row.get(4)?,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn save_registration(conn: &Connection, registration: &Registration) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO push_registrations \
         (cloud_origin, device_id, platform, token, registered_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            registration.cloud_origin,
            registration.device_id,
            registration.platform,
            registration.token,
            registration.registered_at,
        ],
    )
    .map(drop)
    .map_err(|e| e.to_string())
}

pub fn delete_registration(conn: &Connection, origin: &str) -> Result<(), String> {
    conn.execute(
        "DELETE FROM push_registrations WHERE cloud_origin = ?1",
        params![origin],
    )
    .map(drop)
    .map_err(|e| e.to_string())
}

/// What a registration sends. Field names are Cloud's.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RegisterBody<'a> {
    device_id: &'a str,
    platform: &'a str,
    token: &'a str,
    app_version: &'a str,
    locale: &'a str,
}

/// Sign and send one request to Cloud. Only https, or http on loopback: the
/// proof header is a bearer credential for five minutes.
async fn signed_send(
    origin: &str,
    signing_key: &SigningKey,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Result<(), String> {
    if !origin.starts_with("https://") && !is_loopback(origin) {
        return Err("Alexandria Cloud must be https".into());
    }
    let timestamp = chrono::Utc::now().timestamp();
    let n = nonce();
    let signature = proof(signing_key, method.as_str(), path, timestamp, &n);
    let client = reqwest::Client::new();
    let mut request = client
        .request(method, format!("{}{path}", origin.trim_end_matches('/')))
        .header("x-alexandria-timestamp", timestamp.to_string())
        .header("x-alexandria-nonce", &n)
        .header("x-alexandria-proof", signature)
        .timeout(std::time::Duration::from_secs(15));
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        let code = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("{code}: {text}"));
    }
    Ok(())
}

/// Everything about the profile that registering needs, read in one job.
struct ProfileFacts {
    origin: Option<String>,
    existing: Option<Registration>,
    locale: String,
    push_enabled: bool,
}

/// The learner's signing key and DID, from the unlocked vault. The same
/// derivation the commands use; this runs outside a command, so it reads the
/// keystore directly.
async fn issuer_key(state: &AppState) -> Result<(SigningKey, crate::crypto::did::Did), String> {
    let guard = state.keystore.lock().await;
    let keystore = guard.as_ref().ok_or("vault is locked — unlock first")?;
    let mnemonic = keystore.retrieve_mnemonic().map_err(|e| e.to_string())?;
    drop(guard);
    let wallet =
        crate::crypto::wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
    let signing_key = SigningKey::from_bytes(&wallet.signing_key.to_bytes());
    let did = crate::crypto::did::derive_did_key(&signing_key);
    Ok((signing_key, did))
}

fn lease(
    state: &tauri::State<'_, AppState>,
) -> Result<crate::profile::scope::ProfileLease, String> {
    let session = state
        .profile_operations
        .session()
        .ok_or("no profile is unlocked")?;
    state.profile_operations.admit(&session)
}

async fn facts<R: Runtime>(app: &AppHandle<R>) -> Result<ProfileFacts, String> {
    let state = app.state::<AppState>();
    let lease = lease(&state)?;
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            lease,
            "notifications.push.facts",
            |db| {
                let conn = db.conn();
                let origin = cloud_origin(conn);
                let existing = match &origin {
                    Some(origin) => load_registration(conn, origin)?,
                    None => None,
                };
                Ok(ProfileFacts {
                    origin,
                    existing,
                    locale: SettingsStore::get(conn, keys::USER_LANGUAGE),
                    push_enabled: SettingsStore::get(conn, keys::NOTIFICATIONS_PUSH_ENABLED),
                })
            },
        )
        .await
}

fn status_from(facts: Option<&ProfileFacts>) -> PushStatus {
    PushStatus {
        supported: platform().is_some(),
        registered: facts.is_some_and(|f| f.existing.is_some()),
        platform: platform().map(str::to_string),
        cloud_origin: facts.and_then(|f| f.origin.clone()),
        last_error: last_error(),
    }
}

/// Where this device stands, without touching the network.
pub async fn status<R: Runtime>(app: &AppHandle<R>) -> Result<PushStatus, String> {
    let facts = facts(app).await?;
    Ok(status_from(Some(&facts)))
}

/// Ask the platform for a token and tell Cloud about it.
///
/// Idempotent: a second call re-sends the same device id, which Cloud
/// treats as an update. That is also what a rotated token does.
pub async fn register<R: Runtime>(app: &AppHandle<R>) -> Result<PushStatus, String> {
    let facts = facts(app).await?;
    if platform().is_none() {
        return Ok(status_from(Some(&facts)));
    }
    match register_inner(app, &facts).await {
        Ok(()) => {
            set_last_error(None);
            status(app).await
        }
        Err(error) => {
            log::warn!("push: registration failed: {error}");
            set_last_error(Some(error));
            status(app).await
        }
    }
}

async fn register_inner<R: Runtime>(
    app: &AppHandle<R>,
    facts: &ProfileFacts,
) -> Result<(), String> {
    let platform = platform().ok_or("push is not available on this platform")?;
    let origin = facts
        .origin
        .clone()
        .ok_or("no Alexandria Cloud is configured for this network")?;

    match app.notification().request_permission() {
        Ok(PermissionState::Granted) => {}
        Ok(_) => return Err("notification permission was not granted".into()),
        Err(error) => return Err(format!("notification permission: {error}")),
    }
    let token = tauri_plugin_mobile_push::get_token(app)
        .await
        .map_err(|e| e.to_string())?;

    let state = app.state::<AppState>();
    let (signing_key, did) = issuer_key(&state).await?;
    let device_id = facts
        .existing
        .as_ref()
        .map(|r| r.device_id.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let app_version = app.package_info().version.to_string();
    let path = format!("/api/push/devices/for/{}", did.as_str());
    let body = serde_json::to_value(RegisterBody {
        device_id: &device_id,
        platform,
        token: &token,
        app_version: &app_version,
        locale: &facts.locale,
    })
    .map_err(|e| e.to_string())?;
    signed_send(
        &origin,
        &signing_key,
        reqwest::Method::PUT,
        &path,
        Some(body),
    )
    .await?;

    let registration = Registration {
        cloud_origin: origin,
        device_id,
        platform: platform.to_string(),
        token,
        registered_at: crate::commands::credentials::now_rfc3339(),
    };
    let lease = lease(&state)?;
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            lease,
            "notifications.push.save",
            move |db| save_registration(db.conn(), &registration),
        )
        .await
}

/// Tell Cloud to forget this device and forget the registration locally.
/// The local row goes even when Cloud cannot be reached: the learner asked
/// for this device to stop, and a registration Cloud still holds for a token
/// the app no longer refreshes expires on its own.
pub async fn unregister<R: Runtime>(app: &AppHandle<R>) -> Result<PushStatus, String> {
    let facts = facts(app).await?;
    let (Some(origin), Some(existing)) = (facts.origin.clone(), facts.existing.clone()) else {
        return Ok(status_from(Some(&facts)));
    };
    let state = app.state::<AppState>();
    let (signing_key, did) = issuer_key(&state).await?;
    let path = format!(
        "/api/push/devices/for/{}/{}",
        did.as_str(),
        existing.device_id
    );
    if let Err(error) =
        signed_send(&origin, &signing_key, reqwest::Method::DELETE, &path, None).await
    {
        log::warn!("push: Cloud did not acknowledge unregistering: {error}");
        set_last_error(Some(error));
    } else {
        set_last_error(None);
    }
    let lease = lease(&state)?;
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            lease,
            "notifications.push.delete",
            move |db| delete_registration(db.conn(), &origin),
        )
        .await?;
    status(app).await
}

/// A push the platform handed over while the app was running, as the
/// vendored plugin reports it on both platforms.
#[derive(Debug, Clone, Deserialize)]
struct Received {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    data: PushData,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PushData {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    route: Option<String>,
}

/// The notification a received push becomes, or `None` for a payload that
/// names no kind the app knows. Copy comes from the push when it carried
/// some, else from the app's own table for that kind.
fn notification_from_push(received: &Received) -> Option<NewNotification> {
    let kind_name = received.data.kind.as_deref()?;
    if !kind::ALL.contains(&kind_name) {
        return None;
    }
    let id = received.data.id.clone().unwrap_or_default();
    let copy = cloud_copy(kind_name, &id);
    let title = received
        .title
        .clone()
        .or_else(|| copy.as_ref().map(|c| c.0.to_string()))?;
    let body = received
        .body
        .clone()
        .or_else(|| copy.as_ref().map(|c| c.1.to_string()))
        .unwrap_or_default();
    let route = received.data.route.clone().or_else(|| copy.map(|c| c.2));
    let mut notification = NewNotification::new(kind_name, title, body)
        .payload(serde_json::json!({ "id": id }))
        .from_push();
    if let Some(route) = route {
        notification = notification.route(route);
    }
    if !id.is_empty() {
        notification = notification.dedupe(format!("{kind_name}:{id}"));
    }
    Some(notification)
}

/// Record a received push in the unlocked profile and announce it.
async fn deliver_received<R: Runtime>(app: AppHandle<R>, payload: Value) {
    let received: Received = match serde_json::from_value(payload) {
        Ok(received) => received,
        Err(error) => {
            log::debug!("push: unreadable payload: {error}");
            return;
        }
    };
    let Some(notification) = notification_from_push(&received) else {
        log::debug!("push: payload names no known kind; ignored");
        return;
    };
    let state = app.state::<AppState>();
    let lease = match lease(&state) {
        Ok(lease) => lease,
        Err(_) => {
            // No profile is unlocked, so there is nowhere to record it. The
            // platform's own banner (when it showed one) is what the learner
            // has; the next background pass reports the event once a profile
            // is open.
            return;
        }
    };
    let recorded = state
        .db_executor
        .execute(
            DatabaseWorkload::Background,
            lease,
            "notifications.push.received",
            move |db| super::record_if_enabled(db.conn(), &notification),
        )
        .await;
    match recorded {
        Ok(Some(recorded)) => super::announce(&app, &recorded),
        Ok(None) => {}
        Err(error) => log::debug!("push: could not record: {error}"),
    }
}

/// Subscribe to what the platform reports. Called once from the app's setup
/// hook; the plugin holds the subscriptions for the life of the process.
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    if platform().is_none() {
        return;
    }

    let on_received = app.clone();
    tauri_plugin_mobile_push::subscribe("notification-received", move |payload| {
        let app = on_received.clone();
        let payload = payload.clone();
        tauri::async_runtime::spawn(deliver_received(app, payload));
    });

    let on_tapped = app.clone();
    tauri_plugin_mobile_push::subscribe("notification-tapped", move |payload| {
        // A remote push carries the route at the top of `data`. A local
        // banner that iOS handed to this plugin's delegate carries it where
        // the notification plugin put it, under `__EXTRA__`.
        let route = payload
            .get("data")
            .and_then(|data| {
                data.get("route")
                    .or_else(|| data.get("__EXTRA__")?.get("route"))
            })
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                // A payload with a kind and id but no route still names a
                // place to go.
                let data = payload.get("data")?;
                let kind_name = data.get("kind")?.as_str()?;
                let id = data.get("id").and_then(Value::as_str).unwrap_or_default();
                cloud_copy(kind_name, id).map(|c| c.2)
            });
        if let Some(route) = route {
            super::emit_open(&on_tapped, &route);
        }
    });

    // The platform rotated the token. Re-register only a profile that has a
    // registration on record: one that never opted in stays out.
    let on_token = app.clone();
    tauri_plugin_mobile_push::subscribe("token-received", move |_payload| {
        let app = on_token.clone();
        tauri::async_runtime::spawn(async move {
            match facts(&app).await {
                Ok(facts) if facts.existing.is_some() && facts.push_enabled => {
                    if let Err(error) = register(&app).await {
                        log::debug!("push: re-registration after token rotation: {error}");
                    }
                }
                Ok(_) => {}
                Err(error) => log::debug!("push: token rotated with no profile open: {error}"),
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_round_trips() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let conn = db.conn();
        assert_eq!(load_registration(conn, "https://cloud").unwrap(), None);
        let registration = Registration {
            cloud_origin: "https://cloud".into(),
            device_id: "dev-1".into(),
            platform: "fcm".into(),
            token: "abcd".into(),
            registered_at: "t".into(),
        };
        save_registration(conn, &registration).unwrap();
        assert_eq!(
            load_registration(conn, "https://cloud").unwrap(),
            Some(registration.clone())
        );
        // Re-registering keeps one row per origin.
        let rotated = Registration {
            token: "efgh".into(),
            ..registration
        };
        save_registration(conn, &rotated).unwrap();
        assert_eq!(
            load_registration(conn, "https://cloud").unwrap(),
            Some(rotated)
        );
        delete_registration(conn, "https://cloud").unwrap();
        assert_eq!(load_registration(conn, "https://cloud").unwrap(), None);
    }

    #[test]
    fn a_push_becomes_a_notification_with_cloud_copy_as_fallback() {
        let received: Received = serde_json::from_value(serde_json::json!({
            "data": { "kind": "offer_sent", "id": "o-1" }
        }))
        .unwrap();
        let n = notification_from_push(&received).unwrap();
        assert_eq!(n.title, "New offer");
        assert_eq!(n.route.as_deref(), Some("/settings/organisations"));
        assert_eq!(n.dedupe_key.as_deref(), Some("offer_sent:o-1"));
        assert_eq!(n.source, crate::notifications::Source::Push);

        let with_copy: Received = serde_json::from_value(serde_json::json!({
            "title": "T", "body": "B",
            "data": { "kind": "interview_room_opened", "id": "i-1", "route": "/interview-room/i-1" }
        }))
        .unwrap();
        let n = notification_from_push(&with_copy).unwrap();
        assert_eq!((n.title.as_str(), n.body.as_str()), ("T", "B"));
        assert_eq!(n.route.as_deref(), Some("/interview-room/i-1"));

        let unknown: Received = serde_json::from_value(serde_json::json!({
            "data": { "kind": "something_else", "id": "x" }
        }))
        .unwrap();
        assert!(notification_from_push(&unknown).is_none());
    }

    #[test]
    fn the_platform_is_fcm_on_phones_and_nothing_on_desktop() {
        if cfg!(any(target_os = "ios", target_os = "android")) {
            assert_eq!(platform(), Some("fcm"));
        } else {
            assert_eq!(platform(), None);
        }
    }
}
