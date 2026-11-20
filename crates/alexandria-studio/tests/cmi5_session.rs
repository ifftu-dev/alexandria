//! cmi5 sessions as the launching system runs them: which statements a unit
//! may send, what the launching system writes for it, and when units, blocks
//! and the course are satisfied.

use std::collections::BTreeMap;
use std::io::Cursor;

use alexandria_studio::package::progress::{self, ActivityOutcome};
use alexandria_studio::package::{import_package, Limits, MoveOn, PackageDescription};
use alexandria_studio::xapi::cmi5::{
    self, Checked, LaunchMode, LaunchValues, Refusal, Satisfiable, SessionFacts, SessionRecord,
    UnitVerb,
};
use chrono::{TimeZone, Utc};
use serde_json::{json, Value};

#[path = "support/zip_fixture.rs"]
mod fixture;

const AU_ID: &str = "did:key:z6Mk/courses/aa/elements/bb/packages/cc/activities/au";
const PUBLISHER: &str = "https://example.org/au/1";
const REGISTRATION: &str = "760e3480-ba55-4991-94b0-01820dbd23a2";
const SESSION: &str = "3a1c0b4e-7f39-4a5c-9b6e-2f0d4c1a8e77";

fn facts() -> SessionFacts {
    SessionFacts {
        actor: cmi5::actor("0123456789abcdef0123456789abcdef"),
        registration: REGISTRATION.into(),
        session_id: SESSION.into(),
        activity_id: AU_ID.into(),
        publisher_id: PUBLISHER.into(),
        mastery_score: Some(0.8),
        launch_mode: LaunchMode::Normal,
    }
}

/// A cmi5 defined statement from the unit, following its context template.
fn defined(verb: &str, result: Value) -> Value {
    let mut categories = vec![json!({ "id": cmi5::CATEGORY_CMI5 })];
    if result.get("success").is_some() || result.get("completion").is_some() {
        categories.push(json!({ "id": cmi5::CATEGORY_MOVEON }));
    }
    let mut statement = json!({
        "id": "6690e6c9-3ef0-4ed3-8b37-7f3964730bee",
        "actor": cmi5::actor("0123456789abcdef0123456789abcdef"),
        "verb": { "id": format!("http://adlnet.gov/expapi/verbs/{verb}") },
        "object": { "id": AU_ID, "objectType": "Activity" },
        "timestamp": "2026-11-20T10:00:00.000Z",
        "context": {
            "registration": REGISTRATION,
            "contextActivities": {
                "category": categories,
                "grouping": [{ "id": PUBLISHER }],
            },
            "extensions": { cmi5::EXTENSION_SESSION_ID: SESSION },
        },
    });
    if !result.as_object().is_some_and(|r| r.is_empty()) {
        statement["result"] = result;
    }
    statement
}

fn initialized() -> Value {
    defined("initialized", json!({}))
}
fn completed() -> Value {
    defined(
        "completed",
        json!({ "completion": true, "duration": "PT1M" }),
    )
}
fn passed(scaled: f64) -> Value {
    defined(
        "passed",
        json!({ "success": true, "duration": "PT2M", "score": { "scaled": scaled } }),
    )
}
fn failed(scaled: f64) -> Value {
    defined(
        "failed",
        json!({ "success": false, "duration": "PT2M", "score": { "scaled": scaled } }),
    )
}
fn terminated() -> Value {
    defined("terminated", json!({ "duration": "PT3M" }))
}

/// A "cmi5 allowed" statement: the context template without the cmi5
/// category, any verb, any object.
fn allowed() -> Value {
    let mut statement = defined("experienced", json!({}));
    statement["context"]["contextActivities"]
        .as_object_mut()
        .unwrap()
        .remove("category");
    statement["object"]["id"] = json!("https://example.org/au/1/slide/3");
    statement
}

/// Sends statements in order, applying each accepted one; returns each
/// result.
fn run(record: &mut SessionRecord, statements: &[Value]) -> Vec<Result<Checked, Refusal>> {
    statements
        .iter()
        .map(|statement| {
            let checked = cmi5::check(statement, &facts(), record);
            if let Ok(checked) = &checked {
                record.apply(checked);
            }
            checked
        })
        .collect()
}

fn refused(result: &Result<Checked, Refusal>) -> bool {
    matches!(result, Err(Refusal::Rule(_)))
}

#[test]
fn a_session_runs_from_initialized_through_completed_and_passed_to_terminated() {
    let mut record = SessionRecord::default();
    let results = run(
        &mut record,
        &[
            initialized(),
            allowed(),
            completed(),
            passed(0.9),
            terminated(),
        ],
    );
    assert!(results.iter().all(Result::is_ok), "{results:?}");
    let verbs: Vec<_> = results.iter().map(|r| r.as_ref().unwrap().verb).collect();
    assert_eq!(
        verbs,
        [
            Some(UnitVerb::Initialized),
            None,
            Some(UnitVerb::Completed),
            Some(UnitVerb::Passed),
            Some(UnitVerb::Terminated)
        ]
    );
    let passed = results[3].as_ref().unwrap();
    assert_eq!(passed.score, Some(0.9));
    assert_eq!(passed.duration_seconds, Some(120.0));
    assert!(record.terminated());
}

#[test]
fn the_first_statement_of_a_session_must_be_initialized() {
    for first in [completed(), allowed(), terminated()] {
        let results = run(&mut SessionRecord::default(), &[first]);
        assert!(refused(&results[0]), "{results:?}");
    }
}

#[test]
fn nothing_is_accepted_after_terminated_or_abandoned() {
    let mut record = SessionRecord::default();
    let results = run(&mut record, &[initialized(), terminated(), allowed()]);
    assert!(refused(&results[2]));

    let mut record = SessionRecord::default();
    run(&mut record, &[initialized()]);
    record.abandoned = true;
    assert!(refused(&run(&mut record, &[completed()])[0]));
}

#[test]
fn a_defined_verb_is_used_once_per_session_and_passed_or_failed_only_once() {
    let mut record = SessionRecord::default();
    let results = run(
        &mut record,
        &[initialized(), initialized(), failed(0.2), passed(0.9)],
    );
    assert!(refused(&results[1]), "second initialized");
    assert!(results[2].is_ok());
    assert!(refused(&results[3]), "passed after failed in one session");
}

#[test]
fn completed_and_passed_are_used_once_per_registration_and_failed_never_follows_passed() {
    let mut record = SessionRecord {
        completed_before: true,
        passed_before: true,
        ..SessionRecord::default()
    };
    let results = run(
        &mut record,
        &[initialized(), completed(), passed(0.95), failed(0.1)],
    );
    assert!(results[0].is_ok());
    assert!(refused(&results[1]), "completed again in the registration");
    assert!(refused(&results[2]), "passed again in the registration");
    assert!(refused(&results[3]), "failed after passed");
}

#[test]
fn a_statement_outside_the_session_template_is_refused() {
    let mut wrong_actor = initialized();
    wrong_actor["actor"] = cmi5::actor("ffffffffffffffffffffffffffffffff");
    let mut wrong_registration = initialized();
    wrong_registration["context"]["registration"] = json!("00000000-0000-4000-8000-000000000000");
    let mut wrong_session = initialized();
    wrong_session["context"]["extensions"][cmi5::EXTENSION_SESSION_ID] = json!("another");
    let mut no_grouping = initialized();
    no_grouping["context"]["contextActivities"]["grouping"] = json!([]);
    let mut other_object = initialized();
    other_object["object"]["id"] = json!(PUBLISHER);
    for statement in [
        wrong_actor,
        wrong_registration,
        wrong_session,
        no_grouping,
        other_object,
    ] {
        assert!(refused(
            &run(&mut SessionRecord::default(), &[statement])[0]
        ));
    }
}

#[test]
fn results_follow_the_verb_rules_and_the_mastery_score() {
    let mut no_duration = completed();
    no_duration["result"]
        .as_object_mut()
        .unwrap()
        .remove("duration");
    let mut no_moveon = completed();
    no_moveon["context"]["contextActivities"]["category"] = json!([{ "id": cmi5::CATEGORY_CMI5 }]);
    let mut completed_with_score = completed();
    completed_with_score["result"]["score"] = json!({ "scaled": 1.0 });
    let mut initialized_with_moveon = initialized();
    initialized_with_moveon["context"]["contextActivities"]["category"] =
        json!([{ "id": cmi5::CATEGORY_CMI5 }, { "id": cmi5::CATEGORY_MOVEON }]);
    for statement in [
        no_duration,
        no_moveon,
        completed_with_score,
        passed(0.5),
        failed(0.85),
    ] {
        let results = run(&mut SessionRecord::default(), &[initialized(), statement]);
        assert!(refused(&results[1]), "{results:?}");
    }
    assert!(refused(
        &run(&mut SessionRecord::default(), &[initialized_with_moveon])[0]
    ));
}

#[test]
fn a_unit_may_not_void_or_use_the_launching_systems_verbs() {
    let mut voiding = allowed();
    voiding["verb"]["id"] = json!("http://adlnet.gov/expapi/verbs/voided");
    voiding["object"] =
        json!({ "objectType": "StatementRef", "id": "6690e6c9-3ef0-4ed3-8b37-7f3964730bee" });
    let mut satisfied = initialized();
    satisfied["verb"]["id"] = json!("https://w3id.org/xapi/adl/verbs/satisfied");
    let results = run(&mut SessionRecord::default(), &[initialized(), voiding]);
    assert!(refused(&results[1]));
    assert!(refused(
        &run(&mut SessionRecord::default(), &[satisfied])[0]
    ));
}

#[test]
fn the_launching_systems_statements_carry_the_session_context() {
    let at = Utc.with_ymd_and_hms(2026, 11, 20, 10, 0, 0).unwrap();
    let values = LaunchValues {
        move_on: MoveOn::CompletedAndPassed,
        launch_parameters: Some("sample string"),
        entitlement_key: Some("sample value"),
        return_url: Some("package://localhost/p/_alexandria/returned.html"),
        launch_url: "package://localhost/p/index.html?paramA=1",
    };
    let launched = cmi5::launched("id-1", &facts(), &values, at);
    assert_eq!(
        launched["verb"]["id"],
        "http://adlnet.gov/expapi/verbs/launched"
    );
    assert_eq!(launched["object"]["id"], AU_ID);
    assert_eq!(launched["context"]["registration"], REGISTRATION);
    let extensions = &launched["context"]["extensions"];
    assert_eq!(extensions[cmi5::EXTENSION_SESSION_ID], SESSION);
    assert_eq!(extensions[cmi5::EXTENSION_LAUNCH_MODE], "Normal");
    assert_eq!(extensions[cmi5::EXTENSION_MOVE_ON], "CompletedAndPassed");
    assert_eq!(extensions[cmi5::EXTENSION_MASTERY_SCORE], 0.8);
    assert_eq!(
        extensions[cmi5::EXTENSION_LAUNCH_PARAMETERS],
        "sample string"
    );
    assert_eq!(
        launched["context"]["contextActivities"]["grouping"][0]["id"],
        PUBLISHER
    );
    assert_eq!(
        launched["context"]["contextActivities"]["category"][0]["id"],
        cmi5::CATEGORY_CMI5
    );

    let data = cmi5::launch_data(&facts(), &values);
    assert_eq!(
        data["contextTemplate"]["extensions"][cmi5::EXTENSION_SESSION_ID],
        SESSION
    );
    assert_eq!(data["launchMode"], "Normal");
    assert_eq!(data["masteryScore"], 0.8);
    assert_eq!(data["returnURL"], values.return_url.unwrap());
    assert_eq!(data["entitlementKey"]["courseStructure"], "sample value");

    let abandoned = cmi5::abandoned("id-2", &facts(), 75.5, at);
    assert_eq!(
        abandoned["verb"]["id"],
        "https://w3id.org/xapi/adl/verbs/abandoned"
    );
    assert_eq!(abandoned["result"]["duration"], "PT1M15.50S");

    let satisfied = cmi5::satisfied(
        "id-3",
        &facts(),
        Satisfiable::Block,
        "did:key:z6Mk/block",
        "https://example.org/block",
        at,
    );
    assert_eq!(
        satisfied["verb"]["id"],
        "https://w3id.org/xapi/adl/verbs/satisfied"
    );
    assert_eq!(
        satisfied["object"]["definition"]["type"],
        cmi5::ACTIVITY_TYPE_BLOCK
    );
    assert_eq!(
        satisfied["context"]["contextActivities"]["grouping"][0]["id"],
        "https://example.org/block"
    );
    assert_eq!(
        satisfied["context"]["extensions"][cmi5::EXTENSION_SESSION_ID],
        SESSION
    );
}

#[test]
fn durations_parse_from_iso_8601() {
    assert_eq!(cmi5::parse_duration("PT1H2M3.5S"), Some(3723.5));
    assert_eq!(cmi5::parse_duration("P1DT1S"), Some(86_401.0));
    assert_eq!(cmi5::parse_duration("PT"), None);
    assert_eq!(cmi5::parse_duration("1H"), None);
    assert_eq!(cmi5::parse_duration("PT1S2M"), None);
}

/// A course with a block of two units and a unit outside it.
fn course() -> PackageDescription {
    let structure = r#"<?xml version="1.0" encoding="utf-8"?>
<courseStructure xmlns="https://w3id.org/xapi/profiles/cmi5/v1/CourseStructure.xsd">
  <course id="https://example.org/course"><title><langstring lang="en">Course</langstring></title></course>
  <block id="https://example.org/block">
    <title><langstring lang="en">Block</langstring></title>
    <au id="https://example.org/au/a" moveOn="Completed"><title><langstring lang="en">A</langstring></title><url>a.html</url></au>
    <au id="https://example.org/au/b" moveOn="NotApplicable"><title><langstring lang="en">B</langstring></title><url>b.html</url></au>
  </block>
  <au id="https://example.org/au/c" moveOn="CompletedAndPassed"><title><langstring lang="en">C</langstring></title><url>c.html</url></au>
</courseStructure>"#;
    import_package(
        Cursor::new(fixture::zip(&[
            fixture::file("cmi5.xml", structure),
            fixture::file("a.html", "<html></html>"),
            fixture::file("b.html", "<html></html>"),
            fixture::file("c.html", "<html></html>"),
        ])),
        &Limits::default(),
    )
    .expect("import")
}

#[test]
fn units_satisfy_by_move_on_and_blocks_and_the_course_follow_their_units() {
    assert!(progress::unit_satisfied(
        MoveOn::CompletedOrPassed,
        false,
        true
    ));
    assert!(!progress::unit_satisfied(
        MoveOn::CompletedAndPassed,
        true,
        false
    ));
    assert!(progress::unit_satisfied(
        MoveOn::NotApplicable,
        false,
        false
    ));

    let course = course();
    let ids = |nodes: Vec<&alexandria_studio::package::Activity>| {
        nodes.into_iter().map(|n| n.id.clone()).collect::<Vec<_>>()
    };
    let mut outcomes = BTreeMap::new();
    assert!(ids(progress::satisfied_nodes(&course, &outcomes)).is_empty());
    assert!(!progress::rollup(&course, &outcomes, None).complete);

    let done = ActivityOutcome {
        complete: true,
        score: None,
    };
    outcomes.insert("https://example.org/au/a".to_string(), done);
    assert_eq!(
        ids(progress::satisfied_nodes(&course, &outcomes)),
        ["https://example.org/block"]
    );
    assert!(!progress::rollup(&course, &outcomes, None).complete);

    outcomes.insert("https://example.org/au/c".to_string(), done);
    assert_eq!(
        ids(progress::satisfied_nodes(&course, &outcomes)),
        ["https://example.org/block", "https://example.org/course"]
    );
    assert!(progress::rollup(&course, &outcomes, None).complete);
}

#[test]
fn cmi5_records_build_for_a_recipient_with_the_cmi5_categories_and_their_own_actor() {
    use alexandria_studio::xapi::{
        build_statement, Action, ActivityRef, BuildOptions, LearningEvent, Outcome, Profile,
        Recipient, Target,
    };
    let creator = "did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK";
    let element = ActivityRef::course(creator, "aa".repeat(32)).element("bb".repeat(32));
    let unit = element.package_activity("cc".repeat(32), PUBLISHER);
    let block = element.package_activity("cc".repeat(32), "https://example.org/block");
    let recipient = Recipient::pairwise(&[3; 32], "destination|test").expect("recipient");
    let event = |local_id: &str, action: Action| LearningEvent {
        local_id: local_id.into(),
        timestamp: Utc.with_ymd_and_hms(2026, 11, 20, 10, 0, 0).unwrap(),
        registration: Some("attempt".into()),
        action,
        profile: Some(Profile::Cmi5),
    };
    let categories = |statement: &Value| -> Vec<String> {
        statement["context"]["contextActivities"]["category"]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|c| c["id"].as_str().unwrap().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    let build = |local_id: &str, action: Action| {
        let statement = build_statement(
            &event(local_id, action),
            &recipient,
            &BuildOptions::default(),
        )
        .expect("builds");
        serde_json::to_value(&statement).unwrap()
    };

    let completed = build(
        "cmi5|completed",
        Action::Completed {
            target: Target::new(unit.clone()),
            outcome: Outcome::default(),
        },
    );
    assert_eq!(
        completed["actor"]["account"]["name"],
        json!(recipient.account().name)
    );
    assert_eq!(
        categories(&completed),
        [cmi5::CATEGORY_CMI5, cmi5::CATEGORY_MOVEON]
    );

    let satisfied = build(
        "cmi5|satisfied",
        Action::Satisfied {
            target: Target::new(block),
            scope: Satisfiable::Block,
        },
    );
    assert_eq!(
        satisfied["verb"]["id"],
        "https://w3id.org/xapi/adl/verbs/satisfied"
    );
    assert_eq!(
        satisfied["object"]["definition"]["type"],
        cmi5::ACTIVITY_TYPE_BLOCK
    );
    assert_eq!(categories(&satisfied), [cmi5::CATEGORY_CMI5]);

    let abandoned = build(
        "cmi5|abandoned",
        Action::Abandoned {
            target: Target::new(unit),
            outcome: Outcome {
                duration_centiseconds: Some(4_500),
                ..Outcome::default()
            },
        },
    );
    assert_eq!(abandoned["result"]["duration"], "PT45S");
    assert_eq!(categories(&abandoned), [cmi5::CATEGORY_CMI5]);
}
