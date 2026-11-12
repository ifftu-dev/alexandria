//! Paired attested phone for a desktop integrity session (see
//! `sentinel::companion`).
//!
//! Desktop: `integrity_companion_offer` after `integrity_start_session`
//! (and its unsupported attestation), then `integrity_companion_await`
//! until the phone joins, then `integrity_select_profile` with companion
//! consent. Phone: `integrity_companion_join` with the code, then
//! `integrity_companion_send_camera` per window when it is the camera.

use crate::profile::scope::ProfileState as State;
use serde::Serialize;

use crate::db::executor::DatabaseWorkload;
use crate::sentinel::attestation::{native, Unverified};
use crate::sentinel::companion::{session, transport, CompanionOffer, OFFER_TTL_SECS};
use crate::sentinel::scoring::CameraFeatures;
use crate::AppState;

/// The code the desktop shows, and when it stops working.
#[derive(Debug, Serialize)]
pub struct CompanionOfferResponse {
    pub code: String,
    pub expires_at: i64,
}

/// A phone joined the desktop session.
#[derive(Debug, Serialize)]
pub struct CompanionPaired {
    /// `device_attested`, or `local` when the phone's platform cannot be
    /// verified on this build yet (Android without response keys).
    pub assurance_level: String,
    pub camera: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unverified: Option<Unverified>,
}

/// This phone joined a desktop session.
#[derive(Debug, Serialize)]
pub struct CompanionJoined {
    pub session_id: String,
    pub assurance_level: String,
    /// The desktop asked this phone to be the session's camera.
    pub camera: bool,
}

async fn learner_key(state: &AppState) -> Result<ed25519_dalek::SigningKey, String> {
    let keystore = state.keystore.lock().await;
    let mnemonic = keystore
        .as_ref()
        .ok_or("vault is locked — unlock first")?
        .retrieve_mnemonic()
        .map_err(|e| e.to_string())?;
    drop(keystore);
    let wallet =
        crate::crypto::wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
    Ok(wallet.signing_key.clone())
}

/// Desktop: open a one-time code for the learner's phone to join this
/// session. Refused once the trust profile is chosen.
#[tauri::command]
pub async fn integrity_companion_offer(
    state: State<'_, AppState>,
    session_id: String,
    camera: bool,
) -> Result<CompanionOfferResponse, String> {
    let did = alexandria_verify::did::derive_did_key(&learner_key(&state).await?);
    let node = state.content_node_required().await?;
    let desktop = node
        .endpoint_addr()
        .await
        .ok_or("the peer-to-peer node is not running")?;
    let sid = session_id.clone();
    let started_at = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "integrity.companion.offer",
            move |db| session::pairable_started_at(db.conn(), &sid),
        )
        .await?;
    let hub = node.companion();
    let offer = CompanionOffer::new(
        &session_id,
        &started_at,
        did.as_str(),
        desktop,
        camera,
        hub.now(),
    );
    let response = CompanionOfferResponse {
        code: offer.encode()?,
        expires_at: offer.expires_at,
    };
    hub.offer(offer);
    Ok(response)
}

/// Desktop: wait for the phone to join the open code, then store its
/// verified attestation on the session.
#[tauri::command]
pub async fn integrity_companion_await(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<CompanionPaired, String> {
    let hub = state.content_node_required().await?.companion();
    let join = hub
        .take_join(&session_id)
        .ok_or("no companion code is open for this session")?;
    let wait = std::time::Duration::from_secs(OFFER_TTL_SECS as u64 + 5);
    let joined = tokio::time::timeout(wait, join)
        .await
        .map_err(|_| "the companion code expired before a phone joined".to_string())?
        .map_err(|_| "the companion code was withdrawn".to_string())?;
    let accepted = joined.accepted.clone();
    let (sid, record) = (session_id.clone(), accepted.clone());
    let stored = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "integrity.companion.store",
            move |db| session::store_pairing(db.conn(), &sid, &record),
        )
        .await;
    // The phone link is in place once this returns, before the role is read.
    joined.answer(stored.clone());
    let assurance_level = stored?;
    Ok(CompanionPaired {
        assurance_level,
        camera: hub.role(&session_id).is_some_and(|r| r.camera),
        unverified: accepted.unverified,
    })
}

/// Desktop: withdraw the code and drop the phone link.
#[tauri::command]
pub async fn integrity_companion_cancel(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    state
        .content_node_required()
        .await?
        .companion()
        .close(&session_id);
    Ok(())
}

/// Phone: join the desktop session in `code` as this profile's learner.
#[tauri::command]
pub async fn integrity_companion_join(
    state: State<'_, AppState>,
    code: String,
) -> Result<CompanionJoined, String> {
    if !native::supported() {
        return Err("only an iPhone or Android phone can be a companion".into());
    }
    let offer = CompanionOffer::decode(&code)?;
    let key = learner_key(&state).await?;
    let node = state.content_node_required().await?;
    let endpoint = node
        .endpoint()
        .await
        .ok_or("the peer-to-peer node is not running")?;
    let benchmark = if offer.camera {
        tokio::task::spawn_blocking(crate::sentinel::camera_budget::benchmark)
            .await
            .map_err(|e| format!("benchmark worker failed: {e}"))?
            .map_err(|e| log::warn!(target: "sentinel", "companion benchmark failed: {e}"))
            .ok()
    } else {
        None
    };
    let assurance_level = node
        .companion()
        .join(
            &endpoint,
            &offer,
            &key,
            Box::new(|nonce| native::capture(&nonce)),
            std::sync::Arc::new(native::assert_commitment),
            benchmark,
        )
        .await?;
    Ok(CompanionJoined {
        session_id: offer.session_id,
        assurance_level,
        camera: offer.camera,
    })
}

/// Phone: send one window of camera scores (never frames) to the desktop.
#[tauri::command]
pub async fn integrity_companion_send_camera(
    state: State<'_, AppState>,
    camera: CameraFeatures,
) -> Result<(), String> {
    state
        .content_node_required()
        .await?
        .companion()
        .send_camera(camera)
        .await
}

/// Phone: leave the paired session.
#[tauri::command]
pub async fn integrity_companion_leave(state: State<'_, AppState>) -> Result<(), String> {
    state.content_node_required().await?.companion().leave();
    Ok(())
}
