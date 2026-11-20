//! The session endpoint: one request in, one response out. The listener
//! hands every request here; tests call it directly too.
//!
//! Checks run in this order: the Host is the session's own loopback address
//! (else 421, DNS rebinding), the Origin is the package origin (else 403
//! with no CORS headers, a missing Origin included), the session is open,
//! then per resource: the fetch URL takes one POST; xAPI resources need the
//! session's token (401) and a version header (400).

use std::collections::HashMap;

use alexandria_studio::xapi::cmi5::{self as rules, Refusal};
use serde_json::{json, Value};
use tauri::http::header::{HeaderValue, CONTENT_TYPE};
use tauri::http::{Method, Request, Response, StatusCode};

use super::super::{in_transaction, AttemptError};
use super::store::{self, DocumentKey, DocumentKind, StatementQuery};
use super::Live;
use crate::package_serving;
use crate::packages::PackageDb;

/// The largest request body the endpoint reads; cmi5 needs no attachments.
pub(super) const MAX_BODY_BYTES: usize = 1 << 20;

/// The versions the endpoint answers for, newest first.
const VERSIONS: [&str; 2] = ["2.0.0", "1.0.3"];

fn base(status: StatusCode) -> tauri::http::response::Builder {
    Response::builder()
        .status(status)
        .header("Cache-Control", "no-store")
        .header("X-Experience-API-Version", "1.0.3")
}

/// A response the content may read: CORS for the package origin only.
fn readable(status: StatusCode, content_type: Option<&str>, body: Vec<u8>) -> Response<Vec<u8>> {
    let mut builder = base(status)
        .header("Access-Control-Allow-Origin", package_serving::origin())
        .header("Vary", "Origin")
        .header(
            "Access-Control-Expose-Headers",
            "ETag, Last-Modified, Content-Type, X-Experience-API-Version",
        );
    if let Some(content_type) = content_type {
        builder = builder.header(CONTENT_TYPE, content_type);
    }
    builder.body(body).expect("endpoint headers are valid")
}

fn json(status: StatusCode, value: Value) -> Response<Vec<u8>> {
    readable(
        status,
        Some("application/json"),
        value.to_string().into_bytes(),
    )
}

fn error(status: StatusCode, message: impl Into<String>) -> Response<Vec<u8>> {
    json(status, json!({ "error": message.into() }))
}

fn empty(status: StatusCode) -> Response<Vec<u8>> {
    readable(status, None, Vec::new())
}

/// A refusal before the caller is known to be the package: no CORS headers,
/// so a page elsewhere cannot even read why.
fn opaque(status: StatusCode) -> Response<Vec<u8>> {
    base(status)
        .body(Vec::new())
        .expect("endpoint headers are valid")
}

fn header<'a>(request: &'a Request<Vec<u8>>, name: &str) -> Option<&'a str> {
    request
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
}

fn query(request: &Request<Vec<u8>>) -> HashMap<String, String> {
    url::form_urlencoded::parse(request.uri().query().unwrap_or("").as_bytes())
        .into_owned()
        .collect()
}

/// Answers one request to the session's endpoint.
pub(super) async fn handle<D: PackageDb>(
    db: &D,
    live: &Live,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    if header(&request, "host") != Some(format!("127.0.0.1:{}", live.port).as_str()) {
        return opaque(StatusCode::MISDIRECTED_REQUEST);
    }
    if header(&request, "origin") != Some(package_serving::origin()) {
        return opaque(StatusCode::FORBIDDEN);
    }
    if request.method() == Method::OPTIONS {
        return preflight(&request);
    }
    if live.is_closed() {
        return error(StatusCode::FORBIDDEN, "the session is not open");
    }
    let path = request.uri().path().to_string();
    if let Some(code) = path.strip_prefix("/fetch/") {
        return fetch(live, request.method(), code);
    }
    let Some(resource) = path.strip_prefix("/xapi/") else {
        return empty(StatusCode::NOT_FOUND);
    };
    if resource == "about" && request.method() == Method::GET {
        return json(StatusCode::OK, json!({ "version": VERSIONS }));
    }
    if header(&request, "authorization") != Some(format!("Basic {}", live.token).as_str()) {
        return error(StatusCode::UNAUTHORIZED, "the session's token is required");
    }
    if header(&request, "x-experience-api-version").is_none() {
        return error(
            StatusCode::BAD_REQUEST,
            "the X-Experience-API-Version header is required",
        );
    }
    // cmi5 8.1.2: the token is good for nothing once the unit terminated.
    if live
        .record
        .lock()
        .expect("cmi5 session lock poisoned")
        .ended()
    {
        return error(StatusCode::FORBIDDEN, "the session has ended");
    }
    match resource {
        "statements" => statements(db, live, request).await,
        "activities/state" => documents(db, live, DocumentKind::State, request).await,
        "agents/profile" => documents(db, live, DocumentKind::AgentProfile, request).await,
        "activities/profile" => documents(db, live, DocumentKind::ActivityProfile, request).await,
        _ => empty(StatusCode::NOT_FOUND),
    }
}

fn preflight(request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let mut response = empty(StatusCode::NO_CONTENT);
    let headers = response.headers_mut();
    headers.insert(
        "Access-Control-Allow-Methods",
        HeaderValue::from_static("GET, PUT, POST, DELETE"),
    );
    headers.insert(
        "Access-Control-Allow-Headers",
        HeaderValue::from_static(
            "Authorization, Content-Type, X-Experience-API-Version, If-Match, If-None-Match",
        ),
    );
    headers.insert("Access-Control-Max-Age", HeaderValue::from_static("600"));
    if request
        .headers()
        .contains_key("access-control-request-private-network")
    {
        headers.insert(
            "Access-Control-Allow-Private-Network",
            HeaderValue::from_static("true"),
        );
    }
    response
}

/// The fetch URL (cmi5 8.2): one POST returns the token; every later one an
/// error. cmi5 asks for 200 even then.
fn fetch(live: &Live, method: &Method, code: &str) -> Response<Vec<u8>> {
    if method != Method::POST {
        return error(StatusCode::METHOD_NOT_ALLOWED, "the fetch URL takes POST");
    }
    if code != live.fetch_code {
        return json(
            StatusCode::OK,
            json!({ "error-code": "2", "error-text": "This is not the session's fetch URL." }),
        );
    }
    let mut used = live.fetch_used.lock().expect("cmi5 session lock poisoned");
    if *used {
        return json(
            StatusCode::OK,
            json!({ "error-code": "1", "error-text": "The authorization token has already been returned." }),
        );
    }
    *used = true;
    json(StatusCode::OK, json!({ "auth-token": live.token }))
}

fn is_uuid(text: &str) -> bool {
    uuid::Uuid::parse_str(text).is_ok() && text.len() == 36
}

async fn statements<D: PackageDb>(
    db: &D,
    live: &Live,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let params = query(&request);
    match *request.method() {
        Method::GET => read_statements(db, live, &params).await,
        Method::POST | Method::PUT => {
            let put = request.method() == Method::PUT;
            let body: Value = match serde_json::from_slice(request.body()) {
                Ok(body) => body,
                Err(e) => return error(StatusCode::BAD_REQUEST, format!("not JSON: {e}")),
            };
            let mut list = match body {
                Value::Array(list) if !put => list,
                Value::Object(_) => vec![body],
                _ => {
                    return error(
                        StatusCode::BAD_REQUEST,
                        "a statement or a list of statements",
                    )
                }
            };
            if put {
                let Some(id) = params.get("statementId") else {
                    return error(StatusCode::BAD_REQUEST, "PUT needs a statementId");
                };
                match list[0].get("id").and_then(Value::as_str) {
                    Some(own) if own != id => {
                        return error(StatusCode::BAD_REQUEST, "statementId and id differ")
                    }
                    _ => list[0]["id"] = Value::from(id.as_str()),
                }
            }
            write_statements(db, live, list, put).await
        }
        _ => empty(StatusCode::METHOD_NOT_ALLOWED),
    }
}

async fn write_statements<D: PackageDb>(
    db: &D,
    live: &Live,
    mut list: Vec<Value>,
    put: bool,
) -> Response<Vec<u8>> {
    let _writing = live.writing.lock().await;
    if live.is_closed() {
        return error(StatusCode::FORBIDDEN, "the session is not open");
    }
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let mut ids = Vec::with_capacity(list.len());
    for statement in &mut list {
        let Some(object) = statement.as_object_mut() else {
            return error(StatusCode::BAD_REQUEST, "a statement is a JSON object");
        };
        let id = match object.get("id") {
            Some(Value::String(id)) if is_uuid(id) => id.clone(),
            Some(_) => return error(StatusCode::BAD_REQUEST, "a statement id is a UUID"),
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                object.insert("id".into(), id.clone().into());
                id
            }
        };
        if ids.contains(&id) {
            return error(StatusCode::BAD_REQUEST, "a batch repeats a statement id");
        }
        object
            .entry("timestamp")
            .or_insert_with(|| now.clone().into());
        ids.push(id);
    }

    let mut record = live
        .record
        .lock()
        .expect("cmi5 session lock poisoned")
        .clone();
    let mut batch = Vec::with_capacity(list.len());
    for statement in list {
        match rules::check(&statement, &live.facts, &record) {
            Ok(checked) => {
                record.apply(&checked);
                batch.push((statement, checked));
            }
            Err(Refusal::Malformed(why)) => return error(StatusCode::BAD_REQUEST, why),
            Err(Refusal::Rule(why)) => return error(StatusCode::FORBIDDEN, why),
        }
    }
    let attempt_id = live.attempt_id.clone();
    let unit_id = live.unit_id.clone();
    let facts = live.facts.clone();
    let stored = in_transaction(db, "packages.cmi5-statements", move |db| {
        store::store_batch(db, &attempt_id, &unit_id, &facts, &batch)
    })
    .await;
    match stored {
        Ok(Ok(())) => {
            *live.record.lock().expect("cmi5 session lock poisoned") = record;
            if put {
                empty(StatusCode::NO_CONTENT)
            } else {
                json(StatusCode::OK, json!(ids))
            }
        }
        Ok(Err(conflict)) => error(StatusCode::CONFLICT, conflict.0),
        Err(AttemptError::Closed(why)) => error(StatusCode::FORBIDDEN, why),
        Err(e) => {
            log::warn!("cmi5 statements not stored: {e}");
            error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "the statements were not stored",
            )
        }
    }
}

async fn read_statements<D: PackageDb>(
    db: &D,
    live: &Live,
    params: &HashMap<String, String>,
) -> Response<Vec<u8>> {
    let attempt_id = live.attempt_id.clone();
    if params.contains_key("voidedStatementId") {
        return error(StatusCode::NOT_FOUND, "nothing is voided here");
    }
    if let Some(id) = params.get("statementId").cloned() {
        let found = in_transaction(db, "packages.cmi5-statement", move |db| {
            store::statement(db.conn(), &attempt_id, &id)
        })
        .await;
        return match found {
            Ok(Some(statement)) => json(StatusCode::OK, statement),
            Ok(None) => error(StatusCode::NOT_FOUND, "no such statement"),
            Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        };
    }
    if let Some(agent) = params.get("agent") {
        let named: Value = serde_json::from_str(agent).unwrap_or(Value::Null);
        if !rules::same_agent(&named, &live.facts.actor) {
            return json(StatusCode::OK, json!({ "statements": [], "more": "" }));
        }
    }
    let filters = StatementQuery {
        verb: params.get("verb").cloned(),
        activity: params.get("activity").cloned(),
        registration: params.get("registration").cloned(),
        since: params.get("since").cloned(),
        until: params.get("until").cloned(),
        limit: params.get("limit").and_then(|l| l.parse().ok()),
        ascending: params.get("ascending").is_some_and(|a| a == "true"),
    };
    let found = in_transaction(db, "packages.cmi5-statements-query", move |db| {
        store::statements(db.conn(), &attempt_id, &filters)
    })
    .await;
    match found {
        Ok(statements) => json(
            StatusCode::OK,
            json!({ "statements": statements, "more": "" }),
        ),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// Merges a JSON object into another, as a document POST does.
fn merge(stored: &[u8], incoming: &[u8]) -> Option<Vec<u8>> {
    let mut stored: Value = serde_json::from_slice(stored).ok()?;
    let incoming: Value = serde_json::from_slice(incoming).ok()?;
    let (Some(target), Some(source)) = (stored.as_object_mut(), incoming.as_object()) else {
        return None;
    };
    for (key, value) in source {
        target.insert(key.clone(), value.clone());
    }
    Some(stored.to_string().into_bytes())
}

async fn documents<D: PackageDb>(
    db: &D,
    live: &Live,
    kind: DocumentKind,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let params = query(&request);
    if kind != DocumentKind::ActivityProfile {
        let Some(agent) = params.get("agent") else {
            return error(StatusCode::BAD_REQUEST, "agent is required");
        };
        let agent: Value = serde_json::from_str(agent).unwrap_or(Value::Null);
        if !rules::same_agent(&agent, &live.facts.actor) {
            return error(
                StatusCode::FORBIDDEN,
                "documents belong to the session's actor",
            );
        }
    }
    let activity_id = match kind {
        DocumentKind::AgentProfile => String::new(),
        _ => match params.get("activityId") {
            Some(id) => id.clone(),
            None => return error(StatusCode::BAD_REQUEST, "activityId is required"),
        },
    };
    let registration = match kind {
        DocumentKind::State => params.get("registration").cloned().unwrap_or_default(),
        _ => String::new(),
    };
    // cmi5 8.1.4: state requests name the registration the unit was given.
    if kind == DocumentKind::State && registration != live.facts.registration {
        return error(
            StatusCode::FORBIDDEN,
            "state belongs to the session's registration",
        );
    }
    let id_name = if kind == DocumentKind::State {
        "stateId"
    } else {
        "profileId"
    };
    let document_id = params.get(id_name).cloned().unwrap_or_default();
    let method = request.method().clone();
    if kind == DocumentKind::State
        && document_id == rules::LAUNCH_DATA_STATE_ID
        && method != Method::GET
    {
        return error(
            StatusCode::FORBIDDEN,
            "LMS.LaunchData is the launching system's",
        );
    }
    if document_id.is_empty()
        && !(method == Method::GET || (method == Method::DELETE && kind == DocumentKind::State))
    {
        return error(StatusCode::BAD_REQUEST, format!("{id_name} is required"));
    }
    let if_match = header(&request, "if-match").map(str::to_string);
    let if_none_match = header(&request, "if-none-match").map(str::to_string);
    let content_type = header(&request, "content-type")
        .unwrap_or("application/octet-stream")
        .to_string();
    let body = request.into_body();
    let package_hash = live.package_hash.clone();

    let answer = in_transaction(db, "packages.cmi5-documents", move |db| {
        let conn = db.conn();
        let key = DocumentKey {
            package_hash: &package_hash,
            kind,
            activity_id: &activity_id,
            registration: &registration,
            document_id: &document_id,
        };
        if method == Method::GET {
            if document_id.is_empty() {
                return Ok(json(
                    StatusCode::OK,
                    json!(store::document_ids(conn, &key)?),
                ));
            }
            return Ok(match store::get_document(conn, &key)? {
                Some(document) => {
                    let mut response =
                        readable(StatusCode::OK, Some(&document.content_type), document.body);
                    if let Ok(value) = HeaderValue::from_str(&document.etag) {
                        response.headers_mut().insert("ETag", value);
                    }
                    if let Ok(value) = HeaderValue::from_str(&document.updated_at) {
                        response.headers_mut().insert("Last-Modified", value);
                    }
                    response
                }
                None => error(StatusCode::NOT_FOUND, "no such document"),
            });
        }
        // cmi5 11: the learner preferences document keeps its shape.
        let preferences = kind == DocumentKind::AgentProfile
            && document_id == rules::LEARNER_PREFERENCES_PROFILE_ID;
        let refused = || {
            error(
                StatusCode::FORBIDDEN,
                "the learner preferences are a JSON object with languagePreference and audioPreference",
            )
        };
        // Checked before the preconditions: a malformed document is refused
        // whatever is stored.
        let writing = matches!(method, Method::PUT | Method::POST);
        if preferences
            && writing
            && (!content_type.starts_with("application/json")
                || (method == Method::PUT && !rules::valid_learner_preferences(&body)))
        {
            return Ok(refused());
        }
        let existing = if document_id.is_empty() {
            None
        } else {
            store::get_document(conn, &key)?
        };
        // Concurrency control (xAPI): a precondition that fails refuses.
        if let Some(expected) = &if_match {
            let matches = existing
                .as_ref()
                .is_some_and(|d| expected == "*" || expected == &d.etag);
            if !matches {
                return Ok(error(StatusCode::PRECONDITION_FAILED, "If-Match failed"));
            }
        }
        if if_none_match.as_deref() == Some("*") && existing.is_some() {
            return Ok(error(
                StatusCode::PRECONDITION_FAILED,
                "If-None-Match failed",
            ));
        }
        if method == Method::PUT
            && kind != DocumentKind::State
            && existing.is_some()
            && if_match.is_none()
            && if_none_match.is_none()
        {
            return Ok(error(
                StatusCode::CONFLICT,
                "replacing a profile document needs If-Match or If-None-Match",
            ));
        }
        match method {
            Method::PUT => {
                store::put_document(conn, &key, &content_type, &body)?;
                Ok(empty(StatusCode::NO_CONTENT))
            }
            Method::POST => {
                let merged = match &existing {
                    Some(document) => match merge(&document.body, &body) {
                        Some(merged) => merged,
                        None => {
                            return Ok(error(
                                StatusCode::BAD_REQUEST,
                                "POST merges JSON objects only",
                            ))
                        }
                    },
                    None => {
                        if serde_json::from_slice::<Value>(&body)
                            .ok()
                            .is_none_or(|v| !v.is_object())
                        {
                            return Ok(error(
                                StatusCode::BAD_REQUEST,
                                "POST merges JSON objects only",
                            ));
                        }
                        body
                    }
                };
                if preferences && !rules::valid_learner_preferences(&merged) {
                    return Ok(refused());
                }
                store::put_document(conn, &key, "application/json", &merged)?;
                Ok(empty(StatusCode::NO_CONTENT))
            }
            Method::DELETE => {
                store::delete_documents(conn, &key)?;
                Ok(empty(StatusCode::NO_CONTENT))
            }
            _ => Ok(empty(StatusCode::METHOD_NOT_ALLOWED)),
        }
    })
    .await;
    answer.unwrap_or_else(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}
