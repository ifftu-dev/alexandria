//! Hosted attempts, the device's side: an assessment the organisation's
//! hosted service runs, in which the device only shows the items.
//!
//! The service draws, sends one item at a time, times each on its own clock,
//! grades and runs probes; the device never holds an answer key and never
//! computes a score (ADR 0009). [`HostedService`] speaks the candidate API,
//! every request signed with the candidate's key as the other holder fetches
//! are ([`crate::cloud::proof`]).
//!
//! What the device keeps:
//!
//! - **Item text** is held in memory while it is shown and passed to the
//!   screen. It is never written to the profile database or to a log line
//!   ([`wire::Item`]'s `Debug` leaves it out), and each item received is
//!   registered with the assistant's bank guard
//!   ([`crate::ai_chat::bank_guard::hold_hosted_item`]), which keeps only
//!   hashes of it.
//! - **Consent**: no open request is sent until the candidate has accepted
//!   the terms, and the open request names the digest of the terms shown, so
//!   the service holds the candidate to exactly what they read.
//! - **Close**: the integrity session's assertion ([`CloseInputs`]) and, for
//!   an attested session, platform evidence over the close digest
//!   ([`close_digest`]): this phone's own, or the paired phone's through the
//!   attester the caller passes.
//! - **Attested roles**: the open request registers an App Attest key and
//!   each answer and the close carry an assertion from it
//!   ([`PlatformKey`]). The key id is held in memory for the attempt.
//!
//! Refusal codes are explained in plain words by [`refusal`].

pub mod refusal;
pub mod wire;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use ed25519_dalek::SigningKey;
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::crypto::did::Did;
use crate::sentinel::attestation::{
    AttestationPayload, BindingAttester, DeviceAttestation, TrustConfig,
};
use alexandria_verify::attestation::AttestationEvidence;
use wire::{
    AnswerBody, AttemptState, CloseBody, CloseReply, Consent, DeviceKey, OpenBody, ServiceRefusal,
    TermsReply,
};

/// Where the candidate API lives on the Cloud origin.
pub const HOSTED_ATTEMPTS_PATH: &str = "/api/hosted-attempts/for/";
/// Header carrying the App Attest assertion over an answer or close request.
pub const ASSERTION_HEADER: &str = "x-alexandria-app-attest";
/// Domain tag of the close digest.
pub const CLOSE_DIGEST_TAG: &[u8] = b"alexandria-hosted-attempt-close-v1\0";
/// Domain tag of the device key challenge.
pub const DEVICE_KEY_TAG: &[u8] = b"alexandria-hosted-attempt-key-v1\0";
/// The stable prefix of every hosted attempt error at the command boundary:
/// `hosted_attempt:<code>: <message>`.
pub const ERROR_PREFIX: &str = "hosted_attempt";

/// Largest reply read: an item, or the close result.
const MAX_REPLY_BYTES: usize = 256 * 1024;

/// Why a hosted attempt request did not go through.
#[derive(Debug, thiserror::Error)]
pub enum HostedError {
    /// The terms were not accepted; nothing was sent.
    #[error("accept the terms before the first item")]
    ConsentRequired,
    /// The service refused, with its stable code.
    #[error("{message}")]
    Refused {
        status: u16,
        code: String,
        message: String,
        extra: serde_json::Map<String, Value>,
    },
    /// The service could not be reached or did not answer: retry, and the
    /// attempt resumes at the same item inside the disconnection window.
    #[error("the organisation's service could not be reached: {0}")]
    Disconnected(String),
    /// The service answered with something this client cannot read.
    #[error("the organisation's service sent an unreadable reply: {0}")]
    Protocol(String),
    /// This device could not do what the attempt requires of it.
    #[error("{0}")]
    Device(String),
}

impl HostedError {
    /// The stable code: the service's own for a refusal.
    pub fn code(&self) -> &str {
        match self {
            Self::ConsentRequired => "consent_required",
            Self::Refused { code, .. } => code,
            Self::Disconnected(_) => "disconnected",
            Self::Protocol(_) => "protocol",
            Self::Device(_) => "device",
        }
    }

    /// The service will not serve this attempt any more: closed, abandoned
    /// after the disconnection window, or never this candidate's.
    pub fn is_not_found(&self) -> bool {
        self.code() == "not_found"
    }

    /// `hosted_attempt:<code>: <message>`, for the command boundary.
    pub fn into_command_error(self) -> String {
        format!("{ERROR_PREFIX}:{}: {self}", self.code())
    }
}

// ── Digests ───────────────────────────────────────────────────────

/// What the device's platform evidence at close signs:
/// `SHA-256(CLOSE_DIGEST_TAG || commitment_root || 0x00 || did || 0x00 ||
/// attempt id)`.
pub fn close_digest(commitment_root: &str, did: &str, attempt_id: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(CLOSE_DIGEST_TAG);
    h.update(commitment_root.as_bytes());
    h.update([0]);
    h.update(did.as_bytes());
    h.update([0]);
    h.update(attempt_id.to_ascii_lowercase().as_bytes());
    h.finalize().into()
}

/// The client data hash of the attestation that registers the device's key
/// at open: `SHA-256(DEVICE_KEY_TAG || did || 0x00 || invite id || 0x00 ||
/// nonce)`, the nonce being the open request's own.
pub fn device_key_challenge(did: &str, invite_id: &str, nonce: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(DEVICE_KEY_TAG);
    h.update(did.as_bytes());
    h.update([0]);
    h.update(invite_id.to_ascii_lowercase().as_bytes());
    h.update([0]);
    h.update(nonce.as_bytes());
    h.finalize().into()
}

/// The client data of the assertion an attested request carries: the signed
/// challenge, a newline, and the lowercase hex SHA-256 of the body.
pub fn request_client_data(challenge: &str, body: &[u8]) -> Vec<u8> {
    format!("{challenge}\n{}", hex::encode(Sha256::digest(body))).into_bytes()
}

fn terms_digest(terms: &Value) -> Result<String, HostedError> {
    let canonical = serde_json_canonicalizer::to_vec(terms)
        .map_err(|e| HostedError::Protocol(format!("the terms do not canonicalise: {e}")))?;
    Ok(hex::encode(Sha256::digest(canonical)))
}

// ── The platform key of an attested attempt ───────────────────────

/// The App Attest key an attested attempt registers at open and asserts each
/// request with. Tests and unattested attempts use [`NoPlatformKey`].
pub trait PlatformKey: Send + Sync {
    /// Generate and attest a key whose attestation's client data hash is
    /// `challenge`.
    fn register(&self, challenge: &[u8; 32]) -> Result<DeviceKey, String>;
    /// A standard-base64 assertion from `key_id` over `client_data`.
    fn assert(&self, key_id: &str, client_data: &[u8]) -> Result<String, String>;
}

/// No platform key: an attested attempt cannot be opened with it.
pub struct NoPlatformKey;

impl PlatformKey for NoPlatformKey {
    fn register(&self, _: &[u8; 32]) -> Result<DeviceKey, String> {
        Err("this device cannot attest itself".into())
    }
    fn assert(&self, _: &str, _: &[u8]) -> Result<String, String> {
        Err("this device cannot attest itself".into())
    }
}

/// This phone's App Attest ([`crate::sentinel::attestation::native`]). Each
/// call waits on the platform, so it runs where blocking is allowed: call it
/// from the multi-threaded runtime the app runs on.
pub struct NativePlatformKey;

impl PlatformKey for NativePlatformKey {
    fn register(&self, challenge: &[u8; 32]) -> Result<DeviceKey, String> {
        let captured = tokio::task::block_in_place(|| {
            crate::sentinel::attestation::native::capture(challenge)
        })?
        .ok_or("this device cannot attest itself")?;
        match captured.payload {
            AttestationPayload::AppAttest {
                key_id_b64,
                attestation_b64,
                ..
            } => Ok(DeviceKey {
                key_id: key_id_b64,
                attestation: attestation_b64,
            }),
            AttestationPayload::PlayIntegrity { .. } => {
                Err("this assessment needs App Attest, which only an iPhone has".into())
            }
        }
    }

    fn assert(&self, key_id: &str, client_data: &[u8]) -> Result<String, String> {
        tokio::task::block_in_place(|| {
            crate::sentinel::attestation::native::assert_bytes(key_id, client_data)
        })?
        .ok_or_else(|| "this device cannot attest itself".into())
    }
}

/// Attempt id → App Attest key id, for attested attempts opened in this run
/// of the app.
fn device_keys() -> &'static Mutex<HashMap<String, String>> {
    static KEYS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    KEYS.get_or_init(Default::default)
}

fn device_key_for(attempt: &str) -> Option<String> {
    device_keys()
        .lock()
        .ok()
        .and_then(|keys| keys.get(attempt).cloned())
}

// ── The candidate API ─────────────────────────────────────────────

/// The candidate's signed client for one Cloud origin.
pub struct HostedService {
    http: reqwest::Client,
    origin: String,
    key: SigningKey,
    did: Did,
}

impl HostedService {
    /// A client for `origin` (https, or http on loopback).
    pub fn new(origin: &str, key: SigningKey, did: Did) -> Result<Self, HostedError> {
        if !crate::cloud::is_allowed_origin(origin) {
            return Err(HostedError::Device(
                "the organisation's service must be https".into(),
            ));
        }
        Ok(Self {
            http: crate::cloud::client().map_err(HostedError::Device)?,
            origin: origin.trim_end_matches('/').to_string(),
            key,
            did,
        })
    }

    pub fn did(&self) -> &Did {
        &self.did
    }

    fn path(&self, rest: &str) -> String {
        format!("{HOSTED_ATTEMPTS_PATH}{}/{rest}", self.did.as_str())
    }

    /// Send a signed request. `nonce` is the request's own when the caller
    /// needs it beforehand (the open of an attested attempt); `assert_with`
    /// adds an assertion over the signed challenge and the body.
    async fn send<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Vec<u8>>,
        nonce: Option<String>,
        assert_with: Option<(&dyn PlatformKey, &str)>,
    ) -> Result<T, HostedError> {
        let timestamp = chrono::Utc::now().timestamp();
        let nonce = nonce.unwrap_or_else(crate::cloud::nonce);
        let proof = crate::cloud::proof(&self.key, method.as_str(), path, timestamp, &nonce);
        let mut request = self
            .http
            .request(method.clone(), format!("{}{path}", self.origin))
            .header("x-alexandria-timestamp", timestamp.to_string())
            .header("x-alexandria-nonce", &nonce)
            .header("x-alexandria-proof", proof);
        if let Some((platform, key_id)) = assert_with {
            let challenge = format!("{}\n{path}\n{timestamp}\n{nonce}", method.as_str());
            let data = request_client_data(&challenge, body.as_deref().unwrap_or_default());
            let assertion = platform
                .assert(key_id, &data)
                .map_err(HostedError::Device)?;
            request = request.header(ASSERTION_HEADER, assertion);
        }
        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }
        let mut response = request
            .send()
            .await
            .map_err(|e| HostedError::Disconnected(e.without_url().to_string()))?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| HostedError::Disconnected(e.without_url().to_string()))?
        {
            if bytes.len() + chunk.len() > MAX_REPLY_BYTES {
                return Err(HostedError::Protocol("the reply is too large".into()));
            }
            bytes.extend_from_slice(&chunk);
        }
        if status.is_server_error() {
            return Err(HostedError::Disconnected(format!(
                "the service answered {status}"
            )));
        }
        if !status.is_success() {
            return Err(match serde_json::from_slice::<ServiceRefusal>(&bytes) {
                Ok(refusal) => HostedError::Refused {
                    status: status.as_u16(),
                    code: refusal.code,
                    message: refusal.error,
                    extra: refusal.extra,
                },
                Err(_) => HostedError::Protocol(format!("the service answered {status}")),
            });
        }
        serde_json::from_slice(&bytes).map_err(|e| HostedError::Protocol(e.to_string()))
    }

    fn received(state: AttemptState) -> AttemptState {
        if let Some(item) = &state.item {
            crate::ai_chat::bank_guard::hold_hosted_item(&item.question, &item.options);
        }
        state
    }

    /// The terms of a hosted attempt from the invitation `invite`, and
    /// whether one can open now. The digest is checked against the terms
    /// themselves, so what is shown is what is accepted.
    pub async fn terms(&self, invite: &str) -> Result<TermsReply, HostedError> {
        let path = self.path(&format!("invites/{invite}"));
        let raw: Value = self
            .send(reqwest::Method::GET, &path, None, None, None)
            .await?;
        let computed = terms_digest(raw.get("terms").unwrap_or(&Value::Null))?;
        let reply: TermsReply =
            serde_json::from_value(raw).map_err(|e| HostedError::Protocol(e.to_string()))?;
        if reply.terms.format != wire::TERMS_FORMAT {
            return Err(HostedError::Protocol(format!(
                "terms in an unknown format: {}",
                reply.terms.format
            )));
        }
        if computed != reply.terms_digest {
            return Err(HostedError::Protocol(
                "the terms do not match their digest".into(),
            ));
        }
        Ok(reply)
    }

    /// Open an attempt: refused here, before anything is sent, unless the
    /// candidate accepted the terms. An attested role registers this phone's
    /// key over the open request's own nonce. The reply carries the first
    /// item, whose clock starts now.
    pub async fn open(
        &self,
        invite: &str,
        consent: &Consent,
        attested: bool,
        platform: &dyn PlatformKey,
    ) -> Result<AttemptState, HostedError> {
        if !consent.accepted || consent.terms_digest.trim().is_empty() {
            return Err(HostedError::ConsentRequired);
        }
        let path = self.path(&format!("invites/{invite}"));
        let nonce = crate::cloud::nonce();
        let device_key = if attested {
            let challenge = device_key_challenge(self.did.as_str(), invite, &nonce);
            Some(platform.register(&challenge).map_err(HostedError::Device)?)
        } else {
            None
        };
        let key_id = device_key.as_ref().map(|k| k.key_id.clone());
        let body = serde_json::to_vec(&OpenBody {
            consent: consent.clone(),
            device_key,
        })
        .map_err(|e| HostedError::Protocol(e.to_string()))?;
        let state: AttemptState = self
            .send(reqwest::Method::POST, &path, Some(body), Some(nonce), None)
            .await?;
        if let (Some(key_id), Ok(mut keys)) = (key_id, device_keys().lock()) {
            keys.insert(state.attempt.id.clone(), key_id);
        }
        Ok(Self::received(state))
    }

    /// The item the candidate is on, as sent: after a dropped connection the
    /// attempt resumes here and the item's clock has kept running.
    pub async fn resume(&self, attempt: &str) -> Result<AttemptState, HostedError> {
        let path = self.path(&format!("attempts/{attempt}"));
        let state = self
            .send(reqwest::Method::GET, &path, None, None, None)
            .await?;
        Ok(Self::received(state))
    }

    /// Answer the current item; the reply carries the next. Nothing in it
    /// says whether the answer was right.
    pub async fn answer(
        &self,
        attempt: &str,
        answer: &AnswerBody,
        platform: &dyn PlatformKey,
    ) -> Result<AttemptState, HostedError> {
        let path = self.path(&format!("attempts/{attempt}/answers"));
        let body = serde_json::to_vec(answer).map_err(|e| HostedError::Protocol(e.to_string()))?;
        let key = device_key_for(attempt);
        let state = self
            .send(
                reqwest::Method::POST,
                &path,
                Some(body),
                None,
                key.as_deref().map(|k| (platform, k)),
            )
            .await?;
        Ok(Self::received(state))
    }

    /// Close the attempt with the session's assertion and evidence. The reply
    /// carries the service's grade and what became of issuance.
    pub async fn close(
        &self,
        attempt: &str,
        close: &CloseBody,
        platform: &dyn PlatformKey,
    ) -> Result<CloseReply, HostedError> {
        let path = self.path(&format!("attempts/{attempt}/close"));
        let body = serde_json::to_vec(close).map_err(|e| HostedError::Protocol(e.to_string()))?;
        let key = device_key_for(attempt);
        let reply = self
            .send(
                reqwest::Method::POST,
                &path,
                Some(body),
                None,
                key.as_deref().map(|k| (platform, k)),
            )
            .await?;
        if let Ok(mut keys) = device_keys().lock() {
            keys.remove(attempt);
        }
        Ok(reply)
    }

    /// What became of a closed attempt: the service's result and issuance.
    pub async fn result(&self, attempt: &str) -> Result<CloseReply, HostedError> {
        let path = self.path(&format!("attempts/{attempt}/result"));
        self.send(reqwest::Method::GET, &path, None, None, None)
            .await
    }
}

// ── Close ─────────────────────────────────────────────────────────

/// What the close request needs from the profile: the session's integrity
/// assertion and its stored attestation.
#[derive(Debug, Clone)]
pub struct CloseInputs {
    session_id: String,
    integrity: Value,
    commitment_root: String,
    attestation: Option<(DeviceAttestation, String)>,
}

impl CloseInputs {
    /// Read the ended session `session_id`. Its assertion must carry a
    /// commitment root, which the close digest covers.
    pub fn read(
        conn: &Connection,
        session_id: &str,
        now: &str,
        trust: &TrustConfig,
    ) -> Result<Self, HostedError> {
        let (assertion, _) =
            crate::credentials::issue::build_integrity_assertion(conn, session_id, now, trust)
                .map_err(|e| HostedError::Device(e.to_string()))?;
        let commitment_root = assertion
            .commitment_root
            .clone()
            .filter(|r| !r.trim().is_empty())
            .ok_or_else(|| {
                HostedError::Device("the integrity session has no commitment root".into())
            })?;
        let integrity =
            serde_json::to_value(&assertion).map_err(|e| HostedError::Protocol(e.to_string()))?;
        let stored: Option<(String, Option<String>)> = conn
            .query_row(
                "SELECT started_at, attestation_json FROM integrity_sessions WHERE id = ?1",
                params![session_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| HostedError::Device(e.to_string()))?;
        let attestation = match stored {
            Some((started_at, Some(json))) => serde_json::from_str::<DeviceAttestation>(&json)
                .ok()
                .map(|record| (record, started_at)),
            _ => None,
        };
        Ok(Self {
            session_id: session_id.to_string(),
            integrity,
            commitment_root,
            attestation,
        })
    }

    /// The close digest this attempt's evidence must cover.
    pub fn digest(&self, did: &Did, attempt_id: &str) -> [u8; 32] {
        close_digest(&self.commitment_root, did.as_str(), attempt_id)
    }

    /// The close request: the assertion and, for an attested session,
    /// evidence over the close digest from `attester` (this phone, or the
    /// paired one). Evidence that cannot be obtained is left out and the
    /// service judges the attempt without it. May block on a paired phone:
    /// call with no database job running.
    pub fn into_body(
        self,
        did: &Did,
        attempt_id: &str,
        attester: &dyn BindingAttester,
    ) -> CloseBody {
        let digest = self.digest(did, attempt_id);
        let evidence = self.attestation.and_then(|(record, started_at)| {
            match attester.bind(&self.session_id, &record, &digest) {
                Ok(proof) => serde_json::to_value(AttestationEvidence::credential_bound(
                    &record,
                    &started_at,
                    proof,
                ))
                .ok(),
                Err(why) => {
                    log::info!(
                        target: "sentinel",
                        "hosted attempt {attempt_id}: no evidence over the close: {why}"
                    );
                    None
                }
            }
        });
        CloseBody {
            integrity: self.integrity,
            evidence,
        }
    }
}
