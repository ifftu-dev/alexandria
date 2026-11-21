//! Host-seam tests: a learner enrols, opens and completes elements and runs a
//! package through the same host functions the commands call, and what comes
//! out is the activity log as `list` and `usage` report it.

use alexandria_studio::xapi::Action;
use ed25519_dalek::SigningKey;

use super::*;
use crate::crypto::hash::entity_id;
use crate::db::Database;
use crate::domain::enrollment::UpdateProgressRequest;
use crate::settings::{registry::keys, SettingsStore};

/// A did:key for a seed.
fn did(seed: u8) -> String {
    crate::crypto::did::derive_did_key(&SigningKey::from_bytes(&[seed; 32]))
        .as_str()
        .to_string()
}

fn hex_id(label: &str) -> String {
    entity_id(&[label])
}

/// One profile: its identity, a course published by someone else and the
/// course's elements, before enrolment.
struct Profile {
    db: Database,
    learner_did: String,
    course_id: String,
    video: String,
    quiz: String,
}

const CREATOR_SEED: u8 = 9;

impl Profile {
    fn new(seed: u8) -> Self {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        Self::seed(db, seed)
    }

    fn seed(db: Database, seed: u8) -> Self {
        let learner_did = did(seed);
        let course_id = hex_id("course");
        let video = hex_id("video");
        let quiz = hex_id("quiz");
        db.conn()
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address, display_name) \
                 VALUES (1, ?1, 'addr_test1q', 'Ada')",
                [format!("stake_test1u{seed}")],
            )
            .unwrap();
        SettingsStore::set(db.conn(), keys::IDENTITY_LOCAL_DID, learner_did.clone()).unwrap();
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
        Self {
            db,
            learner_did,
            course_id,
            video,
            quiz,
        }
    }

    fn enroll(&self) -> String {
        crate::enrollment::enroll(&self.db, &self.course_id)
            .expect("enrol")
            .id
    }

    /// The id of the enrolment [`Profile::enroll`] made.
    fn enroll_id(&self) -> String {
        crate::enrollment::list(&self.db, None)
            .expect("enrolments")
            .remove(0)
            .id
    }

    fn records(&self) -> Vec<LearningRecord> {
        list(self.db.conn()).expect("list records")
    }

    fn verbs(&self) -> Vec<String> {
        self.records().into_iter().map(|r| r.verb).collect()
    }
}

#[test]
fn enrolling_records_a_registration_in_the_course_naming_the_learner_by_did() {
    let profile = Profile::new(1);
    let enrollment = profile.enroll();

    let records = profile.records();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.verb, "registered");
    assert_eq!(record.kind, RecordKind::Completion);
    assert_eq!(record.learner_did, profile.learner_did);
    assert_eq!(record.course_id, profile.course_id);
    assert_eq!(record.element_id, None);
    assert_eq!(record.registration.as_deref(), Some(enrollment.as_str()));
    assert_eq!(
        record.activity_id,
        format!("{}/courses/{}", did(CREATOR_SEED), profile.course_id)
    );
}

fn progress(profile: &Profile, enrollment: &str, element: &str, status: &str, score: Option<f64>) {
    crate::enrollment::update_progress(
        &profile.db,
        enrollment.to_string(),
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

#[test]
fn opening_an_element_records_its_launch_once() {
    let profile = Profile::new(1);
    let enrollment = profile.enroll();

    progress(&profile, &enrollment, &profile.video, "in_progress", None);
    progress(&profile, &enrollment, &profile.video, "in_progress", None);

    let launches: Vec<_> = profile
        .records()
        .into_iter()
        .filter(|r| r.verb == "launched")
        .collect();
    assert_eq!(launches.len(), 1);
    let launch = &launches[0];
    assert_eq!(launch.element_id.as_deref(), Some(profile.video.as_str()));
    assert_eq!(launch.registration.as_deref(), Some(enrollment.as_str()));
    assert_eq!(
        launch.activity_id,
        format!(
            "{}/courses/{}/elements/{}",
            did(CREATOR_SEED),
            profile.course_id,
            profile.video
        )
    );
}

#[test]
fn completing_an_element_records_the_completion_and_the_course_progress() {
    let profile = Profile::new(1);
    let enrollment = profile.enroll();
    progress(&profile, &enrollment, &profile.video, "in_progress", None);
    progress(&profile, &enrollment, &profile.video, "completed", None);
    // Marking it complete again records nothing new.
    progress(&profile, &enrollment, &profile.video, "completed", None);

    assert_eq!(
        profile.verbs(),
        vec!["progressed", "completed", "launched", "registered"]
    );
    let records = profile.records();
    assert_eq!(
        records[1].element_id.as_deref(),
        Some(profile.video.as_str())
    );
    let course_progress = &records[0];
    assert_eq!(course_progress.element_id, None);
    match &course_progress.event.action {
        Action::Progressed { percent, .. } => assert_eq!(*percent, 50),
        other => panic!("expected progress, got {other:?}"),
    }
}

#[test]
fn an_assessment_is_recorded_as_pass_or_fail_with_its_score_and_never_its_answers() {
    let profile = Profile::new(1);
    let enrollment = profile.enroll();
    crate::enrollment::record_submission(
        &profile.db,
        enrollment.clone(),
        profile.quiz.clone(),
        "quiz".into(),
        r#"{"answers":[2,0,1]}"#.into(),
        0.4,
    )
    .expect("submission");
    assert_eq!(
        profile.verbs(),
        vec!["registered"],
        "answers are never recorded"
    );

    progress(&profile, &enrollment, &profile.quiz, "completed", Some(0.4));
    // A retake that passes is recorded; repeating the same result is not.
    progress(&profile, &enrollment, &profile.quiz, "completed", Some(0.9));
    progress(&profile, &enrollment, &profile.quiz, "completed", Some(0.9));

    let results: Vec<_> = profile
        .records()
        .into_iter()
        .filter(|r| r.element_id.as_deref() == Some(profile.quiz.as_str()))
        .collect();
    assert_eq!(
        results.iter().map(|r| r.verb.as_str()).collect::<Vec<_>>(),
        vec!["passed", "failed"]
    );
    for record in &results {
        let (outcome, expected) = match &record.event.action {
            Action::Passed { outcome, .. } => (outcome, 0.9),
            Action::Failed { outcome, .. } => (outcome, 0.4),
            other => panic!("expected a result, got {other:?}"),
        };
        assert_eq!(
            outcome.score.as_ref().and_then(|s| s.scaled),
            Some(expected)
        );
        assert_eq!(outcome.response, None);
    }
}

#[test]
fn a_record_becomes_a_statement_naming_the_learner_by_did_in_the_local_log() {
    let profile = Profile::new(1);
    profile.enroll();
    let record = profile.records().remove(0);

    let recipient = local_recipient(&[1; 32], &profile.learner_did).expect("recipient");
    let built = statement(&record, &recipient, &BuildOptions::default()).expect("statement");
    let json = serde_json::to_value(&built).unwrap();
    assert_eq!(
        json["actor"]["account"]["name"],
        serde_json::json!(profile.learner_did.trim_start_matches("did:key:"))
    );
    assert_eq!(json["object"]["id"], serde_json::json!(record.activity_id));
    assert_eq!(
        statement(&record, &recipient, &BuildOptions::default()).unwrap(),
        built,
        "the same record and recipient give the same statement"
    );
}

mod package {
    //! A SCORM 1.2 package element in the same course, driven through
    //! `crate::packages::attempts` the way the player drives it.

    use std::sync::Mutex;

    use serde_json::json;
    use tempfile::TempDir;

    use super::*;
    use crate::domain::package_attempt::PackageCommit;
    use crate::package_serving::Launches;
    use crate::packages::{self, attempts, PackageDirs};

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    /// A stored-only zip, enough for a well-formed test package.
    fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut directory = Vec::new();
        for (name, data) in entries {
            let offset = out.len() as u32;
            let mut shared = Vec::new();
            shared.extend_from_slice(&20u16.to_le_bytes());
            shared.extend_from_slice(&0x0800u16.to_le_bytes());
            shared.extend_from_slice(&0u16.to_le_bytes());
            shared.extend_from_slice(&0u16.to_le_bytes());
            shared.extend_from_slice(&0x21u16.to_le_bytes());
            shared.extend_from_slice(&crc32(data).to_le_bytes());
            shared.extend_from_slice(&(data.len() as u32).to_le_bytes());
            shared.extend_from_slice(&(data.len() as u32).to_le_bytes());
            shared.extend_from_slice(&(name.len() as u16).to_le_bytes());
            shared.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&shared);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);
            directory.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            directory.extend_from_slice(&((3u16 << 8) | 20).to_le_bytes());
            directory.extend_from_slice(&shared);
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&0u16.to_le_bytes());
            directory.extend_from_slice(&(0o100644u32 << 16).to_le_bytes());
            directory.extend_from_slice(&offset.to_le_bytes());
            directory.extend_from_slice(name.as_bytes());
        }
        let directory_offset = out.len() as u32;
        out.extend_from_slice(&directory);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(directory.len() as u32).to_le_bytes());
        out.extend_from_slice(&directory_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest identifier="com.example.lifting" version="1.0"
    xmlns="http://www.imsproject.org/xsd/imscp_rootv1p1p2"
    xmlns:adlcp="http://www.adlnet.org/xsd/adlcp_rootv1p2">
  <metadata><schema>ADL SCORM</schema><schemaversion>1.2</schemaversion></metadata>
  <organizations default="org">
    <organization identifier="org">
      <title>Lifting</title>
      <item identifier="welcome" identifierref="res-welcome"><title>Welcome</title></item>
      <item identifier="lifting" identifierref="res-lifting"><title>Lifting and carrying</title></item>
    </organization>
  </organizations>
  <resources>
    <resource identifier="res-welcome" type="webcontent" adlcp:scormtype="sco" href="welcome.html">
      <file href="welcome.html"/>
    </resource>
    <resource identifier="res-lifting" type="webcontent" adlcp:scormtype="sco" href="lifting.html">
      <file href="lifting.html"/>
    </resource>
  </resources>
</manifest>"#;

    pub(super) struct Learner {
        pub(super) db: Mutex<Database>,
        dirs: PackageDirs,
        _root: TempDir,
        pub(super) enrollment: String,
        pub(super) element: String,
        hash: String,
        course_id: String,
    }

    impl Learner {
        pub(super) async fn new() -> Self {
            let profile = Profile::new(1);
            let enrollment = profile.enroll();
            let course_id = profile.course_id.clone();
            let db = Mutex::new(profile.db);
            let root = TempDir::new().expect("temp dir");
            let dirs = PackageDirs {
                archives: root.path().join("packages"),
                cache: root.path().join("packagecache"),
            };
            dirs.ensure().expect("package dirs");
            let source = root.path().join("lifting.zip");
            std::fs::write(
                &source,
                zip(&[
                    ("imsmanifest.xml", MANIFEST.as_bytes()),
                    ("welcome.html", b"<html><body>Welcome</body></html>"),
                    ("lifting.html", b"<html><body>Lift</body></html>"),
                ]),
            )
            .unwrap();
            let package = packages::import(&db, &dirs, &source).await.expect("import");
            let element = hex_id("package");
            db.lock()
                .unwrap()
                .conn()
                .execute(
                    "INSERT INTO course_elements (id, chapter_id, title, element_type, content_cid, position) \
                     VALUES (?1, 'ch1', 'Lifting', 'package', ?2, 2)",
                    rusqlite::params![element, package.hash],
                )
                .unwrap();
            Self {
                db,
                dirs,
                _root: root,
                enrollment,
                element,
                hash: package.hash,
                course_id,
            }
        }

        pub(super) fn records(&self) -> Vec<LearningRecord> {
            list(self.db.lock().unwrap().conn()).expect("list records")
        }

        fn activity_iri(&self, activity: &str) -> String {
            format!(
                "{}/courses/{}/elements/{}/packages/{}/activities/{activity}",
                did(CREATOR_SEED),
                self.course_id,
                self.element,
                self.hash
            )
        }

        pub(super) async fn open(&self) -> String {
            attempts::open(&self.db, &self.enrollment, &self.element)
                .await
                .expect("open")
                .id
        }

        pub(super) async fn launch(&self, attempt: &str, activity: &str) {
            attempts::initialise(&self.db, &[7; 32], attempt, activity)
                .await
                .expect("initialise");
        }

        pub(super) async fn commit(
            &self,
            attempt: &str,
            activity: &str,
            cmi: serde_json::Value,
            session_ended: bool,
        ) {
            attempts::commit(
                &self.db,
                PackageCommit {
                    attempt_id: attempt.to_string(),
                    activity_id: activity.to_string(),
                    session_ended,
                    runtime: json!({ "cmi": cmi }),
                },
            )
            .await
            .expect("commit");
        }

        pub(super) async fn terminate(&self, attempt: &str) {
            attempts::terminate(&self.db, &self.dirs, &Launches::new(), attempt)
                .await
                .expect("terminate");
        }
    }

    /// A passed lesson with one answered question.
    pub(super) fn passed_with_answer() -> serde_json::Value {
        json!({
            "core": {
                "lesson_status": "passed",
                "score": { "raw": "90", "min": "0", "max": "100" },
                "session_time": "0000:05:00",
            },
            "interactions": [
                { "id": "q1", "type": "choice", "student_response": "b", "result": "correct" }
            ],
        })
    }

    #[tokio::test]
    async fn package_launches_results_and_interactions_are_recorded_once() {
        let learner = Learner::new().await;
        let attempt = learner.open().await;
        learner.launch(&attempt, "lifting").await;
        learner
            .commit(&attempt, "lifting", passed_with_answer(), true)
            .await;
        learner.terminate(&attempt).await;

        let records: Vec<_> = learner
            .records()
            .into_iter()
            .filter(|r| r.element_id.as_deref() == Some(learner.element.as_str()))
            .collect();
        let summary: Vec<(&str, RecordKind)> =
            records.iter().map(|r| (r.verb.as_str(), r.kind)).collect();
        assert_eq!(
            summary,
            vec![
                ("responded", RecordKind::Interaction),
                ("passed", RecordKind::Completion),
                ("launched", RecordKind::Completion),
                ("launched", RecordKind::Completion),
            ]
        );
        let (response, passed, package_launch, element_launch) =
            (&records[0], &records[1], &records[2], &records[3]);
        assert_eq!(
            response.activity_id,
            format!("{}/interactions/q1", learner.activity_iri("lifting"))
        );
        assert_eq!(passed.activity_id, learner.activity_iri("lifting"));
        assert_eq!(passed.registration.as_deref(), Some(attempt.as_str()));
        assert_eq!(package_launch.activity_id, learner.activity_iri("lifting"));
        assert_eq!(
            element_launch.registration.as_deref(),
            Some(learner.enrollment.as_str())
        );
        match &response.event.action {
            Action::Responded { outcome, .. } => {
                assert_eq!(outcome.response.as_deref(), Some("b"))
            }
            other => panic!("expected a response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_session_the_content_never_finished_is_recorded_when_the_launch_ends() {
        let learner = Learner::new().await;
        let attempt = learner.open().await;
        learner.launch(&attempt, "welcome").await;
        learner
            .commit(
                &attempt,
                "welcome",
                json!({ "core": { "lesson_status": "completed", "session_time": "0000:01:00" } }),
                false,
            )
            .await;
        assert!(
            !learner.records().iter().any(|r| r.verb == "completed"),
            "nothing is recorded mid-session"
        );

        learner.terminate(&attempt).await;
        let completed: Vec<_> = learner
            .records()
            .into_iter()
            .filter(|r| r.verb == "completed")
            .collect();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].activity_id, learner.activity_iri("welcome"));
    }
}

#[tokio::test]
async fn usage_reports_the_log_size_against_its_visible_limit() {
    let learner = package::Learner::new().await;
    let attempt = learner.open().await;
    learner
        .commit(&attempt, "lifting", package::passed_with_answer(), true)
        .await;

    let records = learner.records();
    let usage = usage(learner.db.lock().unwrap().conn()).expect("usage");
    assert_eq!(usage.limit_bytes, LIMIT_BYTES);
    assert_eq!(usage.records, records.len() as u64);
    assert_eq!(usage.interaction_records, 1);
    assert_eq!(usage.bytes, records.iter().map(|r| r.bytes).sum::<u64>());
    assert!(records.iter().all(|r| r.bytes > 0));
}

#[tokio::test]
async fn past_the_limit_the_oldest_interaction_records_go_first() {
    let learner = package::Learner::new().await;
    let attempt = learner.open().await;
    for (activity, question) in [("welcome", "q-old"), ("lifting", "q-new")] {
        let mut cmi = package::passed_with_answer();
        cmi["interactions"][0]["id"] = serde_json::json!(question);
        learner.commit(&attempt, activity, cmi, true).await;
    }
    let before = learner.records();
    let oldest_response = before
        .iter()
        .rev()
        .find(|r| r.kind == RecordKind::Interaction)
        .unwrap()
        .clone();
    let total: u64 = before.iter().map(|r| r.bytes).sum();

    let conn_guard = learner.db.lock().unwrap();
    let removed = enforce_limit(conn_guard.conn(), total - oldest_response.bytes).expect("limit");
    drop(conn_guard);

    assert_eq!(removed, 1);
    let after = learner.records();
    assert_eq!(after.len(), before.len() - 1);
    assert!(!after.iter().any(|r| r.id == oldest_response.id));
    assert!(after
        .iter()
        .any(|r| r.kind == RecordKind::Interaction && r.activity_id.ends_with("/q-new")));
}

#[tokio::test]
async fn far_past_the_limit_each_attempt_keeps_one_summary_per_activity() {
    let learner = package::Learner::new().await;
    let profile_db = &learner.db;
    let attempt = learner.open().await;
    learner.launch(&attempt, "lifting").await;
    learner
        .commit(&attempt, "lifting", package::passed_with_answer(), true)
        .await;
    // A record still waiting to be sent is never removed.
    let launch_record = learner
        .records()
        .into_iter()
        .find(|r| r.verb == "launched" && r.registration.as_deref() == Some(attempt.as_str()))
        .unwrap();
    profile_db
        .lock()
        .unwrap()
        .conn()
        .execute(
            "INSERT INTO learning_record_outbox (recipient_id, record_id, created_at, updated_at) \
             VALUES ('destination-1', ?1, '2026-11-20T00:00:00Z', '2026-11-20T00:00:00Z')",
            [&launch_record.id],
        )
        .unwrap();

    enforce_limit(profile_db.lock().unwrap().conn(), 1).expect("limit");

    let after = learner.records();
    assert!(after.iter().all(|r| r.kind == RecordKind::Completion));
    let lifting: Vec<_> = after
        .iter()
        .filter(|r| r.registration.as_deref() == Some(attempt.as_str()))
        .collect();
    let mut verbs: Vec<_> = lifting.iter().map(|r| r.verb.as_str()).collect();
    verbs.sort();
    assert_eq!(
        verbs,
        vec!["launched", "passed"],
        "summary plus the queued record"
    );
    assert!(lifting.iter().any(|r| r.verb == "passed" && r.summary));
    // One record per activity of the enrolment: the course and the element.
    let enrolment: Vec<_> = after
        .iter()
        .filter(|r| r.registration.as_deref() == Some(learner.enrollment.as_str()))
        .collect();
    assert_eq!(enrolment.len(), 2);
}

#[test]
fn each_profile_keeps_its_own_records() {
    let root = tempfile::TempDir::new().unwrap();
    let open = |name: &str, key: u8| {
        let db = Database::open_encrypted(&root.path().join(name), &[key; 32]).expect("open");
        db.run_migrations().expect("migrations");
        db
    };
    let first = Profile::seed(open("first.db", 1), 1);
    let second = Profile::seed(open("second.db", 2), 2);
    first.enroll();
    progress(
        &first,
        &first.enroll_id(),
        &first.video,
        "in_progress",
        None,
    );
    second.enroll();

    assert_eq!(first.verbs(), vec!["launched", "registered"]);
    assert_eq!(second.verbs(), vec!["registered"]);
    assert!(first
        .records()
        .iter()
        .all(|r| r.learner_did == first.learner_did));
    assert!(second
        .records()
        .iter()
        .all(|r| r.learner_did == second.learner_did));
    assert_ne!(first.learner_did, second.learner_did);

    // Reopened, each profile's database still holds only its own.
    drop(first);
    let reopened = open("first.db", 1);
    let records = list(reopened.conn()).unwrap();
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|r| r.learner_did == did(1)));
}
