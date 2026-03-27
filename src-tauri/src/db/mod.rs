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

    /// Open an in-memory database (for tests).
    #[cfg(test)]
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

    /// Migrate an existing unencrypted database to SQLCipher encryption.
    ///
    /// Uses SQLCipher's ATTACH + sqlcipher_export mechanism:
    /// 1. Open the plaintext DB
    /// 2. ATTACH a new encrypted DB
    /// 3. Export all data from plaintext → encrypted
    /// 4. Swap files
    pub fn migrate_to_encrypted(path: &Path, key: &[u8; 32]) -> Result<(), DbError> {
        let enc_path = path.with_extension("db.enc");
        let key_hex = hex::encode(key);

        // Open the plaintext database
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_FULL_MUTEX;
        let conn = Connection::open_with_flags(path, flags)?;

        // Attach an encrypted database and export
        conn.execute_batch(format!(
            "ATTACH DATABASE '{}' AS encrypted KEY \"x'{key_hex}'\";",
            enc_path.display()
        ))?;
        conn.execute_batch("SELECT sqlcipher_export('encrypted');")?;
        conn.execute_batch("DETACH DATABASE encrypted;")?;
        drop(conn);

        // Swap files: encrypted → main, delete plaintext
        let backup_path = path.with_extension("db.bak");
        std::fs::rename(path, &backup_path)?;
        std::fs::rename(&enc_path, path)?;
        std::fs::remove_file(&backup_path).ok(); // best-effort cleanup

        log::info!("database migrated to SQLCipher encryption");
        Ok(())
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
}
