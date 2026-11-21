//! The statement builder's output, kept as a fixture the hosted record store
//! is tested against (`fixtures/xapi-store/builder-output.json`).
//!
//! The record store lives in another repository and takes a byte-for-byte
//! copy of this file (`tests/fixtures/xapi/app-builder/` there), so the two
//! halves are shown to agree without either depending on the other. The
//! corpus covers every action, both profiles, the three actor choices, a
//! private course, and the identifier scheme's edges: percent-encoded and IRI
//! package activity ids, an id at the 2083-character cap, and the digest
//! fallbacks past it.
//!
//! When the builder changes on purpose, regenerate and copy it over:
//!
//! ```sh
//! ALEXANDRIA_UPDATE_FIXTURES=1 cargo test -p alexandria-studio --test xapi_store_fixture
//! ```
//!
//! Keys are throwaway, derived from public labels like the canonical
//! examples' (`xapi_statements.rs`).

use std::collections::BTreeMap;
use std::path::Path;

use alexandria_studio::xapi::scorm::{events, ScormEdition, ScormSession};
use alexandria_studio::xapi::{
    build_statement, Account, Action, ActivityRef, BuildOptions, Interaction, InteractionComponent,
    InteractionType, LearningEvent, Names, Outcome, Profile, Recipient, Satisfiable, Score, Target,
    ACCOUNT_HOME_PAGE, MAX_IRI_LEN,
};
use alexandria_verify::did::derive_did_key;
use alexandria_verify::hash::sha256;
use chrono::{DateTime, Utc};
use ed25519_dalek::SigningKey;
use serde_json::{json, Value};

const CREATOR: &str = "did:key:z6MkoRwQfNfmnSY6h11tXjnjt58Ta512cEk3QitVxRh3xU1g";
const COURSE: &str = "59fb4b553e7cf374392d9387f819a087afd1f135663d1a92473845def73c4a66";
const ELEMENT: &str = "b7159f93e1ce2ea232131615ea188b904138b52267893a3f8af92f1ea7a73c8c";
const PACKAGE: &str = "19ffd3ae8430b64d88265892dc4ab2f08bb6ff63010912ee50f76dcae620134a";
const FIXTURE: &str = "fixtures/xapi-store/builder-output.json";

fn secret() -> [u8; 32] {
    sha256(b"ls22:test:learner")
}

fn at() -> DateTime<Utc> {
    "2026-11-21T09:00:00Z".parse().unwrap()
}

fn course() -> ActivityRef {
    ActivityRef::course(CREATOR, COURSE)
}

fn element() -> ActivityRef {
    course().element(ELEMENT)
}

fn package_activity(id: &str) -> ActivityRef {
    element().package_activity(PACKAGE, id)
}

fn event(local_id: &str, action: Action) -> LearningEvent {
    LearningEvent {
        local_id: local_id.into(),
        timestamp: at(),
        registration: Some("enrolment-1".into()),
        action,
        profile: None,
    }
}

fn scored(scaled: f64) -> Outcome {
    Outcome {
        score: Some(Score {
            scaled: Some(scaled),
            ..Score::default()
        }),
        ..Outcome::default()
    }
}

/// Every case: a name, the event, who receives it, and whether names are
/// disclosed.
fn corpus() -> Vec<(String, LearningEvent, Recipient, Names)> {
    let secret = secret();
    let share = Recipient::pairwise(&secret, "share|fixture-org").unwrap();
    let learner_did = derive_did_key(&SigningKey::from_bytes(&secret));
    let real = Recipient::real_did(&secret, learner_did.as_str(), "share|fixture-org").unwrap();
    let proposed = Recipient::proposed(
        &secret,
        Account {
            home_page: "https://hr.example.invalid".into(),
            name: "employee-4711".into(),
        },
        "share|fixture-org",
    )
    .unwrap();

    let mut cases: Vec<(&str, LearningEvent)> = vec![
        (
            "registered-course",
            LearningEvent {
                registration: None,
                ..event(
                    "registered|enrolment-1",
                    Action::Registered {
                        target: Target::named(course(), "en-US", "Fixture course"),
                    },
                )
            },
        ),
        (
            "launched-element",
            event(
                "launched|enrolment-1|element",
                Action::Launched {
                    target: Target::named(element(), "en-US", "Fixture element"),
                },
            ),
        ),
        (
            "progressed-course",
            event(
                "progressed|enrolment-1|40",
                Action::Progressed {
                    target: Target::new(course()),
                    percent: 40,
                },
            ),
        ),
        (
            "completed-element",
            event(
                "completed|enrolment-1|element",
                Action::Completed {
                    target: Target::new(element()),
                    outcome: Outcome {
                        duration_centiseconds: Some(75_000),
                        ..Outcome::default()
                    },
                },
            ),
        ),
        (
            "passed-element",
            event(
                "result|passed",
                Action::Passed {
                    target: Target::new(element()),
                    outcome: Outcome {
                        completion: Some(true),
                        score: Some(Score {
                            scaled: Some(0.85),
                            raw: Some(85.0),
                            min: Some(0.0),
                            max: Some(100.0),
                        }),
                        ..Outcome::default()
                    },
                },
            ),
        ),
        (
            "failed-element",
            event(
                "result|failed",
                Action::Failed {
                    target: Target::new(element()),
                    outcome: scored(0.3),
                },
            ),
        ),
        (
            "scored-element",
            event(
                "result|scored",
                Action::Scored {
                    target: Target::new(element()),
                    outcome: scored(-0.25),
                },
            ),
        ),
        (
            "launched-percent-encoded-activity",
            event(
                "launched|attempt-1|encoded",
                Action::Launched {
                    target: Target::new(package_activity("Lesson 1: Übung/α")),
                },
            ),
        ),
        (
            "launched-iri-activity",
            event(
                "launched|attempt-1|iri",
                Action::Launched {
                    target: Target::new(package_activity("https://publisher.example.invalid/au/1")),
                },
            ),
        ),
        (
            "launched-activity-at-the-cap",
            event(
                "launched|attempt-1|cap",
                Action::Launched {
                    target: Target::new(package_activity(&"x".repeat(1794))),
                },
            ),
        ),
        (
            "launched-activity-digest",
            event(
                "launched|attempt-1|digest",
                Action::Launched {
                    target: Target::new(package_activity(&"x".repeat(1795))),
                },
            ),
        ),
    ];

    // An interaction under an activity at the cap: its id falls back to a
    // digest too.
    let mut interaction = Interaction::new(
        package_activity(&"x".repeat(1794)).interaction("q1"),
        InteractionType::Choice,
    );
    interaction.correct_responses = Some(vec!["b".into()]);
    interaction.choices = vec![
        InteractionComponent {
            id: "a".into(),
            description: Some(BTreeMap::from([("en-US".into(), "A".into())])),
        },
        InteractionComponent {
            id: "b".into(),
            description: Some(BTreeMap::from([("en-US".into(), "B".into())])),
        },
    ];
    let mut responded = event(
        "package|attempt-1|cap|interaction|0",
        Action::Responded {
            interaction,
            outcome: Outcome {
                success: Some(true),
                response: Some("b".into()),
                ..Outcome::default()
            },
        },
    );
    responded.profile = Some(Profile::Scorm);
    cases.push(("responded-interaction-digest", responded));

    // A SCORM 2004 session as the player hands it over: the result and its
    // interactions, through the SCORM profile mapping.
    let session = ScormSession {
        edition: ScormEdition::Scorm2004,
        activity: package_activity("item_1"),
        name: Some(BTreeMap::from([(
            "en-US".to_string(),
            "Package activity item_1".to_string(),
        )])),
        local_id: "package|attempt-1|item_1|passed".into(),
        registration: Some("attempt-1".into()),
        timestamp: at(),
        language: "en-US".into(),
    };
    let cmi: BTreeMap<String, String> = [
        ("cmi.completion_status", "completed"),
        ("cmi.success_status", "passed"),
        ("cmi.score.scaled", "0.9"),
        ("cmi.session_time", "PT1H2M3.5S"),
        ("cmi.interactions.0.id", "q 1/ü"),
        ("cmi.interactions.0.type", "choice"),
        ("cmi.interactions.0.learner_response", "a[,]b"),
        ("cmi.interactions.0.correct_responses.0.pattern", "a[,]b"),
        ("cmi.interactions.0.result", "correct"),
        ("cmi.interactions.1.id", "q2"),
        ("cmi.interactions.1.type", "fill-in"),
        ("cmi.interactions.1.learner_response", "Paris"),
        ("cmi.interactions.1.result", "incorrect"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let scorm = events(&session, &cmi);
    assert_eq!(scorm.len(), 3, "a result and two interactions");
    let scorm_names = [
        "scorm-passed",
        "scorm-responded-choice",
        "scorm-responded-fill-in",
    ];
    for (name, e) in scorm_names.into_iter().zip(scorm) {
        cases.push((name, e));
    }

    // cmi5 records, as the app keeps them for recipients.
    let unit = element().package_activity(PACKAGE, "https://publisher.example.invalid/au/1");
    let block = element().package_activity(PACKAGE, "https://publisher.example.invalid/block");
    let cmi5 = |local_id: &str, action: Action| LearningEvent {
        profile: Some(Profile::Cmi5),
        ..event(local_id, action)
    };
    cases.push((
        "cmi5-abandoned",
        cmi5(
            "cmi5|abandoned",
            Action::Abandoned {
                target: Target::new(unit),
                outcome: Outcome {
                    duration_centiseconds: Some(4_500),
                    ..Outcome::default()
                },
            },
        ),
    ));
    cases.push((
        "cmi5-satisfied-block",
        cmi5(
            "cmi5|satisfied",
            Action::Satisfied {
                target: Target::new(block),
                scope: Satisfiable::Block,
            },
        ),
    ));
    cases.push((
        "voided-launch",
        LearningEvent::voiding("launched|enrolment-1|element", at()),
    ));

    let mut out: Vec<(String, LearningEvent, Recipient, Names)> = cases
        .into_iter()
        .map(|(n, e)| (n.to_string(), e, share.clone(), Names::Disclose))
        .collect();
    out.push((
        "private-course-registered".into(),
        LearningEvent {
            registration: None,
            ..event(
                "registered|enrolment-2",
                Action::Registered {
                    target: Target::named(
                        ActivityRef::course(CREATOR, "ab".repeat(32)),
                        "en-US",
                        "Private course",
                    ),
                },
            )
        },
        share.clone(),
        Names::Omit,
    ));
    out.push((
        "real-did-launched".into(),
        event(
            "launched|enrolment-1|element|real",
            Action::Launched {
                target: Target::new(element()),
            },
        ),
        real,
        Names::Disclose,
    ));
    out.push((
        "proposed-account-completed".into(),
        event(
            "completed|enrolment-1|element|proposed",
            Action::Completed {
                target: Target::new(element()),
                outcome: Outcome::default(),
            },
        ),
        proposed,
        Names::Disclose,
    ));
    out
}

fn built() -> Value {
    let statements: Vec<Value> = corpus()
        .into_iter()
        .map(|(name, event, recipient, names)| {
            let options = BuildOptions {
                language: Some("en-US".into()),
                names,
            };
            let statement = build_statement(&event, &recipient, &options)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            json!({ "name": name, "statement": statement })
        })
        .collect();
    json!({
        "about": "Statement builder output (alexandria-studio xapi), for record store tests. \
                  Generated by tests/xapi_store_fixture.rs; do not edit by hand.",
        "statements": statements,
    })
}

#[test]
fn the_record_store_fixture_is_the_builders_current_output() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE);
    let mut text = serde_json::to_string_pretty(&built()).unwrap();
    text.push('\n');
    if std::env::var("ALEXANDRIA_UPDATE_FIXTURES").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
    }
    let kept = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        kept == text,
        "{FIXTURE} is not the builder's output: regenerate it with \
         ALEXANDRIA_UPDATE_FIXTURES=1 and copy it to the record store's tests"
    );
}

#[test]
fn the_fixture_exercises_the_identifier_scheme() {
    let fixture = built();
    let statements = fixture["statements"].as_array().unwrap();
    let ids: Vec<&str> = statements
        .iter()
        .filter_map(|s| s["statement"]["object"]["id"].as_str())
        .collect();
    // Activity ids are DID URLs under the creator, within the cap, with the
    // digest fallbacks in use.
    let activities: Vec<&&str> = ids.iter().filter(|i| i.starts_with("did:key:")).collect();
    assert!(activities.iter().all(|i| i.len() <= MAX_IRI_LEN));
    assert!(activities.iter().any(|i| i.len() == MAX_IRI_LEN));
    assert!(activities.iter().any(|i| i.contains("/activity-digests/")));
    assert!(activities
        .iter()
        .any(|i| i.contains("/interaction-digests/")));
    assert!(activities.iter().any(|i| i.contains("%C3%9C")));
    // Actors: the fixed urn:uuid home page, except the proposed account.
    let homes: Vec<&str> = statements
        .iter()
        .map(|s| {
            s["statement"]["actor"]["account"]["homePage"]
                .as_str()
                .unwrap()
        })
        .collect();
    assert_eq!(
        homes.iter().filter(|h| **h != ACCOUNT_HOME_PAGE).count(),
        1,
        "{homes:?}"
    );
    assert!(ACCOUNT_HOME_PAGE.starts_with("urn:uuid:"));
}
