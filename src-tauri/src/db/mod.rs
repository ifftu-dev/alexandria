pub mod schema;
#[cfg(feature = "dev-seed")]
pub mod seed;
#[cfg(feature = "dev-seed")]
pub mod seed_content;

use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use thiserror::Error;

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

// SAFETY: Database is always accessed behind a tokio::sync::Mutex which
// guarantees exclusive access. rusqlite::Connection is Send but not Sync;
// the Mutex provides the synchronization, so sharing the Database across
// threads is safe. Note: RwLock cannot be used here because
// Connection::prepare() mutably borrows an internal RefCell, meaning
// even concurrent readers will panic.
unsafe impl Send for Database {}
unsafe impl Sync for Database {}

impl Database {
    /// Open (or create) a SQLite database at the given path.
    ///
    /// Uses SQLITE_OPEN_FULL_MUTEX (serialized mode) so that SQLite
    /// internally serializes all operations. This is a safety net for
    /// the `unsafe impl Sync` on Database — even if two tokio tasks
    /// somehow call into SQLite concurrently, SQLite's own mutex will
    /// prevent memory corruption.
    pub fn open(path: &Path) -> Result<Self, DbError> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_FULL_MUTEX;
        let conn = Connection::open_with_flags(path, flags)?;

        // Enable WAL mode for better concurrent read performance.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // Enable foreign keys.
        conn.pragma_update(None, "foreign_keys", "ON")?;

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
        let key_hex = hex::encode(key);
        conn.pragma_update(None, "key", format!("x'{key_hex}'"))?;

        // Enable WAL mode for better concurrent read performance.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // Enable foreign keys.
        conn.pragma_update(None, "foreign_keys", "ON")?;

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
        // Create the migrations tracking table.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _migrations (
                version  INTEGER PRIMARY KEY,
                name     TEXT NOT NULL,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )?;

        let current_version: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM _migrations",
            [],
            |row| row.get(0),
        )?;

        for (version, name, sql) in schema::MIGRATIONS {
            if *version > current_version {
                log::info!("Running migration {}: {}", version, name);
                self.conn.execute_batch(sql).map_err(|e| {
                    DbError::Migration(format!("migration {} ({}) failed: {}", version, name, e))
                })?;
                self.conn.execute(
                    "INSERT INTO _migrations (version, name) VALUES (?1, ?2)",
                    rusqlite::params![version, name],
                )?;
            }
        }

        Ok(())
    }

    /// Get a reference to the underlying connection.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
