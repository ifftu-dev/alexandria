//! The credential exchange: an organisation asks a holder for a credential,
//! and the holder answers with a W3C Verifiable Presentation.
//!
//! The presentation carries the full credential, the exact request it answers
//! (so request id, audience, nonce, skill and taxonomy are all under the
//! holder's signature), and — when the holder is also the issuer — the
//! credential's current status. Its proof is a holder `DataIntegrityProof`
//! with purpose `authentication`, `challenge` = the request nonce and `domain`
//! = the organisation's audience, valid for at most five minutes.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::did::{derive_did_key, Did, KeyRegistryEntry};
use crate::vc::presentation::{
    sign_presentation, verify_presentation_proof, VerifiablePresentation, MAX_LIFETIME_SECS,
};
use crate::vc::{
    verify::verify_credential, SkillClaim, VerifiableCredential, VerificationPolicy,
    VerificationResult,
};
use crate::{StoreLookup, VerificationStore};

/// The presentation's extra term naming the request it answers.
pub const REQUEST_PROPERTY: &str = "request";
/// The presentation's extra term carrying the issuer's own status claim.
pub const ISSUER_STATE_PROPERTY: &str = "issuerState";
pub const ASSESSMENT_RUBRIC: &str = "assessment-items-bloom-v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CredentialRequest {
    pub id: String,
    pub audience: String,
    pub nonce: String,
    pub organization: String,
    pub subject_did: String,
    pub skill_id: String,
    pub network_id: String,
    pub taxonomy_digest: String,
    pub purpose: String,
    pub role_label: String,
    pub require_new_assessment: bool,
    pub created_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IssuerState {
    pub revoked: bool,
    pub suspended: bool,
    pub suspended_until: Option<String>,
    pub superseded: bool,
}

fn unix_to_rfc3339(t: i64) -> String {
    // Civil from days (Howard Hinnant), second precision, Zulu.
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

/// The holder answers `request` with `credential`, signed at `now` (unix
/// seconds) and valid for at most five minutes or until the request expires.
pub fn present_credential(
    request: CredentialRequest,
    credential: VerifiableCredential,
    issuer_state: Option<IssuerState>,
    now: i64,
    key: &ed25519_dalek::SigningKey,
) -> Result<VerifiablePresentation, String> {
    let holder = derive_did_key(key);
    if holder.as_str() != request.subject_did {
        return Err("the active identity is not the requested candidate".into());
    }
    let expires = (now + MAX_LIFETIME_SECS).min(request.expires_at);
    let mut presentation = VerifiablePresentation::new(
        Some(format!(
            "urn:alexandria:credential-share:{}:{now}",
            request.id
        )),
        holder,
        vec![serde_json::to_value(&credential).map_err(|e| e.to_string())?],
        &unix_to_rfc3339(now),
        &unix_to_rfc3339(expires),
        &request.nonce,
        &request.audience,
    );
    presentation
        .properties
        .insert(REQUEST_PROPERTY.into(), json!(request));
    if let Some(state) = issuer_state {
        presentation
            .properties
            .insert(ISSUER_STATE_PROPERTY.into(), json!(state));
    }
    let signed = sign_presentation(presentation, key).map_err(|e| e.to_string())?;
    if serde_json::to_vec(&signed)
        .map_err(|e| e.to_string())?
        .len()
        > 256 * 1024
    {
        return Err("credential share exceeds 256 KiB".into());
    }
    Ok(signed)
}

/// The parts of a presentation the exchange reads once the proof holds.
pub struct SharedCredential {
    pub request: CredentialRequest,
    pub credential: VerifiableCredential,
    pub issuer_state: Option<IssuerState>,
}

/// Read the request, credential and issuer state out of a presentation
/// without checking anything about them.
pub fn shared_credential(
    presentation: &VerifiablePresentation,
) -> Result<SharedCredential, String> {
    let request: CredentialRequest = presentation
        .properties
        .get(REQUEST_PROPERTY)
        .cloned()
        .ok_or("presentation names no request")
        .and_then(|v| serde_json::from_value(v).map_err(|_| "presentation request is malformed"))?;
    let issuer_state: Option<IssuerState> = match presentation.properties.get(ISSUER_STATE_PROPERTY)
    {
        Some(v) => {
            Some(serde_json::from_value(v.clone()).map_err(|_| "issuer state is malformed")?)
        }
        None => None,
    };
    let [credential] = presentation.verifiable_credential.as_slice() else {
        return Err("a credential share carries exactly one credential".into());
    };
    let credential: VerifiableCredential =
        serde_json::from_value(credential.clone()).map_err(|e| format!("credential: {e}"))?;
    Ok(SharedCredential {
        request,
        credential,
        issuer_state,
    })
}

/// Verify a holder's presentation against the exact request the
/// organisation issued, then verify the credential it carries.
pub fn verify_share(
    presentation: &VerifiablePresentation,
    expected: &CredentialRequest,
    now: i64,
    now_iso: &str,
) -> Result<VerificationResult, String> {
    let shared = shared_credential(presentation)?;
    if &shared.request != expected {
        return Err("credential share does not match this request".into());
    }
    if expected.audience.is_empty() || expected.nonce.is_empty() || expected.expires_at <= now {
        return Err("credential share or request is expired or has invalid timing".into());
    }
    let proof = verify_presentation_proof(presentation, now)
        .map_err(|e| format!("credential share: {e}"))?;
    if proof.challenge != expected.nonce
        || proof.domain != expected.audience
        || proof.holder.as_str() != expected.subject_did
        || proof.created < expected.created_at
        || proof.expires > expected.expires_at
    {
        return Err("credential share is not bound to this request".into());
    }
    let share = &shared;
    let vc = &share.credential;
    if vc.credential_subject.id.as_str() != expected.subject_did
        || vc.id.as_deref().is_none_or(str::is_empty)
    {
        return Err("credential does not identify the requested candidate".into());
    }
    let claim =
        SkillClaim::extract(&vc.credential_subject).ok_or("credential has no skill claim")?;
    if claim.skill_id != expected.skill_id
        || claim.level > 5
        || !claim.score.is_finite()
        || !(0.0..=1.0).contains(&claim.score)
    {
        return Err("credential does not contain the requested skill".into());
    }
    if expected.require_new_assessment
        && (!claim
            .evidence_refs
            .contains(&format!("request:{}:{}", expected.id, expected.nonce))
            || !vc.type_.iter().any(|t| t == "AssessmentCredential")
            || claim.rubric_version.as_deref() != Some(ASSESSMENT_RUBRIC)
            || !vc.integrity.as_ref().is_some_and(|i| {
                matches!(i.status.as_str(), "completed" | "flagged" | "suspended")
            }))
    {
        return Err("request requires a completed assessment with the current rubric".into());
    }
    if share.issuer_state.is_some() && vc.issuer.as_str() != expected.subject_did {
        return Err("a holder cannot assert another issuer's credential status".into());
    }
    let store = ShareStore { share };
    let mut result = verify_credential(&store, vc, now_iso, &VerificationPolicy::default());
    if share
        .issuer_state
        .as_ref()
        .is_some_and(|state| state.revoked)
    {
        result.revoked = true;
        result.acceptance_decision = crate::vc::AcceptanceDecision::Reject;
    }
    Ok(result)
}

struct ShareStore<'a> {
    share: &'a SharedCredential,
}

impl VerificationStore for ShareStore<'_> {
    fn key_at(&self, _: &Did, _: &str) -> StoreLookup<KeyRegistryEntry> {
        StoreLookup::Missing
    }

    fn status_list_bits(&self, list_id: &str) -> StoreLookup<Vec<u8>> {
        let Some(state) = &self.share.issuer_state else {
            return StoreLookup::Unavailable;
        };
        let Some(status) = &self.share.credential.credential_status else {
            return StoreLookup::Missing;
        };
        if status.status_list_credential != list_id {
            return StoreLookup::Missing;
        }
        let Ok(index) = status.status_list_index.parse::<usize>() else {
            return StoreLookup::Missing;
        };
        if index >= 8 * 1024 * 1024 {
            return StoreLookup::Missing;
        }
        let mut bits = vec![0; index / 8 + 1];
        if state.revoked {
            crate::vc::status::set_bit(&mut bits, index, true).ok();
        }
        StoreLookup::Found(bits)
    }

    fn suspension(&self, _: &str) -> StoreLookup<(bool, Option<String>)> {
        match &self.share.issuer_state {
            Some(state) => StoreLookup::Found((state.suspended, state.suspended_until.clone())),
            None => StoreLookup::Unavailable,
        }
    }

    fn is_superseded(&self, _: &str) -> StoreLookup<bool> {
        match &self.share.issuer_state {
            Some(state) => StoreLookup::Found(state.superseded),
            None => StoreLookup::Unavailable,
        }
    }
}
