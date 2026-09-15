pub(crate) mod executor;
pub(crate) mod governance;
pub(crate) mod governance_genesis;
pub(crate) mod opinion_eligibility;
pub mod schema;
#[cfg(test)]
mod schema_tests;
#[cfg(test)]
mod scoring_adversarial_tests;
pub(crate) mod scoring_inputs;
pub mod seed;
pub mod seed_content;
pub mod seed_plugin_demo;

use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior};
use std::path::Path;
use thiserror::Error;
use zeroize::Zeroizing;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration failed: {0}")]
    Migration(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Wraps a SQLite connection with Alexandria-specific operations.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open (or create) a SQLite database at the given path.
    ///
    /// The connection is Send but not Sync. Shared access requires an
    /// exclusive mutex; SQLite's serialized mode does not protect rusqlite's
    /// Rust-side connection state.
    pub fn open(path: &Path) -> Result<Self, DbError> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_FULL_MUTEX;
        let conn = Connection::open_with_flags(path, flags)?;

        // Enable WAL mode for better concurrent read performance.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // Enable foreign keys.
        conn.pragma_update(None, "foreign_keys", "ON")?;

        register_issuer_recognition(&conn)?;

        Ok(Self { conn })
    }

    /// Open (or create) a SQLCipher-encrypted database at the given path.
    ///
    /// The key MUST be set as the very first statement after open.
    /// Uses hex-encoded key format for SQLCipher: `PRAGMA key = "x'...'";`
    pub fn open_encrypted(path: &Path, key: &[u8; 32]) -> Result<Self, DbError> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_FULL_MUTEX;
        let conn = Connection::open_with_flags(path, flags)?;

        // Set the encryption key — MUST be the first PRAGMA after open.
        let key_hex = Zeroizing::new(hex::encode(key));
        let key_pragma = Zeroizing::new(format!("x'{}'", key_hex.as_str()));
        conn.pragma_update(None, "key", key_pragma.as_str())?;

        // Enable WAL mode for better concurrent read performance.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // Enable foreign keys.
        conn.pragma_update(None, "foreign_keys", "ON")?;

        register_issuer_recognition(&conn)?;

        Ok(Self { conn })
    }

    /// Open an unencrypted in-memory database. Used by tests and by
    /// the offline credential bundle verifier — both want a fresh,
    /// transient store that holds nothing sensitive. The encrypted
    /// `open_encrypted` path stays the only way to open a persistent
    /// DB for real user data.
    pub fn open_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        register_issuer_recognition(&conn)?;
        Ok(Self { conn })
    }

    /// Detect whether a database file is unencrypted (legacy).
    ///
    /// Tries to open the file without a key and read `sqlite_master`.
    /// Returns `true` if the database is readable without encryption.
    pub fn is_plaintext(path: &Path) -> bool {
        if !path.exists() {
            return false;
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_FULL_MUTEX;
        let conn = match Connection::open_with_flags(path, flags) {
            Ok(c) => c,
            Err(_) => return false,
        };
        conn.query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        })
        .is_ok()
    }

    /// Run all schema migrations.
    pub fn run_migrations(&self) -> Result<(), DbError> {
        run_migrations_on_connection(&self.conn).map(|_| ())
    }

    #[cfg(test)]
    fn run_migrations_from(&self, migrations: &[(i64, &str, &str)]) -> Result<(), DbError> {
        run_migrations_from_connection(&self.conn, migrations).map(|_| ())
    }

    /// Get a reference to the underlying connection.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

/// Install the schema's pure SQL extension on externally owned connections.
/// Migration 085 calls it when the historical schema is replayed; migration
/// 094 removed every schema object that called it afterwards. Call after
/// configuring an encryption key, before migrations.
pub fn register_issuer_recognition(conn: &Connection) -> rusqlite::Result<()> {
    use rusqlite::functions::FunctionFlags;
    conn.create_scalar_function(
        "legacy_course_authority_did",
        1,
        FunctionFlags::SQLITE_UTF8
            | FunctionFlags::SQLITE_DETERMINISTIC
            | FunctionFlags::SQLITE_INNOCUOUS,
        |context| {
            let author: String = context.get(0)?;
            Ok(crate::crypto::did::course_authority_did(&author).0)
        },
    )
}

/// Apply the shared, atomic migration runner to an externally owned connection.
/// Returns the number of migrations committed by this call.
pub fn run_migrations_on_connection(conn: &Connection) -> Result<usize, DbError> {
    register_issuer_recognition(conn)?;
    run_migrations_from_connection(conn, schema::MIGRATIONS)
}

fn run_migrations_from_connection(
    conn: &Connection,
    migrations: &[(i64, &str, &str)],
) -> Result<usize, DbError> {
    // Create the migrations tracking table.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _migrations (
                version  INTEGER PRIMARY KEY,
                name     TEXT NOT NULL,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
    )?;

    let applied = {
        let mut stmt = conn.prepare("SELECT version, name FROM _migrations ORDER BY version")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for (index, (version, name)) in applied.iter().enumerate() {
        if !matches!(migrations.get(index), Some((expected_version, expected_name, _))
                if version == expected_version && name == expected_name)
        {
            return Err(DbError::Migration(format!(
                "migration history is not a supported prefix at version {version} ({name}); \
                     use a compatible application or repair the database history"
            )));
        }
    }

    let mut migrated = 0;
    for (version, name, sql) in migrations.iter().skip(applied.len()) {
        let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
        // Another connection may have migrated while this one waited for
        // the writer lock. Recheck under that lock before executing SQL.
        let recorded_name: Option<String> = tx
            .query_row(
                "SELECT name FROM _migrations WHERE version = ?1",
                [version],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(recorded_name) = recorded_name {
            if recorded_name != *name {
                return Err(DbError::Migration(format!(
                    "migration {version} has unexpected name {recorded_name}"
                )));
            }
            tx.commit()?;
            continue;
        }
        log::info!("Running migration {}: {}", version, name);
        tx.execute_batch(sql).map_err(|e| {
            DbError::Migration(format!("migration {} ({}) failed: {}", version, name, e))
        })?;
        tx.execute(
            "INSERT INTO _migrations (version, name) VALUES (?1, ?2)",
            rusqlite::params![version, name],
        )?;
        tx.commit()?;
        migrated += 1;
    }

    Ok(migrated)
}

/// Join a caller-owned transaction, or own one until the complete derived-state
/// operation succeeds. Never commit a transaction opened by the caller.
pub(crate) fn with_transaction<T>(
    conn: &Connection,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let tx = if conn.is_autocommit() {
        Some(conn.unchecked_transaction().map_err(|e| e.to_string())?)
    } else {
        None
    };
    let result = operation()?;
    if let Some(tx) = tx {
        tx.commit().map_err(|e| e.to_string())?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_migration_rolls_back_schema_and_data_before_retry() {
        let db = Database::open_in_memory().unwrap();
        let first = (1, "initial", "CREATE TABLE items (id INTEGER PRIMARY KEY);");
        let broken = (
            2,
            "extend",
            "ALTER TABLE items ADD COLUMN label TEXT; \
             INSERT INTO items (id, label) VALUES (1, 'kept only on success'); \
             INSERT INTO missing_table VALUES (1);",
        );
        assert!(db.run_migrations_from(&[first, broken]).is_err());
        assert!(db.conn.is_autocommit());
        assert!(db.conn.prepare("SELECT label FROM items").is_err());
        assert_eq!(
            db.conn
                .query_row("SELECT COUNT(*) FROM items", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.conn
                .query_row("SELECT MAX(version) FROM _migrations", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        let repaired = (2, "extend", "ALTER TABLE items ADD COLUMN label TEXT;");
        db.run_migrations_from(&[first, repaired]).unwrap();
        db.run_migrations_from(&[first, repaired]).unwrap();
        assert!(db.conn.prepare("SELECT label FROM items").is_ok());
    }

    #[test]
    fn migration_marker_failure_rolls_back_the_schema() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations_from(&[]).unwrap();
        db.conn
            .execute_batch(
                "CREATE TRIGGER reject_marker BEFORE INSERT ON _migrations \
             BEGIN SELECT RAISE(ABORT, 'injected marker failure'); END;",
            )
            .unwrap();
        let migrations = [(1, "first", "CREATE TABLE new_table (id INTEGER);")];
        assert!(db.run_migrations_from(&migrations).is_err());
        assert!(db.conn.prepare("SELECT * FROM new_table").is_err());
        db.conn.execute_batch("DROP TRIGGER reject_marker").unwrap();
        db.run_migrations_from(&migrations).unwrap();
    }

    #[test]
    fn migration_history_gaps_unknown_versions_and_renames_are_rejected() {
        for history in [
            "INSERT INTO _migrations (version, name) VALUES (2, 'second')",
            "INSERT INTO _migrations (version, name) VALUES (99, 'future')",
            "INSERT INTO _migrations (version, name) VALUES (1, 'unexpected')",
        ] {
            let db = Database::open_in_memory().unwrap();
            db.run_migrations_from(&[]).unwrap();
            db.conn.execute(history, []).unwrap();
            assert!(matches!(
                db.run_migrations_from(&[(1, "first", "CREATE TABLE untouched (id INTEGER);")]),
                Err(DbError::Migration(_))
            ));
            assert!(db.conn.prepare("SELECT * FROM untouched").is_err());
        }
    }

    #[test]
    fn encrypted_database_reopens_after_failed_upgrade_without_partial_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("migration-test.db");
        let key = [42; 32];
        let first = (1, "initial", "CREATE TABLE saved (id INTEGER PRIMARY KEY);");
        {
            let db = Database::open_encrypted(&path, &key).unwrap();
            db.run_migrations_from(&[first]).unwrap();
            db.conn.execute("INSERT INTO saved VALUES (7)", []).unwrap();
            assert!(db
                .run_migrations_from(&[
                    first,
                    (
                        2,
                        "extend",
                        "ALTER TABLE saved ADD COLUMN label TEXT; INSERT INTO absent VALUES (1);"
                    )
                ])
                .is_err());
        }
        assert!(!Database::is_plaintext(&path));
        let db = Database::open_encrypted(&path, &key).unwrap();
        assert!(db.conn.prepare("SELECT label FROM saved").is_err());
        db.run_migrations_from(&[
            first,
            (2, "extend", "ALTER TABLE saved ADD COLUMN label TEXT;"),
        ])
        .unwrap();
        assert_eq!(
            db.conn
                .query_row("SELECT id FROM saved", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            7
        );
    }

    #[test]
    fn open_in_memory_and_migrate() {
        let db = Database::open_in_memory().expect("failed to open in-memory db");
        db.run_migrations().expect("migrations failed");

        // Verify tables exist by querying sqlite_master
        let table_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE '\\__%' ESCAPE '\\'",
                [],
                |row| row.get(0),
            )
            .expect("failed to query tables");

        // We should have at least the core tables (courses, enrollments, skills, etc.)
        assert!(
            table_count >= 10,
            "expected at least 10 tables after migration, got {}",
            table_count
        );
    }

    #[test]
    fn migrations_are_idempotent() {
        let db = Database::open_in_memory().expect("failed to open in-memory db");
        db.run_migrations().expect("first migration failed");
        db.run_migrations()
            .expect("second migration should be idempotent");
    }

    /// P0 #3 — exercise migration 047 (`sentinel_user_models`) on a DB
    /// that already has rows in tables that existed before. Catches
    /// ALTER-TABLE / FK / unique-index regressions a fresh `migrate()`
    /// run wouldn't surface.
    #[test]
    fn migration_047_runs_on_populated_pre_47_state() {
        // Apply every migration up to 46 inline. We don't run all 47
        // and then "re-run" — we want to enter mig-47 with a real
        // pre-47 state.
        let db = Database::open_in_memory().expect("open in-memory");
        db.conn()
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS _migrations (
                    version INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                );",
            )
            .unwrap();
        for (version, name, sql) in schema::MIGRATIONS {
            if *version > 46 {
                break;
            }
            db.conn()
                .execute_batch(sql)
                .unwrap_or_else(|e| panic!("pre-47 migration {version} failed: {e}"));
            db.conn()
                .execute(
                    "INSERT INTO _migrations (version, name) VALUES (?1, ?2)",
                    rusqlite::params![version, name],
                )
                .unwrap();
        }

        // Populate something from the broader schema so we know the
        // ALTER-free migration 047 doesn't disturb existing rows.
        db.conn()
            .execute(
                "INSERT INTO governance_daos
                    (id, name, description, icon_emoji, scope_type, scope_id, status,
                     committee_size, election_interval_days)
                 VALUES
                    ('test-dao', 'Test', 'Pre-47 row', '🧪', 'sentinel', 'sentinel-global',
                     'active', 5, 365)
                 ON CONFLICT(id) DO NOTHING",
                [],
            )
            .unwrap();

        // Now run all pending migrations (just 47).
        db.run_migrations().expect("mig 47 should apply cleanly");

        // sentinel_user_models exists and is empty.
        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM sentinel_user_models", [], |row| {
                row.get(0)
            })
            .expect("sentinel_user_models should exist after migration 47");
        assert_eq!(count, 0);

        // INSERT exercises the composite PK + ON CONFLICT path used by
        // the production save_user_model helper.
        db.conn()
            .execute(
                "INSERT INTO sentinel_user_models
                    (user_address, device_fp_prefix, model_kind, weights_json,
                     train_loss, trained_epochs, training_samples)
                 VALUES ('addr1xxx', 'fpprefix000', 'keystroke_ae',
                         '{\"trainedEpochs\":1}', 0.5, 1, 50)
                 ON CONFLICT(user_address, device_fp_prefix, model_kind) DO UPDATE SET
                     weights_json = excluded.weights_json",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                // Idempotent re-insert.
                "INSERT INTO sentinel_user_models
                    (user_address, device_fp_prefix, model_kind, weights_json,
                     train_loss, trained_epochs, training_samples)
                 VALUES ('addr1xxx', 'fpprefix000', 'keystroke_ae',
                         '{\"trainedEpochs\":2}', 0.4, 2, 60)
                 ON CONFLICT(user_address, device_fp_prefix, model_kind) DO UPDATE SET
                     weights_json = excluded.weights_json,
                     train_loss = excluded.train_loss,
                     trained_epochs = excluded.trained_epochs,
                     training_samples = excluded.training_samples",
                [],
            )
            .unwrap();
        let (epochs, samples): (i64, i64) = db
            .conn()
            .query_row(
                "SELECT trained_epochs, training_samples FROM sentinel_user_models
                 WHERE user_address = 'addr1xxx' AND device_fp_prefix = 'fpprefix000'
                       AND model_kind = 'keystroke_ae'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(epochs, 2);
        assert_eq!(samples, 60);

        // The pre-47 row we inserted must still be there.
        let dao_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM governance_daos WHERE id = 'test-dao'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(dao_count, 1, "pre-47 governance row should survive");
    }
}
