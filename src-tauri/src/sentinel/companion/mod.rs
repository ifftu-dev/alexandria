//! Paired attested phone: a desktop session borrows a phone's platform
//! attestation.
//!
//! Desktops have no hardware attestation Alexandria can use (on macOS,
//! `DCAppAttestService.isSupported` is false), so a desktop session stays
//! `local` on its own. The learner can pair their phone for one session:
//!
//! 1. The desktop shows a one-time **offer** (a code): the session id and
//!    start instant, the learner's DID, the desktop's iroh address and a
//!    fresh secret. It expires after [`OFFER_TTL_SECS`].
//! 2. The phone joins only if the offer names the DID of its own unlocked
//!    profile ([`answer_offer`]), so a genuine app on someone else's phone
//!    refuses to vouch for this learner. It attests [`companion_nonce`] (the
//!    session nonce bound to the learner DID and the phone's iroh endpoint
//!    id) with its platform (App Attest / Play Integrity), signs a statement
//!    over the result with the learner DID key, and sends the **hello**.
//! 3. The desktop never takes the phone's word, nor its own: it checks the
//!    secret, that the hello arrived on a connection authenticated as the
//!    phone endpoint it names, the DID signature, and then verifies the
//!    platform attestation with [`crate::sentinel::attestation::verify`]
//!    against a nonce it recomputes from its own session row
//!    ([`accept_hello`]).
//! 4. The record is stored on `integrity_sessions.attestation_json` like a
//!    mobile session's, carrying a [`CompanionBinding`]; issuance re-verifies
//!    it (`effective_assurance`), including that the binding names the
//!    profile's learner. Profile selection folds it into the commitment
//!    chain ahead of the selection link.
//! 5. At session end an iOS phone signs the desktop's terminal commitment
//!    root with its attested key (an assertion), as an iPhone does for its
//!    own sessions. Under `camera` / `full` the phone is the camera and sends
//!    only per-window camera scores, never frames.
//!
//! Ceiling: the phone proves "a genuine build on a genuine phone, signed in
//! as this learner, joined this session". It does not prove the phone
//! stayed in the room or watched the learner. The camera scores it sends are
//! not signed one by one, and the end assertion signs the root the desktop
//! presents without the phone recomputing it, so it says "this phone was
//! still linked at the end", not "these exact scores came from it".

pub mod session;
pub mod transport;

use base64::Engine as _;
use ed25519_dalek::{Signer as _, SigningKey, Verifier as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::sentinel::attestation::{
    self as att, native::Captured, AttestationPayload, DeviceAttestation, TrustConfig, Unverified,
};

/// ALPN of the companion protocol on the shared iroh endpoint.
pub const ALPN: &[u8] = b"alexandria/sentinel-companion/1";
/// How long an offer stays valid (seconds).
pub const OFFER_TTL_SECS: i64 = 300;

const NONCE_TAG: &[u8] = b"alexandria-companion-attest-v1\0";
const STATEMENT_TAG: &str = "alexandria-companion-statement-v1";

/// What the desktop shows for the phone to join. Encoded as a code
/// ([`CompanionOffer::encode`]); it carries a secret, so it is shown, not
/// sent over the network.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanionOffer {
    pub session_id: String,
    /// `integrity_sessions.started_at`; with the id it gives the session
    /// nonce the phone attests over.
    pub started_at: String,
    /// The DID of the profile that owns the session.
    pub learner_did: String,
    /// Where the phone dials the desktop.
    pub desktop: iroh::EndpointAddr,
    /// Hex of 32 random bytes; proves the phone saw this offer.
    pub secret_hex: String,
    /// Unix seconds.
    pub expires_at: i64,
    /// Whether the phone is asked to act as the session's camera.
    pub camera: bool,
}

impl std::fmt::Debug for CompanionOffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompanionOffer")
            .field("session_id", &self.session_id)
            .field("secret", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("camera", &self.camera)
            .finish()
    }
}

impl CompanionOffer {
    /// A fresh offer for a session, valid for [`OFFER_TTL_SECS`].
    pub fn new(
        session_id: &str,
        started_at: &str,
        learner_did: &str,
        desktop: iroh::EndpointAddr,
        camera: bool,
        now_unix: i64,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            started_at: started_at.into(),
            learner_did: learner_did.into(),
            desktop,
            secret_hex: hex::encode(crate::crypto::pairing::generate_shared_key()),
            expires_at: now_unix + OFFER_TTL_SECS,
            camera,
        }
    }

    pub fn encode(&self) -> Result<String, String> {
        let json = serde_json::to_vec(self).map_err(|e| format!("encode companion offer: {e}"))?;
        Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json))
    }

    pub fn decode(code: &str) -> Result<Self, String> {
        let json = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(code.trim())
            .map_err(|_| "this is not a companion code".to_string())?;
        serde_json::from_slice(&json).map_err(|_| "this is not a companion code".to_string())
    }

    fn session_nonce(&self) -> [u8; 32] {
        att::session_nonce(&self.session_id, &self.started_at)
    }
}

/// The nonce the phone's platform attests: the desktop session's nonce
/// bound to the learner and to the phone's transport identity.
pub fn companion_nonce(
    session_nonce: &[u8; 32],
    learner_did: &str,
    phone_endpoint: &str,
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(NONCE_TAG);
    h.update(session_nonce);
    h.update(learner_did.as_bytes());
    h.update(b"\0");
    h.update(phone_endpoint.as_bytes());
    h.finalize().into()
}

/// Stored with a companion's attestation: who it vouches for and which
/// phone delivered it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanionBinding {
    pub learner_did: String,
    /// The phone's iroh endpoint id (hex), as authenticated by the
    /// connection that delivered the hello.
    pub phone_endpoint: String,
    /// Ed25519 signature by the learner DID key over [`statement`].
    pub did_signature_b64: String,
}

/// The phone's first message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanionHello {
    pub session_id: String,
    pub secret_hex: String,
    /// The phone's platform attestation, with its [`CompanionBinding`].
    pub attestation: DeviceAttestation,
    /// The phone's camera benchmark when it is offered as the camera.
    /// Reported by an attested build, but not itself attested.
    #[serde(default)]
    pub camera_benchmark: Option<crate::sentinel::camera_budget::BenchmarkResult>,
}

/// Phone side: whether to answer `offer`, and the nonce to attest if so.
/// Refuses an expired offer and one for any learner but the phone's own.
pub fn answer_offer(
    offer: &CompanionOffer,
    local_did: &str,
    phone_endpoint: &str,
    now_unix: i64,
) -> Result<[u8; 32], String> {
    if now_unix > offer.expires_at {
        return Err("this companion code has expired; show a new one on the desktop".into());
    }
    if offer.learner_did != local_did {
        return Err("this companion code is for another learner's session".into());
    }
    Ok(companion_nonce(
        &offer.session_nonce(),
        &offer.learner_did,
        phone_endpoint,
    ))
}

/// The bytes the learner DID signs: the binding's subject and phone, the
/// attested nonce, and a digest of the start-of-session attestation
/// material (the end assertion is added later and is not covered).
fn statement(record: &DeviceAttestation, learner_did: &str, phone_endpoint: &str) -> Vec<u8> {
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
        "{STATEMENT_TAG}\n{learner_did}\n{phone_endpoint}\n{}\n{}\n{digest}",
        record.app_id, record.nonce_hex
    )
    .into_bytes()
}

/// Phone side: wrap the platform's output into the hello.
pub fn build_hello(
    offer: &CompanionOffer,
    learner_key: &SigningKey,
    phone_endpoint: &str,
    nonce: &[u8; 32],
    captured: Captured,
    captured_at: &str,
    camera_benchmark: Option<crate::sentinel::camera_budget::BenchmarkResult>,
) -> CompanionHello {
    let mut record = DeviceAttestation {
        platform: captured.platform,
        nonce_hex: hex::encode(nonce),
        app_id: captured.app_id,
        payload: captured.payload,
        captured_at: captured_at.into(),
        companion: None,
    };
    let signature = learner_key.sign(&statement(&record, &offer.learner_did, phone_endpoint));
    record.companion = Some(CompanionBinding {
        learner_did: offer.learner_did.clone(),
        phone_endpoint: phone_endpoint.into(),
        did_signature_b64: base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()),
    });
    CompanionHello {
        session_id: offer.session_id.clone(),
        secret_hex: offer.secret_hex.clone(),
        attestation: record,
        camera_benchmark,
    }
}

/// The nonce a companion record must attest, once its learner signature
/// checks out. Used at pairing and again at issuance.
pub fn bound_nonce(
    record: &DeviceAttestation,
    binding: &CompanionBinding,
    session_nonce: &[u8; 32],
) -> Result<[u8; 32], Unverified> {
    let failed = |reason: &str| Unverified::Failed {
        reason: format!("companion: {reason}"),
    };
    let did = alexandria_verify::did::parse_did_key(&binding.learner_did)
        .map_err(|_| failed("binding names no did:key learner"))?;
    let key = alexandria_verify::did::resolve_did_key(&did)
        .map_err(|_| failed("binding names no did:key learner"))?;
    let signature = base64::engine::general_purpose::STANDARD
        .decode(binding.did_signature_b64.trim())
        .ok()
        .and_then(|b| ed25519_dalek::Signature::from_slice(&b).ok())
        .ok_or_else(|| failed("learner signature is malformed"))?;
    key.verify(
        &statement(record, &binding.learner_did, &binding.phone_endpoint),
        &signature,
    )
    .map_err(|_| failed("learner signature does not verify"))?;
    Ok(companion_nonce(
        session_nonce,
        &binding.learner_did,
        &binding.phone_endpoint,
    ))
}

/// What the desktop keeps from an accepted hello.
#[derive(Debug, Clone, PartialEq)]
pub struct Accepted {
    /// Store on `integrity_sessions.attestation_json`.
    pub record: DeviceAttestation,
    /// `device_attested` when the platform attestation verified now,
    /// `local` when there is no trust material for it (Android without
    /// response keys).
    pub assurance_level: String,
    pub unverified: Option<Unverified>,
    pub camera_benchmark: Option<crate::sentinel::camera_budget::BenchmarkResult>,
}

/// Desktop side: judge a hello against the offer it answers.
///
/// `remote_endpoint` is the endpoint id the transport authenticated, never
/// a field of the hello. A hello whose attestation fails verification is
/// refused outright; one that cannot be verified yet (no trust material)
/// is kept at `local`, as a phone's own session would be.
pub fn accept_hello(
    offer: &CompanionOffer,
    hello: &CompanionHello,
    remote_endpoint: &str,
    cfg: &TrustConfig,
    now_unix: i64,
) -> Result<Accepted, String> {
    use subtle::ConstantTimeEq as _;
    if now_unix > offer.expires_at {
        return Err("the companion code expired".into());
    }
    if hello.session_id != offer.session_id
        || !bool::from(
            hello
                .secret_hex
                .as_bytes()
                .ct_eq(offer.secret_hex.as_bytes()),
        )
    {
        return Err("the phone did not answer this companion code".into());
    }
    let record = &hello.attestation;
    let binding = record
        .companion
        .as_ref()
        .ok_or("the phone sent no companion binding")?;
    if binding.learner_did != offer.learner_did {
        return Err("the phone is signed in as another learner".into());
    }
    if binding.phone_endpoint != remote_endpoint {
        return Err("the hello names a different phone than the one connected".into());
    }
    let nonce = bound_nonce(record, binding, &offer.session_nonce()).map_err(reason)?;
    let (assurance_level, unverified) = match att::verify(record, &nonce, None, cfg, now_unix) {
        Ok(_) => (att::ASSURANCE_DEVICE_ATTESTED.to_string(), None),
        Err(Unverified::NoTrustMaterial) => (
            att::ASSURANCE_LOCAL.to_string(),
            Some(Unverified::NoTrustMaterial),
        ),
        Err(why) => return Err(reason(why)),
    };
    Ok(Accepted {
        record: record.clone(),
        assurance_level,
        unverified,
        camera_benchmark: hello.camera_benchmark,
    })
}

fn reason(why: Unverified) -> String {
    match why {
        Unverified::NoTrustMaterial => "no trust material for the phone's platform".into(),
        Unverified::Failed { reason } => reason,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::sentinel::attestation::app_attest::tests::{build_assertion, build_world, World};
    use crate::sentinel::attestation::{
        apple_app_id, effective_assurance, Platform, SessionAttestationFacts,
    };

    pub(crate) const SESSION: &str = "isess_desktop";
    pub(crate) const STARTED: &str = "2026-11-12T10:00:00Z";
    pub(crate) const PHONE: &str = "phone-endpoint-id";

    pub(crate) fn learner() -> SigningKey {
        SigningKey::from_bytes(&[21; 32])
    }

    pub(crate) fn did_of(key: &SigningKey) -> String {
        alexandria_verify::did::derive_did_key(key)
            .as_str()
            .to_string()
    }

    pub(crate) fn desktop_addr() -> iroh::EndpointAddr {
        iroh::EndpointAddr::new(iroh::SecretKey::from_bytes(&[3; 32]).public())
    }

    pub(crate) fn offer(now: i64) -> CompanionOffer {
        CompanionOffer::new(
            SESSION,
            STARTED,
            &did_of(&learner()),
            desktop_addr(),
            false,
            now,
        )
    }

    /// A phone that attests whatever nonce it is given with a synthetic
    /// App Attest chain (the verifier's real checks run against it).
    pub(crate) fn phone_capture(nonce: [u8; 32]) -> (World, Captured) {
        let w = build_world(&apple_app_id(), nonce, b"appattest\0\0\0\0\0\0\0", 0);
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        let captured = Captured {
            platform: Platform::Ios,
            app_id: apple_app_id(),
            payload: AttestationPayload::AppAttest {
                key_id_b64: b64(&w.key_id),
                attestation_b64: b64(&w.attestation),
                assertion_b64: None,
                asserted_root: None,
            },
        };
        (w, captured)
    }

    pub(crate) fn trust(w: &World) -> TrustConfig {
        TrustConfig {
            apple_root_pem: w.root_pem.clone(),
            ..TrustConfig::default()
        }
    }

    /// The phone answers `offer` as `key` and builds its hello.
    pub(crate) fn hello_from(
        offer: &CompanionOffer,
        key: &SigningKey,
        now: i64,
    ) -> (World, CompanionHello) {
        let nonce = answer_offer(offer, &did_of(key), PHONE, now).unwrap();
        let (w, captured) = phone_capture(nonce);
        let hello = build_hello(offer, key, PHONE, &nonce, captured, STARTED, None);
        (w, hello)
    }

    fn facts(record: &DeviceAttestation, learner: Option<String>) -> SessionAttestationFacts {
        SessionAttestationFacts {
            session_id: SESSION.into(),
            started_at: STARTED.into(),
            stored_assurance_level: att::ASSURANCE_DEVICE_ATTESTED.into(),
            attestation_json: Some(serde_json::to_string(record).unwrap()),
            commitment_root: None,
            learner_did: learner,
        }
    }

    #[test]
    fn a_paired_phone_attestation_reaches_device_attested_and_re_verifies_at_issuance() {
        let now = build_world(&apple_app_id(), [0; 32], b"appattest\0\0\0\0\0\0\0", 0).now;
        let offer = offer(now);
        let (w, hello) = hello_from(&offer, &learner(), now);

        let accepted = accept_hello(&offer, &hello, PHONE, &trust(&w), now).unwrap();
        assert_eq!(accepted.assurance_level, att::ASSURANCE_DEVICE_ATTESTED);

        // Issuance recomputes everything from the stored record and the
        // profile's learner.
        let (level, why) = effective_assurance(
            &facts(&accepted.record, Some(did_of(&learner()))),
            &trust(&w),
            now,
        );
        assert_eq!(
            (level.as_str(), why),
            (att::ASSURANCE_DEVICE_ATTESTED, None)
        );
    }

    #[test]
    fn a_phone_signed_in_as_someone_else_refuses_to_answer() {
        let now = 1_760_000_000;
        let stranger = SigningKey::from_bytes(&[77; 32]);
        let err = answer_offer(&offer(now), &did_of(&stranger), PHONE, now).unwrap_err();
        assert!(err.contains("another learner"), "{err}");
        let expired = answer_offer(
            &offer(now),
            &did_of(&learner()),
            PHONE,
            now + OFFER_TTL_SECS + 1,
        )
        .unwrap_err();
        assert!(expired.contains("expired"), "{expired}");
    }

    #[test]
    fn the_desktop_refuses_a_hello_it_cannot_verify_from_its_own_session() {
        let now = build_world(&apple_app_id(), [0; 32], b"appattest\0\0\0\0\0\0\0", 0).now;
        let offer = offer(now);
        let (w, hello) = hello_from(&offer, &learner(), now);
        let cfg = trust(&w);

        // Wrong secret: the phone never saw this code.
        let mut wrong_secret = hello.clone();
        wrong_secret.secret_hex = "00".repeat(32);
        assert!(accept_hello(&offer, &wrong_secret, PHONE, &cfg, now).is_err());

        // Delivered over a connection from another endpoint.
        assert!(accept_hello(&offer, &hello, "other-endpoint", &cfg, now)
            .unwrap_err()
            .contains("different phone"));

        // A genuine attestation for another session's nonce.
        let other = CompanionOffer {
            session_id: "isess_other".into(),
            ..offer.clone()
        };
        let (w2, other_hello) = hello_from(&other, &learner(), now);
        let replayed = CompanionHello {
            session_id: offer.session_id.clone(),
            ..other_hello
        };
        assert!(accept_hello(&offer, &replayed, PHONE, &trust(&w2), now).is_err());

        // A binding re-signed by another key, or for another learner.
        let stranger = SigningKey::from_bytes(&[77; 32]);
        let mut forged = hello.clone();
        let binding = forged.attestation.companion.as_mut().unwrap();
        binding.did_signature_b64 =
            base64::engine::general_purpose::STANDARD.encode(stranger.sign(b"anything").to_bytes());
        assert!(accept_hello(&offer, &forged, PHONE, &cfg, now)
            .unwrap_err()
            .contains("signature"));
        let (_, strangers) = {
            let o = CompanionOffer {
                learner_did: did_of(&stranger),
                ..offer.clone()
            };
            hello_from(&o, &stranger, now)
        };
        assert!(accept_hello(&offer, &strangers, PHONE, &cfg, now)
            .unwrap_err()
            .contains("another learner"));

        // The real Apple root does not know the synthetic chain.
        assert!(accept_hello(&offer, &hello, PHONE, &TrustConfig::default(), now).is_err());
    }

    #[test]
    fn issuance_credits_a_companion_only_for_the_profiles_own_learner() {
        let now = build_world(&apple_app_id(), [0; 32], b"appattest\0\0\0\0\0\0\0", 0).now;
        let offer = offer(now);
        let (w, hello) = hello_from(&offer, &learner(), now);
        let record = accept_hello(&offer, &hello, PHONE, &trust(&w), now)
            .unwrap()
            .record;
        let stranger = did_of(&SigningKey::from_bytes(&[77; 32]));
        for learner in [Some(stranger), None] {
            let (level, why) = effective_assurance(&facts(&record, learner), &trust(&w), now);
            assert_eq!(level, att::ASSURANCE_LOCAL);
            assert!(matches!(why, Some(Unverified::Failed { .. })));
        }
    }

    #[test]
    fn the_phone_end_assertion_covers_the_desktop_terminal_root() {
        let now = build_world(&apple_app_id(), [0; 32], b"appattest\0\0\0\0\0\0\0", 0).now;
        let offer = offer(now);
        let (w, hello) = hello_from(&offer, &learner(), now);
        let mut record = accept_hello(&offer, &hello, PHONE, &trust(&w), now)
            .unwrap()
            .record;
        let root = "c0ffee";
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        if let AttestationPayload::AppAttest {
            assertion_b64,
            asserted_root,
            ..
        } = &mut record.payload
        {
            *assertion_b64 = Some(b64(&build_assertion(&w, root.as_bytes(), 1)));
            *asserted_root = Some(root.into());
        }
        let mut f = facts(&record, Some(did_of(&learner())));
        f.commitment_root = Some(root.into());
        assert_eq!(
            effective_assurance(&f, &trust(&w), now).0,
            att::ASSURANCE_DEVICE_ATTESTED
        );
        f.commitment_root = Some("moved".into());
        assert_eq!(
            effective_assurance(&f, &trust(&w), now).0,
            att::ASSURANCE_LOCAL
        );
    }

    #[test]
    fn an_offer_round_trips_as_a_code_and_hides_its_secret_in_logs() {
        let o = offer(1_760_000_000);
        assert_eq!(CompanionOffer::decode(&o.encode().unwrap()).unwrap(), o);
        assert!(!format!("{o:?}").contains(&o.secret_hex));
        assert!(CompanionOffer::decode("not a code").is_err());
    }
}
