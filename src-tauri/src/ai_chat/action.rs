//! Allowing or declining what a chat proposed.
//!
//! A model cannot write the learner's data. A proposal it made (see
//! [`super::artifact`]) is carried out only when the learner allows it in
//! the chat, and only here, through the app's own services:
//!
//! | Proposal      | Allowing it                                   |
//! |---------------|-----------------------------------------------|
//! | `note`, `flashcards` | saves it with the learner's chat notes |
//! | `goal`        | adds a goal with the skills the learner picked |
//! | `goal_change` | renames the goal, adds or removes skills      |
//! | `course_note` | adds it to the lesson's course notes          |
//! | `enroll`      | enrolls the learner in the course             |
//!
//! Everything is checked again at that moment, inside one transaction: no
//! assessment is in progress, the proposal is still waiting, its text does
//! not resemble a stored assessment item, and the target still exists.
//! Allowing twice does nothing more. A declined proposal stays marked as
//! declined and can no longer be allowed. Nothing here issues credentials or
//! changes progress.

use super::artifact::Body;
use super::store::{self, Artifact};
use super::{bank_guard, goal};
use crate::db::Database;

/// What allowing a proposal did.
pub struct Allowed {
    pub artifact: Artifact,
    /// The learner's goals changed, so open views should refresh them.
    pub goals_changed: bool,
}

/// Carries out a proposal the learner allowed. A new goal needs the skills
/// the learner picked for it.
pub fn allow(
    db: &Database,
    artifact_id: &str,
    skill_ids: Option<&[String]>,
    now: &str,
) -> Result<Allowed, String> {
    let conn = db.conn();
    alexandria_studio::gate::require_no_assessment(conn).map_err(|e| e.to_string())?;
    let item = store::artifact(conn, artifact_id).map_err(|e| e.to_string())?;
    match item.status.as_str() {
        "proposed" => {}
        "declined" => return Err("invalid_input: this was not allowed".into()),
        _ => {
            return Ok(Allowed {
                artifact: item,
                goals_changed: false,
            })
        }
    }
    if bank_guard::matches_assessment_item(conn, &format!("{} {}", item.title, item.body.text())) {
        return Err("assessment_item: this cannot hold assessment questions".into());
    }

    let tx = conn
        .unchecked_transaction()
        .map_err(|_| "storage_error: could not start".to_string())?;
    let mut goals_changed = false;
    let artifact = match &item.body {
        Body::Note { .. } | Body::Flashcards { .. } => {
            store::save_artifact(&tx, artifact_id, now).map_err(|e| e.to_string())?
        }
        Body::Goal { .. } => {
            let skills = skill_ids.unwrap_or_default();
            let goal_id = goal::add(&tx, &item.title, skills, now)?;
            goals_changed = true;
            store::mark_goal_added(&tx, artifact_id, &goal_id, now).map_err(|e| e.to_string())?
        }
        Body::GoalChange { .. } => {
            goal::apply_change(&tx, &item.body)?;
            goals_changed = true;
            store::mark_added(&tx, artifact_id, now).map_err(|e| e.to_string())?
        }
        Body::CourseNote {
            course_id,
            element_id,
            text,
            ..
        } => {
            crate::commands::course_notes::insert(&tx, course_id, element_id, &item.title, text)?;
            store::mark_added(&tx, artifact_id, now).map_err(|e| e.to_string())?
        }
        Body::Enroll { course_id, .. } => {
            enroll(db, course_id)?;
            store::mark_added(&tx, artifact_id, now).map_err(|e| e.to_string())?
        }
    };
    tx.commit()
        .map_err(|_| "storage_error: could not be saved".to_string())?;
    log::info!("chat: the learner allowed a {} proposal", item.body.kind());
    Ok(Allowed {
        artifact,
        goals_changed,
    })
}

/// Enrolls through the course service. A course that needs plugins the
/// learner has not installed is left to the course page, which asks before
/// installing anything. Already taking the course counts as done.
fn enroll(db: &Database, course_id: &str) -> Result<(), String> {
    let taking: bool = db
        .conn()
        .query_row(
            "SELECT count(*) > 0 FROM enrollments WHERE course_id = ?1 AND status = 'active'",
            [course_id],
            |r| r.get(0),
        )
        .map_err(|_| "storage_error: enrollments could not be read".to_string())?;
    if taking {
        return Ok(());
    }
    for cid in crate::commands::plugins::course_plugin_cids(db, course_id)? {
        if crate::plugins::registry::get_installed(db, &cid)?.is_none() {
            return Err("needs_plugins: open the course to install what it needs".into());
        }
    }
    crate::commands::enrollment::enroll_db(db, course_id.to_string())
        .map(|_| ())
        .map_err(|e| format!("open_course: {e}"))
}

/// Records that the learner declined a proposal.
pub fn decline(db: &Database, artifact_id: &str, now: &str) -> Result<Artifact, String> {
    store::decline_artifact(db.conn(), artifact_id, now).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_chat::artifact::SkillRef;
    use rusqlite::params;
    use serde_json::Value;

    const NOW: &str = "2026-10-24T12:00:00Z";

    fn db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db.conn()
            .execute_batch(
                r#"INSERT INTO subject_fields(id, name) VALUES ('f', 'Mathematics');
                   INSERT INTO subjects(id, name, subject_field_id) VALUES ('s', 'Probability', 'f');
                   INSERT INTO skills(id, name, subject_id, bloom_level) VALUES
                     ('sk-counting', 'Counting outcomes', 's', 'apply'),
                     ('sk-bayes', 'Bayes rule', 's', 'analyze');
                   INSERT INTO app_settings(key, value) VALUES ('learner.targets',
                     '[{"id":"g1","label":"Data analyst","goal_skill_ids":["sk-bayes"],"created_at":"2026-10-01","kind":"learning_goal"}]');
                   INSERT INTO courses(id, title, author_address, status)
                     VALUES ('course', 'Probability', 'author', 'published');
                   INSERT INTO course_chapters(id, course_id, title, position)
                     VALUES ('ch', 'course', 'Chapter 2', 0);
                   INSERT INTO course_elements(id, chapter_id, title, element_type, content_inline, position)
                     VALUES ('lesson', 'ch', 'Conditional probability', 'text', 'P(A|B)', 0);"#,
            )
            .unwrap();
        db
    }

    fn propose(db: &Database, id: &str, title: &str, body: &Body) {
        db.conn()
            .execute(
                "INSERT INTO chat_artifacts (id, kind, status, title, body_json, created_at, updated_at)
                 VALUES (?1, ?2, 'proposed', ?3, ?4, ?5, ?5)",
                params![id, body.kind(), title, serde_json::to_string(body).unwrap(), NOW],
            )
            .unwrap();
    }

    fn goals(db: &Database) -> Vec<Value> {
        let raw: String = db
            .conn()
            .query_row(
                "SELECT value FROM app_settings WHERE key = 'learner.targets'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    fn status(db: &Database, id: &str) -> String {
        store::artifact(db.conn(), id).unwrap().status
    }

    fn skill(id: &str, name: &str) -> SkillRef {
        SkillRef {
            id: id.into(),
            name: name.into(),
        }
    }

    #[test]
    fn a_goal_is_added_with_the_skills_the_learner_picked_and_only_once() {
        let db = db();
        let body = Body::Goal {
            text: "Count outcomes".into(),
            goal_id: None,
        };
        propose(&db, "a", "Counting", &body);
        assert!(allow(&db, "a", None, NOW).is_err(), "a goal needs skills");
        assert!(allow(&db, "a", Some(&["sk-astrology".into()]), NOW).is_err());
        assert_eq!(status(&db, "a"), "proposed");
        assert_eq!(goals(&db).len(), 1, "nothing written");

        let picked = ["sk-counting".to_string(), "sk-counting".to_string()];
        let allowed = allow(&db, "a", Some(&picked), NOW).unwrap();
        assert!(allowed.goals_changed);
        assert_eq!(allowed.artifact.status, "added");
        let saved = goals(&db);
        assert_eq!(saved.len(), 2);
        assert_eq!(
            saved[0]["kind"], "learning_goal",
            "other goals keep their fields"
        );
        assert_eq!(saved[1]["label"], "Counting");
        assert_eq!(
            saved[1]["goal_skill_ids"],
            serde_json::json!(["sk-counting"])
        );
        let Body::Goal { goal_id, .. } = allowed.artifact.body else {
            panic!("goal")
        };
        assert_eq!(goal_id.as_deref(), saved[1]["id"].as_str());

        let again = allow(&db, "a", Some(&picked), NOW).unwrap();
        assert!(!again.goals_changed, "allowing twice does nothing more");
        assert_eq!(goals(&db).len(), 2);

        propose(&db, "b", "Same skills", &body);
        let same = allow(&db, "b", Some(&["sk-bayes".into()]), NOW).unwrap();
        let Body::Goal { goal_id, .. } = same.artifact.body else {
            panic!("goal")
        };
        assert_eq!(goal_id.as_deref(), Some("g1"), "an existing goal is reused");
        assert_eq!(goals(&db).len(), 2);
    }

    #[test]
    fn a_goal_change_is_applied_to_the_goal_as_it_is_now() {
        let db = db();
        let empty = Body::GoalChange {
            goal_id: "g1".into(),
            rename: None,
            add: vec![],
            remove: vec![skill("sk-bayes", "Bayes rule")],
            text: "why".into(),
        };
        propose(&db, "empty", "Data analyst", &empty);
        assert!(allow(&db, "empty", None, NOW).is_err(), "never left empty");
        assert_eq!(status(&db, "empty"), "proposed");

        let swap = Body::GoalChange {
            goal_id: "g1".into(),
            rename: Some("Counting first".into()),
            add: vec![skill("sk-counting", "Counting outcomes")],
            remove: vec![skill("sk-bayes", "Bayes rule")],
            text: "why".into(),
        };
        propose(&db, "swap", "Data analyst", &swap);
        let allowed = allow(&db, "swap", None, NOW).unwrap();
        assert!(allowed.goals_changed);
        let saved = goals(&db);
        assert_eq!(saved[0]["label"], "Counting first");
        assert_eq!(
            saved[0]["goal_skill_ids"],
            serde_json::json!(["sk-counting"])
        );
        assert_eq!(saved[0]["kind"], "learning_goal");

        db.conn()
            .execute(
                "UPDATE app_settings SET value = '[]' WHERE key = 'learner.targets'",
                [],
            )
            .unwrap();
        propose(&db, "gone", "Data analyst", &swap);
        let err = allow(&db, "gone", None, NOW).err().unwrap();
        assert!(err.starts_with("not_found"), "{err}");
    }

    #[test]
    fn a_course_note_needs_the_course_and_lands_at_its_lesson_once() {
        let db = db();
        let body = Body::CourseNote {
            course_id: "course".into(),
            element_id: "lesson".into(),
            lesson: "Conditional probability".into(),
            text: "Bayes: update a prior with evidence.".into(),
        };
        propose(&db, "n", "Bayes rule", &body);
        let err = allow(&db, "n", None, NOW).err().unwrap();
        assert!(err.starts_with("not_enrolled"), "{err}");
        assert_eq!(status(&db, "n"), "proposed");

        db.conn()
            .execute_batch("INSERT INTO enrollments(id, course_id) VALUES ('en', 'course');")
            .unwrap();
        assert_eq!(allow(&db, "n", None, NOW).unwrap().artifact.status, "added");
        propose(&db, "n2", "Bayes rule", &body);
        allow(&db, "n2", None, NOW).unwrap();
        let notes = crate::commands::course_notes::list(db.conn(), "course").unwrap();
        assert_eq!(notes.len(), 1, "the same note is kept once");
        assert_eq!(notes[0].element_id.as_deref(), Some("lesson"));
        assert_eq!(notes[0].origin.as_deref(), Some("chat"));
        assert_eq!(notes[0].title.as_deref(), Some("Bayes rule"));
    }

    #[test]
    fn enrolling_goes_through_the_course_service() {
        let db = db();
        let body = Body::Enroll {
            course_id: "course".into(),
            goal_id: "g1".into(),
            text: "Covers counting.".into(),
        };
        propose(&db, "e", "Probability", &body);
        let err = allow(&db, "e", None, NOW).err().unwrap();
        assert!(err.starts_with("open_course"), "{err}");
        assert_eq!(status(&db, "e"), "proposed");

        db.conn()
            .execute_batch("INSERT INTO enrollments(id, course_id) VALUES ('en', 'course');")
            .unwrap();
        assert_eq!(allow(&db, "e", None, NOW).unwrap().artifact.status, "added");
    }

    #[test]
    fn a_declined_proposal_or_an_assessment_writes_nothing() {
        let db = db();
        let note = Body::Note {
            text: "Prior: belief before evidence".into(),
        };
        propose(&db, "d", "Terms", &note);
        assert_eq!(decline(&db, "d", NOW).unwrap().status, "declined");
        assert!(allow(&db, "d", None, NOW).is_err());
        assert!(store::saved_artifacts(db.conn()).unwrap().is_empty());

        propose(&db, "p", "Terms", &note);
        db.conn()
            .execute("INSERT INTO integrity_sessions(id) VALUES ('attempt')", [])
            .unwrap();
        let err = allow(&db, "p", None, NOW).err().unwrap();
        assert!(err.contains("assessment"), "{err}");
        assert_eq!(status(&db, "p"), "proposed");
    }
}
