//! Host-seam tests: a learner with learning records approves record
//! destinations, the background sender's pass runs, and what comes out is
//! what a small in-process xAPI store received, the destinations' status
//! and the access log.

use std::sync::{Arc, Mutex};

use alexandria_studio::xapi::{
    self, ActivityRef, Interaction, InteractionType, LearningEvent, Outcome, Profile,
};
use chrono::{DateTime, Duration, TimeZone, Utc};
use ed25519_dalek::SigningKey;
use serde_json::Value;

use super::access_log::{self, AccessAction};
use super::destinations;
use super::xapi_stub::XapiStub;
use super::*;
use crate::crypto::hash::entity_id;
use crate::db::Database;
use crate::domain::enrollment::UpdateProgressRequest;
use crate::domain::record_sending::{IdentifierChoice, NewDestinationRequest, RecordDestination};
use crate::settings::{registry::keys, SettingsStore};

pub(super) const LEARNER_SEED: u8 = 1;
const CREATOR_SEED: u8 = 9;

pub(super) fn did(seed: u8) -> String {
    crate::crypto::did::derive_did_key(&SigningKey::from_bytes(&[seed; 32]))
        .as_str()
        .to_string()
}

pub(super) fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 20, 9, 0, 0).unwrap()
}

/// A learner enrolled in two courses, with learning records written by the
/// real host functions: a registration in each, and in the first an element
/// launched and completed (which also records course progress).
pub(super) struct Learner {
    pub(super) handle: Arc<Mutex<Option<Database>>>,
    pub(super) db: InboundDatabase,
    pub(super) secret: [u8; 32],
    pub(super) did: String,
    pub(super) course: String,
    pub(super) other_course: String,
    pub(super) video: String,
}

impl Learner {
    pub(super) async fn new() -> Self {
        let database = Database::open_in_memory().expect("in-memory db");
        database.run_migrations().expect("migrations");
        let learner_did = did(LEARNER_SEED);
        let course = entity_id(&["course"]);
        let other_course = entity_id(&["other course"]);
        let video = entity_id(&["video"]);
        {
            let conn = database.conn();
            conn.execute(
                "INSERT INTO local_identity (id, stake_address, payment_address, display_name) \
                 VALUES (1, 'stake_test1ulearner', 'addr_test1q', 'Ada')",
                [],
            )
            .unwrap();
            SettingsStore::set(conn, keys::IDENTITY_LOCAL_DID, learner_did.clone()).unwrap();
            for (id, title, chapter) in [
                (&course, "Warehouse safety", "ch1"),
                (&other_course, "Forklift basics", "ch2"),
            ] {
                conn.execute(
                    "INSERT INTO courses (id, title, author_address, status, content_cid, \
                     course_document_version, creator_did) \
                     VALUES (?1, ?2, 'stake_test1uauthor', 'published', ?3, 2, ?4)",
                    rusqlite::params![id, title, "11".repeat(32), did(CREATOR_SEED)],
                )
                .unwrap();
                conn.execute(
                    "INSERT INTO course_chapters (id, course_id, title, position) \
                     VALUES (?1, ?2, 'Chapter', 0)",
                    rusqlite::params![chapter, id],
                )
                .unwrap();
            }
            conn.execute(
                "INSERT INTO course_elements (id, chapter_id, title, element_type, position) \
                 VALUES (?1, 'ch1', 'Lifting', 'video', 0)",
                [&video],
            )
            .unwrap();
        }
        let enrollment = crate::enrollment::enroll(&database, &course).unwrap().id;
        crate::enrollment::enroll(&database, &other_course).unwrap();
        for status in ["in_progress", "completed"] {
            crate::enrollment::update_progress(
                &database,
                enrollment.clone(),
                UpdateProgressRequest {
                    element_id: video.clone(),
                    status: status.to_string(),
                    score: None,
                    time_spent: None,
                    integrity_session_id: None,
                    integrity_score: None,
                },
            )
            .unwrap();
        }
        let handle = Arc::new(Mutex::new(Some(database)));
        let db = InboundDatabase::detached(handle.clone()).await.unwrap();
        Self {
            handle,
            db,
            secret: [LEARNER_SEED; 32],
            did: learner_did,
            course,
            other_course,
            video,
        }
    }

    pub(super) fn with_conn<T>(&self, f: impl FnOnce(&rusqlite::Connection) -> T) -> T {
        let guard = self.handle.lock().unwrap();
        f(guard.as_ref().unwrap().conn())
    }

    /// Local ids of the records in `course`, oldest first.
    pub(super) fn record_ids(&self, course: &str) -> Vec<String> {
        self.with_conn(|conn| {
            let mut ids: Vec<String> = crate::learning_records::list(conn)
                .unwrap()
                .into_iter()
                .filter(|r| r.course_id == course)
                .map(|r| r.id)
                .collect();
            ids.reverse();
            ids
        })
    }

    /// A response a package reported, as the package writer stores it.
    pub(super) fn add_interaction_record(&self) -> String {
        self.add_numbered_interaction_record(0)
    }

    /// The `n`th response a package reported in the attempt.
    pub(super) fn add_numbered_interaction_record(&self, n: usize) -> String {
        let activity = ActivityRef::course(did(CREATOR_SEED), &self.course)
            .element(&self.video)
            .package_activity("ab".repeat(32), "lifting")
            .interaction("q1");
        let event = LearningEvent {
            local_id: format!("package|attempt|lifting|interaction|{n}"),
            timestamp: t0(),
            registration: Some("attempt".into()),
            action: xapi::Action::Responded {
                interaction: Interaction::new(activity.clone(), InteractionType::FillIn),
                outcome: Outcome {
                    response: Some("bend the knees".into()),
                    ..Outcome::default()
                },
            },
            profile: Some(Profile::Scorm),
        };
        let json = serde_json::to_string(&event).unwrap();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO learning_records (id, learner_did, kind, verb, course_id, element_id, \
                 registration, activity_id, event_json, bytes, recorded_at) \
                 VALUES (?1, ?2, 'interaction', 'responded', ?3, ?4, 'attempt', ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    event.local_id,
                    self.did,
                    self.course,
                    self.video,
                    activity.iri().unwrap(),
                    json,
                    json.len() as i64,
                    "2026-11-20T09:00:00.000Z"
                ],
            )
            .unwrap();
        });
        event.local_id
    }

    fn request(&self, stub: &XapiStub, username: &str, courses: &[&str]) -> NewDestinationRequest {
        NewDestinationRequest {
            address: stub.address(),
            username: username.into(),
            secret: "store-issued-secret".into(),
            course_ids: courses.iter().map(|c| c.to_string()).collect(),
            include_interactions: false,
            identifier: IdentifierChoice::Pairwise,
            confirm_private: true,
        }
    }

    async fn add(&self, request: NewDestinationRequest) -> Result<RecordDestination, String> {
        destinations::add(&self.db, request, &self.secret, &self.did, t0())
            .await
            .map_err(|e| e.to_string())
    }

    async fn pass(&self, at: DateTime<Utc>) -> BatchOutcome {
        destinations::send_pending(&self.db, &self.secret, &self.did, at)
            .await
            .expect("pass")
    }

    fn destinations(&self) -> Vec<RecordDestination> {
        self.with_conn(|conn| destinations::list(conn).unwrap())
    }

    pub(super) fn access_log(&self) -> Vec<access_log::AccessLogEntry> {
        self.with_conn(|conn| access_log::list(conn).unwrap())
    }
}

/// The recipient's statement id for a local record id, worked out the way
/// the destination's label and approval prescribe.
fn statement_id(learner: &Learner, endpoint: &str, username: &str, local_id: &str) -> String {
    let label = destinations::label(endpoint, username);
    let approval = learner
        .destinations()
        .into_iter()
        .find(|d| d.endpoint == endpoint && d.username == username)
        .expect("the destination is approved")
        .id;
    // Its identifier key is derived from the instance label alone.
    xapi::Recipient::pairwise(
        &learner.secret,
        &recipients::instance_label(&label, &approval),
    )
    .unwrap()
    .statement_id(local_id)
}

pub(super) fn actor_names(statements: &[Value]) -> Vec<String> {
    statements
        .iter()
        .map(|s| s["actor"]["account"]["name"].as_str().unwrap().to_string())
        .collect()
}

pub(super) fn ids(statements: &[Value]) -> Vec<String> {
    statements
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn nothing_is_sent_before_the_learner_approves() {
    let learner = Learner::new().await;
    let store = XapiStub::start(&["1.0.3", "2.0.0"]).await;

    let preview = destinations::preview(
        &learner.db,
        &store.address(),
        "learner-key",
        &learner.secret,
        &learner.did,
    )
    .await
    .unwrap();
    learner.pass(t0()).await;
    assert!(
        store.requests().is_empty(),
        "the preview and a pass before approval contacted the store"
    );
    assert_eq!(preview.endpoint, store.address());
    assert_eq!(preview.location, "device");
    assert!(preview.needs_confirmation);
    let titles: Vec<_> = preview.courses.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, vec!["Forklift basics", "Warehouse safety"]);

    learner
        .add(learner.request(&store, "learner-key", &[&learner.course]))
        .await
        .unwrap();
    assert!(
        store.statement_posts().is_empty(),
        "approval itself sends no records"
    );
    learner.pass(t0()).await;

    let course_records = learner.record_ids(&learner.course);
    assert_eq!(course_records.len(), 4);
    let expected: Vec<String> = course_records
        .iter()
        .map(|id| statement_id(&learner, &store.address(), "learner-key", id))
        .collect();
    assert_eq!(ids(&store.accepted()), expected);
    assert_eq!(
        actor_names(&store.accepted())[0],
        preview.pairwise_actor,
        "records carry the identifier the consent screen showed"
    );
}

#[tokio::test]
async fn a_private_or_local_address_needs_explicit_confirmation() {
    let learner = Learner::new().await;
    let store = XapiStub::start(&["2.0.0"]).await;
    let mut request = learner.request(&store, "learner-key", &[&learner.course]);
    request.confirm_private = false;

    let refused = learner.add(request).await.unwrap_err();
    assert!(refused.starts_with("confirmation_required:"), "{refused}");
    assert!(store.requests().is_empty());
    assert!(learner.destinations().is_empty());
}

#[tokio::test]
async fn the_version_is_negotiated_and_the_learners_credentials_are_used() {
    let learner = Learner::new().await;
    let store = XapiStub::start(&["1.0.0", "1.0.3"]).await;
    let added = learner
        .add(learner.request(&store, "learner-key", &[&learner.course]))
        .await
        .unwrap();
    assert_eq!(added.xapi_version, "1.0.3");
    learner.pass(t0()).await;

    let posts = store.statement_posts();
    assert_eq!(posts.len(), 1, "four records go in one batch");
    let post = &posts[0];
    assert_eq!(post.headers["x-experience-api-version"], "1.0.3");
    use base64::Engine;
    let expected =
        base64::engine::general_purpose::STANDARD.encode("learner-key:store-issued-secret");
    assert_eq!(post.headers["authorization"], format!("Basic {expected}"));

    let listed = serde_json::to_string(&learner.destinations()).unwrap();
    assert!(
        !listed.contains("store-issued-secret"),
        "the secret is never returned"
    );
}

#[tokio::test]
async fn only_the_courses_and_kinds_in_scope_are_sent() {
    let learner = Learner::new().await;
    let interaction = learner.add_interaction_record();
    let completion_only = XapiStub::start(&["2.0.0"]).await;
    let with_responses = XapiStub::start(&["2.0.0"]).await;
    learner
        .add(learner.request(&completion_only, "learner-key", &[&learner.course]))
        .await
        .unwrap();
    let mut request = learner.request(&with_responses, "learner-key", &[&learner.course]);
    request.include_interactions = true;
    learner.add(request).await.unwrap();

    learner.pass(t0()).await;

    let verbs = |store: &XapiStub| -> Vec<String> {
        store
            .accepted()
            .iter()
            .map(|s| {
                s["verb"]["id"]
                    .as_str()
                    .unwrap()
                    .rsplit('/')
                    .next()
                    .unwrap()
                    .to_string()
            })
            .collect()
    };
    assert_eq!(
        verbs(&completion_only),
        vec!["registered", "launched", "completed", "progressed"],
        "completion-level only, and nothing from the course outside the scope"
    );
    assert_eq!(verbs(&with_responses).len(), 5);
    let responded = statement_id(
        &learner,
        &with_responses.address(),
        "learner-key",
        &interaction,
    );
    assert!(ids(&with_responses.accepted()).contains(&responded));
    let other = learner.record_ids(&learner.other_course);
    assert_eq!(other.len(), 1);
    for store in [&completion_only, &with_responses] {
        let sent = ids(&store.accepted());
        assert!(!sent.contains(&statement_id(
            &learner,
            &store.address(),
            "learner-key",
            &other[0]
        )));
    }
}

#[tokio::test]
async fn each_destination_sees_identifiers_made_only_for_it_unless_the_learner_chooses_their_did() {
    let learner = Learner::new().await;
    let first = XapiStub::start(&["2.0.0"]).await;
    let second = XapiStub::start(&["2.0.0"]).await;
    let real = XapiStub::start(&["2.0.0"]).await;
    learner
        .add(learner.request(&first, "learner-key", &[&learner.course]))
        .await
        .unwrap();
    learner
        .add(learner.request(&second, "learner-key", &[&learner.course]))
        .await
        .unwrap();
    let mut request = learner.request(&real, "learner-key", &[&learner.course]);
    request.identifier = IdentifierChoice::RealDid;
    let chosen = learner.add(request).await.unwrap();

    learner.pass(t0()).await;

    let learner_name = learner.did.strip_prefix("did:key:").unwrap().to_string();
    let (a, b, c) = (first.accepted(), second.accepted(), real.accepted());
    assert_eq!(a.len(), 4);
    let actor = |s: &[Value]| actor_names(s)[0].clone();
    assert_ne!(actor(&a), actor(&b));
    assert_ne!(actor(&a), learner_name);
    assert_ne!(actor(&b), learner_name);
    assert_eq!(actor(&c), learner_name);
    assert_eq!(chosen.actor_name, learner_name);
    assert_eq!(chosen.identifier, "real_did");
    let registrations = |s: &[Value]| -> Vec<String> {
        s.iter()
            .filter_map(|s| s["context"]["registration"].as_str().map(str::to_string))
            .collect()
    };
    for (one, other) in [(&a, &b), (&a, &c), (&b, &c)] {
        assert!(ids(one).iter().all(|id| !ids(other).contains(id)));
        assert!(registrations(one)
            .iter()
            .all(|r| !registrations(other).contains(r)));
    }
}

#[tokio::test]
async fn records_queued_while_the_store_is_unreachable_are_sent_when_it_is_back() {
    let learner = Learner::new().await;
    let mut store = XapiStub::start(&["2.0.0"]).await;
    learner
        .add(learner.request(&store, "learner-key", &[&learner.course]))
        .await
        .unwrap();
    store.stop().await;

    let offline = learner.pass(t0()).await;
    assert_eq!(offline.waiting, 4);
    let status = learner.destinations()[0].status.clone();
    assert_eq!((status.queued, status.failed, status.sent), (4, 0, 0));
    assert!(status.last_error.unwrap().starts_with("unreachable:"));
    assert!(status.next_attempt_at.is_some());
    assert!(learner.access_log().is_empty(), "nothing left the device");

    store.restart().await;
    learner.pass(t0() + Duration::seconds(5)).await;
    assert!(
        store.accepted().is_empty(),
        "retried only after the backoff"
    );

    let back = learner.pass(t0() + Duration::minutes(5)).await;
    assert_eq!(back.sent, 4);
    assert_eq!(store.accepted().len(), 4);
    let status = learner.destinations()[0].status.clone();
    assert_eq!((status.queued, status.failed, status.sent), (0, 0, 4));
    assert_eq!(status.last_error, None);
}

#[tokio::test]
async fn a_refused_send_is_visible_and_retried() {
    let learner = Learner::new().await;
    let store = XapiStub::start(&["2.0.0"]).await;
    learner
        .add(learner.request(&store, "learner-key", &[&learner.course]))
        .await
        .unwrap();
    store.refuse(401, "{\"error\":\"bad credentials\"}");

    let refused = learner.pass(t0()).await;
    assert_eq!(refused.failed, 4);
    let status = learner.destinations()[0].status.clone();
    assert_eq!(status.failed, 4);
    let error = status.last_error.unwrap();
    assert!(
        error.contains("401") && error.contains("bad credentials"),
        "{error}"
    );
    let failures = learner.with_conn(|conn| outbox::failures(conn).unwrap());
    assert_eq!(failures.len(), 4);
    let log = learner.access_log();
    assert_eq!(log.len(), 1);
    assert!(!log[0].accepted);
    assert_eq!(log[0].record_count, 4);

    store.accept_all();
    learner.pass(t0() + Duration::seconds(1)).await;
    assert!(store.accepted().is_empty(), "not retried before its time");
    learner.pass(t0() + Duration::minutes(1)).await;
    assert_eq!(store.accepted().len(), 4);
    assert_eq!(learner.destinations()[0].status.failed, 0);
}

#[tokio::test]
async fn one_refused_statement_does_not_hold_back_the_rest_of_its_batch() {
    let learner = Learner::new().await;
    let store = XapiStub::start(&["2.0.0"]).await;
    learner
        .add(learner.request(&store, "learner-key", &[&learner.course]))
        .await
        .unwrap();
    store.refuse_containing("verbs/launched");

    let outcome = learner.pass(t0()).await;
    assert_eq!((outcome.sent, outcome.failed), (3, 1));
    let status = learner.destinations()[0].status.clone();
    assert_eq!((status.sent, status.failed), (3, 1));
}

#[tokio::test]
async fn every_send_is_written_to_the_access_log() {
    let learner = Learner::new().await;
    let store = XapiStub::start(&["2.0.0"]).await;
    let added = learner
        .add(learner.request(&store, "learner-key", &[&learner.course]))
        .await
        .unwrap();
    learner.pass(t0()).await;

    let log = learner.access_log();
    assert_eq!(log.len(), 1);
    let entry = &log[0];
    assert_eq!(entry.action, AccessAction::Send);
    assert_eq!(entry.recipient_kind, RecipientKind::Destination);
    assert_eq!(entry.recipient_name, store.address());
    assert_eq!(entry.record_count, 4);
    assert_eq!(entry.identifier, added.actor_name);
    assert_eq!(entry.scope.course_ids, vec![learner.course.clone()]);
    assert!(!entry.scope.include_interactions);
    assert!(entry.accepted);
    assert_eq!(entry.at, "2026-11-20T09:00:00.000Z");
}

#[tokio::test]
async fn removing_a_destination_stops_sending_and_voids_what_it_was_sent() {
    let learner = Learner::new().await;
    let store = XapiStub::start(&["2.0.0"]).await;
    let added = learner
        .add(learner.request(&store, "learner-key", &[&learner.course]))
        .await
        .unwrap();
    learner.pass(t0()).await;
    let sent = ids(&store.accepted());
    assert_eq!(sent.len(), 4);

    let report = destinations::remove(&learner.db, &added.id, &learner.secret, &learner.did, t0())
        .await
        .unwrap();
    assert_eq!(report.voided, 4);
    assert_eq!(report.void_error, None);
    let voids: Vec<Value> = store.accepted()[4..].to_vec();
    let voided: Vec<String> = voids
        .iter()
        .map(|s| {
            assert_eq!(s["verb"]["id"], "http://adlnet.gov/expapi/verbs/voided");
            s["object"]["id"].as_str().unwrap().to_string()
        })
        .collect();
    assert_eq!(voided, sent);
    assert!(learner.destinations().is_empty());
    let log = learner.access_log();
    assert_eq!(log[0].action, AccessAction::Void);
    assert_eq!(log[0].record_count, 4);
    assert!(log[0].accepted);

    // A record written after removal goes nowhere.
    learner.add_interaction_record();
    let posts_before = store.statement_posts().len();
    learner.pass(t0() + Duration::hours(1)).await;
    assert_eq!(store.statement_posts().len(), posts_before);
}

#[tokio::test]
async fn removal_stops_sending_even_when_voiding_cannot_reach_the_store() {
    let learner = Learner::new().await;
    let mut store = XapiStub::start(&["2.0.0"]).await;
    let added = learner
        .add(learner.request(&store, "learner-key", &[&learner.course]))
        .await
        .unwrap();
    learner.pass(t0()).await;
    store.stop().await;

    let report = destinations::remove(&learner.db, &added.id, &learner.secret, &learner.did, t0())
        .await
        .unwrap();
    assert_eq!(report.voided, 0);
    assert!(report.void_error.unwrap().starts_with("unreachable:"));
    assert!(learner.destinations().is_empty());
}

#[tokio::test]
async fn a_store_added_again_after_removal_gets_fresh_statement_ids() {
    let learner = Learner::new().await;
    let store = XapiStub::start(&["2.0.0"]).await;
    let first = learner
        .add(learner.request(&store, "learner-key", &[&learner.course]))
        .await
        .unwrap();
    learner.pass(t0()).await;
    let sent = ids(&store.accepted());
    destinations::remove(&learner.db, &first.id, &learner.secret, &learner.did, t0())
        .await
        .unwrap();
    let after_voids = store.accepted().len();

    learner
        .add(learner.request(&store, "learner-key", &[&learner.course]))
        .await
        .unwrap();
    learner.pass(t0() + Duration::hours(1)).await;

    // The voided ids would be duplicates the store ignores; these are new.
    let resent = ids(&store.accepted()[after_voids..]);
    assert_eq!(resent.len(), sent.len());
    for id in &resent {
        assert!(!sent.contains(id), "{id} was voided at this store");
    }
}

#[tokio::test]
async fn a_removal_whose_voiding_stops_partway_reports_what_was_voided() {
    let learner = Learner::new().await;
    for n in 0..60 {
        learner.add_numbered_interaction_record(n);
    }
    let store = XapiStub::start(&["2.0.0"]).await;
    let mut request = learner.request(&store, "learner-key", &[&learner.course]);
    request.include_interactions = true;
    let added = learner.add(request).await.unwrap();
    learner.pass(t0()).await;
    let sent = ids(&store.accepted());
    assert!(sent.len() > send::BATCH_SIZE, "two void batches");

    // The second void batch is refused; the first went through.
    store.refuse_containing(&sent[send::BATCH_SIZE + 1]);
    let report = destinations::remove(&learner.db, &added.id, &learner.secret, &learner.did, t0())
        .await
        .unwrap();
    assert_eq!(report.voided as usize, send::BATCH_SIZE);
    assert!(report.void_error.is_some());
}

#[tokio::test]
async fn a_supervised_profile_cannot_add_a_destination() {
    let store = XapiStub::start(&["2.0.0"]).await;
    let minor = Learner::new().await;
    minor.with_conn(|conn| {
        let birthdate = (Utc::now() - Duration::days(365 * 12)).format("%Y-%m-%d");
        conn.execute(
            "UPDATE local_identity SET birthdate = ?1 WHERE id = 1",
            [birthdate.to_string()],
        )
        .unwrap();
    });
    let ward = Learner::new().await;
    ward.with_conn(|conn| {
        conn.execute(
            "INSERT INTO guardian_links (id, side, peer_did, shared_key, status) \
             VALUES ('link-1', 'ward', 'did:key:z6Mkguardian', x'00', 'active')",
            [],
        )
        .unwrap();
    });

    for learner in [&minor, &ward] {
        let refused = learner
            .add(learner.request(&store, "learner-key", &[&learner.course]))
            .await
            .unwrap_err();
        assert!(
            refused.starts_with("guardian_approval_required:"),
            "{refused}"
        );
        assert!(learner.destinations().is_empty());
    }
    assert!(store.statement_posts().is_empty());
}

#[tokio::test]
async fn the_same_store_and_key_cannot_be_added_twice() {
    let learner = Learner::new().await;
    let store = XapiStub::start(&["2.0.0"]).await;
    learner
        .add(learner.request(&store, "learner-key", &[&learner.course]))
        .await
        .unwrap();
    let again = learner
        .add(learner.request(&store, "learner-key", &[&learner.other_course]))
        .await
        .unwrap_err();
    assert!(again.starts_with("already_added:"), "{again}");
}
