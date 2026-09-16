//! The fixture profile and client harness the native suites share: one
//! unlocked profile served by the real broker, and real `alexandria-mcp`
//! client processes talking to it over a Unix socket.
#![allow(dead_code)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use alexandria_studio::{
    broker::{self, BrokerHost},
    grants::Grants,
    Error,
};
use rmcp::{
    model::CallToolRequestParams, service::RunningService, transport::TokioChildProcess,
    RoleClient, ServiceExt,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[path = "../../../../src-tauri/src/db/schema.rs"]
mod schema;

pub type Client = RunningService<RoleClient, ()>;

pub struct Pause {
    pub entered: mpsc::Sender<()>,
    pub proceed: mpsc::Receiver<()>,
}

/// One unlocked fixture profile, mirroring the app's `StudioRuntime` and
/// `with_db` lock checks.
pub struct Profile {
    pub root: tempfile::TempDir,
    pub db: Mutex<Connection>,
    pub grants: Mutex<Grants>,
    pub gate: tokio::sync::RwLock<()>,
    pub epoch: AtomicU64,
    pub locked: AtomicBool,
    pub now: AtomicI64,
    pub pause: Mutex<Option<Pause>>,
    /// Content blobs "peers" can provide, and a hold the test can keep a fetch waiting on.
    pub content: Mutex<std::collections::HashMap<String, Vec<u8>>>,
    pub fetch_hold: Arc<tokio::sync::Mutex<()>>,
    pub fetches: AtomicUsize,
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

    fn fetch_content(&self, blob: &str) -> broker::ContentFuture<'_> {
        let blob = blob.to_string();
        Box::pin(async move {
            self.fetches.fetch_add(1, Ordering::SeqCst);
            let _hold = self.fetch_hold.lock().await;
            let found = self.content.lock().unwrap().get(&blob).cloned();
            found.ok_or_else(|| "not provided".to_string())
        })
    }
}

impl Profile {
    pub fn new(label: &str) -> Arc<Self> {
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
            content: Mutex::default(),
            fetch_hold: Arc::default(),
            fetches: AtomicUsize::new(0),
        })
    }

    pub fn dir(&self) -> PathBuf {
        broker::private_directory(self.root.path()).unwrap()
    }

    pub fn socket(&self) -> PathBuf {
        broker::socket_path(&self.dir())
    }

    /// Mirrors app startup: sweep leftovers, then bind this process's socket.
    pub fn serve(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        broker::sweep(&self.dir(), &[], None);
        let listener = tokio::net::UnixListener::bind(self.socket()).unwrap();
        tokio::spawn(broker::serve(listener, self.clone()))
    }

    pub fn grant(&self, client: &str, scopes: &[&str]) -> (String, PathBuf) {
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
    pub async fn lock(&self) {
        let _lease = self.gate.write().await;
        self.invalidate();
        broker::sweep(&self.dir(), &[], Some(&self.socket()));
    }

    pub fn invalidate(&self) {
        let _db = self.db.lock().unwrap();
        self.grants.lock().unwrap().clear();
        self.locked.store(true, Ordering::SeqCst);
        self.epoch.fetch_add(1, Ordering::SeqCst);
    }

    pub fn unlock(&self) {
        let _db = self.db.lock().unwrap();
        self.epoch.fetch_add(1, Ordering::SeqCst);
        self.locked.store(false, Ordering::SeqCst);
    }

    /// Makes the next database section wait until the test releases it.
    pub fn pause(&self) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (entered, entered_rx) = mpsc::channel();
        let (proceed_tx, proceed) = mpsc::channel();
        *self.pause.lock().unwrap() = Some(Pause { entered, proceed });
        (entered_rx, proceed_tx)
    }

    pub fn lesson_text(&self) -> String {
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

pub fn lesson() -> Value {
    json!({"course_id":"course","element_id":"lesson"})
}

pub fn learn(course: &str, element: &str) -> Value {
    json!({"course_id":course,"element_id":element})
}

pub fn token_of(file: &Path) -> String {
    let credential: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    credential["token"].as_str().unwrap().to_string()
}

pub fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

pub async fn client(file: &Path) -> Client {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_alexandria-mcp"));
    command.env("ALEXANDRIA_MCP_CONNECTION_FILE", file);
    ().serve(TokioChildProcess::new(command).unwrap())
        .await
        .unwrap()
}

/// Calls a tool through the real stdio client. Returns (is_error, result JSON).
pub async fn call(client: &Client, tool: &'static str, args: Value) -> (bool, Value) {
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
pub async fn raw(socket: PathBuf, frame: Vec<u8>) -> Option<Value> {
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

pub async fn raw_read(socket: PathBuf, token: &str) -> String {
    let request = json!({"token":token,"operation":"read_lesson_draft","course_id":"course","element_id":"lesson"});
    let response = raw(socket, format!("{request}\n").into_bytes())
        .await
        .unwrap();
    response["error"].as_str().unwrap_or_default().to_string()
}
