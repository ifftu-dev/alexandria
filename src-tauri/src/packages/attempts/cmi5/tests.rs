//! Host-seam tests: the cmi5 fixture package from the player checks goes in
//! through `crate::packages::import`, an enrolment takes it, and each unit is
//! launched through [`launch`]. The unit's side is played over real HTTP to
//! the session's loopback endpoint, as the package frame would send it. What
//! comes out is what the unit, the learner and their records see.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ed25519_dalek::SigningKey;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};
use tempfile::TempDir;

use super::*;
use crate::content_store::node::ContentNode;
use crate::db::Database;
use crate::domain::package_attempt::{PackageAttempt, PackageAttemptStatus};
use crate::package_serving::{self, Launches};
use crate::packages::{self, PackageDirs};
use crate::settings::{registry::keys, SettingsStore};

const WALKAROUND: &str = "https://example.org/cmi5/forklift/walkaround";
const CHECK: &str = "https://example.org/cmi5/forklift/check";
const HANDBOOK: &str = "https://example.org/cmi5/forklift/handbook";
const BLOCK: &str = "https://example.org/cmi5/forklift/before-you-drive";
const COURSE: &str = "https://example.org/cmi5/forklift";

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/desktop-ui/package-player/cmi5/package")
}

fn fixture_files(dir: &Path, prefix: &str, out: &mut Vec<(String, Vec<u8>)>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .expect("fixture directory")
        .map(|entry| entry.expect("entry").path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        if path.is_dir() {
            fixture_files(&path, &format!("{prefix}{name}/"), out);
        } else {
            out.push((format!("{prefix}{name}"), std::fs::read(&path).unwrap()));
        }
    }
}

pub(super) fn did(seed: u8) -> String {
    crate::crypto::did::derive_did_key(&SigningKey::from_bytes(&[seed; 32]))
        .as_str()
        .to_string()
}

/// A learner enrolled in a course whose one element is the cmi5 fixture,
/// with the package launch open the way the player opens it.
pub(super) struct Learner {
    pub(super) db: Arc<Mutex<Database>>,
    pub(super) sessions: Arc<Cmi5Sessions>,
    pub(super) launches: Arc<Launches>,
    pub(super) key: [u8; 32],
    pub(super) attempt: PackageAttempt,
    pub(super) prefix: String,
    _node: ContentNode,
    _root: TempDir,
}

const ELEMENT: &str = "e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5";
const COURSE_ID: &str = "c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0";
const ENROLLMENT: &str = "enr1";

impl Learner {
    async fn new() -> Self {
        let mut files = Vec::new();
        fixture_files(&fixture_dir(), "", &mut files);
        Self::with_package(&files).await
    }

    /// A learner enrolled in a course whose one element is the package made
    /// of `files` (archive path, bytes).
    pub(super) async fn with_package(files: &[(String, Vec<u8>)]) -> Self {
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
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address, display_name) \
                 VALUES (1, 'stake_test1ulearner', 'addr_test1q', 'Ada Learner')",
                [],
            )
            .unwrap();
        SettingsStore::set(db.conn(), keys::IDENTITY_LOCAL_DID, did(1)).unwrap();
        db.conn()
            .execute(
                "INSERT INTO courses (id, title, author_address, status, content_cid, \
                 course_document_version, creator_did) \
                 VALUES (?1, 'Forklift', 'stake_test1uauthor', 'published', ?2, 2, ?3)",
                rusqlite::params![COURSE_ID, "11".repeat(32), did(9)],
            )
            .unwrap();
        db.conn()
            .execute_batch(&format!(
                "INSERT INTO course_chapters (id, course_id, title, position) VALUES ('ch1', '{COURSE_ID}', 'Chapter', 0); \
                 INSERT INTO enrollments (id, course_id, status, course_document_cid, course_document_version) \
                 VALUES ('{ENROLLMENT}', '{COURSE_ID}', 'active', '{doc}', 2);",
                doc = "11".repeat(32)
            ))
            .unwrap();
        let db = Arc::new(Mutex::new(db));

        let entries: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(name, data)| (name.as_str(), data.as_slice()))
            .collect();
        let source = root.path().join("forklift.zip");
        std::fs::write(&source, super::super::tests::fixture::zip(&entries)).unwrap();
        let package = packages::import(&*db, &dirs, &source)
            .await
            .expect("import");
        db.lock()
            .unwrap()
            .conn()
            .execute(
                "INSERT INTO course_elements (id, chapter_id, title, element_type, content_cid, position) \
                 VALUES (?1, 'ch1', 'Forklift basics', 'package', ?2, 0)",
                rusqlite::params![ELEMENT, package.hash],
            )
            .unwrap();
        let attempt = super::super::open(&*db, ENROLLMENT, ELEMENT)
            .await
            .expect("open attempt");
        let launches = Arc::new(Launches::new());
        let opened =
            package_serving::open_launch(&launches, &*db, &node, &dirs, &package.hash, |_| {})
                .await
                .expect("open launch");
        Self {
            db,
            sessions: Arc::new(Cmi5Sessions::new()),
            launches,
            key: [7; 32],
            attempt,
            prefix: opened.prefix,
            _node: node,
            _root: root,
        }
    }

    pub(super) async fn launch(&self, unit: &str) -> Result<Cmi5Launch, AttemptError> {
        launch(
            self.db.clone(),
            &self.sessions,
            &self.launches,
            &self.key,
            LaunchRequest {
                attempt_id: self.attempt.id.clone(),
                activity_id: unit.to_string(),
                prefix: self.prefix.clone(),
                languages: "en-US".into(),
            },
        )
        .await
    }

    pub(super) async fn unit(&self, unit: &str) -> Unit {
        Unit::from_launch(self.launch(unit).await.expect("launch"))
    }

    pub(super) async fn end(&self, session_id: &str) -> PackageAttemptProgress {
        end(&*self.db, &self.sessions, &self.launches, session_id)
            .await
            .expect("end session")
    }

    fn verbs(&self) -> Vec<(String, String)> {
        let db = self.db.lock().unwrap();
        let mut select = db
            .conn()
            .prepare("SELECT activity_id, COALESCE(verb, '') FROM cmi5_statements ORDER BY seq")
            .unwrap();
        let verbs = select
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        verbs
    }

    fn record_verbs(&self) -> Vec<String> {
        let db = self.db.lock().unwrap();
        let mut verbs: Vec<String> = crate::learning_records::list(db.conn())
            .expect("records")
            .into_iter()
            .map(|record| record.verb)
            .collect();
        verbs.reverse();
        verbs
    }
}

/// The unit's side of a session: what the launch URL gave it, over HTTP.
pub(super) struct Unit {
    pub(super) launch: Cmi5Launch,
    pub(super) client: reqwest::Client,
    pub(super) endpoint: String,
    pub(super) fetch: String,
    pub(super) actor: Value,
    pub(super) registration: String,
    pub(super) activity_id: String,
    pub(super) token: Option<String>,
    pub(super) template: Value,
    /// LMS.LaunchData `masteryScore`, once read.
    pub(super) mastery: Option<f64>,
}

impl Unit {
    pub(super) fn from_launch(launch: Cmi5Launch) -> Self {
        let url = url::Url::parse(&launch.url.replace("package://localhost", "http://package"))
            .expect("launch url parses");
        let param = |name: &str| {
            url.query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
                .unwrap_or_else(|| panic!("launch url has no {name}: {}", launch.url))
        };
        Self {
            endpoint: param("endpoint"),
            fetch: param("fetch"),
            actor: serde_json::from_str(&param("actor")).expect("actor is JSON"),
            registration: param("registration"),
            activity_id: param("activityId"),
            client: reqwest::Client::new(),
            token: None,
            template: Value::Null,
            mastery: None,
            launch,
        }
    }

    pub(super) async fn post_fetch(&self) -> Value {
        self.client
            .post(&self.fetch)
            .header("Origin", package_serving::origin())
            .send()
            .await
            .expect("fetch request")
            .json()
            .await
            .expect("fetch answers JSON")
    }

    /// Takes the token and reads LMS.LaunchData, as every unit starts.
    pub(super) async fn start(&mut self) -> Value {
        let answer = self.post_fetch().await;
        self.token = Some(answer["auth-token"].as_str().expect("token").to_string());
        let (status, data) = self
            .xapi(Method::GET, &self.state_path("LMS.LaunchData"), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{data}");
        self.template = data["contextTemplate"].clone();
        self.mastery = data["masteryScore"].as_f64();
        data
    }

    pub(super) fn state_path(&self, state_id: &str) -> String {
        format!(
            "activities/state?activityId={}&agent={}&registration={}&stateId={state_id}",
            urlencode(&self.activity_id),
            urlencode(&self.actor.to_string()),
            urlencode(&self.registration),
        )
    }

    pub(super) async fn xapi(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.endpoint))
            .header("Origin", package_serving::origin())
            .header("X-Experience-API-Version", "1.0.3");
        if let Some(token) = &self.token {
            request = request.header("Authorization", format!("Basic {token}"));
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.expect("request reaches the endpoint");
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }

    /// A cmi5 defined statement on the context template.
    fn statement(&self, verb: &str, result: Value) -> Value {
        let mut activities = self.template["contextActivities"].clone();
        let mut category = vec![json!({ "id": rules_category() })];
        if result.get("success").is_some() || result.get("completion").is_some() {
            category.push(json!({ "id": alexandria_studio::xapi::cmi5::CATEGORY_MOVEON }));
        }
        activities["category"] = json!(category);
        let mut statement = json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "actor": self.actor,
            "verb": { "id": format!("http://adlnet.gov/expapi/verbs/{verb}") },
            "object": { "objectType": "Activity", "id": self.activity_id },
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "context": {
                "registration": self.registration,
                "contextActivities": activities,
                "extensions": self.template["extensions"],
            },
        });
        if let (Some(mastery), false) = (self.mastery, result["score"].is_null()) {
            statement["context"]["extensions"]
                [alexandria_studio::xapi::cmi5::EXTENSION_MASTERY_SCORE] = json!(mastery);
        }
        if result.as_object().is_some_and(|r| !r.is_empty()) {
            statement["result"] = result;
        }
        statement
    }

    async fn send(&self, verb: &str, result: Value) -> StatusCode {
        let statement = self.statement(verb, result);
        self.xapi(Method::POST, "statements", Some(statement))
            .await
            .0
    }
}

fn rules_category() -> &'static str {
    alexandria_studio::xapi::cmi5::CATEGORY_CMI5
}

pub(super) fn urlencode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

#[tokio::test]
async fn a_unit_gets_the_cmi5_launch_parameters_and_its_launch_data() {
    let learner = Learner::new().await;
    let mut unit = learner.unit(WALKAROUND).await;

    // The unit's own query stays, the cmi5 parameters follow it.
    assert!(
        unit.launch
            .url
            .contains("/au/walkaround.html?paramA=1&endpoint="),
        "{}",
        unit.launch.url
    );
    assert!(unit.endpoint.starts_with("http://127.0.0.1:"));
    assert!(unit.endpoint.ends_with("/xapi/"));
    assert_eq!(
        unit.actor["account"]["name"],
        json!(progress::learner_id(
            &learner.key,
            &learner.attempt.package_hash
        ))
    );
    assert!(uuid::Uuid::parse_str(&unit.registration).is_ok());
    assert_ne!(unit.activity_id, WALKAROUND, "never the publisher id");
    assert!(
        unit.activity_id.starts_with(&did(9)),
        "{}",
        unit.activity_id
    );
    assert!(!unit.launch.own_window);

    let data = unit.start().await;
    assert_eq!(data["launchMode"], "Normal");
    assert_eq!(data["moveOn"], "Completed");
    assert_eq!(
        data["returnURL"],
        json!(format!(
            "{}/{}/{RETURN_PAGE}",
            package_serving::origin(),
            learner.prefix
        ))
    );
    assert_eq!(
        data["contextTemplate"]["extensions"][alexandria_studio::xapi::cmi5::EXTENSION_SESSION_ID],
        json!(unit.launch.session_id)
    );
    assert_eq!(
        data["contextTemplate"]["contextActivities"]["grouping"][0]["id"],
        WALKAROUND
    );

    let preferences = format!(
        "agents/profile?agent={}&profileId=cmi5LearnerPreferences",
        urlencode(&unit.actor.to_string())
    );
    let (status, preferences) = unit.xapi(Method::GET, &preferences, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preferences["languagePreference"], "en-US");

    // `launched` was recorded before the unit started, and the unit can read it.
    let (status, found) = unit
        .xapi(
            Method::GET,
            "statements?verb=http%3A%2F%2Fadlnet.gov%2Fexpapi%2Fverbs%2Flaunched",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let launched = &found["statements"][0];
    assert_eq!(launched["object"]["id"], json!(unit.activity_id));
    assert_eq!(
        launched["context"]["registration"],
        json!(unit.registration)
    );

    // Ending the session leaves the package launch itself open.
    learner.end(&unit.launch.session_id).await;
    assert!(learner.launches.package_of(&learner.prefix).is_some());
    assert!(!learner.sessions.is_open(&unit.launch.session_id));
}

#[tokio::test]
async fn the_package_completes_when_every_unit_meets_its_move_on_criteria() {
    let learner = Learner::new().await;

    let mut walkaround = learner.unit(WALKAROUND).await;
    walkaround.start().await;
    assert_eq!(
        walkaround.send("initialized", json!({})).await,
        StatusCode::OK
    );
    assert_eq!(
        walkaround
            .send(
                "completed",
                json!({ "completion": true, "duration": "PT2M" })
            )
            .await,
        StatusCode::OK
    );
    assert_eq!(
        walkaround
            .send("terminated", json!({ "duration": "PT2M30S" }))
            .await,
        StatusCode::OK
    );
    let after_walkaround = learner.end(&walkaround.launch.session_id).await;
    assert!(!after_walkaround.package_completed);

    let mut check = learner.unit(CHECK).await;
    let data = check.start().await;
    assert_eq!(data["masteryScore"], 0.8);
    assert_eq!(data["launchParameters"], "route=b");
    check.send("initialized", json!({})).await;
    assert_eq!(
        check
            .send(
                "passed",
                json!({ "success": true, "duration": "PT1M", "score": { "scaled": 0.9 } })
            )
            .await,
        StatusCode::OK
    );
    check
        .send("terminated", json!({ "duration": "PT1M5S" }))
        .await;
    let done = learner.end(&check.launch.session_id).await;

    assert!(done.package_completed);
    assert_eq!(done.attempt.status, PackageAttemptStatus::Completed);
    assert_eq!(done.element_progress.status, "completed");
    assert_eq!(done.element_progress.score, Some(0.9));
    assert_eq!(done.element_progress.time_spent, 215);

    // The block was satisfied by the check's session, then the course; the
    // handbook (NotApplicable) never had to be launched.
    let verbs = learner.verbs();
    let satisfied: Vec<_> = verbs
        .iter()
        .filter(|(_, verb)| verb == "satisfied")
        .map(|(activity, _)| activity.as_str())
        .collect();
    assert_eq!(satisfied, [BLOCK, COURSE]);
    let records = learner.record_verbs();
    for verb in ["launched", "completed", "passed", "satisfied"] {
        assert!(records.iter().any(|v| v == verb), "{verb}: {records:?}");
    }
    assert!(!verbs.iter().any(|(activity, _)| activity == HANDBOOK));
}

#[tokio::test]
async fn the_fetch_url_hands_out_the_token_once() {
    let learner = Learner::new().await;
    let unit = learner.unit(WALKAROUND).await;

    let first = unit.post_fetch().await;
    let token = first["auth-token"].as_str().expect("token the first time");
    assert_eq!(token.len(), 64);

    let second = unit.post_fetch().await;
    assert_eq!(second["error-code"], "1");
    assert!(second.get("auth-token").is_none());

    // A GET is not a fetch.
    let get = unit
        .client
        .get(&unit.fetch)
        .header("Origin", package_serving::origin())
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn a_statement_breaking_the_session_rules_is_refused_and_not_stored() {
    let learner = Learner::new().await;
    let mut unit = learner.unit(WALKAROUND).await;
    unit.start().await;

    // Nothing before `initialized`.
    let early = unit
        .send(
            "completed",
            json!({ "completion": true, "duration": "PT1M" }),
        )
        .await;
    assert_eq!(early, StatusCode::FORBIDDEN);
    assert_eq!(unit.send("initialized", json!({})).await, StatusCode::OK);
    // A second `initialized` in one session.
    assert_eq!(
        unit.send("initialized", json!({})).await,
        StatusCode::FORBIDDEN
    );
    // A batch is taken whole or not at all: the good statement goes with the bad.
    let good = unit.statement(
        "completed",
        json!({ "completion": true, "duration": "PT1M" }),
    );
    let mut bad = unit.statement("terminated", json!({ "duration": "PT1M" }));
    bad["context"]["extensions"][alexandria_studio::xapi::cmi5::EXTENSION_SESSION_ID] =
        json!("someone else's session");
    let (status, _) = unit
        .xapi(Method::POST, "statements", Some(json!([good, bad])))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // A unit may not void.
    let mut voiding = unit.statement("completed", json!({}));
    voiding["verb"]["id"] = json!("http://adlnet.gov/expapi/verbs/voided");
    voiding["object"] =
        json!({ "objectType": "StatementRef", "id": uuid::Uuid::new_v4().to_string() });
    let (status, _) = unit.xapi(Method::POST, "statements", Some(voiding)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // The unit may not rewrite its launch data.
    let (status, _) = unit
        .xapi(
            Method::PUT,
            &unit.state_path("LMS.LaunchData"),
            Some(json!({ "moveOn": "NotApplicable" })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let unit_verbs: Vec<_> = learner
        .verbs()
        .into_iter()
        .filter(|(activity, _)| activity == WALKAROUND)
        .map(|(_, verb)| verb)
        .collect();
    assert_eq!(unit_verbs, ["launched", "initialized"]);
    learner.end(&unit.launch.session_id).await;
}

#[tokio::test]
async fn a_session_the_unit_never_terminated_is_abandoned_once() {
    let learner = Learner::new().await;
    let mut unit = learner.unit(WALKAROUND).await;
    unit.start().await;
    unit.send("initialized", json!({})).await;

    let ended = learner.end(&unit.launch.session_id).await;
    assert!(!ended.package_completed);
    // Ending again writes nothing more.
    learner.end(&unit.launch.session_id).await;

    let abandoned: Vec<_> = learner
        .verbs()
        .into_iter()
        .filter(|(_, verb)| verb == "abandoned")
        .collect();
    assert_eq!(abandoned.len(), 1);
    assert!(learner.record_verbs().iter().any(|v| v == "abandoned"));

    // A terminated session is not abandoned.
    let mut again = learner.unit(WALKAROUND).await;
    again.start().await;
    again.send("initialized", json!({})).await;
    again
        .send("terminated", json!({ "duration": "PT5S" }))
        .await;
    learner.end(&again.launch.session_id).await;
    let abandoned = learner
        .verbs()
        .into_iter()
        .filter(|(_, verb)| verb == "abandoned")
        .count();
    assert_eq!(abandoned, 1);
}

#[tokio::test]
async fn a_session_left_open_when_the_app_stopped_is_abandoned_at_the_next_launch() {
    let learner = Learner::new().await;
    let mut unit = learner.unit(WALKAROUND).await;
    unit.start().await;
    unit.send("initialized", json!({})).await;

    // The profile stops: endpoints go, the session stays open in the database.
    learner.sessions.close_all();
    assert!(learner.verbs().iter().all(|(_, verb)| verb != "abandoned"));

    let next = learner.unit(CHECK).await;
    let verbs = learner.verbs();
    let abandoned: Vec<_> = verbs
        .iter()
        .filter(|(_, verb)| verb == "abandoned")
        .map(|(activity, _)| activity.as_str())
        .collect();
    assert_eq!(abandoned, [WALKAROUND]);
    learner.end(&next.launch.session_id).await;
}

#[tokio::test]
async fn launching_again_in_the_registration_abandons_the_open_session() {
    let learner = Learner::new().await;
    let mut first = learner.unit(WALKAROUND).await;
    first.start().await;
    first.send("initialized", json!({})).await;

    let second = learner.unit(WALKAROUND).await;
    assert!(!learner.sessions.is_open(&first.launch.session_id));
    assert!(learner.sessions.is_open(&second.launch.session_id));
    assert_eq!(
        learner
            .verbs()
            .iter()
            .filter(|(_, verb)| verb == "abandoned")
            .count(),
        1
    );
    learner.end(&second.launch.session_id).await;
}

#[tokio::test]
async fn the_endpoint_answers_only_its_own_open_session() {
    let learner = Learner::new().await;
    let mut unit = learner.unit(WALKAROUND).await;
    unit.start().await;

    // Another origin, even holding the token.
    let foreign = unit
        .client
        .post(format!("{}statements", unit.endpoint))
        .header("Origin", "https://evil.example")
        .header(
            "Authorization",
            format!("Basic {}", unit.token.as_deref().unwrap()),
        )
        .header("X-Experience-API-Version", "1.0.3")
        .json(&unit.statement("initialized", json!({})))
        .send()
        .await
        .unwrap();
    assert_eq!(foreign.status(), StatusCode::FORBIDDEN);
    assert!(foreign
        .headers()
        .get("access-control-allow-origin")
        .is_none());
    // No Origin at all.
    let bare = unit
        .client
        .get(format!("{}about", unit.endpoint))
        .send()
        .await
        .unwrap();
    assert_eq!(bare.status(), StatusCode::FORBIDDEN);
    // A forged Host (DNS rebinding).
    let rebound = unit
        .client
        .get(format!("{}about", unit.endpoint))
        .header("Origin", package_serving::origin())
        .header("Host", "attacker.example")
        .send()
        .await
        .unwrap();
    assert_eq!(rebound.status(), StatusCode::MISDIRECTED_REQUEST);
    // A wrong token, readable by the content.
    let mut stranger = Unit::from_launch(unit.launch.clone());
    stranger.token = Some("0".repeat(64));
    stranger.template = unit.template.clone();
    let (status, _) = stranger
        .xapi(
            Method::POST,
            "statements",
            Some(unit.statement("initialized", json!({}))),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // Another agent's documents.
    let other_agent = alexandria_studio::xapi::cmi5::actor("ffffffffffffffffffffffffffffffff");
    let (status, _) = unit
        .xapi(
            Method::GET,
            &format!(
                "agents/profile?agent={}&profileId=cmi5LearnerPreferences",
                urlencode(&other_agent.to_string())
            ),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Once the session ends nothing answers on its address.
    learner.end(&unit.launch.session_id).await;
    let after = unit
        .client
        .get(format!("{}about", unit.endpoint))
        .header("Origin", package_serving::origin())
        .send()
        .await;
    assert!(after.is_err(), "the listener is gone: {after:?}");
}

#[tokio::test]
async fn documents_keep_concurrency_control_and_merge_on_post() {
    let learner = Learner::new().await;
    let mut unit = learner.unit(WALKAROUND).await;
    unit.start().await;
    let bookmark = unit.state_path("bookmark");

    let (status, _) = unit
        .xapi(Method::PUT, &bookmark, Some(json!({ "page": 1 })))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = unit
        .xapi(Method::POST, &bookmark, Some(json!({ "seen": [1] })))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, stored) = unit.xapi(Method::GET, &bookmark, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored, json!({ "page": 1, "seen": [1] }));

    let profile = format!(
        "activities/profile?activityId={}&profileId=notes",
        urlencode(&unit.activity_id)
    );
    unit.xapi(Method::PUT, &profile, Some(json!({ "a": 1 })))
        .await;
    let (status, _) = unit
        .xapi(Method::PUT, &profile, Some(json!({ "a": 2 })))
        .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a profile PUT needs a precondition"
    );
    learner.end(&unit.launch.session_id).await;
}

#[tokio::test]
async fn a_unit_asking_for_its_own_window_gets_one_on_desktop() {
    let learner = Learner::new().await;
    let handbook = learner.launch(HANDBOOK).await.expect("launch");
    assert_eq!(handbook.own_window, OWN_WINDOWS);
    assert_eq!(handbook.focus_forced, !OWN_WINDOWS);
    learner.end(&handbook.session_id).await;
}

#[test]
fn cmi5_parameters_join_the_units_own_query_before_its_fragment() {
    assert_eq!(merge_query(None, "a=1"), "?a=1");
    assert_eq!(merge_query(Some("?x=2"), "a=1"), "?x=2&a=1");
    assert_eq!(merge_query(Some("?x=2#top"), "a=1"), "?x=2&a=1#top");
    assert_eq!(merge_query(Some("#top"), "a=1"), "?a=1#top");
}

#[test]
fn a_unit_window_stays_under_its_own_launch() {
    let origin = package_serving::origin();
    let prefix = "0123456789abcdef0123456789abcdef";
    let at = |text: &str| url::Url::parse(text).expect("url");
    let allowed = [
        format!("{origin}/{prefix}/au/index.html?endpoint=x"),
        format!("{origin}/{prefix}/au/next.html#part"),
        format!("{origin}/{prefix}/{RETURN_PAGE}"),
    ];
    for target in &allowed {
        assert!(unit_window_may_navigate(prefix, &at(target)), "{target}");
    }
    let refused = [
        "https://example.org/phish".to_string(),
        "http://127.0.0.1:41234/xapi/statements".to_string(),
        "data:text/html,<p>hi</p>".to_string(),
        "about:blank".to_string(),
        format!("{origin}/fedcba9876543210fedcba9876543210/au/index.html"),
        format!("{origin}/{prefix}"),
        format!("{origin}/{prefix}/../fedcba9876543210fedcba9876543210/x.html"),
        format!("{origin}.evil.example/{prefix}/au/index.html"),
        "tauri://localhost/".to_string(),
    ];
    for target in &refused {
        assert!(!unit_window_may_navigate(prefix, &at(target)), "{target}");
    }
}
