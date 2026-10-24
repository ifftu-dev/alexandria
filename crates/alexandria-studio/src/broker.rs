//! OS-local assistant broker shared by the app and its native lifecycle tests.
//!
//! The running app owns the profile database and grants. `alexandria-mcp`
//! sends one newline-delimited JSON request per connection; every request is
//! reauthorized against scope, expiry and the current profile epoch.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use crate::{
    credentials,
    grants::{Activity, Grants, StudioGrant},
    learning, skills, store, Error, Result,
};

pub const MAX_REQUEST_BYTES: usize = 262_144;
pub const MAX_CONCURRENT_REQUESTS: usize = 8;
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a lesson fetch from peers may take before it is reported unavailable.
pub const CONTENT_TIMEOUT: Duration = Duration::from_secs(10);

pub type ContentFuture<'a> =
    Pin<Box<dyn Future<Output = std::result::Result<Vec<u8>, String>> + Send + 'a>>;

/// Application state the broker needs. The app implements this over its
/// Tauri state; tests implement it over fixture profiles.
pub trait BrokerHost: Send + Sync + 'static {
    /// Requests hold the read side through authorization, database work and
    /// response writing. Lock, switch and revoke take the write side.
    fn gate(&self) -> &tokio::sync::RwLock<()>;
    fn grants(&self) -> &Mutex<Grants>;
    fn epoch(&self) -> u64;
    fn now(&self) -> i64 {
        chrono::Utc::now().timestamp()
    }
    /// Runs `f` against the unlocked profile database, or fails when locked.
    fn with_db(
        &self,
        f: &mut dyn FnMut(&Connection) -> Result<Value>,
    ) -> std::result::Result<Value, String>;
    /// Records that an assistant called while an assessment was in progress.
    /// The app writes it as an integrity flag; test hosts ignore it.
    fn report_attempt_flag(&self, _db: &Connection, _flag: &str) {}
    /// Fetches a published content blob by hash: from this device, else from
    /// peers. Hosts without a content store report it unavailable.
    fn fetch_content(&self, _blob: &str) -> ContentFuture<'_> {
        Box::pin(std::future::ready(Err("no content store".to_string())))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    token: String,
    operation: String,
    #[serde(default)]
    course_id: String,
    #[serde(default)]
    element_id: String,
    #[serde(default)]
    fingerprint: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    request_id: String,
    #[serde(default)]
    query: String,
    #[serde(default)]
    skill_id: String,
    #[serde(default)]
    stored_only: bool,
    #[serde(default)]
    limit: u32,
    #[serde(default)]
    cursor: String,
    #[serde(default)]
    start: usize,
    #[serde(default)]
    goal_kind: String,
    #[serde(default)]
    key: String,
    #[serde(default)]
    board: String,
    #[serde(default)]
    grade: String,
    #[serde(default)]
    goal_skill_ids: Vec<String>,
    #[serde(default)]
    credential_id: String,
    #[serde(default)]
    include_revoked: bool,
    #[serde(default)]
    audience: String,
    #[serde(default)]
    presentation_json: String,
}

/// A lesson body fetched between the two authorized phases of `read_lesson`.
struct Fetched {
    blob: String,
    content: std::result::Result<Vec<u8>, String>,
}

enum Dispatched {
    Value(Value),
    NeedsContent(String),
}

enum Step {
    Respond(Value),
    Fetch(String),
}

/// Accepts connections until the listener fails. Excess concurrent
/// connections are closed without a response.
pub async fn serve<H: BrokerHost>(listener: UnixListener, host: Arc<H>) {
    let capacity = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_REQUESTS));
    while let Ok((stream, _)) = listener.accept().await {
        let Ok(permit) = capacity.clone().try_acquire_owned() else {
            continue;
        };
        let host = host.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _ = tokio::time::timeout(REQUEST_TIMEOUT, handle(&*host, stream)).await;
        });
    }
}

async fn handle<H: BrokerHost>(host: &H, stream: UnixStream) -> std::result::Result<(), ()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = tokio::io::BufReader::new(reader.take(MAX_REQUEST_BYTES as u64 + 1));
    let mut bytes = Vec::new();
    reader.read_until(b'\n', &mut bytes).await.map_err(|_| ())?;
    let response = if bytes.len() > MAX_REQUEST_BYTES {
        json!({"error":"invalid_input: broker request too large"})
    } else if bytes.last() != Some(&b'\n') {
        json!({"error":"invalid_input: incomplete broker request"})
    } else {
        match serde_json::from_slice::<Request>(&bytes) {
            Err(_) => json!({"error":"invalid_input: malformed broker request"}),
            Ok(request) => {
                let blob = {
                    let _lease = host.gate().read().await;
                    match authorized_step(host, &request, None) {
                        Step::Respond(response) => {
                            return write_frame(&mut writer, &response).await
                        }
                        Step::Fetch(blob) => blob,
                    }
                };
                // Published lesson content is fetched outside the lease, so a slow
                // peer cannot hold up a profile lock. The request is authorized
                // again, under the lease, before anything is returned.
                let content = tokio::time::timeout(CONTENT_TIMEOUT, host.fetch_content(&blob))
                    .await
                    .unwrap_or_else(|_| Err("timed out".into()));
                let fetched = Fetched { blob, content };
                let _lease = host.gate().read().await;
                let response = match authorized_step(host, &request, Some(&fetched)) {
                    Step::Respond(response) => response,
                    Step::Fetch(_) => {
                        json!({"error":"unavailable: the lesson changed while it was loading; try again"})
                    }
                };
                return write_frame(&mut writer, &response).await;
            }
        }
    };
    write_frame(&mut writer, &response).await
}

async fn write_frame(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    response: &Value,
) -> std::result::Result<(), ()> {
    let mut frame = response.to_string().into_bytes();
    frame.push(b'\n');
    writer.write_all(&frame).await.map_err(|_| ())
}

/// Authorizes and runs one request against the profile database. A lesson
/// whose body must first be fetched asks for it instead of responding.
fn authorized_step<H: BrokerHost>(host: &H, request: &Request, fetched: Option<&Fetched>) -> Step {
    let mut blob = None;
    let result = host.with_db(&mut |db| match dispatch(
        db,
        host.grants(),
        host.epoch(),
        host.now(),
        request,
        fetched,
        &|db| host.report_attempt_flag(db, "mcp_call_during_attempt"),
    )? {
        Dispatched::Value(value) => Ok(value),
        Dispatched::NeedsContent(needed) => {
            blob = Some(needed);
            Ok(Value::Null)
        }
    });
    match (result, blob) {
        (Ok(_), Some(blob)) => Step::Fetch(blob),
        (Ok(value), None) => Step::Respond(json!({"result":value})),
        (Err(error), _) => Step::Respond(json!({"error":error})),
    }
}

fn dispatch(
    db: &Connection,
    grants: &Mutex<Grants>,
    epoch: u64,
    now: i64,
    request: &Request,
    fetched: Option<&Fetched>,
    on_attempt: &dyn Fn(&Connection),
) -> Result<Dispatched> {
    let scope = match request.operation.as_str() {
        "search_catalog"
        | "get_course"
        | "read_lesson"
        | "get_skill_graph"
        | "get_learning_progress"
        | "resolve_goal"
        | "compute_learning_path" => "learning:read",
        "list_my_credentials" | "get_credential" => "credentials:read",
        "verify_presentation" => "credentials:verify",
        "list_course_drafts" | "read_lesson_draft" => "drafts:read",
        "propose_lesson_draft" => "drafts:propose",
        _ => return Err(Error::Permission),
    };
    let grant = grants.lock().map_err(|_| Error::Permission)?.authorize(
        &request.token,
        scope,
        epoch,
        now,
    )?;
    // After authorization, so an invalid token learns nothing about state.
    let gated = crate::gate::require_no_assessment(db);
    if matches!(gated, Err(Error::AssessmentActive)) {
        on_attempt(db);
    }
    let outcome = gated.and_then(|()| operate(db, &grant, request, fetched));
    // A lesson that must come from peers spends the grant's fetch budget.
    let outcome = match outcome {
        Ok(Dispatched::NeedsContent(blob)) => {
            let allowed = grants
                .lock()
                .map_err(|_| Error::Permission)?
                .take_fetch(&grant.id, now);
            if allowed {
                return Ok(Dispatched::NeedsContent(blob));
            }
            Err(Error::Unavailable(
                "this assistant has fetched its hourly limit of lessons from peers; lessons stored on this device are still readable".into(),
            ))
        }
        other => other,
    };
    let label = match &outcome {
        Ok(Dispatched::Value(value)) => value
            .get("status")
            .and_then(Value::as_str)
            .filter(|_| request.operation == "read_lesson")
            .unwrap_or("returned")
            .to_string(),
        Ok(Dispatched::NeedsContent(_)) => "fetching".into(),
        Err(error) => error
            .to_string()
            .split(':')
            .next()
            .unwrap_or("error")
            .to_string(),
    };
    if let Ok(mut grants) = grants.lock() {
        grants.record(
            &grant.id,
            Activity {
                at: now,
                tool: request.operation.clone(),
                detail: activity_detail(request),
                outcome: label,
            },
        );
    }
    outcome
}

/// What a request asked about, for the learner's activity log.
fn activity_detail(request: &Request) -> String {
    let clip = |s: &str| s.chars().take(80).collect::<String>();
    match request.operation.as_str() {
        "read_lesson" | "read_lesson_draft" | "propose_lesson_draft" => {
            clip(&format!("{}/{}", request.course_id, request.element_id))
        }
        "get_course" | "get_learning_progress" => clip(&request.course_id),
        "search_catalog" => clip(&request.query),
        "get_credential" => clip(&request.credential_id),
        "resolve_goal" => clip(&request.goal_kind),
        "verify_presentation" => clip(&request.audience),
        _ => String::new(),
    }
}

/// Runs one authorized operation.
fn operate(
    db: &Connection,
    grant: &StudioGrant,
    request: &Request,
    fetched: Option<&Fetched>,
) -> Result<Dispatched> {
    let value = match request.operation.as_str() {
        "search_catalog" => learning::search_catalog(
            db,
            &learning::CatalogQuery {
                query: &request.query,
                skill_id: &request.skill_id,
                stored_only: request.stored_only,
                limit: request.limit,
                cursor: &request.cursor,
            },
        )?,
        "get_course" => learning::get_course(db, &request.course_id)?,
        "read_lesson" => {
            let target = learning::lesson_target(db, &request.course_id, &request.element_id)?;
            let body = match (target.blob(), fetched) {
                (None, _) => learning::Body::Stored,
                (Some(blob), Some(fetched)) if fetched.blob == blob => {
                    learning::Body::Fetched(fetched.content.as_deref().map_err(String::as_str))
                }
                (Some(blob), _) => return Ok(Dispatched::NeedsContent(blob.to_string())),
            };
            learning::render_lesson(db, &target, body, request.start)?
        }
        // The owner's own view: every earned skill, each flagged with whether
        // it is one the owner shares with peers.
        // Private skills only when the learner opted in for this grant.
        "get_skill_graph" => {
            let did = skills::local_did(db);
            let private = grant.has("learning:private_skills");
            serde_json::to_value(skills::skill_graph(db, &did, private)?)?
        }
        "get_learning_progress" => {
            let course = (!request.course_id.is_empty()).then_some(request.course_id.as_str());
            skills::learning_progress(db, course)?
        }
        "resolve_goal" => {
            let goal = match request.goal_kind.as_str() {
                "exam" => skills::Goal::Exam { key: &request.key },
                "job_role" => skills::Goal::JobRole { key: &request.key },
                "curriculum" => skills::Goal::Curriculum {
                    board: &request.board,
                    grade: &request.grade,
                },
                // A job description arrives as text. Nothing an assistant says
                // makes this device fetch a URL.
                "text" => skills::Goal::Text {
                    text: &request.text,
                },
                _ => return Err(Error::Invalid("unknown goal kind".into())),
            };
            serde_json::to_value(skills::resolve_goal(db, goal)?)?
        }
        "compute_learning_path" => {
            serde_json::to_value(skills::my_learning_path(db, &request.goal_skill_ids)?)?
        }
        "list_my_credentials" => credentials::list_credentials(
            db,
            &request.skill_id,
            request.include_revoked,
            request.limit,
            &request.cursor,
        )?,
        "get_credential" => credentials::get_credential(db, &request.credential_id)?,
        // Caller-supplied data, but the replay check reads and records device
        // state, so it sits behind a grant like every other broker operation.
        "verify_presentation" => {
            if request.audience.is_empty() {
                return Err(Error::Invalid("an expected audience is required".into()));
            }
            let envelope: credentials::PresentationEnvelope =
                serde_json::from_str(&request.presentation_json)
                    .map_err(|_| Error::Invalid("this is not a presentation envelope".into()))?;
            let verdict = credentials::verify_presentation(db, &envelope, &request.audience)?;
            json!({
                "result": verdict.as_str(),
                "presentation_id": envelope.id.clone().unwrap_or_default(),
                "audience": request.audience,
                "replay_checked": true,
            })
        }
        "list_course_drafts" => {
            let mut stmt = db.prepare(
                "SELECT c.id,c.title,e.id,e.title FROM courses c JOIN local_identity i ON i.id=1 AND i.stake_address=c.author_address LEFT JOIN course_chapters ch ON ch.course_id=c.id LEFT JOIN course_elements e ON e.chapter_id=ch.id AND e.element_type='text' ORDER BY c.id,ch.position,e.position LIMIT 101",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(json!({"course_id":r.get::<_,String>(0)?,"course_title":r.get::<_,String>(1)?,"element_id":r.get::<_,Option<String>>(2)?,"element_title":r.get::<_,Option<String>>(3)?}))
            })?;
            let mut items = rows.collect::<std::result::Result<Vec<_>, _>>()?;
            let truncated = items.len() > 100;
            items.truncate(100);
            json!({"items":items,"truncated":truncated})
        }
        "read_lesson_draft" => store::read_draft(db, &request.course_id, &request.element_id)?,
        "propose_lesson_draft" => {
            let proposal = store::propose_draft(
                db,
                &request.course_id,
                &request.element_id,
                &request.fingerprint,
                &request.text,
                &grant.client_name,
                &format!("{}:{}", grant.id, request.request_id),
            )?;
            json!({"run_id":proposal.id,"status":proposal.value.status,"requires_instructor_review":true})
        }
        _ => return Err(Error::Permission),
    };
    Ok(Dispatched::Value(value))
}

/// Returns the private per-user broker directory, creating it with 0700.
pub fn private_directory(root: &Path) -> std::result::Result<PathBuf, String> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let dir = root.join("studio-mcp");
    if !dir.exists() {
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .map_err(|_| "Could not create private assistant directory".to_string())?;
    }
    let metadata = std::fs::symlink_metadata(&dir)
        .map_err(|_| "Assistant directory unavailable".to_string())?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err("Assistant directory must be private to this OS user".into());
    }
    Ok(dir)
}

pub fn socket_path(dir: &Path) -> PathBuf {
    dir.join(format!("broker-{}.sock", std::process::id()))
}

/// Grant identifiers become file names, so only canonical UUIDs are accepted.
fn is_grant_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.hyphenated().to_string() == id)
}

/// Issues a grant and writes its connection file, pruning files left by
/// grants that have since expired or been revoked.
pub fn issue_connection(
    dir: &Path,
    grants: &mut Grants,
    client_name: String,
    scopes: Vec<String>,
    epoch: u64,
    now: i64,
) -> Result<(StudioGrant, PathBuf)> {
    let (grant, token) = grants.issue(client_name, scopes, epoch, now)?;
    prune_connections(dir, grants, now);
    match write_connection_file(dir, &grant.id, &token, &socket_path(dir)) {
        Ok(file) => Ok((grant, file)),
        Err(error) => {
            grants.revoke(&grant.id);
            Err(Error::Unavailable(error))
        }
    }
}

/// Removes connection files for grants that are no longer live. Borrowing the
/// grants keeps callers under the grants lock, so a file written for a
/// concurrently issued grant is never pruned.
pub fn prune_connections(dir: &Path, grants: &Grants, now: i64) {
    let live: Vec<String> = grants.list(now).into_iter().map(|grant| grant.id).collect();
    sweep(dir, &live, Some(&socket_path(dir)));
}

pub fn revoke_connection(dir: &Path, grants: &mut Grants, grant_id: &str) {
    grants.revoke(grant_id);
    remove_connection_file(dir, grant_id);
}

/// Writes the 0600 connection file handed to an assistant client.
fn write_connection_file(
    dir: &Path,
    grant_id: &str,
    token: &str,
    socket: &Path,
) -> std::result::Result<PathBuf, String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if !is_grant_id(grant_id) {
        return Err("Invalid assistant grant".into());
    }
    let file = dir.join(format!("{grant_id}.json"));
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&file)
        .map_err(|_| "Could not create connection file".to_string())?;
    output
        .write_all(
            json!({"socket":socket,"token":token})
                .to_string()
                .as_bytes(),
        )
        .map_err(|_| {
            let _ = std::fs::remove_file(&file);
            "Could not write connection file".to_string()
        })?;
    Ok(file)
}

fn remove_connection_file(dir: &Path, grant_id: &str) {
    if is_grant_id(grant_id) {
        let _ = std::fs::remove_file(dir.join(format!("{grant_id}.json")));
    }
}

/// Removes connection files for grants that are no longer live and sockets
/// left by broker processes that no longer accept connections.
pub fn sweep(dir: &Path, live_grant_ids: &[String], current_socket: Option<&Path>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let stale = if let Some(id) = name.strip_suffix(".json") {
            is_grant_id(id) && !live_grant_ids.iter().any(|live| live == id)
        } else if name.starts_with("broker-") && name.ends_with(".sock") {
            Some(path.as_path()) != current_socket
                && std::os::unix::net::UnixStream::connect(&path).is_err()
        } else {
            false
        };
        if stale {
            let _ = std::fs::remove_file(&path);
        }
    }
}
