//! Native lifecycle validation for the local assistant broker: two fixture
//! profiles served by the shared broker, real `alexandria-mcp` client
//! processes, and lock/revoke/expiry/restart/malformed-transport behaviour.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use alexandria_studio::{
    broker::{self, BrokerHost},
    grants::Grants,
    model::StudioRun,
    store, Error,
};
use rmcp::{
    model::CallToolRequestParams, service::RunningService, transport::TokioChildProcess,
    RoleClient, ServiceExt,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[path = "../../../src-tauri/src/db/schema.rs"]
mod schema;

type Client = RunningService<RoleClient, ()>;

struct Pause {
    entered: mpsc::Sender<()>,
    proceed: mpsc::Receiver<()>,
}

/// One unlocked fixture profile, mirroring the app's `StudioRuntime` and
/// `with_db` lock checks.
struct Profile {
    root: tempfile::TempDir,
    db: Mutex<Connection>,
    grants: Mutex<Grants>,
    gate: tokio::sync::RwLock<()>,
    epoch: AtomicU64,
    locked: AtomicBool,
    now: AtomicI64,
    pause: Mutex<Option<Pause>>,
}

impl BrokerHost for Profile {
    fn gate(&self) -> &tokio::sync::RwLock<()> {
        &self.gate
    }

    fn grants(&self) -> &Mutex<Grants> {
        &self.grants
    }

    fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }

    fn now(&self) -> i64 {
        self.now.load(Ordering::SeqCst)
    }

    fn with_db(
        &self,
        f: &mut dyn FnMut(&Connection) -> alexandria_studio::Result<Value>,
    ) -> Result<Value, String> {
        let db = self.db.lock().unwrap();
        if self.locked.load(Ordering::SeqCst) {
            return Err(Error::Locked.to_string());
        }
        let pause = self.pause.lock().unwrap().take();
        if let Some(pause) = pause {
            pause.entered.send(()).unwrap();
            pause.proceed.recv().unwrap();
        }
        f(&db).map_err(|error| error.to_string())
    }
}

impl Profile {
    fn new(label: &str) -> Arc<Self> {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        for (_, _, sql) in schema::MIGRATIONS {
            db.execute_batch(sql).unwrap();
        }
        db.execute_batch(&format!(
            "INSERT INTO local_identity(id,stake_address,payment_address) VALUES (1,'author','payment');
             INSERT INTO courses(id,title,author_address) VALUES ('course','Course {label}','author');
             INSERT INTO course_chapters(id,course_id,title,position) VALUES ('chapter','course','Chapter',0);
             INSERT INTO course_elements(id,chapter_id,title,element_type,content_inline,position) VALUES
                 ('lesson','chapter','Lesson','text','Profile {label} lesson',0),
                 ('exam','chapter','Exam','assessment','SECRET {label} ANSWERS',1);"
        ))
        .unwrap();
        Arc::new(Self {
            root: tempfile::tempdir().unwrap(),
            db: Mutex::new(db),
            grants: Mutex::default(),
            gate: tokio::sync::RwLock::new(()),
            epoch: AtomicU64::new(1),
            locked: AtomicBool::new(false),
            now: AtomicI64::new(1_000_000),
            pause: Mutex::new(None),
        })
    }

    fn dir(&self) -> PathBuf {
        broker::private_directory(self.root.path()).unwrap()
    }

    fn socket(&self) -> PathBuf {
        broker::socket_path(&self.dir())
    }

    /// Mirrors app startup: sweep leftovers, then bind this process's socket.
    fn serve(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        broker::sweep(&self.dir(), &[], None);
        let listener = tokio::net::UnixListener::bind(self.socket()).unwrap();
        tokio::spawn(broker::serve(listener, self.clone()))
    }

    fn grant(&self, client: &str, scopes: &[&str]) -> (String, PathBuf) {
        let (grant, file) = broker::issue_connection(
            &self.dir(),
            &mut self.grants.lock().unwrap(),
            client.into(),
            scopes.iter().map(|scope| scope.to_string()).collect(),
            self.epoch(),
            self.now(),
        )
        .unwrap();
        (grant.id, file)
    }

    /// Mirrors `stop_active_profile`: exclusive lease, invalidate, remove files.
    async fn lock(&self) {
        let _lease = self.gate.write().await;
        self.invalidate();
        broker::sweep(&self.dir(), &[], Some(&self.socket()));
    }

    fn invalidate(&self) {
        let _db = self.db.lock().unwrap();
        self.grants.lock().unwrap().clear();
        self.locked.store(true, Ordering::SeqCst);
        self.epoch.fetch_add(1, Ordering::SeqCst);
    }

    fn unlock(&self) {
        let _db = self.db.lock().unwrap();
        self.epoch.fetch_add(1, Ordering::SeqCst);
        self.locked.store(false, Ordering::SeqCst);
    }

    /// Makes the next database section wait until the test releases it.
    fn pause(&self) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (entered, entered_rx) = mpsc::channel();
        let (proceed_tx, proceed) = mpsc::channel();
        *self.pause.lock().unwrap() = Some(Pause { entered, proceed });
        (entered_rx, proceed_tx)
    }

    fn lesson_text(&self) -> String {
        self.db
            .lock()
            .unwrap()
            .query_row(
                "SELECT content_inline FROM course_elements WHERE id='lesson'",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }
}

fn lesson() -> Value {
    json!({"course_id":"course","element_id":"lesson"})
}

fn token_of(file: &Path) -> String {
    let credential: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    credential["token"].as_str().unwrap().to_string()
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

async fn client(file: &Path) -> Client {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_alexandria-mcp"));
    command.env("ALEXANDRIA_MCP_CONNECTION_FILE", file);
    ().serve(TokioChildProcess::new(command).unwrap())
        .await
        .unwrap()
}

/// Calls a tool through the real stdio client. Returns (is_error, result JSON).
async fn call(client: &Client, tool: &'static str, args: Value) -> (bool, Value) {
    let mut params = CallToolRequestParams::new(tool);
    if let Value::Object(arguments) = args {
        if !arguments.is_empty() {
            params = params.with_arguments(arguments);
        }
    }
    let result = client.call_tool(params).await.unwrap();
    (
        result.is_error.unwrap_or(false),
        serde_json::to_value(&result).unwrap(),
    )
}

/// Sends raw bytes to a broker socket and returns its single response frame.
async fn raw(socket: PathBuf, frame: Vec<u8>) -> Option<Value> {
    let mut stream = tokio::net::UnixStream::connect(socket).await.ok()?;
    let _ = stream.write_all(&frame).await;
    let _ = stream.shutdown().await;
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(6), stream.read_to_end(&mut response))
        .await
        .ok()?
        .ok()?;
    serde_json::from_slice(&response).ok()
}

async fn raw_read(socket: PathBuf, token: &str) -> String {
    let request = json!({"token":token,"operation":"read_lesson_draft","course_id":"course","element_id":"lesson"});
    let response = raw(socket, format!("{request}\n").into_bytes())
        .await
        .unwrap();
    response["error"].as_str().unwrap_or_default().to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_profiles_and_clients_stay_isolated_and_scoped() {
    let (a, b) = (Profile::new("A"), Profile::new("B"));
    let _servers = (a.serve(), b.serve());
    let (_, file_a) = a.grant("Writer A", &["drafts:read", "drafts:propose"]);
    let (_, file_b) = b.grant("Reader B", &["drafts:read"]);
    assert_eq!(mode(&a.dir()), 0o700);
    assert_eq!(mode(&file_a), 0o600);
    let tokens = [token_of(&file_a), token_of(&file_b)];
    let (client_a, client_b) = (client(&file_a).await, client(&file_b).await);
    let mut outputs = Vec::new();

    let (error, list_a) = call(&client_a, "list_course_drafts", json!({})).await;
    assert!(!error);
    assert_eq!(
        list_a["structuredContent"]["items"][0]["course_title"],
        "Course A"
    );
    let (error, list_b) = call(&client_b, "list_course_drafts", json!({})).await;
    assert!(!error);
    assert_eq!(
        list_b["structuredContent"]["items"][0]["course_title"],
        "Course B"
    );
    assert!(!list_a.to_string().contains("Course B"));
    assert!(!list_b.to_string().contains("Course A"));

    let (error, read_a) = call(&client_a, "read_lesson_draft", lesson()).await;
    assert!(!error);
    assert_eq!(read_a["structuredContent"]["text"], "Profile A lesson");
    let (error, read_b) = call(&client_b, "read_lesson_draft", lesson()).await;
    assert!(!error);
    assert_eq!(read_b["structuredContent"]["text"], "Profile B lesson");

    // Profile A's token presented to profile B's broker.
    let crossed = a.root.path().join("crossed.json");
    std::fs::write(
        &crossed,
        json!({"socket":b.socket(),"token":tokens[0]}).to_string(),
    )
    .unwrap();
    std::fs::set_permissions(&crossed, std::fs::Permissions::from_mode(0o600)).unwrap();
    let client_crossed = client(&crossed).await;
    let (error, body) = call(&client_crossed, "read_lesson_draft", lesson()).await;
    assert!(error);
    assert!(body.to_string().contains("permission_denied"));
    assert!(!body.to_string().contains("Profile B lesson"));
    outputs.push(body);

    let (error, body) = call(
        &client_a,
        "read_lesson_draft",
        json!({"course_id":"course","element_id":"exam"}),
    )
    .await;
    assert!(error);
    assert!(!body.to_string().contains("SECRET"));
    outputs.push(body);

    let proposal = |read: &Value, text: &str| json!({"course_id":"course","element_id":"lesson","fingerprint":read["structuredContent"]["fingerprint"],"text":text,"request_id":"request-1"});
    let (error, body) = call(
        &client_b,
        "propose_lesson_draft",
        proposal(&read_b, "Rewrite B"),
    )
    .await;
    assert!(error, "a read-only grant must not propose");
    assert!(body.to_string().contains("permission_denied"));
    outputs.push(body);

    let (error, first) = call(
        &client_a,
        "propose_lesson_draft",
        proposal(&read_a, "Rewrite A"),
    )
    .await;
    assert!(!error);
    assert_eq!(first["structuredContent"]["status"], "review");
    assert_eq!(
        first["structuredContent"]["requires_instructor_review"],
        true
    );
    let (error, retry) = call(
        &client_a,
        "propose_lesson_draft",
        proposal(&read_a, "Rewrite A"),
    )
    .await;
    assert!(!error);
    assert_eq!(
        retry["structuredContent"]["run_id"],
        first["structuredContent"]["run_id"]
    );
    let (error, changed) = call(
        &client_a,
        "propose_lesson_draft",
        proposal(&read_a, "Different rewrite"),
    )
    .await;
    assert!(error);
    assert!(changed.to_string().contains("conflict"));

    // Proposals are review items in the owning profile only; nothing is applied.
    let run_id = first["structuredContent"]["run_id"].as_str().unwrap();
    let run = store::get::<StudioRun>(&a.db.lock().unwrap(), "run", run_id)
        .unwrap()
        .unwrap();
    assert_eq!(run.value.status, "review");
    assert_eq!(a.lesson_text(), "Profile A lesson");
    assert!(store::list::<StudioRun>(&b.db.lock().unwrap(), "run")
        .unwrap()
        .is_empty());

    outputs.extend([list_a, list_b, read_a, read_b, first, retry, changed]);
    for output in outputs {
        for token in &tokens {
            assert!(!output.to_string().contains(token.as_str()));
        }
    }
    for client in [client_a, client_b, client_crossed] {
        client.cancel().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn revocation_expiry_and_lock_invalidate_connected_clients() {
    let profile = Profile::new("A");
    let _server = profile.serve();

    let (grant_id, file) = profile.grant("Revoked", &["drafts:read"]);
    let token = token_of(&file);
    let revoked = client(&file).await;
    assert!(!call(&revoked, "read_lesson_draft", lesson()).await.0);
    {
        let _lease = profile.gate.write().await;
        broker::revoke_connection(
            &profile.dir(),
            &mut profile.grants.lock().unwrap(),
            &grant_id,
        );
    }
    assert!(!file.exists(), "revocation removes the connection file");
    let (error, body) = call(&revoked, "read_lesson_draft", lesson()).await;
    assert!(error);
    assert!(body.to_string().contains("unavailable"));
    assert!(raw_read(profile.socket(), &token)
        .await
        .starts_with("permission_denied"));
    // Traversal-shaped identifiers are not treated as file names.
    broker::revoke_connection(
        &profile.dir(),
        &mut profile.grants.lock().unwrap(),
        "../crossed",
    );
    revoked.cancel().await.unwrap();

    let (_, expiring_file) = profile.grant("Expiring", &["drafts:read"]);
    let expiring = client(&expiring_file).await;
    assert!(!call(&expiring, "read_lesson_draft", lesson()).await.0);
    profile.now.fetch_add(3600, Ordering::SeqCst);
    let (error, body) = call(&expiring, "read_lesson_draft", lesson()).await;
    assert!(error);
    assert!(body.to_string().contains("permission_denied"));
    let (_, next_file) = profile.grant("Next", &["drafts:read"]);
    assert!(
        !expiring_file.exists(),
        "issuing a grant prunes expired connection files"
    );
    assert!(next_file.exists());
    expiring.cancel().await.unwrap();

    let token = token_of(&next_file);
    let locked = client(&next_file).await;
    assert!(!call(&locked, "read_lesson_draft", lesson()).await.0);
    profile.lock().await;
    assert!(!next_file.exists(), "profile lock removes connection files");
    let (error, body) = call(&locked, "read_lesson_draft", lesson()).await;
    assert!(error);
    assert!(body.to_string().contains("unavailable"));
    assert!(raw_read(profile.socket(), &token)
        .await
        .starts_with("profile_locked"));
    profile.unlock();
    assert!(
        raw_read(profile.socket(), &token)
            .await
            .starts_with("permission_denied"),
        "a grant from the previous unlock stays invalid"
    );
    locked.cancel().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn invalidation_waits_for_in_flight_work_and_rejects_later_requests() {
    let profile = Profile::new("A");
    let _server = profile.serve();
    let (_, file) = profile.grant("Writer", &["drafts:read", "drafts:propose"]);
    let writer = Arc::new(client(&file).await);

    // A read already inside the database section finishes before lock completes.
    let (entered, proceed) = profile.pause();
    let reader = {
        let writer = writer.clone();
        tokio::spawn(async move { call(&writer, "read_lesson_draft", lesson()).await })
    };
    tokio::task::spawn_blocking(move || entered.recv().unwrap())
        .await
        .unwrap();
    let locker = {
        let profile = profile.clone();
        tokio::spawn(async move { profile.lock().await })
    };
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(
        !locker.is_finished(),
        "lock must wait for the in-flight lease"
    );
    proceed.send(()).unwrap();
    let (error, body) = reader.await.unwrap();
    assert!(!error);
    assert_eq!(body["structuredContent"]["text"], "Profile A lesson");
    locker.await.unwrap();
    drop(writer);

    // A request that arrives while invalidation holds the lease sees the new state.
    profile.unlock();
    let (_, file) = profile.grant("Writer", &["drafts:read", "drafts:propose"]);
    let token = token_of(&file);
    let lease = profile.gate.write().await;
    let pending = tokio::spawn({
        let socket = profile.socket();
        async move { raw_read(socket, &token).await }
    });
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(!pending.is_finished());
    profile.invalidate();
    drop(lease);
    assert!(pending.await.unwrap().starts_with("profile_locked"));

    // An in-flight proposal commits once, before the lock takes effect.
    profile.unlock();
    let (_, file) = profile.grant("Writer", &["drafts:read", "drafts:propose"]);
    let writer = Arc::new(client(&file).await);
    let (_, read) = call(&writer, "read_lesson_draft", lesson()).await;
    let (entered, proceed) = profile.pause();
    let proposer = {
        let writer = writer.clone();
        let args = json!({"course_id":"course","element_id":"lesson","fingerprint":read["structuredContent"]["fingerprint"],"text":"Rewrite","request_id":"in-flight"});
        tokio::spawn(async move { call(&writer, "propose_lesson_draft", args).await })
    };
    tokio::task::spawn_blocking(move || entered.recv().unwrap())
        .await
        .unwrap();
    let locker = {
        let profile = profile.clone();
        tokio::spawn(async move { profile.lock().await })
    };
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(!locker.is_finished());
    proceed.send(()).unwrap();
    let (error, body) = proposer.await.unwrap();
    assert!(!error);
    locker.await.unwrap();
    let run_id = body["structuredContent"]["run_id"].as_str().unwrap();
    let runs = store::list::<StudioRun>(&profile.db.lock().unwrap(), "run").unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id, run_id);
    assert_eq!(profile.lesson_text(), "Profile A lesson");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn restart_malformed_frames_and_capacity_fail_cleanly() {
    let profile = Profile::new("A");
    let dead = profile.dir().join("broker-1.sock");
    drop(std::os::unix::net::UnixListener::bind(&dead).unwrap());
    let server = profile.serve();
    assert!(!dead.exists(), "startup removes sockets of exited brokers");
    let socket = profile.socket();
    let (_, file) = profile.grant("Assistant", &["drafts:read"]);
    let token = token_of(&file);

    let frame = |value: Value| format!("{value}\n").into_bytes();
    for (bytes, expected) in [
        (b"not json\n".to_vec(), "invalid_input: malformed"),
        (
            frame(json!({"token":token,"operation":"read_lesson_draft","extra":true})),
            "invalid_input: malformed",
        ),
        (b"{\"token\":".to_vec(), "invalid_input: incomplete"),
        (
            frame(json!({"token":token,"operation":"publish_course"})),
            "permission_denied",
        ),
    ] {
        let response = raw(socket.clone(), bytes).await.unwrap();
        assert!(
            response["error"].as_str().unwrap().starts_with(expected),
            "{response}"
        );
    }
    if let Some(response) = raw(socket.clone(), vec![b'x'; broker::MAX_REQUEST_BYTES + 8]).await {
        assert!(response["error"]
            .as_str()
            .unwrap()
            .starts_with("invalid_input"));
    }

    // Connections beyond the concurrency limit close without a response.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut idle = Vec::new();
    for _ in 0..broker::MAX_CONCURRENT_REQUESTS {
        idle.push(tokio::net::UnixStream::connect(&socket).await.unwrap());
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut extra = tokio::net::UnixStream::connect(&socket).await.unwrap();
    let mut response = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(2), extra.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(read, 0);
    drop(idle);
    tokio::time::sleep(Duration::from_millis(100)).await;

    let assistant = client(&file).await;
    assert!(!call(&assistant, "read_lesson_draft", lesson()).await.0);

    // App exit: the socket stays on disk but nothing accepts connections.
    server.abort();
    let _ = server.await;
    let (error, body) = call(&assistant, "read_lesson_draft", lesson()).await;
    assert!(error);
    assert!(body.to_string().contains("unavailable"));

    // Restart: grants are memory-only, and startup removes old files and sockets.
    profile.grants.lock().unwrap().clear();
    profile.epoch.fetch_add(1, Ordering::SeqCst);
    let _server = profile.serve();
    assert!(!file.exists());
    let (error, body) = call(&assistant, "read_lesson_draft", lesson()).await;
    assert!(error);
    assert!(body.to_string().contains("unavailable"));
    assert!(raw_read(socket, &token)
        .await
        .starts_with("permission_denied"));
    assistant.cancel().await.unwrap();
}
