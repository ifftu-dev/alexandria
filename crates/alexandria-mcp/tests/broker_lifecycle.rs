//! Native lifecycle validation for the local assistant broker: two fixture
//! profiles served by the shared broker, real `alexandria-mcp` client
//! processes, and lock/revoke/expiry/restart/malformed-transport behaviour.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
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
    /// Content blobs "peers" can provide, and a hold the test can keep a fetch waiting on.
    content: Mutex<std::collections::HashMap<String, Vec<u8>>>,
    fetch_hold: Arc<tokio::sync::Mutex<()>>,
    fetches: AtomicUsize,
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
            content: Mutex::default(),
            fetch_hold: Arc::default(),
            fetches: AtomicUsize::new(0),
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

fn learn(course: &str, element: &str) -> Value {
    json!({"course_id":course,"element_id":element})
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
async fn learners_read_published_content_without_answers_or_drafts() {
    let a = Profile::new("A");
    let _server = a.serve();
    a.db.lock()
        .unwrap()
        .execute_batch(
            r#"INSERT INTO courses(id,title,author_address,status) VALUES
                 ('pub','Published Course','someone','published'),
                 ('draft2','Hidden Draft','someone','draft');
               INSERT INTO catalog(course_id,title,author_address,content_cid,tags,skill_ids,version,published_at,signature) VALUES
                 ('pub','Published Course','someone','root-pub','["rust"]','["skill_a"]',1,'2026-09-01','sig'),
                 ('remote','Remote Course','someone','root-remote','[]','[]',1,'2026-09-02','sig');
               INSERT INTO course_chapters(id,course_id,title,position) VALUES
                 ('pch','pub','Basics',0),('dch','draft2','Draft chapter',0);
               INSERT INTO course_elements(id,chapter_id,title,element_type,content_inline,content_cid,position) VALUES
                 ('intro','pch','Intro','text','Inline lesson text',NULL,0),
                 ('blob','pch','Blob lesson','text',NULL,'blobhash',1),
                 ('quiz','pch','Quiz','quiz','{"title":"Q","questions":[{"id":"q1","type":"single_choice","prompt":"Pick one","options":["a","b"],"correct_indices":[1],"explanation":"EXPLAINS","points":1,"difficulty":1}]}',NULL,2),
                 ('mcq','pch','MCQ','objective_single_mcq','{"question":"Which?","options":[{"id":"o1","text":"x"},{"id":"o2","text":"y"}],"correct_option_index":1,"explanation":"EXPLAINS"}',NULL,3),
                 ('final','pch','Final','assessment','SECRET FINAL',NULL,4),
                 ('vid','pch','Video','video',NULL,'vidhash',5),
                 ('gone','pch','Gone','text',NULL,'absent',6),
                 ('dl','dch','Draft lesson','text','DRAFT TEXT',NULL,0);
               INSERT INTO video_chapters(id,element_id,title,start_seconds,position) VALUES
                 ('vc1','vid','Start',0,0);"#,
        )
        .unwrap();
    a.content
        .lock()
        .unwrap()
        .insert("blobhash".into(), b"Blob lesson body".to_vec());
    let (_, learner_file) = a.grant("Learner", &["learning:read"]);
    let (_, drafts_file) = a.grant("Drafts", &["drafts:read"]);
    let (learner, drafter) = (client(&learner_file).await, client(&drafts_file).await);
    let mut outputs = Vec::new();

    let (error, page) = call(&learner, "search_catalog", json!({"query": ""})).await;
    assert!(!error, "{page}");
    let items = page["structuredContent"]["items"]
        .as_array()
        .unwrap()
        .clone();
    let ids: Vec<&str> = items
        .iter()
        .map(|item| item["course_id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        ["remote", "pub"],
        "published courses only, newest first"
    );
    assert_eq!(items[0]["stored_on_device"], false);
    assert_eq!(items[1]["stored_on_device"], true);
    let (_, by_skill) = call(&learner, "search_catalog", json!({"skill_id": "skill_a"})).await;
    assert_eq!(
        by_skill["structuredContent"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let (_, wildcard) = call(&learner, "search_catalog", json!({"query": "_"})).await;
    assert!(
        wildcard["structuredContent"]["items"]
            .as_array()
            .unwrap()
            .is_empty(),
        "LIKE wildcards in a query are literal"
    );
    outputs.extend([page, by_skill, wildcard]);

    let (error, outline) = call(&learner, "get_course", json!({"course_id": "pub"})).await;
    assert!(!error, "{outline}");
    let contents: Vec<&str> = outline["structuredContent"]["chapters"][0]["elements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|element| element["content"].as_str().unwrap())
        .collect();
    assert_eq!(
        contents,
        [
            "text",
            "text",
            "questions",
            "questions",
            "withheld",
            "video_chapters",
            "text"
        ]
    );
    let (error, remote) = call(&learner, "get_course", json!({"course_id": "remote"})).await;
    assert!(!error, "{remote}");
    assert_eq!(remote["structuredContent"]["stored_on_device"], false);
    assert!(remote["structuredContent"]["chapters"]
        .as_array()
        .unwrap()
        .is_empty());
    outputs.extend([outline, remote]);

    // The profile's own authored course and someone's unpublished draft do not exist for a learning grant.
    for hidden in ["course", "draft2"] {
        let (error, body) = call(&learner, "get_course", json!({"course_id": hidden})).await;
        assert!(error && body.to_string().contains("not_found"), "{body}");
        outputs.push(body);
    }
    for (course, element) in [("course", "lesson"), ("course", "exam"), ("draft2", "dl")] {
        let (error, body) = call(&learner, "read_lesson", learn(course, element)).await;
        assert!(error && body.to_string().contains("not_found"), "{body}");
        outputs.push(body);
    }

    let (error, intro) = call(&learner, "read_lesson", learn("pub", "intro")).await;
    assert!(!error, "{intro}");
    assert_eq!(intro["structuredContent"]["text"], "Inline lesson text");
    let (_, blob) = call(&learner, "read_lesson", learn("pub", "blob")).await;
    assert_eq!(blob["structuredContent"]["text"], "Blob lesson body");
    let (_, gone) = call(&learner, "read_lesson", learn("pub", "gone")).await;
    assert_eq!(gone["structuredContent"]["status"], "unavailable");
    let (_, quiz) = call(&learner, "read_lesson", learn("pub", "quiz")).await;
    assert_eq!(
        quiz["structuredContent"]["questions"][0]["prompt"],
        "Pick one"
    );
    assert_eq!(
        quiz["structuredContent"]["questions"][0]["options"],
        json!(["a", "b"])
    );
    let (_, mcq) = call(&learner, "read_lesson", learn("pub", "mcq")).await;
    assert_eq!(
        mcq["structuredContent"]["questions"][0]["options"],
        json!(["x", "y"])
    );
    let (_, exam) = call(&learner, "read_lesson", learn("pub", "final")).await;
    assert_eq!(exam["structuredContent"]["status"], "withheld");
    let (_, video) = call(&learner, "read_lesson", learn("pub", "vid")).await;
    assert_eq!(
        video["structuredContent"]["video_chapters"][0]["title"],
        "Start"
    );
    outputs.extend([intro, blob, gone, quiz, mcq, exam, video]);

    // Learning and draft scopes do not stand in for each other.
    let (error, body) = call(&learner, "list_course_drafts", json!({})).await;
    assert!(
        error && body.to_string().contains("permission_denied"),
        "{body}"
    );
    outputs.push(body);
    let (error, body) = call(&drafter, "read_lesson", learn("pub", "intro")).await;
    assert!(
        error && body.to_string().contains("permission_denied"),
        "{body}"
    );
    outputs.push(body);

    for output in &outputs {
        let text = output.to_string();
        for secret in [
            "EXPLAINS",
            "correct",
            "SECRET",
            "DRAFT TEXT",
            "Profile A lesson",
        ] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
    }

    // A lock while a lesson is being fetched from peers is not held up by the
    // fetch, and the fetched content is not returned afterwards.
    let hold = a.fetch_hold.clone().lock_owned().await;
    let before = a.fetches.load(Ordering::SeqCst);
    let pending = tokio::spawn(async move {
        let result = call(&learner, "read_lesson", learn("pub", "blob")).await;
        (learner, result)
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while a.fetches.load(Ordering::SeqCst) == before {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the fetch started");
    tokio::time::timeout(Duration::from_secs(2), a.lock())
        .await
        .expect("a content fetch must not hold up a profile lock");
    drop(hold);
    let (learner, (error, body)) = pending.await.unwrap();
    assert!(error, "{body}");
    assert!(!body.to_string().contains("Blob lesson body"), "{body}");

    for client in [learner, drafter] {
        client.cancel().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn learners_read_their_own_graph_progress_and_goals() {
    let a = Profile::new("A");
    let _server = a.serve();
    a.db.lock()
        .unwrap()
        .execute_batch(
            r#"INSERT INTO app_settings(key,value) VALUES
                 ('identity.local_did','did:key:me'),
                 ('instructor.graph_prefs','{"b":{"public":false,"teaching":true}}');
               INSERT INTO subject_fields(id,name) VALUES ('field','Science');
               INSERT INTO subjects(id,name,subject_field_id) VALUES ('sub','Computing','field');
               INSERT INTO skills(id,name,bloom_level,subject_id,synonyms) VALUES
                 ('a','Alpha','remember','sub','first steps'),
                 ('b','Beta','apply','sub',NULL),
                 ('c','Gamma','create','sub',NULL);
               INSERT INTO skill_prerequisites(skill_id,prerequisite_id) VALUES ('b','a'),('c','b');
               INSERT INTO credentials(id,issuer_did,subject_did,credential_type,claim_kind,skill_id,issuance_date,signed_vc_json,integrity_hash,revoked) VALUES
                 ('v1','did:key:issuer','did:key:me','FormalCredential','skill','a','2026-01-01','{}','hash',0),
                 ('v2','did:key:issuer','did:key:me','FormalCredential','skill','b','2026-01-02','{}','hash',0);
               INSERT INTO courses(id,title,author_address,status,skill_ids) VALUES
                 ('gamma','Gamma course','someone','published','["c"]');
               INSERT INTO goal_templates(id,kind,key,label,skill_ids,ratified) VALUES
                 ('t1','exam','finals','Final exams','["c"]',1);
               INSERT INTO enrollments(id,course_id,status) VALUES ('e1','gamma','active');
               INSERT INTO course_chapters(id,course_id,title,position) VALUES ('gch','gamma','Chapter',0);
               INSERT INTO course_elements(id,chapter_id,title,element_type,position) VALUES
                 ('g1','gch','Lesson one','text',0),
                 ('g2','gch','Lesson two','text',1);
               INSERT INTO element_progress(id,enrollment_id,element_id,status,score) VALUES
                 ('p1','e1','g1','completed',0.9),
                 ('p2','e1','g2','in_progress',NULL);"#,
        )
        .unwrap();
    let (_, learner_file) = a.grant("Learner", &["learning:read"]);
    let (_, drafts_file) = a.grant("Drafts", &["drafts:read"]);
    let (learner, drafter) = (client(&learner_file).await, client(&drafts_file).await);

    // The owner's own view keeps private skills, flagged as such.
    let (error, graph) = call(&learner, "get_skill_graph", json!({})).await;
    assert!(!error, "{graph}");
    let graph = &graph["structuredContent"];
    assert_eq!(graph["includes_private"], true);
    assert_eq!(graph["nodes"][0]["skill_id"], "a");
    assert_eq!(graph["nodes"][0]["public"], true);
    assert_eq!(graph["nodes"][1]["skill_id"], "b");
    assert_eq!(graph["nodes"][1]["public"], false);
    assert_eq!(graph["nodes"][1]["teaching"], true);
    assert_eq!(graph["edges"][0]["prerequisite_id"], "a");

    let (error, progress) = call(&learner, "get_learning_progress", json!({})).await;
    assert!(!error, "{progress}");
    let enrolment = &progress["structuredContent"]["enrolments"][0];
    assert_eq!(enrolment["course_id"], "gamma");
    assert_eq!(enrolment["course_title"], "Gamma course");
    assert_eq!(enrolment["elements_total"], 2);
    assert_eq!(enrolment["elements_completed"], 1);
    assert_eq!(enrolment["elements"][0]["score"], 0.9);

    let (error, goal) = call(
        &learner,
        "resolve_goal",
        json!({"kind":"exam","key":"finals"}),
    )
    .await;
    assert!(!error, "{goal}");
    assert_eq!(goal["structuredContent"]["goal_skill_ids"][0], "c");
    assert_eq!(
        goal["structuredContent"]["resolution_provenance"],
        "template"
    );

    let (error, parsed) = call(
        &learner,
        "resolve_goal",
        json!({"kind":"text","text":"Looking for first steps and Gamma work"}),
    )
    .await;
    assert!(!error, "{parsed}");
    assert_eq!(
        parsed["structuredContent"]["resolution_provenance"],
        "text_parsed"
    );
    assert!(
        parsed["structuredContent"]["goal_skill_ids"]
            .as_array()
            .unwrap()
            .is_empty(),
        "matched text only suggests"
    );
    let suggested: Vec<&str> = parsed["structuredContent"]["suggestions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["skill_id"].as_str().unwrap())
        .collect();
    assert!(suggested.contains(&"a"), "{suggested:?}");

    let (error, path) = call(
        &learner,
        "compute_learning_path",
        json!({"goal_skill_ids":["c"]}),
    )
    .await;
    assert!(!error, "{path}");
    let steps = path["structuredContent"]["steps"].as_array().unwrap();
    let order: Vec<&str> = steps
        .iter()
        .map(|s| s["skill_id"].as_str().unwrap())
        .collect();
    assert_eq!(order, ["a", "b", "c"], "prerequisites first");
    let statuses: Vec<&str> = steps
        .iter()
        .map(|s| s["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, ["earned", "earned", "available"]);
    assert_eq!(steps[2]["course_recs"][0]["course_id"], "gamma");
    assert_eq!(path["structuredContent"]["earned_count"], 2);

    // A goal kind the tool does not offer, and a draft grant, are both refused.
    let (error, body) = call(
        &learner,
        "resolve_goal",
        json!({"kind":"link","key":"https://example.com"}),
    )
    .await;
    assert!(
        error && body.to_string().contains("invalid_input"),
        "{body}"
    );
    for tool in [
        "get_skill_graph",
        "get_learning_progress",
        "compute_learning_path",
    ] {
        let args = if tool == "compute_learning_path" {
            json!({"goal_skill_ids":["c"]})
        } else {
            json!({})
        };
        let (error, body) = call(&drafter, tool, args).await;
        assert!(
            error && body.to_string().contains("permission_denied"),
            "{tool}: {body}"
        );
    }

    for client in [learner, drafter] {
        client.cancel().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn credential_summaries_never_carry_the_signed_document() {
    let a = Profile::new("A");
    let _server = a.serve();
    a.db.lock()
        .unwrap()
        .execute_batch(
            r#"INSERT INTO app_settings(key,value) VALUES ('identity.local_did','did:key:me');
               INSERT INTO subject_fields(id,name) VALUES ('field','Science');
               INSERT INTO subjects(id,name,subject_field_id) VALUES ('sub','Computing','field');
               INSERT INTO skills(id,name,bloom_level,subject_id) VALUES ('a','Alpha','apply','sub');
               INSERT INTO credentials(id,issuer_did,subject_did,credential_type,claim_kind,skill_id,
                   issuance_date,signed_vc_json,integrity_hash,revoked,received_at) VALUES
                 ('mine','did:key:issuer','did:key:me','FormalCredential','skill','a','2026-01-01',
                  '{"proof":"SECRET SIGNED DOCUMENT"}','hash-1',0,'2026-02-01'),
                 ('gone','did:key:issuer','did:key:me','FormalCredential','skill','a','2026-01-02',
                  '{"proof":"SECRET SIGNED DOCUMENT"}','hash-2',1,'2026-02-02'),
                 ('theirs','did:key:issuer','did:key:other','FormalCredential','skill','a','2026-01-03',
                  '{"proof":"SECRET SIGNED DOCUMENT"}','hash-3',0,'2026-02-03');"#,
        )
        .unwrap();
    let (_, credentials_file) = a.grant("Credentials", &["credentials:read"]);
    let (_, learning_file) = a.grant("Learning", &["learning:read"]);
    let (holder, learner) = (
        client(&credentials_file).await,
        client(&learning_file).await,
    );
    let mut outputs = Vec::new();

    let (error, page) = call(&holder, "list_my_credentials", json!({})).await;
    assert!(!error, "{page}");
    let items = page["structuredContent"]["items"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(items.len(), 1, "revoked and other subjects are left out");
    assert_eq!(items[0]["credential_id"], "mine");
    assert_eq!(items[0]["skill_name"], "Alpha");
    assert_eq!(items[0]["integrity_hash"], "hash-1");
    assert_eq!(page["structuredContent"]["subject_did"], "did:key:me");

    let (_, revoked) = call(
        &holder,
        "list_my_credentials",
        json!({"include_revoked": true}),
    )
    .await;
    assert_eq!(
        revoked["structuredContent"]["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let (error, one) = call(&holder, "get_credential", json!({"credential_id": "mine"})).await;
    assert!(!error, "{one}");
    assert_eq!(one["structuredContent"]["issuer_did"], "did:key:issuer");
    let (error, foreign) = call(
        &holder,
        "get_credential",
        json!({"credential_id": "theirs"}),
    )
    .await;
    assert!(
        error && foreign.to_string().contains("not_found"),
        "another subject's credential is not readable: {foreign}"
    );
    outputs.extend([page, revoked, one, foreign]);

    // A presentation for another audience, and one whose proof does not verify,
    // are both refused — and neither records a nonce.
    let envelope = |audience: &str, nonce: &str| {
        let payload = json!({"audience": audience, "nonce": nonce, "bundle": []}).to_string();
        json!({
            "presentation_json": json!({
                "id": "urn:presentation:test",
                "payload_json": payload,
                // Not a proof this device will accept. The audience and replay
                // checks run before the proof is looked at.
                "proof": "eyJhbGciOiJFZERTQSJ9..AAAA",
                "subject": "did:key:not-a-real-key",
            })
            .to_string(),
            "audience": "did:key:verifier",
        })
    };
    let (error, mismatched) = call(
        &holder,
        "verify_presentation",
        envelope("did:key:elsewhere", "nonce-1"),
    )
    .await;
    assert!(!error, "{mismatched}");
    assert_eq!(
        mismatched["structuredContent"]["result"],
        "audience_mismatch"
    );
    assert_eq!(mismatched["structuredContent"]["replay_checked"], true);

    let (error, bad) = call(
        &holder,
        "verify_presentation",
        envelope("did:key:verifier", "nonce-2"),
    )
    .await;
    assert!(!error, "{bad}");
    assert!(
        ["malformed", "bad_signature"]
            .contains(&bad["structuredContent"]["result"].as_str().unwrap()),
        "an envelope without a usable proof is refused: {bad}"
    );
    let seen: i64 =
        a.db.lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM presentations_seen", [], |row| {
                row.get(0)
            })
            .unwrap();
    assert_eq!(seen, 0, "a refused presentation records nothing");
    outputs.extend([mismatched, bad]);

    // A learning grant does not reach credentials.
    for (tool, args) in [
        ("list_my_credentials", json!({})),
        ("get_credential", json!({"credential_id": "mine"})),
        (
            "verify_presentation",
            json!({"presentation_json": "{}", "audience": "did:key:verifier"}),
        ),
    ] {
        let (error, body) = call(&learner, tool, args).await;
        assert!(
            error && body.to_string().contains("permission_denied"),
            "{tool}: {body}"
        );
        outputs.push(body);
    }

    for output in &outputs {
        assert!(
            !output.to_string().contains("SECRET"),
            "signed document leaked: {output}"
        );
    }

    for client in [holder, learner] {
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
