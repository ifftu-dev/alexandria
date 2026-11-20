//! Shared vectors for access grants, including record shares.
//!
//! `tests/vectors/access/` holds one JSON file per case. Each file is
//! self-describing:
//!
//! ```json
//! { "description": "...", "now": 1791000000,
//!   "request": { AccessRequest }   // optional: check the grant answers it
//!   "current": { GrantStatement }  // optional: check the statement follows it
//!   "signed":  { SignedGrantStatement },
//!   "expect":  "accept" | "malformed" | "bad_signature" | "stale" | "mismatch" | "expired" }
//! ```
//!
//! The outcome is the first refusal of, in order: [`access::verify`] at
//! `now`; [`access::validate_request`] and [`access::check_answers`] when a
//! request is given; [`access::check_follows`] when a current statement is
//! given. The hosted service runs the same files from its vendored copy, so
//! the two sides of the grant path agree on what a record share may say.
//!
//! # Regenerating
//!
//! ```sh
//! ALEXANDRIA_REGENERATE_VECTORS=1 cargo test -p alexandria-verify --test access_vectors
//! ```
//!
//! Ed25519 is deterministic and every input is fixed, so regeneration gives
//! the same bytes; a changed file means the format moved.

use std::fs;
use std::path::PathBuf;

use alexandria_verify::access::{
    self, AccessError, AccessRequest, GrantAction, GrantStatement, RecordAccount, RecordKind,
    RecordScope, SignedGrantStatement, GRANT_FORMAT,
};
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};

const NOW: i64 = 1_791_000_000;
const COURSE_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const COURSE_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const COURSE_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Vector {
    description: String,
    now: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    request: Option<AccessRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current: Option<GrantStatement>,
    signed: SignedGrantStatement,
    expect: String,
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors/access")
}

fn key() -> SigningKey {
    SigningKey::from_bytes(&[7; 32])
}

fn did() -> String {
    alexandria_verify::did::derive_did_key(&key())
        .as_str()
        .to_string()
}

fn hr_account() -> RecordAccount {
    RecordAccount {
        home_page: "https://hr.example.org".into(),
        name: "E-1042".into(),
    }
}

/// What the learner's app derives for this organisation: the fixed account
/// home page Alexandria's statements use and a per-recipient name.
fn derived_account() -> RecordAccount {
    RecordAccount {
        home_page: "urn:uuid:044eac38-2871-47c7-a8d8-5c25a8a9479c".into(),
        name: "z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK".into(),
    }
}

fn scope(courses: &[&str], kinds: &[RecordKind], account: Option<RecordAccount>) -> RecordScope {
    RecordScope {
        course_ids: courses.iter().map(|c| c.to_string()).collect(),
        kinds: kinds.to_vec(),
        account,
    }
}

const BOTH: &[RecordKind] = &[RecordKind::Completion, RecordKind::Interaction];
const COMPLETION: &[RecordKind] = &[RecordKind::Completion];

fn skills_request() -> AccessRequest {
    AccessRequest {
        id: "6f1d0c52-3a8e-4f7b-9d2a-0c1e5b7a9f10".into(),
        audience: alexandria_verify::org::audience("hr.example.org"),
        nonce: "2b7e1516-28ae-4d2a-abf7-158809cf4f3c".into(),
        organization: "Example Org".into(),
        subject_did: did(),
        skill_ids: vec!["skill_big_o".into()],
        purpose: "Warehouse team lead".into(),
        network_id: "preprod".into(),
        created_at: NOW - 60,
        expires_at: NOW + 14 * 86_400,
        records: None,
    }
}

/// Completion and per-answer records from two courses, the organisation's
/// HR identifier proposed, no skills.
fn records_request() -> AccessRequest {
    AccessRequest {
        skill_ids: Vec::new(),
        purpose: "Mandatory safety training reporting".into(),
        records: Some(scope(&[COURSE_A, COURSE_B], BOTH, Some(hr_account()))),
        ..skills_request()
    }
}

fn grant_for(request: &AccessRequest, records: Option<RecordScope>) -> GrantStatement {
    GrantStatement {
        format: GRANT_FORMAT.into(),
        action: GrantAction::Grant,
        grant_id: "0d5b8f3e-1c2a-4b6d-8e9f-7a1b2c3d4e5f".into(),
        request_id: request.id.clone(),
        audience: request.audience.clone(),
        nonce: request.nonce.clone(),
        subject_did: did(),
        skill_ids: request.skill_ids.clone(),
        sync: false,
        created: NOW,
        records,
    }
}

fn update(current: &GrantStatement, records: Option<RecordScope>) -> GrantStatement {
    GrantStatement {
        action: GrantAction::Update,
        created: NOW + 60,
        records,
        ..current.clone()
    }
}

fn signed(statement: &GrantStatement) -> SignedGrantStatement {
    access::sign(statement, &key()).unwrap()
}

fn case(
    description: &str,
    request: Option<AccessRequest>,
    current: Option<GrantStatement>,
    signed: SignedGrantStatement,
    expect: &str,
) -> Vector {
    let now = signed.statement.created;
    Vector {
        description: description.into(),
        now,
        request,
        current,
        signed,
        expect: expect.into(),
    }
}

fn build_all() -> Vec<(&'static str, Vector)> {
    let req = records_request();
    let full = grant_for(
        &req,
        Some(scope(&[COURSE_A, COURSE_B], BOTH, Some(derived_account()))),
    );
    let narrowed = grant_for(
        &req,
        Some(scope(&[COURSE_A], COMPLETION, Some(derived_account()))),
    );
    let mut tampered = signed(&narrowed);
    tampered.statement.records = Some(scope(
        &[COURSE_A, COURSE_B],
        COMPLETION,
        Some(derived_account()),
    ));
    let mut stale = signed(&narrowed);
    let stale_now = NOW + 3_600;
    vec![
        (
            "01-skills-grant-without-records.json",
            case(
                "A skills grant made before record shares existed: no records field, the bytes it always signed.",
                Some(skills_request()),
                None,
                signed(&grant_for(&skills_request(), None)),
                "accept",
            ),
        ),
        (
            "02-record-share-derived-account.json",
            case(
                "A record-only share of everything asked for, under the account the app derived for this organisation.",
                Some(req.clone()),
                None,
                signed(&full),
                "accept",
            ),
        ),
        (
            "03-record-share-narrowed.json",
            case(
                "The learner shares one of the two courses, completion records only.",
                Some(req.clone()),
                None,
                signed(&narrowed),
                "accept",
            ),
        ),
        (
            "04-record-share-proposed-account.json",
            case(
                "The learner approves the account the organisation proposed.",
                Some(req.clone()),
                None,
                signed(&grant_for(&req, Some(scope(&[COURSE_A, COURSE_B], COMPLETION, Some(hr_account()))))),
                "accept",
            ),
        ),
        (
            "05-record-share-wider-courses.json",
            case(
                "A share naming a course that was not asked for.",
                Some(req.clone()),
                None,
                signed(&grant_for(&req, Some(scope(&[COURSE_A, COURSE_C], COMPLETION, Some(derived_account()))))),
                "mismatch",
            ),
        ),
        (
            "06-record-share-unrequested-kind.json",
            case(
                "Per-answer records shared when only completion records were asked for.",
                Some(AccessRequest {
                    records: Some(scope(&[COURSE_A], COMPLETION, None)),
                    ..req.clone()
                }),
                None,
                signed(&grant_for(&req, Some(scope(&[COURSE_A], BOTH, Some(derived_account()))))),
                "mismatch",
            ),
        ),
        (
            "07-records-not-asked-for.json",
            case(
                "Records shared in answer to a request for skills alone.",
                Some(skills_request()),
                None,
                signed(&grant_for(&skills_request(), Some(scope(&[COURSE_A], COMPLETION, Some(derived_account()))))),
                "mismatch",
            ),
        ),
        (
            "08-grants-nothing.json",
            case(
                "A grant with no skills and no records.",
                None,
                None,
                signed(&grant_for(&req, None)),
                "malformed",
            ),
        ),
        (
            "09-record-scope-without-completion.json",
            case(
                "A scope of per-answer records alone: completion records are always part of a share.",
                None,
                None,
                signed(&grant_for(&req, Some(scope(&[COURSE_A], &[RecordKind::Interaction], Some(derived_account()))))),
                "malformed",
            ),
        ),
        (
            "10-record-scope-duplicate-course.json",
            case(
                "A scope naming one course twice.",
                None,
                None,
                signed(&grant_for(&req, Some(scope(&[COURSE_A, COURSE_A], COMPLETION, Some(derived_account()))))),
                "malformed",
            ),
        ),
        (
            "11-grant-without-account.json",
            case(
                "A grant that does not say which account its records carry.",
                None,
                None,
                signed(&grant_for(&req, Some(scope(&[COURSE_A], COMPLETION, None)))),
                "malformed",
            ),
        ),
        (
            "12-record-scope-tampered.json",
            case(
                "A course added to the scope after signing.",
                None,
                None,
                tampered,
                "bad_signature",
            ),
        ),
        (
            "13-update-narrows-records.json",
            case(
                "An update that drops a course and the per-answer records.",
                None,
                Some(full.clone()),
                signed(&update(&full, Some(scope(&[COURSE_B], COMPLETION, Some(derived_account()))))),
                "accept",
            ),
        ),
        (
            "14-update-widens-records.json",
            case(
                "An update that adds per-answer records back.",
                None,
                Some(narrowed.clone()),
                signed(&update(&narrowed, Some(scope(&[COURSE_A], BOTH, Some(derived_account()))))),
                "mismatch",
            ),
        ),
        (
            "15-update-changes-account.json",
            case(
                "An update that moves the records to another account.",
                None,
                Some(full.clone()),
                signed(&update(&full, Some(scope(&[COURSE_A], COMPLETION, Some(hr_account()))))),
                "mismatch",
            ),
        ),
        (
            "16-revoke-record-share.json",
            case(
                "A revoke of a record share: no skills, no scope.",
                None,
                Some(narrowed.clone()),
                signed(&GrantStatement {
                    action: GrantAction::Revoke,
                    created: NOW + 120,
                    records: None,
                    ..narrowed.clone()
                }),
                "accept",
            ),
        ),
        ("17-record-share-stale.json", {
            stale.statement.created = NOW;
            Vector {
                now: stale_now,
                ..case(
                    "A record share signed an hour before it is checked.",
                    None,
                    None,
                    stale,
                    "stale",
                )
            }
        }),
    ]
}

fn outcome(v: &Vector) -> &'static str {
    let code = |e: AccessError| match e {
        AccessError::Malformed(_) | AccessError::Snapshot(_) => "malformed",
        AccessError::BadSignature => "bad_signature",
        AccessError::Stale => "stale",
        AccessError::Mismatch(_) => "mismatch",
        AccessError::Expired => "expired",
    };
    let checked = access::verify(&v.signed, v.now)
        .and_then(|()| match &v.request {
            Some(request) => access::validate_request(request)
                .and_then(|()| access::check_answers(request, &v.signed.statement, v.now)),
            None => Ok(()),
        })
        .and_then(|()| match &v.current {
            Some(current) => access::check_follows(current, &v.signed.statement),
            None => Ok(()),
        });
    match checked {
        Ok(()) => "accept",
        Err(e) => code(e),
    }
}

fn encode(v: &Vector) -> String {
    let mut text = serde_json::to_string_pretty(v).unwrap();
    text.push('\n');
    text
}

#[test]
fn access_vectors_match_this_implementation() {
    let dir = dir();
    if std::env::var("ALEXANDRIA_REGENERATE_VECTORS").is_ok() {
        fs::create_dir_all(&dir).unwrap();
        for (file, vector) in build_all() {
            fs::write(dir.join(file), encode(&vector)).unwrap();
        }
        return;
    }
    let mut on_disk: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read access vectors: {e}; regenerate"))
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    on_disk.sort();
    let built = build_all();
    let names: Vec<&str> = built.iter().map(|(file, _)| *file).collect();
    assert_eq!(on_disk, names, "access vector files are stale; regenerate");
    for (file, vector) in built {
        let text = fs::read_to_string(dir.join(file)).unwrap();
        assert_eq!(text, encode(&vector), "{file}: stale; regenerate");
        let parsed: Vector = serde_json::from_str(&text).unwrap();
        assert_eq!(outcome(&parsed), parsed.expect, "{file}");
    }
}
