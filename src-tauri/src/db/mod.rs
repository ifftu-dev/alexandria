pub mod schema;
pub mod seed;
pub mod seed_content;

use rusqlite::Connection;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration failed: {0}")]
    Migration(String),
}

/// Wraps a SQLite connection with Alexandria-specific operations.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open (or create) a SQLite database at the given path.
    pub fn open(path: &Path) -> Result<Self, DbError> {
        let conn = Connection::open(path)?;

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
