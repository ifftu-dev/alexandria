//! cmi5 conformance at the host seam, with the published test packages.
//!
//! Each `lts_*` test takes one package of the CATAPULT LMS Test Suite (the
//! cmi5 project's conformance packages; course structures committed under
//! `src-tauri/tests/fixtures/catapult-lts/`), imports it through the app's
//! import path, launches its unit through [`launch`] and replays over real
//! HTTP the requests that package's unit makes (`lts/pkg/src/<package>/
//! <package>.js`, statements shaped as cmi5.js 3 shapes them). The unit's own
//! pass/fail checks and the suite's post-run checks (`lts/__tests__/
//! runtime.js`, statements read back by registration) become assertions;
//! messages carry the cmi5 requirement id.
//!
//! Where the app departs from what a check expects, a `known_deviation_*`
//! test pins what the app does instead. The matrix is `conformance.md`
//! beside this file; the last test fails if a row names a missing test.

use std::path::{Path, PathBuf};

use reqwest::{Method, StatusCode};
use serde_json::{json, Value};

use super::store::{self, StatementQuery};
use super::tests::{urlencode, Learner, Unit};
use super::*;
use crate::package_serving;

const LTS: &str = "https://w3id.org/xapi/cmi5/catapult/lts";
const LAUNCHED: &str = "http://adlnet.gov/expapi/verbs/launched";
const SATISFIED: &str = "https://w3id.org/xapi/adl/verbs/satisfied";
const ABANDONED: &str = "https://w3id.org/xapi/adl/verbs/abandoned";
const CATEGORY_CMI5: &str = "https://w3id.org/xapi/cmi5/context/categories/cmi5";
const CATEGORY_MOVEON: &str = "https://w3id.org/xapi/cmi5/context/categories/moveon";
const EXT_SESSION: &str = "https://w3id.org/xapi/cmi5/context/extensions/sessionid";
const EXT_MASTERY: &str = "https://w3id.org/xapi/cmi5/context/extensions/masteryscore";
const EXT_LAUNCH_MODE: &str = "https://w3id.org/xapi/cmi5/context/extensions/launchmode";
const EXT_LAUNCH_URL: &str = "https://w3id.org/xapi/cmi5/context/extensions/launchurl";
const EXT_MOVE_ON: &str = "https://w3id.org/xapi/cmi5/context/extensions/moveon";
const EXT_LAUNCH_PARAMETERS: &str =
    "https://w3id.org/xapi/cmi5/context/extensions/launchparameters";

fn manifest_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// The archive of one LTS package: its published course structure and a
/// page standing in for the unit's built bundle.
fn lts_package(name: &str) -> Vec<(String, Vec<u8>)> {
    let structure = manifest_dir().join(format!("tests/fixtures/catapult-lts/{name}/cmi5.xml"));
    vec![
        (
            "cmi5.xml".into(),
            std::fs::read(&structure).expect("LTS course structure"),
        ),
        (
            "index.html".into(),
            format!("<!doctype html><title>{name} - Project CATAPULT</title>").into_bytes(),
        ),
    ]
}

fn au_id(name: &str) -> String {
    format!("{LTS}/au/{name}")
}

async fn lts(name: &str) -> Learner {
    Learner::with_package(&lts_package(name)).await
}

fn ids(list: &Value) -> Vec<&str> {
    list.as_array()
        .map(|list| list.iter().filter_map(|a| a["id"].as_str()).collect())
        .unwrap_or_default()
}

/// The suite's `fetchStatements`: every statement of the registration,
/// oldest first, as the record store holds it after the session.
fn registration_statements(learner: &Learner, registration: &str) -> Vec<Value> {
    let db = learner.db.lock().unwrap();
    store::statements(
        db.conn(),
        &learner.attempt.id,
        &StatementQuery {
            registration: Some(registration.to_string()),
            ascending: true,
            ..StatementQuery::default()
        },
    )
    .expect("statements by registration")
}

fn verb(statement: &Value) -> &str {
    statement["verb"]["id"].as_str().unwrap_or_default()
}

/// The unit's side, as cmi5.js 3 (the library every LTS unit is built on)
/// talks to the launching system.
struct Au {
    unit: Unit,
    launch_data: Value,
}

impl Au {
    async fn launch(learner: &Learner, publisher_id: &str) -> Self {
        Self {
            unit: learner.unit(publisher_id).await,
            launch_data: Value::Null,
        }
    }

    fn session_id(&self) -> &str {
        &self.unit.launch.session_id
    }

    /// `postFetch` and `loadLMSLaunchData`.
    async fn fetch_and_load(&mut self) {
        let answer = self.unit.post_fetch().await;
        self.unit.token = Some(
            answer["auth-token"]
                .as_str()
                .expect("8.2.2.0-1: auth-token")
                .to_string(),
        );
        let (status, data) = self.get_state("LMS.LaunchData").await;
        assert_eq!(status, StatusCode::OK, "10.1.0.0-1: {data}");
        self.unit.template = data["contextTemplate"].clone();
        self.launch_data = data;
    }

    /// `loadLearnerPrefs`.
    async fn load_preferences(&self) -> (StatusCode, Value) {
        self.unit
            .xapi(Method::GET, &self.preferences_path(None), None)
            .await
    }

    /// `start`: fetch, launch data, learner preferences, `initialized`.
    async fn start(&mut self) {
        self.fetch_and_load().await;
        let (status, _) = self.load_preferences().await;
        assert!(
            status == StatusCode::OK || status == StatusCode::NOT_FOUND,
            "{status}"
        );
        assert_eq!(
            self.put(&self.initialized()).await,
            StatusCode::NO_CONTENT,
            "initialized"
        );
    }

    fn preferences_path(&self, agent: Option<&Value>) -> String {
        let mut path = "agents/profile?profileId=cmi5LearnerPreferences".to_string();
        if let Some(agent) = agent.or(Some(&self.unit.actor)) {
            path.push_str(&format!("&agent={}", urlencode(&agent.to_string())));
        }
        path
    }

    async fn get_state(&self, state_id: &str) -> (StatusCode, Value) {
        self.unit
            .xapi(Method::GET, &self.unit.state_path(state_id), None)
            .await
    }

    /// `prepareStatement`: a "cmi5 allowed" statement on the context
    /// template, with cmi5.js's own source activity under `other`.
    fn prepare(&self, verb: &str) -> Value {
        let mut context = self.unit.template.clone();
        context["registration"] = json!(self.unit.registration);
        let activities = context
            .as_object_mut()
            .unwrap()
            .entry("contextActivities")
            .or_insert_with(|| json!({}));
        activities["other"] = json!([{
            "id": "http://id.tincanapi.com/activity/software/@rusticisoftware/cmi5/3.0.0",
            "definition": { "type": "http://id.tincanapi.com/activitytype/source" },
        }]);
        json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "timestamp": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "actor": self.unit.actor,
            "verb": { "id": verb },
            "object": { "id": self.unit.activity_id },
            "context": context,
        })
    }

    /// `_prepareStatement`: a "cmi5 defined" statement.
    fn defined(&self, name: &str) -> Value {
        let mut statement = self.prepare(&format!("http://adlnet.gov/expapi/verbs/{name}"));
        statement["verb"]["display"] = json!({ "en-US": name });
        statement["context"]["contextActivities"]["category"] = json!([{ "id": CATEGORY_CMI5 }]);
        statement
    }

    fn initialized(&self) -> Value {
        self.defined("initialized")
    }

    fn terminated(&self) -> Value {
        let mut statement = self.defined("terminated");
        statement["result"] = json!({ "duration": "PT1S" });
        statement
    }

    fn completed(&self) -> Value {
        let mut statement = self.defined("completed");
        statement["result"] = json!({ "completion": true, "duration": "PT1S" });
        push_category(&mut statement, CATEGORY_MOVEON);
        statement
    }

    fn judged(&self, name: &str, success: bool, score: Option<Value>) -> Value {
        let mut statement = self.defined(name);
        statement["result"] = json!({ "success": success, "duration": "PT1S" });
        if let Some(score) = score {
            if let (Some(mastery), true) = (
                self.launch_data["masteryScore"].as_f64(),
                score.get("scaled").is_some(),
            ) {
                statement["context"]["extensions"][EXT_MASTERY] = json!(mastery);
            }
            statement["result"]["score"] = score;
        }
        push_category(&mut statement, CATEGORY_MOVEON);
        statement
    }

    fn passed(&self, score: Option<Value>) -> Value {
        self.judged("passed", true, score)
    }

    fn failed(&self, score: Option<Value>) -> Value {
        self.judged("failed", false, score)
    }

    /// `sendStatement`: PUT with the statement's id.
    async fn put(&self, statement: &Value) -> StatusCode {
        let id = statement["id"].as_str().unwrap_or_default();
        self.unit
            .xapi(
                Method::PUT,
                &format!("statements?statementId={id}"),
                Some(statement.clone()),
            )
            .await
            .0
    }

    async fn post(&self, statement: &Value) -> StatusCode {
        self.unit
            .xapi(Method::POST, "statements", Some(statement.clone()))
            .await
            .0
    }

    /// A request with exactly these headers besides the Origin.
    async fn raw(
        &self,
        method: Method,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<&str>,
    ) -> StatusCode {
        let mut request = self
            .unit
            .client
            .request(method, format!("{}{path}", self.unit.endpoint))
            .header("Origin", package_serving::origin());
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        if let Some(body) = body {
            request = request.body(body.to_string());
        }
        request
            .send()
            .await
            .expect("request reaches the endpoint")
            .status()
    }

    /// `Helpers.sendStatement` with no expectations: the launching system
    /// must refuse the statement with 403.
    async fn refused(&self, statement: &Value, requirement: &str) {
        let status = self.put(statement).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{requirement}: {statement}");
    }

    fn auth(&self) -> String {
        format!("Basic {}", self.unit.token.as_deref().unwrap_or_default())
    }
}

fn push_category(statement: &mut Value, id: &str) {
    statement["context"]["contextActivities"]["category"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "id": id }));
}

fn launch_url(url: &str) -> url::Url {
    url::Url::parse(&url.replace(package_serving::origin(), "http://package"))
        .expect("launch url parses")
}

// ---- 001-essentials --------------------------------------------------------

#[tokio::test]
async fn lts_001_essentials_launch_launch_data_and_statement_order() {
    let name = "001-essentials";
    let learner = lts(name).await;
    let mut au = Au::launch(&learner, &au_id(name)).await;

    // The unit's checks of its launch URL (8.1).
    let url = launch_url(&au.unit.launch.url);
    assert!(url.path().ends_with("/index.html"), "8.1.0.0-5");
    let param = |key: &str| {
        url.query_pairs()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.into_owned())
    };
    assert!(param("endpoint").is_some(), "8.1.1.0-1");
    assert!(param("fetch").is_some(), "8.1.2.0-1");
    let actor: Value = serde_json::from_str(&param("actor").expect("8.1.3.0-1")).unwrap();
    assert!(
        actor["account"]["name"].is_string() && actor["account"]["homePage"].is_string(),
        "8.1.3.0-1"
    );
    assert!(
        actor["objectType"].is_null() || actor["objectType"] == "Agent",
        "9.2.0.0-2"
    );
    assert!(param("registration").is_some(), "8.1.4.0-1");
    let activity_id = param("activityId").expect("8.1.5.0-2");
    assert_ne!(activity_id, au_id(name), "8.1.5.0-3");
    assert_eq!(param("paramA").as_deref(), Some("1"), "13.1.4.0-3");
    assert_eq!(param("paramB").as_deref(), Some("2"), "13.1.4.0-3");

    // The fetch URL (8.2).
    let fetched = au
        .unit
        .client
        .post(&au.unit.fetch)
        .header("Origin", package_serving::origin())
        .send()
        .await
        .unwrap();
    assert_eq!(fetched.status(), StatusCode::OK, "8.2.2.0-3");
    assert!(
        fetched
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/json")),
        "8.2.2.0-2"
    );
    let token: Value = fetched.json().await.unwrap();
    au.unit.token = Some(token["auth-token"].as_str().expect("8.2.2.0-1").into());

    // LMS.LaunchData, read with the launch's activityId, actor (as the URL
    // gave it) and registration (10.1.0.0-3 to -5).
    let (status, data) = au
        .unit
        .xapi(
            Method::GET,
            &format!(
                "activities/state?stateId=LMS.LaunchData&activityId={}&agent={}&registration={}",
                urlencode(&activity_id),
                urlencode(&param("actor").unwrap()),
                urlencode(&param("registration").unwrap())
            ),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "10.1.0.0-1");
    assert!(data.is_object(), "10.1.0.0-2");
    let template = &data["contextTemplate"];
    assert!(template.is_object(), "10.2.1.0-1");
    assert!(
        template["extensions"][EXT_SESSION].is_string(),
        "10.2.1.0-2"
    );
    assert!(
        ids(&template["contextActivities"]["grouping"]).contains(&au_id(name).as_str()),
        "10.2.1.0-3"
    );
    assert!(data["launchMode"].is_string(), "10.2.2.0-4");
    assert_eq!(data["launchParameters"], "sample string", "10.2.3.0-1");
    assert_eq!(data["masteryScore"], 0.9, "10.2.4.0-1");
    assert_eq!(data["moveOn"], "CompletedAndPassed", "10.2.5.0-1");
    assert_eq!(
        data["entitlementKey"]["courseStructure"], "sample value",
        "10.2.7.0-1, 10.2.7.1-1"
    );
    assert!(data["returnURL"].is_string());
    au.unit.template = template.clone();
    au.launch_data = data.clone();

    let (status, preferences) = au.load_preferences().await;
    assert_eq!(status, StatusCode::OK);
    assert!(preferences["languagePreference"].is_string(), "11.0.0.0-5");
    assert!(preferences["audioPreference"].is_string(), "11.0.0.0-5");

    // initialized, passed, completed, terminated: each PUT answers 204.
    for statement in [
        au.initialized(),
        au.passed(None),
        au.completed(),
        au.terminated(),
    ] {
        assert_eq!(
            au.put(&statement).await,
            StatusCode::NO_CONTENT,
            "4.2.0.0-2: {}",
            verb(&statement)
        );
    }
    learner.end(au.session_id()).await;

    // The suite's checks of what the registration recorded.
    let found = registration_statements(&learner, &au.unit.registration);
    let verbs: Vec<_> = found.iter().map(verb).collect();
    assert_eq!(
        verbs.iter().filter(|v| **v == LAUNCHED).count(),
        1,
        "9.3.1.0-2"
    );
    assert_eq!(verbs[0], LAUNCHED, "9.3.1.0-1: {verbs:?}");
    for index in [0, 4, 5] {
        assert!(
            ids(&found[index]["context"]["contextActivities"]["category"]).contains(&CATEGORY_CMI5),
            "9.6.2.1-1: {index}"
        );
    }
    let launched = &found[0]["context"]["extensions"];
    assert_eq!(launched[EXT_SESSION], json!(au.session_id()), "9.6.3.1-3");
    assert_eq!(launched[EXT_MASTERY], 0.9, "9.6.3.2-1");
    assert_eq!(launched[EXT_LAUNCH_MODE], data["launchMode"], "9.6.3.3-1");
    let recorded = launch_url(launched[EXT_LAUNCH_URL].as_str().expect("9.6.3.4-1"));
    assert_eq!(recorded.origin(), url.origin(), "9.6.3.4-1");
    assert_eq!(recorded.path(), url.path(), "9.6.3.4-1");
    let recorded_query: Vec<_> = recorded
        .query_pairs()
        .map(|(k, _)| k.into_owned())
        .collect();
    assert_eq!(recorded_query, ["paramA", "paramB"], "9.6.3.4-1");
    assert_eq!(launched[EXT_MOVE_ON], data["moveOn"], "9.6.3.6-1");
    assert_eq!(
        launched[EXT_LAUNCH_PARAMETERS], data["launchParameters"],
        "9.6.3.7-1"
    );

    assert_eq!(verbs[4], SATISFIED, "9.3.9.0-1: {verbs:?}");
    assert_eq!(
        found[4]["object"]["definition"]["type"], "https://w3id.org/xapi/cmi5/activitytype/block",
        "9.3.9.0-2"
    );
    assert_ne!(
        found[4]["object"]["id"],
        json!(format!("{LTS}/block/{name}")),
        "9.3.9.0-4"
    );
    assert_eq!(verbs[5], SATISFIED, "9.3.9.0-5");
    assert_eq!(
        found[5]["object"]["definition"]["type"], "https://w3id.org/xapi/cmi5/activitytype/course",
        "9.3.9.0-6"
    );
    assert_ne!(
        found[5]["object"]["id"],
        json!(format!("{LTS}/course/{name}")),
        "9.3.9.0-8"
    );
    for index in [4, 5] {
        assert_eq!(
            found[index]["context"]["extensions"][EXT_SESSION],
            json!(au.session_id()),
            "9.3.9.0-9"
        );
    }
    assert!(
        ids(&found[4]["context"]["contextActivities"]["grouping"])
            .contains(&format!("{LTS}/block/{name}").as_str()),
        "9.6.2.3-2"
    );
    assert!(
        ids(&found[5]["context"]["contextActivities"]["grouping"])
            .contains(&format!("{LTS}/course/{name}").as_str()),
        "9.6.2.3-2"
    );
}

// ---- 002-allowed, 003-launchMethod-OwnWindow --------------------------------

#[tokio::test]
async fn lts_002_allowed_statements_are_recorded_and_reported() {
    let name = "002-allowed";
    let learner = lts(name).await;
    let mut au = Au::launch(&learner, &au_id(name)).await;
    au.start().await;
    let experienced = au.prepare("http://adlnet.gov/expapi/verbs/experienced");
    assert_eq!(
        au.put(&experienced).await,
        StatusCode::NO_CONTENT,
        "9.3.0.0-9"
    );
    assert_eq!(au.put(&au.terminated()).await, StatusCode::NO_CONTENT);
    learner.end(au.session_id()).await;

    let found = registration_statements(&learner, &au.unit.registration);
    assert_eq!(
        verb(&found[2]),
        "http://adlnet.gov/expapi/verbs/experienced",
        "9.3.0.0-9"
    );
}

#[tokio::test]
async fn lts_003_an_own_window_unit_gets_its_own_window_on_desktop() {
    let name = "003-launchMethod-OwnWindow";
    let learner = lts(name).await;
    let launch = learner.launch(&au_id(name)).await.expect("launch");
    // 8.1.0.0-2: desktop opens a window of its own; mobile plays the unit in
    // the frame with focus mode forced (known deviation).
    assert_eq!(launch.own_window, OWN_WINDOWS);
    assert_eq!(launch.focus_forced, !OWN_WINDOWS);
    let mut au = Au {
        unit: Unit::from_launch(launch),
        launch_data: Value::Null,
    };
    au.start().await;
    assert_eq!(au.put(&au.terminated()).await, StatusCode::NO_CONTENT);
    learner.end(au.session_id()).await;
}

// ---- 004 moveOn --------------------------------------------------------------

/// 004-1 to 004-4: one statement meets the unit's moveOn criteria, and the
/// block and the course are satisfied right after it, before terminated.
async fn satisfied_by(name: &str, move_on: &str, completed: bool) {
    let learner = lts(name).await;
    let mut au = Au::launch(&learner, &au_id(name)).await;
    au.start().await;
    assert_eq!(au.launch_data["moveOn"], move_on);
    let statement = if completed {
        au.completed()
    } else {
        au.passed(None)
    };
    assert_eq!(au.put(&statement).await, StatusCode::NO_CONTENT);
    assert_eq!(au.put(&au.terminated()).await, StatusCode::NO_CONTENT);
    learner.end(au.session_id()).await;

    let found = registration_statements(&learner, &au.unit.registration);
    let requirement = if completed { "9.3.3.0-3" } else { "9.3.4.0-4" };
    assert_eq!(verb(&found[3]), SATISFIED, "{requirement}");
    assert_eq!(verb(&found[4]), SATISFIED, "{requirement}");
    assert!(
        ids(&found[3]["context"]["contextActivities"]["grouping"])
            .contains(&format!("{LTS}/block/{name}").as_str()),
        "9.6.2.3-2"
    );
    assert!(
        ids(&found[4]["context"]["contextActivities"]["grouping"])
            .contains(&format!("{LTS}/course/{name}").as_str()),
        "9.6.2.3-2"
    );
}

#[tokio::test]
async fn lts_004_1_completed_satisfies_a_completed_unit() {
    satisfied_by("004-1-moveOn-Completed", "Completed", true).await;
}

#[tokio::test]
async fn lts_004_2_completed_satisfies_a_completed_or_passed_unit() {
    satisfied_by("004-2-moveOn-CompletedOrPassed", "CompletedOrPassed", true).await;
}

#[tokio::test]
async fn lts_004_3_passed_satisfies_a_passed_unit() {
    satisfied_by("004-3-moveOn-Passed", "Passed", false).await;
}

#[tokio::test]
async fn lts_004_4_passed_satisfies_a_completed_or_passed_unit() {
    satisfied_by("004-4-moveOn-CompletedOrPassed", "CompletedOrPassed", false).await;
}

#[tokio::test]
async fn lts_004_5_a_not_applicable_unit_is_satisfied_when_the_registration_starts() {
    let name = "004-5-moveOn-NotApplicable";
    let learner = lts(name).await;
    let mut au = Au::launch(&learner, &au_id(name)).await;
    au.start().await;
    assert_eq!(au.put(&au.terminated()).await, StatusCode::NO_CONTENT);
    learner.end(au.session_id()).await;

    let found = registration_statements(&learner, &au.unit.registration);
    let verbs: Vec<_> = found.iter().map(verb).collect();
    assert_eq!(verbs[0], SATISFIED, "9.6.1.1-3: {verbs:?}");
    assert_eq!(verbs[1], SATISFIED, "9.6.1.1-3: {verbs:?}");
    assert!(
        ids(&found[0]["context"]["contextActivities"]["grouping"])
            .contains(&format!("{LTS}/block/{name}").as_str()),
        "9.6.2.3-2"
    );
    assert!(
        ids(&found[1]["context"]["contextActivities"]["grouping"])
            .contains(&format!("{LTS}/course/{name}").as_str()),
        "9.6.2.3-2"
    );
}

// ---- 005-1 and 005-2: what the launching system must refuse -----------------

#[tokio::test]
async fn lts_005_1_the_token_the_fetch_url_and_launch_data_are_guarded() {
    let name = "005-1-invalid-au";
    let learner = lts(name).await;
    let mut au = Au::launch(&learner, &au_id(name)).await;

    // A statement before the token was fetched, with made-up credentials.
    let status = au
        .raw(
            Method::PUT,
            &format!("statements?statementId={}", uuid::Uuid::new_v4()),
            &[
                ("X-Experience-API-Version", "1.0.3"),
                (
                    "Authorization",
                    "Basic Y2F0YXB1bHQ6ZmlyZSBzb21lIFJ1c3RpY2kgU29mdHdhcmUgcm9ja3Mh",
                ),
                ("Content-Type", "application/json"),
            ],
            Some("{}"),
        )
        .await;
    assert!(
        status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN,
        "8.1.2.0-2 (d): {status}"
    );

    au.fetch_and_load().await;
    au.load_preferences().await;

    // A second POST to the fetch URL: 200, no token, an error code.
    let again = au.unit.post_fetch().await;
    assert!(again.get("auth-token").is_none(), "8.2.3.0-1");
    assert!(
        ["1", "2", "3"].contains(&again["error-code"].as_str().unwrap_or_default()),
        "8.2.3.2-1"
    );

    // LMS.LaunchData is the launching system's: no PUT, no DELETE.
    let state = au.unit.state_path("LMS.LaunchData");
    let auth = au.auth();
    for method in [Method::PUT, Method::POST, Method::DELETE] {
        let status = au
            .raw(
                method.clone(),
                &state,
                &[
                    ("X-Experience-API-Version", "1.0.3"),
                    ("Authorization", &auth),
                    ("Content-Type", "application/json"),
                ],
                Some(r#"{"test":true}"#),
            )
            .await;
        assert!(!status.is_success(), "10.2.1.0-5 (d): {method} {status}");
    }

    // State requests name the session's registration.
    let foreign = state.replace(
        &urlencode(&au.unit.registration),
        "ccaf384c-f8d4-4e7a-8304-49af58f0b176",
    );
    let (status, _) = au.unit.xapi(Method::GET, &foreign, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "8.1.4.0-3 (d)");

    // A request without the token.
    let initialized = au.initialized();
    let status = au
        .raw(
            Method::PUT,
            &format!(
                "statements?statementId={}",
                initialized["id"].as_str().unwrap()
            ),
            &[
                ("X-Experience-API-Version", "1.0.3"),
                ("Content-Type", "application/json"),
            ],
            Some(&initialized.to_string()),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "8.1.2.0-5 (d)");
    learner.end(au.session_id()).await;
}

#[tokio::test]
async fn lts_005_1_learner_preferences_writes_are_checked() {
    let name = "005-1-invalid-au";
    let learner = lts(name).await;
    let mut au = Au::launch(&learner, &au_id(name)).await;
    au.fetch_and_load().await;
    let auth = au.auth();
    let valid = r#"{"languagePreference":"en-US,fr-FR,fr-BE","audioPreference":"on"}"#;
    let put = |path: String, content_type: Option<&'static str>, body: String| {
        let au = &au;
        let auth = auth.clone();
        async move {
            let mut headers = vec![
                ("X-Experience-API-Version", "1.0.3"),
                ("Authorization", auth.as_str()),
            ];
            if let Some(content_type) = content_type {
                headers.push(("Content-Type", content_type));
            }
            au.raw(Method::PUT, &path, &headers, Some(&body)).await
        }
    };
    let own = au.preferences_path(None);
    let no_agent = "agents/profile?profileId=cmi5LearnerPreferences".to_string();
    let other = au.preferences_path(Some(&json!({ "mbox": "mailto:catapult@example.com" })));

    // No agent at all is an xAPI error (400); someone else's agent, 403.
    assert_eq!(
        put(no_agent, Some("application/json"), valid.into()).await,
        StatusCode::BAD_REQUEST,
        "11.0.0.0-1 (d)"
    );
    assert_eq!(
        put(other, Some("application/json"), valid.into()).await,
        StatusCode::FORBIDDEN,
        "11.0.0.0-1 (d)"
    );
    // Not JSON, not an object, or missing one of the two properties: 403.
    for (content_type, body) in [
        (None, valid.to_string()),
        (Some("text/plain"), "just some text".to_string()),
        (
            Some("application/json"),
            r#"{"audioPreference":"on"}"#.into(),
        ),
        (
            Some("application/json"),
            r#"{"languagePreference":"en-US,fr-FR,fr-BE"}"#.into(),
        ),
    ] {
        assert_eq!(
            put(own.clone(), content_type, body.clone()).await,
            StatusCode::FORBIDDEN,
            "11.0.0.0-5 (d): {content_type:?} {body}"
        );
    }
    // languagePreference is a comma-separated list of language tags.
    for languages in ["not comma separated", ""] {
        let body = json!({ "languagePreference": languages, "audioPreference": "on" });
        assert_eq!(
            put(own.clone(), Some("application/json"), body.to_string()).await,
            StatusCode::FORBIDDEN,
            "11.1.0.0-1 (d): {languages:?}"
        );
    }
    // What a unit may write (the preferences document is the learner's).
    let (_, stored) = au.load_preferences().await;
    assert_ne!(stored["languagePreference"], "not comma separated");
    learner.end(au.session_id()).await;
}

/// The rows of 005-1's statement table, in its order: each altered statement
/// must be refused with 403, except where the row expects it stored.
#[tokio::test]
async fn lts_005_1_statements_breaking_the_rules_are_refused() {
    let name = "005-1-invalid-au";
    let learner = lts(name).await;
    let mut au = Au::launch(&learner, &au_id(name)).await;
    au.fetch_and_load().await;
    au.load_preferences().await;

    // Before initialized.
    let experienced = au.prepare("http://adlnet.gov/expapi/verbs/experienced");
    au.refused(&experienced, "9.3.0.0-4 (d2), 7.1.1.0-1 (d2)")
        .await;
    assert_eq!(au.put(&au.initialized()).await, StatusCode::NO_CONTENT);

    // An allowed statement is stored; voiding it is refused.
    assert_eq!(
        au.put(&experienced).await,
        StatusCode::NO_CONTENT,
        "9.3.0.0-9"
    );
    let mut voiding = au.prepare("http://adlnet.gov/expapi/verbs/voided");
    voiding["object"] = json!({ "id": experienced["id"], "objectType": "StatementRef" });
    au.refused(&voiding, "6.3.0.0-1").await;

    let full_score = || json!({ "min": 0, "max": 100, "scaled": 0.9, "raw": 90 });
    let low_score = || json!({ "min": 0, "max": 100, "scaled": 0.89, "raw": 89 });
    type Alter = fn(&mut Value);
    let table: Vec<(&str, &str, Alter)> = vec![
        // 9.1.0.0-1 (d) and 9.7.0.0-1 (d), a statement without an id or a
        // timestamp: known deviations, see the tests named for them.
        ("allowed", "9.7.0.0-2 (d)", |st| {
            let at = st["timestamp"].as_str().unwrap().replace('Z', "-06:00");
            st["timestamp"] = json!(at);
        }),
        ("passed", "9.2.0.0-2", |st| {
            st["actor"]["objectType"] = json!("Group")
        }),
        ("passed", "9.2.0.0-2", |st| {
            st["actor"]["objectType"] = json!("Unknown")
        }),
        ("passed", "9.2.0.0-3", |st| {
            st["actor"].as_object_mut().unwrap().remove("account");
            st["actor"]["mbox"] = json!("mailto:catapult@example.com");
        }),
        ("initialized", "9.3.2.0-3 (d)", |_| {}),
        ("passed", "8.1.5.0-6 (d), 9.4.0.0-1 (d)", |st| {
            st["object"]["id"] = json!("http://example.com/not/the/lms/id")
        }),
        ("passed", "9.6.0.0-1 (d)", |st| {
            st.as_object_mut().unwrap().remove("context");
        }),
        ("passed", "9.6.1.0-1 (d)", |st| {
            st["context"]
                .as_object_mut()
                .unwrap()
                .remove("registration");
        }),
        ("passed", "9.6.1.0-1 (d)", |st| {
            st["context"]["registration"] = json!("ccaf384c-f8d4-4e7a-8304-49af58f0b176")
        }),
        ("passed", "9.5.3.0-2 (d)", |st| {
            st["result"]["completion"] = json!(true)
        }),
        ("completed", "9.5.2.0-3 (d)", |st| {
            st["result"]["success"] = json!(true)
        }),
        ("completed", "9.5.3.0-1 (d1)", |st| {
            st["result"].as_object_mut().unwrap().remove("completion");
        }),
        ("completed", "9.5.3.0-1 (d1)", |st| {
            st["result"]["completion"] = json!(false)
        }),
        ("completed", "9.5.1.0-2 (d)", |st| {
            st["result"]["score"] = json!({ "raw": 50, "scaled": 0.5, "min": 0, "max": 100 })
        }),
        ("terminated", "9.5.4.1-1 (d)", |st| {
            st.as_object_mut().unwrap().remove("result");
        }),
        ("terminated", "9.5.4.1-1 (d)", |st| {
            st["result"].as_object_mut().unwrap().remove("duration");
        }),
        ("completed", "9.5.4.1-2 (d)", |st| {
            st["result"].as_object_mut().unwrap().remove("duration");
        }),
        ("passed", "9.5.4.1-3 (d)", |st| {
            st["result"].as_object_mut().unwrap().remove("duration");
        }),
        ("failed", "9.5.4.1-4 (d)", |st| {
            st["result"].as_object_mut().unwrap().remove("duration");
        }),
        ("completed", "store", |_| {}),
        ("completed", "9.3.0.0-2 (d)", |_| {}),
        ("passed", "9.5.2.0-1 (d1)", |st| {
            st["result"].as_object_mut().unwrap().remove("success");
        }),
        ("passed", "9.5.2.0-1 (d1)", |st| {
            st["result"]["success"] = json!(false)
        }),
        ("failed", "9.5.2.0-2 (d)", |st| {
            st["result"].as_object_mut().unwrap().remove("success");
        }),
        ("failed", "9.5.2.0-2 (d)", |st| {
            st["result"]["success"] = json!(true)
        }),
        ("passed", "9.3.4.0-2 (d2)", |st| {
            st["result"]["score"] = json!({ "scaled": 0.89 })
        }),
        ("failed", "9.3.5.0-2 (d)", |st| {
            st["result"]["score"] = json!({ "scaled": 0.9 })
        }),
        ("failed", "9.3.5.0-2 (d)", |st| {
            st["result"]["score"] = json!({ "scaled": 0.91 })
        }),
        ("passed", "9.5.1.0-3 (d1)", |st| {
            st["result"]["score"].as_object_mut().unwrap().remove("min");
        }),
        ("passed", "9.5.1.0-3 (d2)", |st| {
            st["result"]["score"].as_object_mut().unwrap().remove("max");
        }),
        ("completed", "9.6.2.2-1 (d1)", |st| {
            st["context"]["contextActivities"]["category"]
                .as_array_mut()
                .unwrap()
                .pop();
        }),
        ("passed", "9.6.2.2-1 (d2)", |st| {
            st["context"]["contextActivities"]["category"]
                .as_array_mut()
                .unwrap()
                .pop();
        }),
        ("failed", "9.6.2.2-1 (d3)", |st| {
            st["context"]["contextActivities"]["category"]
                .as_array_mut()
                .unwrap()
                .pop();
        }),
        ("allowed", "9.6.2.2-2 (d)", |st| {
            st["context"]["contextActivities"]["category"] = json!([{ "id": CATEGORY_MOVEON }])
        }),
        ("terminated", "9.6.2.2-2 (d)", |st| {
            push_category(st, CATEGORY_MOVEON)
        }),
        ("passed", "9.6.3.2-2 (d)", |st| {
            st["context"]["extensions"]
                .as_object_mut()
                .unwrap()
                .remove(EXT_MASTERY);
        }),
        ("passed", "9.6.3.2-2 (d)", |st| {
            st["context"]["extensions"][EXT_MASTERY] = json!(0.8)
        }),
        ("allowed", "9.6.3.1-4 (d)", |st| {
            st["context"]["extensions"]
                .as_object_mut()
                .unwrap()
                .remove(EXT_SESSION);
        }),
        ("passed", "store", |_| {}),
        ("passed", "9.3.0.0-2 (d)", |_| {}),
        ("failed", "9.3.0.0-8 (d)", |_| {}),
    ];
    for (kind, requirement, alter) in table {
        let mut statement = match kind {
            "allowed" => au.prepare("http://adlnet.gov/expapi/verbs/experienced"),
            "initialized" => au.initialized(),
            "completed" => au.completed(),
            "passed" => au.passed(Some(full_score())),
            "failed" => au.failed(Some(low_score())),
            "terminated" => au.terminated(),
            _ => unreachable!(),
        };
        alter(&mut statement);
        if requirement == "store" {
            assert_eq!(au.put(&statement).await, StatusCode::NO_CONTENT, "{kind}");
        } else {
            au.refused(&statement, requirement).await;
        }
    }

    // After terminated nothing more is taken, and the token reads nothing.
    assert_eq!(au.put(&au.terminated()).await, StatusCode::NO_CONTENT);
    let late = au.prepare("http://adlnet.gov/expapi/verbs/experienced");
    let status = au.put(&late).await;
    assert!(
        status == StatusCode::FORBIDDEN || status == StatusCode::UNAUTHORIZED,
        "9.3.0.0-5 (d2): {status}"
    );
    au.unit
        .xapi(
            Method::PUT,
            &au.unit.state_path("bookmark"),
            Some(json!({})),
        )
        .await;
    let (status, _) = au.get_state("bookmark").await;
    assert!(!status.is_success(), "8.1.2.0-2 (d): {status}");
    learner.end(au.session_id()).await;
}

#[tokio::test]
async fn lts_005_2_after_failed_neither_failed_nor_passed_is_taken() {
    let name = "005-2-invalid-au";
    let learner = lts(name).await;
    let mut au = Au::launch(&learner, &au_id(name)).await;
    au.start().await;
    assert_eq!(au.put(&au.failed(None)).await, StatusCode::NO_CONTENT);
    au.refused(&au.failed(None), "9.3.0.0-2 (d)").await;
    au.refused(&au.passed(None), "9.3.0.0-3 (d)").await;
    assert_eq!(au.put(&au.terminated()).await, StatusCode::NO_CONTENT);
    learner.end(au.session_id()).await;
}

// ---- 007 multi-session, 008 abandoned ----------------------------------------

/// 007-1 and 007-2: the unit keeps a state document between two sessions of
/// one registration; the second session's repeats are refused.
/// A statement the unit builds.
type Build = fn(&Au) -> Value;

async fn second_session(name: &str, first: Build, again: &[(Build, &str)]) {
    let learner = lts(name).await;
    let state = name;

    let mut one = Au::launch(&learner, &au_id(name)).await;
    one.start().await;
    let (status, _) = one.get_state(state).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(one.put(&first(&one)).await, StatusCode::NO_CONTENT, "d1");
    let (status, _) = one
        .unit
        .xapi(
            Method::PUT,
            &one.unit.state_path(state),
            Some(json!({ "firstRun": true })),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(one.put(&one.terminated()).await, StatusCode::NO_CONTENT);
    learner.end(one.session_id()).await;

    let mut two = Au::launch(&learner, &au_id(name)).await;
    assert_eq!(two.unit.activity_id, one.unit.activity_id, "8.1.5.0-4");
    assert_eq!(two.unit.registration, one.unit.registration);
    two.start().await;
    let (status, stored) = two.get_state(state).await;
    assert_eq!(status, StatusCode::OK, "8.1.5.0-4");
    assert_eq!(stored, json!({ "firstRun": true }));
    for (statement, requirement) in again {
        two.refused(&statement(&two), requirement).await;
    }
    assert_eq!(two.put(&two.terminated()).await, StatusCode::NO_CONTENT);
    learner.end(two.session_id()).await;
}

#[tokio::test]
async fn lts_007_1_completed_is_taken_once_per_registration() {
    second_session(
        "007-1-multi-session",
        Au::completed,
        &[(Au::completed, "9.3.0.0-6 (d2)")],
    )
    .await;
}

#[tokio::test]
async fn lts_007_2_passed_is_taken_once_per_registration_and_failed_never_after_it() {
    second_session(
        "007-2-multi-session",
        |au| au.passed(None),
        &[
            (|au| au.passed(None), "9.3.0.0-7 (d2)"),
            (|au| au.failed(None), "9.3.0.0-8 (d)"),
        ],
    )
    .await;
}

#[tokio::test]
async fn lts_008_1_an_abandoned_session_is_recorded_and_takes_nothing_more() {
    let name = "008-1-abandoned";
    let learner = lts(name).await;
    let mut au = Au::launch(&learner, &au_id(name)).await;
    au.start().await;
    // The suite's abandonSession: the player leaves without terminated.
    learner.end(au.session_id()).await;

    // 9.3.6.0-2, 8.1.2.0-2 (d): nothing more is taken. The suite expects 401
    // or 403; the endpoint is gone instead (see the known deviation).
    let late = au.prepare("http://adlnet.gov/expapi/verbs/experienced");
    let sent = au
        .unit
        .client
        .put(format!(
            "{}statements?statementId={}",
            au.unit.endpoint,
            late["id"].as_str().unwrap()
        ))
        .header("Origin", package_serving::origin())
        .header("Authorization", au.auth())
        .json(&late)
        .send()
        .await;
    assert!(sent.is_err() || !sent.unwrap().status().is_success());

    let found = registration_statements(&learner, &au.unit.registration);
    let abandoned = &found[2];
    assert_eq!(verb(abandoned), ABANDONED, "9.3.6.0-1");
    assert_eq!(
        abandoned["context"]["extensions"][EXT_SESSION],
        json!(au.session_id()),
        "9.6.3.1-3"
    );
    assert!(
        ids(&abandoned["context"]["contextActivities"]["category"]).contains(&CATEGORY_CMI5),
        "9.6.2.1-1"
    );
    assert!(abandoned["result"]["duration"].is_string(), "9.5.4.2-1");
    assert_eq!(abandoned["actor"], au.unit.actor);
    assert_eq!(abandoned["object"]["id"], json!(au.unit.activity_id));
    assert_eq!(
        abandoned["context"]["registration"],
        json!(au.unit.registration)
    );
    assert_eq!(
        found.iter().filter(|s| verb(s) == ABANDONED).count(),
        1,
        "9.3.0.0-10"
    );
}

// ---- known deviations ----------------------------------------------------------

#[tokio::test]
async fn known_deviation_a_statement_without_an_id_or_timestamp_gets_one() {
    // 9.1.0.0-1 (d), 9.7.0.0-1 (d): the suite expects 403.
    let name = "005-1-invalid-au";
    let learner = lts(name).await;
    let mut au = Au::launch(&learner, &au_id(name)).await;
    au.start().await;
    let mut no_id = au.prepare("http://adlnet.gov/expapi/verbs/experienced");
    no_id.as_object_mut().unwrap().remove("id");
    assert_eq!(au.post(&no_id).await, StatusCode::OK);
    let mut no_timestamp = au.prepare("http://adlnet.gov/expapi/verbs/experienced");
    no_timestamp.as_object_mut().unwrap().remove("timestamp");
    assert_eq!(au.put(&no_timestamp).await, StatusCode::NO_CONTENT);
    learner.end(au.session_id()).await;

    let found = registration_statements(&learner, &au.unit.registration);
    let allowed: Vec<_> = found
        .iter()
        .filter(|s| verb(s) == "http://adlnet.gov/expapi/verbs/experienced")
        .collect();
    assert_eq!(allowed.len(), 2);
    assert!(allowed
        .iter()
        .all(|s| s["id"].is_string() && s["timestamp"].is_string()));
}

#[tokio::test]
async fn known_deviation_initialized_is_taken_before_the_learner_preferences_are_read() {
    // 11.0.0.0-3 (d2): the suite expects 403.
    let name = "005-1-invalid-au";
    let learner = lts(name).await;
    let mut au = Au::launch(&learner, &au_id(name)).await;
    au.fetch_and_load().await;
    assert_eq!(au.put(&au.initialized()).await, StatusCode::NO_CONTENT);
    learner.end(au.session_id()).await;
}

#[tokio::test]
async fn known_deviation_every_unit_is_launched_in_normal_mode() {
    // 006-launchMode needs a Browse or Review launch, which the app never
    // offers (10.2.2.0-2, 10.2.2.0-3); the rules for those modes are tested
    // in the studio crate.
    let name = "006-launchMode";
    let learner = lts(name).await;
    let mut au = Au::launch(&learner, &au_id(name)).await;
    au.start().await;
    assert_eq!(au.launch_data["launchMode"], "Normal");
    learner.end(au.session_id()).await;
    let found = registration_statements(&learner, &au.unit.registration);
    assert_eq!(found[0]["context"]["extensions"][EXT_LAUNCH_MODE], "Normal");
}

#[tokio::test]
async fn known_deviation_no_unit_is_ever_waived() {
    // 009-1-waived: the app has no way to waive a unit (9.3.7).
    let name = "009-1-waived";
    let learner = lts(name).await;
    let mut au = Au::launch(&learner, &format!("{}/0", au_id(name))).await;
    au.start().await;
    assert_eq!(au.put(&au.terminated()).await, StatusCode::NO_CONTENT);
    learner.end(au.session_id()).await;
    let found = registration_statements(&learner, &au.unit.registration);
    assert!(found
        .iter()
        .all(|s| verb(s) != "https://w3id.org/xapi/adl/verbs/waived"));
}

#[tokio::test]
async fn known_deviation_a_statement_after_terminated_is_refused_at_once() {
    // 9.3.8.0-4 allows a grace period after terminated; there is none.
    let name = "002-allowed";
    let learner = lts(name).await;
    let mut au = Au::launch(&learner, &au_id(name)).await;
    au.start().await;
    assert_eq!(au.put(&au.terminated()).await, StatusCode::NO_CONTENT);
    let late = au.prepare("http://adlnet.gov/expapi/verbs/experienced");
    assert_eq!(au.put(&late).await, StatusCode::FORBIDDEN);
    learner.end(au.session_id()).await;
}

#[tokio::test]
async fn known_deviation_the_endpoint_is_gone_once_the_session_ends() {
    // 008-1 expects 401 or 403 after the session is abandoned; the listener
    // has stopped, so the request fails to connect.
    let name = "008-1-abandoned";
    let learner = lts(name).await;
    let mut au = Au::launch(&learner, &au_id(name)).await;
    au.start().await;
    learner.end(au.session_id()).await;
    let after = au
        .unit
        .client
        .get(format!("{}about", au.unit.endpoint))
        .header("Origin", package_serving::origin())
        .send()
        .await;
    assert!(after.is_err(), "{after:?}");
}

#[tokio::test]
async fn known_deviation_statement_reads_never_page_and_voided_reads_find_nothing() {
    // Not a conformant record store (4.2.0.0-1, 6.0.0.0-1).
    let name = "002-allowed";
    let learner = lts(name).await;
    let mut au = Au::launch(&learner, &au_id(name)).await;
    au.start().await;
    let (status, page) = au.unit.xapi(Method::GET, "statements?limit=1", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["statements"].as_array().unwrap().len(), 1);
    assert_eq!(page["more"], "");
    let (status, _) = au
        .unit
        .xapi(
            Method::GET,
            &format!("statements?voidedStatementId={}", uuid::Uuid::new_v4()),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    learner.end(au.session_id()).await;
}

// ---- the matrix ------------------------------------------------------------------

/// Every test the matrix names, in «guillemets», exists in one of the test
/// files it draws on.
#[test]
fn every_row_of_the_cmi5_matrix_names_a_test_that_exists() {
    let root = manifest_dir().join("..");
    let matrix =
        std::fs::read_to_string(manifest_dir().join("src/packages/attempts/cmi5/conformance.md"))
            .expect("matrix");
    let sources: String = [
        "src-tauri/src/packages/attempts/cmi5/conformance.rs",
        "src-tauri/src/packages/attempts/cmi5/tests.rs",
        "crates/alexandria-studio/tests/cmi5_session.rs",
        "crates/alexandria-studio/tests/cmi5_import.rs",
        "crates/alexandria-studio/tests/package_import.rs",
        "tests/desktop-ui/cmi5-player.spec.ts",
    ]
    .iter()
    .map(|path| std::fs::read_to_string(root.join(path)).expect(path))
    .collect();
    let named: Vec<&str> = matrix
        .split('«')
        .skip(1)
        .filter_map(|rest| rest.split_once('»').map(|(name, _)| name))
        .collect();
    assert!(named.len() > 20, "the matrix names its tests");
    let missing: Vec<_> = named
        .iter()
        .filter(|name| {
            !sources.contains(&format!("fn {name}("))
                && !sources.contains(&format!("test('{name}'"))
        })
        .collect();
    assert!(
        missing.is_empty(),
        "named in the matrix, not found: {missing:?}"
    );
}
