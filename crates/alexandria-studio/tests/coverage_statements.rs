//! Coverage statements: one per trust profile and verification rung,
//! content-addressed, and embedding the certification in force.

use std::path::Path;

use alexandria_studio::integrity::measurement::Certification;
use alexandria_studio::integrity::{
    coverage_statement, coverage_statements, resolve_coverage, TrustProfile, VerificationRung,
};

fn certification(synthetic: bool) -> (Certification, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/integrity-corpus/certification.synthetic.json");
    let (mut cert, digest) = Certification::load(&path).unwrap();
    cert.synthetic = synthetic;
    (cert, digest)
}

#[test]
fn every_profile_and_rung_has_its_own_statement_that_resolves_from_its_digest() {
    let statements = coverage_statements(None);
    assert_eq!(statements.len(), 4 * 5);
    let mut digests: Vec<String> = statements.iter().map(|s| s.digest()).collect();
    digests.sort();
    digests.dedup();
    assert_eq!(digests.len(), 20, "digests are distinct");

    for statement in &statements {
        let resolved = resolve_coverage(&statement.digest(), None).expect("resolves");
        assert_eq!(&resolved, statement);
        assert!(statement.digest().starts_with("blake3:"));
    }
    assert_eq!(resolve_coverage("blake3:00", None), None);
}

#[test]
fn every_statement_says_what_monitoring_cannot_see() {
    for statement in coverage_statements(None) {
        assert!(
            statement
                .not_checked
                .iter()
                .any(|l| l.contains("second person") && l.contains("second device")),
            "{:?} {:?}",
            statement.profile,
            statement.rung
        );
        assert!(statement.certification.is_none());
        assert!(statement.certified_signals.is_empty());
        assert!(statement.measurement.contains("advisory"));
    }
}

#[test]
fn a_profile_without_a_signal_says_it_did_not_run_and_a_rung_states_what_lies_above() {
    let core = coverage_statement(TrustProfile::Core, VerificationRung::Process, None);
    assert!(core
        .not_checked
        .iter()
        .any(|l| l.contains("camera was not used")));
    assert!(core
        .not_checked
        .iter()
        .any(|l| l.contains("reported by the device")));
    assert!(core.checked.iter().any(|l| l.contains("Response timing")));
    assert!(core
        .not_checked
        .iter()
        .any(|l| l.contains("Follow-up questions")));

    let full = coverage_statement(TrustProfile::Full, VerificationRung::Live, None);
    assert!(full.checked.iter().any(|l| l.contains("camera")));
    assert!(full
        .checked
        .iter()
        .any(|l| l.contains("platform attestation")));
    assert!(full.checked.iter().any(|l| l.contains("live segment")));
    assert!(!full
        .not_checked
        .iter()
        .any(|l| l.contains("camera was not used")));
}

#[test]
fn a_certification_in_force_is_embedded_with_the_profiles_rates() {
    let (cert, digest) = certification(false);
    let statement = coverage_statement(
        TrustProfile::Camera,
        VerificationRung::Process,
        Some((&cert, &digest)),
    );
    let embedded = statement.certification.as_ref().expect("embedded");
    assert_eq!(embedded.digest, digest);
    assert_eq!(embedded.version, "synthetic-fixture-1");
    assert_eq!(
        statement.certified_signals.keys().collect::<Vec<_>>(),
        vec!["device_glance", "multiple_faces"]
    );
    assert_eq!(statement.certified_signals["multiple_faces"].recall, 0.5);

    // A new certification is a new statement.
    let uncertified = coverage_statement(TrustProfile::Camera, VerificationRung::Process, None);
    assert_ne!(statement.digest(), uncertified.digest());
    assert_eq!(
        resolve_coverage(&statement.digest(), Some((&cert, &digest))),
        Some(statement)
    );
}

#[test]
fn a_synthetic_certification_is_never_embedded() {
    let (cert, digest) = certification(true);
    let statement = coverage_statement(
        TrustProfile::Core,
        VerificationRung::None,
        Some((&cert, &digest)),
    );
    assert!(statement.certification.is_none());
    assert!(statement.certified_signals.is_empty());
}
