//! One durable, recipient-bound destination. Reading does not consume it;
//! only the matching admitted profile may acknowledge successful navigation.
use super::recipients;
use crate::{profile::scope::ProfileLease, AppState};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

static FILE_LOCK: Mutex<()> = Mutex::new(());
const FILE: &str = "notification_open.json";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NotificationOpen {
    pub id: String,
    pub profile_id: String,
    pub recipient: Option<String>,
    pub route: String,
    pub network_id: String,
}

#[derive(Default, Serialize, Deserialize)]
struct OpenState {
    pending: Option<NotificationOpen>,
    consumed: Vec<String>,
}

fn load(state: &AppState) -> Result<OpenState, String> {
    match std::fs::read(state.app_data_dir.join(FILE)) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(OpenState::default()),
        Err(e) => Err(e.to_string()),
    }
}

fn save(state: &AppState, opens: &OpenState) -> Result<(), String> {
    use std::io::Write;
    let path = state.app_data_dir.join(FILE);
    let tmp = state.app_data_dir.join(format!("{FILE}.tmp"));
    let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec(opens).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    std::fs::rename(tmp, path).map_err(|e| e.to_string())
}

fn valid(state: &AppState, intent: &NotificationOpen) -> bool {
    if !recipients::valid_profile(state, &intent.profile_id, &intent.network_id)
        || !intent.route.starts_with('/')
        || intent.route.starts_with("//")
        || intent.route.contains('\\')
        || intent.route.chars().any(char::is_control)
    {
        return false;
    }
    intent.recipient.as_ref().is_none_or(|recipient| {
        recipients::resolve(state, recipient).is_some_and(|binding| {
            binding.profile_id == intent.profile_id && binding.network_id == intent.network_id
        })
    })
}

pub fn hold(
    state: &AppState,
    intent: NotificationOpen,
) -> Result<Option<NotificationOpen>, String> {
    if !valid(state, &intent) {
        return Ok(None);
    }
    let _guard = FILE_LOCK.lock().map_err(|e| e.to_string())?;
    let mut opens = load(state)?;
    if opens.consumed.contains(&intent.id) {
        return Ok(None);
    }
    if opens.pending.as_ref().is_some_and(|p| p.id == intent.id) {
        return Ok(None);
    }
    opens.pending = Some(intent.clone());
    save(state, &opens)?;
    Ok(Some(intent))
}

pub fn pending(state: &AppState) -> Result<Option<NotificationOpen>, String> {
    let _guard = FILE_LOCK.lock().map_err(|e| e.to_string())?;
    Ok(load(state)?.pending.filter(|intent| valid(state, intent)))
}

pub fn consume(state: &AppState, lease: &ProfileLease, id: &str) -> Result<(), String> {
    lease.with_current(|| {
        let _guard = FILE_LOCK.lock().map_err(|e| e.to_string())?;
        let mut opens = load(state)?;
        let Some(intent) = &opens.pending else {
            return Ok(());
        };
        if intent.id != id {
            return Ok(());
        }
        if !valid(state, intent)
            || state.active_id().as_ref().map(|p| p.as_str()) != Some(intent.profile_id.as_str())
        {
            return Err("notification belongs to another profile".into());
        }
        opens.pending = None;
        opens.consumed.push(id.into());
        if opens.consumed.len() > 256 {
            opens.consumed.remove(0);
        }
        save(state, &opens)
    })
}

/// Local banner extras are produced by this app; remote data cannot use this adapter.
pub fn local_tap(
    state: &AppState,
    extra: &serde_json::Map<String, serde_json::Value>,
) -> Result<Option<NotificationOpen>, String> {
    let text = |key: &str| extra.get(key).and_then(serde_json::Value::as_str);
    let (Some(profile), Some(network), Some(notification), Some(route)) = (
        text("profileId"),
        text("networkId"),
        text("notificationId"),
        text("route"),
    ) else {
        return Ok(None);
    };
    if !super::push::canonical_uuid(notification) {
        return Ok(None);
    }
    hold(
        state,
        NotificationOpen {
            id: format!("local:{profile}:{notification}"),
            profile_id: profile.into(),
            recipient: None,
            route: route.into(),
            network_id: network.into(),
        },
    )
}
