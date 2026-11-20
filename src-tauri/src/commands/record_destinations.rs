//! Record destinations: stores the learner names and sends learning records
//! to directly, and the local access log of those sends. The logic is in
//! [`crate::record_sending`]; these commands only fetch the learner's key
//! and hand over.

use crate::credentials::Issuer;
use crate::domain::record_sending::{
    DestinationPreview, NewDestinationRequest, RecordAccessEntry, RecordDestination, RemovalReport,
};
use crate::profile::scope::ProfileState as State;
use crate::record_sending::outbox::{self, SendFailure};
use crate::record_sending::{access_log, destinations, LeasedDb};
use crate::AppState;

/// The learner's key seed and DID, which every identifier a recipient sees
/// is derived from.
async fn learner_key(state: &State<'_, AppState>) -> Result<([u8; 32], String), String> {
    let issuer = Issuer::from_keystore(&state.keystore)
        .await
        .map_err(|e| e.to_string())?;
    Ok((issuer.key().to_bytes(), issuer.did().as_str().to_string()))
}

fn leased(state: &State<'_, AppState>) -> LeasedDb {
    LeasedDb::new(state.db_executor.clone(), state.profile_lease())
}

/// What the consent screen shows for a store before the learner approves
/// it: where the address leads and the identifier records would carry.
/// Sends nothing to the store.
#[tauri::command]
pub async fn record_destination_preview(
    state: State<'_, AppState>,
    address: String,
    username: String,
) -> Result<DestinationPreview, String> {
    let (secret, did) = learner_key(&state).await?;
    let secret = zeroize::Zeroizing::new(secret);
    destinations::preview(&leased(&state), &address, &username, &secret, &did).await
}

/// The learner approved a store.
#[tauri::command]
pub async fn record_destination_add(
    state: State<'_, AppState>,
    request: NewDestinationRequest,
) -> Result<RecordDestination, String> {
    let (secret, did) = learner_key(&state).await?;
    let secret = zeroize::Zeroizing::new(secret);
    destinations::add(&leased(&state), request, &secret, &did, chrono::Utc::now()).await
}

#[tauri::command]
pub async fn record_destinations(
    state: State<'_, AppState>,
) -> Result<Vec<RecordDestination>, String> {
    state
        .db_executor
        .execute(
            crate::db::executor::DatabaseWorkload::Learner,
            state.profile_lease(),
            "records.destinations.list",
            |db| destinations::list(db.conn()).map_err(|e| e.to_string()),
        )
        .await
}

/// Stops sending to a store and asks it to void what it was sent.
#[tauri::command]
pub async fn record_destination_remove(
    state: State<'_, AppState>,
    id: String,
) -> Result<RemovalReport, String> {
    let (secret, did) = learner_key(&state).await?;
    let secret = zeroize::Zeroizing::new(secret);
    destinations::remove(&leased(&state), &id, &secret, &did, chrono::Utc::now()).await
}

/// Asks the background sender to try every destination now.
#[tauri::command]
pub async fn record_destinations_send_now(_state: State<'_, AppState>) -> Result<(), String> {
    destinations::wake();
    Ok(())
}

/// Every send of the learner's records this device made, newest first.
#[tauri::command]
pub async fn record_access_log(
    state: State<'_, AppState>,
) -> Result<Vec<RecordAccessEntry>, String> {
    state
        .db_executor
        .execute(
            crate::db::executor::DatabaseWorkload::Learner,
            state.profile_lease(),
            "records.access-log.list",
            |db| access_log::list(db.conn()).map_err(|e| e.to_string()),
        )
        .await
}

/// Records a recipient refused at their last try, for the activity log.
#[tauri::command]
pub async fn record_send_failures(state: State<'_, AppState>) -> Result<Vec<SendFailure>, String> {
    state
        .db_executor
        .execute(
            crate::db::executor::DatabaseWorkload::Learner,
            state.profile_lease(),
            "records.send-failures.list",
            |db| outbox::failures(db.conn()).map_err(|e| e.to_string()),
        )
        .await
}
