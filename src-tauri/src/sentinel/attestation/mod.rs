//! Device attestation: the `device_attested` assurance rung.
//!
//! Mobile platforms will attest that a genuine, unmodified build of this app
//! on a genuine device produced a given nonce. Bound to the integrity
//! session, that turns "the client says so" into "the platform says this
//! client was ours". Desktop has no equivalent of its own (on macOS App
//! Attest reports unsupported) and stays `local` unless the learner pairs an
//! attested phone (`sentinel::companion`).
//!
//! The verification itself (App Attest chains and assertions, Play Integrity
//! tokens, companion bindings, credential evidence) lives in
//! [`alexandria_verify::attestation`], so the app, the hosted service and any
//! third-party verifier run one implementation. This module supplies what is
//! the app's own: the platform capture (`native`), this build's trust inputs
//! ([`TrustConfig`]: Apple's root, this app's identifiers, the network
//! profile's Play Integrity response keys) and the issuance-time decision of
//! what assurance a session may claim ([`effective_assurance`]).
//!
//! What crosses IPC and is stored: the opaque attestation blobs plus the
//! nonce and app id. Nothing here identifies the person; the platform key
//! is per-app, per-install, per-session.

pub mod native;

pub use alexandria_verify::attestation::{
    app_attest, play_integrity, session_nonce, AttestationPayload, AttestationTrust,
    DeviceAttestation, Platform, PlayIntegrityKeys, Unverified, Verified,
    APPLE_APP_ATTEST_ROOT_PEM,
};
use alexandria_verify::attestation::{AppleTrust, PlayIntegrityTrust};

/// Assurance a session reaches when its device attestation verifies.
pub const ASSURANCE_DEVICE_ATTESTED: &str = "device_attested";
/// Assurance every session has without attestation.
pub const ASSURANCE_LOCAL: &str = "local";

/// Apple Developer Team ID this app is signed under (Xcode project
/// `DEVELOPMENT_TEAM`). Part of the App Attest app id, `TEAMID.bundleid`.
pub const APPLE_TEAM_ID: &str = "VLMNL3V44U";
/// Bundle id (iOS) and application id (Android), from `tauri.conf.json`.
pub const APP_BUNDLE_ID: &str = "org.alexandria.node";

/// The App Attest `appID` string hashed into `authData.rpIdHash`.
pub fn apple_app_id() -> String {
    format!("{APPLE_TEAM_ID}.{APP_BUNDLE_ID}")
}

/// What this build trusts. [`TrustConfig::inputs`] turns it into the
/// verifier library's trust inputs, adding this app's identifiers.
#[derive(Debug, Clone)]
pub struct TrustConfig {
    /// PEM of the Apple App Attestation Root CA.
    pub apple_root_pem: String,
    /// Accept the `appattestdevelop` environment (development builds).
    pub allow_apple_development: bool,
    pub play_integrity_keys: Option<PlayIntegrityKeys>,
}

impl Default for TrustConfig {
    fn default() -> Self {
        Self {
            apple_root_pem: APPLE_APP_ATTEST_ROOT_PEM.to_string(),
            allow_apple_development: cfg!(debug_assertions),
            play_integrity_keys: None,
        }
    }
}

impl TrustConfig {
    /// Trust material for this build: Apple's root plus whatever Play
    /// Integrity response keys the embedded network profile carries.
    pub fn from_profile() -> Self {
        let keys = crate::network_profile::embedded_preprod()
            .ok()
            .and_then(|p| p.play_integrity_response_keys.clone());
        Self {
            play_integrity_keys: keys,
            ..Self::default()
        }
    }

    /// The verifier library's trust inputs: Apple's root for this app's App
    /// Attest id, and Play Integrity for this app's package when the keys
    /// are known.
    pub fn inputs(&self) -> AttestationTrust {
        AttestationTrust {
            apple: Some(AppleTrust {
                root_pem: self.apple_root_pem.clone(),
                app_id: apple_app_id(),
                allow_development: self.allow_apple_development,
            }),
            play_integrity: self
                .play_integrity_keys
                .clone()
                .map(|keys| PlayIntegrityTrust {
                    package_name: APP_BUNDLE_ID.into(),
                    keys,
                }),
        }
    }
}

/// This build's trust inputs from the embedded network profile, read once.
/// What the app verifies received credentials' attestation evidence against.
pub fn profile_trust_inputs() -> &'static AttestationTrust {
    static INPUTS: std::sync::OnceLock<AttestationTrust> = std::sync::OnceLock::new();
    INPUTS.get_or_init(|| TrustConfig::from_profile().inputs())
}

/// The session columns the assurance decision reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionAttestationFacts {
    pub session_id: String,
    pub started_at: String,
    pub stored_assurance_level: String,
    pub attestation_json: Option<String>,
    pub commitment_root: Option<String>,
    /// The DID of the profile that owns the session. A paired phone's
    /// attestation is credited only when its binding names this learner.
    pub learner_did: Option<String>,
}

/// Decide what a credential may claim for this session. The stored level
/// is a hint, never evidence: `device_attested` is asserted only when the
/// stored attestation re-verifies right now against the trust material.
/// Returns the level and, when it stayed `local` despite an attestation,
/// why.
pub fn effective_assurance(
    facts: &SessionAttestationFacts,
    cfg: &TrustConfig,
    now_unix: i64,
) -> (String, Option<Unverified>) {
    use alexandria_verify::attestation::InvalidReason;
    let Some(json) = facts.attestation_json.as_deref() else {
        return (ASSURANCE_LOCAL.to_string(), None);
    };
    let att: DeviceAttestation = match serde_json::from_str(json) {
        Ok(a) => a,
        Err(e) => {
            return (
                ASSURANCE_LOCAL.to_string(),
                Some(Unverified::failed(
                    InvalidReason::MalformedEvidence,
                    format!("stored attestation is unreadable: {e}"),
                )),
            )
        }
    };
    let nonce = match expected_nonce(&att, facts) {
        Ok(nonce) => nonce,
        Err(why) => return (ASSURANCE_LOCAL.to_string(), Some(why)),
    };
    match verify(
        &att,
        &nonce,
        facts.commitment_root.as_deref(),
        cfg,
        now_unix,
    ) {
        Ok(_) => (ASSURANCE_DEVICE_ATTESTED.to_string(), None),
        Err(why) => (ASSURANCE_LOCAL.to_string(), Some(why)),
    }
}

/// The nonce a stored record must attest: the session nonce for the
/// device's own attestation; for a paired phone, the companion nonce, once
/// the binding's learner signature verifies and the binding names the
/// profile's learner.
fn expected_nonce(
    att: &DeviceAttestation,
    facts: &SessionAttestationFacts,
) -> Result<[u8; 32], Unverified> {
    let nonce = session_nonce(&facts.session_id, &facts.started_at);
    let Some(binding) = &att.companion else {
        return Ok(nonce);
    };
    if facts.learner_did.as_deref() != Some(binding.learner_did.as_str()) {
        return Err(Unverified::failed(
            alexandria_verify::attestation::InvalidReason::CompanionMismatch,
            "companion attestation is bound to another learner",
        ));
    }
    crate::sentinel::companion::bound_nonce(att, binding, &nonce)
}

/// Verify a stored attestation against the session it claims to bind, with
/// this build's trust inputs ([`alexandria_verify::attestation::verify_record`]).
///
/// `expected_nonce` is [`session_nonce`] recomputed from the row;
/// `commitment_root` is the row's terminal root (an iOS assertion over it
/// is required when present). `now_unix` is used for certificate validity.
pub fn verify(
    att: &DeviceAttestation,
    expected_nonce: &[u8; 32],
    commitment_root: Option<&str>,
    cfg: &TrustConfig,
    now_unix: i64,
) -> Result<Verified, Unverified> {
    alexandria_verify::attestation::verify_record(
        att,
        expected_nonce,
        commitment_root,
        &cfg.inputs(),
        now_unix,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use alexandria_verify::attestation::test_support::{build_assertion, build_world, AAGUID_PROD};

    #[test]
    fn a_nonce_mismatch_fails_before_any_crypto() {
        let att = DeviceAttestation {
            platform: Platform::Android,
            nonce_hex: "00".repeat(32),
            app_id: APP_BUNDLE_ID.into(),
            payload: AttestationPayload::PlayIntegrity {
                token: "x.y.z.w.v".into(),
            },
            captured_at: "now".into(),
            companion: None,
        };
        let err = verify(&att, &[1u8; 32], None, &TrustConfig::default(), 0).unwrap_err();
        assert!(matches!(err, Unverified::Failed { .. }));
    }

    #[test]
    fn android_without_response_keys_is_unverified_not_failed() {
        let nonce = [7u8; 32];
        let att = DeviceAttestation {
            platform: Platform::Android,
            nonce_hex: hex::encode(nonce),
            app_id: APP_BUNDLE_ID.into(),
            payload: AttestationPayload::PlayIntegrity {
                token: "x.y.z.w.v".into(),
            },
            captured_at: "now".into(),
            companion: None,
        };
        assert_eq!(
            verify(&att, &nonce, None, &TrustConfig::default(), 0).unwrap_err(),
            Unverified::NoTrustMaterial
        );
    }

    #[test]
    fn payload_platform_mismatch_is_refused() {
        let nonce = [7u8; 32];
        let att = DeviceAttestation {
            platform: Platform::Ios,
            nonce_hex: hex::encode(nonce),
            app_id: apple_app_id(),
            payload: AttestationPayload::PlayIntegrity {
                token: "x.y.z.w.v".into(),
            },
            captured_at: "now".into(),
            companion: None,
        };
        assert!(matches!(
            verify(&att, &nonce, None, &TrustConfig::default(), 0),
            Err(Unverified::Failed { .. })
        ));
    }

    /// A synthetic App Attest chain stored on a session raises the
    /// effective assurance; the same blob against the real Apple root, or
    /// with a stale nonce, falls back to local with a reason.
    #[test]
    fn effective_assurance_requires_a_fresh_verification() {
        let session_id = "isess_e2e";
        let started_at = "2026-10-10T10:00:00Z";
        let nonce = session_nonce(session_id, started_at);
        let w = build_world(&apple_app_id(), nonce, AAGUID_PROD, 0);
        let root = "deadbeef";
        let assertion = build_assertion(&w, root.as_bytes(), 1);
        use base64::Engine as _;
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        let att = DeviceAttestation {
            platform: Platform::Ios,
            nonce_hex: hex::encode(nonce),
            app_id: apple_app_id(),
            payload: AttestationPayload::AppAttest {
                key_id_b64: b64(&w.key_id),
                attestation_b64: b64(&w.attestation),
                assertion_b64: Some(b64(&assertion)),
                asserted_root: Some(root.into()),
            },
            captured_at: started_at.into(),
            companion: None,
        };
        let facts = SessionAttestationFacts {
            session_id: session_id.into(),
            started_at: started_at.into(),
            stored_assurance_level: ASSURANCE_DEVICE_ATTESTED.into(),
            attestation_json: Some(serde_json::to_string(&att).unwrap()),
            commitment_root: Some(root.into()),
            learner_did: None,
        };
        let cfg = TrustConfig {
            apple_root_pem: w.root_pem.clone(),
            ..TrustConfig::default()
        };
        let (level, why) = effective_assurance(&facts, &cfg, w.now);
        assert_eq!(level, ASSURANCE_DEVICE_ATTESTED);
        assert!(why.is_none());

        // The real root does not know this chain.
        let (level, why) = effective_assurance(&facts, &TrustConfig::default(), w.now);
        assert_eq!(level, ASSURANCE_LOCAL);
        assert!(matches!(why, Some(Unverified::Failed { .. })));

        // A different commitment root than the one asserted.
        let moved = SessionAttestationFacts {
            commitment_root: Some("cafebabe".into()),
            ..facts.clone()
        };
        let (level, why) = effective_assurance(&moved, &cfg, w.now);
        assert_eq!(level, ASSURANCE_LOCAL);
        assert!(
            matches!(why, Some(Unverified::Failed { ref reason, .. }) if reason.contains("different commitment root"))
        );

        // No attestation at all: plain local, no reason.
        let none = SessionAttestationFacts {
            attestation_json: None,
            ..facts.clone()
        };
        assert_eq!(
            effective_assurance(&none, &cfg, w.now),
            (ASSURANCE_LOCAL.to_string(), None)
        );
    }

    #[test]
    fn trust_inputs_name_this_app_and_carry_play_keys_only_when_known() {
        let inputs = TrustConfig::default().inputs();
        let apple = inputs.apple.expect("apple trust");
        assert_eq!(apple.app_id, apple_app_id());
        assert_eq!(apple.root_pem, APPLE_APP_ATTEST_ROOT_PEM);
        assert!(inputs.play_integrity.is_none());
        let keys = PlayIntegrityKeys {
            decryption_key_b64: "a".into(),
            verification_key_b64: "b".into(),
        };
        let with_keys = TrustConfig {
            play_integrity_keys: Some(keys.clone()),
            ..TrustConfig::default()
        }
        .inputs();
        let play = with_keys.play_integrity.expect("play trust");
        assert_eq!(play.package_name, APP_BUNDLE_ID);
        assert_eq!(play.keys, keys);
    }
}
