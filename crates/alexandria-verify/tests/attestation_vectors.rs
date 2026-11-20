//! Interoperability vectors for device attestation evidence.
//!
//! `tests/vectors/attestation/*.json` is the authority. Each file is
//! self-contained: a signed credential carrying (or not carrying) an
//! attestation entry in `evidence`, the trust inputs a verifier is given (a
//! synthetic App Attest root standing in for Apple's, synthetic Play
//! Integrity response keys), the verification time, the policy, and the
//! expected attestation outcome, verified assurance level and decision.
//!
//! Built with the `attestation` feature, the crate must reproduce every
//! expectation. Built without it, evidence still parses: an entry that parses
//! is `unverifiable` and never rejects, a duplicated or unparsable one is still
//! invalid.
//!
//! # Regenerating
//!
//! ```sh
//! ALEXANDRIA_REGENERATE_VECTORS=1 cargo test -p alexandria-verify --features attestation --test attestation_vectors
//! ```
//!
//! Every key, certificate and token derives from fixed labels, so regeneration
//! is byte-identical. Expectations are written by hand below, not computed by
//! the verifier under test.

#[allow(dead_code)]
#[path = "../src/attestation/test_support.rs"]
mod fixtures;

use std::fs;
use std::path::PathBuf;

use ed25519_dalek::{Signer as _, SigningKey};
use serde::{Deserialize, Serialize};

use alexandria_verify::attestation::{
    binding_digest_of, companion_nonce, companion_statement, session_nonce, AppleTrust,
    AttestationEvidence, AttestationOutcome, AttestationPayload, AttestationTrust,
    CompanionBinding, DeviceAttestation, InvalidReason, Platform, PlayIntegrityKeys,
    PlayIntegrityTrust, VerifiedAssuranceLevel,
};
use alexandria_verify::did::{derive_did_key, Did};
use alexandria_verify::vc::sign::{sign_credential, UnsignedCredential};
use alexandria_verify::vc::verify::verify_credential_with_attestation;
use alexandria_verify::vc::{
    AcceptanceDecision, CredentialSubject, IntegrityAssertion, Proof, VerifiableCredential,
    VerificationPolicy,
};
use alexandria_verify::NullStore;

use fixtures::{
    b64, build_assertion, build_world_seeded, mint_play_token, p256_key, play_keys_b64,
    play_verdict, World, AAGUID_PROD,
};

// ---------------------------------------------------------------------------
// Vector format
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Vector {
    description: String,
    verification_time: String,
    policy: VerificationPolicy,
    /// What the verifier trusts. A platform left out makes its evidence
    /// unverifiable.
    trust: AttestationTrust,
    credential: VerifiableCredential,
    expect: Expect,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expect {
    attestation: AttestationOutcome,
    verified_assurance_level: VerifiedAssuranceLevel,
    acceptance_decision: AcceptanceDecision,
}

// ---------------------------------------------------------------------------
// Fixed material
// ---------------------------------------------------------------------------

const NOW: &str = "2026-06-01T00:00:00Z";
const SESSION: &str = "isess_vector";
const STARTED: &str = "2026-05-31T11:00:00Z";
const GENERATED: &str = "2026-05-31T12:00:00Z";
const ROOT: &str = "c0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ffeec0ff";
const APPLE_APP_ID: &str = "TESTTEAM.org.alexandria.vector";
const PACKAGE: &str = "org.alexandria.vector";
const PHONE: &str = "vector-phone-endpoint";
const KEK: [u8; 32] = [5u8; 32];

fn ed_key(label: &str) -> SigningKey {
    let mut bytes = [0u8; 32];
    let b = label.as_bytes();
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = b[i % b.len()];
    }
    SigningKey::from_bytes(&bytes)
}

fn integrity() -> IntegrityAssertion {
    IntegrityAssertion {
        session_id: SESSION.into(),
        status: "completed".into(),
        integrity_score: Some(0.97),
        critical_count: 0,
        warning_count: 1,
        assurance_level: "device_attested".into(),
        commitment_root: Some(ROOT.into()),
        anchor_ref: None,
        generated_at: GENERATED.into(),
        trust_profile: Some("attested".into()),
        verification_rung: Some("process".into()),
        coverage_statement_digest: None,
        signals_run: vec!["core".into(), "device_attestation".into()],
        signals_degraded: vec![],
        signals_not_run: vec!["camera".into()],
        camera_coverage: None,
        probe: None,
    }
}

/// What a desktop session backed by a paired phone states.
fn companion_integrity() -> IntegrityAssertion {
    IntegrityAssertion {
        assurance_level: "companion_attested".into(),
        ..integrity()
    }
}

fn subject(did: &Did, score: f64) -> CredentialSubject {
    CredentialSubject {
        id: did.clone(),
        properties: serde_json::from_value(serde_json::json!({
            "skillId": "skill_vector",
            "level": 3,
            "score": score,
            "evidenceRefs": []
        }))
        .expect("subject properties"),
    }
}

/// A self-issued assessment credential, unsigned, with `evidence`.
fn credential(
    learner: &Did,
    score: f64,
    evidence: Vec<AttestationEvidence>,
) -> VerifiableCredential {
    VerifiableCredential {
        context: vec!["https://www.w3.org/ns/credentials/v2".into()],
        id: Some("urn:uuid:attestation-vector".into()),
        type_: vec!["VerifiableCredential".into(), "AssessmentCredential".into()],
        issuer: learner.clone().into(),
        valid_from: GENERATED.into(),
        valid_until: None,
        credential_subject: subject(learner, score),
        credential_status: None,
        terms_of_use: None,
        witness: None,
        integrity: Some(integrity()),
        evidence: evidence.into_iter().map(|e| e.into_entry()).collect(),
        related_resource: Vec::new(),
        proof: Proof::unsigned(GENERATED),
    }
}

fn sign(vc: VerifiableCredential, key: &SigningKey) -> VerifiableCredential {
    let did = derive_did_key(key);
    sign_credential(UnsignedCredential { credential: vc }, key, &did).expect("sign")
}

fn digest(learner: &Did, score: f64) -> [u8; 32] {
    binding_digest_of(&subject(learner, score), &integrity()).expect("digest")
}

/// [`credential`] stating [`companion_integrity`].
fn companion_credential(
    learner: &Did,
    score: f64,
    evidence: Vec<AttestationEvidence>,
) -> VerifiableCredential {
    VerifiableCredential {
        integrity: Some(companion_integrity()),
        ..credential(learner, score, evidence)
    }
}

fn companion_digest(learner: &Did, score: f64) -> [u8; 32] {
    binding_digest_of(&subject(learner, score), &companion_integrity()).expect("digest")
}

fn ios_world(seed: &str, app_id: &str, attested_nonce: [u8; 32]) -> World {
    build_world_seeded(seed.as_bytes(), app_id, attested_nonce, AAGUID_PROD, 0)
}

/// The session's stored App Attest record, with its end assertion over ROOT.
fn ios_record(w: &World, app_id: &str, nonce: [u8; 32]) -> DeviceAttestation {
    DeviceAttestation {
        platform: Platform::Ios,
        nonce_hex: hex::encode(nonce),
        app_id: app_id.into(),
        payload: AttestationPayload::AppAttest {
            key_id_b64: w.key_id_b64(),
            attestation_b64: w.attestation_b64(),
            assertion_b64: Some(b64(&build_assertion(w, ROOT.as_bytes(), 1))),
            asserted_root: Some(ROOT.into()),
        },
        captured_at: STARTED.into(),
        companion: None,
    }
}

fn apple_trust(w: &World) -> AppleTrust {
    AppleTrust {
        root_pem: w.root_pem.clone(),
        app_id: APPLE_APP_ID.into(),
        allow_development: false,
    }
}

fn play_signer() -> p256::ecdsa::SigningKey {
    p256_key(b"vector-play-signer")
}

fn play_trust() -> PlayIntegrityTrust {
    let (decryption_key_b64, verification_key_b64) = play_keys_b64(&KEK, &play_signer());
    PlayIntegrityTrust {
        package_name: PACKAGE.into(),
        keys: PlayIntegrityKeys {
            decryption_key_b64,
            verification_key_b64,
        },
    }
}

fn play_token(nonce: &[u8; 32], iv: u8) -> String {
    mint_play_token(
        &play_verdict(nonce, PACKAGE),
        &KEK,
        &play_signer(),
        [iv; 12],
    )
}

fn vectors_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/attestation")
}

// ---------------------------------------------------------------------------
// The vectors
// ---------------------------------------------------------------------------

fn build_all() -> Vec<(&'static str, Vector)> {
    use AcceptanceDecision::{Accept, Reject};
    use VerifiedAssuranceLevel::{CompanionAttested, DeviceAttested, Local};

    let learner_key = ed_key("attestation-vector-learner");
    let learner = derive_did_key(&learner_key);
    let session = session_nonce(SESSION, STARTED);
    let world = ios_world("vector-ios", APPLE_APP_ID, session);
    let full_trust = AttestationTrust {
        apple: Some(apple_trust(&world)),
        play_integrity: Some(play_trust()),
    };
    let record = ios_record(&world, APPLE_APP_ID, session);
    let ios_bound = |score: f64| {
        AttestationEvidence::credential_bound(
            &record,
            STARTED,
            b64(&build_assertion(&world, &digest(&learner, score), 2)),
        )
    };
    let expect = |attestation, verified_assurance_level, acceptance_decision| Expect {
        attestation,
        verified_assurance_level,
        acceptance_decision,
    };
    let invalid = |reason| AttestationOutcome::Invalid { reason };
    let vector = |description: &str, trust: &AttestationTrust, vc, expect| Vector {
        description: description.into(),
        verification_time: NOW.into(),
        policy: VerificationPolicy::default(),
        trust: trust.clone(),
        credential: sign(vc, &learner_key),
        expect,
    };

    let mut out = Vec::new();

    out.push((
        "01-not-supplied",
        vector(
            "A self-issued credential with an integrity assertion and no attestation \
             evidence. It verifies as before; the outcome is not_supplied and the \
             verified level stays local, whatever level the assertion states.",
            &full_trust,
            credential(&learner, 0.9, vec![]),
            expect(AttestationOutcome::NotSupplied, Local, Accept),
        ),
    ));

    out.push((
        "02-ios-credential-bound",
        vector(
            "App Attest evidence: the attestation over the session nonce, an assertion \
             over the commitment root, and an assertion over this credential's binding \
             digest. Verified credential-bound; the verified level is device_attested.",
            &full_trust,
            credential(&learner, 0.9, vec![ios_bound(0.9)]),
            expect(
                AttestationOutcome::VerifiedCredentialBound {
                    platform: Platform::Ios,
                    companion: false,
                },
                DeviceAttested,
                Accept,
            ),
        ),
    ));

    let android_record = DeviceAttestation {
        platform: Platform::Android,
        nonce_hex: hex::encode(session),
        app_id: PACKAGE.into(),
        payload: AttestationPayload::PlayIntegrity {
            token: play_token(&session, 1),
        },
        captured_at: STARTED.into(),
        companion: None,
    };
    let android_bound = AttestationEvidence::credential_bound(
        &android_record,
        STARTED,
        play_token(&digest(&learner, 0.9), 2),
    );
    out.push((
        "03-android-credential-bound",
        vector(
            "Play Integrity evidence: a token over the session nonce and a token whose \
             nonce is this credential's binding digest, opened with the supplied \
             response keys. Verified credential-bound.",
            &full_trust,
            credential(&learner, 0.9, vec![android_bound.clone()]),
            expect(
                AttestationOutcome::VerifiedCredentialBound {
                    platform: Platform::Android,
                    companion: false,
                },
                DeviceAttested,
                Accept,
            ),
        ),
    ));

    out.push((
        "04-ios-session-bound",
        vector(
            "The session's own App Attest evidence without an assertion over the \
             binding digest. Verified session-bound: it never rejects and never raises \
             the verified level above local.",
            &full_trust,
            credential(
                &learner,
                0.9,
                vec![AttestationEvidence::session_bound(&record, STARTED)],
            ),
            expect(
                AttestationOutcome::VerifiedSessionBound {
                    platform: Platform::Ios,
                },
                Local,
                Accept,
            ),
        ),
    ));

    out.push((
        "05-unverifiable-without-play-keys",
        vector(
            "Valid Play Integrity evidence for a verifier given no Play Integrity \
             response keys. Unverifiable, not invalid: the credential is accepted at \
             local.",
            &AttestationTrust {
                apple: Some(apple_trust(&world)),
                play_integrity: None,
            },
            credential(&learner, 0.9, vec![android_bound]),
            expect(
                AttestationOutcome::Unverifiable {
                    platform: Platform::Android,
                },
                Local,
                Accept,
            ),
        ),
    ));

    let stranger_world = ios_world("vector-ios-other-root", APPLE_APP_ID, session);
    out.push((
        "06-invalid-untrusted-chain",
        vector(
            "App Attest evidence whose certificate chain leads to a root other than \
             the trusted one. Invalid with untrusted_chain; the credential is rejected.",
            &AttestationTrust {
                apple: Some(apple_trust(&stranger_world)),
                play_integrity: Some(play_trust()),
            },
            credential(&learner, 0.9, vec![ios_bound(0.9)]),
            expect(invalid(InvalidReason::UntrustedChain), Local, Reject),
        ),
    ));

    let other_session = session_nonce("isess_other", STARTED);
    let other_world = ios_world("vector-ios-other-session", APPLE_APP_ID, other_session);
    let other_record = DeviceAttestation {
        nonce_hex: hex::encode(session),
        ..ios_record(&other_world, APPLE_APP_ID, other_session)
    };
    out.push((
        "07-invalid-wrong-nonce",
        vector(
            "A genuine App Attest attestation made for another session's nonce, \
             presented for this session. Invalid with wrong_nonce.",
            &AttestationTrust {
                apple: Some(apple_trust(&other_world)),
                play_integrity: None,
            },
            credential(
                &learner,
                0.9,
                vec![AttestationEvidence::credential_bound(
                    &other_record,
                    STARTED,
                    b64(&build_assertion(&other_world, &digest(&learner, 0.9), 2)),
                )],
            ),
            expect(invalid(InvalidReason::WrongNonce), Local, Reject),
        ),
    ));

    let foreign_app = "OTHERTEAM.com.example.other";
    let foreign_world = ios_world("vector-ios-foreign-app", foreign_app, session);
    let foreign_record = ios_record(&foreign_world, foreign_app, session);
    out.push((
        "08-invalid-wrong-app-id",
        vector(
            "A genuine App Attest attestation from another app. Invalid with \
             wrong_app_id.",
            &AttestationTrust {
                apple: Some(apple_trust(&foreign_world)),
                play_integrity: None,
            },
            credential(
                &learner,
                0.9,
                vec![AttestationEvidence::credential_bound(
                    &foreign_record,
                    STARTED,
                    b64(&build_assertion(&foreign_world, &digest(&learner, 0.9), 2)),
                )],
            ),
            expect(invalid(InvalidReason::WrongAppId), Local, Reject),
        ),
    ));

    out.push((
        "09-invalid-lifted-from-another-credential",
        vector(
            "Credential-bound evidence from an honest credential (score 0.4) attached \
             to a credential stating a higher score from the same session. Its binding \
             assertion covers the other credential's digest: invalid with \
             binding_mismatch.",
            &full_trust,
            credential(&learner, 0.99, vec![ios_bound(0.4)]),
            expect(invalid(InvalidReason::BindingMismatch), Local, Reject),
        ),
    ));

    let paired = companion_nonce(&session, learner.as_str(), PHONE);
    let phone = ios_world("vector-ios-phone", APPLE_APP_ID, paired);
    let companion_record = |key: &SigningKey| {
        let did = derive_did_key(key);
        let mut record = DeviceAttestation {
            platform: Platform::Ios,
            nonce_hex: hex::encode(companion_nonce(&session, did.as_str(), PHONE)),
            app_id: APPLE_APP_ID.into(),
            payload: AttestationPayload::AppAttest {
                key_id_b64: phone.key_id_b64(),
                attestation_b64: phone.attestation_b64(),
                assertion_b64: None,
                asserted_root: None,
            },
            captured_at: STARTED.into(),
            companion: None,
        };
        let signature = key.sign(&companion_statement(&record, did.as_str(), PHONE));
        record.companion = Some(CompanionBinding {
            learner_did: did.as_str().into(),
            phone_endpoint: PHONE.into(),
            did_signature_b64: b64(&signature.to_bytes()),
        });
        record
    };
    let phone_trust = AttestationTrust {
        apple: Some(apple_trust(&phone)),
        play_integrity: None,
    };
    out.push((
        "10-companion-credential-bound",
        vector(
            "A desktop session backed by a paired phone: the phone's App Attest \
             attestation over the companion nonce (session nonce, learner DID, phone \
             endpoint), the learner's signature over the companion statement, and the \
             phone's assertion over the binding digest. The session states \
             companion_attested. Verified credential-bound through a companion: the phone was present and attested, nothing is \
             claimed about the desktop, so the verified level is companion_attested, \
             never device_attested.",
            &phone_trust,
            companion_credential(
                &learner,
                0.9,
                vec![AttestationEvidence::credential_bound(
                    &companion_record(&learner_key),
                    STARTED,
                    b64(&build_assertion(&phone, &companion_digest(&learner, 0.9), 1)),
                )],
            ),
            expect(
                AttestationOutcome::VerifiedCredentialBound {
                    platform: Platform::Ios,
                    companion: true,
                },
                CompanionAttested,
                Accept,
            ),
        ),
    ));

    let stranger = ed_key("attestation-vector-stranger");
    out.push((
        "11-invalid-companion-for-another-learner",
        vector(
            "A paired phone's evidence whose companion binding names another learner \
             than the credential subject. Invalid with companion_mismatch.",
            &phone_trust,
            credential(
                &learner,
                0.9,
                vec![AttestationEvidence::credential_bound(
                    &companion_record(&stranger),
                    STARTED,
                    b64(&build_assertion(&phone, &digest(&learner, 0.9), 1)),
                )],
            ),
            expect(invalid(InvalidReason::CompanionMismatch), Local, Reject),
        ),
    ));

    out.push((
        "12-invalid-duplicate-evidence",
        vector(
            "Two attestation entries in one credential. A credential carries at most \
             one: invalid with duplicate_evidence, with or without attestation support.",
            &full_trust,
            credential(&learner, 0.9, vec![ios_bound(0.9), ios_bound(0.9)]),
            expect(invalid(InvalidReason::DuplicateEvidence), Local, Reject),
        ),
    ));

    out
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

fn load() -> Vec<(PathBuf, Vector)> {
    let dir = vectors_dir();
    let mut files: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| {
            panic!(
                "read {}: {e}. Regenerate with ALEXANDRIA_REGENERATE_VECTORS=1",
                dir.display()
            )
        })
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no vectors in {}", dir.display());
    files
        .into_iter()
        .map(|path| {
            let raw = fs::read_to_string(&path).expect("read vector");
            let v: Vector =
                serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            (path, v)
        })
        .collect()
}

/// What a build without the `attestation` feature must report: parsed
/// evidence is unverifiable, unparsable or duplicated evidence is invalid.
#[cfg_attr(feature = "attestation", allow(dead_code))]
fn expected_without_feature(v: &Vector) -> Expect {
    let attestation = match AttestationEvidence::find(&v.credential) {
        Ok(None) => AttestationOutcome::NotSupplied,
        Ok(Some(e)) => AttestationOutcome::Unverifiable {
            platform: e.platform,
        },
        Err(reason) => AttestationOutcome::Invalid { reason },
    };
    Expect {
        acceptance_decision: if attestation.rejects() {
            AcceptanceDecision::Reject
        } else {
            AcceptanceDecision::Accept
        },
        attestation,
        verified_assurance_level: VerifiedAssuranceLevel::Local,
    }
}

#[test]
fn attestation_vectors_match_this_implementation() {
    if std::env::var("ALEXANDRIA_REGENERATE_VECTORS").is_ok() {
        regenerate();
        return;
    }
    for (path, v) in load() {
        let got = verify_credential_with_attestation(
            &NullStore,
            &v.credential,
            &v.verification_time,
            &v.policy,
            &v.trust,
        );
        let got = Expect {
            attestation: got.attestation,
            verified_assurance_level: got.verified_assurance_level,
            acceptance_decision: got.acceptance_decision,
        };
        #[cfg(feature = "attestation")]
        let want = v.expect.clone();
        #[cfg(not(feature = "attestation"))]
        let want = expected_without_feature(&v);
        assert_eq!(got, want, "{}", path.display());
    }
}

/// The committed files are what `build_all` produces, so the hand-written
/// expectations above are the ones the files carry.
#[test]
fn committed_vectors_are_the_built_ones() {
    if std::env::var("ALEXANDRIA_REGENERATE_VECTORS").is_ok() {
        return;
    }
    let built = build_all();
    let loaded = load();
    assert_eq!(
        built.len(),
        loaded.len(),
        "regenerate the attestation vectors"
    );
    for ((name, b), (path, l)) in built.iter().zip(&loaded) {
        assert_eq!(
            path.file_stem().and_then(|s| s.to_str()),
            Some(*name),
            "regenerate the attestation vectors"
        );
        assert_eq!(
            serde_json::to_value(b).unwrap(),
            serde_json::to_value(l).unwrap(),
            "{} differs from what the suite builds; regenerate",
            path.display()
        );
    }
}

/// Every outcome, and each named rejection, appears in the suite.
#[test]
fn the_suite_covers_every_outcome() {
    let all = build_all();
    let has =
        |f: &dyn Fn(&AttestationOutcome) -> bool| all.iter().any(|(_, v)| f(&v.expect.attestation));
    use AttestationOutcome as O;
    assert!(has(&|o| matches!(o, O::NotSupplied)));
    assert!(has(&|o| matches!(o, O::VerifiedCredentialBound { .. })));
    assert!(has(&|o| matches!(o, O::VerifiedSessionBound { .. })));
    assert!(has(&|o| matches!(o, O::Unverifiable { .. })));
    for level in VerifiedAssuranceLevel::ALL {
        assert!(
            all.iter()
                .any(|(_, v)| v.expect.verified_assurance_level == level),
            "no vector verifies as {level:?}"
        );
    }
    for reason in [
        InvalidReason::UntrustedChain,
        InvalidReason::WrongNonce,
        InvalidReason::WrongAppId,
        InvalidReason::BindingMismatch,
    ] {
        assert!(
            has(&|o| *o == O::Invalid { reason }),
            "no vector for {reason:?}"
        );
    }
}

fn regenerate() {
    let dir = vectors_dir();
    fs::create_dir_all(&dir).expect("create vectors dir");
    for entry in fs::read_dir(&dir).expect("read dir").flatten() {
        let p = entry.path();
        if p.extension().is_some_and(|x| x == "json") {
            fs::remove_file(p).expect("remove stale vector");
        }
    }
    for (name, v) in build_all() {
        let path = dir.join(format!("{name}.json"));
        let json = serde_json::to_string_pretty(&v).expect("serialize vector");
        fs::write(&path, json + "\n").expect("write vector");
        println!("wrote {}", path.display());
    }
}
