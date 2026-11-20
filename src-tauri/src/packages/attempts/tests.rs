//! Host-seam tests: a package goes in through `crate::packages::import`, an
//! enrolment takes it, and attempts are driven through the public functions
//! of `crate::packages::attempts` the way the player drives them. What comes
//! out is what a learner sees: the attempt, the state a launch resumes, the
//! identifier content receives and element progress.

use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::json;
use tempfile::TempDir;

use super::*;
use crate::content_store::node::ContentNode;
use crate::db::Database;
use crate::domain::package_attempt::{PackageAttemptProgress, PackageAttemptStatus, PackageCommit};
use crate::package_serving::Launches;
use crate::packages::{self, PackageDirs};

mod fixture {
    //! A stored-only zip writer, enough for well-formed test packages.

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

    pub fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut directory = Vec::new();
        for (name, data) in entries {
            let offset = out.len() as u32;
            let mut shared = Vec::new();
            shared.extend_from_slice(&20u16.to_le_bytes()); // version needed
            shared.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
            shared.extend_from_slice(&0u16.to_le_bytes()); // stored
            shared.extend_from_slice(&0u16.to_le_bytes()); // time
            shared.extend_from_slice(&0x21u16.to_le_bytes()); // date: 1980-01-01
            shared.extend_from_slice(&crc32(data).to_le_bytes());
            shared.extend_from_slice(&(data.len() as u32).to_le_bytes());
            shared.extend_from_slice(&(data.len() as u32).to_le_bytes());
            shared.extend_from_slice(&(name.len() as u16).to_le_bytes());
            shared.extend_from_slice(&0u16.to_le_bytes()); // extra field length

            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&shared);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);

            directory.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            directory.extend_from_slice(&((3u16 << 8) | 20).to_le_bytes()); // made on Unix
            directory.extend_from_slice(&shared);
            directory.extend_from_slice(&0u16.to_le_bytes()); // comment length
            directory.extend_from_slice(&0u16.to_le_bytes()); // disk number
            directory.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
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
}

/// A SCORM 1.2 package: two SCOs, the second with a mastery score and launch
/// data, and a handout that is only shown.
fn induction_manifest(identifier: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest identifier="{identifier}" version="1.0"
    xmlns="http://www.imsproject.org/xsd/imscp_rootv1p1p2"
    xmlns:adlcp="http://www.adlnet.org/xsd/adlcp_rootv1p2">
  <metadata>
    <schema>ADL SCORM</schema>
    <schemaversion>1.2</schemaversion>
  </metadata>
  <organizations default="org">
    <organization identifier="org">
      <title>Warehouse induction</title>
      <item identifier="welcome" identifierref="res-welcome">
        <title>Welcome</title>
      </item>
      <item identifier="lifting" identifierref="res-lifting">
        <title>Lifting and carrying</title>
        <adlcp:datafromlms>route=b</adlcp:datafromlms>
        <adlcp:masteryscore>80</adlcp:masteryscore>
      </item>
      <item identifier="handout" identifierref="res-handout">
        <title>Handout</title>
      </item>
    </organization>
  </organizations>
  <resources>
    <resource identifier="res-welcome" type="webcontent" adlcp:scormtype="sco" href="welcome/index.html">
      <file href="welcome/index.html"/>
    </resource>
    <resource identifier="res-lifting" type="webcontent" adlcp:scormtype="sco" href="lifting/index.html">
      <file href="lifting/index.html"/>
    </resource>
    <resource identifier="res-handout" type="webcontent" adlcp:scormtype="asset" href="handout.html">
      <file href="handout.html"/>
    </resource>
  </resources>
</manifest>"#
    )
}

fn package_bytes(identifier: &str) -> Vec<u8> {
    let manifest = induction_manifest(identifier);
    fixture::zip(&[
        ("imsmanifest.xml", manifest.as_bytes()),
        ("welcome/index.html", b"<html><body>Welcome</body></html>"),
        ("lifting/index.html", b"<html><body>Lift</body></html>"),
        ("handout.html", b"<html><body>Handout</body></html>"),
    ])
}

/// A learner enrolled in a course whose elements are package elements, one
/// per imported package, with a running content node for extraction.
struct Learner {
    db: Mutex<Database>,
    dirs: PackageDirs,
    node: ContentNode,
    key: [u8; 32],
    root: TempDir,
}

const ENROLLMENT: &str = "enr1";

impl Learner {
    async fn new() -> Self {
        let root = TempDir::new().expect("temp dir");
        let dirs = PackageDirs {
            archives: root.path().join("packages"),
            cache: root.path().join("packagecache"),
        };
        dirs.ensure().expect("package dirs");
        let node = ContentNode::new(&root.path().join("iroh"));
        node.start(None).await.expect("start node");
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db.conn()
            .execute_batch(&format!(
                "INSERT INTO local_identity (id, stake_address, payment_address, display_name) \
                 VALUES (1, 'stake_test1ulearner', 'addr_test1q123', 'Ada Learner'); \
                 INSERT INTO courses (id, title, author_address, status, content_cid, course_document_version) \
                 VALUES ('c1', 'Induction', 'stake_test1uauthor', 'published', '{doc}', 2); \
                 INSERT INTO course_chapters (id, course_id, title, position) VALUES ('ch1', 'c1', 'Chapter', 0); \
                 INSERT INTO enrollments (id, course_id, status, course_document_cid, course_document_version) \
                 VALUES ('{ENROLLMENT}', 'c1', 'active', '{doc}', 2);",
                doc = "11".repeat(32)
            ))
            .expect("seed course and enrolment");
        Self {
            db: Mutex::new(db),
            dirs,
            node,
            key: [7; 32],
            root,
        }
    }

    /// Imports a package and adds it to the course as element `element_id`.
    /// Returns the package hash.
    async fn add_package(&self, element_id: &str, identifier: &str) -> String {
        self.add_package_bytes(element_id, &package_bytes(identifier))
            .await
    }

    /// Imports the package archive `bytes` and adds it to the course as
    /// element `element_id`. Returns the package hash.
    async fn add_package_bytes(&self, element_id: &str, bytes: &[u8]) -> String {
        let source: PathBuf = self.root.path().join(format!("{element_id}.zip"));
        std::fs::write(&source, bytes).expect("write package");
        let package = packages::import(&self.db, &self.dirs, &source)
            .await
            .expect("import");
        self.db
            .lock()
            .unwrap()
            .conn()
            .execute(
                "INSERT INTO course_elements (id, chapter_id, title, element_type, content_cid, position) \
                 VALUES (?1, 'ch1', 'Induction', 'package', ?2, 0)",
                rusqlite::params![element_id, package.hash],
            )
            .expect("package element");
        package.hash
    }

    fn element_progress(&self, element_id: &str) -> Option<ElementProgress> {
        let db = self.db.lock().unwrap();
        crate::enrollment::progress(&db, ENROLLMENT)
            .expect("progress")
            .into_iter()
            .find(|progress| progress.element_id == element_id)
    }
}

#[tokio::test]
async fn opening_a_package_element_starts_the_first_attempt_and_opening_again_resumes_it() {
    let learner = Learner::new().await;
    let hash = learner.add_package("el_pkg", "com.example.induction").await;

    let attempt = open(&learner.db, ENROLLMENT, "el_pkg").await.expect("open");
    assert_eq!(attempt.attempt_number, 1);
    assert_eq!(attempt.status, PackageAttemptStatus::InProgress);
    assert_eq!(attempt.package_hash, hash);
    assert_eq!(attempt.ended_at, None);

    let again = open(&learner.db, ENROLLMENT, "el_pkg")
        .await
        .expect("open again");
    assert_eq!(again.id, attempt.id);

    let progress = learner
        .element_progress("el_pkg")
        .expect("element progress");
    assert_eq!(progress.status, "in_progress");
}

#[tokio::test]
async fn a_first_launch_gets_the_host_values_and_starts_from_the_beginning() {
    let learner = Learner::new().await;
    learner.add_package("el_pkg", "com.example.induction").await;
    let attempt = open(&learner.db, ENROLLMENT, "el_pkg").await.expect("open");

    let init = initialise(&learner.db, &learner.key, &attempt.id, "lifting")
        .await
        .expect("initialise");

    assert_eq!(init.attempt_id, attempt.id);
    assert_eq!(init.activity_id, "lifting");
    assert_eq!(init.learner_name, "Ada Learner");
    assert_eq!(init.entry, "ab-initio");
    assert_eq!(init.launch_data.as_deref(), Some("route=b"));
    assert_eq!(init.mastery_score, Some(80.0));
    assert!(!init.learner_id.is_empty());

    // What the wrapper loads into the SCORM 1.2 run-time before the lesson
    // starts.
    let cmi = &init.runtime["cmi"];
    assert_eq!(cmi["core"]["student_id"], json!(init.learner_id));
    assert_eq!(cmi["core"]["student_name"], json!("Ada Learner"));
    assert_eq!(cmi["core"]["entry"], json!("ab-initio"));
    assert_eq!(cmi["core"]["credit"], json!("credit"));
    assert_eq!(cmi["core"]["lesson_mode"], json!("normal"));
    assert_eq!(cmi["core"]["total_time"], json!("0000:00:00"));
    assert_eq!(cmi["launch_data"], json!("route=b"));
    assert_eq!(cmi["student_data"]["mastery_score"], json!("80"));
}

/// A SCORM 1.2 commit payload as the library renders it.
fn scorm12_commit(
    attempt_id: &str,
    activity_id: &str,
    core: serde_json::Value,
    session_ended: bool,
) -> PackageCommit {
    PackageCommit {
        attempt_id: attempt_id.to_string(),
        activity_id: activity_id.to_string(),
        session_ended,
        runtime: json!({ "cmi": { "core": core, "suspend_data": "seen=1,2" } }),
    }
}

#[tokio::test]
async fn a_suspended_lesson_resumes_where_it_left_off_with_its_time_counted_once() {
    let learner = Learner::new().await;
    learner.add_package("el_pkg", "com.example.induction").await;
    let attempt = open(&learner.db, ENROLLMENT, "el_pkg").await.expect("open");
    initialise(&learner.db, &learner.key, &attempt.id, "welcome")
        .await
        .expect("first launch");

    // Two coalesced commits in one session; each carries the session so far.
    for (session_time, ended) in [("0000:04:00", false), ("0000:10:00.50", true)] {
        commit(
            &learner.db,
            scorm12_commit(
                &attempt.id,
                "welcome",
                json!({
                    "lesson_status": "incomplete",
                    "lesson_location": "page-2",
                    "exit": "suspend",
                    "session_time": session_time,
                }),
                ended,
            ),
        )
        .await
        .expect("commit");
    }

    let resumed = initialise(&learner.db, &learner.key, &attempt.id, "welcome")
        .await
        .expect("second launch");
    assert_eq!(resumed.entry, "resume");
    let core = &resumed.runtime["cmi"]["core"];
    assert_eq!(core["entry"], json!("resume"));
    assert_eq!(core["lesson_location"], json!("page-2"));
    assert_eq!(core["lesson_status"], json!("incomplete"));
    assert_eq!(core["total_time"], json!("0000:10:00.50"));
    assert_eq!(
        core.get("session_time"),
        None,
        "the last session's time is not reloaded"
    );
    assert_eq!(core.get("exit"), None);
    assert_eq!(resumed.runtime["cmi"]["suspend_data"], json!("seen=1,2"));

    let progress = learner
        .element_progress("el_pkg")
        .expect("element progress");
    assert_eq!(progress.status, "in_progress");
    assert_eq!(
        progress.time_spent, 601,
        "600.5 seconds, to the nearest second"
    );
}

#[tokio::test]
async fn a_session_the_content_never_finished_still_counts_its_time() {
    let learner = Learner::new().await;
    learner.add_package("el_pkg", "com.example.induction").await;
    let attempt = open(&learner.db, ENROLLMENT, "el_pkg").await.expect("open");
    commit(
        &learner.db,
        scorm12_commit(
            &attempt.id,
            "welcome",
            json!({ "lesson_status": "incomplete", "session_time": "0000:03:00" }),
            false,
        ),
    )
    .await
    .expect("commit");

    // The app went away mid-session; the next launch closes that session.
    let next = initialise(&learner.db, &learner.key, &attempt.id, "welcome")
        .await
        .expect("next launch");
    assert_eq!(next.entry, "");
    assert_eq!(
        next.runtime["cmi"]["core"]["total_time"],
        json!("0000:03:00")
    );
    commit(
        &learner.db,
        scorm12_commit(
            &attempt.id,
            "welcome",
            json!({ "lesson_status": "incomplete", "session_time": "0000:01:00" }),
            true,
        ),
    )
    .await
    .expect("commit");
    let progress = learner
        .element_progress("el_pkg")
        .expect("element progress");
    assert_eq!(progress.time_spent, 240);
}

/// Finishes a lesson in one session with the given status and score.
async fn finish_lesson(
    learner: &Learner,
    attempt_id: &str,
    activity_id: &str,
    status: &str,
    score: serde_json::Value,
) -> PackageAttemptProgress {
    initialise(&learner.db, &learner.key, attempt_id, activity_id)
        .await
        .expect("launch");
    commit(
        &learner.db,
        scorm12_commit(
            attempt_id,
            activity_id,
            json!({ "lesson_status": status, "score": score, "session_time": "0000:01:00" }),
            true,
        ),
    )
    .await
    .expect("commit")
}

#[tokio::test]
async fn the_element_completes_when_every_lesson_is_completed_or_passed_and_the_handout_was_opened()
{
    let learner = Learner::new().await;
    learner.add_package("el_pkg", "com.example.induction").await;
    let attempt = open(&learner.db, ENROLLMENT, "el_pkg").await.expect("open");

    // 30 on a 0..50 scale.
    let after_welcome = finish_lesson(
        &learner,
        &attempt.id,
        "welcome",
        "completed",
        json!({ "raw": "30", "min": "0", "max": "50" }),
    )
    .await;
    assert!(!after_welcome.package_completed);

    // A failed lesson is not a complete one.
    let after_failing = finish_lesson(
        &learner,
        &attempt.id,
        "lifting",
        "failed",
        json!({ "raw": "40" }),
    )
    .await;
    assert!(!after_failing.package_completed);
    assert_eq!(after_failing.element_progress.status, "in_progress");

    // 90 on the default 0..100 scale.
    let after_passing = finish_lesson(
        &learner,
        &attempt.id,
        "lifting",
        "passed",
        json!({ "raw": "90" }),
    )
    .await;
    assert!(
        !after_passing.package_completed,
        "the handout is still unopened"
    );
    assert_eq!(
        after_passing.attempt.status,
        PackageAttemptStatus::InProgress
    );

    initialise(&learner.db, &learner.key, &attempt.id, "handout")
        .await
        .expect("open the handout");

    let progress = learner
        .element_progress("el_pkg")
        .expect("element progress");
    assert_eq!(progress.status, "completed");
    let score = progress.score.expect("a score");
    assert!(
        (score - 0.75).abs() < 1e-9,
        "mean of 0.6 and 0.9, got {score}"
    );
    assert_eq!(progress.time_spent, 180, "three one-minute sessions");

    let reopened = open(&learner.db, ENROLLMENT, "el_pkg").await.expect("open");
    assert_eq!(
        reopened.id, attempt.id,
        "a completed attempt is resumed for review"
    );
    assert_eq!(reopened.status, PackageAttemptStatus::Completed);
    assert!(reopened.ended_at.is_some());
}

#[tokio::test]
async fn an_activity_that_is_not_a_lesson_of_the_package_is_refused() {
    let learner = Learner::new().await;
    learner.add_package("el_pkg", "com.example.induction").await;
    let attempt = open(&learner.db, ENROLLMENT, "el_pkg").await.expect("open");

    let error = initialise(&learner.db, &learner.key, &attempt.id, "org")
        .await
        .expect_err("the organisation is not launchable");
    assert!(matches!(error, AttemptError::NotFound(_)), "{error}");
}

#[tokio::test]
async fn only_a_package_element_of_the_enrolled_course_opens_an_attempt() {
    let learner = Learner::new().await;
    learner.add_package("el_pkg", "com.example.induction").await;
    learner
        .db
        .lock()
        .unwrap()
        .conn()
        .execute(
            "INSERT INTO course_elements (id, chapter_id, title, element_type, position) \
             VALUES ('el_video', 'ch1', 'Video', 'video', 1)",
            [],
        )
        .expect("video element");

    let video = open(&learner.db, ENROLLMENT, "el_video")
        .await
        .expect_err("a video");
    assert!(matches!(video, AttemptError::NotAPackage(_)), "{video}");
    let elsewhere = open(&learner.db, "enr-other", "el_pkg")
        .await
        .expect_err("another enrolment");
    assert!(
        matches!(elsewhere, AttemptError::NotFound(_)),
        "{elsewhere}"
    );
}

#[tokio::test]
async fn a_commit_that_is_not_run_time_data_or_is_too_large_is_refused_and_changes_nothing() {
    let learner = Learner::new().await;
    learner.add_package("el_pkg", "com.example.induction").await;
    let attempt = open(&learner.db, ENROLLMENT, "el_pkg").await.expect("open");

    let shapeless = PackageCommit {
        attempt_id: attempt.id.clone(),
        activity_id: "welcome".into(),
        session_ended: true,
        runtime: json!(["not", "run-time", "data"]),
    };
    let huge = scorm12_commit(
        &attempt.id,
        "welcome",
        json!({ "lesson_status": "completed", "lesson_location": "x".repeat(MAX_RUNTIME_BYTES) }),
        true,
    );
    for refused in [shapeless, huge] {
        let error = commit(&learner.db, refused).await.expect_err("refused");
        assert!(matches!(error, AttemptError::InvalidCommit(_)), "{error}");
    }

    let init = initialise(&learner.db, &learner.key, &attempt.id, "welcome")
        .await
        .expect("launch");
    assert_eq!(init.entry, "ab-initio", "nothing was stored");
}

#[tokio::test]
async fn a_fresh_attempt_starts_from_the_beginning_and_keeps_the_element_complete() {
    let learner = Learner::new().await;
    learner.add_package("el_pkg", "com.example.induction").await;
    let first = open(&learner.db, ENROLLMENT, "el_pkg").await.expect("open");
    finish_lesson(
        &learner,
        &first.id,
        "welcome",
        "completed",
        json!({ "raw": "100" }),
    )
    .await;
    finish_lesson(
        &learner,
        &first.id,
        "lifting",
        "passed",
        json!({ "raw": "100" }),
    )
    .await;
    let first_init = initialise(&learner.db, &learner.key, &first.id, "handout")
        .await
        .expect("open the handout");
    assert_eq!(
        learner.element_progress("el_pkg").expect("progress").status,
        "completed"
    );

    let second = fresh(&learner.db, ENROLLMENT, "el_pkg")
        .await
        .expect("fresh attempt");
    assert_eq!(second.attempt_number, 2);
    assert_eq!(second.status, PackageAttemptStatus::InProgress);
    assert_ne!(second.id, first.id);
    assert_eq!(
        open(&learner.db, ENROLLMENT, "el_pkg")
            .await
            .expect("open")
            .id,
        second.id,
        "opening the element now resumes the fresh attempt"
    );

    let init = initialise(&learner.db, &learner.key, &second.id, "welcome")
        .await
        .expect("launch in the fresh attempt");
    assert_eq!(init.entry, "ab-initio");
    assert_eq!(init.runtime["cmi"]["core"].get("lesson_status"), None);
    assert_eq!(
        init.runtime["cmi"]["core"]["total_time"],
        json!("0000:00:00")
    );
    assert_eq!(
        init.learner_id, first_init.learner_id,
        "stable across attempts"
    );

    // The replaced attempt takes no more state.
    let late = commit(
        &learner.db,
        scorm12_commit(
            &first.id,
            "welcome",
            json!({ "lesson_status": "failed" }),
            true,
        ),
    )
    .await
    .expect_err("a closed attempt refuses commits");
    assert!(matches!(late, AttemptError::Closed(_)), "{late}");

    // Starting over does not take back a completion; time keeps adding up.
    let after = commit(
        &learner.db,
        scorm12_commit(
            &second.id,
            "welcome",
            json!({ "lesson_status": "incomplete", "session_time": "0000:01:00" }),
            true,
        ),
    )
    .await
    .expect("commit to the fresh attempt");
    assert!(!after.package_completed);
    assert_eq!(after.element_progress.status, "completed");
    assert_eq!(after.element_progress.time_spent, 180);
}

#[tokio::test]
async fn the_learner_identifier_is_stable_per_package_and_different_across_packages() {
    let learner = Learner::new().await;
    learner.add_package("el_a", "com.example.induction").await;
    learner.add_package("el_b", "com.example.forklift").await;
    let a = open(&learner.db, ENROLLMENT, "el_a").await.expect("open a");
    let b = open(&learner.db, ENROLLMENT, "el_b").await.expect("open b");

    let id = |key: [u8; 32], attempt: String, activity: &'static str| {
        let db = &learner.db;
        async move {
            initialise(db, &key, &attempt, activity)
                .await
                .expect("initialise")
                .learner_id
        }
    };
    let a_welcome = id(learner.key, a.id.clone(), "welcome").await;
    let a_lifting = id(learner.key, a.id.clone(), "lifting").await;
    let b_welcome = id(learner.key, b.id.clone(), "welcome").await;
    let other_learner = id([9; 32], a.id.clone(), "welcome").await;

    assert_eq!(a_welcome, a_lifting, "one identifier for the whole package");
    assert_ne!(a_welcome, b_welcome, "unlinkable across packages");
    assert_ne!(a_welcome, other_learner, "derived from the learner's key");
    for identifier in [&a_welcome, &b_welcome] {
        assert!(!identifier.contains("did:"), "never the learner's DID");
        assert!(!identifier.contains("stake"), "never the learner's address");
    }
}

#[tokio::test]
async fn terminating_an_unfinished_attempt_keeps_the_extracted_files_and_counts_the_open_session() {
    let learner = Learner::new().await;
    let hash = learner.add_package("el_pkg", "com.example.induction").await;
    let extracted =
        packages::ensure_extracted(&learner.db, &learner.node, &learner.dirs, &hash, |_| {})
            .await
            .expect("extract");
    let attempt = open(&learner.db, ENROLLMENT, "el_pkg").await.expect("open");
    commit(
        &learner.db,
        scorm12_commit(
            &attempt.id,
            "welcome",
            json!({ "lesson_status": "incomplete", "session_time": "0000:02:00" }),
            false,
        ),
    )
    .await
    .expect("commit");

    let ended = terminate(&learner.db, &learner.dirs, &Launches::new(), &attempt.id)
        .await
        .expect("terminate");
    assert!(!ended.package_completed);
    assert!(!ended.cache_dropped);
    assert!(extracted.root.join("welcome/index.html").is_file());
    assert_eq!(ended.element_progress.time_spent, 120);

    // The session is over: the next launch adds nothing twice.
    let next = initialise(&learner.db, &learner.key, &attempt.id, "welcome")
        .await
        .expect("next launch");
    assert_eq!(
        next.runtime["cmi"]["core"]["total_time"],
        json!("0000:02:00")
    );
}

#[tokio::test]
async fn terminating_a_completed_attempt_drops_the_extracted_files_and_the_package_stays_launchable(
) {
    let learner = Learner::new().await;
    let hash = learner.add_package("el_pkg", "com.example.induction").await;
    let extracted =
        packages::ensure_extracted(&learner.db, &learner.node, &learner.dirs, &hash, |_| {})
            .await
            .expect("extract");
    let attempt = open(&learner.db, ENROLLMENT, "el_pkg").await.expect("open");
    finish_lesson(&learner, &attempt.id, "welcome", "completed", json!({})).await;
    finish_lesson(&learner, &attempt.id, "lifting", "passed", json!({})).await;
    initialise(&learner.db, &learner.key, &attempt.id, "handout")
        .await
        .expect("open the handout");
    assert!(
        extracted.root.is_dir(),
        "files stay while the launch is still open"
    );

    let ended = terminate(&learner.db, &learner.dirs, &Launches::new(), &attempt.id)
        .await
        .expect("terminate");
    assert!(ended.package_completed);
    assert!(ended.cache_dropped);
    assert_eq!(ended.attempt.status, PackageAttemptStatus::Completed);
    assert!(!extracted.root.exists(), "the extracted files are gone");

    let rebuilt =
        packages::ensure_extracted(&learner.db, &learner.node, &learner.dirs, &hash, |_| {})
            .await
            .expect("the archive is intact and extracts again");
    assert!(rebuilt.root.join("lifting/index.html").is_file());
}

#[tokio::test]
async fn completing_a_package_while_a_launch_of_it_is_open_keeps_the_files_until_it_closes() {
    let learner = Learner::new().await;
    let hash = learner.add_package("el_pkg", "com.example.induction").await;
    let extracted =
        packages::ensure_extracted(&learner.db, &learner.node, &learner.dirs, &hash, |_| {})
            .await
            .expect("extract");
    let launches = Launches::new();
    let launch = launches
        .open(&hash, &extracted.root)
        .expect("a launch serves the files");
    let attempt = open(&learner.db, ENROLLMENT, "el_pkg").await.expect("open");
    finish_lesson(&learner, &attempt.id, "welcome", "completed", json!({})).await;
    finish_lesson(&learner, &attempt.id, "lifting", "passed", json!({})).await;
    initialise(&learner.db, &learner.key, &attempt.id, "handout")
        .await
        .expect("open the handout");

    let ended = terminate(&learner.db, &learner.dirs, &launches, &attempt.id)
        .await
        .expect("terminate");
    assert!(ended.package_completed);
    assert!(!ended.cache_dropped, "an open launch still reads the files");
    assert!(extracted.root.join("welcome/index.html").is_file());

    assert!(launches.close(&launch.prefix));
    let ended = terminate(&learner.db, &learner.dirs, &launches, &attempt.id)
        .await
        .expect("terminate after the launch closed");
    assert!(ended.cache_dropped);
    assert!(!extracted.root.exists(), "the extracted files are gone");
}

#[tokio::test]
async fn the_table_of_contents_lists_every_lesson_in_order_with_what_it_reported() {
    let learner = Learner::new().await;
    learner.add_package("el_pkg", "com.example.induction").await;
    let attempt = open(&learner.db, ENROLLMENT, "el_pkg").await.expect("open");

    let untouched = activities(&learner.db, &attempt.id)
        .await
        .expect("results of a new attempt");
    assert_eq!(
        untouched
            .iter()
            .map(|result| (result.activity_id.as_str(), result.started, result.complete))
            .collect::<Vec<_>>(),
        [
            ("welcome", false, false),
            ("lifting", false, false),
            ("handout", false, false)
        ]
    );

    // Welcome left part-way, lifting passed at 90.
    initialise(&learner.db, &learner.key, &attempt.id, "welcome")
        .await
        .expect("launch welcome");
    commit(
        &learner.db,
        scorm12_commit(
            &attempt.id,
            "welcome",
            json!({ "lesson_status": "incomplete", "exit": "suspend" }),
            true,
        ),
    )
    .await
    .expect("commit welcome");
    finish_lesson(
        &learner,
        &attempt.id,
        "lifting",
        "passed",
        json!({ "raw": "90" }),
    )
    .await;

    let results = activities(&learner.db, &attempt.id).await.expect("results");
    let welcome = &results[0];
    assert_eq!(welcome.activity_id, "welcome");
    assert!(welcome.started && !welcome.complete);
    assert_eq!(welcome.score, None);
    let lifting = &results[1];
    assert!(lifting.started && lifting.complete);
    assert_eq!(lifting.score, Some(0.9));
    assert!(!results[2].started, "the handout was never opened");
}

#[tokio::test]
async fn launching_shown_only_content_hands_the_player_the_progress_it_completed() {
    let learner = Learner::new().await;
    learner.add_package("el_pkg", "com.example.induction").await;
    let attempt = open(&learner.db, ENROLLMENT, "el_pkg").await.expect("open");

    let lesson = initialise(&learner.db, &learner.key, &attempt.id, "welcome")
        .await
        .expect("launch a lesson");
    assert!(
        lesson.progress.is_none(),
        "a lesson reports through its commits"
    );

    let handout = initialise(&learner.db, &learner.key, &attempt.id, "handout")
        .await
        .expect("open the handout");
    let progress = handout.progress.expect("the handout's launch completed it");
    assert!(!progress.package_completed, "two lessons are still open");
    assert_eq!(progress.element_progress.element_id, "el_pkg");
    assert_eq!(progress.element_progress.status, "in_progress");
}

#[tokio::test]
async fn the_table_of_contents_of_an_unknown_attempt_is_refused() {
    let learner = Learner::new().await;
    learner.add_package("el_pkg", "com.example.induction").await;

    let error = activities(&learner.db, "no-such-attempt")
        .await
        .expect_err("unknown attempt");
    assert!(error.to_string().starts_with("not_found:"), "{error}");
}

#[path = "scorm2004_tests.rs"]
mod scorm2004;
