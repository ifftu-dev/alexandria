use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey};
use serde::{Deserialize, Serialize};

use crate::did::{derive_did_key, resolve_did_key, Did, KeyRegistryEntry};
use crate::vc::{
    verify::verify_credential, SkillClaim, VerifiableCredential, VerificationPolicy,
    VerificationResult,
};
use crate::{StoreLookup, VerificationStore};

pub const FORMAT: &str = "alexandria-credential-exchange/1";
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IssuerState {
    pub revoked: bool,
    pub suspended: bool,
    pub suspended_until: Option<String>,
    pub superseded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialShare {
    pub format: String,
    pub request: CredentialRequest,
    pub issued_at: i64,
    pub expires_at: i64,
    pub credential: VerifiableCredential,
    pub issuer_state: Option<IssuerState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedCredentialShare {
    pub share: CredentialShare,
    pub signature: String,
}

fn signing_bytes(share: &CredentialShare) -> Result<Vec<u8>, String> {
    let mut bytes = FORMAT.as_bytes().to_vec();
    bytes.push(0);
    bytes.extend(serde_json_canonicalizer::to_vec(share).map_err(|e| e.to_string())?);
    if bytes.len() > 256 * 1024 {
        return Err("credential share exceeds 256 KiB".into());
    }
    Ok(bytes)
}

pub fn sign_share(
    share: CredentialShare,
    key: &SigningKey,
) -> Result<SignedCredentialShare, String> {
    if derive_did_key(key).as_str() != share.request.subject_did {
        return Err("the active identity is not the requested candidate".into());
    }
    let signature = URL_SAFE_NO_PAD.encode(key.sign(&signing_bytes(&share)?).to_bytes());
    Ok(SignedCredentialShare { share, signature })
}

pub fn verify_share(
    signed: &SignedCredentialShare,
    expected: &CredentialRequest,
    now: i64,
    now_iso: &str,
) -> Result<VerificationResult, String> {
    let share = &signed.share;
    if share.format != FORMAT || &share.request != expected {
        return Err("credential share does not match this request".into());
    }
    if expected.audience.is_empty()
        || expected.nonce.is_empty()
        || expected.expires_at <= now
        || share.expires_at <= now
        || share.issued_at > now.saturating_add(30)
        || share.issued_at < expected.created_at
        || share.expires_at <= share.issued_at
        || share.expires_at > share.issued_at.saturating_add(300)
        || share.expires_at > expected.expires_at
    {
        return Err("credential share or request is expired or has invalid timing".into());
    }
    let key = resolve_did_key(&Did(expected.subject_did.clone())).map_err(|e| e.to_string())?;
    let signature = Signature::from_slice(
        &URL_SAFE_NO_PAD
            .decode(&signed.signature)
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    key.verify_strict(&signing_bytes(share)?, &signature)
        .map_err(|_| "invalid holder signature")?;
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
    share: &'a CredentialShare,
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
