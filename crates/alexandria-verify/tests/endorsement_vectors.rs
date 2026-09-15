//! Cross-language vectors for course completion endorsements.
//!
//! `tests/vectors/endorsements/` holds exact endorsement byte files, and
//! `manifest.json` supplies the signing domain, structural limits, signed
//! course policy, expected completion binding, the outcome for each file, and
//! threshold evaluations over sets of files. The same bytes are checked by
//! this crate, by `independent-verifier.mjs`, and by the app's endorsement
//! import.
//!
//! # Regenerating
//!
//! ```sh
//! ALEXANDRIA_REGENERATE_VECTORS=1 cargo test -p alexandria-verify --test endorsement_vectors
//! ```
//!
//! The keys derive from fixed byte patterns, are published on purpose, and
//! secure nothing.

use std::fs;
use std::path::PathBuf;

use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};

use alexandria_verify::course::{
    decode_completion_endorsement, evaluate_completion_endorsements, sign_completion_endorsement,
    verify_completion_endorsement, AuthorizedAttestor, CompletionEvidence, CourseCompletionBinding,
    CourseCompletionPolicy, CourseTrustError, EvidenceRequirement,
    COMPLETION_ENDORSEMENT_FORMAT_VERSION, COMPLETION_ENDORSEMENT_JSON_LIMITS,
    COMPLETION_POLICY_FORMAT_VERSION,
};
use alexandria_verify::did::did_from_verifying_key;
use alexandria_verify::json::UntrustedJsonError;

const DOMAIN: &[u8] = b"alexandria/course-completion-endorsement/v1";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Limits {
    max_bytes: usize,
    max_depth: usize,
    max_array_len: usize,
    max_object_entries: usize,
    max_string_bytes: usize,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    file: String,
    description: String,
    expect: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Threshold {
    description: String,
    files: Vec<String>,
    valid_attestors: usize,
    rejected_endorsements: usize,
    satisfied: bool,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    domain: String,
    limits: Limits,
    policy: CourseCompletionPolicy,
    expected_binding: CourseCompletionBinding,
    cases: Vec<Case>,
    thresholds: Vec<Threshold>,
}

fn endorsements_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/endorsements")
}

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn policy() -> CourseCompletionPolicy {
    let mut authorized_attestors = [key(1), key(2)]
        .iter()
        .map(|attestor| AuthorizedAttestor {
            did: did_from_verifying_key(&attestor.verifying_key()),
            public_key_hex: hex::encode(attestor.verifying_key().as_bytes()),
        })
        .collect::<Vec<_>>();
    authorized_attestors.sort_by(|left, right| left.did.as_str().cmp(right.did.as_str()));
    CourseCompletionPolicy {
        format_version: COMPLETION_POLICY_FORMAT_VERSION,
        required_attestors: 2,
        authorized_attestors,
        evidence_requirements: vec![EvidenceRequirement {
            kind: "completion-root".into(),
            format_version: 1,
        }],
    }
}

fn binding() -> CourseCompletionBinding {
    CourseCompletionBinding {
        format_version: COMPLETION_ENDORSEMENT_FORMAT_VERSION,
        network_id: "preprod".into(),
        subject_did: did_from_verifying_key(&key(9).verifying_key()),
        course_id: "course".into(),
        course_document_cid: "11".repeat(32),
        course_document_version: 2,
        completion_root: "22".repeat(32),
        evidence: vec![CompletionEvidence {
            kind: "completion-root".into(),
            format_version: 1,
            id: "course-completion".into(),
            digest: "22".repeat(32),
        }],
        witness_tx_hash: None,
    }
}

fn signature_hex(
    signer: &SigningKey,
    domain: &[u8],
    signed_binding: &CourseCompletionBinding,
) -> String {
    let mut message = domain.to_vec();
    message.push(0);
    message.extend(serde_json_canonicalizer::to_vec(signed_binding).unwrap());
    hex::encode(signer.sign(&message).to_bytes())
}

fn outcome(
    policy: &CourseCompletionPolicy,
    binding: &CourseCompletionBinding,
    bytes: &[u8],
) -> &'static str {
    let endorsement = match decode_completion_endorsement(bytes) {
        Ok(endorsement) => endorsement,
        Err(UntrustedJsonError::TooLarge { .. }) => return "too_large",
        Err(UntrustedJsonError::TooDeep { .. }) => return "too_deep",
        Err(UntrustedJsonError::TooManyElements { .. }) => return "too_many_elements",
        Err(UntrustedJsonError::TooManyEntries { .. }) => return "too_many_entries",
        Err(UntrustedJsonError::StringTooLong { .. }) => return "string_too_long",
        Err(UntrustedJsonError::DuplicateKey(_)) => return "duplicate_key",
        Err(UntrustedJsonError::UnsafeNumber(_)) => return "unsafe_number",
        Err(UntrustedJsonError::Invalid(_)) => return "invalid",
        Err(UntrustedJsonError::InvalidLimits(reason)) => panic!("invalid limits: {reason}"),
    };
    match verify_completion_endorsement(policy, binding, &endorsement) {
        Ok(()) => "valid",
        Err(CourseTrustError::InvalidBinding(_)) => "binding_mismatch",
        Err(CourseTrustError::UnauthorizedAttestor) => "unauthorized_attestor",
        Err(CourseTrustError::AttestorIdentityMismatch) => "identity_mismatch",
        Err(CourseTrustError::InvalidPublicKey) => "invalid_public_key",
        Err(CourseTrustError::InvalidSignature) => "invalid_signature",
        Err(other) => panic!("unexpected endorsement error: {other}"),
    }
}

type BuiltCase = (&'static str, &'static str, Vec<u8>, &'static str);

fn build_cases() -> Vec<BuiltCase> {
    let policy = policy();
    let binding = binding();
    let signed = |signer: &SigningKey| {
        serde_json::to_value(sign_completion_endorsement(&policy, binding.clone(), signer).unwrap())
            .unwrap()
    };
    let first = signed(&key(1));
    let second = signed(&key(2));
    let tampered = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut value = first.clone();
        edit(&mut value);
        serde_json::to_vec(&value).unwrap()
    };
    let first_bytes = serde_json::to_vec(&first).unwrap();
    let first_text = String::from_utf8(first_bytes.clone()).unwrap();
    let with_member =
        |member: &str| format!("{{{member},{}", first_text.strip_prefix('{').unwrap()).into_bytes();
    let padded_to = |len: usize| {
        let mut bytes = first_bytes.clone();
        assert!(bytes.len() <= len);
        bytes.resize(len, b' ');
        bytes
    };
    let mut other_network = binding.clone();
    other_network.network_id = "other".into();
    let outsider = key(3);
    let unlisted = serde_json::json!({
        "binding": binding,
        "attestor_did": did_from_verifying_key(&outsider.verifying_key()),
        "attestor_public_key_hex": hex::encode(outsider.verifying_key().as_bytes()),
        "signature_hex": signature_hex(&outsider, DOMAIN, &binding),
    });
    let second_key_hex = hex::encode(key(2).verifying_key().as_bytes());
    let wrong_domain = signature_hex(
        &key(1),
        b"alexandria/course-completion-endorsement/v0",
        &binding,
    );
    let other_binding_signature = signature_hex(&key(1), DOMAIN, &other_network);
    let nested = format!("\"extra\":{}{}", "[".repeat(8), "]".repeat(8));
    let unsafe_number = first_text.replacen(
        "\"format_version\":1",
        "\"format_version\":9007199254740992",
        1,
    );

    vec![
        (
            "01-valid-first-attestor.json",
            "An authorized attestor's endorsement of the exact expected binding.",
            first_bytes.clone(),
            "valid",
        ),
        (
            "02-valid-second-attestor.json",
            "The second authorized attestor's endorsement of the same binding.",
            serde_json::to_vec(&second).unwrap(),
            "valid",
        ),
        (
            "03-tampered-network-id.json",
            "The embedded binding names another network.",
            tampered(&|value| value["binding"]["network_id"] = "other".into()),
            "binding_mismatch",
        ),
        (
            "04-tampered-course-document-cid.json",
            "The embedded binding names another course document.",
            tampered(&|value| value["binding"]["course_document_cid"] = "44".repeat(32).into()),
            "binding_mismatch",
        ),
        (
            "05-tampered-completion-root.json",
            "The embedded binding names another completion root.",
            tampered(&|value| value["binding"]["completion_root"] = "55".repeat(32).into()),
            "binding_mismatch",
        ),
        (
            "06-tampered-subject.json",
            "The embedded binding names another learner.",
            tampered(&|value| {
                value["binding"]["subject_did"] = did_from_verifying_key(&key(10).verifying_key())
                    .as_str()
                    .into()
            }),
            "binding_mismatch",
        ),
        (
            "07-tampered-evidence-digest.json",
            "The embedded binding carries another evidence digest.",
            tampered(&|value| value["binding"]["evidence"][0]["digest"] = "66".repeat(32).into()),
            "binding_mismatch",
        ),
        (
            "08-signature-bit-flip.json",
            "The last hex digit of the signature is changed.",
            tampered(&|value| {
                let mut signature = value["signature_hex"].as_str().unwrap().to_string();
                let last = signature.pop().unwrap();
                signature.push(if last == '0' { '1' } else { '0' });
                value["signature_hex"] = signature.into();
            }),
            "invalid_signature",
        ),
        (
            "09-uppercase-signature-hex.json",
            "The signature is spelled in uppercase hex, which is not canonical.",
            tampered(&|value| {
                value["signature_hex"] = value["signature_hex"]
                    .as_str()
                    .unwrap()
                    .to_ascii_uppercase()
                    .into();
            }),
            "invalid_signature",
        ),
        (
            "10-wrong-signing-domain.json",
            "Signed over the binding under a different domain string.",
            tampered(&|value| value["signature_hex"] = wrong_domain.clone().into()),
            "invalid_signature",
        ),
        (
            "11-signature-over-other-binding.json",
            "Signed over a binding for another network, then presented with the expected binding.",
            tampered(&|value| value["signature_hex"] = other_binding_signature.clone().into()),
            "invalid_signature",
        ),
        (
            "12-unlisted-attestor.json",
            "A valid signature by a key the course policy does not authorize.",
            serde_json::to_vec(&unlisted).unwrap(),
            "unauthorized_attestor",
        ),
        (
            "13-swapped-public-key.json",
            "An authorized DID presented with the other attestor's public key.",
            tampered(&|value| value["attestor_public_key_hex"] = second_key_hex.clone().into()),
            "unauthorized_attestor",
        ),
        (
            "14-uppercase-public-key-hex.json",
            "The authorized public key spelled in uppercase hex, which does not match the policy.",
            tampered(&|value| {
                value["attestor_public_key_hex"] = value["attestor_public_key_hex"]
                    .as_str()
                    .unwrap()
                    .to_ascii_uppercase()
                    .into();
            }),
            "unauthorized_attestor",
        ),
        (
            "15-duplicate-key.json",
            "The signature key appears twice in the endorsement object.",
            with_member("\"signature_hex\":\"00\""),
            "duplicate_key",
        ),
        (
            "16-bytes-at-limit.json",
            "The valid endorsement padded with whitespace to exactly the byte limit.",
            padded_to(COMPLETION_ENDORSEMENT_JSON_LIMITS.max_bytes),
            "valid",
        ),
        (
            "17-bytes-over-limit.json",
            "One byte over the byte limit, refused before parsing.",
            padded_to(COMPLETION_ENDORSEMENT_JSON_LIMITS.max_bytes + 1),
            "too_large",
        ),
        (
            "18-nesting-over-limit.json",
            "A member nested one level beyond the depth limit.",
            with_member(&nested),
            "too_deep",
        ),
        (
            "19-unsafe-number.json",
            "A format version outside the exact JavaScript integer range.",
            unsafe_number.into_bytes(),
            "unsafe_number",
        ),
    ]
}

fn thresholds() -> Vec<Threshold> {
    let threshold =
        |description: &str, files: &[&str], valid: usize, rejected: usize, satisfied: bool| {
            Threshold {
                description: description.into(),
                files: files.iter().map(|file| file.to_string()).collect(),
                valid_attestors: valid,
                rejected_endorsements: rejected,
                satisfied,
            }
        };
    vec![
        threshold(
            "Two distinct authorized attestors meet the two-attestor policy.",
            &[
                "01-valid-first-attestor.json",
                "02-valid-second-attestor.json",
            ],
            2,
            0,
            true,
        ),
        threshold(
            "The same attestor twice counts once.",
            &[
                "01-valid-first-attestor.json",
                "01-valid-first-attestor.json",
            ],
            1,
            1,
            false,
        ),
        threshold(
            "Unauthorized and invalid endorsements never count toward the threshold.",
            &[
                "01-valid-first-attestor.json",
                "12-unlisted-attestor.json",
                "08-signature-bit-flip.json",
            ],
            1,
            2,
            false,
        ),
    ]
}

fn manifest() -> Manifest {
    let limits = COMPLETION_ENDORSEMENT_JSON_LIMITS;
    Manifest {
        domain: String::from_utf8(DOMAIN.to_vec()).unwrap(),
        limits: Limits {
            max_bytes: limits.max_bytes,
            max_depth: limits.max_depth,
            max_array_len: limits.max_array_len,
            max_object_entries: limits.max_object_entries,
            max_string_bytes: limits.max_string_bytes,
        },
        policy: policy(),
        expected_binding: binding(),
        cases: build_cases()
            .into_iter()
            .map(|(file, description, _, expect)| Case {
                file: file.into(),
                description: description.into(),
                expect: expect.into(),
            })
            .collect(),
        thresholds: thresholds(),
    }
}

#[test]
fn endorsement_vectors_match_this_implementation() {
    if std::env::var("ALEXANDRIA_REGENERATE_VECTORS").is_ok() {
        regenerate();
        return;
    }
    let dir = endorsements_dir();
    let raw = fs::read_to_string(dir.join("manifest.json")).unwrap_or_else(|error| {
        panic!(
            "read endorsement manifest: {error}. Regenerate with ALEXANDRIA_REGENERATE_VECTORS=1"
        )
    });
    let recorded: Manifest = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        recorded,
        manifest(),
        "endorsement manifest is stale; regenerate"
    );

    let policy = policy();
    let binding = binding();
    for (file, _, bytes, expect) in build_cases() {
        let on_disk = fs::read(dir.join(file)).unwrap();
        assert!(on_disk == bytes, "{file}: bytes are stale; regenerate");
        assert_eq!(outcome(&policy, &binding, &on_disk), expect, "{file}");
    }

    for threshold in thresholds() {
        let endorsements = threshold
            .files
            .iter()
            .map(|file| decode_completion_endorsement(&fs::read(dir.join(file)).unwrap()).unwrap())
            .collect::<Vec<_>>();
        let result = evaluate_completion_endorsements(&policy, &binding, &endorsements).unwrap();
        assert_eq!(
            result.valid_attestors.len(),
            threshold.valid_attestors,
            "{}",
            threshold.description
        );
        assert_eq!(
            result.rejected_endorsements, threshold.rejected_endorsements,
            "{}",
            threshold.description
        );
        assert_eq!(
            result.satisfied, threshold.satisfied,
            "{}",
            threshold.description
        );
    }
}

#[test]
fn the_endorsement_suite_covers_each_reachable_rejection() {
    let cases = build_cases();
    for kind in [
        "valid",
        "binding_mismatch",
        "invalid_signature",
        "unauthorized_attestor",
        "duplicate_key",
        "too_large",
        "too_deep",
        "unsafe_number",
    ] {
        assert!(
            cases.iter().any(|(_, _, _, expect)| *expect == kind),
            "no endorsement vector produces {kind}"
        );
    }
}

fn regenerate() {
    let dir = endorsements_dir();
    fs::create_dir_all(&dir).unwrap();
    for entry in fs::read_dir(&dir).unwrap().flatten() {
        fs::remove_file(entry.path()).unwrap();
    }
    for (file, _, bytes, _) in build_cases() {
        fs::write(dir.join(file), bytes).unwrap();
    }
    let manifest = serde_json::to_string_pretty(&manifest()).unwrap();
    fs::write(dir.join("manifest.json"), manifest + "\n").unwrap();
    println!(
        "wrote {} endorsement vectors to {}",
        build_cases().len(),
        dir.display()
    );
}
