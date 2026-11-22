//! Host-seam tests of the activity log view's operations: a learner's records
//! are written by the same host functions the commands call, then listed,
//! exported and deleted the way the activity log commands do it.

use alexandria_verify::record_export;
use ed25519_dalek::SigningKey;

use super::activity_log::{delete, delete_all, export, page};
use super::*;
use crate::crypto::hash::entity_id;
use crate::db::Database;
use crate::domain::enrollment::UpdateProgressRequest;
use crate::settings::{registry::keys, SettingsStore};

const LEARNER_SEED: u8 = 1;
const CREATOR_SEED: u8 = 9;

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn did(seed: u8) -> String {
    crate::crypto::did::derive_did_key(&key(seed))
        .as_str()
        .to_string()
}

/// A profile enrolled in a two-element course published by someone else.
struct Learner {
    db: Database,
    enrollment: String,
    video: String,
    quiz: String,
}

impl Learner {
    fn new() -> Self {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        let course_id = entity_id(&["course"]);
        let video = entity_id(&["video"]);
        let quiz = entity_id(&["quiz"]);
        db.conn()
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address, display_name) \
                 VALUES (1, 'stake_test1ulearner', 'addr_test1q', 'Ada')",
                [],
            )
            .unwrap();
        SettingsStore::set(db.conn(), keys::IDENTITY_LOCAL_DID, did(LEARNER_SEED)).unwrap();
        db.conn()
            .execute(
                "INSERT INTO courses (id, title, author_address, status, content_cid, \
                 course_document_version, creator_did) \
                 VALUES (?1, 'Warehouse safety', 'stake_test1uauthor', 'published', ?2, 2, ?3)",
                rusqlite::params![course_id, "11".repeat(32), did(CREATOR_SEED)],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO course_chapters (id, course_id, title, position) \
                 VALUES ('ch1', ?1, 'Chapter', 0)",
                [&course_id],
            )
            .unwrap();
        for (id, title, element_type, position) in
            [(&video, "Lifting", "video", 0), (&quiz, "Check", "quiz", 1)]
        {
            db.conn()
                .execute(
                    "INSERT INTO course_elements (id, chapter_id, title, element_type, position) \
                     VALUES (?1, 'ch1', ?2, ?3, ?4)",
                    rusqlite::params![id, title, element_type, position],
                )
                .unwrap();
        }
        let enrollment = crate::enrollment::enroll(&db, &course_id)
            .expect("enrol")
            .id;
        Self {
            db,
            enrollment,
            video,
            quiz,
        }
    }

    fn progress(&self, element: &str, status: &str, score: Option<f64>) {
        crate::enrollment::update_progress(
            &self.db,
            self.enrollment.clone(),
            UpdateProgressRequest {
                element_id: element.to_string(),
                status: status.to_string(),
                score,
                time_spent: None,
                integrity_session_id: None,
                integrity_score: None,
            },
        )
        .expect("update progress");
    }

    /// Registered, then the video opened and completed: four records.
    fn with_video_completed() -> Self {
        let learner = Self::new();
        learner.progress(&learner.video, "in_progress", None);
        learner.progress(&learner.video, "completed", None);
        learner
    }

    fn records(&self) -> Vec<LearningRecord> {
        list(self.db.conn()).expect("list")
    }

    fn ids(&self) -> Vec<String> {
        self.records().into_iter().map(|r| r.id).collect()
    }
}

#[test]
fn an_export_verifies_against_the_learners_key_over_the_exact_file() {
    let learner = Learner::with_video_completed();

    let exported = export(learner.db.conn(), &key(LEARNER_SEED)).expect("export");

    let signer = record_export::verify(&exported.statements, &exported.signature)
        .expect("the signature verifies over the statement file");
    assert_eq!(signer.as_str(), did(LEARNER_SEED));
}

#[test]
fn an_export_holds_every_record_as_a_statement_naming_the_learner_oldest_first() {
    let learner = Learner::with_video_completed();

    let exported = export(learner.db.conn(), &key(LEARNER_SEED)).expect("export");

    let statements: Vec<serde_json::Value> =
        serde_json::from_slice(&exported.statements).expect("a JSON array of statements");
    let verbs: Vec<&str> = statements
        .iter()
        .map(|s| s["verb"]["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        verbs,
        vec![
            "http://adlnet.gov/expapi/verbs/registered",
            "http://adlnet.gov/expapi/verbs/launched",
            "http://adlnet.gov/expapi/verbs/completed",
            "http://adlnet.gov/expapi/verbs/progressed",
        ]
    );
    let learner_key = did(LEARNER_SEED);
    for statement in &statements {
        assert_eq!(
            statement["actor"]["account"]["name"],
            learner_key.strip_prefix("did:key:").unwrap()
        );
        assert!(statement.get("signature").is_none());
        assert!(statement.get("attachments").is_none());
    }
    // Names are disclosed in the learner's own export.
    assert_eq!(
        statements[1]["object"]["definition"]["name"]["und"],
        "Lifting"
    );
}

#[test]
fn a_changed_export_no_longer_verifies() {
    let learner = Learner::with_video_completed();
    let exported = export(learner.db.conn(), &key(LEARNER_SEED)).expect("export");

    let text = String::from_utf8(exported.statements.clone()).unwrap();
    let tampered = text.replacen("verbs/completed", "verbs/passed", 1);
    assert_ne!(tampered, text);

    assert!(record_export::verify(tampered.as_bytes(), &exported.signature).is_err());
    // Nor does the right file with another learner's signature.
    let other = export(learner.db.conn(), &key(2)).expect("export");
    let mut forged: serde_json::Value = serde_json::from_slice(&other.signature).unwrap();
    forged["signer"] = did(LEARNER_SEED).into();
    forged["sha256"] =
        serde_json::from_slice::<serde_json::Value>(&exported.signature).unwrap()["sha256"].clone();
    assert!(
        record_export::verify(&exported.statements, &serde_json::to_vec(&forged).unwrap()).is_err()
    );
}

#[test]
fn an_empty_log_exports_an_empty_signed_file() {
    let db = Database::open_in_memory().expect("in-memory db");
    db.run_migrations().expect("migrations");

    let exported = export(db.conn(), &key(LEARNER_SEED)).expect("export");

    assert_eq!(exported.statements, b"[]\n");
    assert!(record_export::verify(&exported.statements, &exported.signature).is_ok());
}

#[test]
fn the_log_lists_records_newest_first_with_what_they_are_about() {
    let learner = Learner::with_video_completed();
    learner.progress(&learner.quiz, "completed", Some(0.8));

    let entries = page(learner.db.conn(), 0, 50).expect("page");

    let summary: Vec<(&str, Option<&str>)> = entries
        .iter()
        .map(|e| (e.verb.as_str(), e.element_title.as_deref()))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("progressed", None),
            ("passed", Some("Check")),
            ("progressed", None),
            ("completed", Some("Lifting")),
            ("launched", Some("Lifting")),
            ("registered", None),
        ]
    );
    assert!(entries
        .iter()
        .all(|e| e.course_title.as_deref() == Some("Warehouse safety")));
    assert!(entries.iter().all(|e| e.kind == "completion"));
    assert_eq!(entries[0].percent, Some(100));
    assert_eq!(entries[1].score, Some(0.8));
    assert_eq!(entries[1].percent, None);
    assert_eq!(entries[2].percent, Some(50));
    let records = learner.records();
    assert_eq!(entries[0].id, records[0].id);
    assert_eq!(entries[0].bytes, records[0].bytes);
    assert_eq!(entries[0].recorded_at, records[0].recorded_at);
}

#[test]
fn the_log_is_read_a_page_at_a_time() {
    let learner = Learner::with_video_completed();
    let ids = learner.ids();

    let first = page(learner.db.conn(), 0, 3).expect("page");
    let rest = page(learner.db.conn(), 3, 3).expect("page");

    let paged: Vec<String> = first.into_iter().chain(rest).map(|e| e.id).collect();
    assert_eq!(paged, ids);
    assert!(page(learner.db.conn(), 4, 3).unwrap().is_empty());
}

#[test]
fn deleting_one_record_removes_only_that_record() {
    let learner = Learner::with_video_completed();
    let ids = learner.ids();

    let removed = delete(learner.db.conn(), &ids[1..2]).expect("delete");

    assert_eq!(removed, 1);
    let left = learner.ids();
    assert_eq!(left, vec![ids[0].clone(), ids[2].clone(), ids[3].clone()]);
    assert_eq!(usage(learner.db.conn()).unwrap().records, 3);
    // And the export no longer holds it.
    let exported = export(learner.db.conn(), &key(LEARNER_SEED)).expect("export");
    let statements: Vec<serde_json::Value> = serde_json::from_slice(&exported.statements).unwrap();
    assert_eq!(statements.len(), 3);
}

#[test]
fn deleting_a_selection_removes_those_records_and_ignores_unknown_ids() {
    let learner = Learner::with_video_completed();
    let ids = learner.ids();

    let removed = delete(
        learner.db.conn(),
        &[ids[0].clone(), ids[3].clone(), "no-such-record".to_string()],
    )
    .expect("delete");

    assert_eq!(removed, 2);
    assert_eq!(learner.ids(), vec![ids[1].clone(), ids[2].clone()]);
}

#[test]
fn deleting_everything_empties_the_log() {
    let learner = Learner::with_video_completed();

    let removed = delete_all(learner.db.conn()).expect("delete all");

    assert_eq!(removed, 4);
    assert!(learner.records().is_empty());
    let usage = usage(learner.db.conn()).unwrap();
    assert_eq!((usage.records, usage.bytes), (0, 0));
    // The learner's activity goes on being recorded afterwards.
    learner.progress(&learner.quiz, "in_progress", None);
    assert_eq!(learner.records().len(), 1);
}

fn queue(learner: &Learner, recipient: &str, record_id: &str, state: &str) {
    learner
        .db
        .conn()
        .execute(
            "INSERT INTO learning_record_outbox (recipient_id, record_id, state, created_at, updated_at) \
             VALUES (?1, ?2, ?3, '2026-11-20T00:00:00Z', '2026-11-20T00:00:00Z')",
            rusqlite::params![recipient, record_id, state],
        )
        .unwrap();
}

fn outbox(learner: &Learner) -> Vec<(String, String)> {
    let conn = learner.db.conn();
    let mut statement = conn
        .prepare("SELECT recipient_id, record_id FROM learning_record_outbox ORDER BY recipient_id")
        .unwrap();
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap();
    rows.map(|row| row.unwrap()).collect()
}

#[test]
fn a_deleted_record_waiting_to_be_sent_is_no_longer_sent() {
    let learner = Learner::with_video_completed();
    let ids = learner.ids();
    queue(&learner, "destination-1", &ids[0], "queued");
    queue(&learner, "destination-2", &ids[0], "failed");
    queue(&learner, "destination-1", &ids[1], "queued");

    delete(learner.db.conn(), &ids[0..1]).expect("delete");

    assert_eq!(
        outbox(&learner),
        vec![("destination-1".to_string(), ids[1].clone())]
    );
    delete_all(learner.db.conn()).expect("delete all");
    assert!(outbox(&learner).is_empty());
}

// ── What cmi5 units write on their own ───────────────────────────────────

const PACKAGE: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const UNIT_ACTOR: &str = "unit-pseudonym";

/// A cmi5 attempt at the learner's video element with one session in
/// `status`, in which the unit sent one statement of its own (`answered`)
/// and saved one state document. Returns the statement's id.
fn unit_wrote(learner: &Learner, session: &str, status: &str) -> String {
    let conn = learner.db.conn();
    conn.execute(
        "INSERT OR IGNORE INTO packages (hash, standard, title, description_json, archive_bytes, \
         location, imported_at) VALUES (?1, 'cmi5', 'Forklift', '{}', 1, 'store', '2026-11-20T00:00:00Z')",
        [PACKAGE],
    )
    .unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO package_attempts (id, enrollment_id, element_id, package_hash, \
         attempt_number, status, started_at, updated_at) \
         VALUES ('attempt-1', ?1, ?2, ?3, 1, 'in_progress', '2026-11-20T00:00:00Z', '2026-11-20T00:00:00Z')",
        rusqlite::params![learner.enrollment, learner.video, PACKAGE],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO cmi5_sessions (id, attempt_id, activity_id, status, launched_at) \
         VALUES (?1, 'attempt-1', 'au-1', ?2, '2026-11-20T00:00:00Z')",
        [session, status],
    )
    .unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let statement = serde_json::json!({
        "id": id,
        "actor": { "objectType": "Agent", "account": { "homePage": "https://example.org", "name": UNIT_ACTOR } },
        "verb": { "id": "http://adlnet.gov/expapi/verbs/answered" },
        "object": { "id": "https://example.org/forklift/q1" },
        "result": { "score": { "scaled": 0.5 } },
        "context": { "registration": "11111111-1111-4111-8111-111111111111" },
        "stored": "2099-01-01T00:00:00.000Z"
    });
    conn.execute(
        "INSERT INTO cmi5_statements (id, attempt_id, session_id, activity_id, verb, statement_json, stored_at) \
         VALUES (?1, 'attempt-1', ?2, 'au-1', NULL, ?3, '2099-01-01T00:00:00.000Z')",
        rusqlite::params![id, session, statement.to_string()],
    )
    .unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO xapi_documents (package_hash, kind, activity_id, document_id, \
         content_type, body, etag, updated_at) \
         VALUES (?1, 'state', 'au-1', ?2, 'application/json', x'7b7d', '\"e\"', '2026-11-20T00:00:00Z')",
        [PACKAGE, session],
    )
    .unwrap();
    id
}

fn count(learner: &Learner, table: &str) -> i64 {
    learner
        .db
        .conn()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn what_a_unit_wrote_is_listed_counted_and_exported_for_the_learner() {
    let learner = Learner::with_video_completed();
    let before = usage(learner.db.conn()).expect("usage");
    let id = unit_wrote(&learner, "session-1", "terminated");

    let entries = page(learner.db.conn(), 0, 50).expect("page");
    let entry = entries
        .iter()
        .find(|entry| entry.id == id)
        .expect("listed in the log");
    assert_eq!(entry.kind, "interaction");
    assert_eq!(entry.verb, "answered");
    assert_eq!(entry.course_title.as_deref(), Some("Warehouse safety"));
    assert_eq!(entry.element_title.as_deref(), Some("Lifting"));
    assert_eq!(entry.activity_id, "https://example.org/forklift/q1");
    assert_eq!(entry.score, Some(0.5));
    assert!(entry.bytes > 0);

    let after = usage(learner.db.conn()).expect("usage");
    assert_eq!(after.records, before.records + 1);
    assert_eq!(after.interaction_records, before.interaction_records + 1);
    assert_eq!(after.bytes, before.bytes + entry.bytes);

    let exported = export(learner.db.conn(), &key(LEARNER_SEED)).expect("export");
    let text = String::from_utf8(exported.statements.clone()).unwrap();
    assert!(
        !text.contains(UNIT_ACTOR),
        "the unit's pseudonym stays home"
    );
    assert!(!text.contains(&id), "the statement id is the learner's own");
    let statements: Vec<serde_json::Value> = serde_json::from_slice(&exported.statements).unwrap();
    let unit = statements
        .iter()
        .find(|s| s["object"]["id"] == "https://example.org/forklift/q1")
        .expect("exported");
    let learner_name = did(LEARNER_SEED).trim_start_matches("did:key:").to_string();
    assert_eq!(unit["actor"]["account"]["name"], learner_name.as_str());
    assert_ne!(
        unit["context"]["registration"],
        "11111111-1111-4111-8111-111111111111"
    );
    record_export::verify(&exported.statements, &exported.signature).expect("still signed");
}

#[test]
fn deleting_removes_what_units_wrote_except_in_an_open_session() {
    let learner = Learner::with_video_completed();
    let one = unit_wrote(&learner, "session-1", "terminated");
    unit_wrote(&learner, "session-2", "abandoned");

    assert_eq!(delete(learner.db.conn(), &[one]).expect("delete"), 1);
    assert_eq!(count(&learner, "cmi5_statements"), 1);

    unit_wrote(&learner, "session-3", "open");
    delete_all(learner.db.conn()).expect("delete all");
    assert_eq!(
        count(&learner, "cmi5_statements"),
        1,
        "the open session's stays"
    );
    assert_eq!(
        count(&learner, "xapi_documents"),
        3,
        "its package has a session open: documents stay"
    );

    learner
        .db
        .conn()
        .execute("UPDATE cmi5_sessions SET status = 'terminated'", [])
        .unwrap();
    delete_all(learner.db.conn()).expect("delete all");
    assert_eq!(count(&learner, "cmi5_statements"), 0);
    assert_eq!(count(&learner, "xapi_documents"), 0);
}

#[test]
fn what_units_wrote_goes_first_when_the_log_is_over_its_limit() {
    let learner = Learner::with_video_completed();
    let records = learner.ids().len();
    unit_wrote(&learner, "session-1", "terminated");
    unit_wrote(&learner, "session-2", "open");
    let total = usage(learner.db.conn()).expect("usage").bytes;

    // Over by one byte: the closed session's statement goes, nothing else.
    enforce_limit(learner.db.conn(), total - 1).expect("limit");
    assert_eq!(
        count(&learner, "cmi5_statements"),
        1,
        "the open session's stays"
    );
    assert_eq!(learner.ids().len(), records, "every learning record stays");
}
