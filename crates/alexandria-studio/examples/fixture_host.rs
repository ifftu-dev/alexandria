//! A throwaway Alexandria profile, served by the real assistant broker.
//!
//! The application is a desktop app: useful for a person, awkward for a
//! reproducible check. This builds the fixture the execution plan describes in
//! a temporary directory, serves it over the same broker core the app runs, and
//! prints what to paste into an MCP client. Nothing here touches a real profile:
//! the database, the socket and the grant all live under the directory given on
//! the command line.
//!
//! It is a fixture, not the application. There is no vault, no profile
//! lock/switch lifecycle and no UI; what it proves is the tools and the broker,
//! against data whose shape is known.
//!
//! The default location is `/tmp/alexandria-mcp-fixture` rather than `target/`
//! because a Unix socket path has a hard length limit that a deep checkout
//! exceeds; pass a different directory if you prefer, keeping it short.
//!
//!     cargo run -p alexandria-studio --example fixture_host

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use alexandria_studio::{
    broker::{self, BrokerHost},
    grants::Grants,
};
use rusqlite::Connection;
use serde_json::Value;

#[path = "../../../src-tauri/src/db/schema.rs"]
mod schema;

/// The learner this profile belongs to, and the one whose work it holds.
const OWNER_DID: &str = "did:key:z6MkfixtureOwnerAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const GRANT_SCOPES: &[&str] = &[
    "learning:read",
    "credentials:read",
    "drafts:read",
    "drafts:propose",
];

struct Fixture {
    db: Mutex<Connection>,
    grants: Mutex<Grants>,
    gate: tokio::sync::RwLock<()>,
}

impl BrokerHost for Fixture {
    fn gate(&self) -> &tokio::sync::RwLock<()> {
        &self.gate
    }

    fn grants(&self) -> &Mutex<Grants> {
        &self.grants
    }

    fn epoch(&self) -> u64 {
        1
    }

    fn with_db(
        &self,
        f: &mut dyn FnMut(&Connection) -> alexandria_studio::Result<Value>,
    ) -> Result<Value, String> {
        let db = self.db.lock().map_err(|_| "fixture database".to_string())?;
        f(&db).map_err(|error| error.to_string())
    }
}

/// Two learners, two published courses, and a three-skill chain where the goal
/// rests on a skill the owner has not been assessed on.
///
///   alpha ──► beta ──► gamma          owner holds: alpha
///   goal "fixture-exam" wants: gamma
const FIXTURE: &str = r#"
INSERT INTO local_identity(id, stake_address, payment_address)
     VALUES (1, 'stake_fixture_owner', 'addr_fixture_owner');
INSERT INTO app_settings(key, value) VALUES ('identity.local_did', '__OWNER__');

INSERT INTO subject_fields(id, name) VALUES ('field-science', 'Science');
INSERT INTO subjects(id, name, subject_field_id)
     VALUES ('subject-computing', 'Computing', 'field-science');
INSERT INTO skills(id, name, bloom_level, subject_id, synonyms) VALUES
    ('skill-alpha', 'Alpha',  'remember', 'subject-computing', 'first steps'),
    ('skill-beta',  'Beta',   'apply',    'subject-computing', NULL),
    ('skill-gamma', 'Gamma',  'analyze',  'subject-computing', NULL);
INSERT INTO skill_prerequisites(skill_id, prerequisite_id) VALUES
    ('skill-beta',  'skill-alpha'),
    ('skill-gamma', 'skill-beta');

-- The owner has been assessed on alpha only, so gamma is two steps away and
-- beta is the one assessment that unlocks the next.
INSERT INTO credentials(id, issuer_did, subject_did, credential_type, claim_kind, skill_id,
                        issuance_date, signed_vc_json, integrity_hash, revoked, received_at)
     VALUES ('urn:uuid:fixture-alpha', 'did:key:z6MkfixtureIssuerAAAAAAAAAAAAAAAAAAAAAAAAA',
             '__OWNER__', 'FormalCredential', 'skill', 'skill-alpha', '2026-01-01',
             '{"note":"fixture credential; verify_credential takes caller-supplied JSON"}',
             'fixture-hash-alpha', 0, '2026-01-02');

INSERT INTO courses(id, title, description, author_address, status, skill_ids, published_at) VALUES
    ('course-beta',  'Getting to Beta',  'Teaches beta.',  'stake_other_author', 'published', '["skill-beta"]',  '2026-02-01'),
    ('course-gamma', 'Getting to Gamma', 'Teaches gamma.', 'stake_other_author', 'published', '["skill-gamma"]', '2026-02-02');
INSERT INTO catalog(course_id, title, description, author_address, content_cid, tags, skill_ids,
                    version, published_at, signature) VALUES
    ('course-beta',  'Getting to Beta',  'Teaches beta.',  'stake_other_author', 'cid-beta',
     '["fixture"]', '["skill-beta"]',  1, '2026-02-01', 'fixture-signature'),
    ('course-gamma', 'Getting to Gamma', 'Teaches gamma.', 'stake_other_author', 'cid-gamma',
     '["fixture"]', '["skill-gamma"]', 1, '2026-02-02', 'fixture-signature');

INSERT INTO course_chapters(id, course_id, title, position) VALUES
    ('chapter-beta',  'course-beta',  'Chapter one', 0),
    ('chapter-gamma', 'course-gamma', 'Chapter one', 0);
INSERT INTO course_elements(id, chapter_id, title, element_type, content_inline, position) VALUES
    ('lesson-beta',  'chapter-beta',  'What beta is', 'text',
     'Beta builds on alpha. This lesson is fixture text, safe to read aloud.', 0),
    ('quiz-beta',    'chapter-beta',  'Check yourself', 'quiz',
     '{"title":"Check","questions":[{"id":"q1","type":"single_choice","prompt":"What does beta rest on?","options":["alpha","gamma"],"correct_indices":[0],"explanation":"IF YOU CAN READ THIS, ANSWERS LEAKED","points":1}]}', 1),
    ('exam-beta',    'chapter-beta',  'Final assessment', 'assessment',
     'IF YOU CAN READ THIS, ASSESSMENT CONTENT LEAKED', 2),
    ('lesson-gamma', 'chapter-gamma', 'What gamma is', 'text',
     'Gamma builds on beta, which builds on alpha.', 0);

-- A course the owner is writing, never published: the draft tools read it and
-- propose changes to it, and nothing that lists published courses may show it.
INSERT INTO courses(id, title, description, author_address, status, skill_ids) VALUES
    ('course-draft', 'Owner draft', 'Being written.', 'stake_fixture_owner', 'draft', '["skill-beta"]');
INSERT INTO course_chapters(id, course_id, title, position) VALUES
    ('chapter-draft', 'course-draft', 'Chapter one', 0);
INSERT INTO course_elements(id, chapter_id, title, element_type, content_inline, position) VALUES
    ('lesson-draft', 'chapter-draft', 'Unpublished lesson', 'text',
     'DRAFT TEXT: the owner is still writing this.', 0);

INSERT INTO enrollments(id, course_id, status) VALUES ('enrol-beta', 'course-beta', 'active');
INSERT INTO element_progress(id, enrollment_id, element_id, status, score, time_spent)
     VALUES ('progress-beta', 'enrol-beta', 'lesson-beta', 'completed', NULL, 240);

INSERT INTO goal_templates(id, kind, key, label, skill_ids, ratified)
     VALUES ('template-fixture', 'exam', 'fixture-exam', 'Fixture exam', '["skill-gamma"]', 1);
"#;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "/tmp/alexandria-mcp-fixture".to_string()),
    );
    if root.exists() {
        // Predictable reruns: the fixture is rebuilt, never merged into.
        std::fs::remove_dir_all(&root)?;
    }
    std::fs::create_dir_all(&root)?;
    let root = root.canonicalize()?;

    let db = Connection::open(root.join("profile.db"))?;
    db.execute_batch("PRAGMA foreign_keys=ON;")?;
    for (_, _, sql) in schema::MIGRATIONS {
        db.execute_batch(sql)?;
    }
    db.execute_batch(&FIXTURE.replace("__OWNER__", OWNER_DID))?;

    let host = Arc::new(Fixture {
        db: Mutex::new(db),
        grants: Mutex::default(),
        gate: tokio::sync::RwLock::new(()),
    });

    let dir = broker::private_directory(&root)?;
    broker::sweep(&dir, &[], None);
    let listener = tokio::net::UnixListener::bind(broker::socket_path(&dir))?;
    let (_, connection_file) = broker::issue_connection(
        &dir,
        &mut host.grants.lock().unwrap(),
        "Fixture assistant".into(),
        GRANT_SCOPES.iter().map(|scope| scope.to_string()).collect(),
        host.epoch(),
        chrono::Utc::now().timestamp(),
    )?;

    let vectors = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../alexandria-verify/tests/vectors")
        .canonicalize()
        .unwrap_or_default();
    println!(
        r#"
Alexandria fixture profile is serving.

  profile           {profile}
  connection file   {connection}
  grant             {scopes} (one hour)

Point an MCP client at the built server with that file in its environment:

  {{
    "command": "{binary}",
    "env": {{ "ALEXANDRIA_MCP_CONNECTION_FILE": "{connection}" }}
  }}

Or run the Inspector against it:

  ALEXANDRIA_MCP_CONNECTION_FILE={connection} scripts/mcp/inspect.sh

Things worth asking it, and what should come back:

  search_catalog {{}}                       two published courses
  get_course {{"course_id":"course-beta"}}   a text lesson, a quiz, a withheld assessment
  read_lesson {{"course_id":"course-beta","element_id":"quiz-beta"}}
                                           the question, never the answer or explanation
  read_lesson {{"course_id":"course-beta","element_id":"exam-beta"}}
                                           withheld: assessments are not shared
  get_skill_graph {{}}                      alpha only — the owner's evidence
  resolve_goal {{"kind":"exam","key":"fixture-exam"}}
                                           gamma
  compute_learning_path {{"goal_skill_ids":["skill-gamma"]}}
                                           alpha earned, beta available, gamma locked
  list_my_credentials {{}}                  one summary, no signed document
  list_course_drafts {{}}                   the owner's one unpublished lesson
  read_lesson_draft {{"course_id":"course-draft","element_id":"lesson-draft"}}
                                           its text and a fingerprint
  propose_lesson_draft                     with that fingerprint: waits for review,
                                           changes nothing until the owner applies it
  verify_credential                        caller-supplied JSON; vectors in
                                           {vectors}

Ctrl-C stops it. Everything it made is under {root}.
"#,
        profile = root.join("profile.db").display(),
        connection = connection_file.display(),
        scopes = GRANT_SCOPES.join(", "),
        binary = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/debug/alexandria-mcp")
            .canonicalize()
            .unwrap_or_default()
            .display(),
        vectors = vectors.display(),
        root = root.display(),
    );

    broker::serve(listener, host).await;
    Ok(())
}
