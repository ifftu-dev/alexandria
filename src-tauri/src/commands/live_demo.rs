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
        None,
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

// ─────────────────────────────────────────────────────────────────────────────
// The same path through the real IPC surface: the `#[tauri::command]` handlers
// the Vue screens invoke, dispatched by a mock Tauri app with the profile
// session header, from `create_profile` onward. What differs from the test
// above is only the entry point: here nothing is seeded by hand, the profile
// start installs the bundled banks and plugins, and every step is the command
// a button calls.
// ─────────────────────────────────────────────────────────────────────────────

mod ipc {
    use super::*;
    use crate::AppState;
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime};
    use tauri::Manager;

    struct App {
        webview: tauri::WebviewWindow<MockRuntime>,
        session: Option<String>,
    }

    impl App {
        async fn invoke(&self, cmd: &str, args: Value) -> Result<Value, Value> {
            let webview = self.webview.clone();
            let cmd = cmd.to_string();
            let mut headers = tauri::http::HeaderMap::new();
            if let Some(session) = &self.session {
                headers.insert(
                    "x-alexandria-profile-session",
                    session.parse().expect("header value"),
                );
            }
            tokio::task::spawn_blocking(move || {
                get_ipc_response(
                    &webview,
                    tauri::webview::InvokeRequest {
                        cmd,
                        callback: tauri::ipc::CallbackFn(0),
                        error: tauri::ipc::CallbackFn(1),
                        url: if cfg!(any(target_os = "windows", target_os = "android")) {
                            "http://tauri.localhost"
                        } else {
                            "tauri://localhost"
                        }
                        .parse()
                        .expect("local URL"),
                        body: tauri::ipc::InvokeBody::Json(args),
                        headers,
                        invoke_key: tauri::test::INVOKE_KEY.to_string(),
                    },
                )
                .map(|body| body.deserialize::<Value>().expect("JSON response"))
            })
            .await
            .expect("IPC task")
        }

        async fn call(&self, cmd: &str, args: Value) -> Value {
            match self.invoke(cmd, args).await {
                Ok(value) => value,
                Err(error) => panic!("{cmd} failed: {error}"),
            }
        }
    }

    /// The answers for a served attempt, read the way only the device can:
    /// the key from the item store, mapped through the served option order.
    fn correct_answers(state: &AppState, attempt_id: &str, questions: &[Value]) -> Vec<Value> {
        let guard = state.db.lock().unwrap();
        let db = guard.as_ref().expect("profile database is open");
        let orders: String = db
            .conn()
            .query_row(
                "SELECT option_orders FROM assessment_attempts WHERE id=?1",
                [attempt_id],
                |r| r.get(0),
            )
            .unwrap();
        let orders: Vec<Vec<usize>> = serde_json::from_str(&orders).unwrap();
        questions
            .iter()
            .zip(&orders)
            .map(|(question, order)| {
                let id = question["id"].as_str().unwrap();
                let private: String = db
                    .conn()
                    .query_row(
                        "SELECT grader_private FROM assessment_items WHERE id=?1",
                        [id],
                        |r| r.get(0),
                    )
                    .unwrap();
                let private: Value = serde_json::from_str(&private).unwrap();
                let correct: Vec<usize> =
                    serde_json::from_value(private["correct_indices"].clone()).unwrap();
                let selected: Vec<usize> = order
                    .iter()
                    .enumerate()
                    .filter(|(_, original)| correct.contains(original))
                    .map(|(position, _)| position)
                    .collect();
                json!({"question_id": id, "selected": selected})
            })
            .collect()
    }

    /// Start monitoring, take the attempt, answer, end monitoring, grade —
    /// the four commands the assessment screen calls, in its order.
    async fn take_assessment(
        app: &App,
        state: &AppState,
        start: (&str, Value),
    ) -> (String, String) {
        let session = app
            .call(
                "integrity_start_session",
                json!({"enrollmentId": null, "purpose": "assessment"}),
            )
            .await;
        let session_id = session["session_id"].as_str().unwrap().to_string();
        let (command, mut args) = start;
        args["integritySessionId"] = json!(session_id);
        let started = app.call(command, args).await;
        let attempt_id = started["attempt_id"].as_str().unwrap().to_string();
        let answers = correct_answers(state, &attempt_id, started["questions"].as_array().unwrap());
        app.call(
            "assessment_submit_answers",
            json!({"attemptId": attempt_id, "answers": answers}),
        )
        .await;
        app.call(
            "integrity_end_session",
            json!({"sessionId": session_id, "req": {"overall_integrity_score": 0.92, "overall_consistency_score": 0.9}}),
        )
        .await;
        let graded = app
            .call(
                "assessment_grade",
                json!({"attemptId": attempt_id, "answers": answers}),
            )
            .await;
        assert_eq!(graded["passed"], true, "{graded}");
        (
            attempt_id,
            graded["credential_id"]
                .as_str()
                .expect("credential issued")
                .to_string(),
        )
    }

    #[tokio::test]
    #[ignore = "needs a running Cloud: ALEXANDRIA_DEMO_CLOUD=http://127.0.0.1:8787"]
    async fn the_whole_demo_path_works_through_the_tauri_commands() {
        let base = std::env::var("ALEXANDRIA_DEMO_CLOUD")
            .unwrap_or_else(|_| "http://127.0.0.1:8787".into());
        let cloud = Cloud::connect(&base).await;

        let directory = tempfile::TempDir::new().expect("temporary app directory");
        let state = crate::profile::lifecycle_tests::state_in(directory.path());
        let state = std::sync::Arc::try_unwrap(state).unwrap_or_else(|_| panic!("unique state"));
        let tauri_app = mock_builder()
            .manage(state)
            .invoke_handler(tauri::generate_handler![
                crate::commands::profile::get_profile_session_token,
                crate::commands::identity::get_local_did,
                crate::commands::holder_pull::set_directories,
                crate::commands::holder_pull::publish_listing,
                crate::commands::integrity::integrity_start_session,
                crate::commands::integrity::integrity_end_session,
                crate::commands::assessment::assessment_start_attempt,
                crate::commands::assessment::assessment_submit_answers,
                crate::commands::assessment::assessment_grade,
                crate::commands::credentials::get_credential,
                crate::commands::credentials::revoke_credential,
                crate::commands::credentials::publish_status_lists,
                crate::commands::credentials::export_credentials_bundle,
                crate::commands::talent_index::set_talent_index_consent,
                crate::commands::exchange::exchange_requests,
                crate::commands::exchange::exchange_start_assessment,
                crate::commands::exchange::exchange_credentials,
                crate::commands::exchange::exchange_preview,
                crate::commands::exchange::exchange_send,
                crate::commands::hiring::hiring_interviews,
                crate::commands::hiring::hiring_interview_respond,
                crate::commands::hiring::hiring_offers,
                crate::commands::hiring::hiring_offer_respond,
                crate::commands::hiring::hiring_history,
            ])
            .build(mock_context(noop_assets()))
            .expect("mock app");
        let webview = tauri::WebviewWindowBuilder::new(&tauri_app, "main", Default::default())
            .build()
            .expect("mock webview");
        let mut app = App {
            webview,
            session: None,
        };
        let state = tauri_app.state::<AppState>();

        // ── Onboarding ────────────────────────────────────────────────────
        // `create_profile` takes the production `AppHandle` type, which a mock
        // runtime cannot supply, so its body is replayed here step for step:
        // reserve the profile, create the vault, generate the wallet, bring
        // the profile online (which installs the bundled banks and plugins),
        // record the identity. Everything after this line is the real command.
        let operations = state.profile_operations.clone();
        let handle = tauri_app.handle().clone();
        operations
            .activate(
                async move {
                    let state = handle.state::<AppState>();
                    let paths = state
                        .profile_manager
                        .create_on_network("Live learner", Default::default(), "preprod")
                        .map_err(|e| e.to_string())?;
                    #[allow(unused_mut)]
                    let mut keystore =
                        crate::crypto::keystore::Keystore::create(&paths.vault_dir, "correct horse battery staple")
                            .map_err(|e| e.to_string())?;
                    let wallet = crate::crypto::wallet::generate_wallet().map_err(|e| e.to_string())?;
                    keystore.store_mnemonic(&wallet.mnemonic).map_err(|e| e.to_string())?;
                    state.start_new_profile(paths.clone(), keystore).await?;
                    {
                        let guard = state.db.lock().map_err(|_| "db lock".to_string())?;
                        let db = guard.as_ref().ok_or("database not initialized")?;
                        db.conn()
                            .execute(
                                "INSERT OR REPLACE INTO local_identity (id, stake_address, payment_address, username, display_name, visibility, account_roles, birthdate, activation_state) \
                                 VALUES (1, ?1, ?2, 'livelearner', 'Live learner', 'public', '[\"learner\"]', NULL, 'active')",
                                rusqlite::params![wallet.stake_address, wallet.payment_address],
                            )
                            .map_err(|e| e.to_string())?;
                    }
                    state
                        .profile_manager
                        .touch_unlocked(&paths.id)
                        .map_err(|e| e.to_string())?;
                    Ok(())
                },
                async { Ok(()) },
            )
            .await
            .expect("profile comes online");
        let session = app.call("get_profile_session_token", json!({})).await;
        app.session = Some(session.as_str().expect("session token").to_string());
        let did = app.call("get_local_did", json!({})).await;
        let did = Did(did.as_str().expect("local DID").to_string());
        app.call(
            "set_directories",
            json!({"directories": [{"name": "Local demo", "url": base}]}),
        )
        .await;
        // Profile start installs the bundled banks in every build; nothing
        // is seeded by hand here or by the operator.
        {
            let guard = state.db.lock().unwrap();
            let banks: i64 = guard
                .as_ref()
                .unwrap()
                .conn()
                .query_row(
                    "SELECT count(*) FROM question_banks WHERE ratified=1",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(
                banks >= 2,
                "bundled banks installed on profile start: {banks}"
            );
        }
        println!(
            "0. profile created; {}; Local demo configured; bundled banks present",
            did.as_str()
        );

        // ── 1. Earn a credential; the grade command pushes the list itself ──
        let (_, first_id) = take_assessment(
            &app,
            &state,
            (
                "assessment_start_attempt",
                json!({"skillId": "skill_javascript"}),
            ),
        )
        .await;
        let first: VerifiableCredential = serde_json::from_value(
            app.call("get_credential", json!({"credentialId": first_id}))
                .await,
        )
        .unwrap();
        let list_url = status::list_url(&base, &did, 1);
        let reference = first.credential_status.clone().expect("status reference");
        assert_eq!(reference.status_list_credential, list_url);
        let (code, served) = cloud.open("GET", &list_url[base.len()..], None).await;
        assert_eq!(code, 200, "grading published the list: {served}");
        let served: VerifiableCredential = serde_json::from_value(served).unwrap();
        let bits = status::verify_fetched_list(&served, &list_url, &did, "revocation").unwrap();
        assert_eq!(
            status::get_bit(&bits, reference.status_list_index.parse().unwrap()),
            Some(false)
        );
        println!("1. credential {first_id} issued; list served by Cloud");

        // ── 2. Consent and publish ────────────────────────────────────────
        let preview = app
            .call(
                "set_talent_index_consent",
                json!({"consent": {"skills": ["skill_javascript"], "displayName": true, "bio": false}}),
            )
            .await;
        assert!(preview["record"].is_object(), "{preview}");
        app.call("publish_listing", json!({"directoryUrl": base}))
            .await;
        println!("2. listing published");

        // ── 3. Found; candidate created ───────────────────────────────────
        let (code, body) = cloud
            .org(
                "GET",
                "/api/index/search?skill=skill_javascript&min_level=1",
                None,
            )
            .await;
        assert_eq!(code, 200, "{body}");
        let found = body["matches"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["did"] == did.as_str())
            .unwrap_or_else(|| panic!("learner is found: {body}"));
        assert_eq!(
            found["name"], "Live learner",
            "the consented name is listed"
        );
        let (_, body) = cloud
            .org(
                "POST",
                "/api/candidates",
                Some(json!({"did": did.as_str(), "name": "Live learner"})),
            )
            .await;
        let candidate = body["id"].as_str().unwrap().to_string();
        println!("3. found in search as 'Live learner'; candidate {candidate}");

        // ── 4. Requested assessment, through the exchange commands ────────
        let (code, body) = cloud
            .org(
                "POST",
                "/api/assessment-requests",
                Some(json!({"candidate_id": candidate, "skill_id": "skill_big_o", "role_label": "Junior engineer", "purpose": "IPC rehearsal of the assessment exchange for the engineering role", "require_new_assessment": true})),
            )
            .await;
        assert_eq!(code, 200, "{body}");
        let request_id = body["request"]["id"].as_str().unwrap().to_string();
        let inbox = app.call("exchange_requests", json!({})).await;
        assert_eq!(inbox["problems"], json!([]), "{inbox}");
        let item = inbox["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["request"]["id"] == request_id.as_str())
            .unwrap_or_else(|| panic!("request reaches the inbox: {inbox}"));
        let request = item["request"].clone();
        let (_, second_id) = take_assessment(
            &app,
            &state,
            (
                "exchange_start_assessment",
                json!({"directoryUrl": base, "requestId": request_id}),
            ),
        )
        .await;
        let shareable = app
            .call("exchange_credentials", json!({"request": request}))
            .await;
        assert!(
            shareable
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["id"] == second_id.as_str()),
            "the new credential is offered for this request: {shareable}"
        );
        let signed = app
            .call(
                "exchange_preview",
                json!({"request": request, "credentialId": second_id}),
            )
            .await;
        assert_eq!(signed["type"][0], "VerifiablePresentation", "{signed}");
        let receipt = app
            .call(
                "exchange_send",
                json!({"directoryUrl": base, "signed": signed}),
            )
            .await;
        assert_eq!(
            receipt["verification"]["acceptanceDecision"], "accept",
            "{receipt}"
        );
        assert_eq!(
            receipt["verification"]["hostedStatus"], "clear",
            "{receipt}"
        );
        println!("4. requested assessment taken and shared through exchange_*; accept, hosted list clear");

        // ── 5. Interview and offer, through the hiring commands ───────────
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
        let invite_id = body["invite"]["id"].as_str().unwrap().to_string();
        let inbox = app.call("hiring_interviews", json!({})).await;
        let invite = inbox["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["invite"]["id"] == invite_id.as_str())
            .unwrap_or_else(|| panic!("invitation reaches the inbox: {inbox}"))["invite"]
            .clone();
        let slot = invite["proposed_slots"][0].clone();
        let receipt = app
            .call(
                "hiring_interview_respond",
                json!({"directoryUrl": base, "invite": invite, "decision": "accept", "chosenSlot": slot, "note": "Looking forward to it."}),
            )
            .await;
        assert_eq!(receipt["status"], "accepted", "{receipt}");
        let (code, body) = cloud
            .org(
                "POST",
                &format!("/api/interviews/{invite_id}/conduct"),
                Some(json!({"outcome": "advance", "notes": "Clear on complexity trade-offs."})),
            )
            .await;
        assert_eq!(code, 200, "{body}");
        let (code, body) = cloud
            .org(
                "POST",
                "/api/offers",
                Some(json!({"interview_id": invite_id, "terms": "Full time, start in four weeks.", "start_date": "2026-11-09"})),
            )
            .await;
        assert_eq!(code, 200, "{body}");
        let offer_id = body["offer"]["id"].as_str().unwrap().to_string();
        let inbox = app.call("hiring_offers", json!({})).await;
        let offer = inbox["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["offer"]["id"] == offer_id.as_str())
            .unwrap_or_else(|| panic!("offer reaches the inbox: {inbox}"))["offer"]
            .clone();
        let receipt = app
            .call(
                "hiring_offer_respond",
                json!({"directoryUrl": base, "offer": offer, "decision": "accept", "note": null}),
            )
            .await;
        assert_eq!(receipt["status"], "accepted", "{receipt}");
        let history = app.call("hiring_history", json!({})).await;
        assert_eq!(
            history.as_array().unwrap().len(),
            2,
            "both answers are kept: {history}"
        );
        println!("5. interview and offer accepted through hiring_*; history has both");

        // ── 6. Verify without Alexandria ──────────────────────────────────
        let (exit, out) = node_verify(&first);
        assert_eq!(exit, 0, "{out}");
        assert!(out.contains("decision   ACCEPT"), "{out}");
        println!("6. stdlib verifier: ACCEPT, list fetched from Cloud");

        // ── 7. Revoke through the command; the list flips on Cloud ────────
        app.call(
            "revoke_credential",
            json!({"credentialId": first_id, "reason": "IPC rehearsal"}),
        )
        .await;
        let report = app.call("publish_status_lists", json!({})).await;
        assert_eq!(report["errors"], json!([]), "{report}");
        let (exit, out) = node_verify(&first);
        assert_eq!(exit, 3, "{out}");
        assert!(out.contains("status     revoked"), "{out}");
        let bundle = app.call("export_credentials_bundle", json!({})).await;
        let (accepted, total) =
            verify_bundle_offline_impl(bundle.as_str().unwrap(), &now_rfc3339()).unwrap();
        assert!(total >= 2 && accepted == total - 1, "{accepted} of {total}");
        println!("7. revoked via IPC; verifier REJECTs; bundle {accepted} of {total} accepted");

        let handle = tauri_app.handle().clone();
        operations
            .lock(async move { handle.state::<AppState>().stop_active_profile().await })
            .await
            .expect("profile locks");
        println!("8. profile locked");
    }
}
