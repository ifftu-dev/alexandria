use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::json::{decode_untrusted, JsonLimits};

pub const DOCUMENT_CIRCUIT: &str = "anon-aadhaar-v2.0.0";
pub const VERIFICATION_KEY_SHA256: &str =
    "40f2ea24b56ffe2b6e6e3578053cbda15e773cb560697bac883a404077fcf177";
pub const SYNTHETIC_ISSUER: &str =
    "15134874015316324267425466444584014077184337590635665158241104437045239495873";
const SCALAR_MODULUS: &str =
    "21888242871839275222246405745257275088548364400416034343698204186575808495617";
const MAX_TIME: u64 = (1_u64 << 53) - 1;
const LIMITS: JsonLimits = JsonLimits {
    max_bytes: 32 * 1024,
    max_depth: 5,
    max_array_len: 32,
    max_object_entries: 24,
    max_string_bytes: 512,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PolicyError {
    #[error("real document policy is disabled")]
    Disabled,
    #[error("document policy must match the exact bytes pinned by the network")]
    PinMismatch,
    #[error("invalid document policy encoding or fields")]
    Invalid,
    #[error("document policy belongs to another network")]
    Network,
    #[error("document policy is not currently valid")]
    PolicyExpired,
    #[error("document circuit, key, seed, or disclosure policy mismatch")]
    Circuit,
    #[error("document issuer is unknown, disabled, revoked, or outside its validity window")]
    Issuer,
    #[error("document is outside the freshness window")]
    DocumentTime,
}

type Result<T> = std::result::Result<T, PolicyError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PolicyKind {
    LocalDocumentReceipt,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IssuerStatus {
    Active,
    Disabled,
    Revoked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DocumentIssuer {
    pub id: String,
    pub circuit_key_hash: String,
    pub certificate_sha256: String,
    pub certificate_source: String,
    pub valid_from: u64,
    pub valid_until: u64,
    pub status: IssuerStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DocumentPolicy {
    pub schema_version: u32,
    pub policy_version: u32,
    pub network_id: String,
    pub kind: PolicyKind,
    pub document_format: String,
    pub circuit_id: String,
    pub verification_key_sha256: String,
    pub valid_from: u64,
    pub valid_until: u64,
    pub nullifier_scope: String,
    pub nullifier_seed: String,
    pub max_document_age_seconds: u64,
    pub max_future_skew_seconds: u64,
    pub challenge_ttl_seconds: u64,
    pub receipt_ttl_seconds: u64,
    pub issuers: Vec<DocumentIssuer>,
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn scalar(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= SCALAR_MODULUS.len()
        && (value.len() == 1 || !value.starts_with('0'))
        && value.bytes().all(|b| b.is_ascii_digit())
        && (value.len() < SCALAR_MODULUS.len() || value < SCALAR_MODULUS)
}

fn interval(start: u64, end: u64) -> bool {
    start < end && end <= MAX_TIME
}

fn certificate_source(value: &str) -> bool {
    let path = value
        .strip_prefix("https://uidai.gov.in/")
        .or_else(|| value.strip_prefix("https://www.uidai.gov.in/"))
        .or_else(|| value.strip_prefix("https://backend.uidai.gov.in/"));
    path.is_some_and(|path| {
        !path.is_empty()
            && path.len() <= 400
            && path
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b))
            && path
                .split('/')
                .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
    })
}

impl DocumentPolicy {
    fn validate(&self, network: &str) -> Result<()> {
        if self.network_id != network {
            return Err(PolicyError::Network);
        }
        if self.schema_version != 1
            || self.policy_version == 0
            || !identifier(network)
            || self.document_format != "aadhaar_secure_qr_v2"
            || self.circuit_id != DOCUMENT_CIRCUIT
            || self.verification_key_sha256 != VERIFICATION_KEY_SHA256
            || !interval(self.valid_from, self.valid_until)
            || self.nullifier_scope != "network_personhood_v1"
            || !scalar(&self.nullifier_seed)
            || self.nullifier_seed == "0"
            || !(1..=604800).contains(&self.max_document_age_seconds)
            || self.max_future_skew_seconds > 300
            || !(1..=120).contains(&self.challenge_ttl_seconds)
            || !(1..=86400).contains(&self.receipt_ttl_seconds)
            || self.issuers.is_empty()
            || self.issuers.len() > 32
        {
            return Err(PolicyError::Invalid);
        }
        let mut ids = HashSet::new();
        let mut keys = HashSet::new();
        let mut certificates = HashSet::new();
        for issuer in &self.issuers {
            if !identifier(&issuer.id)
                || !ids.insert(&issuer.id)
                || !scalar(&issuer.circuit_key_hash)
                || issuer.circuit_key_hash == "0"
                || issuer.circuit_key_hash == SYNTHETIC_ISSUER
                || !keys.insert(&issuer.circuit_key_hash)
                || !digest(&issuer.certificate_sha256)
                || !certificates.insert(&issuer.certificate_sha256)
                || !certificate_source(&issuer.certificate_source)
                || !interval(issuer.valid_from, issuer.valid_until)
            {
                return Err(PolicyError::Invalid);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PinnedDocumentPolicy {
    policy: Option<DocumentPolicy>,
    digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuerWindow {
    pub policy_digest: String,
    pub receipt_expires_at: u64,
    pub challenge_ttl_seconds: u64,
}

impl PinnedDocumentPolicy {
    pub fn from_pinned(
        document: Option<&[u8]>,
        pinned_digest: Option<&str>,
        network: &str,
    ) -> Result<Self> {
        match (document, pinned_digest) {
            (None, None) if identifier(network) => Ok(Self {
                policy: None,
                digest: None,
            }),
            (Some(bytes), Some(pin)) => {
                if bytes.len() > LIMITS.max_bytes
                    || !digest(pin)
                    || hex::encode(Sha256::digest(bytes)) != pin
                {
                    return Err(PolicyError::PinMismatch);
                }
                let policy: DocumentPolicy =
                    decode_untrusted(bytes, &LIMITS).map_err(|_| PolicyError::Invalid)?;
                if serde_json_canonicalizer::to_vec(&policy).map_err(|_| PolicyError::Invalid)?
                    != bytes
                {
                    return Err(PolicyError::Invalid);
                }
                policy.validate(network)?;
                Ok(Self {
                    policy: Some(policy),
                    digest: Some(pin.to_owned()),
                })
            }
            _ => Err(PolicyError::PinMismatch),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.policy.is_some()
    }

    // Checks issuer policy only. The caller must still verify proof, account,
    // challenge, and replay state and repeat this check at acceptance time.
    pub fn check_public_signals(
        &self,
        circuit: &str,
        signals: &[String; 9],
        now: u64,
    ) -> Result<IssuerWindow> {
        let policy = self.policy.as_ref().ok_or(PolicyError::Disabled)?;
        if now < policy.valid_from || now >= policy.valid_until {
            return Err(PolicyError::PolicyExpired);
        }
        if circuit != policy.circuit_id
            || signals.iter().any(|value| !scalar(value))
            || signals[7] != policy.nullifier_seed
            || signals[3..7].iter().any(|value| value != "0")
        {
            return Err(PolicyError::Circuit);
        }
        let issuer = policy
            .issuers
            .iter()
            .find(|issuer| issuer.circuit_key_hash == signals[0])
            .ok_or(PolicyError::Issuer)?;
        if issuer.status != IssuerStatus::Active
            || now < issuer.valid_from
            || now >= issuer.valid_until
        {
            return Err(PolicyError::Issuer);
        }
        let timestamp: u64 = signals[2].parse().map_err(|_| PolicyError::DocumentTime)?;
        let document_expiry = timestamp
            .checked_add(policy.max_document_age_seconds)
            .ok_or(PolicyError::DocumentTime)?;
        if timestamp < issuer.valid_from
            || timestamp >= issuer.valid_until
            || timestamp > now.saturating_add(policy.max_future_skew_seconds)
            || now >= document_expiry
        {
            return Err(PolicyError::DocumentTime);
        }
        let receipt_expires_at = now
            .checked_add(policy.receipt_ttl_seconds)
            .ok_or(PolicyError::DocumentTime)?
            .min(document_expiry)
            .min(policy.valid_until)
            .min(issuer.valid_until);
        Ok(IssuerWindow {
            policy_digest: self.digest.clone().ok_or(PolicyError::PinMismatch)?,
            receipt_expires_at,
            challenge_ttl_seconds: policy.challenge_ttl_seconds,
        })
    }
}
