//! The demo path, end to end, against a running Alexandria Cloud.
//!
//! This drives the same functions the Tauri commands call — grading, issuing,
//! signing, pushing, presenting — with a real database and real keys, and
//! talks to a real Cloud over HTTP exactly as the app does. It is the
//! rehearsal of `docs/demo-runbook.md`: learner earns a credential, publishes
//! a listing, is found, completes a requested assessment, shares the result,
//! accepts an interview and an offer, is enrolled in a validity pilot, and
//! the credential is verified by the stdlib script and then revoked.
//!
//! Ignored by default because it needs Cloud up (`scripts/demo-assessment.sh`
//! in the Cloud checkout) and the Node verifier on PATH:
//!
//!   ALEXANDRIA_DEMO_CLOUD=http://127.0.0.1:8787 \
//!   cargo test --lib live_demo -- --ignored --nocapture

use alexandria_verify::exchange::{
    present_credential, verify_share, CredentialRequest, IssuerState,
};
use alexandria_verify::vc::status;
use alexandria_verify::vc::VerifiableCredential;
use ed25519_dalek::SigningKey;
use serde_json::{json, Value};

use crate::commands::credentials::{
    export_bundle_signed_impl, mark_status_lists_published, now_rfc3339, pending_status_lists,
    push_status_lists, revoke_credential_impl, verify_bundle_offline_impl,
};
use crate::commands::hiring::{
    sign_interview_response, sign_offer_response, InterviewInvite, Offer,
};
use crate::commands::talent_index::{get_talent_index_preview_impl, set_talent_index_consent_impl};
use crate::crypto::did::{derive_did_key, Did};
use crate::db::Database;
use crate::domain::talent_index::{sign_record, TalentIndexConsent};
use crate::settings::registry::{keys, JsonSetting};
use crate::settings::SettingsStore;

struct Cloud {
    base: String,
    client: reqwest::Client,
    cookie: String,
}

impl Cloud {
    async fn connect(base: &str) -> Self {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        // DEV_AUTH=1: the login route mints a session for the first organisation.
        let response = client
            .get(format!("{base}/auth/login"))
            .send()
            .await
            .expect("Cloud is reachable");
        let cookie = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find_map(|v| v.split(';').next().filter(|c| c.starts_with("ac_session=")))
            .expect("dev sign-in sets a session cookie")
            .to_string();
        Self {
            base: base.trim_end_matches('/').to_string(),
            client,
            cookie,
        }
    }

    async fn org(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut request = self
            .client
            .request(method.parse().unwrap(), format!("{}{path}", self.base))
            .header("cookie", &self.cookie);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    async fn open(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut request = self
            .client
            .request(method.parse().unwrap(), format!("{}{path}", self.base));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    /// A holder's signed GET, exactly as the app sends it.
    async fn holder_get(&self, key: &SigningKey, path: &str) -> (u16, Value) {
        let timestamp = chrono::Utc::now().timestamp();
        let nonce = uuid::Uuid::new_v4().to_string();
        let proof = crate::commands::holder_pull::proof(key, "GET", path, timestamp, &nonce);
        let response = self
            .client
            .get(format!("{}{path}", self.base))
            .header("x-alexandria-timestamp", timestamp.to_string())
            .header("x-alexandria-nonce", nonce)
            .header("x-alexandria-proof", proof)
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }
}

/// Take and pass an assessment the way the app does: start the attempt under a
/// live monitoring session, answer every served question with the key's
/// correct options mapped through the served order, end the session, grade.
fn pass_assessment(
    db: &Database,
    key: &SigningKey,
    did: &Did,
    skill: &str,
    session: &str,
    attempt_id: &str,
    binding: Option<&str>,
) -> String {
    use crate::commands::assessment::{
        grade_attempt_impl, start_attempt_db, submit_answers_impl, SubmittedAnswer,
    };
    let conn = db.conn();
    conn.execute(
        "INSERT INTO integrity_sessions (id, status) VALUES (?1, 'active')",
        [session],
    )
    .unwrap();
    let now = now_rfc3339();
    let started = start_attempt_db(
        db,
        skill.into(),
        Some(session.into()),
        rand::random(),
        attempt_id.into(),
        now.clone(),
    )
    .expect("attempt starts");
    if let Some(binding) = binding {
        conn.execute(
            "UPDATE assessment_attempts SET exchange_binding=?2 WHERE id=?1",
            rusqlite::params![attempt_id, binding],
        )
        .unwrap();
    }
    let orders: String = conn
        .query_row(
            "SELECT option_orders FROM assessment_attempts WHERE id=?1",
            [attempt_id],
            |r| r.get(0),
        )
        .unwrap();
    let orders: Vec<Vec<usize>> = serde_json::from_str(&orders).unwrap();
    let mut answers = Vec::new();
    for (question, order) in started.questions.iter().zip(&orders) {
        let private: String = conn
            .query_row(
                "SELECT grader_private FROM assessment_items WHERE id=?1",
                [&question.id],
                |r| r.get(0),
            )
            .unwrap();
        let private: Value = serde_json::from_str(&private).unwrap();
        let correct: Vec<usize> =
            serde_json::from_value(private["correct_indices"].clone()).unwrap();
        // order[served position] = original index.
        let selected = order
            .iter()
            .enumerate()
            .filter(|(_, original)| correct.contains(original))
            .map(|(position, _)| position)
            .collect();
        answers.push(SubmittedAnswer {
            question_id: question.id.clone(),
            selected,
        });
    }
    submit_answers_impl(conn, attempt_id, &answers).unwrap();
    conn.execute(
        "UPDATE integrity_sessions SET ended_at=?2, status='completed', integrity_score=0.92 WHERE id=?1",
        rusqlite::params![session, now],
    )
    .unwrap();
    #[cfg(desktop)]
    let runtime = crate::plugins::wasm_runtime::GraderRuntime::new().unwrap();
    #[cfg(desktop)]
    let engine = crate::assessment::items::GradeEngine {
        runtime: &runtime,
        budgets: Default::default(),
    };
    #[cfg(not(desktop))]
    let engine = crate::assessment::items::GradeEngine::default();
    let graded = grade_attempt_impl(db, &engine, key, did, attempt_id, &answers, &now).unwrap();
    assert!(graded.passed, "all-correct answers pass: {graded:?}");
    graded
        .credential_id
        .expect("a passing attempt issues a credential")
}

fn credential(db: &Database, id: &str) -> VerifiableCredential {
    let json: String = db
        .conn()
        .query_row(
            "SELECT signed_vc_json FROM credentials WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    serde_json::from_str(&json).unwrap()
}

/// Push whatever is pending, as the app does after issue, revoke and grading.
/// Issuing into an existing list changes no bit, so nothing may be pending.
async fn publish_lists(db: &Database, key: &SigningKey, did: &Did) {
    let pending = pending_status_lists(db.conn(), did).unwrap();
    if pending.is_empty() {
        return;
    }
    let (accepted, errors) = push_status_lists(&pending, key, did, &now_rfc3339()).await;
    assert!(errors.is_empty(), "{errors:?}");
    mark_status_lists_published(db.conn(), &accepted).unwrap();
    assert!(pending_status_lists(db.conn(), did).unwrap().is_empty());
}

/// Run the stdlib verifier on a bare credential; returns (exit code, stdout).
fn node_verify(vc: &VerifiableCredential) -> (i32, String) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("credential.json");
    std::fs::write(&file, serde_json::to_string_pretty(vc).unwrap()).unwrap();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../scripts/demo/verify-credential.mjs");
    let output = std::process::Command::new("node")
        .arg(&script)
        .arg(&file)
        .output()
        .expect("node is on PATH");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
    )
}

#[tokio::test]
#[ignore = "needs a running Cloud: ALEXANDRIA_DEMO_CLOUD=http://127.0.0.1:8787"]
async fn the_whole_demo_path_works_against_a_live_cloud() {
    let base =
        std::env::var("ALEXANDRIA_DEMO_CLOUD").unwrap_or_else(|_| "http://127.0.0.1:8787".into());
    let cloud = Cloud::connect(&base).await;

    // ── A fresh learner device ────────────────────────────────────────
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().unwrap();
    crate::db::bundled::install_foundation(db.conn()).unwrap();
    crate::db::bundled::install_bundled_data(db.conn()).unwrap();
    let plugin_dir = tempfile::tempdir().unwrap();
    let mcq = crate::plugins::builtins::BUILTIN_PLUGINS
        .iter()
        .find(|b| b.slug == "mcq")
        .unwrap();
    crate::plugins::registry::install_builtin(&db, plugin_dir.path(), mcq).unwrap();
    let key = SigningKey::from_bytes(&rand::random::<[u8; 32]>());
    let did = derive_did_key(&key);
    SettingsStore::set(
        db.conn(),
        keys::IDENTITY_LOCAL_DID,
        did.as_str().to_string(),
    )
    .unwrap();
    // Settings → Directories → Local demo. This is also what makes the
    // status host resolve to Cloud.
    SettingsStore::set(
        db.conn(),
        keys::HOLDER_DIRECTORIES,
        JsonSetting(json!([{"name": "Local demo", "url": base}])),
    )
    .unwrap();
    assert_eq!(
        crate::commands::credentials::status_list_host(db.conn()).as_deref(),
        Some(base.trim_end_matches('/'))
    );

    // ── 1. Learner earns a credential ─────────────────────────────────
    let first_id = pass_assessment(
        &db,
        &key,
        &did,
        "skill_javascript",
        "live-1",
        "live-att-1",
        None,
    );
    let first = credential(&db, &first_id);
    let list_url = status::list_url(&base, &did, 1);
    let reference = first.credential_status.clone().expect("status reference");
    assert_eq!(reference.status_list_credential, list_url);
    assert_eq!(
        pending_status_lists(db.conn(), &did).unwrap().len(),
        1,
        "a new list is pending"
    );
    publish_lists(&db, &key, &did).await;
    let (code, served) = cloud.open("GET", &list_url[base.len()..], None).await;
    assert_eq!(code, 200, "{served}");
    let served: VerifiableCredential = serde_json::from_value(served).unwrap();
    let bits = status::verify_fetched_list(&served, &list_url, &did, "revocation").unwrap();
    let index: usize = reference.status_list_index.parse().unwrap();
    assert_eq!(status::get_bit(&bits, index), Some(false));
    println!("1. credential {first_id} issued; list served at {list_url}");

    // ── 2. Learner makes it public ────────────────────────────────────
    set_talent_index_consent_impl(
        db.conn(),
        &TalentIndexConsent {
            skills: vec!["skill_javascript".into()],
            display_name: false,
            bio: false,
        },
    )
    .unwrap();
    let preview = get_talent_index_preview_impl(db.conn(), did.as_str(), &now_rfc3339()).unwrap();
    let record = preview.record.expect("consented record");
    assert_eq!(record.skills.len(), 1, "{record:?}");
    let signed_record = sign_record(&record, &key, &now_rfc3339()).unwrap();
    let (code, body) = cloud
        .open("POST", "/api/index/records", Some(json!(signed_record)))
        .await;
    assert_eq!(code, 200, "{body}");
    println!("2. listing published: {body}");

    // ── 3. Organisation finds the learner ─────────────────────────────
    let (code, body) = cloud
        .org(
            "GET",
            "/api/index/search?skill=skill_javascript&min_level=1",
            None,
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert!(
        body["matches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["did"] == did.as_str()),
        "learner is found: {body}"
    );
    let (code, body) = cloud
        .org(
            "POST",
            "/api/candidates",
            Some(json!({"did": did.as_str(), "name": "Live demo learner"})),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let candidate = body["id"].as_str().unwrap().to_string();
    println!("3. found in search; candidate {candidate}");

    // ── 4. Organisation requests another assessment; learner completes it ──
    let (code, body) = cloud
        .org(
            "POST",
            "/api/assessment-requests",
            Some(json!({"candidate_id": candidate, "skill_id": "skill_big_o", "role_label": "Junior engineer", "purpose": "Live rehearsal of the assessment exchange for the engineering role", "require_new_assessment": true})),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let request_id = body["request"]["id"].as_str().unwrap().to_string();
    let (code, pending) = cloud
        .holder_get(
            &key,
            &format!("/api/assessment-requests/for/{}", did.as_str()),
        )
        .await;
    assert_eq!(code, 200, "{pending}");
    let requests: Vec<CredentialRequest> = serde_json::from_value(pending).unwrap();
    let request = requests
        .into_iter()
        .find(|r| r.id == request_id)
        .expect("the request reaches the learner");
    crate::commands::exchange::validate_request(&request, did.as_str()).unwrap();
    let binding = format!("request:{}:{}", request.id, request.nonce);
    let second_id = pass_assessment(
        &db,
        &key,
        &did,
        "skill_big_o",
        "live-2",
        "live-att-2",
        Some(&binding),
    );
    let second = credential(&db, &second_id);
    assert!(
        alexandria_verify::vc::SkillClaim::extract(&second.credential_subject)
            .is_some_and(|c| c.evidence_refs.contains(&binding)),
        "the credential carries the request binding"
    );
    publish_lists(&db, &key, &did).await;
    let now = chrono::Utc::now().timestamp();
    let presentation = present_credential(
        request.clone(),
        second.clone(),
        Some(IssuerState {
            revoked: false,
            suspended: false,
            suspended_until: None,
            superseded: false,
        }),
        now,
        &key,
    )
    .unwrap();
    verify_share(&presentation, &request, now, &now_rfc3339()).unwrap();
    let (code, body) = cloud
        .open(
            "POST",
            &format!("/api/assessment-requests/{request_id}/share"),
            Some(json!(presentation)),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(
        body["verification"]["acceptanceDecision"], "accept",
        "{body}"
    );
    assert_eq!(body["verification"]["hostedStatus"], "clear", "{body}");
    let (code, run) = cloud
        .org(
            "GET",
            &format!("/api/assessment-requests/{request_id}"),
            None,
        )
        .await;
    assert_eq!(code, 200);
    assert!(run["received_at"].is_string(), "{run}");
    println!(
        "4. requested assessment completed and shared; verification accept, hosted list clear"
    );

    // ── 5. Interview and offer ────────────────────────────────────────
    let slot_a = (chrono::Utc::now() + chrono::Duration::days(2)).to_rfc3339();
    let slot_b = (chrono::Utc::now() + chrono::Duration::days(3)).to_rfc3339();
    let (code, body) = cloud
        .org(
            "POST",
            "/api/interviews",
            Some(json!({"candidate_id": candidate, "role_label": "Junior engineer", "message": "We would like to discuss the assessment you completed.", "mode": "video", "proposed_slots": [slot_a, slot_b], "meeting_url": "https://meet.example.test/alexandria", "run_id": request_id})),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let (code, invites) = cloud
        .holder_get(&key, &format!("/api/interviews/for/{}", did.as_str()))
        .await;
    assert_eq!(code, 200, "{invites}");
    let invites: Vec<InterviewInvite> = serde_json::from_value(invites).unwrap();
    let invite = invites
        .into_iter()
        .find(|i| i.id == body["invite"]["id"].as_str().unwrap())
        .expect("invitation reaches the learner");
    let answer = sign_interview_response(
        invite.clone(),
        "accept",
        Some(invite.proposed_slots[0]),
        Some("Looking forward to it.".into()),
        &key,
        did.as_str(),
        chrono::Utc::now().timestamp(),
    )
    .unwrap();
    let (code, body) = cloud
        .open(
            "POST",
            &format!("/api/interviews/{}/respond", invite.id),
            Some(json!(answer)),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["status"], "accepted");
    let (code, body) = cloud
        .org(
            "POST",
            &format!("/api/interviews/{}/conduct", invite.id),
            Some(json!({"outcome": "advance", "notes": "Clear on complexity trade-offs."})),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let (code, body) = cloud
        .org(
            "POST",
            "/api/offers",
            Some(json!({"interview_id": invite.id, "terms": "Full time, start in four weeks.", "start_date": "2026-11-09"})),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let (code, offers) = cloud
        .holder_get(&key, &format!("/api/offers/for/{}", did.as_str()))
        .await;
    assert_eq!(code, 200, "{offers}");
    let offers: Vec<Offer> = serde_json::from_value(offers).unwrap();
    let offer = offers
        .into_iter()
        .find(|o| o.id == body["offer"]["id"].as_str().unwrap())
        .expect("offer reaches the learner");
    let answer = sign_offer_response(
        offer.clone(),
        "accept",
        None,
        &key,
        did.as_str(),
        chrono::Utc::now().timestamp(),
    )
    .unwrap();
    let (code, body) = cloud
        .open(
            "POST",
            &format!("/api/offers/{}/respond", offer.id),
            Some(json!(answer)),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["status"], "accepted");
    println!("5. interview accepted, conducted; offer accepted");

    // ── 6. Verify without Alexandria: fetches the hosted list ─────────
    let (exit, out) = node_verify(&first);
    assert_eq!(exit, 0, "{out}");
    assert!(out.contains("decision   ACCEPT"), "{out}");
    assert!(
        out.contains(&format!("list fetched from {list_url}")),
        "{out}"
    );
    println!("6. stdlib verifier: ACCEPT (list fetched from Cloud)");

    // ── 7. Validity pilot ─────────────────────────────────────────────
    let (code, body) = cloud
        .org(
            "POST",
            "/api/pilots",
            Some(json!({"name": "Live rehearsal pilot", "pathway": "Entry-level software engineering", "target_participants": 4, "target_completed": 2})),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    let pilot = format!("/api/pilots/{}", body["id"].as_str().unwrap());
    let (code, _) = cloud
        .org(
            "POST",
            &format!("{pilot}/sign-off"),
            Some(json!({"reviewer": "Outside reviewer"})),
        )
        .await;
    assert_eq!(code, 200);
    let mut codes = Vec::new();
    let mut arms = Vec::new();
    for n in 0..4 {
        let subject = (n == 0).then(|| did.as_str().to_string());
        let (code, body) = cloud
            .org(
                "POST",
                &format!("{pilot}/participants"),
                Some(json!({"consent": true, "subject_did": subject})),
            )
            .await;
        assert_eq!(code, 200, "{body}");
        codes.push(body["code"].as_str().unwrap().to_string());
        arms.push(body["arm"].as_str().unwrap().to_string());
    }
    for (reviewer, role) in [
        ("Reviewer A", "screening_conventional"),
        ("Reviewer C", "screening_capability"),
        ("Scorer 1", "practical"),
    ] {
        let (code, body) = cloud
            .org(
                "POST",
                &format!("{pilot}/reviewers"),
                Some(json!({"reviewer": reviewer, "role": role})),
            )
            .await;
        assert_eq!(code, 200, "{body}");
    }
    for (code_, arm) in codes.iter().zip(&arms) {
        let reviewer = if arm == "conventional" {
            "Reviewer A"
        } else {
            "Reviewer C"
        };
        let (code, body) = cloud
            .org(
                "POST",
                &format!("{pilot}/screenings"),
                Some(
                    json!({"participant_code": code_, "reviewer": reviewer, "decision": "advance"}),
                ),
            )
            .await;
        assert_eq!(code, 200, "{body}");
        let (code, body) = cloud
            .org(
                "POST",
                &format!("{pilot}/practical-scores"),
                Some(json!({"participant_code": code_, "scorer": "Scorer 1", "score": 75.0})),
            )
            .await;
        assert_eq!(code, 200, "{body}");
    }
    let (code, report) = cloud.org("GET", &format!("{pilot}/report"), None).await;
    assert_eq!(code, 200, "{report}");
    assert_eq!(report["totals"]["enrolled"], 4);
    assert_eq!(report["totals"]["completedAssessment"], 1, "{report}");
    let (code, export) = cloud.org("GET", &format!("{pilot}/export"), None).await;
    assert_eq!(code, 200);
    assert!(!export.to_string().contains("did:key"));
    println!(
        "7. pilot: {} enrolled, arms {arms:?}, export de-identified",
        codes.len()
    );

    // ── 8. Revoke: the hosted list flips, the verifier sees it ────────
    revoke_credential_impl(db.conn(), &did, &first_id, "live rehearsal", &now_rfc3339()).unwrap();
    assert_eq!(
        pending_status_lists(db.conn(), &did).unwrap().len(),
        1,
        "revocation leaves the list pending"
    );
    publish_lists(&db, &key, &did).await;
    let (exit, out) = node_verify(&first);
    assert_eq!(exit, 3, "{out}");
    assert!(out.contains("status     revoked"), "{out}");
    // And the exported bundle still verifies on its own, revocation included.
    let bundle = export_bundle_signed_impl(db.conn(), Some((&key, &did))).unwrap();
    let (accepted, total) = verify_bundle_offline_impl(&bundle, &now_rfc3339()).unwrap();
    assert_eq!(
        (accepted, total),
        (1, 2),
        "two credentials, one of them revoked"
    );
    println!("8. revoked; verifier now REJECTs; offline bundle: {accepted} of {total} accepted");
}
