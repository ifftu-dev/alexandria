//! Host-seam tests for record shares: an organisation's request reaches a
//! learner with learning records, the learner approves some of it through
//! the grant path, the background pass sends signed batches to the
//! organisation's hosted service (a small in-process stand-in), and
//! narrowing or revoking the grant changes what is sent.

use alexandria_studio::xapi;
use alexandria_verify::access::{
    self, AccessRequest, GrantAction, GrantStatement, RecordAccount, RecordKind,
};
use chrono::Duration;
use ed25519_dalek::SigningKey;
use serde_json::Value;

use super::shares::{self, RecordBatch, ShareApproval};
use super::tests::{actor_names, did, ids, t0, Learner, LEARNER_SEED};
use super::xapi_stub::XapiStub;
use super::*;
use crate::domain::record_sending::{IdentifierChoice, RecordShareDecision};
use crate::settings::{registry::keys, SettingsStore};

const ORG_DOMAIN: &str = "hr.example.org";

fn key() -> SigningKey {
    SigningKey::from_bytes(&[LEARNER_SEED; 32])
}

fn hr_account() -> RecordAccount {
    RecordAccount {
        home_page: "https://hr.example.org/people".into(),
        name: "E-1042".into(),
    }
}

/// What the organisation's hosted service sends the learner: records from
/// both courses, per-answer detail included, its HR identifier proposed.
fn request(learner: &Learner) -> AccessRequest {
    AccessRequest {
        id: "6f1d0c52-3a8e-4f7b-9d2a-0c1e5b7a9f10".into(),
        audience: alexandria_verify::org::audience(ORG_DOMAIN),
        nonce: "2b7e1516-28ae-4d2a-abf7-158809cf4f3c".into(),
        organization: "Example Org".into(),
        subject_did: learner.did.clone(),
        skill_ids: Vec::new(),
        purpose: "Mandatory safety training reporting".into(),
        network_id: "preprod".into(),
        created_at: t0().timestamp() - 60,
        expires_at: t0().timestamp() + 14 * 86_400,
        records: Some(access::RecordScope {
            course_ids: vec![learner.course.clone(), learner.other_course.clone()],
            kinds: vec![RecordKind::Completion, RecordKind::Interaction],
            account: Some(hr_account()),
        }),
    }
}

fn decision(
    courses: &[&str],
    interactions: bool,
    identifier: IdentifierChoice,
) -> RecordShareDecision {
    RecordShareDecision {
        course_ids: courses.iter().map(|c| c.to_string()).collect(),
        include_interactions: interactions,
        identifier,
    }
}

/// The learner approves: what `org_grant` does, minus the request to the
/// hosted service. The grant statement is built, checked against the
/// request and signed, the grant is stored with the service's address, and
/// the share is set up to send.
fn approve(
    learner: &Learner,
    service: &XapiStub,
    request: &AccessRequest,
    choice: &RecordShareDecision,
) -> GrantStatement {
    let grant_id = uuid::Uuid::new_v4().to_string();
    let ShareApproval { records, entry } = shares::approval(
        request,
        choice,
        &grant_id,
        &learner.secret,
        &learner.did,
        t0(),
    )
    .expect("approval");
    let statement = shares::grant_statement(
        request,
        &grant_id,
        Vec::new(),
        false,
        Some(records),
        t0().timestamp(),
    );
    access::check_answers(request, &statement, t0().timestamp()).expect("answers the request");
    let signed = access::sign(&statement, &key()).expect("sign");
    access::verify(&signed, t0().timestamp()).expect("verifies");
    learner.with_conn(|conn| {
        conn.execute(
            "INSERT INTO org_grants (grant_id, cloud_url, org_domain, org_name, purpose, \
             skill_ids_json, sync, statement_json, granted_at) \
             VALUES (?1, ?2, ?3, 'Example Org', ?4, '[]', 0, ?5, ?6)",
            rusqlite::params![
                grant_id,
                service.base(),
                ORG_DOMAIN,
                request.purpose,
                serde_json::to_string(&statement).unwrap(),
                t0().timestamp()
            ],
        )
        .unwrap();
        shares::approve(conn, &entry, t0()).expect("set up the share");
    });
    statement
}

async fn pass(learner: &Learner, at: chrono::DateTime<chrono::Utc>) -> BatchOutcome {
    shares::send_pending(&learner.db, &key(), at)
        .await
        .expect("pass")
}

fn courses_of(statements: &[Value]) -> Vec<String> {
    statements
        .iter()
        .map(|s| s["object"]["id"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn a_request_alone_sends_nothing_and_the_consent_screen_shows_what_it_asks() {
    let learner = Learner::new().await;
    let service = XapiStub::start(&["2.0.0"]).await;
    let request = request(&learner);

    let offer = learner
        .with_conn(|conn| shares::offer(conn, &request, &learner.secret, &learner.did))
        .unwrap()
        .expect("the request asks for records");
    pass(&learner, t0()).await;
    assert!(
        service.requests().is_empty(),
        "nothing is sent before approval"
    );

    let titles: Vec<_> = offer.courses.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, vec!["Warehouse safety", "Forklift basics"]);
    assert!(offer.interactions_requested);
    assert_eq!(offer.proposed_account, Some(hr_account()));
    assert_eq!(
        offer.real_did_actor,
        learner.did.trim_start_matches("did:key:")
    );
    assert_ne!(offer.pairwise_actor, offer.real_did_actor);
}

#[tokio::test]
async fn the_learner_may_approve_a_narrower_scope_and_only_that_is_sent() {
    let learner = Learner::new().await;
    let interaction = learner.add_interaction_record();
    let service = XapiStub::start(&["2.0.0"]).await;
    let request = request(&learner);
    let offer = learner
        .with_conn(|conn| shares::offer(conn, &request, &learner.secret, &learner.did))
        .unwrap()
        .unwrap();

    // One of the two courses, completion records only, derived identifier.
    let statement = approve(
        &learner,
        &service,
        &request,
        &decision(&[&learner.course], false, IdentifierChoice::Pairwise),
    );
    let granted = statement.records.clone().unwrap();
    assert_eq!(granted.course_ids, vec![learner.course.clone()]);
    assert_eq!(granted.kinds, vec![RecordKind::Completion]);
    assert_eq!(
        granted.account.as_ref().map(|a| a.name.as_str()),
        Some(offer.pairwise_actor.as_str()),
        "the grant names the identifier the consent screen showed"
    );
    assert!(
        service.requests().is_empty(),
        "approval itself sends no records"
    );

    let outcome = pass(&learner, t0()).await;
    let sent = service.accepted();
    assert_eq!(outcome.sent, 4);
    let course_records: Vec<String> = learner
        .record_ids(&learner.course)
        .into_iter()
        .filter(|id| *id != interaction)
        .collect();
    let label = shares::label(&request);
    let expected: Vec<String> = course_records
        .iter()
        .map(|id| {
            xapi::Recipient::pairwise(&learner.secret, &label)
                .unwrap()
                .statement_id(id)
        })
        .collect();
    assert_eq!(
        ids(&sent),
        expected,
        "the other course and the response stay home"
    );
    assert!(actor_names(&sent)
        .iter()
        .all(|n| n == &offer.pairwise_actor));
    assert!(
        !serde_json::to_string(&sent).unwrap().contains(&learner.did),
        "the learner's DID is not in the records"
    );

    let batches = service.batches();
    assert_eq!(batches.len(), 1);
    let batch: RecordBatch = serde_json::from_value(batches[0]["batch"].clone()).unwrap();
    assert_eq!(batch.grant_id, statement.grant_id);
    assert_eq!(batch.subject_did, learner.did);
    assert_eq!(batch.format, shares::RECORD_BATCH_FORMAT);
}

#[tokio::test]
async fn an_approved_proposed_account_is_what_records_carry() {
    let learner = Learner::new().await;
    let service = XapiStub::start(&["2.0.0"]).await;
    let request = request(&learner);
    let statement = approve(
        &learner,
        &service,
        &request,
        &decision(&[&learner.other_course], false, IdentifierChoice::Proposed),
    );
    assert_eq!(statement.records.unwrap().account, Some(hr_account()));
    pass(&learner, t0()).await;
    let sent = service.accepted();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0]["actor"]["account"]["homePage"],
        "https://hr.example.org/people"
    );
    assert_eq!(sent[0]["actor"]["account"]["name"], "E-1042");
}

#[tokio::test]
async fn an_approval_cannot_reach_past_the_request() {
    let learner = Learner::new().await;
    let mut request = request(&learner);
    let approval = |request: &AccessRequest, choice: RecordShareDecision| {
        shares::approval(
            request,
            &choice,
            "grant",
            &learner.secret,
            &learner.did,
            t0(),
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    };
    let stranger = "dd".repeat(32);
    let refused = approval(
        &request,
        decision(&[&stranger], false, IdentifierChoice::Pairwise),
    )
    .unwrap_err();
    assert!(refused.starts_with("invalid_scope:"), "{refused}");
    assert!(approval(&request, decision(&[], false, IdentifierChoice::Pairwise)).is_err());

    request.records.as_mut().unwrap().kinds = vec![RecordKind::Completion];
    request.records.as_mut().unwrap().account = None;
    assert!(
        approval(
            &request,
            decision(&[&learner.course], true, IdentifierChoice::Pairwise)
        )
        .is_err(),
        "per-answer records were not asked for"
    );
    assert!(
        approval(
            &request,
            decision(&[&learner.course], false, IdentifierChoice::Proposed)
        )
        .is_err(),
        "no account was proposed"
    );
    request.records = None;
    assert!(approval(
        &request,
        decision(&[&learner.course], false, IdentifierChoice::Pairwise)
    )
    .is_err());
}

#[tokio::test]
async fn narrowing_a_share_stops_sending_what_falls_outside_it() {
    let learner = Learner::new().await;
    let service = XapiStub::start(&["2.0.0"]).await;
    let request = request(&learner);
    let current = approve(
        &learner,
        &service,
        &request,
        &decision(
            &[&learner.course, &learner.other_course],
            true,
            IdentifierChoice::Pairwise,
        ),
    );

    let (next, scope) = shares::narrowed(
        &current,
        vec![learner.other_course.clone()],
        false,
        t0().timestamp() + 10,
    )
    .expect("narrowed");
    assert_eq!(next.action, GrantAction::Update);
    access::check_follows(&current, &next).expect("an update that narrows");
    access::verify(&access::sign(&next, &key()).unwrap(), t0().timestamp() + 10).unwrap();
    learner.with_conn(|conn| shares::narrow(conn, &current.grant_id, &scope).unwrap());

    pass(&learner, t0()).await;
    let sent = service.accepted();
    assert_eq!(
        sent.len(),
        1,
        "only the other course's registration is left"
    );
    assert!(courses_of(&sent)[0].contains(&learner.other_course));

    // Widening is not narrowing.
    let widened = shares::narrowed(
        &next,
        vec![learner.course.clone(), learner.other_course.clone()],
        false,
        t0().timestamp() + 20,
    );
    assert!(widened.is_err());
}

#[tokio::test]
async fn revoking_a_share_stops_further_sends() {
    let learner = Learner::new().await;
    let service = XapiStub::start(&["2.0.0"]).await;
    let request = request(&learner);
    let grant = approve(
        &learner,
        &service,
        &request,
        &decision(&[&learner.course], true, IdentifierChoice::Pairwise),
    );
    pass(&learner, t0()).await;
    assert_eq!(service.accepted().len(), 4);

    learner.with_conn(|conn| assert!(shares::stop(conn, &grant.grant_id).unwrap()));
    learner.add_interaction_record();
    pass(&learner, t0() + Duration::minutes(5)).await;
    assert_eq!(
        service.accepted().len(),
        4,
        "nothing is sent after revoking"
    );
}

#[tokio::test]
async fn a_revoked_grant_sends_nothing_even_if_its_share_lingers() {
    let learner = Learner::new().await;
    let service = XapiStub::start(&["2.0.0"]).await;
    let request = request(&learner);
    let grant = approve(
        &learner,
        &service,
        &request,
        &decision(&[&learner.course], false, IdentifierChoice::Pairwise),
    );
    learner.with_conn(|conn| {
        conn.execute(
            "UPDATE org_grants SET revoked_at = ?2 WHERE grant_id = ?1",
            rusqlite::params![grant.grant_id, t0().timestamp()],
        )
        .unwrap();
    });
    pass(&learner, t0()).await;
    assert!(service.requests().is_empty());
}

#[tokio::test]
async fn shares_wait_while_organisations_are_switched_off() {
    let learner = Learner::new().await;
    let service = XapiStub::start(&["2.0.0"]).await;
    let request = request(&learner);
    approve(
        &learner,
        &service,
        &request,
        &decision(&[&learner.course], false, IdentifierChoice::Pairwise),
    );
    learner.with_conn(|conn| SettingsStore::set(conn, keys::ORGANISATIONS_ENABLED, false).unwrap());
    pass(&learner, t0()).await;
    assert!(service.requests().is_empty());

    learner.with_conn(|conn| SettingsStore::set(conn, keys::ORGANISATIONS_ENABLED, true).unwrap());
    assert_eq!(pass(&learner, t0()).await.sent, 4);
}

#[tokio::test]
async fn a_refused_batch_is_visible_and_logged_as_a_share() {
    let learner = Learner::new().await;
    let service = XapiStub::start(&["2.0.0"]).await;
    let request = request(&learner);
    approve(
        &learner,
        &service,
        &request,
        &decision(&[&learner.course], false, IdentifierChoice::Pairwise),
    );
    service.refuse(403, "{\"error\":\"share revoked\"}");
    let outcome = pass(&learner, t0()).await;
    assert_eq!(outcome.failed, 4);
    let log = learner.access_log();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].recipient_kind, RecipientKind::Share);
    assert_eq!(log[0].recipient_name, "Example Org");
    assert!(!log[0].accepted);
    assert!(log[0].detail.as_deref().unwrap_or_default().contains("403"));
}

#[test]
fn a_signed_batch_matches_the_shared_vector() {
    let vector = shares::tests_support::vector();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/record-share/signed-batch.json");
    if std::env::var("ALEXANDRIA_REGENERATE_VECTORS").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            format!("{}\n", serde_json::to_string_pretty(&vector).unwrap()),
        )
        .unwrap();
    }
    let on_disk: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        on_disk,
        serde_json::to_value(&vector).unwrap(),
        "the batch wire format moved; regenerate and copy to the hosted service"
    );
    assert_eq!(on_disk["batch"]["subject_did"], did(LEARNER_SEED));
    assert!(
        super::xapi_stub::batch_signature_checks(&on_disk),
        "the signature checks from the JSON alone, as the service checks it"
    );
    let mut tampered = on_disk.clone();
    tampered["batch"]["grant_id"] = Value::from("another-grant");
    assert!(!super::xapi_stub::batch_signature_checks(&tampered));
}
