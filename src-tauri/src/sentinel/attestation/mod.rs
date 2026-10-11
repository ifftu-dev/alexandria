//! Device attestation — the `device_attested` assurance rung.
//!
//! Every Sentinel figure is device-reported, so every credential so far
//! carries `assuranceLevel: "local"`. Mobile platforms can do better: the
//! OS vendor will attest that a genuine, unmodified build of this app on a
//! genuine device produced a given nonce. Bound to the integrity session,
//! that turns "the client says so" into "the platform says this client was
//! ours". Desktop has no equivalent and stays `local`.
//!
//! Two attestation families, both verified **on the device that issues**
//! (local-first, no new service):
//!
//! * **iOS App Attest** — `DCAppAttestService` attests a fresh Secure
//!   Enclave key over our nonce at session start; at session end the same
//!   key signs the terminal commitment root (an assertion). Verification is
//!   pure crypto: the certificate chain to Apple's App Attestation Root CA
//!   (embedded below), the nonce extension, the key id, the app id hash and
//!   the counter. See `app_attest`.
//! * **Android Play Integrity** — Google returns an encrypted token (JWE
//!   around an ES256 JWS) carrying the verdict for our nonce. Decrypting and
//!   verifying it needs the developer's response keys from the Play
//!   Console; they come from the network profile and may be absent, in
//!   which case the token is stored and the session stays `local`. See
//!   `play_integrity`.
//!
//! What crosses IPC and is stored: the opaque attestation blobs plus the
//! nonce and app id. Nothing here identifies the person; the platform key
//! is per-app, per-install.

pub mod app_attest;
pub mod native;
pub mod play_integrity;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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

/// Apple App Attestation Root CA (public, from
/// <https://www.apple.com/certificateauthority/>). SHA-256 fingerprint
/// `1CB9823BA28BA6AD2D33A006941DE2AE4F513EF1D4E831B9F7E0FA7B6242C932`,
/// valid 2020-03-18 to 2045-03-15.
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

/// The challenge both platforms attest over. Derived, not random, so the
/// verifier can recompute it from the session row alone; the session id is
/// already unguessable (hash of enrollment + start instant).
pub fn session_nonce(session_id: &str, started_at: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"alexandria-device-attest-v1\0");
    h.update(session_id.as_bytes());
    h.update(b"\0");
    h.update(started_at.as_bytes());
    h.finalize().into()
}

/// Which platform produced the attestation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Ios,
    Android,
}

/// The platform-specific blob(s). Opaque until verification.
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

/// Everything stored on `integrity_sessions.attestation_json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceAttestation {
    pub platform: Platform,
    /// Hex of [`session_nonce`] at capture time; the verifier recomputes
    /// and compares rather than trusting this field.
    pub nonce_hex: String,
    /// `TEAMID.bundle` (iOS) or the package name (Android).
    pub app_id: String,
    pub payload: AttestationPayload,
    pub captured_at: String,
}

/// Play Integrity response keys from the Play Console ("Manage and download
/// my response encryption keys"). Base64 as the console hands them out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayIntegrityKeys {
    pub decryption_key_b64: String,
    pub verification_key_b64: String,
}

/// Trust material the verifier runs against.
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
}

/// The session columns the assurance decision reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionAttestationFacts {
    pub session_id: String,
    pub started_at: String,
    pub stored_assurance_level: String,
    pub attestation_json: Option<String>,
    pub commitment_root: Option<String>,
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
    let Some(json) = facts.attestation_json.as_deref() else {
        return (ASSURANCE_LOCAL.to_string(), None);
    };
    let att: DeviceAttestation = match serde_json::from_str(json) {
        Ok(a) => a,
        Err(e) => {
            return (
                ASSURANCE_LOCAL.to_string(),
                Some(Unverified::Failed {
                    reason: format!("stored attestation is unreadable: {e}"),
                }),
            )
        }
    };
    let nonce = session_nonce(&facts.session_id, &facts.started_at);
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

/// Why a stored attestation did not raise the session's assurance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum Unverified {
    /// Nothing to verify with yet (Play Integrity keys absent).
    NoTrustMaterial,
    /// The blob is malformed or its proof fails. Treated as `local`, and
    /// surfaced so an operator can see a tampered or stale attestation.
    Failed { reason: String },
}

/// What a verified attestation establishes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Verified {
    pub platform: Platform,
    /// `appattest` / `appattestdevelop` on iOS; the device verdict labels
    /// on Android.
    pub environment: String,
    /// True when the terminal commitment root is covered by the
    /// attestation (iOS assertion present and valid).
    pub covers_commitment: bool,
}

/// Verify a stored attestation against the session it claims to bind.
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
    if att.nonce_hex != hex::encode(expected_nonce) {
        return Err(Unverified::Failed {
            reason: "attestation nonce does not match the session".into(),
        });
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
            if att.app_id != apple_app_id() {
                return Err(Unverified::Failed {
                    reason: format!("attestation app id {} is not ours", att.app_id),
                });
            }
            let key_id = b64(key_id_b64)?;
            let attestation = b64(attestation_b64)?;
            let app_id_hash: [u8; 32] = Sha256::digest(att.app_id.as_bytes()).into();
            let attested = app_attest::verify_attestation(
                &attestation,
                &key_id,
                expected_nonce,
                &app_id_hash,
                &cfg.apple_root_pem,
                cfg.allow_apple_development,
                now_unix,
            )
            .map_err(|e| Unverified::Failed {
                reason: format!("app attest: {e}"),
            })?;
            let covers_commitment = match (commitment_root, assertion_b64, asserted_root) {
                (Some(root), Some(assertion), Some(asserted)) => {
                    if asserted != root {
                        return Err(Unverified::Failed {
                            reason: "assertion covers a different commitment root".into(),
                        });
                    }
                    let assertion = b64(assertion)?;
                    app_attest::verify_assertion(
                        &assertion,
                        root.as_bytes(),
                        &attested.public_key_sec1,
                        &app_id_hash,
                        attested.counter,
                    )
                    .map_err(|e| Unverified::Failed {
                        reason: format!("app attest assertion: {e}"),
                    })?;
                    true
                }
                (_, None, _) | (None, _, _) => false,
                (Some(_), Some(_), None) => {
                    return Err(Unverified::Failed {
                        reason: "assertion present without the root it signs".into(),
                    })
                }
            };
            Ok(Verified {
                platform: Platform::Ios,
                environment: attested.environment,
                covers_commitment,
            })
        }
        (Platform::Android, AttestationPayload::PlayIntegrity { token }) => {
            let Some(keys) = &cfg.play_integrity_keys else {
                return Err(Unverified::NoTrustMaterial);
            };
            if att.app_id != APP_BUNDLE_ID {
                return Err(Unverified::Failed {
                    reason: format!("attestation package {} is not ours", att.app_id),
                });
            }
            let verdict = play_integrity::decrypt_and_verify(token, keys).map_err(|e| {
                Unverified::Failed {
                    reason: format!("play integrity: {e}"),
                }
            })?;
            play_integrity::check_verdict(&verdict, expected_nonce, APP_BUNDLE_ID).map_err(
                |e| Unverified::Failed {
                    reason: format!("play integrity verdict: {e}"),
                },
            )?;
            Ok(Verified {
                platform: Platform::Android,
                environment: play_integrity::device_labels(&verdict).join(","),
                covers_commitment: false,
            })
        }
        _ => Err(Unverified::Failed {
            reason: "attestation payload does not match its platform".into(),
        }),
    }
}

fn b64(s: &str) -> Result<Vec<u8>, Unverified> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s.trim()))
        .map_err(|e| Unverified::Failed {
            reason: format!("attestation field is not base64: {e}"),
        })
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
        use super::app_attest::tests::{build_assertion, build_world};
        let session_id = "isess_e2e";
        let started_at = "2026-10-10T10:00:00Z";
        let nonce = session_nonce(session_id, started_at);
        let w = build_world(&apple_app_id(), nonce, b"appattest\0\0\0\0\0\0\0", 0);
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
        };
        let facts = SessionAttestationFacts {
            session_id: session_id.into(),
            started_at: started_at.into(),
            stored_assurance_level: ASSURANCE_DEVICE_ATTESTED.into(),
            attestation_json: Some(serde_json::to_string(&att).unwrap()),
            commitment_root: Some(root.into()),
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
            matches!(why, Some(Unverified::Failed { ref reason }) if reason.contains("different commitment root"))
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
    fn stored_shape_round_trips_with_snake_case_tags() {
        let att = DeviceAttestation {
            platform: Platform::Ios,
            nonce_hex: "ab".repeat(32),
            app_id: apple_app_id(),
            payload: AttestationPayload::AppAttest {
                key_id_b64: "a2V5".into(),
                attestation_b64: "YXR0".into(),
                assertion_b64: None,
                asserted_root: None,
            },
            captured_at: "2026-10-10T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&att).unwrap();
        assert!(json.contains("\"platform\":\"ios\""));
        assert!(json.contains("\"kind\":\"app_attest\""));
        assert!(!json.contains("assertion_b64"));
        let back: DeviceAttestation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, att);
    }
}
