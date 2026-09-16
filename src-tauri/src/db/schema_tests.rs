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

#[test]
fn the_baseline_is_the_only_migration() {
    assert_eq!(MIGRATIONS.len(), 1);
    let (version, name, _) = MIGRATIONS[0];
    assert_eq!(version, 1);
    assert_eq!(
        name, "baseline",
        "the baseline must not reuse the old chain's name for version 1"
    );
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
    assert_eq!(names(db.conn(), "trigger").len(), 3);
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
#[test]
fn the_schema_object_counts_are_pinned() {
    let db = migrated();
    let tables = names(db.conn(), "table")
        .into_iter()
        .filter(|t| t != "_migrations" && t != "_schema_identity")
        .count();
    assert_eq!(tables, 92, "baseline table count changed");
    assert_eq!(names(db.conn(), "index").len(), 105, "index count changed");
    assert_eq!(
        names(db.conn(), "trigger").len(),
        3,
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
    assert_eq!(applied, 1);
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
