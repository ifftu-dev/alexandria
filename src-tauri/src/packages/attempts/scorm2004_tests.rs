//! Host-seam tests for SCORM 2004 packages: what a launch is handed, what
//! the host keeps from a lesson's commits, the sequencing engine's state
//! stored per attempt, and the rollup into element progress, which follows
//! the root activity's completion status.

use alexandria_studio::package::progress::RootStatus;
use serde_json::json;

use super::{fixture, Learner, ENROLLMENT};
use crate::domain::package_attempt::{PackageAttemptStatus, PackageCommit, PackageSequencingState};
use crate::packages::attempts::*;

/// A SCORM 2004 4th Edition package: a lesson that hands the content launch
/// data and a time limit, and a quiz passed at a scaled score of 0.8.
const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest identifier="com.example.fire" version="1"
    xmlns="http://www.imsglobal.org/xsd/imscp_v1p1"
    xmlns:adlcp="http://www.adlnet.org/xsd/adlcp_v1p3"
    xmlns:adlseq="http://www.adlnet.org/xsd/adlseq_v1p3"
    xmlns:adlnav="http://www.adlnet.org/xsd/adlnav_v1p3"
    xmlns:imsss="http://www.imsglobal.org/xsd/imsss">
  <metadata>
    <schema>ADL SCORM</schema>
    <schemaversion>2004 4th Edition</schemaversion>
  </metadata>
  <organizations default="course">
    <organization identifier="course">
      <title>Fire safety</title>
      <item identifier="intro" identifierref="r-intro">
        <title>Introduction</title>
        <adlcp:dataFromLMS>route=b</adlcp:dataFromLMS>
        <adlcp:timeLimitAction>exit,message</adlcp:timeLimitAction>
        <imsss:sequencing>
          <imsss:limitConditions attemptAbsoluteDurationLimit="PT30M"/>
        </imsss:sequencing>
      </item>
      <item identifier="quiz" identifierref="r-quiz">
        <title>Check</title>
        <imsss:sequencing>
          <imsss:objectives>
            <imsss:primaryObjective objectiveID="quiz-pass" satisfiedByMeasure="true">
              <imsss:minNormalizedMeasure>0.8</imsss:minNormalizedMeasure>
            </imsss:primaryObjective>
          </imsss:objectives>
        </imsss:sequencing>
      </item>
      <imsss:sequencing>
        <imsss:controlMode choice="true" flow="true"/>
      </imsss:sequencing>
    </organization>
  </organizations>
  <resources>
    <resource identifier="r-intro" type="webcontent" adlcp:scormType="sco" href="intro.html"/>
    <resource identifier="r-quiz" type="webcontent" adlcp:scormType="sco" href="quiz.html"/>
  </resources>
</manifest>"#;

fn package_bytes() -> Vec<u8> {
    fixture::zip(&[
        ("imsmanifest.xml", MANIFEST.as_bytes()),
        ("intro.html", b"<html><body>Intro</body></html>"),
        ("quiz.html", b"<html><body>Quiz</body></html>"),
    ])
}

async fn learner_with_package() -> (Learner, String) {
    let learner = Learner::new().await;
    learner.add_package_bytes("el_fire", &package_bytes()).await;
    let attempt = open(&learner.db, ENROLLMENT, "el_fire")
        .await
        .expect("open");
    (learner, attempt.id)
}

#[tokio::test]
async fn a_first_scorm2004_launch_gets_the_host_values_in_the_2004_data_model() {
    let (learner, attempt) = learner_with_package().await;

    let init = initialise(&learner.db, &learner.key, &attempt, "intro")
        .await
        .expect("initialise");

    assert_eq!(init.entry, "ab-initio");
    assert_eq!(init.launch_data.as_deref(), Some("route=b"));
    let cmi = &init.runtime["cmi"];
    assert_eq!(cmi["learner_id"], json!(init.learner_id));
    assert_eq!(cmi["learner_name"], json!("Ada Learner"));
    assert_eq!(cmi["entry"], json!("ab-initio"));
    assert_eq!(cmi["credit"], json!("credit"));
    assert_eq!(cmi["mode"], json!("normal"));
    assert_eq!(cmi["launch_data"], json!("route=b"));
    assert_eq!(cmi["max_time_allowed"], json!("PT30M"));
    assert_eq!(cmi["time_limit_action"], json!("exit,message"));
    assert_eq!(cmi.get("core"), None, "no SCORM 1.2 names");

    let quiz = initialise(&learner.db, &learner.key, &attempt, "quiz")
        .await
        .expect("initialise quiz");
    assert_eq!(quiz.runtime["cmi"]["scaled_passing_score"], json!("0.8"));
}

/// A SCORM 2004 commit payload as the library renders it.
fn scorm2004_commit(
    attempt_id: &str,
    activity_id: &str,
    cmi: serde_json::Value,
    session_ended: bool,
) -> PackageCommit {
    PackageCommit {
        attempt_id: attempt_id.to_string(),
        activity_id: activity_id.to_string(),
        session_ended,
        runtime: json!({ "cmi": cmi }),
    }
}

/// What the wrapper hands the host after the engine has navigated.
fn engine_state(attempt_id: &str, snapshot: &str, root_completion: &str) -> PackageSequencingState {
    PackageSequencingState {
        attempt_id: attempt_id.to_string(),
        snapshot: snapshot.to_string(),
        root: RootStatus {
            completion_status: root_completion.to_string(),
        },
    }
}

#[tokio::test]
async fn the_element_completes_when_the_root_activity_does_not_when_each_lesson_reports_completed()
{
    let (learner, attempt) = learner_with_package().await;
    for (lesson, scaled, time) in [("intro", "0.9", "PT10M"), ("quiz", "0.5", "PT5M30S")] {
        initialise(&learner.db, &learner.key, &attempt, lesson)
            .await
            .expect("launch");
        let progress = commit(
            &learner.db,
            scorm2004_commit(
                &attempt,
                lesson,
                json!({ "completion_status": "completed", "score": { "scaled": scaled }, "session_time": time }),
                true,
            ),
        )
        .await
        .expect("commit");
        assert!(
            !progress.package_completed,
            "{lesson} alone does not complete the package"
        );
    }
    let lessons = activities(&learner.db, &attempt).await.expect("activities");
    assert!(lessons.iter().all(|lesson| lesson.complete));
    assert_eq!(
        learner.element_progress("el_fire").unwrap().status,
        "in_progress"
    );

    // The engine's rollup of the root says not yet (a rule asks for more).
    let progress = save_sequencing(&learner.db, engine_state(&attempt, "s1", "incomplete"))
        .await
        .expect("save");
    assert!(!progress.package_completed);
    assert_eq!(progress.element_progress.status, "in_progress");

    let progress = save_sequencing(&learner.db, engine_state(&attempt, "s2", "completed"))
        .await
        .expect("save");
    assert!(progress.package_completed);
    assert_eq!(progress.attempt.status, PackageAttemptStatus::Completed);
    let element = progress.element_progress;
    assert_eq!(element.status, "completed");
    // Score and time as for SCORM 1.2: the mean of the lessons' scores and
    // the sum of their sessions.
    assert_eq!(element.score, Some(0.7));
    assert_eq!(element.time_spent, 930);
}

#[tokio::test]
async fn the_engine_state_is_kept_per_attempt_and_a_fresh_attempt_starts_without_it() {
    let (learner, attempt) = learner_with_package().await;
    assert_eq!(sequencing(&learner.db, &attempt).await.expect("read"), None);

    save_sequencing(&learner.db, engine_state(&attempt, "first", "incomplete"))
        .await
        .expect("save");
    save_sequencing(&learner.db, engine_state(&attempt, "second", "incomplete"))
        .await
        .expect("save again");
    assert_eq!(
        sequencing(&learner.db, &attempt)
            .await
            .expect("read")
            .as_deref(),
        Some("second")
    );

    let next = fresh(&learner.db, ENROLLMENT, "el_fire")
        .await
        .expect("fresh");
    assert_eq!(sequencing(&learner.db, &next.id).await.expect("read"), None);
    let refused = save_sequencing(&learner.db, engine_state(&attempt, "late", "completed"))
        .await
        .expect_err("a closed attempt takes no more state");
    assert!(
        refused.to_string().starts_with("attempt_closed:"),
        "{refused}"
    );
    assert_eq!(
        sequencing(&learner.db, &attempt)
            .await
            .expect("read")
            .as_deref(),
        Some("second")
    );
}

#[tokio::test]
async fn a_lesson_that_did_not_suspend_starts_a_new_attempt_unless_the_engine_resumes_it() {
    let (learner, attempt) = learner_with_package().await;
    initialise(&learner.db, &learner.key, &attempt, "intro")
        .await
        .expect("launch");
    let left = json!({ "location": "page-3", "suspend_data": "1,2,3", "exit": "", "session_time": "PT4M" });
    commit(&learner.db, scorm2004_commit(&attempt, "intro", left, true))
        .await
        .expect("commit");

    // Delivered again by a choice: a new attempt on the lesson.
    let again = initialise(&learner.db, &learner.key, &attempt, "intro")
        .await
        .expect("relaunch");
    assert_eq!(again.entry, "ab-initio");
    assert_eq!(again.runtime["cmi"].get("location"), None);
    assert_eq!(again.runtime["cmi"]["total_time"], json!("PT0S"));

    // Delivered by resume all after the player suspended the package: the
    // lesson picks up where it was.
    let resumed = initialise_delivery(&learner.db, &learner.key, &attempt, "intro", true)
        .await
        .expect("resume");
    assert_eq!(resumed.entry, "resume");
    let cmi = &resumed.runtime["cmi"];
    assert_eq!(cmi["entry"], json!("resume"));
    assert_eq!(cmi["location"], json!("page-3"));
    assert_eq!(cmi["suspend_data"], json!("1,2,3"));
    assert_eq!(cmi["total_time"], json!("PT4M"));
    assert_eq!(cmi.get("session_time"), None);
    assert_eq!(cmi.get("exit"), None);

    // A lesson that exits with suspend resumes on any delivery.
    let suspended = json!({ "location": "page-5", "exit": "suspend", "session_time": "PT1M" });
    commit(
        &learner.db,
        scorm2004_commit(&attempt, "intro", suspended, true),
    )
    .await
    .expect("commit");
    let back = initialise(&learner.db, &learner.key, &attempt, "intro")
        .await
        .expect("relaunch");
    assert_eq!(back.entry, "resume");
    assert_eq!(back.runtime["cmi"]["location"], json!("page-5"));
}
