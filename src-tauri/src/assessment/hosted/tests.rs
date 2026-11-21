//! The hosted attempt client against a stub hosted service and an in-memory
//! profile database: what the device sends, and what it keeps.

use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::wire::{AnswerBody, Consent};
use super::*;
use crate::credentials::fixtures::{self, test_key, NOW};
use crate::crypto::did::derive_did_key;
use crate::sentinel::attestation::{NativeAttester, TrustConfig};

const INVITE: &str = "5f0c4d52-6f7e-4a59-9c39-6b2f1e0b8a11";
const ATTEMPT: &str = "0b6f2a7e-1c3d-4e5f-8a9b-0c1d2e3f4a5b";

/// Distinct item text, long enough for the assistant's bank guard to compare.
const QUESTIONS: [&str; 2] = [
    "Which isolation level prevents phantom reads in a relational database transaction?",
    "A service retries a failed payment request; which property makes the retry safe?",
];
const OPTIONS: [&str; 2] = [
    "Serializable isolation with predicate locking held until commit",
    "An idempotency key the server deduplicates on before charging",
];

// ── Captured logs ─────────────────────────────────────────────────

/// Captures what is logged on threads that opted in ([`capturing`]), at
/// every level. Other tests in the binary share the global logger but are
/// never formatted or stored, so they keep their speed and their own
/// locking.
struct Capture(Mutex<Vec<String>>);

thread_local! {
    static CAPTURING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        CAPTURING.with(|c| c.get())
    }
    fn log(&self, record: &log::Record<'_>) {
        if !CAPTURING.with(|c| c.get()) {
            return;
        }
        // Formatted before the lock: formatting may itself log.
        let line = format!("{} {}", record.target(), record.args());
        if let Ok(mut lines) = self.0.lock() {
            lines.push(line);
        }
    }
    fn flush(&self) {}
}

/// Capture every log line written on this thread from here on. The tests run
/// on a current-thread runtime, so the client, the HTTP stack and the stub
/// all log on it.
fn capturing() -> &'static Capture {
    static CAPTURE: OnceLock<&'static Capture> = OnceLock::new();
    let capture = CAPTURE.get_or_init(|| {
        let capture: &'static Capture = Box::leak(Box::new(Capture(Mutex::new(Vec::new()))));
        log::set_logger(capture).expect("no other logger in the test binary");
        log::set_max_level(log::LevelFilter::Trace);
        capture
    });
    CAPTURING.with(|c| c.set(true));
    capture
}

// ── A stub hosted service ─────────────────────────────────────────

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    body: String,
}

/// Serves one hosted attempt for [`INVITE`] (by default the choice items of
/// [`QUESTIONS`]) and refuses issuance at close with `refusal_code`. Records
/// every request.
struct Stub {
    origin: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

/// The probe [`Stub::with_probe`] holds back until Start.
const PROBE_QUESTION: &str =
    "Which header lets a client tell a cache it accepts a stale response while revalidating?";

fn terms() -> Value {
    json!({
        "format": "alexandria-hosted-attempt-terms/1",
        "organisation": "Example Ltd", "role": "Backend engineer",
        "items": QUESTIONS.len(), "max_probes": 0,
        "device_requirement": "any",
        "retakes": {"limit": 1, "wait_days": 7, "abandoned_counts": true},
        "disconnect_grace_secs": 600, "retention_days": 180,
        "statements": ["Your answers and the time you take on each item go to Example Ltd."],
    })
}

fn digest(terms: &Value) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(
        serde_json_canonicalizer::to_vec(terms).unwrap(),
    ))
}

/// The choice items of [`QUESTIONS`], as the service sends them (without
/// `seq` and clocks).
fn choice_items() -> Vec<Value> {
    QUESTIONS
        .iter()
        .zip(OPTIONS)
        .map(|(question, option)| {
            json!({
                "question": question, "options": [option, "Neither of these"],
                "select": "one",
            })
        })
        .collect()
}

fn state(items: &[Value], answered: usize) -> Value {
    let item = items.get(answered).map(|item| {
        let mut item = item.clone();
        let fields = json!({
            "seq": answered + 1, "probe": false, "time_limit_ms": 45000,
            "sent_at": NOW, "remaining_ms": 31200,
        });
        if let (Value::Object(item), Value::Object(fields)) = (&mut item, fields) {
            item.extend(fields);
        }
        item
    });
    json!({
        "attempt": {
            "id": ATTEMPT, "status": "open", "opened_at": NOW,
            "items": items.len(), "answered": answered, "max_probes": 0,
            "requires_attested": false, "ready_to_close": item.is_none(),
        },
        "item": item,
    })
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<Seen> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    let header_end = loop {
        let n = socket.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..n]);
        if let Some(at) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let length = head
        .lines()
        .find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while buffer.len() < header_end + length {
        let n = socket.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..n]);
    }
    let mut first = head.lines().next()?.split(' ');
    Some(Seen {
        method: first.next()?.to_string(),
        path: first.next()?.to_string(),
        body: String::from_utf8_lossy(&buffer[header_end..]).to_string(),
    })
}

impl Stub {
    async fn start(refusal_code: &'static str) -> Self {
        Self::serving(refusal_code, choice_items()).await
    }

    async fn serving(refusal_code: &'static str, items: Vec<Value>) -> Self {
        Self::serving_with(refusal_code, items, false).await
    }

    /// The first answer earns a probe, which waits for Start: the answer's
    /// reply carries only its limit, and `POST …/probe` sends it.
    async fn with_probe() -> Self {
        Self::serving_with("rung_below_minimum", choice_items(), true).await
    }

    async fn serving_with(refusal_code: &'static str, items: Vec<Value>, probe: bool) -> Self {
        let items = Arc::new(items);
        let probe_state = Arc::new(Mutex::new(if probe { 1u8 } else { 0 }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
        let answered = Arc::new(Mutex::new(0usize));
        let served = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let (seen, answered, items) = (served.clone(), answered.clone(), items.clone());
                let probe_state = probe_state.clone();
                tokio::spawn(async move {
                    let Some(request) = read_request(&mut socket).await else {
                        return;
                    };
                    seen.lock().unwrap().push(request.clone());
                    let base = "/api/hosted-attempts/for/";
                    let rest = request.path.strip_prefix(base).unwrap_or_default();
                    let route = rest.split_once('/').map(|(_, r)| r).unwrap_or_default();
                    let (status, body) = match (request.method.as_str(), route) {
                        ("GET", r) if r == format!("invites/{INVITE}") => (
                            200,
                            json!({
                                "terms": terms(), "terms_digest": digest(&terms()),
                                "open_attempt": null, "attempts_used": 0,
                                "attempts_allowed": 2, "next_attempt_at": null,
                                "can_open": true,
                            }),
                        ),
                        ("POST", r) if r == format!("invites/{INVITE}") => {
                            let body: Value = serde_json::from_str(&request.body).unwrap();
                            if body["consent"]["accepted"] == true
                                && body["consent"]["terms_digest"] == digest(&terms())
                            {
                                (200, state(&items, 0))
                            } else {
                                (400, json!({"error": "accept", "code": "consent_required"}))
                            }
                        }
                        ("GET", r) if r == format!("attempts/{ATTEMPT}") => {
                            (200, state(&items, *answered.lock().unwrap()))
                        }
                        ("POST", r) if r == format!("attempts/{ATTEMPT}/answers") => {
                            let mut probe = probe_state.lock().unwrap();
                            match *probe {
                                // The first answer earns the probe.
                                1 => {
                                    *probe = 2;
                                    let mut waiting = state(&items, 1);
                                    waiting["item"] = Value::Null;
                                    waiting["pending_probe"] = json!({"time_limit_ms": 30000});
                                    (200, waiting)
                                }
                                // The probe answered: on to the next drawn item.
                                3 => {
                                    *probe = 0;
                                    let mut n = answered.lock().unwrap();
                                    *n += 1;
                                    (200, state(&items, *n))
                                }
                                _ => {
                                    let mut n = answered.lock().unwrap();
                                    *n += 1;
                                    (200, state(&items, *n))
                                }
                            }
                        }
                        ("POST", r) if r == format!("attempts/{ATTEMPT}/probe") => {
                            let mut probe = probe_state.lock().unwrap();
                            let mut sent = state(&items, 1);
                            if *probe == 2 {
                                *probe = 3;
                                sent["item"]["probe"] = json!(true);
                                sent["item"]["question"] = json!(PROBE_QUESTION);
                                sent["item"]["time_limit_ms"] = json!(30000);
                                sent["item"]["remaining_ms"] = json!(30000);
                            }
                            (200, sent)
                        }
                        ("GET", "malformed") => {
                            // An item whose `seq` is its text: serde would
                            // quote it in its error.
                            let mut sent = state(&items, 0);
                            sent["item"]["seq"] = json!(QUESTIONS[0]);
                            (200, sent)
                        }
                        ("POST", r) if r == format!("attempts/{ATTEMPT}/close") => (
                            200,
                            json!({
                                "attempt": {"id": ATTEMPT, "status": "closed"},
                                "result": {
                                    "score": {"correct": 1, "total": 2, "fraction": 0.5},
                                    "probes": {"max": 0, "asked": 0, "passed": 0, "failed": 0,
                                               "late": 0, "unavailable": 0, "probed": false},
                                    "flagged": false,
                                },
                                "issuance": {"state": "refused", "refusal": {
                                    "code": refusal_code, "rung": "process", "required": "probed"}},
                                "close_digest": "00",
                            }),
                        ),
                        _ => (
                            404,
                            json!({"error": "There is no hosted attempt here for this key.",
                                   "code": "not_found"}),
                        ),
                    };
                    let body = body.to_string();
                    let reason = match status {
                        200 => "OK",
                        400 => "Bad Request",
                        _ => "Not Found",
                    };
                    let response = format!(
                        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        Self { origin, seen }
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

// ── Database scan ─────────────────────────────────────────────────

/// Whether any cell of any table holds `needle`.
fn database_holds(conn: &rusqlite::Connection, needle: &str) -> bool {
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for table in tables {
        let mut stmt = conn.prepare(&format!("SELECT * FROM \"{table}\"")).unwrap();
        let columns = stmt.column_count();
        let mut rows = stmt.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for i in 0..columns {
                let text = match row.get_ref(i).unwrap() {
                    rusqlite::types::ValueRef::Text(t) | rusqlite::types::ValueRef::Blob(t) => {
                        String::from_utf8_lossy(t).to_string()
                    }
                    _ => continue,
                };
                if text.contains(needle) {
                    return true;
                }
            }
        }
    }
    false
}

fn finished_session(conn: &rusqlite::Connection, id: &str) {
    fixtures::session_on_profile(conn, id, false, false, None);
    conn.execute(
        "UPDATE integrity_sessions SET commitment_root = 'ab12' WHERE id = ?1",
        [id],
    )
    .unwrap();
}

// ── Tests ─────────────────────────────────────────────────────────

/// A whole hosted attempt, open to close: the items the service sent are in
/// no table and no log line afterwards, and the assistant's bank guard still
/// recognises them.
#[tokio::test]
async fn a_hosted_attempts_item_text_stays_out_of_the_database_and_logs() {
    let logs = capturing();
    let stub = Stub::start("rung_below_minimum").await;
    let (db, _, _, _) = fixtures::setup();
    finished_session(db.conn(), "sess-hosted");
    let key = test_key("candidate");
    let did = derive_did_key(&key);
    let service = HostedService::new(&stub.origin, key, did.clone()).unwrap();

    let terms = service.terms(INVITE).await.unwrap();
    let consent = Consent {
        accepted: true,
        terms_digest: terms.terms_digest.clone(),
    };
    let mut state = service
        .open(INVITE, &consent, false, &NoPlatformKey)
        .await
        .unwrap();
    // A dropped connection: the same item again.
    let resumed = service.resume(&state.attempt.id).await.unwrap();
    assert_eq!(resumed.item, state.item);
    while let Some(item) = state.item.clone() {
        state = service
            .answer(
                ATTEMPT,
                &AnswerBody {
                    seq: item.seq,
                    selected: vec![0],
                    submission: None,
                    device_timings: Some(json!({"shown_ms": 1200})),
                },
                &NoPlatformKey,
            )
            .await
            .unwrap();
    }
    assert!(state.attempt.ready_to_close);

    let body = CloseInputs::read(db.conn(), "sess-hosted", NOW, &TrustConfig::default())
        .unwrap()
        .into_body(&did, ATTEMPT, &NativeAttester);
    let closed = service.close(ATTEMPT, &body, &NoPlatformKey).await.unwrap();
    assert!(matches!(closed.issuance, wire::Issuance::Refused { .. }));
    let close = stub
        .seen()
        .into_iter()
        .find(|s| s.path.ends_with("/close"))
        .unwrap();
    let sent: Value = serde_json::from_str(&close.body).unwrap();
    assert_eq!(sent["integrity"]["commitmentRoot"], "ab12");
    assert!(
        sent.get("evidence").is_none(),
        "an unattested session sends no evidence"
    );

    let lines = logs.0.lock().unwrap().clone();
    assert!(
        lines.iter().any(|l| l.contains("/answers")),
        "the attempt's requests were logged and captured"
    );
    for text in QUESTIONS.iter().chain(OPTIONS.iter()) {
        assert!(!database_holds(db.conn(), text), "stored: {text}");
        assert!(!lines.iter().any(|l| l.contains(text)), "logged: {text}");
    }
    for question in QUESTIONS {
        let pasted = format!("Can you help me answer this? {question}");
        assert!(
            crate::ai_chat::bank_guard::matches_assessment_item(db.conn(), &pasted),
            "the assistant's bank guard does not know: {question}"
        );
    }
}

/// A probe states its limit before its text arrives (story 15): the answer
/// that earns it carries only the limit, and the probe is fetched with
/// `POST …/probe` once the candidate presses Start.
#[tokio::test]
async fn a_waiting_probe_is_fetched_only_on_start() {
    let stub = Stub::with_probe().await;
    let key = test_key("candidate");
    let did = derive_did_key(&key);
    let service = HostedService::new(&stub.origin, key, did).unwrap();
    let terms = service.terms(INVITE).await.unwrap();
    let consent = Consent {
        accepted: true,
        terms_digest: terms.terms_digest.clone(),
    };
    let state = service
        .open(INVITE, &consent, false, &NoPlatformKey)
        .await
        .unwrap();
    let first = state.item.unwrap();
    let answer = AnswerBody {
        seq: first.seq,
        selected: vec![0],
        submission: None,
        device_timings: None,
    };
    let waiting = service
        .answer(ATTEMPT, &answer, &NoPlatformKey)
        .await
        .unwrap();
    assert!(waiting.item.is_none());
    assert!(!waiting.attempt.ready_to_close);
    assert_eq!(
        waiting.pending_probe.as_ref().map(|p| p.time_limit_ms),
        Some(30000)
    );
    assert!(
        !stub.seen().iter().any(|s| s.path.ends_with("/probe")),
        "nothing asks for the probe before Start"
    );

    let started = service.start_probe(ATTEMPT).await.unwrap();
    let probe = started.item.expect("the probe, sent on Start");
    assert!(probe.probe);
    assert_eq!(probe.question, PROBE_QUESTION);
    assert!(started.pending_probe.is_none());
    let asked = stub
        .seen()
        .into_iter()
        .find(|s| s.path.ends_with("/probe"))
        .unwrap();
    assert_eq!(asked.method, "POST");
    // The bank guard knows the probe like any other item.
    let (db, _, _, _) = fixtures::setup();
    assert!(crate::ai_chat::bank_guard::matches_assessment_item(
        db.conn(),
        &format!("Help me with this: {PROBE_QUESTION}")
    ));
}

/// A reply the client cannot read never quotes itself into the error: it may
/// hold item text, and command errors reach the screen and logs.
#[tokio::test]
async fn a_malformed_reply_is_refused_without_quoting_it() {
    let stub = Stub::start("rung_below_minimum").await;
    let key = test_key("candidate");
    let did = derive_did_key(&key);
    let service = HostedService::new(&stub.origin, key, did).unwrap();
    let error = service
        .send::<wire::AttemptState>(
            reqwest::Method::GET,
            &service.path("malformed"),
            None,
            None,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "protocol");
    let message = error.into_command_error();
    assert!(!message.contains("phantom"), "quoted the reply: {message}");
    assert!(
        !message.contains(QUESTIONS[0]),
        "quoted the reply: {message}"
    );
}

/// Declining the terms sends nothing: no open request, so no item.
#[tokio::test]
async fn no_request_reaches_the_service_before_consent_is_accepted() {
    let stub = Stub::start("rung_below_minimum").await;
    let key = test_key("candidate");
    let did = derive_did_key(&key);
    let service = HostedService::new(&stub.origin, key, did).unwrap();
    let terms = service.terms(INVITE).await.unwrap();

    for consent in [
        Consent {
            accepted: false,
            terms_digest: terms.terms_digest.clone(),
        },
        Consent {
            accepted: true,
            terms_digest: String::new(),
        },
    ] {
        let refused = service
            .open(INVITE, &consent, false, &NoPlatformKey)
            .await
            .unwrap_err();
        assert_eq!(refused.code(), "consent_required");
    }
    let sent: Vec<String> = stub
        .seen()
        .iter()
        .map(|s| format!("{} {}", s.method, s.path))
        .collect();
    assert_eq!(sent.len(), 1, "only the terms were asked for: {sent:?}");
    assert!(sent[0].starts_with("GET "));
}

// ── Plugin items ──────────────────────────────────────────────────

const PROMPT: &str =
    "Read a whole number from input and print the sum of its decimal digits on one line.";
const STARTER: &str =
    "// Read the number with readLine() and print the digit sum\nconst n = readLine();\n";
const SOLUTION: &str = "console.log([...readLine()].reduce((a, d) => a + Number(d), 0));";

/// The plugin identifier of the JavaScript editor this app bundles.
fn bundled_js_editor() -> String {
    let bundle = crate::plugins::builtins::BUILTIN_PLUGINS
        .iter()
        .find(|b| b.slug == "editor-javascript")
        .unwrap();
    crate::plugins::verifier::compute_plugin_cid(bundle.manifest_json)
}

fn plugin_item(cid: &str) -> Value {
    json!({
        "kind": "plugin", "question": PROMPT, "options": [], "select": "submission",
        "plugin": {"cid": cid, "starter": STARTER},
    })
}

/// A plugin item arrives with its bundled plugin and starter, is answered with
/// the editor's submission, and its text is held by the assistant's bank guard
/// and found in no table and no log line.
#[tokio::test]
async fn a_hosted_plugin_item_is_answered_with_the_editors_submission() {
    let logs = capturing();
    let cid = bundled_js_editor();
    let stub = Stub::serving("rung_below_minimum", vec![plugin_item(&cid)]).await;
    let (db, _, _, _) = fixtures::setup();
    let key = test_key("candidate");
    let did = derive_did_key(&key);
    let service = HostedService::new(&stub.origin, key, did).unwrap();

    let terms = service.terms(INVITE).await.unwrap();
    let consent = Consent {
        accepted: true,
        terms_digest: terms.terms_digest,
    };
    let state = service
        .open(INVITE, &consent, false, &NoPlatformKey)
        .await
        .unwrap();
    let item = state.item.expect("the plugin item");
    let plugin = item.plugin.clone().expect("sent as a plugin item");
    assert_eq!(plugin.cid, cid);
    assert_eq!(plugin.starter, STARTER);
    assert!(item.is_plugin());

    let after = service
        .answer(
            ATTEMPT,
            &AnswerBody {
                seq: item.seq,
                selected: vec![],
                submission: Some(json!({"source": SOLUTION})),
                device_timings: Some(json!({"shown_ms": 90_000})),
            },
            &NoPlatformKey,
        )
        .await
        .unwrap();
    assert!(after.attempt.ready_to_close);
    let answer = stub
        .seen()
        .into_iter()
        .find(|s| s.path.ends_with("/answers"))
        .unwrap();
    let sent: Value = serde_json::from_str(&answer.body).unwrap();
    assert_eq!(sent["submission"], json!({"source": SOLUTION}));
    assert_eq!(sent["selected"], json!([]));

    let lines = logs.0.lock().unwrap().clone();
    for text in [PROMPT, STARTER, SOLUTION] {
        assert!(!database_holds(db.conn(), text), "stored: {text}");
        assert!(!lines.iter().any(|l| l.contains(text)), "logged: {text}");
    }
    for text in [PROMPT, STARTER] {
        let pasted = format!("Can you write this for me? {text}");
        assert!(
            crate::ai_chat::bank_guard::matches_assessment_item(db.conn(), &pasted),
            "the assistant's bank guard does not know: {text}"
        );
    }
}

/// An item naming a plugin this app does not bundle is not shown: only the
/// exact bundled plugins run a hosted item.
#[tokio::test]
async fn a_plugin_item_naming_a_plugin_the_app_does_not_bundle_is_refused() {
    let stub = Stub::serving("rung_below_minimum", vec![plugin_item(&"ab".repeat(32))]).await;
    let key = test_key("candidate");
    let did = derive_did_key(&key);
    let service = HostedService::new(&stub.origin, key, did).unwrap();
    let terms = service.terms(INVITE).await.unwrap();
    let consent = Consent {
        accepted: true,
        terms_digest: terms.terms_digest,
    };
    let refused = service
        .open(INVITE, &consent, false, &NoPlatformKey)
        .await
        .unwrap_err();
    assert_eq!(refused.code(), "plugin_not_bundled");
}
