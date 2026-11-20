//! Device attestation evidence: what it is, what it binds, and how a
//! verifier checks it.
//!
//! Mobile platforms can vouch that a genuine, unmodified build of the app on
//! a genuine device produced a given value. Two families are supported:
//!
//! * **iOS App Attest**: `DCAppAttestService` attests a per-session Secure
//!   Enclave key over the session nonce; the same key later signs values
//!   (assertions). Checked against Apple's App Attestation Root CA.
//! * **Android Play Integrity**: Google returns an encrypted token carrying
//!   the verdict for a nonce. Opened with the developer's response keys.
//!
//! A credential carries the platform's material as one typed entry of its
//! standard `evidence` property ([`AttestationEvidence`]). What the material
//! covers is its **binding** ([`BindingKind`]):
//!
//! * *credential-bound*: platform evidence over this credential's
//!   [`binding_digest`] (subject, claim, integrity assertion and commitment
//!   root). Evidence lifted onto another credential fails.
//! * *session-bound*: only the session's own attestation, over the session
//!   nonce (and, on iOS, an assertion over the terminal commitment root).
//!
//! [`verify_evidence`] reports an [`AttestationOutcome`]; only verified
//! credential-bound evidence raises the [`VerifiedAssuranceLevel`]: to
//! `device_attested` when the attempt's own device produced it, to
//! `companion_attested` when it carries a [`CompanionBinding`] (a paired
//! phone was present and attested; nothing is claimed about the desktop).
//! Invalid evidence rejects the credential; every other outcome leaves it to
//! the rest of verification.
//!
//! Trust inputs ([`AttestationTrust`]: Apple root, app identifiers, Play
//! Integrity response keys, development-build allowance) come from the
//! caller. Nothing here performs I/O. The cryptographic checks are behind the
//! `attestation` cargo feature; without it evidence entries still parse and
//! verify as `unverifiable`.

#[cfg(feature = "attestation")]
pub mod app_attest;
#[cfg(feature = "attestation")]
pub mod play_integrity;
#[cfg(any(feature = "test-support", all(test, feature = "attestation")))]
pub mod test_support;
#[cfg(feature = "attestation")]
mod time;

use base64::Engine as _;
use ed25519_dalek::Verifier as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::vc::{CredentialSubject, EvidenceEntry, IntegrityAssertion, VerifiableCredential};

/// `type` of the attestation entry in a credential's `evidence`.
pub const ATTESTATION_EVIDENCE_TYPE: &str = "DeviceAttestationEvidence";

/// Domain tag of [`binding_digest`]. A new tag is a new binding format.
pub const BINDING_DIGEST_TAG: &str = "alexandria-attestation-binding-v1";

const SESSION_NONCE_TAG: &[u8] = b"alexandria-device-attest-v1\0";
const COMPANION_NONCE_TAG: &[u8] = b"alexandria-companion-attest-v1\0";
const COMPANION_STATEMENT_TAG: &str = "alexandria-companion-statement-v1";

/// Apple App Attestation Root CA (public, from
/// <https://www.apple.com/certificateauthority/>). SHA-256 fingerprint
/// `1CB9823BA28BA6AD2D33A006941DE2AE4F513EF1D4E831B9F7E0FA7B6242C932`,
/// valid 2020-03-18 to 2045-03-15. Callers pass it as [`AppleTrust::root_pem`].
pub const APPLE_APP_ATTEST_ROOT_PEM: &str = "-----BEGIN CERTIFICATE-----
MIICITCCAaegAwIBAgIQC/O+DvHN0uD7jG5yH2IXmDAKBggqhkjOPQQDAzBSMSYw
JAYDVQQDDB1BcHBsZSBBcHAgQXR0ZXN0YXRpb24gUm9vdCBDQTETMBEGA1UECgwK
QXBwbGUgSW5jLjETMBEGA1UECAwKQ2FsaWZvcm5pYTAeFw0yMDAzMTgxODMyNTNa
Fw00NTAzMTUwMDAwMDBaMFIxJjAkBgNVBAMMHUFwcGxlIEFwcCBBdHRlc3RhdGlv
biBSb290IENBMRMwEQYDVQQKDApBcHBsZSBJbmMuMRMwEQYDVQQIDApDYWxpZm9y
bmlhMHYwEAYHKoZIzj0CAQYFK4EEACIDYgAERTHhmLW07ATaFQIEVwTtT4dyctdh
NbJhFs/Ii2FdCgAHGbpphY3+d8qjuDngIN3WVhQUBHAoMeQ/cLiP1sOUtgjqK9au
Yen1mMEvRq9Sk3Jm5X8U62H+xTD3FE9TgS41o0IwQDAPBgNVHRMBAf8EBTADAQH/
MB0GA1UdDgQWBBSskRBTM72+aEH/pwyp5frq5eWKoTAOBgNVHQ8BAf8EBAMCAQYw
CgYIKoZIzj0EAwMDaAAwZQIwQgFGnByvsiVbpTKwSga0kP0e8EeDS4+sQmTvb7vn
53O5+FRXgeLhpJ06ysC5PrOyAjEAp5U4xDgEgllF7En3VcE3iexZZtKeYnpqtijV
oyFraWVIyd/dganmrduC1bmTBGwD
-----END CERTIFICATE-----
";

// ---------------------------------------------------------------------------
// The stored attestation record
// ---------------------------------------------------------------------------

/// Which platform produced the attestation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Ios,
    Android,
}

/// The platform-specific blob(s) of a session's attestation. Opaque until
/// verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttestationPayload {
    /// `DCAppAttestService` output: the key id and the attestation object
    /// from session start, plus the assertion over the terminal commitment
    /// root from session end when the session ended cleanly.
    AppAttest {
        key_id_b64: String,
        attestation_b64: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        assertion_b64: Option<String>,
        /// The commitment root the assertion signs, as stored at end.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        asserted_root: Option<String>,
    },
    /// Play Integrity token (compact JWE) for the session nonce.
    PlayIntegrity { token: String },
}

/// A session's device attestation as the issuing device stores it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceAttestation {
    pub platform: Platform,
    /// Hex of the attested nonce at capture time; verifiers recompute and
    /// compare rather than trusting this field.
    pub nonce_hex: String,
    /// `TEAMID.bundle` (iOS) or the package name (Android).
    pub app_id: String,
    pub payload: AttestationPayload,
    pub captured_at: String,
    /// Present when a paired phone produced this attestation for a desktop
    /// session. The attested nonce is then [`companion_nonce`], bound to the
    /// learner and the phone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub companion: Option<CompanionBinding>,
}

/// Who a paired phone's attestation vouches for and which phone delivered it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanionBinding {
    pub learner_did: String,
    /// The phone's iroh endpoint id (hex), as authenticated by the
    /// connection that delivered the attestation.
    pub phone_endpoint: String,
    /// Ed25519 signature by the learner DID key over [`companion_statement`].
    pub did_signature_b64: String,
}

/// The challenge both platforms attest over at session start. Derived, not
/// random, so a verifier recomputes it from the session id and start instant.
pub fn session_nonce(session_id: &str, started_at: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(SESSION_NONCE_TAG);
    h.update(session_id.as_bytes());
    h.update(b"\0");
    h.update(started_at.as_bytes());
    h.finalize().into()
}

/// The nonce a paired phone attests: the desktop session's nonce bound to
/// the learner and to the phone's transport identity.
pub fn companion_nonce(
    session_nonce: &[u8; 32],
    learner_did: &str,
    phone_endpoint: &str,
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(COMPANION_NONCE_TAG);
    h.update(session_nonce);
    h.update(learner_did.as_bytes());
    h.update(b"\0");
    h.update(phone_endpoint.as_bytes());
    h.finalize().into()
}

/// The bytes a companion's learner DID signs: the binding's subject and
/// phone, the attested nonce, and a digest of the start-of-session
/// attestation material (a later assertion is not covered).
pub fn companion_statement(
    record: &DeviceAttestation,
    learner_did: &str,
    phone_endpoint: &str,
) -> Vec<u8> {
    let start_material = match &record.payload {
        AttestationPayload::AppAttest {
            key_id_b64,
            attestation_b64,
            ..
        } => format!("app_attest|{key_id_b64}|{attestation_b64}"),
        AttestationPayload::PlayIntegrity { token } => format!("play_integrity|{token}"),
    };
    let digest = hex::encode(Sha256::digest(start_material.as_bytes()));
    format!(
        "{COMPANION_STATEMENT_TAG}\n{learner_did}\n{phone_endpoint}\n{}\n{}\n{digest}",
        record.app_id, record.nonce_hex
    )
    .into_bytes()
}

/// The nonce a companion record must attest, once the binding's learner
/// signature verifies over [`companion_statement`].
pub fn companion_bound_nonce(
    record: &DeviceAttestation,
    binding: &CompanionBinding,
    session_nonce: &[u8; 32],
) -> Result<[u8; 32], Unverified> {
    let failed = |reason: &str| Unverified::failed(InvalidReason::CompanionMismatch, reason);
    let did = crate::did::parse_did_key(&binding.learner_did)
        .map_err(|_| failed("companion: binding names no did:key learner"))?;
    let key = crate::did::resolve_did_key(&did)
        .map_err(|_| failed("companion: binding names no did:key learner"))?;
    let signature = base64::engine::general_purpose::STANDARD
        .decode(binding.did_signature_b64.trim())
        .ok()
        .and_then(|b| ed25519_dalek::Signature::from_slice(&b).ok())
        .ok_or_else(|| failed("companion: learner signature is malformed"))?;
    key.verify(
        &companion_statement(record, &binding.learner_did, &binding.phone_endpoint),
        &signature,
    )
    .map_err(|_| failed("companion: learner signature does not verify"))?;
    Ok(companion_nonce(
        session_nonce,
        &binding.learner_did,
        &binding.phone_endpoint,
    ))
}

// ---------------------------------------------------------------------------
// Trust inputs
// ---------------------------------------------------------------------------

/// Play Integrity response keys from the Play Console ("Manage and download
/// my response encryption keys"). Base64 as the console hands them out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayIntegrityKeys {
    pub decryption_key_b64: String,
    pub verification_key_b64: String,
}

/// What a verifier trusts for App Attest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppleTrust {
    /// PEM of the App Attestation Root CA, normally
    /// [`APPLE_APP_ATTEST_ROOT_PEM`].
    pub root_pem: String,
    /// The App Attest app id, `TEAMID.bundleid`.
    pub app_id: String,
    /// Accept the `appattestdevelop` environment (development builds).
    #[serde(default)]
    pub allow_development: bool,
}

/// What a verifier trusts for Play Integrity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayIntegrityTrust {
    /// The Android application id the verdict must name.
    pub package_name: String,
    pub keys: PlayIntegrityKeys,
}

/// Trust inputs for attestation checking, supplied by the caller. A platform
/// left out makes its evidence `unverifiable`, never invalid.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttestationTrust {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apple: Option<AppleTrust>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub play_integrity: Option<PlayIntegrityTrust>,
}

// ---------------------------------------------------------------------------
// Outcomes
// ---------------------------------------------------------------------------

/// Stable reason codes for invalid attestation evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvalidReason {
    /// The entry, or a blob inside it, does not parse, or its material does
    /// not match its platform.
    MalformedEvidence,
    /// More than one attestation entry.
    DuplicateEvidence,
    /// Attestation evidence on a credential without an integrity assertion.
    MissingIntegrityAssertion,
    /// The certificate chain does not verify to the trusted root, or is
    /// outside its validity at the assertion's `generatedAt`.
    UntrustedChain,
    /// The platform attested a nonce other than this session's.
    WrongNonce,
    /// The evidence names an app other than the trusted one.
    WrongAppId,
    /// The attested key does not match its key id.
    KeyMismatch,
    /// An App Attest counter did not have the required value.
    Counter,
    /// An App Attest environment the trust inputs do not accept.
    EnvironmentNotAccepted,
    /// A signature or authenticated decryption failed under the trust inputs.
    InvalidSignature,
    /// Play did not recognise the app or the device.
    DeviceIntegrity,
    /// The evidence does not cover what its binding claims: a credential
    /// binding not over this credential's [`binding_digest`], or a session
    /// assertion over another commitment root.
    BindingMismatch,
    /// A companion binding names another learner than the subject, or its
    /// learner signature does not verify.
    CompanionMismatch,
}

impl InvalidReason {
    /// The snake_case wire token.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MalformedEvidence => "malformed_evidence",
            Self::DuplicateEvidence => "duplicate_evidence",
            Self::MissingIntegrityAssertion => "missing_integrity_assertion",
            Self::UntrustedChain => "untrusted_chain",
            Self::WrongNonce => "wrong_nonce",
            Self::WrongAppId => "wrong_app_id",
            Self::KeyMismatch => "key_mismatch",
            Self::Counter => "counter",
            Self::EnvironmentNotAccepted => "environment_not_accepted",
            Self::InvalidSignature => "invalid_signature",
            Self::DeviceIntegrity => "device_integrity",
            Self::BindingMismatch => "binding_mismatch",
            Self::CompanionMismatch => "companion_mismatch",
        }
    }
}

/// What a verifier established about a credential's attestation evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AttestationOutcome {
    /// The credential carries no attestation evidence: self-reported.
    #[default]
    NotSupplied,
    /// The platform vouched for this credential's own content. `companion`
    /// is true when a paired phone produced the evidence for a desktop
    /// session (the entry carries a [`CompanionBinding`]); omitted on the
    /// wire when false.
    VerifiedCredentialBound {
        platform: Platform,
        #[serde(default, skip_serializing_if = "is_false")]
        companion: bool,
    },
    /// The platform vouched for the session, not for this credential.
    VerifiedSessionBound { platform: Platform },
    /// Evidence is present but cannot be checked here: no trust inputs for
    /// its platform, or the library was built without `attestation`.
    Unverifiable { platform: Platform },
    /// The evidence fails a check. Rejects the credential.
    Invalid { reason: InvalidReason },
}

impl AttestationOutcome {
    /// Whether this outcome rejects the credential. Only `invalid` does.
    pub fn rejects(&self) -> bool {
        matches!(self, Self::Invalid { .. })
    }

    /// The assurance level this outcome establishes for a verifier:
    /// `device_attested` for credential-bound evidence from the attempt's own
    /// device, `companion_attested` for credential-bound evidence from a
    /// paired phone, `local` otherwise.
    pub fn verified_assurance_level(&self) -> VerifiedAssuranceLevel {
        match self {
            Self::VerifiedCredentialBound {
                companion: false, ..
            } => VerifiedAssuranceLevel::DeviceAttested,
            Self::VerifiedCredentialBound {
                companion: true, ..
            } => VerifiedAssuranceLevel::CompanionAttested,
            _ => VerifiedAssuranceLevel::Local,
        }
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// The assurance level a verifier established from the evidence, as opposed
/// to the `assuranceLevel` the integrity assertion states. Ordered
/// `local` < `companion_attested` < `device_attested`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum VerifiedAssuranceLevel {
    /// Nothing beyond the issuing device's word.
    #[default]
    Local,
    /// Verified credential-bound evidence from a paired phone backing a
    /// desktop session: a genuine phone was present and attested. Nothing is
    /// claimed about the desktop or the software it ran.
    CompanionAttested,
    /// Verified credential-bound evidence from the device the attempt ran on.
    DeviceAttested,
}

impl VerifiedAssuranceLevel {
    /// Every level, weakest first.
    pub const ALL: [Self; 3] = [Self::Local, Self::CompanionAttested, Self::DeviceAttested];

    /// The wire token, matching the assertion's `assuranceLevel` values.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::CompanionAttested => "companion_attested",
            Self::DeviceAttested => "device_attested",
        }
    }

    /// The level a wire token names; `None` for the reserved `anchored` and
    /// `high_assurance` and anything unknown.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|level| level.as_str() == s)
    }
}

/// Why a stored attestation record did not verify.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum Unverified {
    /// No trust inputs for the record's platform.
    NoTrustMaterial,
    /// The record is malformed or a proof fails.
    Failed { code: InvalidReason, reason: String },
}

impl Unverified {
    pub fn failed(code: InvalidReason, reason: impl Into<String>) -> Self {
        Self::Failed {
            code,
            reason: reason.into(),
        }
    }
}

/// What a verified attestation record establishes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Verified {
    pub platform: Platform,
    /// `appattest` / `appattestdevelop` on iOS; the device verdict labels
    /// on Android.
    pub environment: String,
    /// True when the terminal commitment root is covered (iOS assertion
    /// present and valid).
    pub covers_commitment: bool,
}

// ---------------------------------------------------------------------------
// The evidence entry
// ---------------------------------------------------------------------------

/// What the attestation evidence covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingKind {
    /// Platform evidence over this credential's [`binding_digest`].
    Credential,
    /// Only the session's own attestation.
    Session,
}

/// The platform material of an [`AttestationEvidence`] entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum EvidenceMaterial {
    AppAttest {
        /// Standard base64 of the key id, exactly as the session stored it.
        key_id: String,
        /// Standard base64 of the attestation object over the session (or
        /// companion) nonce.
        attestation: String,
        /// Assertion over the integrity assertion's commitment root, when the
        /// session ended with one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_assertion: Option<String>,
        /// Credential binding: assertion whose client data is the 32 bytes
        /// of the [`binding_digest`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        binding_assertion: Option<String>,
    },
    PlayIntegrity {
        /// Token over the session (or companion) nonce.
        session_token: String,
        /// Credential binding: token whose nonce is the [`binding_digest`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        binding_token: Option<String>,
    },
}

/// The attestation entry of a credential's `evidence`: platform, material,
/// binding kind and, for a paired phone, the companion binding.
///
/// Unknown fields are ignored, so a later format can add fields without
/// turning older verifiers' results into rejections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttestationEvidence {
    /// Always [`ATTESTATION_EVIDENCE_TYPE`].
    #[serde(rename = "type")]
    pub type_: String,
    pub platform: Platform,
    pub binding: BindingKind,
    /// The session's start instant. With the integrity assertion's session
    /// id it gives the [`session_nonce`] the platform attested.
    pub session_started_at: String,
    /// `TEAMID.bundle` (iOS) or the package name (Android).
    pub app_id: String,
    pub material: EvidenceMaterial,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub companion: Option<CompanionBinding>,
}

impl AttestationEvidence {
    /// Session-bound evidence: the session's stored record as it is.
    pub fn session_bound(record: &DeviceAttestation, session_started_at: &str) -> Self {
        Self::build(record, session_started_at, BindingKind::Session, None)
    }

    /// Credential-bound evidence: the session's stored record plus
    /// `binding_proof` over this credential's [`binding_digest`]: on iOS a
    /// standard-base64 assertion from the session's attested key, on Android
    /// a Play Integrity token requested with the digest as nonce.
    pub fn credential_bound(
        record: &DeviceAttestation,
        session_started_at: &str,
        binding_proof: String,
    ) -> Self {
        Self::build(
            record,
            session_started_at,
            BindingKind::Credential,
            Some(binding_proof),
        )
    }

    fn build(
        record: &DeviceAttestation,
        session_started_at: &str,
        binding: BindingKind,
        binding_proof: Option<String>,
    ) -> Self {
        let material = match &record.payload {
            AttestationPayload::AppAttest {
                key_id_b64,
                attestation_b64,
                assertion_b64,
                ..
            } => EvidenceMaterial::AppAttest {
                key_id: key_id_b64.clone(),
                attestation: attestation_b64.clone(),
                session_assertion: assertion_b64.clone(),
                binding_assertion: binding_proof,
            },
            AttestationPayload::PlayIntegrity { token } => EvidenceMaterial::PlayIntegrity {
                session_token: token.clone(),
                binding_token: binding_proof,
            },
        };
        Self {
            type_: ATTESTATION_EVIDENCE_TYPE.into(),
            platform: record.platform,
            binding,
            session_started_at: session_started_at.into(),
            app_id: record.app_id.clone(),
            material,
            companion: record.companion.clone(),
        }
    }

    /// This entry as a member of a credential's `evidence`.
    pub fn into_entry(self) -> EvidenceEntry {
        match serde_json::to_value(self).expect("attestation evidence serializes") {
            serde_json::Value::Object(map) => EvidenceEntry(map),
            _ => unreachable!("attestation evidence serializes to an object"),
        }
    }

    /// The credential's attestation entry: `Ok(None)` when it has none, an
    /// invalid reason when it has several or the one it has does not parse.
    pub fn find(credential: &VerifiableCredential) -> Result<Option<Self>, InvalidReason> {
        let mut entries = credential
            .evidence
            .iter()
            .filter(|e| e.has_type(ATTESTATION_EVIDENCE_TYPE));
        let Some(entry) = entries.next() else {
            return Ok(None);
        };
        if entries.next().is_some() {
            return Err(InvalidReason::DuplicateEvidence);
        }
        serde_json::from_value(serde_json::Value::Object(entry.0.clone()))
            .map(Some)
            .map_err(|_| InvalidReason::MalformedEvidence)
    }

    /// The session record this entry carries, with `nonce` as its nonce and
    /// `commitment_root` as the root its session assertion signs.
    pub fn session_record(
        &self,
        nonce: &[u8; 32],
        commitment_root: Option<&str>,
    ) -> DeviceAttestation {
        let payload = match &self.material {
            EvidenceMaterial::AppAttest {
                key_id,
                attestation,
                session_assertion,
                ..
            } => AttestationPayload::AppAttest {
                key_id_b64: key_id.clone(),
                attestation_b64: attestation.clone(),
                assertion_b64: session_assertion.clone(),
                asserted_root: session_assertion
                    .as_ref()
                    .and(commitment_root.map(str::to_string)),
            },
            EvidenceMaterial::PlayIntegrity { session_token, .. } => {
                AttestationPayload::PlayIntegrity {
                    token: session_token.clone(),
                }
            }
        };
        DeviceAttestation {
            platform: self.platform,
            nonce_hex: hex::encode(nonce),
            app_id: self.app_id.clone(),
            payload,
            captured_at: self.session_started_at.clone(),
            companion: self.companion.clone(),
        }
    }

    /// Whether the material carries a proof over a binding digest.
    pub fn has_binding_proof(&self) -> bool {
        match &self.material {
            EvidenceMaterial::AppAttest {
                binding_assertion, ..
            } => binding_assertion.is_some(),
            EvidenceMaterial::PlayIntegrity { binding_token, .. } => binding_token.is_some(),
        }
    }
}

// ---------------------------------------------------------------------------
// Binding digest
// ---------------------------------------------------------------------------

/// Why a binding digest could not be computed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BindingDigestError {
    #[error("the credential has no integrity assertion")]
    MissingIntegrityAssertion,
    #[error("canonicalization failed: {0}")]
    Canonicalize(String),
}

/// The digest credential-bound evidence covers: SHA-256 of the JCS
/// (RFC 8785, as the proof uses) canonical form of
///
/// ```json
/// { "tag": "alexandria-attestation-binding-v1",
///   "commitmentRoot": <integrity.commitmentRoot or null>,
///   "subject": <credentialSubject.id>,
///   "claim": <credentialSubject without id>,
///   "integrity": <the integrity assertion> }
/// ```
///
/// Issuers obtain platform evidence over it before signing; verifiers
/// recompute it from the credential. Issuer, type, id and validity are not
/// covered, so the digest is the same for any credential stating this
/// subject, claim and integrity assertion.
pub fn binding_digest(credential: &VerifiableCredential) -> Result<[u8; 32], BindingDigestError> {
    let integrity = credential
        .integrity
        .as_ref()
        .ok_or(BindingDigestError::MissingIntegrityAssertion)?;
    binding_digest_of(&credential.credential_subject, integrity)
}

/// [`binding_digest`] from its parts, for an issuer that has not built the
/// credential yet.
pub fn binding_digest_of(
    subject: &CredentialSubject,
    integrity: &IntegrityAssertion,
) -> Result<[u8; 32], BindingDigestError> {
    let canonical_error = |e: crate::vc::VcError| BindingDigestError::Canonicalize(e.to_string());
    let integrity_value = serde_json::to_value(integrity)
        .map_err(|e| BindingDigestError::Canonicalize(e.to_string()))?;
    let value = serde_json::json!({
        "tag": BINDING_DIGEST_TAG,
        "commitmentRoot": integrity.commitment_root,
        "subject": subject.id.as_str(),
        "claim": subject.properties,
        "integrity": integrity_value,
    });
    let canonical = crate::vc::canonicalize::canonicalize(&value).map_err(canonical_error)?;
    Ok(Sha256::digest(&canonical).into())
}

// ---------------------------------------------------------------------------
// Verification
// ---------------------------------------------------------------------------

/// Check a credential's attestation evidence against `trust`.
///
/// Not supplied when the credential has no attestation entry; invalid when
/// the entry is duplicated, malformed or fails a check; unverifiable when
/// `trust` has nothing for its platform (or without the `attestation`
/// feature); otherwise verified with the binding it proved. Certificate
/// validity is judged at the integrity assertion's `generatedAt`, the
/// instant the issuing device verified the same evidence, so the outcome
/// does not change with the verification time.
pub fn verify_evidence(
    credential: &VerifiableCredential,
    trust: &AttestationTrust,
) -> AttestationOutcome {
    let evidence = match AttestationEvidence::find(credential) {
        Ok(None) => return AttestationOutcome::NotSupplied,
        Ok(Some(evidence)) => evidence,
        Err(reason) => return AttestationOutcome::Invalid { reason },
    };
    #[cfg(feature = "attestation")]
    {
        checks::verify(credential, &evidence, trust)
    }
    #[cfg(not(feature = "attestation"))]
    {
        let _ = trust;
        AttestationOutcome::Unverifiable {
            platform: evidence.platform,
        }
    }
}

/// Verify a stored attestation record against the nonce it must attest.
///
/// `commitment_root` is the session's terminal root (an iOS assertion over it
/// is checked when present). `at_unix` is the instant certificate validity is
/// judged at.
#[cfg(feature = "attestation")]
pub fn verify_record(
    record: &DeviceAttestation,
    expected_nonce: &[u8; 32],
    commitment_root: Option<&str>,
    trust: &AttestationTrust,
    at_unix: i64,
) -> Result<Verified, Unverified> {
    checks::record(record, expected_nonce, commitment_root, trust, at_unix).map(|c| c.verified)
}

#[cfg(feature = "attestation")]
mod checks {
    use super::*;

    /// A verified record, with what a binding assertion is checked against.
    pub(super) struct RecordCheck {
        pub verified: Verified,
        /// iOS: the attested key, the app id hash and the last counter seen.
        pub ios_key: Option<(Vec<u8>, [u8; 32], u32)>,
    }

    fn b64(s: &str) -> Result<Vec<u8>, Unverified> {
        base64::engine::general_purpose::STANDARD
            .decode(s.trim())
            .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s.trim()))
            .map_err(|e| {
                Unverified::failed(
                    InvalidReason::MalformedEvidence,
                    format!("attestation field is not base64: {e}"),
                )
            })
    }

    pub(super) fn record(
        att: &DeviceAttestation,
        expected_nonce: &[u8; 32],
        commitment_root: Option<&str>,
        trust: &AttestationTrust,
        at_unix: i64,
    ) -> Result<RecordCheck, Unverified> {
        use InvalidReason as R;
        if att.nonce_hex != hex::encode(expected_nonce) {
            return Err(Unverified::failed(
                R::WrongNonce,
                "attestation nonce does not match the session",
            ));
        }
        match (&att.platform, &att.payload) {
            (
                Platform::Ios,
                AttestationPayload::AppAttest {
                    key_id_b64,
                    attestation_b64,
                    assertion_b64,
                    asserted_root,
                },
            ) => {
                let Some(apple) = &trust.apple else {
                    return Err(Unverified::NoTrustMaterial);
                };
                if att.app_id != apple.app_id {
                    return Err(Unverified::failed(
                        R::WrongAppId,
                        format!("attestation app id {} is not the trusted one", att.app_id),
                    ));
                }
                let key_id = b64(key_id_b64)?;
                let attestation = b64(attestation_b64)?;
                let app_id_hash: [u8; 32] = Sha256::digest(att.app_id.as_bytes()).into();
                let attested = app_attest::verify_attestation(
                    &attestation,
                    &key_id,
                    expected_nonce,
                    &app_id_hash,
                    &apple.root_pem,
                    apple.allow_development,
                    at_unix,
                )
                .map_err(|e| Unverified::failed(e.reason, format!("app attest: {e}")))?;
                let mut counter = attested.counter;
                let covers_commitment = match (commitment_root, assertion_b64, asserted_root) {
                    (Some(root), Some(assertion), Some(asserted)) => {
                        if asserted != root {
                            return Err(Unverified::failed(
                                R::BindingMismatch,
                                "assertion covers a different commitment root",
                            ));
                        }
                        let assertion = b64(assertion)?;
                        counter = app_attest::verify_assertion(
                            &assertion,
                            root.as_bytes(),
                            &attested.public_key_sec1,
                            &app_id_hash,
                            attested.counter,
                        )
                        .map_err(|e| {
                            Unverified::failed(e.reason, format!("app attest assertion: {e}"))
                        })?;
                        true
                    }
                    (_, None, _) | (None, _, _) => false,
                    (Some(_), Some(_), None) => {
                        return Err(Unverified::failed(
                            R::MalformedEvidence,
                            "assertion present without the root it signs",
                        ))
                    }
                };
                Ok(RecordCheck {
                    verified: Verified {
                        platform: Platform::Ios,
                        environment: attested.environment,
                        covers_commitment,
                    },
                    ios_key: Some((attested.public_key_sec1, app_id_hash, counter)),
                })
            }
            (Platform::Android, AttestationPayload::PlayIntegrity { token }) => {
                let Some(play) = &trust.play_integrity else {
                    return Err(Unverified::NoTrustMaterial);
                };
                if att.app_id != play.package_name {
                    return Err(Unverified::failed(
                        R::WrongAppId,
                        format!("attestation package {} is not the trusted one", att.app_id),
                    ));
                }
                let verdict = play_integrity::decrypt_and_verify(token, &play.keys)
                    .map_err(|e| Unverified::failed(e.reason, format!("play integrity: {e}")))?;
                play_integrity::check_verdict(&verdict, expected_nonce, &play.package_name)
                    .map_err(|e| {
                        Unverified::failed(e.reason, format!("play integrity verdict: {e}"))
                    })?;
                Ok(RecordCheck {
                    verified: Verified {
                        platform: Platform::Android,
                        environment: play_integrity::device_labels(&verdict).join(","),
                        covers_commitment: false,
                    },
                    ios_key: None,
                })
            }
            _ => Err(Unverified::failed(
                R::MalformedEvidence,
                "attestation payload does not match its platform",
            )),
        }
    }

    pub(super) fn verify(
        credential: &VerifiableCredential,
        evidence: &AttestationEvidence,
        trust: &AttestationTrust,
    ) -> AttestationOutcome {
        match verify_inner(credential, evidence, trust) {
            Ok(outcome) => outcome,
            Err(Unverified::NoTrustMaterial) => AttestationOutcome::Unverifiable {
                platform: evidence.platform,
            },
            Err(Unverified::Failed { code, .. }) => AttestationOutcome::Invalid { reason: code },
        }
    }

    fn invalid(reason: InvalidReason) -> Unverified {
        Unverified::failed(reason, reason.as_str())
    }

    fn verify_inner(
        credential: &VerifiableCredential,
        evidence: &AttestationEvidence,
        trust: &AttestationTrust,
    ) -> Result<AttestationOutcome, Unverified> {
        use InvalidReason as R;
        let integrity = credential
            .integrity
            .as_ref()
            .ok_or_else(|| invalid(R::MissingIntegrityAssertion))?;
        let at_unix = super::time::rfc3339_to_unix(&integrity.generated_at)
            .ok_or_else(|| invalid(R::MalformedEvidence))?;
        let root = integrity.commitment_root.as_deref();
        let session = session_nonce(&integrity.session_id, &evidence.session_started_at);
        let expected = match &evidence.companion {
            None => session,
            Some(binding) => {
                if binding.learner_did != credential.credential_subject.id.as_str() {
                    return Err(invalid(R::CompanionMismatch));
                }
                let nonce =
                    companion_nonce(&session, &binding.learner_did, &binding.phone_endpoint);
                companion_bound_nonce(&evidence.session_record(&nonce, root), binding, &session)?
            }
        };
        let record = evidence.session_record(&expected, root);
        let checked = super::checks::record(&record, &expected, root, trust, at_unix)?;
        let platform = evidence.platform;
        match evidence.binding {
            BindingKind::Session if evidence.has_binding_proof() => {
                Err(invalid(R::MalformedEvidence))
            }
            BindingKind::Session => Ok(AttestationOutcome::VerifiedSessionBound { platform }),
            BindingKind::Credential => {
                let digest =
                    binding_digest(credential).map_err(|_| invalid(R::MalformedEvidence))?;
                match (&evidence.material, &checked.ios_key, &trust.play_integrity) {
                    (
                        EvidenceMaterial::AppAttest {
                            binding_assertion: Some(assertion),
                            ..
                        },
                        Some((key, app_id_hash, counter)),
                        _,
                    ) => {
                        app_attest::verify_assertion(
                            &b64(assertion)?,
                            &digest,
                            key,
                            app_id_hash,
                            *counter,
                        )
                        .map_err(|e| match e.reason {
                            R::InvalidSignature => invalid(R::BindingMismatch),
                            other => invalid(other),
                        })?;
                    }
                    (
                        EvidenceMaterial::PlayIntegrity {
                            binding_token: Some(token),
                            ..
                        },
                        _,
                        Some(play),
                    ) => {
                        let verdict = play_integrity::decrypt_and_verify(token, &play.keys)
                            .map_err(|e| invalid(e.reason))?;
                        play_integrity::check_verdict(&verdict, &digest, &play.package_name)
                            .map_err(|e| match e.reason {
                                R::WrongNonce => invalid(R::BindingMismatch),
                                other => invalid(other),
                            })?;
                    }
                    _ => return Err(invalid(R::BindingMismatch)),
                }
                Ok(AttestationOutcome::VerifiedCredentialBound {
                    platform,
                    companion: evidence.companion.is_some(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nonce_is_deterministic_and_session_specific() {
        let a = session_nonce("isess_1", "2026-10-10T00:00:00Z");
        assert_eq!(a, session_nonce("isess_1", "2026-10-10T00:00:00Z"));
        assert_ne!(a, session_nonce("isess_2", "2026-10-10T00:00:00Z"));
        assert_ne!(a, session_nonce("isess_1", "2026-10-10T00:00:01Z"));
    }

    #[test]
    fn stored_shape_round_trips_with_snake_case_tags() {
        let att = DeviceAttestation {
            platform: Platform::Ios,
            nonce_hex: "ab".repeat(32),
            app_id: "TEAM.bundle".into(),
            payload: AttestationPayload::AppAttest {
                key_id_b64: "a2V5".into(),
                attestation_b64: "YXR0".into(),
                assertion_b64: None,
                asserted_root: None,
            },
            captured_at: "2026-10-10T00:00:00Z".into(),
            companion: None,
        };
        let json = serde_json::to_string(&att).unwrap();
        assert!(json.contains("\"platform\":\"ios\""));
        assert!(json.contains("\"kind\":\"app_attest\""));
        assert!(!json.contains("assertion_b64"));
        let back: DeviceAttestation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, att);
    }

    #[test]
    fn outcome_and_level_wire_shapes_are_snake_case() {
        assert_eq!(
            serde_json::to_value(AttestationOutcome::Invalid {
                reason: InvalidReason::BindingMismatch
            })
            .unwrap(),
            serde_json::json!({"outcome": "invalid", "reason": "binding_mismatch"})
        );
        assert_eq!(
            serde_json::to_value(AttestationOutcome::VerifiedCredentialBound {
                platform: Platform::Ios,
                companion: false,
            })
            .unwrap(),
            serde_json::json!({"outcome": "verified_credential_bound", "platform": "ios"})
        );
        assert_eq!(
            serde_json::to_value(AttestationOutcome::VerifiedCredentialBound {
                platform: Platform::Ios,
                companion: true,
            })
            .unwrap(),
            serde_json::json!({
                "outcome": "verified_credential_bound",
                "platform": "ios",
                "companion": true
            })
        );
        for level in VerifiedAssuranceLevel::ALL {
            assert_eq!(
                serde_json::to_value(level).unwrap(),
                serde_json::json!(level.as_str())
            );
            assert_eq!(VerifiedAssuranceLevel::parse(level.as_str()), Some(level));
        }
        assert_eq!(
            VerifiedAssuranceLevel::ALL.map(|l| l.as_str()),
            ["local", "companion_attested", "device_attested"]
        );
        for reserved in ["anchored", "high_assurance", "unknown"] {
            assert_eq!(VerifiedAssuranceLevel::parse(reserved), None);
        }
        for reason in [
            InvalidReason::MalformedEvidence,
            InvalidReason::DuplicateEvidence,
            InvalidReason::MissingIntegrityAssertion,
            InvalidReason::UntrustedChain,
            InvalidReason::WrongNonce,
            InvalidReason::WrongAppId,
            InvalidReason::KeyMismatch,
            InvalidReason::Counter,
            InvalidReason::EnvironmentNotAccepted,
            InvalidReason::InvalidSignature,
            InvalidReason::DeviceIntegrity,
            InvalidReason::BindingMismatch,
            InvalidReason::CompanionMismatch,
        ] {
            assert_eq!(
                serde_json::to_value(reason).unwrap(),
                serde_json::json!(reason.as_str())
            );
        }
    }

    #[test]
    fn levels_are_ordered_local_companion_device() {
        use VerifiedAssuranceLevel::{CompanionAttested, DeviceAttested, Local};
        assert!(Local < CompanionAttested && CompanionAttested < DeviceAttested);
    }

    #[test]
    fn only_credential_bound_evidence_raises_the_verified_level() {
        let p = Platform::Ios;
        let cases = [
            (
                AttestationOutcome::NotSupplied,
                VerifiedAssuranceLevel::Local,
            ),
            (
                AttestationOutcome::VerifiedCredentialBound {
                    platform: p,
                    companion: false,
                },
                VerifiedAssuranceLevel::DeviceAttested,
            ),
            (
                AttestationOutcome::VerifiedCredentialBound {
                    platform: p,
                    companion: true,
                },
                VerifiedAssuranceLevel::CompanionAttested,
            ),
            (
                AttestationOutcome::VerifiedSessionBound { platform: p },
                VerifiedAssuranceLevel::Local,
            ),
            (
                AttestationOutcome::Unverifiable { platform: p },
                VerifiedAssuranceLevel::Local,
            ),
            (
                AttestationOutcome::Invalid {
                    reason: InvalidReason::WrongNonce,
                },
                VerifiedAssuranceLevel::Local,
            ),
        ];
        for (outcome, level) in cases {
            assert_eq!(outcome.verified_assurance_level(), level, "{outcome:?}");
            assert_eq!(
                outcome.rejects(),
                matches!(outcome, AttestationOutcome::Invalid { .. })
            );
        }
    }

    #[cfg(feature = "attestation")]
    #[test]
    fn apple_root_parses_and_matches_the_published_fingerprint() {
        let der = app_attest::pem_to_der(APPLE_APP_ATTEST_ROOT_PEM).unwrap();
        let fp = hex::encode(Sha256::digest(&der));
        assert_eq!(
            fp,
            "1cb9823ba28ba6ad2d33a006941de2ae4f513ef1d4e831b9f7e0fa7b6242c932"
        );
        let (_, cert) = x509_parser::parse_x509_certificate(&der).unwrap();
        assert!(cert.is_ca());
    }
}
