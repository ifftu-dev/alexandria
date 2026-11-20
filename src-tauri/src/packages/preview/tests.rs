//! Preview at the host seam: an author previews a package element of their
//! own draft course, through the same lifecycle the player drives for an
//! attempt, and nothing is stored.

use std::sync::Mutex;

use alexandria_studio::package::progress;
use serde_json::json;
use tempfile::TempDir;

use super::*;
use crate::db::Database;
use crate::packages::tests::induction_package;
use crate::packages::{self, PackageDirs};

const KEY: [u8; 32] = [5u8; 32];

struct Author {
    db: Mutex<Database>,
    previews: Previews,
    hash: String,
    _root: TempDir,
}

impl Author {
    /// An author with a draft course whose one element is the induction
    /// package, and a foreign course holding the same package.
    async fn new() -> Self {
        let root = TempDir::new().expect("temp dir");
        let dirs = PackageDirs {
            archives: root.path().join("packages"),
            cache: root.path().join("packagecache"),
        };
        dirs.ensure().expect("package dirs");
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db.conn()
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address, display_name) \
                 VALUES (1, 'stake_test1uauthor', 'addr_test1q123', 'Ada Author')",
                [],
            )
            .expect("identity");
        let db = Mutex::new(db);
        let source = root.path().join("induction.zip");
        std::fs::write(&source, induction_package()).expect("write package");
        let package = packages::import(&db, &dirs, &source).await.expect("import");
        {
            let db = db.lock().unwrap();
            for (course, author, element) in [
                ("course-own", "stake_test1uauthor", "element-own"),
                ("course-foreign", "stake_test1uother", "element-foreign"),
            ] {
                db.conn()
                    .execute(
                        "INSERT INTO courses (id, title, author_address) VALUES (?1, 'Induction', ?2)",
                        [course, author],
                    )
                    .unwrap();
                let chapter = format!("{course}-chapter");
                db.conn()
                    .execute(
                        "INSERT INTO course_chapters (id, course_id, title, position) VALUES (?1, ?2, 'Start', 0)",
                        [chapter.as_str(), course],
                    )
                    .unwrap();
                db.conn()
                    .execute(
                        "INSERT INTO course_elements (id, chapter_id, title, element_type, content_cid, position) \
                         VALUES (?1, ?2, 'Induction', 'package', ?3, 0)",
                        [element, chapter.as_str(), package.hash.as_str()],
                    )
                    .unwrap();
            }
        }
        Self {
            db,
            previews: Previews::new(),
            hash: package.hash,
            _root: root,
        }
    }

    fn count(&self, table: &str) -> i64 {
        self.db
            .lock()
            .unwrap()
            .conn()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    fn commit(
        &self,
        attempt: &str,
        activity: &str,
        ended: bool,
        core: serde_json::Value,
    ) -> PackageAttemptProgress {
        commit(
            &self.previews,
            PackageCommit {
                attempt_id: attempt.to_string(),
                activity_id: activity.to_string(),
                session_ended: ended,
                runtime: json!({ "cmi": { "core": core } }),
            },
        )
        .expect("commit")
    }
}

#[tokio::test]
async fn a_preview_runs_the_package_as_a_learner_would_and_stores_nothing() {
    let author = Author::new().await;

    let attempt = open(&author.db, &author.previews, "element-own")
        .await
        .expect("preview opens");
    assert_eq!(attempt.package_hash, author.hash);

    let init = initialise(&author.db, &author.previews, &KEY, &attempt.id, "welcome")
        .await
        .expect("initialise");
    assert_eq!(init.entry, "ab-initio");
    assert_eq!(init.learner_id, progress::learner_id(&KEY, &author.hash));
    assert_eq!(init.learner_name, "Ada Author");
    assert_eq!(init.runtime["cmi"]["core"]["student_id"], init.learner_id);

    // The first lesson is suspended part-way, then resumed with its state.
    let saved = author.commit(
        &attempt.id,
        "welcome",
        true,
        json!({ "lesson_status": "incomplete", "lesson_location": "page-2", "exit": "suspend" }),
    );
    assert!(!saved.package_completed);
    assert_eq!(saved.element_progress.status, "in_progress");
    let resumed = initialise(&author.db, &author.previews, &KEY, &attempt.id, "welcome")
        .await
        .expect("initialise again");
    assert_eq!(resumed.entry, "resume");
    assert_eq!(resumed.runtime["cmi"]["core"]["lesson_location"], "page-2");

    author.commit(
        &attempt.id,
        "welcome",
        true,
        json!({ "lesson_status": "passed", "score": { "raw": "80" } }),
    );
    let finished = author.commit(
        &attempt.id,
        "lifting",
        true,
        json!({ "lesson_status": "completed" }),
    );
    assert!(finished.package_completed);
    assert_eq!(finished.element_progress.status, "completed");
    let results = activities(&author.previews, &attempt.id).expect("activities");
    assert!(results.iter().all(|result| result.complete), "{results:?}");

    let ended = terminate(&author.previews, &attempt.id).expect("terminate");
    assert!(ended.package_completed);
    assert!(
        activities(&author.previews, &attempt.id).is_err(),
        "a terminated preview is gone"
    );

    // Nothing reached the learner's records: no attempt, no enrolment, no
    // element progress, no learning record.
    for table in [
        "package_attempts",
        "package_activity_state",
        "enrollments",
        "element_progress",
        "learning_records",
    ] {
        assert_eq!(author.count(table), 0, "{table}");
    }
}

#[tokio::test]
async fn only_the_course_author_can_preview_a_package_element() {
    let author = Author::new().await;

    let error = open(&author.db, &author.previews, "element-foreign")
        .await
        .expect_err("refused");

    assert!(error.to_string().starts_with("not_found"), "{error}");
}

#[tokio::test]
async fn a_fresh_preview_starts_with_no_state() {
    let author = Author::new().await;
    let first = open(&author.db, &author.previews, "element-own")
        .await
        .unwrap();
    author.commit(
        &first.id,
        "welcome",
        true,
        json!({ "lesson_status": "completed" }),
    );

    let second = fresh(&author.db, &author.previews, "element-own")
        .await
        .expect("fresh");

    assert_ne!(second.id, first.id);
    assert_eq!(second.attempt_number, 2);
    let init = initialise(&author.db, &author.previews, &KEY, &second.id, "welcome")
        .await
        .unwrap();
    assert_eq!(init.entry, "ab-initio");
    assert!(
        activities(&author.previews, &first.id).is_err(),
        "the old preview is dropped"
    );
}
