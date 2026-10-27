// SPDX-License-Identifier: AGPL-3.0-or-later
//! Recipient-bound mobile push. The encrypted profile DB owns registration
//! intent; the locked-access sidecar contains only routing associations.
use ed25519_dalek::SigningKey;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager, Runtime};

use super::{cloud_copy, recipients, NewNotification, Recorded};
use crate::commands::holder_pull::{cloud_origin, is_loopback, nonce, proof};
use crate::db::executor::DatabaseWorkload;
use crate::profile::scope::ProfileLease;
use crate::settings::{registry::keys, SettingsStore};
use crate::AppState;

// Serialize intent, PUT and DELETE, including SDK callbacks. The profile lease
// cancels waiters on lock; this gate owns no profile resources or session state.
static OPERATIONS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushStatus {
    pub supported: bool,
    pub registered: bool,
    pub platform: Option<String>,
    pub cloud_origin: Option<String>,
    pub last_error: Option<String>,
    pub enabled: bool,
    pub pending_cleanup: bool,
    pub pending_registration: bool,
}

pub fn platform() -> Option<&'static str> {
    cfg!(any(target_os = "ios", target_os = "android")).then_some("fcm")
}

#[derive(Clone)]
struct Registration {
    origin: String,
    device_id: String,
    recipient: Option<String>,
    pending: String,
    revision: i64,
    error: Option<String>,
    put_body: Option<String>,
}

fn registrations(conn: &Connection) -> Result<Vec<Registration>, String> {
    let mut statement = conn.prepare("SELECT cloud_origin, device_id, recipient, pending, revision, last_error, put_body FROM push_registrations ORDER BY cloud_origin").map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(Registration {
                origin: row.get(0)?,
                device_id: row.get(1)?,
                recipient: row.get(2)?,
                pending: row.get(3)?,
                revision: row.get(4)?,
                error: row.get(5)?,
                put_body: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

struct Facts {
    origin: Option<String>,
    rows: Vec<Registration>,
    enabled: bool,
    locale: String,
    revision: i64,
    native_error: Option<String>,
}

async fn facts(state: &AppState, lease: &ProfileLease) -> Result<Facts, String> {
    state.db_executor.execute(DatabaseWorkload::Learner, lease.clone(), "notifications.push.facts", |db| {
        Ok(Facts { origin: cloud_origin(db.conn()), rows: registrations(db.conn())?,
            enabled: SettingsStore::get(db.conn(), keys::NOTIFICATIONS_PUSH_ENABLED),
            locale: SettingsStore::get(db.conn(), keys::USER_LANGUAGE),
            revision: db.conn().query_row("SELECT revision FROM push_operation_state WHERE singleton=1", [], |r| r.get(0)).map_err(|e| e.to_string())?,
            native_error: db.conn().query_row("SELECT last_error FROM push_operation_state WHERE singleton=1 AND error_origin IS ?1", params![cloud_origin(db.conn())], |r| r.get(0)).optional().map_err(|e| e.to_string())?.flatten(),
        })
    }).await
}

fn status_from(f: &Facts) -> PushStatus {
    PushStatus {
        supported: platform().is_some(),
        platform: platform().map(str::to_string),
        cloud_origin: f.origin.clone(),
        enabled: f.enabled,
        registered: f.enabled
            && f.rows.iter().any(|r| {
                Some(&r.origin) == f.origin.as_ref() && r.pending == "none" && r.recipient.is_some()
            }),
        pending_cleanup: f
            .rows
            .iter()
            .any(|r| r.pending == "delete" || !f.enabled || Some(&r.origin) != f.origin.as_ref()),
        pending_registration: f
            .rows
            .iter()
            .any(|r| r.pending == "put" && f.enabled && Some(&r.origin) == f.origin.as_ref()),
        last_error: f
            .rows
            .iter()
            .find_map(|r| r.error.clone())
            .or_else(|| f.native_error.clone()),
    }
}

pub async fn status(state: &AppState, lease: ProfileLease) -> Result<PushStatus, String> {
    let f = facts(state, &lease).await?;
    if !lease.is_current() {
        return Err("profile session changed".into());
    }
    Ok(status_from(&f))
}

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

async fn signed_send(
    origin: &str,
    key: &SigningKey,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
    revision: i64,
) -> Result<reqwest::Response, String> {
    if !origin.starts_with("https://") && !is_loopback(origin) {
        return Err("Alexandria Cloud must be https".into());
    }
    let timestamp = chrono::Utc::now().timestamp();
    let n = nonce();
    let signature = proof(key, method.as_str(), path, timestamp, &n);
    // A redirect must never forward holder proofs to another origin.
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let mut request = client
        .request(method, format!("{}{path}", origin.trim_end_matches('/')))
        .header("x-alexandria-timestamp", timestamp.to_string())
        .header("x-alexandria-nonce", n)
        .header("x-alexandria-proof", signature)
        .header("x-alexandria-push-version", revision.to_string())
        .timeout(std::time::Duration::from_secs(15));
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request
        .send()
        .await
        .map_err(|_| "Cloud push request failed".to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "Cloud push request returned {}",
            response.status().as_u16()
        ));
    }
    // Only the versioned acknowledgement proves the mutation participated in
    // Cloud's durable ordering. A legacy 2xx is not a deletion barrier.
    if response
        .headers()
        .get("x-alexandria-push-protocol")
        .and_then(|v| v.to_str().ok())
        != Some("versioned-v1")
        || response
            .headers()
            .get("x-alexandria-push-version")
            .and_then(|v| v.to_str().ok())
            != Some(revision.to_string().as_str())
    {
        return Err("Cloud did not acknowledge the push mutation version".into());
    }
    Ok(response)
}

pub(crate) fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|uuid| !uuid.is_nil() && uuid.to_string() == value)
}

/// Reconcile a captured profile session using a token obtained at the native
/// boundary. A missing token still permits cleanup. Never infer opt-in from it.
pub(crate) async fn reconcile(
    state: &AppState,
    lease: ProfileLease,
    token: Option<String>,
    enabled: Option<bool>,
) -> Result<PushStatus, String> {
    let work = lease.clone();
    lease
        .run_until_closed(async {
            let _serial = OPERATIONS.lock().await;
            reconcile_inner(state, &work, token, enabled).await
        })
        .await
        .ok_or("profile session changed")?
}

async fn reconcile_inner(
    state: &AppState,
    lease: &ProfileLease,
    token: Option<String>,
    enabled: Option<bool>,
) -> Result<PushStatus, String> {
    let profile = state
        .active_id()
        .ok_or("no profile is unlocked")?
        .to_string();
    let before = facts(state, lease).await?;
    let desired = enabled.unwrap_or(before.enabled);
    // Retire routing first. A crash before the DB transaction can lose a
    // usable association, never leave a retired one routable while locked.
    for row in &before.rows {
        if !desired || Some(&row.origin) != before.origin.as_ref() || row.pending == "delete" {
            recipients::replace(state, &profile, &row.origin, None)?;
        }
    }
    let current_origin = before.origin.clone();
    state.db_executor.execute(DatabaseWorkload::Learner, lease.clone(), "notifications.push.intent", move |db| {
        let tx = db.conn().unchecked_transaction().map_err(|e| e.to_string())?;
        if let Some(enabled) = enabled {
            SettingsStore::set(&tx, keys::NOTIFICATIONS_PUSH_ENABLED, enabled).map_err(|e| e.to_string())?;
        }
        tx.execute("UPDATE push_registrations SET pending='delete',recipient=NULL,revision=revision+1 WHERE pending!='delete' AND (?1=0 OR cloud_origin IS NOT ?2)", params![desired, current_origin]).map_err(|e| e.to_string())?;
        tx.execute("UPDATE push_operation_state SET revision=revision+1, last_error=CASE WHEN ?1 THEN last_error ELSE NULL END, error_origin=CASE WHEN ?1 THEN error_origin ELSE NULL END WHERE singleton=1", params![desired]).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }).await?;
    let staged = facts(state, lease).await?;
    for row in staged.rows.iter().filter(|r| r.pending == "delete") {
        let (key, did) = issuer_key(state).await?;
        let result = signed_send(
            &row.origin,
            &key,
            reqwest::Method::DELETE,
            &format!("/api/push/devices/for/{}/{}", did.as_str(), row.device_id),
            None,
            row.revision,
        )
        .await
        .and_then(|response| {
            if response.status() == reqwest::StatusCode::NO_CONTENT {
                Ok(())
            } else {
                Err("Cloud did not acknowledge removal".into())
            }
        });
        let origin = row.origin.clone();
        let revision = row.revision;
        state.db_executor.execute(DatabaseWorkload::Learner, lease.clone(), "notifications.push.cleanup", move |db| {
            match result {
                Ok(()) => db.conn().execute("DELETE FROM push_registrations WHERE cloud_origin=?1 AND revision=?2 AND pending='delete'", params![origin, revision]),
                Err(error) => db.conn().execute("UPDATE push_registrations SET last_error=?1 WHERE cloud_origin=?2 AND revision=?3", params![error, origin, revision]),
            }.map(drop).map_err(|e| e.to_string())
        }).await?;
    }
    let f = facts(state, lease).await?;
    if !f.enabled
        || f.rows
            .iter()
            .any(|r| Some(&r.origin) == f.origin.as_ref() && r.pending == "delete")
    {
        return Ok(status_from(&f));
    }
    let (Some(origin), Some(token)) = (f.origin, token) else {
        return status(state, lease.clone()).await;
    };
    let existing = f.rows.iter().find(|r| r.origin == origin);
    let device_id = existing
        .map(|r| r.device_id.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let body = serde_json::json!({
        "deviceId": device_id, "platform": "fcm", "token": token.trim(),
        "appVersion": env!("CARGO_PKG_VERSION"), "locale": f.locale.trim().chars().take(32).collect::<String>()
    });
    let serialized = body.to_string();
    let revision = match existing {
        Some(row) if row.revision > 0 && row.put_body.as_deref() == Some(&serialized) => {
            row.revision
        }
        Some(row) => row
            .revision
            .checked_add(1)
            .filter(|v| *v < i64::MAX)
            .ok_or("push mutation version exhausted")?,
        None => 1,
    };
    recipients::replace(state, &profile, &origin, None)?;
    let staged_origin = origin.clone();
    let staged_device = device_id.clone();
    let staged_token = token.clone();
    let network = recipients::network()?;
    state.db_executor.execute(DatabaseWorkload::Learner, lease.clone(), "notifications.push.stage", move |db| {
        db.conn().execute("INSERT INTO push_registrations(cloud_origin, device_id, platform, token, registered_at, pending, revision, network_id, put_body) VALUES (?1,?2,'fcm',?3,'','put',?4,?5,?6) ON CONFLICT(cloud_origin) DO UPDATE SET token=excluded.token, recipient=NULL, pending='put', revision=excluded.revision, last_error=NULL, network_id=excluded.network_id, put_body=excluded.put_body", params![staged_origin, staged_device, staged_token, revision, network, serialized]).map(drop).map_err(|e| e.to_string())
    }).await?;
    let (key, did) = issuer_key(state).await?;
    let result = async {
        let response = signed_send(
            &origin,
            &key,
            reqwest::Method::PUT,
            &format!("/api/push/devices/for/{}", did.as_str()),
            Some(body),
            revision,
        )
        .await?;
        if response.status() != reqwest::StatusCode::OK {
            return Err("Cloud did not return a recipient binding".to_string());
        }
        let body: Value = response
            .json()
            .await
            .map_err(|_| "Cloud returned an invalid recipient binding".to_string())?;
        let recipient = body
            .get("recipient")
            .and_then(Value::as_str)
            .filter(|v| canonical_uuid(v))
            .ok_or("Cloud returned an invalid recipient binding")?
            .to_string();
        Ok::<_, String>(recipient)
    }
    .await;
    let saved_origin = origin.clone();
    let saved_result = result.clone();
    let committed = state.db_executor.execute(DatabaseWorkload::Learner, lease.clone(), "notifications.push.commit", move |db| {
        if !SettingsStore::get(db.conn(), keys::NOTIFICATIONS_PUSH_ENABLED)
            || cloud_origin(db.conn()).as_ref() != Some(&saved_origin) {
            db.conn().execute("UPDATE push_registrations SET recipient=NULL,pending='delete',revision=revision+1 WHERE cloud_origin=?1 AND revision=?2", params![saved_origin, revision]).map_err(|e| e.to_string())?;
            return Ok(false);
        }
        match saved_result {
            Ok(recipient) => {
                let count = db.conn().execute("UPDATE push_registrations SET recipient=?1,pending='none',registered_at=datetime('now'),last_error=NULL WHERE cloud_origin=?2 AND revision=?3", params![recipient, saved_origin, revision]).map_err(|e| e.to_string())?;
                db.conn().execute("UPDATE push_operation_state SET last_error=NULL,error_origin=NULL WHERE singleton=1", []).map_err(|e| e.to_string())?;
                Ok(count == 1)
            },
            Err(error) => db.conn().execute("UPDATE push_registrations SET last_error=?1 WHERE cloud_origin=?2 AND revision=?3", params![error, saved_origin, revision]).map(|_| false),
        }.map_err(|e| e.to_string())
    }).await?;
    if committed && lease.is_current() {
        if let Ok(recipient) = result {
            recipients::replace(state, &profile, &origin, Some(&recipient))?;
        }
    }
    status(state, lease.clone()).await
}

fn admit(state: &AppState) -> Result<ProfileLease, String> {
    state.profile_operations.admit(
        &state
            .profile_operations
            .session()
            .ok_or("no profile is unlocked")?,
    )
}

/// Native SDK work owns only device-global resources. Release admission while
/// awaiting it (and join it normally), then re-admit the exact original session.
/// An intervening registration/preference operation makes its token obsolete.
pub(crate) async fn register_with_token(
    state: &AppState,
    lease: ProfileLease,
    token: impl std::future::Future<Output = Result<String, String>>,
) -> Result<PushStatus, String> {
    let session = lease.session_id().to_string();
    let cleaned = reconcile(state, lease.clone(), None, None).await?;
    if !cleaned.enabled || cleaned.cloud_origin.is_none() {
        return Ok(cleaned);
    }
    let before = facts(state, &lease).await?;
    drop(lease);
    let token = token.await;
    let lease = state.profile_operations.admit(&session)?;
    let work = lease.clone();
    lease.run_until_closed(async {
        let _serial = OPERATIONS.lock().await;
        let now = facts(state, &work).await?;
        if now.revision != before.revision || now.origin != before.origin || !now.enabled {
            return Ok(status_from(&now));
        }
        let error = token.as_ref().err().cloned();
        let origin = now.origin.clone();
        state.db_executor.execute(DatabaseWorkload::Learner, work.clone(), "notifications.push.native_status", move |db| {
            db.conn().execute("UPDATE push_operation_state SET last_error=?1,error_origin=?2 WHERE singleton=1", params![error, origin]).map(drop).map_err(|e| e.to_string())
        }).await?;
        match token {
            Ok(token) => reconcile_inner(state, &work, Some(token), None).await,
            Err(_) => status(state, work.clone()).await,
        }
    }).await.ok_or("profile session changed")?
}

pub async fn register<R: Runtime>(
    app: &AppHandle<R>,
    lease: ProfileLease,
) -> Result<PushStatus, String> {
    let state = app.state::<AppState>();
    if platform().is_none() {
        return reconcile(&state, lease, None, None).await;
    }
    register_with_token(&state, lease, async {
        let granted = tauri_plugin_mobile_push::request_permission(app)
            .await
            .map_err(|e| e.to_string())?;
        if !granted {
            return Err("Notification permission was not granted".into());
        }
        tauri_plugin_mobile_push::get_token(app)
            .await
            .map_err(|e| e.to_string())
    })
    .await
}

pub async fn unregister<R: Runtime>(
    app: &AppHandle<R>,
    lease: ProfileLease,
) -> Result<PushStatus, String> {
    reconcile(&app.state::<AppState>(), lease, None, Some(false)).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PushData {
    recipient: String,
    event_id: String,
    kind: String,
    id: String,
    route: String,
    title: String,
    body: String,
}

fn parse(payload: Value) -> Option<PushData> {
    let data: PushData = serde_json::from_value(payload.get("data")?.clone()).ok()?;
    if !canonical_uuid(&data.recipient)
        || !canonical_uuid(&data.id)
        || data.title.len() + data.body.len() > 4096
    {
        return None;
    }
    let (_, _, route) = cloud_copy(&data.kind, &data.id)?;
    if route != data.route {
        return None;
    }
    let prefix = format!("{}:{}", data.kind, data.id);
    if data.kind == super::kind::INTERVIEW_ROOM_OPENED {
        if !data
            .event_id
            .strip_prefix(&format!("{prefix}:"))
            .is_some_and(canonical_uuid)
        {
            return None;
        }
    } else if data.event_id != prefix {
        return None;
    }
    Some(data)
}

fn notification(data: &PushData, origin: &str) -> Option<NewNotification> {
    let (title, body, route) = cloud_copy(&data.kind, &data.id)?;
    Some(
        NewNotification::new(&data.kind, title, body)
            .route(route)
            .dedupe(&data.event_id)
            .payload(
                serde_json::json!({"id": data.id, "eventId": data.event_id, "cloudOrigin": origin}),
            )
            .from_push(),
    )
}

pub(crate) async fn receive(
    state: &AppState,
    lease: ProfileLease,
    payload: Value,
) -> Result<Option<Recorded>, String> {
    let Some(data) = parse(payload) else {
        return Ok(None);
    };
    let Some(binding) = recipients::resolve(state, &data.recipient) else {
        return Ok(None);
    };
    if state.active_id().as_ref().map(|p| p.as_str()) != Some(binding.profile_id.as_str()) {
        return Ok(None);
    }
    let Some(new) = notification(&data, &binding.origin) else {
        return Ok(None);
    };
    state
        .db_executor
        .execute(
            DatabaseWorkload::Background,
            lease,
            "notifications.push.received",
            move |db| {
                if !registrations(db.conn())?.iter().any(|r| {
                    r.origin == binding.origin
                        && r.pending == "none"
                        && r.recipient.as_deref() == Some(&data.recipient)
                }) {
                    return Ok(None);
                }
                super::record_if_enabled(db.conn(), &new)
            },
        )
        .await
}

pub(crate) fn tapped(
    state: &AppState,
    payload: Value,
) -> Result<Option<super::opens::NotificationOpen>, String> {
    if payload.get("local").and_then(Value::as_bool) == Some(true) {
        return match payload
            .get("data")
            .and_then(|d| d.get("__EXTRA__"))
            .and_then(Value::as_object)
        {
            Some(extra) => super::opens::local_tap(state, extra),
            None => Ok(None),
        };
    }
    let Some(data) = parse(payload) else {
        return Ok(None);
    };
    let Some(binding) = recipients::resolve(state, &data.recipient) else {
        return Ok(None);
    };
    let Some(new) = notification(&data, &binding.origin) else {
        return Ok(None);
    };
    let mut route = new.route.unwrap_or_default();
    // A locked/inactive tap has no inbox row yet. Carry only the origin
    // resolved from this local binding; provider routes cannot supply one.
    if data.kind == super::kind::INTERVIEW_ROOM_OPENED {
        route = format!(
            "{route}?cloud={}",
            url::form_urlencoded::byte_serialize(binding.origin.as_bytes()).collect::<String>()
        );
    }
    super::opens::hold(
        state,
        super::opens::NotificationOpen {
            id: format!("{}:{}", data.recipient, data.event_id),
            profile_id: binding.profile_id,
            recipient: Some(data.recipient),
            route,
            network_id: binding.network_id,
        },
    )
}

pub fn install<R: Runtime>(app: &AppHandle<R>) {
    if platform().is_none() {
        return;
    }
    let received_app = app.clone();
    tauri_plugin_mobile_push::subscribe("notification-received", move |payload| {
        let app = received_app.clone();
        let Ok(lease) = admit(&app.state::<AppState>()) else {
            return;
        };
        let payload = payload.clone();
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            if let Ok(Some(recorded)) = receive(&state, lease.clone(), payload).await {
                if lease.is_current() {
                    super::announce(&app, &recorded);
                }
            }
        });
    });
    let tapped_app = app.clone();
    tauri_plugin_mobile_push::subscribe("notification-tapped", move |payload| {
        if let Ok(Some(intent)) = tapped(&tapped_app.state::<AppState>(), payload.clone()) {
            super::emit_open(&tapped_app, &intent);
        }
    });
    let token_app = app.clone();
    tauri_plugin_mobile_push::subscribe("token-received", move |payload| {
        let app = token_app.clone();
        let Ok(lease) = admit(&app.state::<AppState>()) else {
            return;
        };
        let token = payload
            .get("token")
            .and_then(Value::as_str)
            .map(str::to_string);
        tauri::async_runtime::spawn(async move {
            let _ = reconcile(&app.state::<AppState>(), lease, token, None).await;
        });
    });
}

#[cfg(test)]
#[path = "push_tests.rs"]
mod boundary_tests;
