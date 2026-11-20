//! Hosted attempts: an assessment request whose organisation holds the
//! answers is taken as an attempt on its hosted service
//! ([`crate::assessment::hosted`]). These commands are thin: they find the
//! Cloud origin and the candidate's key, and pass the service's replies to
//! the screen. Item text goes from the service to the screen and nowhere
//! else.
//!
//! Errors read `hosted_attempt:<code>: <message>`
//! ([`HostedError::into_command_error`]), except the verified-device refusal,
//! which keeps its own `verified_device_required:` form.

use serde::Serialize;
use serde_json::Value;

use crate::assessment::hosted::refusal::{self, Explanation};
use crate::assessment::hosted::wire::{
    AnswerBody, AttemptResult, AttemptState, CloseReply, Consent, Issuance, TermsReply,
};
use crate::assessment::hosted::{
    CloseInputs, HostedError, HostedService, NativePlatformKey, NoPlatformKey, PlatformKey,
};
use crate::credentials::collect::Outcome;
use crate::credentials::Issuer;
use crate::db::executor::DatabaseWorkload;
use crate::profile::scope::ProfileState as State;
use crate::AppState;

/// What became of a closed hosted attempt, for the screen.
#[derive(Debug, Clone, Serialize)]
pub struct HostedOutcome {
    pub attempt_id: String,
    /// The service's grade and probe outcome.
    pub result: AttemptResult,
    pub issuance: HostedIssuance,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum HostedIssuance {
    /// Signed as the organisation. `collected` is what the signed credential
    /// fetch did with it: `stored`, `already_present`, `pending`,
    /// `rejected`, or `null` when the fetch did not reach it.
    Issued {
        credential_id: String,
        collected: Option<String>,
    },
    /// Not issued, and why, in plain words.
    Refused { refusal: Explanation },
    /// Waiting for a live interview before issuing.
    Held { waiting_for: String },
    /// The request carried no issuance policy: the result is recorded only.
    None,
}

async fn service(
    state: &State<'_, AppState>,
    directory_url: &str,
) -> Result<HostedService, String> {
    let dir = super::exchange::directory(state, directory_url).await?;
    let (key, did) = Issuer::from_keystore(&state.keystore)
        .await
        .map_err(|e| e.to_string())?
        .into_parts();
    HostedService::new(&dir.url, key, did).map_err(HostedError::into_command_error)
}

/// This device's platform key: App Attest on an iPhone, none elsewhere (the
/// verified-device check has already refused those).
fn platform() -> &'static dyn PlatformKey {
    if cfg!(target_os = "ios") {
        &NativePlatformKey
    } else {
        &NoPlatformKey
    }
}

/// The terms of a hosted attempt from the request `request_id`: what goes to
/// the organisation, the device requirement, the retake rule and retention,
/// shown before the first item; and whether an attempt can open or resume.
#[tauri::command]
pub async fn hosted_attempt_terms(
    state: State<'_, AppState>,
    directory_url: String,
    request_id: String,
) -> Result<TermsReply, String> {
    service(&state, &directory_url)
        .await?
        .terms(&request_id)
        .await
        .map_err(HostedError::into_command_error)
}

/// Open the attempt once the candidate accepted the terms they were shown.
/// Nothing is sent without acceptance. A role requiring `device_attested` is
/// refused on a device that cannot reach it before anything opens.
#[tauri::command]
pub async fn hosted_attempt_open(
    state: State<'_, AppState>,
    directory_url: String,
    request_id: String,
    consent: Consent,
) -> Result<AttemptState, String> {
    if !consent.accepted {
        return Err(HostedError::ConsentRequired.into_command_error());
    }
    let service = service(&state, &directory_url).await?;
    let terms = service
        .terms(&request_id)
        .await
        .map_err(HostedError::into_command_error)?;
    // The terms changed since they were shown: show them again.
    if terms.terms_digest != consent.terms_digest {
        return Err(HostedError::ConsentRequired.into_command_error());
    }
    let attested = terms.terms.requires_device_attested();
    if attested {
        crate::sentinel::attestation::require_verified_device(
            Some(crate::sentinel::attestation::ASSURANCE_DEVICE_ATTESTED),
            crate::sentinel::attestation::AttemptDevice::current(),
        )?;
    }
    service
        .open(&request_id, &consent, attested, platform())
        .await
        .map_err(HostedError::into_command_error)
}

/// The item the candidate is on: after a dropped connection the attempt
/// resumes here, its clock having kept running. `not_found` once the
/// attempt is closed or abandoned.
#[tauri::command]
pub async fn hosted_attempt_resume(
    state: State<'_, AppState>,
    directory_url: String,
    attempt_id: String,
) -> Result<AttemptState, String> {
    service(&state, &directory_url)
        .await?
        .resume(&attempt_id)
        .await
        .map_err(HostedError::into_command_error)
}

/// Answer the current item. `device_timings` is stored beside the service's
/// own timings and decides nothing.
#[tauri::command]
pub async fn hosted_attempt_answer(
    state: State<'_, AppState>,
    directory_url: String,
    attempt_id: String,
    seq: i32,
    selected: Vec<i64>,
    device_timings: Option<Value>,
) -> Result<AttemptState, String> {
    service(&state, &directory_url)
        .await?
        .answer(
            &attempt_id,
            &AnswerBody {
                seq,
                selected,
                device_timings,
            },
            platform(),
        )
        .await
        .map_err(HostedError::into_command_error)
}

/// Close the attempt with the ended integrity session's assertion and, for
/// an attested session, evidence over the close digest (a paired phone is
/// asked over its link). An issued credential is collected through the
/// signed credential fetch before this returns.
#[tauri::command]
pub async fn hosted_attempt_close(
    state: State<'_, AppState>,
    directory_url: String,
    attempt_id: String,
    integrity_session_id: String,
) -> Result<HostedOutcome, String> {
    let service = service(&state, &directory_url).await?;
    let trust = crate::sentinel::attestation::TrustConfig::from_profile();
    let now = crate::clock::now_rfc3339();
    let inputs = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "hosted_attempt.close.read",
            move |db| {
                CloseInputs::read(db.conn(), &integrity_session_id, &now, &trust)
                    .map_err(HostedError::into_command_error)
            },
        )
        .await?;
    let attester = crate::sentinel::companion::transport::PairedAttester {
        hub: state.content_node.companion(),
        runtime: tokio::runtime::Handle::current(),
    };
    let (did, attempt) = (service.did().clone(), attempt_id.clone());
    // A paired phone answers in seconds: wait with no database job running.
    let body = tokio::task::spawn_blocking(move || inputs.into_body(&did, &attempt, &attester))
        .await
        .map_err(|e| format!("evidence worker failed: {e}"))?;
    let reply = service
        .close(&attempt_id, &body, platform())
        .await
        .map_err(HostedError::into_command_error)?;
    Ok(outcome(&state, &directory_url, reply).await)
}

/// What became of a closed attempt, read again: a held attempt issues once
/// its live interview is done.
#[tauri::command]
pub async fn hosted_attempt_result(
    state: State<'_, AppState>,
    directory_url: String,
    attempt_id: String,
) -> Result<HostedOutcome, String> {
    let reply = service(&state, &directory_url)
        .await?
        .result(&attempt_id)
        .await
        .map_err(HostedError::into_command_error)?;
    Ok(outcome(&state, &directory_url, reply).await)
}

async fn outcome(
    state: &State<'_, AppState>,
    directory_url: &str,
    reply: CloseReply,
) -> HostedOutcome {
    let issuance = match reply.issuance {
        Issuance::Issued { credential_id } => {
            let collected = collect(state, directory_url, &credential_id).await;
            HostedIssuance::Issued {
                credential_id,
                collected,
            }
        }
        Issuance::Refused { refusal } => HostedIssuance::Refused {
            refusal: refusal::explain(&refusal),
        },
        Issuance::Held { waiting_for } => HostedIssuance::Held { waiting_for },
        Issuance::None => HostedIssuance::None,
    };
    HostedOutcome {
        attempt_id: reply.attempt.id,
        result: reply.result,
        issuance,
    }
}

/// Collect the issued credential through the signed fetch, verified and
/// stored like every collected credential. `None` when the fetch failed or
/// did not list it; the background pass collects it later.
async fn collect(
    state: &State<'_, AppState>,
    directory_url: &str,
    credential_id: &str,
) -> Option<String> {
    let dir = super::exchange::directory(state, directory_url)
        .await
        .ok()?;
    let (key, did) = Issuer::from_keystore(&state.keystore)
        .await
        .ok()?
        .into_parts();
    let lease = state.profile_lease();
    let collected =
        match crate::credentials::collect::run(&state.db_executor, &lease, &dir.url, &key, &did)
            .await
        {
            Ok(collected) => collected,
            Err(error) => {
                log::debug!("hosted attempt: credential collection: {error}");
                return None;
            }
        };
    collected
        .into_iter()
        .find(|c| c.credential_id == credential_id)
        .map(|c| match c.outcome {
            Outcome::Stored => "stored".into(),
            Outcome::AlreadyPresent => "already_present".into(),
            Outcome::Pending { .. } => "pending".into(),
            Outcome::Rejected { .. } => "rejected".into(),
        })
}
