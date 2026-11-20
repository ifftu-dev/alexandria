//! SCORM run-time data to learning records, following the xAPI SCORM
//! profile (`https://w3id.org/xapi/scorm`). Expected values are written by
//! hand from the SCORM data model and the profile, and the SCORM 1.2 pass is
//! checked against the canonical example statement r05.

use std::collections::BTreeMap;
use std::path::Path;

use alexandria_studio::xapi::scorm::{events, flatten_cmi, ScormEdition, ScormSession};
use alexandria_studio::xapi::{
    build_statement, Account, Action, ActivityRef, BuildOptions, IdKey, InteractionType,
    LearningEvent, Outcome, Profile, Recipient, RecordKind, Score,
};
use alexandria_verify::did::derive_did_key;
use alexandria_verify::hash::sha256;
use ed25519_dalek::SigningKey;
use serde_json::{json, Value};

const CREATOR: &str = "did:key:z6MkoRwQfNfmnSY6h11tXjnjt58Ta512cEk3QitVxRh3xU1g";
const COURSE: &str = "59fb4b553e7cf374392d9387f819a087afd1f135663d1a92473845def73c4a66";
const ELEMENT: &str = "b7159f93e1ce2ea232131615ea188b904138b52267893a3f8af92f1ea7a73c8c";
const PACKAGE: &str = "19ffd3ae8430b64d88265892dc4ab2f08bb6ff63010912ee50f76dcae620134a";

fn sco(id: &str) -> ActivityRef {
    ActivityRef::course(CREATOR, COURSE)
        .element(ELEMENT)
        .package_activity(PACKAGE, id)
}

fn session(edition: ScormEdition, local_id: &str) -> ScormSession {
    ScormSession {
        edition,
        activity: sco("item_1"),
        name: Some(BTreeMap::from([(
            "en-US".to_string(),
            "Package activity item_1".to_string(),
        )])),
        local_id: local_id.into(),
        registration: Some("2.0.0|attempt-1".into()),
        timestamp: "2026-11-19T10:00:00Z".parse().unwrap(),
        language: "en-US".into(),
    }
}

fn cmi(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn recipient_a() -> Recipient {
    let pairwise = SigningKey::from_bytes(&sha256(b"ls12:test:learner|recipient-a"));
    let account = Account::for_did(derive_did_key(&pairwise).as_str()).unwrap();
    let learner = sha256(b"ls12:test:learner");
    Recipient::new(account, IdKey::derive(&learner, "recipient-a").unwrap())
}

fn outcome_of(event: &LearningEvent) -> &Outcome {
    match &event.action {
        Action::Completed { outcome, .. }
        | Action::Passed { outcome, .. }
        | Action::Failed { outcome, .. }
        | Action::Scored { outcome, .. }
        | Action::Responded { outcome, .. } => outcome,
        other => panic!("no outcome on {other:?}"),
    }
}

#[test]
fn a_scorm_12_pass_becomes_the_canonical_passed_statement() {
    let local = "2.0.0|r05-passed-package-activity|b208e0b0-de05-4862-a55c-862598d3cb24";
    let data = cmi(&[
        ("cmi.core.lesson_status", "passed"),
        ("cmi.core.score.raw", "85"),
        ("cmi.core.score.min", "0"),
        ("cmi.core.score.max", "100"),
        ("cmi.core.session_time", "0000:08:00.00"),
    ]);
    let out = events(&session(ScormEdition::Scorm12, local), &data);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].kind(), RecordKind::Completion);

    let options = BuildOptions {
        language: Some("en-US".into()),
        ..BuildOptions::default()
    };
    let built =
        serde_json::to_value(build_statement(&out[0], &recipient_a(), &options).unwrap()).unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/xapi-canonical/2.0.0/r05-passed-package-activity.json");
    let mut expected: Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    expected["context"]["contextActivities"]["category"][0]["definition"] =
        json!({ "type": "http://adlnet.gov/expapi/activities/profile" });
    assert_eq!(built, expected);
}

#[test]
fn scorm_12_interactions_become_responses_in_the_xapi_format() {
    let data = cmi(&[
        ("cmi.core.lesson_status", "incomplete"),
        ("cmi.interactions.0.id", "q1"),
        ("cmi.interactions.0.type", "choice"),
        ("cmi.interactions.0.student_response", "a,c"),
        ("cmi.interactions.0.correct_responses.0.pattern", "{a,b}"),
        ("cmi.interactions.0.result", "wrong"),
        ("cmi.interactions.1.id", "q2"),
        ("cmi.interactions.1.type", "matching"),
        ("cmi.interactions.1.student_response", "1.a,2.b"),
        ("cmi.interactions.1.result", "correct"),
        ("cmi.interactions.2.id", "q3"),
        ("cmi.interactions.2.type", "true-false"),
        ("cmi.interactions.2.student_response", "t"),
        ("cmi.interactions.2.result", "neutral"),
    ]);
    let out = events(&session(ScormEdition::Scorm12, "attempt-1|item_1"), &data);
    assert_eq!(out.len(), 3, "an incomplete lesson reports no result");

    let expected = [
        ("q1", InteractionType::Choice, "a[,]c", Some(false)),
        ("q2", InteractionType::Matching, "1[.]a[,]2[.]b", Some(true)),
        ("q3", InteractionType::TrueFalse, "true", None),
    ];
    for (event, (id, kind, response, success)) in out.iter().zip(expected) {
        assert_eq!(event.kind(), RecordKind::Interaction);
        assert_eq!(event.profile, Some(Profile::Scorm));
        let Action::Responded {
            interaction,
            outcome,
        } = &event.action
        else {
            panic!("not a response: {event:?}");
        };
        assert_eq!(interaction.activity, sco("item_1").interaction(id));
        assert_eq!(interaction.interaction_type, kind);
        assert_eq!(outcome.response.as_deref(), Some(response));
        assert_eq!(outcome.success, success);
    }
    let Action::Responded { interaction, .. } = &out[0].action else {
        unreachable!()
    };
    assert_eq!(interaction.correct_responses, Some(vec!["a[,]b".into()]));
    // Every mapped record passes the builder's checks.
    for event in &out {
        build_statement(event, &recipient_a(), &BuildOptions::default()).unwrap();
    }
    // Each response keeps its own stable local id.
    assert_eq!(out[0].local_id, "attempt-1|item_1|interaction|0");
    assert_eq!(out[2].local_id, "attempt-1|item_1|interaction|2");
}

#[test]
fn scorm_2004_statuses_pick_the_verb_and_carry_score_and_time() {
    let run =
        |pairs: &[(&str, &str)]| events(&session(ScormEdition::Scorm2004, "a|1"), &cmi(pairs));

    let completed = run(&[
        ("cmi.completion_status", "completed"),
        ("cmi.success_status", "unknown"),
        ("cmi.score.scaled", "0.5"),
        ("cmi.session_time", "PT1H2M3.5S"),
    ]);
    assert_eq!(completed.len(), 1);
    assert!(matches!(completed[0].action, Action::Completed { .. }));
    assert_eq!(
        outcome_of(&completed[0]),
        &Outcome {
            score: Some(Score {
                scaled: Some(0.5),
                ..Score::default()
            }),
            duration_centiseconds: Some(372_350),
            ..Outcome::default()
        }
    );

    let failed = run(&[
        ("cmi.completion_status", "incomplete"),
        ("cmi.success_status", "failed"),
        ("cmi.score.raw", "3"),
        ("cmi.score.min", "0"),
        ("cmi.score.max", "10"),
    ]);
    assert!(matches!(failed[0].action, Action::Failed { .. }));
    assert_eq!(outcome_of(&failed[0]).completion, Some(false));
    assert_eq!(
        outcome_of(&failed[0]).score,
        Some(Score {
            scaled: None,
            raw: Some(3.0),
            min: Some(0.0),
            max: Some(10.0),
        })
    );

    let scored = run(&[
        ("cmi.completion_status", "incomplete"),
        ("cmi.score.scaled", "-0.25"),
    ]);
    assert!(matches!(scored[0].action, Action::Scored { .. }));

    assert!(run(&[("cmi.completion_status", "incomplete")]).is_empty());
    assert!(run(&[]).is_empty());
}

#[test]
fn values_content_got_wrong_are_left_out_rather_than_losing_the_record() {
    let out = events(
        &session(ScormEdition::Scorm2004, "a|1"),
        &cmi(&[
            ("cmi.completion_status", "completed"),
            ("cmi.score.scaled", "5"),
            ("cmi.session_time", "about ten minutes"),
            ("cmi.interactions.0.id", "q1"),
            ("cmi.interactions.0.type", "drag-and-drop"),
            ("cmi.interactions.1.id", ""),
            ("cmi.interactions.1.type", "choice"),
        ]),
    );
    assert_eq!(out.len(), 1);
    assert!(matches!(out[0].action, Action::Completed { .. }));
    assert_eq!(outcome_of(&out[0]), &Outcome::default());

    let scorm12 = events(
        &session(ScormEdition::Scorm12, "a|1"),
        &cmi(&[
            ("cmi.core.lesson_status", "completed"),
            ("cmi.core.score.raw", "120"),
            ("cmi.core.score.max", "100"),
        ]),
    );
    assert_eq!(outcome_of(&scorm12[0]).score, None);
}

#[test]
fn a_stored_event_rebuilds_the_same_statement() {
    let data = cmi(&[
        ("cmi.core.lesson_status", "passed"),
        ("cmi.core.score.raw", "85"),
        ("cmi.interactions.0.id", "q1"),
        ("cmi.interactions.0.type", "choice"),
        ("cmi.interactions.0.student_response", "b"),
    ]);
    for event in events(&session(ScormEdition::Scorm12, "a|1"), &data) {
        let stored = serde_json::to_string(&event).unwrap();
        let restored: LearningEvent = serde_json::from_str(&stored).unwrap();
        assert_eq!(restored, event);
        let options = BuildOptions::default();
        assert_eq!(
            build_statement(&restored, &recipient_a(), &options).unwrap(),
            build_statement(&event, &recipient_a(), &options).unwrap()
        );
    }
}

#[test]
fn scorm_12_scores_without_a_range_scale_by_a_hundred() {
    let out = events(
        &session(ScormEdition::Scorm12, "a|1"),
        &cmi(&[
            ("cmi.core.lesson_status", "failed"),
            ("cmi.core.score.raw", "42"),
        ]),
    );
    assert!(matches!(out[0].action, Action::Failed { .. }));
    let outcome = outcome_of(&out[0]);
    assert_eq!(outcome.completion, None);
    assert_eq!(
        outcome.score,
        Some(Score {
            scaled: Some(0.42),
            raw: Some(42.0),
            ..Score::default()
        })
    );
}

#[test]
fn stored_runtime_json_flattens_to_data_model_names() {
    let stored = json!({
        "cmi": {
            "core": { "lesson_status": "passed", "score": { "raw": 85, "max": "" } },
            "interactions": [
                { "id": "q1", "correct_responses": [ { "pattern": "b" } ] }
            ],
            "objectives": { "0": { "id": "obj-1" } },
            "suspend_data": null,
            "flag": true
        }
    });
    let flat = flatten_cmi(&stored);
    assert_eq!(
        flat,
        cmi(&[
            ("cmi.core.lesson_status", "passed"),
            ("cmi.core.score.max", ""),
            ("cmi.core.score.raw", "85"),
            ("cmi.flag", "true"),
            ("cmi.interactions.0.correct_responses.0.pattern", "b"),
            ("cmi.interactions.0.id", "q1"),
            ("cmi.objectives.0.id", "obj-1"),
        ])
    );
    // The bare `cmi` object flattens the same way.
    assert_eq!(flatten_cmi(&stored["cmi"]), flat);
}
