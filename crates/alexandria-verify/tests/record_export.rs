//! The signed learning-record export: a statement file and a detached
//! signature over its exact bytes.
//!
//! `tests/vectors/record-export/` holds exact byte files and a manifest naming
//! the outcome each statement file and signature file pair must produce, so a
//! verifier written in another language can be checked against the same bytes.
//! The format is described in `src/record_export.rs`.
//!
//! # Regenerating
//!
//! ```sh
//! ALEXANDRIA_REGENERATE_VECTORS=1 cargo test -p alexandria-verify --test record_export
//! ```
//!
//! Ed25519 signing is deterministic and the keys are fixed seeds, so
//! regeneration gives the same bytes unless the format changed.

use std::fs;
use std::path::PathBuf;

use alexandria_verify::did::derive_did_key;
use alexandria_verify::record_export::{self, ExportError, ExportSignature, EXPORT_FORMAT};
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

/// The learner's key in every vector.
const LEARNER_SEED: u8 = 7;

/// A two-statement file as the app writes it: a JSON array, pretty-printed.
const STATEMENTS: &str = r#"[
  {
    "id": "5f0c2a3e-91d4-8b6f-a2c7-3e4d5b6a7c8d",
    "actor": {
      "objectType": "Agent",
      "account": {
        "homePage": "urn:uuid:044eac38-2871-47c7-a8d8-5c25a8a9479c",
        "name": "z6MkpTHR8VNsBxYAAWHut2Geadd9jSwuBV8xRoAnwWsdvktH"
      }
    },
    "verb": {
      "id": "http://adlnet.gov/expapi/verbs/launched",
      "display": { "en-US": "launched" }
    },
    "object": {
      "objectType": "Activity",
      "id": "did:key:z6MkpTHR8VNsBxYAAWHut2Geadd9jSwuBV8xRoAnwWsdvktH/courses/11"
    },
    "timestamp": "2026-11-20T10:00:00.000Z"
  },
  {
    "id": "7a1b2c3d-4e5f-8a6b-9c7d-8e9f0a1b2c3d",
    "actor": {
      "objectType": "Agent",
      "account": {
        "homePage": "urn:uuid:044eac38-2871-47c7-a8d8-5c25a8a9479c",
        "name": "z6MkpTHR8VNsBxYAAWHut2Geadd9jSwuBV8xRoAnwWsdvktH"
      }
    },
    "verb": {
      "id": "http://adlnet.gov/expapi/verbs/completed",
      "display": { "en-US": "completed" }
    },
    "object": {
      "objectType": "Activity",
      "id": "did:key:z6MkpTHR8VNsBxYAAWHut2Geadd9jSwuBV8xRoAnwWsdvktH/courses/11"
    },
    "result": { "completion": true },
    "timestamp": "2026-11-20T10:05:00.000Z"
  }
]
"#;

fn statements() -> Vec<u8> {
    STATEMENTS.as_bytes().to_vec()
}

/// The same file with one statement's verb changed.
fn tampered() -> Vec<u8> {
    STATEMENTS
        .replacen("verbs/completed", "verbs/passed", 1)
        .into_bytes()
}

#[test]
fn a_signed_file_verifies_and_names_its_signer() {
    let signature = record_export::sign(&statements(), &key(LEARNER_SEED));
    let signature_file = record_export::signature_file(&signature);

    let signer = record_export::verify(&statements(), &signature_file).expect("verifies");

    assert_eq!(signer, derive_did_key(&key(LEARNER_SEED)));
}

#[test]
fn a_changed_statement_file_fails() {
    let signature_file =
        record_export::signature_file(&record_export::sign(&statements(), &key(LEARNER_SEED)));

    let error = record_export::verify(&tampered(), &signature_file).unwrap_err();

    assert_eq!(error, ExportError::WrongFile);
}

#[test]
fn one_added_byte_fails() {
    let signature_file =
        record_export::signature_file(&record_export::sign(&statements(), &key(LEARNER_SEED)));
    let mut file = statements();
    file.push(b'\n');

    assert_eq!(
        record_export::verify(&file, &signature_file),
        Err(ExportError::WrongFile)
    );
}

#[test]
fn a_digest_updated_to_match_a_changed_file_fails_the_signature() {
    let mut signature = record_export::sign(&statements(), &key(LEARNER_SEED));
    signature.sha256 = record_export::sign(&tampered(), &key(LEARNER_SEED)).sha256;

    let error =
        record_export::verify(&tampered(), &record_export::signature_file(&signature)).unwrap_err();

    assert_eq!(error, ExportError::BadSignature);
}

#[test]
fn a_signature_claimed_for_another_learner_fails() {
    let mut signature = record_export::sign(&statements(), &key(LEARNER_SEED));
    signature.signer = derive_did_key(&key(8)).as_str().to_string();

    let error = record_export::verify(&statements(), &record_export::signature_file(&signature))
        .unwrap_err();

    assert_eq!(error, ExportError::BadSignature);
}

#[test]
fn an_oversized_signature_file_is_refused_before_parsing() {
    let mut signature_file =
        record_export::signature_file(&record_export::sign(&statements(), &key(LEARNER_SEED)));
    signature_file.resize(record_export::MAX_SIGNATURE_FILE_BYTES + 1, b' ');

    let error = record_export::verify(&statements(), &signature_file).unwrap_err();

    assert_eq!(error.code(), "malformed");
}

// ---- Vectors -------------------------------------------------------------

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    statements: String,
    signature: String,
    description: String,
    /// `accept` or an [`ExportError::code`].
    expect: String,
    /// The signer `verify` returns, for `accept` cases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    signer: Option<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: String,
    cases: Vec<Case>,
}

fn vectors_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/record-export")
}

/// The statement files, by name.
fn statement_files() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("statements.json", statements()),
        ("statements-tampered.json", tampered()),
    ]
}

fn edited(edit: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>)) -> Vec<u8> {
    let signature = record_export::sign(&statements(), &key(LEARNER_SEED));
    let mut value = serde_json::to_value(signature).unwrap();
    edit(value.as_object_mut().unwrap());
    let mut bytes = serde_json::to_vec_pretty(&value).unwrap();
    bytes.push(b'\n');
    bytes
}

/// The signature files, by name.
fn signature_files() -> Vec<(&'static str, Vec<u8>)> {
    let valid = record_export::sign(&statements(), &key(LEARNER_SEED));
    let forged_digest = ExportSignature {
        sha256: record_export::sign(&tampered(), &key(LEARNER_SEED)).sha256,
        ..valid.clone()
    };
    let other_signer = ExportSignature {
        signer: derive_did_key(&key(8)).as_str().to_string(),
        ..valid.clone()
    };
    vec![
        ("statements.json.sig", record_export::signature_file(&valid)),
        (
            "forged-digest.sig",
            record_export::signature_file(&forged_digest),
        ),
        (
            "other-signer.sig",
            record_export::signature_file(&other_signer),
        ),
        (
            "unknown-format.sig",
            edited(|o| {
                o.insert("format".into(), "alexandria-record-export/2".into());
            }),
        ),
        (
            "unknown-field.sig",
            edited(|o| {
                o.insert("created".into(), "2026-11-20T10:05:00Z".into());
            }),
        ),
        (
            "not-a-did-key.sig",
            edited(|o| {
                o.insert("signer".into(), "did:web:example.org".into());
            }),
        ),
    ]
}

fn manifest() -> Manifest {
    let learner = derive_did_key(&key(LEARNER_SEED)).as_str().to_string();
    let case = |statements: &str, signature: &str, description: &str, expect: &str| Case {
        statements: statements.into(),
        signature: signature.into(),
        description: description.into(),
        expect: expect.into(),
        signer: (expect == "accept").then(|| learner.clone()),
    };
    Manifest {
        format: EXPORT_FORMAT.into(),
        cases: vec![
            case(
                "statements.json",
                "statements.json.sig",
                "The file and the signature the learner made over it.",
                "accept",
            ),
            case(
                "statements-tampered.json",
                "statements.json.sig",
                "One verb changed after signing: the digest no longer matches.",
                "wrong_file",
            ),
            case(
                "statements-tampered.json",
                "forged-digest.sig",
                "The digest edited to match the changed file: the signature no longer verifies.",
                "bad_signature",
            ),
            case(
                "statements.json",
                "other-signer.sig",
                "The signer changed to another learner's DID.",
                "bad_signature",
            ),
            case(
                "statements.json",
                "unknown-format.sig",
                "A format tag this version does not know.",
                "unknown_format",
            ),
            case(
                "statements.json",
                "unknown-field.sig",
                "An extra member in the signature file.",
                "malformed",
            ),
            case(
                "statements.json",
                "not-a-did-key.sig",
                "A signer that is not a did:key.",
                "invalid_signer",
            ),
        ],
    }
}

fn outcome(statements: &[u8], signature: &[u8]) -> (String, Option<String>) {
    match record_export::verify(statements, signature) {
        Ok(signer) => ("accept".into(), Some(signer.as_str().to_string())),
        Err(error) => (error.code().into(), None),
    }
}

fn regenerate() {
    let dir = vectors_dir();
    fs::create_dir_all(&dir).unwrap();
    for (name, bytes) in statement_files().into_iter().chain(signature_files()) {
        fs::write(dir.join(name), bytes).unwrap();
    }
    let mut manifest = serde_json::to_vec_pretty(&manifest()).unwrap();
    manifest.push(b'\n');
    fs::write(dir.join("manifest.json"), manifest).unwrap();
}

#[test]
fn record_export_vectors_match_this_implementation() {
    if std::env::var("ALEXANDRIA_REGENERATE_VECTORS").is_ok() {
        regenerate();
        return;
    }
    let dir = vectors_dir();
    let raw = fs::read_to_string(dir.join("manifest.json")).unwrap_or_else(|error| {
        panic!("read manifest: {error}. Regenerate with ALEXANDRIA_REGENERATE_VECTORS=1")
    });
    let recorded: Manifest = serde_json::from_str(&raw).unwrap();
    assert_eq!(recorded, manifest(), "manifest is stale; regenerate");
    for (name, bytes) in statement_files().into_iter().chain(signature_files()) {
        assert!(
            fs::read(dir.join(name)).unwrap() == bytes,
            "{name}: bytes are stale; regenerate"
        );
    }
    for case in recorded.cases {
        let statements = fs::read(dir.join(&case.statements)).unwrap();
        let signature = fs::read(dir.join(&case.signature)).unwrap();
        assert_eq!(
            outcome(&statements, &signature),
            (case.expect.clone(), case.signer.clone()),
            "{} with {}",
            case.statements,
            case.signature
        );
    }
}
