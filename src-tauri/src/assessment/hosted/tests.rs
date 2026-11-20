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

struct Capture(Mutex<Vec<String>>);

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        if let Ok(mut lines) = self.0.lock() {
            lines.push(format!("{} {}", record.target(), record.args()));
        }
    }
    fn flush(&self) {}
}

/// Every log line the process writes from the first call on, at every level.
fn logs() -> &'static Capture {
    static CAPTURE: OnceLock<&'static Capture> = OnceLock::new();
    CAPTURE.get_or_init(|| {
        let capture: &'static Capture = Box::leak(Box::new(Capture(Mutex::new(Vec::new()))));
        log::set_logger(capture).expect("no other logger in the test binary");
        log::set_max_level(log::LevelFilter::Trace);
        capture
    })
}

// ── A stub hosted service ─────────────────────────────────────────

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    body: String,
}

/// Serves one hosted attempt of [`QUESTIONS`] for [`INVITE`] and refuses
/// issuance at close with `refusal_code`. Records every request.
struct Stub {
    origin: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

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

fn state(answered: usize) -> Value {
    let item = (answered < QUESTIONS.len()).then(|| {
        json!({
            "seq": answered + 1, "probe": false,
            "question": QUESTIONS[answered],
            "options": [OPTIONS[answered], "Neither of these"],
            "select": "one", "time_limit_ms": 45000,
            "sent_at": NOW, "remaining_ms": 31200,
        })
    });
    json!({
        "attempt": {
            "id": ATTEMPT, "status": "open", "opened_at": NOW,
            "items": QUESTIONS.len(), "answered": answered, "max_probes": 0,
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
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
        let answered = Arc::new(Mutex::new(0usize));
        let served = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let (seen, answered) = (served.clone(), answered.clone());
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
                                (200, state(0))
                            } else {
                                (400, json!({"error": "accept", "code": "consent_required"}))
                            }
                        }
                        ("GET", r) if r == format!("attempts/{ATTEMPT}") => {
                            (200, state(*answered.lock().unwrap()))
                        }
                        ("POST", r) if r == format!("attempts/{ATTEMPT}/answers") => {
                            let mut n = answered.lock().unwrap();
                            *n += 1;
                            (200, state(*n))
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
    let logs = logs();
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
