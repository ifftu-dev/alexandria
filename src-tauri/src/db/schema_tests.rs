//! Baseline schema invariants.
//!
//! The tests these replace each asserted that one migration transformed rows
//! correctly: 072's backfill, 073's bloom normalisation, 083 and 084's
//! rebuilds, 088's snapshot columns, 091's course binding. They died with the
//! chain they tested, and they could only ever describe a journey between two
//! schemas.
//!
//! What matters about a baseline is different: which objects it creates, which
//! ones it must never create again, and that it refuses to adopt a database
//! written by the old chain instead of silently treating it as current.

use rusqlite::Connection;

use super::schema::MIGRATIONS;
use super::Database;

fn migrated() -> Database {
    let db = Database::open_in_memory().expect("open database");
    db.run_migrations().expect("apply baseline");
    db
}

fn names(conn: &Connection, kind: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = ?1 AND name NOT LIKE 'sqlite_%'")
        .expect("prepare");
    stmt.query_map([kind], |row| row.get::<_, String>(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("read names")
}

fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |row| row.get::<_, i64>(0),
    )
    .expect("count")
        > 0
}

fn columns(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
        .expect("prepare");
    stmt.query_map([], |row| row.get::<_, String>(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("read columns")
}

/// Tables the baseline must never create. Each lost the code that gave it
/// authority in D01 or D02, and a schema is the last place such a thing can
/// hide: the rows outlive the feature unless the table goes too.
const FORBIDDEN_TABLES: [&str; 19] = [
    "credential_challenges",
    "credential_challenge_votes",
    "plugin_attestations",
    "plugin_advisories",
    "sentinel_kill_switch",
    "sentinel_weights_blocklist",
    "sentinel_priors",
    "integrity_attestations",
    "onchain_governance_queue",
    "governance_daos",
    "governance_dao_members",
    "governance_proposals",
    "governance_elections",
    "governance_election_nominees",
    "governance_election_votes",
    "governance_proposal_votes",
    "bank_questions",
    "question_bank_versions",
    // Recorded a DAO committee signature withdrawing an opinion. Its signing
    // authority went with local governance, and its foreign key pointed at a
    // table the baseline no longer creates.
    "opinion_withdrawals",
];

/// The baseline is migration 1 and later schema changes append to it. The
/// runner requires applied history to be an exact prefix of this list, so a
/// gap or a reused number would strand every database that recorded it.
#[test]
fn the_baseline_comes_first_and_migrations_are_contiguous() {
    let (version, name, _) = MIGRATIONS[0];
    assert_eq!(version, 1);
    assert_eq!(
        name, "baseline",
        "the baseline must not reuse the old chain's name for version 1"
    );
    let mut names = std::collections::HashSet::new();
    for (index, (version, name, _)) in MIGRATIONS.iter().enumerate() {
        assert_eq!(
            *version,
            index as i64 + 1,
            "migration versions must be contiguous"
        );
        assert!(names.insert(*name), "migration name {name} is reused");
    }
}

#[test]
fn forbidden_tables_are_absent() {
    let db = migrated();
    for table in FORBIDDEN_TABLES {
        assert!(
            !table_exists(db.conn(), table),
            "{table} is retired and must not be created by the baseline"
        );
    }
}

#[test]
fn retired_columns_are_absent() {
    let db = migrated();
    let identity = columns(db.conn(), "local_identity");
    assert!(
        !identity.iter().any(|c| c == "account_role"),
        "the single-valued account_role is superseded by account_roles"
    );
    assert!(identity.iter().any(|c| c == "account_roles"));

    let snapshots = columns(db.conn(), "reputation_snapshots");
    for retired in [
        "policy_id",
        "ref_asset_name",
        "user_asset_name",
        "snapshot_format",
        "snapshot_scope",
    ] {
        assert!(
            !snapshots.iter().any(|c| c == retired),
            "{retired} belongs to the deleted CIP-68 mint"
        );
    }
    assert!(snapshots.iter().any(|c| c == "credential_id"));
}

#[test]
fn the_schema_keeps_what_the_runtime_reads() {
    let db = migrated();
    // A sample of tables and the single view that surviving code queries. The
    // bundled seed defect was exactly this shape -- code reading a table the
    // schema no longer served -- so the view is asserted by name.
    for table in [
        "credentials",
        "assessment_items",
        "assessment_item_skills",
        "question_banks",
        "reputation_assertions",
        "reputation_snapshots",
        "reputation_snapshot_inputs",
        "governance_genesis_trust_anchors",
        "chain_submissions",
        "completion_claims",
    ] {
        assert!(table_exists(db.conn(), table), "{table} is missing");
    }
    assert!(names(db.conn(), "view")
        .iter()
        .any(|v| v == "current_reputation_assertions"));
    // Three integrity triggers, and three keeping chat search in step.
    assert_eq!(names(db.conn(), "trigger").len(), 6);
}

/// Asserting that a wanted object is present cannot catch an unwanted one
/// coming back. The retired scoring view would pass every other test in this
/// file, so the view set is pinned exactly.
#[test]
fn the_view_set_is_exact() {
    let db = migrated();
    let mut views = names(db.conn(), "view");
    views.sort();
    assert_eq!(views, vec!["current_reputation_assertions".to_string()]);
}

/// The baseline was verified once, during construction, against a replay of
/// the 94 migrations it replaced. That reference no longer exists in the repo,
/// so the parity argument cannot be re-run. These counts are what remains: a
/// drift detector that fails on any object added or removed without intent.
/// They cover every migration: the baseline's 92 tables and 105 indexes, plus
/// the instructor studio's 5 tables and 1 index, personhood's table and index,
/// the decision shadow sample table, local opinion examples, and the
/// learner's hiring answers. The attempt role target adds 1 index; the
/// device attestation adds a column only. Organisation access adds 3 tables
/// (descriptor cache, grants, listing publications) and 1 index; the
/// interview panel link adds 1 index. Global AI chat adds 8 tables (threads,
/// messages, attachments, excerpts, snapshots, snapshot excerpts, runs,
/// drafts) and 4 indexes; chat practice adds 2 tables (items, attempts) and 2
/// indexes; learning memory adds 1 table and 1 index;
/// guardian approval of remote models adds 2 tables and 1 index; chat
/// search adds one FTS5 table (6 schema tables with its shadows) and 3
/// triggers; chat artifacts add 1 table and 2 indexes. Notifications add
/// 4 tables (inbox, deduplication, registration, operation state) and 1 unread index.
#[test]
fn the_schema_object_counts_are_pinned() {
    let db = migrated();
    let tables = names(db.conn(), "table")
        .into_iter()
        .filter(|t| t != "_migrations" && t != "_schema_identity")
        .count();
    assert_eq!(tables, 135, "table count changed");
    assert_eq!(names(db.conn(), "index").len(), 127, "index count changed");
    assert_eq!(
        names(db.conn(), "trigger").len(),
        6,
        "trigger count changed"
    );
    assert_eq!(names(db.conn(), "view").len(), 1, "view count changed");
}

/// The scalar function the old chain registered on every open. Nothing in the
/// baseline references it, and a database that still resolved it would mean
/// the registration had crept back.
#[test]
fn the_retired_course_authority_function_is_not_registered() {
    let db = migrated();
    let resolved = db
        .conn()
        .query_row("SELECT legacy_course_authority_did('addr')", [], |row| {
            row.get::<_, String>(0)
        });
    assert!(
        resolved.is_err(),
        "legacy_course_authority_did resolved; the retired registration is back"
    );
}

/// Every foreign key must point at a table this schema creates.
///
/// `pragma_foreign_key_check` cannot catch this: it inspects existing rows, so
/// a table that is always empty keeps a dangling reference indefinitely. That
/// is exactly what happened when the baseline dropped the governance tables
/// and left `opinion_withdrawals` pointing at one of them — the table applied
/// cleanly, passed the row-level check, and would have failed on first insert
/// with "no such table". Removing a table has to account for what points AT
/// it, not only what it points at.
#[test]
fn no_foreign_key_points_at_a_missing_table() {
    let db = migrated();
    let tables: Vec<String> = names(db.conn(), "table");
    let mut dangling = Vec::new();
    for table in &tables {
        let mut stmt = db
            .conn()
            .prepare(&format!("PRAGMA foreign_key_list({table})"))
            .expect("prepare");
        let targets: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(2))
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("read targets");
        for target in targets {
            if !tables.iter().any(|t| t == &target) {
                dangling.push(format!("{table} -> {target}"));
            }
        }
    }
    assert!(dangling.is_empty(), "dangling foreign keys: {dangling:?}");
}

#[test]
fn foreign_keys_are_valid() {
    let db = migrated();
    let violations: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .expect("foreign key check");
    assert_eq!(violations, 0);
}

#[test]
fn initialising_twice_changes_nothing() {
    let db = migrated();
    let before = names(db.conn(), "table");
    db.run_migrations().expect("second run");
    let after = names(db.conn(), "table");
    assert_eq!(before, after);
    let applied: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM _migrations", [], |row| row.get(0))
        .expect("count applied");
    assert_eq!(applied, MIGRATIONS.len() as i64);
}

#[test]
fn a_database_from_the_old_chain_is_refused() {
    // Both an old database at version 1 and a fully migrated one record
    // `initial_schema`, which is not this schema family. Neither may be
    // treated as already satisfying the baseline.
    for history in [vec![(1_i64, "initial_schema")], {
        let mut rows = vec![(1_i64, "initial_schema")];
        rows.extend((2..=94).map(|v| (v as i64, "later")));
        rows
    }] {
        let db = Database::open_in_memory().expect("open database");
        db.conn()
            .execute_batch(
                "CREATE TABLE _migrations (
                     version INTEGER PRIMARY KEY,
                     name TEXT NOT NULL,
                     applied_at TEXT NOT NULL DEFAULT (datetime('now')));",
            )
            .expect("create history");
        for (version, name) in &history {
            db.conn()
                .execute(
                    "INSERT INTO _migrations (version, name) VALUES (?1, ?2)",
                    rusqlite::params![version, name],
                )
                .expect("record history");
        }

        let error = db
            .run_migrations()
            .expect_err("an old-chain database must not be adopted");
        let message = error.to_string();
        assert!(
            message.contains("unsupported profile database"),
            "refusal should name the schema family, got: {message}"
        );
        assert!(
            !table_exists(db.conn(), "credentials"),
            "a refused database must not be written to"
        );
    }
}

/// Schema-family identity.
///
/// The prefix check on `_migrations` can only compare a version number and a
/// name recorded *inside* the database. These assert the stronger property:
/// the family is stamped in the SQLite file header, so a foreign or future
/// database is identified before a table is read, and is refused rather than
/// migrated or deleted.
#[cfg(test)]
mod identity {
    use super::*;
    use crate::db::{SCHEMA_APPLICATION_ID, SCHEMA_EPOCH, SCHEMA_FAMILY};

    fn pragma(db: &Database, name: &str) -> i32 {
        db.conn()
            .query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
            .expect("read pragma")
    }

    #[test]
    fn a_fresh_database_is_stamped_with_the_family() {
        let db = migrated();
        assert_eq!(pragma(&db, "application_id"), SCHEMA_APPLICATION_ID);
        assert_eq!(pragma(&db, "user_version"), SCHEMA_EPOCH);
        let (family, epoch): (String, i32) = db
            .conn()
            .query_row("SELECT family, epoch FROM _schema_identity", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .expect("identity row");
        assert_eq!(family, SCHEMA_FAMILY);
        assert_eq!(epoch, SCHEMA_EPOCH);
    }

    #[test]
    fn a_foreign_application_id_is_refused() {
        let db = Database::open_in_memory().expect("open database");
        db.conn()
            .pragma_update(None, "application_id", 0x0BAD_F00D_u32 as i32)
            .expect("stamp foreign id");

        let message = db
            .run_migrations()
            .expect_err("a foreign database must be refused")
            .to_string();
        assert!(
            message.contains("not an Alexandria profile database"),
            "got: {message}"
        );
        assert!(!table_exists(db.conn(), "credentials"));
    }

    #[test]
    fn a_newer_schema_epoch_is_refused() {
        let db = Database::open_in_memory().expect("open database");
        db.conn()
            .pragma_update(None, "application_id", SCHEMA_APPLICATION_ID)
            .expect("stamp id");
        db.conn()
            .pragma_update(None, "user_version", SCHEMA_EPOCH + 1)
            .expect("stamp future epoch");

        let message = db
            .run_migrations()
            .expect_err("a future schema must be refused")
            .to_string();
        assert!(
            message.contains("newer Alexandria"),
            "a future database should say so plainly, got: {message}"
        );
    }

    #[test]
    fn a_header_that_disagrees_with_the_contents_is_refused() {
        let db = migrated();
        db.conn()
            .execute("UPDATE _schema_identity SET family = 'someone.else'", [])
            .expect("tamper");

        let message = db
            .run_migrations()
            .expect_err("a disagreeing identity must be refused")
            .to_string();
        assert!(
            message.contains("header and contents disagree"),
            "got: {message}"
        );
    }

    /// The branch that refuses a database from an older family. Until a second
    /// epoch exists there is no upgrade path, and an untested refusal branch is
    /// indistinguishable from one that silently accepts.
    #[test]
    fn an_earlier_schema_epoch_is_refused() {
        let db = Database::open_in_memory().expect("open database");
        db.conn()
            .pragma_update(None, "application_id", SCHEMA_APPLICATION_ID)
            .expect("stamp id");
        db.conn()
            .pragma_update(None, "user_version", SCHEMA_EPOCH - 1)
            .expect("stamp earlier epoch");

        let message = db
            .run_migrations()
            .expect_err("an earlier family must be refused")
            .to_string();
        assert!(message.contains("earlier schema family"), "got: {message}");
        assert!(!table_exists(db.conn(), "credentials"));
    }

    #[test]
    fn every_refusal_offers_a_non_destructive_remedy() {
        let db = Database::open_in_memory().expect("open database");
        db.conn()
            .pragma_update(None, "application_id", 0x0BAD_F00D_u32 as i32)
            .expect("stamp foreign id");

        let message = db.run_migrations().expect_err("refused").to_string();
        assert!(
            message.contains("Nothing has been changed or deleted"),
            "a refusal must not imply the file was touched, got: {message}"
        );
        assert!(
            !message.contains("delete the database") && !message.contains("will be removed"),
            "a refusal must never direct destruction of an unexamined file: {message}"
        );
    }
}

#[test]
fn a_failed_baseline_leaves_no_schema_and_can_retry() {
    let db = Database::open_in_memory().expect("open database");
    db.conn()
        .execute_batch(
            "CREATE TABLE _migrations (
                 version INTEGER PRIMARY KEY,
                 name TEXT NOT NULL,
                 applied_at TEXT NOT NULL DEFAULT (datetime('now')));
             CREATE TEMP TRIGGER fail_record BEFORE INSERT ON _migrations
             BEGIN SELECT RAISE(ABORT, 'injected record failure'); END;",
        )
        .expect("install fault");

    assert!(db.run_migrations().is_err());
    assert!(
        !table_exists(db.conn(), "credentials"),
        "the baseline and its marker commit together, so a failed marker \
         must take the whole schema with it"
    );

    db.conn()
        .execute_batch("DROP TRIGGER fail_record")
        .expect("remove fault");
    db.run_migrations()
        .expect("retry after the fault is cleared");
    assert!(table_exists(db.conn(), "credentials"));
}

// Frozen SQL from the provisional notification branch, before main assigned
// version 18 to call kinds. Keep this fixture independent of migration 28.
const PROVISIONAL_NOTIFICATIONS: &str = r#"
CREATE TABLE notifications (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    route TEXT,
    source TEXT NOT NULL DEFAULT 'local' CHECK (source IN ('local','push')),
    dedupe_key TEXT UNIQUE,
    payload_json TEXT CHECK (payload_json IS NULL OR json_valid(payload_json)),
    created_at TEXT NOT NULL,
    read_at TEXT
);
CREATE INDEX idx_notifications_unread ON notifications(created_at) WHERE read_at IS NULL;
CREATE TABLE notification_dedupe (
    dedupe_key TEXT PRIMARY KEY,
    seen_at TEXT NOT NULL
);
CREATE TABLE push_registrations (
    cloud_origin TEXT PRIMARY KEY,
    device_id TEXT NOT NULL,
    platform TEXT NOT NULL,
    token TEXT NOT NULL,
    registered_at TEXT NOT NULL
);
"#;

fn provisional_notification_database() -> Database {
    with_provisional_notifications(Database::open_in_memory().unwrap())
}

fn with_provisional_notifications(db: Database) -> Database {
    let mut history = MIGRATIONS[..17].to_vec();
    history.push((18, "notifications", PROVISIONAL_NOTIFICATIONS));
    db.run_migrations_from(&history).unwrap();
    db.conn()
        .execute_batch(
            r#"INSERT INTO notifications
            (id, kind, title, body, route, source, dedupe_key, payload_json, created_at, read_at)
         VALUES ('saved', 'interview', 'Invitation', 'Saved body', '/interviews', 'push',
                 'interview:saved', '{"ref":"saved"}', '2026-10-25', '2026-10-26');
         INSERT INTO notification_dedupe VALUES ('interview:saved', '2026-10-25');
         INSERT INTO notification_dedupe VALUES ('offer:cleared', '2026-10-24');
         INSERT INTO push_registrations VALUES
            ('https://cloud.example', 'device-saved', 'fcm', 'test-token', '2026-10-25');
         INSERT INTO tutoring_sessions (id, title) VALUES ('old-call', 'Saved call');
         INSERT INTO interview_sessions
            (id, title, tutoring_session_id, expires_at, cloud_interview_id)
         VALUES ('old-interview', 'Saved interview', 'old-call', '2026-10-28', 'cloud-interview');
         UPDATE _migrations SET applied_at = '2026-10-25' WHERE version = 18;"#,
        )
        .unwrap();
    db
}

#[test]
fn provisional_notifications_upgrade_preserves_data_and_backfills_call_kinds() {
    let db = provisional_notification_database();
    assert_eq!(super::run_migrations_on_connection(db.conn()).unwrap(), 11);
    assert_eq!(super::run_migrations_on_connection(db.conn()).unwrap(), 0);
    let saved: (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
    ) = db
        .conn()
        .query_row(
            "SELECT kind, title, body, route, source, dedupe_key, payload_json, created_at, read_at
         FROM notifications WHERE id = 'saved'",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        saved,
        (
            "interview".into(),
            "Invitation".into(),
            "Saved body".into(),
            "/interviews".into(),
            "push".into(),
            "interview:saved".into(),
            r#"{"ref":"saved"}"#.into(),
            "2026-10-25".into(),
            "2026-10-26".into()
        )
    );
    let dedupe: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM notification_dedupe WHERE
         (dedupe_key = 'interview:saved' AND seen_at = '2026-10-25') OR
         (dedupe_key = 'offer:cleared' AND seen_at = '2026-10-24')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(dedupe, 2);
    let registration: (String, String, String, String) = db
        .conn()
        .query_row(
            "SELECT device_id, platform, token, registered_at FROM push_registrations
         WHERE cloud_origin = 'https://cloud.example'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        registration,
        (
            "device-saved".into(),
            "fcm".into(),
            "test-token".into(),
            "2026-10-25".into()
        )
    );
    let call: (String, String, String) = db
        .conn()
        .query_row(
            "SELECT title, kind, context_id FROM tutoring_sessions WHERE id = 'old-call'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        call,
        (
            "Saved call".into(),
            "interview_panel".into(),
            "old-interview".into()
        )
    );
    let history: Vec<(i64, String)> = db
        .conn()
        .prepare("SELECT version, name FROM _migrations WHERE version >= 18 ORDER BY version")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        history,
        vec![
            (18, "call_kinds".into()),
            (19, "ai_chat".into()),
            (20, "ai_chat_practice".into()),
            (21, "learning_memory".into()),
            (22, "ai_connection_approvals".into()),
            (23, "ai_chat_search".into()),
            (24, "ai_chat_goal_bindings".into()),
            (25, "ai_chat_artifacts".into()),
            (26, "course_notes_body".into()),
            (27, "ai_chat_actions".into()),
            (28, "notifications".into()),
            (29, "notification_recipient_lifecycle".into())
        ]
    );
    let applied_at: String = db
        .conn()
        .query_row(
            "SELECT applied_at FROM _migrations WHERE version = 28",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(applied_at, "2026-10-25");
}

#[test]
fn provisional_notifications_with_missing_storage_are_not_marked_upgraded() {
    for table in ["notifications", "notification_dedupe", "push_registrations"] {
        let db = provisional_notification_database();
        db.conn()
            .execute_batch(&format!("DROP TABLE {table}"))
            .unwrap();
        assert!(db.run_migrations().is_err(), "accepted missing {table}");
        let marker: String = db
            .conn()
            .query_row("SELECT name FROM _migrations WHERE version = 18", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(marker, "notifications");
        assert!(!columns(db.conn(), "tutoring_sessions").contains(&"kind".to_string()));
    }
}

#[test]
fn notifications_migrate_fresh_and_current_main_without_changing_existing_calls() {
    for prior_version in [0, 27] {
        let db = Database::open_in_memory().unwrap();
        if prior_version == 27 {
            db.run_migrations_from(&MIGRATIONS[..27]).unwrap();
            db.conn()
                .execute_batch(
                    "INSERT INTO tutoring_sessions (id, title, kind, context_id)
                 VALUES ('saved', 'Existing call', 'plugin', 'plugin-saved');",
                )
                .unwrap();
        }
        assert_eq!(
            super::run_migrations_on_connection(db.conn()).unwrap(),
            29 - prior_version
        );
        assert_eq!(super::run_migrations_on_connection(db.conn()).unwrap(), 0);
        db.conn().execute_batch(
            "INSERT INTO notifications (id, kind, title, created_at)
             VALUES ('new', 'credential', 'New credential', '2026-10-27');
             INSERT INTO notification_dedupe VALUES ('credential:new', '2026-10-27');
             INSERT INTO push_registrations(cloud_origin,device_id,platform,token,registered_at) VALUES ('https://cloud.example', 'device', 'fcm', 'token', '2026-10-27');"
        ).unwrap();
        assert_eq!(crate::notifications::unread_count(db.conn()).unwrap(), 1);
        let kind_marker: String = db
            .conn()
            .query_row("SELECT name FROM _migrations WHERE version = 18", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(kind_marker, "call_kinds");
        let notification_marker: String = db
            .conn()
            .query_row("SELECT name FROM _migrations WHERE version = 28", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(notification_marker, "notifications");
        if prior_version == 27 {
            let call: (String, String, String) = db
                .conn()
                .query_row(
                    "SELECT title, kind, context_id FROM tutoring_sessions WHERE id = 'saved'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            assert_eq!(
                call,
                (
                    "Existing call".into(),
                    "plugin".into(),
                    "plugin-saved".into()
                )
            );
        }
    }
}

#[test]
fn provisional_notifications_failed_upgrade_reopens_and_retries_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("provisional.db");
    let key = [37; 32];
    {
        let db = with_provisional_notifications(Database::open_encrypted(&path, &key).unwrap());
        db.conn()
            .execute_batch(
                "CREATE TRIGGER fail_upgrade BEFORE INSERT ON _migrations WHEN NEW.version = 25
             BEGIN SELECT RAISE(ABORT, 'injected upgrade failure'); END;",
            )
            .unwrap();
        let error = db.run_migrations().unwrap_err().to_string();
        assert!(error.contains("injected upgrade failure"), "{error}");
    }
    assert!(!Database::is_plaintext(&path));
    let db = Database::open_encrypted(&path, &key).unwrap();
    let marker: (String, String) = db
        .conn()
        .query_row(
            "SELECT name, applied_at FROM _migrations WHERE version = 18",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(marker, ("notifications".into(), "2026-10-25".into()));
    assert!(!columns(db.conn(), "tutoring_sessions").contains(&"kind".to_string()));
    assert!(!table_exists(db.conn(), "chat_threads"));
    let read_at: String = db
        .conn()
        .query_row(
            "SELECT read_at FROM notifications WHERE id = 'saved'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(read_at, "2026-10-26");
    db.conn()
        .execute_batch("DROP TRIGGER fail_upgrade")
        .unwrap();
    assert_eq!(super::run_migrations_on_connection(db.conn()).unwrap(), 11);
    assert_eq!(super::run_migrations_on_connection(db.conn()).unwrap(), 0);
}

#[test]
fn provisional_notifications_unknown_histories_are_refused_without_rewriting_them() {
    for damage in [
        "UPDATE _migrations SET name = 'unknown_notifications' WHERE version = 18",
        "UPDATE _migrations SET name = 'other_baseline' WHERE version = 1",
        "DELETE FROM _migrations WHERE version = 5",
        "INSERT INTO _migrations (version, name) VALUES (19, 'unknown_extension')",
    ] {
        let db = provisional_notification_database();
        db.conn().execute_batch(damage).unwrap();
        let history = || {
            db.conn()
                .prepare("SELECT version, name, applied_at FROM _migrations ORDER BY version")
                .unwrap()
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        let before = history();
        assert!(db.run_migrations().is_err());
        assert_eq!(history(), before);
        assert!(!columns(db.conn(), "tutoring_sessions").contains(&"kind".to_string()));
        assert_eq!(
            crate::notifications::list(db.conn(), None, false)
                .unwrap()
                .len(),
            1
        );
    }
}
