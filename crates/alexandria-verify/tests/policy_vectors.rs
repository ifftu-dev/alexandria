//! Interoperability vectors for the shared issuance policy rule.
//!
//! `tests/vectors/issuance-policy/*.json` is the authority. Each file is
//! self-contained: an issuance policy, the integrity assertion a credential
//! carries, the attestation outcome a verifier established for that
//! credential, and the expected result: no refusal, or the first unmet
//! requirement with its stable code. A vector with an `assessed` entry is a
//! role credential issued on an assessed credential, judged from that entry
//! (`evaluate_assessed`). The app and the hosted service judge a
//! role's policy with this one rule, so both must reproduce every file.
//!
//! # Regenerating
//!
//! ```sh
//! ALEXANDRIA_REGENERATE_VECTORS=1 cargo test -p alexandria-verify --test policy_vectors
//! ```
//!
//! Expectations are written by hand below, not computed by the rule under
//! test.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use alexandria_verify::assessed::{AssessedCredentialEvidence, ASSESSED_CREDENTIAL_EVIDENCE_TYPE};
use alexandria_verify::attestation::{AttestationOutcome, Platform, VerifiedAssuranceLevel};
use alexandria_verify::integrity::{TrustProfile, VerificationRung};
use alexandria_verify::policy::{
    evaluate, evaluate_assessed, IssuancePolicy, PolicyRefusal, ProbePolicy,
};
use alexandria_verify::vc::{IntegrityAssertion, ProbeSummary};

// ---------------------------------------------------------------------------
// Vector format
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Vector {
    description: String,
    policy: IssuancePolicy,
    integrity: IntegrityAssertion,
    attestation: AttestationOutcome,
    /// For a role credential: the entry naming the credential it was issued
    /// on, from which the rule takes its assurance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    assessed: Option<AssessedCredentialEvidence>,
    expect: Expect,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expect {
    /// The first unmet requirement; `null` when the policy is satisfied.
    refusal: Option<PolicyRefusal>,
}

fn vectors_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/issuance-policy")
}

// ---------------------------------------------------------------------------
// The vectors
// ---------------------------------------------------------------------------

/// A clean session on the `camera` profile that reached `probed` under the
/// `reseed` probe policy.
fn clean() -> IntegrityAssertion {
    IntegrityAssertion {
        session_id: "isess_policy_vector".into(),
        status: "completed".into(),
        integrity_score: Some(0.91),
        critical_count: 0,
        warning_count: 1,
        assurance_level: "local".into(),
        commitment_root: None,
        anchor_ref: None,
        generated_at: "2026-06-01T00:00:00Z".into(),
        trust_profile: Some("camera".into()),
        verification_rung: Some("probed".into()),
        coverage_statement_digest: None,
        signals_run: vec!["core".into(), "camera".into()],
        signals_degraded: vec![],
        signals_not_run: vec!["device_attestation".into()],
        probe: Some(ProbeSummary {
            outcome: "satisfied".into(),
            policies: vec!["reseed".into()],
            satisfied: 2,
            failed: 0,
            skipped: 0,
        }),
    }
}

/// The same session locked to `attested`, its device attestation run.
fn attested() -> IntegrityAssertion {
    IntegrityAssertion {
        assurance_level: "device_attested".into(),
        trust_profile: Some("attested".into()),
        signals_run: vec!["core".into(), "device_attestation".into()],
        signals_not_run: vec!["camera".into()],
        ..clean()
    }
}

fn build_all() -> Vec<(&'static str, Vector)> {
    let credential_bound = AttestationOutcome::VerifiedCredentialBound {
        platform: Platform::Ios,
    };
    let session_bound = AttestationOutcome::VerifiedSessionBound {
        platform: Platform::Ios,
    };
    let none = AttestationOutcome::NotSupplied;
    let vector = |description: &str,
                  policy: IssuancePolicy,
                  integrity: IntegrityAssertion,
                  attestation: AttestationOutcome,
                  refusal: Option<PolicyRefusal>| Vector {
        description: description.into(),
        policy,
        integrity,
        attestation,
        assessed: None,
        expect: Expect { refusal },
    };
    let role_on = |description: &str,
                   assessed_outcome: AttestationOutcome,
                   stated: VerifiedAssuranceLevel,
                   refusal: Option<PolicyRefusal>| Vector {
        description: description.into(),
        policy: IssuancePolicy {
            required_assurance_level: Some("device_attested".into()),
            ..Default::default()
        },
        integrity: attested(),
        // The role credential's own evidence: the issuer's device could
        // only carry the session's.
        attestation: session_bound,
        assessed: Some(AssessedCredentialEvidence {
            type_: ASSESSED_CREDENTIAL_EVIDENCE_TYPE.into(),
            credential_id: Some("urn:uuid:11111111-1111-4111-8111-111111111111".into()),
            integrity_hash: "4f0c3a1e9b2d7c6a5e8f1b0d2c4a6e8f0b1d3c5a7e9f2b4d6c8a0e1f3b5d7c9a"
                .into(),
            attestation: assessed_outcome,
            verified_assurance_level: stated,
        }),
        expect: Expect { refusal },
    };
    let every_bound = IssuancePolicy {
        min_integrity: Some(0.8),
        max_critical: Some(0),
        max_warning: Some(1),
        require_clean: true,
        required_assurance_level: Some("local".into()),
        probe_policy: Some(ProbePolicy::Reseed),
        probe_connection_id: None,
        min_trust_profile: Some(TrustProfile::Camera),
        min_verification_rung: Some(VerificationRung::Probed),
    };

    vec![
        (
            "01-satisfied",
            vector(
                "Every bound set and met. A required `local` level is met by any \
                 credential.",
                every_bound.clone(),
                clean(),
                none,
                None,
            ),
        ),
        (
            "02-not-clean",
            vector(
                "A clean session is required and the session was flagged.",
                IssuancePolicy {
                    require_clean: true,
                    ..Default::default()
                },
                IntegrityAssertion {
                    status: "flagged".into(),
                    ..clean()
                },
                none,
                Some(PolicyRefusal::NotClean {
                    status: "flagged".into(),
                }),
            ),
        ),
        (
            "03-integrity-below-minimum",
            vector(
                "The integrity score is below the policy's minimum.",
                IssuancePolicy {
                    min_integrity: Some(0.95),
                    ..Default::default()
                },
                clean(),
                none,
                Some(PolicyRefusal::IntegrityBelowMinimum {
                    score: 0.91,
                    min: 0.95,
                }),
            ),
        ),
        (
            "04-no-integrity-score",
            vector(
                "A minimum score is required and the session recorded none.",
                IssuancePolicy {
                    min_integrity: Some(0.5),
                    ..Default::default()
                },
                IntegrityAssertion {
                    integrity_score: None,
                    ..clean()
                },
                none,
                Some(PolicyRefusal::NoIntegrityScore),
            ),
        ),
        (
            "05-too-many-critical",
            vector(
                "More critical flags than the policy tolerates.",
                IssuancePolicy {
                    max_critical: Some(0),
                    ..Default::default()
                },
                IntegrityAssertion {
                    critical_count: 2,
                    ..clean()
                },
                none,
                Some(PolicyRefusal::TooManyCritical { count: 2, max: 0 }),
            ),
        ),
        (
            "06-too-many-warnings",
            vector(
                "More warning flags than the policy tolerates.",
                IssuancePolicy {
                    max_warning: Some(0),
                    ..Default::default()
                },
                clean(),
                none,
                Some(PolicyRefusal::TooManyWarnings { count: 1, max: 0 }),
            ),
        ),
        (
            "07-device-attested-session-bound",
            vector(
                "device_attested is required. The assertion states device_attested and \
                 the evidence verifies, but only session-bound: the platform vouched for \
                 the session, not this credential, so the verified level is local.",
                IssuancePolicy {
                    required_assurance_level: Some("device_attested".into()),
                    ..Default::default()
                },
                attested(),
                session_bound,
                Some(PolicyRefusal::AssuranceBelowRequired {
                    level: "local".into(),
                    required: "device_attested".into(),
                }),
            ),
        ),
        (
            "08-device-attested-credential-bound",
            vector(
                "device_attested is required and the evidence verifies credential-bound.",
                IssuancePolicy {
                    required_assurance_level: Some("device_attested".into()),
                    ..Default::default()
                },
                attested(),
                credential_bound,
                None,
            ),
        ),
        (
            "09-device-attested-stated-not-evidenced",
            vector(
                "device_attested is required. The assertion states it, but the credential \
                 carries no evidence: the stated level is the issuing device's word.",
                IssuancePolicy {
                    required_assurance_level: Some("device_attested".into()),
                    ..Default::default()
                },
                attested(),
                none,
                Some(PolicyRefusal::AssuranceBelowRequired {
                    level: "local".into(),
                    required: "device_attested".into(),
                }),
            ),
        ),
        (
            "10-reserved-assurance-level",
            vector(
                "A reserved level no verified path can reach is never met.",
                IssuancePolicy {
                    required_assurance_level: Some("anchored".into()),
                    ..Default::default()
                },
                attested(),
                credential_bound,
                Some(PolicyRefusal::AssuranceBelowRequired {
                    level: "device_attested".into(),
                    required: "anchored".into(),
                }),
            ),
        ),
        (
            "11-trust-profile-below-floor",
            vector(
                "A camera floor and a session on the core profile.",
                IssuancePolicy {
                    min_trust_profile: Some(TrustProfile::Camera),
                    ..Default::default()
                },
                IntegrityAssertion {
                    trust_profile: Some("core".into()),
                    signals_run: vec!["core".into()],
                    signals_not_run: vec!["device_attestation".into(), "camera".into()],
                    ..clean()
                },
                none,
                Some(PolicyRefusal::TrustProfileBelowFloor {
                    profile: "core".into(),
                    required: "camera".into(),
                }),
            ),
        ),
        (
            "12-floor-signal-degraded",
            vector(
                "A camera floor and a camera session whose camera was degraded: a \
                 verifier cannot tell how much of the session it covered, so a degraded \
                 signal does not carry a floor.",
                IssuancePolicy {
                    min_trust_profile: Some(TrustProfile::Camera),
                    ..Default::default()
                },
                IntegrityAssertion {
                    signals_degraded: vec!["camera".into()],
                    ..clean()
                },
                none,
                Some(PolicyRefusal::TrustProfileBelowFloor {
                    profile: "camera".into(),
                    required: "camera".into(),
                }),
            ),
        ),
        (
            "13-no-trust-profile",
            vector(
                "Any floor, even core, and a session that never chose a profile.",
                IssuancePolicy {
                    min_trust_profile: Some(TrustProfile::Core),
                    ..Default::default()
                },
                IntegrityAssertion {
                    trust_profile: None,
                    signals_run: vec![],
                    signals_not_run: vec![],
                    ..clean()
                },
                none,
                Some(PolicyRefusal::TrustProfileBelowFloor {
                    profile: "none".into(),
                    required: "core".into(),
                }),
            ),
        ),
        (
            "14-attested-floor-without-verified-evidence",
            vector(
                "An attested floor and an attested session whose evidence is not \
                 supplied: the device attestation signal counts only when its evidence \
                 verifies.",
                IssuancePolicy {
                    min_trust_profile: Some(TrustProfile::Attested),
                    ..Default::default()
                },
                attested(),
                none,
                Some(PolicyRefusal::TrustProfileBelowFloor {
                    profile: "attested".into(),
                    required: "attested".into(),
                }),
            ),
        ),
        (
            "15-attested-floor-session-bound",
            vector(
                "An attested floor and session-bound evidence that verifies: the \
                 platform vouched for the session's attestation, which is what the \
                 profile runs.",
                IssuancePolicy {
                    min_trust_profile: Some(TrustProfile::Attested),
                    ..Default::default()
                },
                attested(),
                session_bound,
                None,
            ),
        ),
        (
            "16-rung-below-minimum",
            vector(
                "A minimum rung above the one the session reached.",
                IssuancePolicy {
                    min_verification_rung: Some(VerificationRung::Consistent),
                    ..Default::default()
                },
                clean(),
                none,
                Some(PolicyRefusal::RungBelowMinimum {
                    rung: "probed".into(),
                    required: "consistent".into(),
                }),
            ),
        ),
        (
            "17-probe-policy-not-run",
            vector(
                "The role's probe policy is `authored` and the probes ran under \
                 `reseed`.",
                IssuancePolicy {
                    probe_policy: Some(ProbePolicy::Authored),
                    ..Default::default()
                },
                clean(),
                none,
                Some(PolicyRefusal::ProbePolicyNotRun {
                    policy: "authored".into(),
                }),
            ),
        ),
        (
            "18-probe-failed",
            vector(
                "The role's probe policy ran and a probe failed.",
                IssuancePolicy {
                    probe_policy: Some(ProbePolicy::Reseed),
                    ..Default::default()
                },
                IntegrityAssertion {
                    probe: Some(ProbeSummary {
                        outcome: "failed".into(),
                        policies: vec!["reseed".into()],
                        satisfied: 1,
                        failed: 1,
                        skipped: 0,
                    }),
                    ..clean()
                },
                none,
                Some(PolicyRefusal::ProbeFailed {
                    policy: "reseed".into(),
                }),
            ),
        ),
        (
            "19-first-unmet-requirement",
            vector(
                "Several bounds unmet: the refusal is the first in the rule's order \
                 (status, score, flags, assurance, floor, rung, probes).",
                every_bound,
                IntegrityAssertion {
                    status: "suspended".into(),
                    integrity_score: Some(0.2),
                    critical_count: 3,
                    probe: None,
                    ..clean()
                },
                none,
                Some(PolicyRefusal::NotClean {
                    status: "suspended".into(),
                }),
            ),
        ),
        (
            "20-role-on-credential-bound-assessment",
            role_on(
                "A role credential requiring `device_attested`, issued on an assessed \
                 credential whose evidence verified credential-bound. Its own evidence is \
                 only session-bound; the entry's verified level meets the requirement.",
                credential_bound,
                VerifiedAssuranceLevel::DeviceAttested,
                None,
            ),
        ),
        (
            "21-role-on-session-bound-assessment",
            role_on(
                "A role credential requiring `device_attested`, issued on an assessed \
                 credential whose evidence verified only session-bound.",
                session_bound,
                VerifiedAssuranceLevel::Local,
                Some(PolicyRefusal::AssuranceBelowRequired {
                    level: "local".into(),
                    required: "device_attested".into(),
                }),
            ),
        ),
        (
            "22-role-entry-level-not-evidenced",
            role_on(
                "The entry states `device_attested` beside an outcome that does not \
                 verify it: judged `local`.",
                none,
                VerifiedAssuranceLevel::DeviceAttested,
                Some(PolicyRefusal::AssuranceBelowRequired {
                    level: "local".into(),
                    required: "device_attested".into(),
                }),
            ),
        ),
    ]
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

#[test]
fn policy_vectors_match_this_implementation() {
    if std::env::var("ALEXANDRIA_REGENERATE_VECTORS").is_ok() {
        regenerate();
        return;
    }
    for (path, v) in load() {
        let judged = match &v.assessed {
            Some(assessed) => evaluate_assessed(&v.policy, &v.integrity, assessed),
            None => evaluate(&v.policy, &v.integrity, &v.attestation),
        };
        let got = Expect {
            refusal: judged.err(),
        };
        assert_eq!(got, v.expect, "{}", path.display());
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
    assert_eq!(built.len(), loaded.len(), "regenerate the policy vectors");
    for ((name, b), (path, l)) in built.iter().zip(&loaded) {
        assert_eq!(
            path.file_stem().and_then(|s| s.to_str()),
            Some(*name),
            "regenerate the policy vectors"
        );
        assert_eq!(
            serde_json::to_value(b).unwrap(),
            serde_json::to_value(l).unwrap(),
            "{} differs from what the suite builds; regenerate",
            path.display()
        );
    }
}

/// Every refusal code appears in the suite, and each code is the one its
/// vector's file carries on the wire.
#[test]
fn the_suite_covers_every_refusal_code() {
    let codes: Vec<String> = build_all()
        .iter()
        .filter_map(|(_, v)| v.expect.refusal.as_ref())
        .map(|r| {
            let wire = serde_json::to_value(r).unwrap();
            assert_eq!(wire["code"], r.code(), "{r:?}");
            r.code().to_string()
        })
        .collect();
    for code in [
        "not_clean",
        "integrity_below_minimum",
        "no_integrity_score",
        "too_many_critical",
        "too_many_warnings",
        "assurance_below_required",
        "trust_profile_below_floor",
        "rung_below_minimum",
        "probe_policy_not_run",
        "probe_failed",
    ] {
        assert!(codes.iter().any(|c| c == code), "no vector for {code}");
    }
}

/// Policies stored before the rule moved into this library (role rows in
/// the app's database) deserialize unchanged and serialize back to the same
/// JSON.
#[test]
fn stored_role_policies_deserialize_unchanged() {
    for stored in [
        r#"{"min_trust_profile":"attested"}"#,
        r#"{"min_integrity":0.7,"max_critical":null,"max_warning":null,"require_clean":true,"required_assurance_level":null,"probe_policy":null,"min_trust_profile":null,"min_verification_rung":null}"#,
        r#"{"min_integrity":null,"max_critical":0,"max_warning":2,"require_clean":false,"required_assurance_level":"device_attested","probe_policy":"llm","probe_connection_id":"conn","min_trust_profile":"camera","min_verification_rung":"probed"}"#,
    ] {
        let policy: IssuancePolicy = serde_json::from_str(stored).expect(stored);
        let back: IssuancePolicy =
            serde_json::from_str(&serde_json::to_string(&policy).unwrap()).unwrap();
        assert_eq!(policy, back, "{stored}");
    }
    let full: IssuancePolicy = serde_json::from_str(
        r#"{"min_integrity":null,"max_critical":0,"max_warning":2,"require_clean":false,"required_assurance_level":"device_attested","probe_policy":"llm","probe_connection_id":"conn","min_trust_profile":"camera","min_verification_rung":"probed"}"#,
    )
    .unwrap();
    assert_eq!(
        full,
        IssuancePolicy {
            min_integrity: None,
            max_critical: Some(0),
            max_warning: Some(2),
            require_clean: false,
            required_assurance_level: Some("device_attested".into()),
            probe_policy: Some(ProbePolicy::Llm),
            probe_connection_id: Some("conn".into()),
            min_trust_profile: Some(TrustProfile::Camera),
            min_verification_rung: Some(VerificationRung::Probed),
        }
    );
    // The serialized shape is the one the app has always written: every
    // field by its snake_case name, the connection id only when set.
    assert_eq!(
        serde_json::to_value(IssuancePolicy {
            require_clean: true,
            ..Default::default()
        })
        .unwrap(),
        serde_json::json!({
            "min_integrity": null,
            "max_critical": null,
            "max_warning": null,
            "require_clean": true,
            "required_assurance_level": null,
            "probe_policy": null,
            "min_trust_profile": null,
            "min_verification_rung": null,
        })
    );
}

/// The device_attested pair judged from real evidence: the attestation
/// vectors' iOS credential-bound and session-bound credentials, verified with
/// their synthetic platform root, then judged by the rule.
#[cfg(feature = "attestation")]
#[test]
fn device_attested_is_met_by_credential_bound_evidence_only() {
    use alexandria_verify::attestation::{verify_evidence, AttestationTrust};
    use alexandria_verify::vc::VerifiableCredential;

    #[derive(Deserialize)]
    struct AttestationVector {
        trust: AttestationTrust,
        credential: VerifiableCredential,
    }
    let judge = |file: &str| {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/vectors/attestation")
            .join(file);
        let v: AttestationVector =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let outcome = verify_evidence(&v.credential, &v.trust);
        let policy = IssuancePolicy {
            required_assurance_level: Some("device_attested".into()),
            ..Default::default()
        };
        evaluate(&policy, v.credential.integrity.as_ref().unwrap(), &outcome)
    };
    assert_eq!(judge("02-ios-credential-bound.json"), Ok(()));
    assert_eq!(
        judge("04-ios-session-bound.json"),
        Err(PolicyRefusal::AssuranceBelowRequired {
            level: "local".into(),
            required: "device_attested".into(),
        })
    );
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
