//! The statement builder, tested against the canonical example statements of
//! the identifier compatibility check (`fixtures/xapi-canonical/`). Those
//! statements were written by an independent script, accepted by four
//! external record stores in both xAPI versions, and use throwaway keys
//! derived from public labels:
//!
//! - learner key material: SHA-256 of `ls12:test:learner`;
//! - recipient `recipient-a`: actor key from the Ed25519 seed SHA-256 of
//!   `ls12:test:learner|recipient-a`, identifier key from the learner key
//!   material and the label `recipient-a`;
//! - local statement ids `<version>|<file>|<original id>`, local registration
//!   `<version>|attempt-1`.

use std::collections::BTreeMap;
use std::path::Path;

use alexandria_studio::xapi::{
    build_batch, build_statement, Account, Action, ActivityRef, BuildOptions, IdKey, Interaction,
    InteractionComponent, InteractionType, LearningEvent, Names, Outcome, Profile, Recipient,
    RecordKind, Score, Statement, Target, ACCOUNT_HOME_PAGE,
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

const VERSIONS: [&str; 2] = ["1.0.3", "2.0.0"];

/// Original statement ids the canonical script derived its ids from.
fn original_id(version: &str, name: &str) -> &'static str {
    match (version, name) {
        ("1.0.3", "r01") => "2969ba8e-7da9-47b2-9862-3188734fb3d7",
        ("2.0.0", "r01") => "b452aeb5-4869-4a90-8bd2-107963702e24",
        ("1.0.3", "r02") => "42632d2d-9946-4ebf-b62e-598aea197174",
        ("2.0.0", "r02") => "f94c7bfb-e041-4bc3-97e8-c009b1578789",
        ("1.0.3", "r03") => "f961daca-aa0e-432e-a1a7-e8b6827765dd",
        ("2.0.0", "r03") => "d7bc7f89-0bc5-4a13-8606-9f663b977abe",
        ("1.0.3", "r04") => "837c8c3a-bcb9-4cd0-94a5-8b167135ccbe",
        ("2.0.0", "r04") => "6bbb6759-4864-4121-936c-ec77448c02cb",
        ("1.0.3", "r05") => "73177173-fd73-4b44-be5f-f8c077556cd9",
        ("2.0.0", "r05") => "b208e0b0-de05-4862-a55c-862598d3cb24",
        ("1.0.3", "r06") => "80329e18-8996-4e80-bfb9-7ba5ba3493d9",
        ("2.0.0", "r06") => "3ecb7ca1-a1e3-449c-b0ba-2218a7426ff8",
        ("1.0.3", "r07") => "229739d7-ab48-4308-88fc-74749e2e69c1",
        ("2.0.0", "r07") => "4c8a72d1-1c04-4ea7-9d4a-74b14246636e",
        ("1.0.3", "r08") => "65c946a9-3cc7-444c-a429-9205390a8ff2",
        ("2.0.0", "r08") => "670f5ad5-0432-4a46-96ad-9a9557109796",
        _ => panic!("no original id for {version} {name}"),
    }
}

fn learner_secret() -> [u8; 32] {
    sha256(b"ls12:test:learner")
}

fn recipient_a() -> Recipient {
    let pairwise = SigningKey::from_bytes(&sha256(b"ls12:test:learner|recipient-a"));
    let account = Account::for_did(derive_did_key(&pairwise).as_str()).unwrap();
    Recipient::new(
        account,
        IdKey::derive(&learner_secret(), "recipient-a").unwrap(),
    )
}

fn canonical(version: &str, file: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/xapi-canonical")
        .join(version)
        .join(format!("{file}.json"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn at() -> DateTime<Utc> {
    "2026-11-19T10:00:00Z".parse().unwrap()
}

fn options() -> BuildOptions {
    BuildOptions {
        language: Some("en-US".into()),
        ..BuildOptions::default()
    }
}

fn event(version: &str, file: &str, action: Action) -> LearningEvent {
    let short = &file[..3];
    LearningEvent {
        local_id: format!("{version}|{file}|{}", original_id(version, short)),
        timestamp: at(),
        registration: Some(format!("{version}|attempt-1")),
        action,
        profile: None,
    }
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

fn built(event: &LearningEvent) -> Value {
    let statement = alexandria_studio::xapi::build_statement(event, &recipient_a(), &options())
        .expect("builds");
    serde_json::to_value(statement).unwrap()
}

#[test]
fn enrolment_builds_the_canonical_registered_statement() {
    for version in VERSIONS {
        let file = "r01-registered-course";
        let e = event(
            version,
            file,
            Action::Registered {
                target: Target::named(course(), "en-US", "Test course"),
            },
        );
        assert_eq!(built(&e), canonical(version, file), "{version}");
    }
}

const PREFIX_ACTIVITIES: &str = "did:key:z6MkoRwQfNfmnSY6h11tXjnjt58Ta512cEk3QitVxRh3xU1g/courses/59fb4b553e7cf374392d9387f819a087afd1f135663d1a92473845def73c4a66/elements/b7159f93e1ce2ea232131615ea188b904138b52267893a3f8af92f1ea7a73c8c/packages/19ffd3ae8430b64d88265892dc4ab2f08bb6ff63010912ee50f76dcae620134a/activities/";

#[test]
fn package_activity_ids_take_one_canonical_percent_encoding() {
    // The forms the identifier check sent to the stores (a04, a05).
    assert_eq!(
        package_activity("Lesson 1: Übung/α").iri().unwrap(),
        format!("{PREFIX_ACTIVITIES}Lesson%201%3A%20%C3%9Cbung%2F%CE%B1")
    );
    assert_eq!(
        package_activity("https://publisher.example.invalid/au/1")
            .iri()
            .unwrap(),
        format!("{PREFIX_ACTIVITIES}https%3A%2F%2Fpublisher.example.invalid%2Fau%2F1")
    );
    assert_eq!(
        package_activity("a-b.c_d~e").iri().unwrap(),
        format!("{PREFIX_ACTIVITIES}a-b.c_d~e")
    );
    // Dot segments are never emitted.
    assert_eq!(
        package_activity(".").iri().unwrap(),
        format!("{PREFIX_ACTIVITIES}%2E")
    );
    assert_eq!(
        package_activity("..").iri().unwrap(),
        format!("{PREFIX_ACTIVITIES}%2E%2E")
    );
    assert_eq!(
        package_activity("item_1").interaction("q 1").iri().unwrap(),
        format!("{PREFIX_ACTIVITIES}item_1/interactions/q%201")
    );
}

#[test]
fn identifiers_past_the_store_limit_fall_back_to_a_digest() {
    // 289 characters of prefix: an id of 1794 fits exactly at 2083.
    assert_eq!(PREFIX_ACTIVITIES.len(), 289);
    let fits = "x".repeat(1794);
    assert_eq!(package_activity(&fits).iri().unwrap().len(), 2083);

    let long = "x".repeat(1795);
    let iri = package_activity(&long).iri().unwrap();
    let element_prefix = &PREFIX_ACTIVITIES[..PREFIX_ACTIVITIES.len() - "activities/".len()];
    // SHA-256 of 1795 'x' characters.
    assert_eq!(
        iri,
        format!(
            "{element_prefix}activity-digests/{}",
            hex(&sha256(long.as_bytes()))
        )
    );

    // An interaction under an activity that itself sits at the cap.
    let interaction = package_activity(&fits).interaction("q1").iri().unwrap();
    assert!(interaction.len() <= 2083, "{}", interaction.len());
    assert!(interaction.contains("/interaction-digests/"));
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn two_recipients_never_share_an_actor_statement_or_registration_id() {
    let secret = learner_secret();
    let learner_did = derive_did_key(&SigningKey::from_bytes(&secret));
    let a = Recipient::pairwise(&secret, "destination|https://lrs.one.invalid/xapi|key-1").unwrap();
    let b = Recipient::pairwise(&secret, "share|org-2").unwrap();
    let launch = event(
        "2.0.0",
        "r02-launched-element",
        Action::Launched {
            target: Target::new(element()),
        },
    );
    let for_a = build_statement(&launch, &a, &options()).unwrap();
    let for_b = build_statement(&launch, &b, &options()).unwrap();

    assert_ne!(for_a.actor, for_b.actor);
    assert_ne!(for_a.id, for_b.id);
    let registration = |s: &Statement| s.context.as_ref().unwrap().registration.clone().unwrap();
    assert_ne!(registration(&for_a), registration(&for_b));

    // Neither carries the learner's DID.
    let learner_name = learner_did.as_str().strip_prefix("did:key:").unwrap();
    for recipient in [&a, &b] {
        assert_ne!(recipient.account().name, learner_name);
        assert_eq!(recipient.account().name.len(), 48);
        assert_eq!(recipient.account().home_page, ACCOUNT_HOME_PAGE);
    }

    // Resending gives the same identifiers.
    let again = Recipient::pairwise(&secret, "share|org-2").unwrap();
    assert_eq!(build_statement(&launch, &again, &options()).unwrap(), for_b);
}

#[test]
fn the_learners_own_did_appears_only_when_chosen() {
    let secret = learner_secret();
    let learner_did = derive_did_key(&SigningKey::from_bytes(&secret));
    let chosen = Recipient::real_did(&secret, learner_did.as_str(), "share|org-2").unwrap();
    assert_eq!(
        chosen.account().name,
        learner_did.as_str().strip_prefix("did:key:").unwrap()
    );
    // The statement ids stay per recipient even then.
    let pairwise = Recipient::pairwise(&secret, "share|org-2").unwrap();
    let other = Recipient::real_did(&secret, learner_did.as_str(), "share|org-3").unwrap();
    assert_eq!(chosen.statement_id("r1"), pairwise.statement_id("r1"));
    assert_ne!(chosen.statement_id("r1"), other.statement_id("r1"));

    // A proposed account is used verbatim, within the name limit.
    let proposed = Account {
        home_page: "https://hr.example.invalid".into(),
        name: "employee-4711".into(),
    };
    let r = Recipient::proposed(&secret, proposed.clone(), "share|org-2").unwrap();
    assert_eq!(r.account(), &proposed);
    let too_long = Account {
        name: "n".repeat(51),
        ..proposed
    };
    let refused = Recipient::proposed(&secret, too_long, "share|org-2").unwrap_err();
    assert_eq!(refused.code(), "invalid_account");
}

#[test]
fn invalid_events_are_refused_before_anything_is_stored() {
    let refuse = |action: Action| {
        let e = event("2.0.0", "r02-launched-element", action);
        build_statement(&e, &recipient_a(), &options())
            .unwrap_err()
            .code()
    };
    let launch = |activity: ActivityRef| Action::Launched {
        target: Target::new(activity),
    };
    assert_eq!(
        refuse(launch(ActivityRef::course(
            "did:web:example.invalid",
            COURSE
        ))),
        "invalid_did"
    );
    assert_eq!(
        refuse(launch(ActivityRef::course(CREATOR, "not-hex"))),
        "invalid_activity"
    );
    assert_eq!(
        refuse(launch(course().element(COURSE.to_uppercase()))),
        "invalid_activity"
    );
    assert_eq!(refuse(launch(package_activity(""))), "invalid_activity");
    let mut gap = package_activity("item_1");
    gap.element_id = None;
    assert_eq!(refuse(launch(gap)), "invalid_activity");
    assert_eq!(
        refuse(Action::Registered {
            target: Target::new(element())
        }),
        "invalid_activity"
    );
    assert_eq!(
        refuse(Action::Progressed {
            target: Target::new(element()),
            percent: 101
        }),
        "invalid_result"
    );
    let with_score = |score: Score| Outcome {
        score: Some(score),
        ..Outcome::default()
    };
    assert_eq!(
        refuse(Action::Scored {
            target: Target::new(element()),
            outcome: with_score(Score {
                scaled: Some(1.5),
                ..Score::default()
            })
        }),
        "invalid_result"
    );
    assert_eq!(
        refuse(Action::Scored {
            target: Target::new(element()),
            outcome: with_score(Score {
                raw: Some(120.0),
                min: Some(0.0),
                max: Some(100.0),
                ..Score::default()
            })
        }),
        "invalid_result"
    );
    assert_eq!(
        refuse(Action::Passed {
            target: Target::new(element()),
            outcome: Outcome {
                success: Some(false),
                ..Outcome::default()
            }
        }),
        "invalid_result"
    );
    assert_eq!(
        refuse(Action::Completed {
            target: Target::new(element()),
            outcome: Outcome {
                response: Some("b".into()),
                ..Outcome::default()
            }
        }),
        "invalid_result"
    );
    let mut name = Target::new(element());
    name.name = Some(BTreeMap::from([("not a tag".to_string(), "x".to_string())]));
    assert_eq!(
        refuse(Action::Launched { target: name }),
        "invalid_language"
    );

    let mut empty = event("2.0.0", "r02-launched-element", launch(element()));
    empty.local_id.clear();
    assert_eq!(
        build_statement(&empty, &recipient_a(), &options())
            .unwrap_err()
            .code(),
        "invalid_identifier"
    );
}

#[test]
fn a_stored_statement_is_checked_again_when_read_back() {
    let mut value = canonical("2.0.0", "r05-passed-package-activity");
    let statement: Statement = serde_json::from_value(value.clone()).unwrap();
    statement.validate().unwrap();

    value["context"]["registration"] = json!("attempt-1");
    let bad: Statement = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(bad.validate().unwrap_err().code(), "invalid_identifier");

    value["context"]["registration"] = json!("a0e9d119-dded-8ecd-bb40-f852f5730c3d");
    value["result"]["duration"] = json!("P1D");
    let bad: Statement = serde_json::from_value(value).unwrap();
    assert_eq!(bad.validate().unwrap_err().code(), "invalid_result");

    // Anything outside the shared 1.0.3 / 2.0 subset does not parse at all.
    let mut agents = canonical("2.0.0", "r02-launched-element");
    agents["context"]["contextAgents"] = json!([]);
    assert!(serde_json::from_value::<Statement>(agents).is_err());
}

#[test]
fn a_private_course_omits_names_but_keeps_identifiers() {
    let e = event(
        "2.0.0",
        "r01-registered-course",
        Action::Registered {
            target: Target::named(course(), "en-US", "Test course"),
        },
    );
    let private = BuildOptions {
        names: Names::Omit,
        ..options()
    };
    let statement =
        serde_json::to_value(build_statement(&e, &recipient_a(), &private).unwrap()).unwrap();
    let mut expected = canonical("2.0.0", "r01-registered-course");
    expected["object"]["definition"]
        .as_object_mut()
        .unwrap()
        .remove("name");
    assert_eq!(statement, expected);
}

#[test]
fn element_launch_progress_and_completion_build_the_canonical_statements() {
    for version in VERSIONS {
        let target = || Target::named(element(), "en-US", "Test element");
        let cases = [
            (
                "r02-launched-element",
                Action::Launched { target: target() },
            ),
            (
                "r03-progressed-element",
                Action::Progressed {
                    target: target(),
                    percent: 40,
                },
            ),
            (
                "r04-completed-element",
                Action::Completed {
                    target: target(),
                    outcome: Outcome {
                        duration_centiseconds: Some(75_000),
                        ..Outcome::default()
                    },
                },
            ),
        ];
        for (file, action) in cases {
            let e = event(version, file, action);
            assert_eq!(built(&e), canonical(version, file), "{version} {file}");
        }
    }
}

#[test]
fn package_activity_pass_and_fail_build_the_canonical_statements() {
    for version in VERSIONS {
        let file = "r05-passed-package-activity";
        let mut passed = event(
            version,
            file,
            Action::Passed {
                target: Target::named(
                    package_activity("item_1"),
                    "en-US",
                    "Package activity item_1",
                ),
                outcome: Outcome {
                    completion: Some(true),
                    score: Some(Score {
                        scaled: Some(0.85),
                        raw: Some(85.0),
                        min: Some(0.0),
                        max: Some(100.0),
                    }),
                    duration_centiseconds: Some(48_000),
                    ..Outcome::default()
                },
            },
        );
        passed.profile = Some(Profile::Scorm);
        // The SCORM profile requires the category activity to carry the
        // profile activity type; the hand-built example left it out.
        let mut expected = canonical(version, file);
        expected["context"]["contextActivities"]["category"][0]["definition"] =
            json!({ "type": "http://adlnet.gov/expapi/activities/profile" });
        assert_eq!(built(&passed), expected, "{version} {file}");

        let file = "r06-failed-package-activity";
        let failed = event(
            version,
            file,
            Action::Failed {
                target: Target::named(
                    package_activity("item_2"),
                    "en-US",
                    "Package activity item_2",
                ),
                outcome: Outcome {
                    score: Some(Score {
                        scaled: Some(0.3),
                        ..Score::default()
                    }),
                    ..Outcome::default()
                },
            },
        );
        assert_eq!(built(&failed), canonical(version, file), "{version} {file}");
    }
}

#[test]
fn an_interaction_response_builds_the_canonical_interaction_record() {
    for version in VERSIONS {
        let file = "r07-answered-interaction";
        let component = |id: &str, text: &str| InteractionComponent {
            id: id.into(),
            description: Some(BTreeMap::from([("en-US".to_string(), text.to_string())])),
        };
        let mut interaction = Interaction::new(
            package_activity("item_1").interaction("q1"),
            InteractionType::Choice,
        );
        interaction.correct_responses = Some(vec!["b".into()]);
        interaction.choices = vec![component("a", "A"), component("b", "B")];
        let e = event(
            version,
            file,
            Action::Responded {
                interaction,
                outcome: Outcome {
                    success: Some(true),
                    response: Some("b".into()),
                    ..Outcome::default()
                },
            },
        );
        assert_eq!(e.kind(), RecordKind::Interaction);
        // The SCORM profile reports interactions with `responded`; the
        // hand-built example used `answered`.
        let mut expected = canonical(version, file);
        expected["verb"] = json!({
            "id": "http://adlnet.gov/expapi/verbs/responded",
            "display": { "en-US": "responded" }
        });
        assert_eq!(built(&e), expected, "{version} {file}");
    }
}

#[test]
fn voiding_references_the_recipients_own_statement_id() {
    for version in VERSIONS {
        let file = "r08-voiding";
        let target = format!(
            "{version}|r02-launched-element|{}",
            original_id(version, "r02")
        );
        let mut void = LearningEvent::voiding(&target, at());
        // The example's own id came from its file name, not `void|<target>`.
        void.local_id = format!("{version}|{file}|{}", original_id(version, "r08"));
        assert_eq!(built(&void), canonical(version, file), "{version}");
    }
}

#[test]
fn a_batch_builds_the_canonical_outbox_post() {
    let originals = [
        (
            "1.0.3",
            [
                "3ebb605b-5035-47b4-804b-5ab9d84560ad",
                "6e892965-76df-4147-b805-1eab007de423",
                "0dee4985-48b4-4a62-a37f-2e71a3640aec",
            ],
        ),
        (
            "2.0.0",
            [
                "e3ffc99c-4e22-4334-925b-f5dfc264d15d",
                "9303d04f-69d1-4721-91cc-0bc7a7f660db",
                "23f0b980-bd86-4d7c-9827-21703cfb8809",
            ],
        ),
    ];
    let elements = [
        "b14d25ce1cd6b71db169bae5c83dbd44af4987cd9d580bb3d308729af2c83cba",
        "c903d9b1ca840737e4e8df6d2f3e6d37c94a26be23bb70b342f85df0d6955346",
        "628f7b7ec574299e54464e8acd33dad4a6c3dcb7a8c1fb3920668647f6e12ef1",
    ];
    for (version, ids) in originals {
        let events: Vec<LearningEvent> = (0..3)
            .map(|i| LearningEvent {
                local_id: format!("{version}|r09-batch-{i}|{}", ids[i]),
                timestamp: at(),
                registration: Some(format!("{version}|attempt-1")),
                action: Action::Completed {
                    target: Target::named(
                        course().element(elements[i]),
                        "en-US",
                        &format!("Batch element {}", i + 1),
                    ),
                    outcome: Outcome {
                        duration_centiseconds: Some(75_000),
                        ..Outcome::default()
                    },
                },
                profile: None,
            })
            .collect();
        let batch = build_batch(&events, &recipient_a(), &options()).unwrap();
        assert_eq!(
            serde_json::to_value(batch).unwrap(),
            canonical(version, "r09-batch"),
            "{version}"
        );
    }
}
