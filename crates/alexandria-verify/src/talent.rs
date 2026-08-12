//! Talent-index records: the wire format, and its signature.
//!
//! A learner may consent to publish a small, signed summary of their skills to
//! an index so that employers can find them. This module is the part anybody
//! needs in order to check that such a record is genuine — the shape on the
//! wire, and `verify_record`.
//!
//! It sits in this crate rather than in the application for the same reason
//! credential verification does. An index verifies submissions, and an index is
//! not necessarily ours: anyone should be able to run one, and reject a forged
//! listing, without linking a desktop app. The consent rules that decide what a
//! learner is willing to publish stay in the application, because they are
//! about a person's choice rather than about a signature.
//!
//! The rule that matters is in `verify_record`: the verifying key is resolved
//! from the record's own `subjectDid`, never from `proof.verificationMethod`.
//! Trusting the proof to name its own key would let anyone sign a record
//! claiming to be anyone.

use serde::{Deserialize, Serialize};

/// Carries the derived level and the evidence behind it, not the raw score.
/// The level is the unit the rest of the product already speaks in, and
/// exposing `raw_score`/`confidence` would invite employers to rank on numbers
/// whose derivation they cannot audit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishedSkill {
    pub skill_id: String,
    /// Human-readable name at publication time. Denormalized deliberately: an
    /// index should not have to resolve a taxonomy to render a listing.
    pub name: String,
    /// Derived level, 1–5.
    pub level: u8,
    /// How many independent issuer clusters back this skill. An employer's
    /// most useful single signal, and it cannot be inflated by one issuer
    /// repeatedly attesting.
    pub issuer_clusters: u32,
}

/// Exactly what leaves the device.
///
/// Serialized shape is the wire format. It is intentionally small and flat:
/// everything here is something the learner ticked, and anything a learner
/// cannot see in a preview has no business being in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TalentIndexRecord {
    /// The learner's DID. Always present — a listing has to identify someone,
    /// and this is the identifier an employer later verifies credentials
    /// against.
    pub subject_did: String,
    /// Consented skills, in the order given by the source, deduplicated.
    pub skills: Vec<PublishedSkill>,
    /// Present only with explicit consent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Present only with explicit consent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bio: Option<String>,
    /// RFC 3339 instant after which this record must be ignored.
    ///
    /// Required, and inside the signed payload. This is the withdrawal
    /// mechanism: a learner who unticks everything simply stops republishing,
    /// and the record lapses without anyone having to honour a delete request.
    /// An index that ignores expiry is misbehaving in a way a signature cannot
    /// prevent — but a *stale* record cannot be passed off as current.
    pub valid_until: String,
}

/// Detached-JWS proof over a [`TalentIndexRecord`].
///
/// Same shape as a credential's proof so there is one signature format in the
/// system rather than two.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordProof {
    #[serde(rename = "type")]
    pub type_: String,
    pub created: String,
    /// The key that signed, as `<did>#key-1`.
    pub verification_method: String,
    /// Detached compact JWS: `header..signature`.
    pub jws: String,
}

/// A record plus its signature — what actually leaves the device.
///
/// The proof sits alongside the payload rather than inside it, so signing needs
/// no "clear the signature field first" dance and the consent preview can show
/// the payload without a JWS blob in the middle of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedTalentIndexRecord {
    pub record: TalentIndexRecord,
    pub proof: RecordProof,
}

/// Why a signed record was not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordError {
    /// `subjectDid` is not a resolvable `did:key`.
    UnresolvableSubject,
    /// Signature did not verify against the subject's key.
    BadSignature,
    /// Proof was not the expected shape.
    MalformedProof(String),
    /// The record's term has passed.
    Expired,
}

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnresolvableSubject => write!(f, "subject DID is not resolvable"),
            Self::BadSignature => write!(f, "signature does not match the subject"),
            Self::MalformedProof(why) => write!(f, "malformed proof: {why}"),
            Self::Expired => write!(f, "record has expired"),
        }
    }
}

/// JWS protected header. Fixed, so it canonicalizes identically for signer and
/// verifier — same constant as the credential path, for the same reason.
const JWS_HEADER_JSON: &str = r#"{"alg":"EdDSA","b64":false,"crit":["b64"]}"#;

/// Bytes that get signed: `header_b64 || '.' || JCS(record)`.
fn signing_input(record: &TalentIndexRecord, header_b64: &str) -> Result<Vec<u8>, String> {
    let value = serde_json::to_value(record).map_err(|e| e.to_string())?;
    let canonical = crate::vc::canonicalize::canonicalize(&value)
        .map_err(|e| format!("canonicalize record: {e}"))?;
    let mut out = Vec::with_capacity(header_b64.len() + 1 + canonical.len());
    out.extend_from_slice(header_b64.as_bytes());
    out.push(b'.');
    out.extend_from_slice(&canonical);
    Ok(out)
}

/// Sign a record with the subject's key.
///
/// Refuses when the key does not derive the DID the record names. Signing a
/// record about somebody else would produce something that verifies against the
/// wrong key and fails at the far end for a confusing reason; better to fail
/// here, where the cause is obvious.
pub fn sign_record(
    record: &TalentIndexRecord,
    signing_key: &ed25519_dalek::SigningKey,
    now: &str,
) -> Result<SignedTalentIndexRecord, String> {
    use ed25519_dalek::Signer;

    let did = crate::did::derive_did_key(signing_key);
    if did.as_str() != record.subject_did {
        return Err(format!(
            "cannot sign a record for {} with the key for {}",
            record.subject_did,
            did.as_str()
        ));
    }

    let header_b64 = crate::vc::sign::b64url(JWS_HEADER_JSON.as_bytes());
    let input = signing_input(record, &header_b64)?;
    let sig = signing_key.sign(&input);
    let sig_b64 = crate::vc::sign::b64url(&sig.to_bytes());

    Ok(SignedTalentIndexRecord {
        record: record.clone(),
        proof: RecordProof {
            type_: "DataIntegrityProof".into(),
            created: now.to_string(),
            verification_method: format!("{}#key-1", did.as_str()),
            jws: format!("{header_b64}..{sig_b64}"),
        },
    })
}

/// Verify a signed record against the subject DID it names.
///
/// The key comes from the record's own `subjectDid` via `did:key`
/// self-resolution, **not** from `proof.verificationMethod`. Trusting the proof
/// to name its own key would let a forger sign with any key and point the
/// verifier at it — the whole check would become "is this internally
/// consistent", which every forgery is.
pub fn verify_record(
    signed: &SignedTalentIndexRecord,
    verification_time: &str,
) -> Result<(), RecordError> {
    use ed25519_dalek::Verifier;

    if signed.record.valid_until.as_str() <= verification_time {
        return Err(RecordError::Expired);
    }

    let subject = crate::did::Did(signed.record.subject_did.clone());
    let key =
        crate::did::resolve_did_key(&subject).map_err(|_| RecordError::UnresolvableSubject)?;

    let (header_b64, sig_b64) = signed
        .proof
        .jws
        .split_once("..")
        .ok_or_else(|| RecordError::MalformedProof("expected detached compact JWS".into()))?;

    let sig_bytes = crate::vc::sign::b64url_decode(sig_b64)
        .ok_or_else(|| RecordError::MalformedProof("signature is not base64url".into()))?;
    let sig_array: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| RecordError::MalformedProof("signature is not 64 bytes".into()))?;
    let sig = ed25519_dalek::Signature::from_bytes(&sig_array);

    let input = signing_input(&signed.record, header_b64).map_err(RecordError::MalformedProof)?;

    key.verify(&input, &sig)
        .map_err(|_| RecordError::BadSignature)
}
