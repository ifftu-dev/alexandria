//! Locked-access recipient routing. No tokens, DIDs, keys or notification copy.
use std::sync::Mutex;

use crate::{profile::ProfileId, AppState};
use serde::{Deserialize, Serialize};

static FILE_LOCK: Mutex<()> = Mutex::new(());
const FILE: &str = "notification_recipients.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Binding {
    pub recipient: String,
    pub profile_id: String,
    pub network_id: String,
    pub origin: String,
}

fn load(state: &AppState) -> Result<Vec<Binding>, String> {
    match std::fs::read(state.app_data_dir.join(FILE)) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn network() -> Result<String, String> {
    crate::network_profile::embedded_preprod()
        .map(|p| p.network_id.clone())
        .map_err(|e| e.to_string())
}

pub fn valid_profile(state: &AppState, profile: &str, network_id: &str) -> bool {
    network().is_ok_and(|n| n == network_id)
        && ProfileId::parse(profile)
            .ok()
            .and_then(|id| state.profile_manager.get(&id))
            .is_some_and(|p| p.network_id == network_id)
}

pub fn resolve(state: &AppState, recipient: &str) -> Option<Binding> {
    let _guard = FILE_LOCK.lock().ok()?;
    let entries = load(state).ok()?;
    let matches: Vec<_> = entries
        .into_iter()
        .filter(|b| b.recipient == recipient)
        .collect();
    if matches.len() != 1 {
        return None;
    }
    let binding = matches.into_iter().next()?;
    valid_profile(state, &binding.profile_id, &binding.network_id).then_some(binding)
}

/// Replace all bindings for this profile/origin, retiring a superseded recipient.
pub fn replace(
    state: &AppState,
    profile: &str,
    origin: &str,
    recipient: Option<&str>,
) -> Result<(), String> {
    let _guard = FILE_LOCK.lock().map_err(|e| e.to_string())?;
    let mut bindings = load(state)?;
    bindings.retain(|b| b.profile_id != profile || b.origin != origin);
    if let Some(recipient) = recipient {
        bindings.push(Binding {
            recipient: recipient.into(),
            profile_id: profile.into(),
            network_id: network()?,
            origin: origin.into(),
        });
    }
    let path = state.app_data_dir.join(FILE);
    let tmp = state.app_data_dir.join(format!("{FILE}.tmp"));
    let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    use std::io::Write;
    file.write_all(&serde_json::to_vec(&bindings).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    std::fs::rename(tmp, path).map_err(|e| e.to_string())
}
