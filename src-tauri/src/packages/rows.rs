//! The `packages` table.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::domain::package::{ArchiveLocation, Package};

const COLUMNS: &str = "hash, standard, edition, title, description_json, archive_bytes, \
                       location, imported_at, last_opened_at";

/// Records a package unless one with the same hash is already known, and
/// returns the stored row. An existing row wins: a package that has moved
/// into the served store stays there when the same archive is imported again.
pub(super) fn insert(conn: &Connection, package: &Package) -> Result<Package, String> {
    let description = serde_json::to_string(&package.description).map_err(|e| e.to_string())?;
    let archive_bytes =
        i64::try_from(package.archive_bytes).map_err(|_| "archive size exceeds SQLite range")?;
    conn.execute(
        "INSERT INTO packages (hash, standard, edition, title, description_json, archive_bytes, \
                               location, imported_at, last_opened_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT(hash) DO NOTHING",
        params![
            package.hash,
            package.standard.as_str(),
            package.edition.map(|edition| edition.as_str()),
            package.title,
            description,
            archive_bytes,
            package.location.as_str(),
            package.imported_at,
            package.last_opened_at,
        ],
    )
    .map_err(|e| e.to_string())?;
    get(conn, &package.hash)?.ok_or_else(|| format!("package {} vanished", package.hash))
}

pub(super) fn get(conn: &Connection, hash: &str) -> Result<Option<Package>, String> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM packages WHERE hash = ?1"),
        params![hash],
        from_row,
    )
    .optional()
    .map_err(|e| e.to_string())?
    .transpose()
}

pub(super) fn mark_opened(conn: &Connection, hash: &str, now: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE packages SET last_opened_at = ?2 WHERE hash = ?1",
        params![hash, now],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Moves the record to the served store and pins the archive so eviction
/// never removes it.
pub(super) fn record_published(conn: &Connection, hash: &str, size: u64) -> Result<(), String> {
    crate::content_store::storage::upsert_pin(conn, hash, "course", size, false)?;
    conn.execute(
        "UPDATE packages SET location = 'store' WHERE hash = ?1",
        params![hash],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Packages not extracted for a launch since `cutoff` (RFC 3339, UTC).
pub(super) fn idle_since(conn: &Connection, cutoff: &str) -> Result<Vec<String>, String> {
    let mut statement = conn
        .prepare(
            "SELECT hash FROM packages \
             WHERE COALESCE(last_opened_at, imported_at) < ?1 ORDER BY hash",
        )
        .map_err(|e| e.to_string())?;
    let hashes = statement
        .query_map(params![cutoff], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<String>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(hashes)
}

/// A row as SQLite holds it, before the JSON and enum columns are decoded.
struct StoredRow {
    hash: String,
    standard: String,
    edition: Option<String>,
    title: String,
    description: String,
    archive_bytes: i64,
    location: String,
    imported_at: String,
    last_opened_at: Option<String>,
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Result<Package, String>> {
    Ok(decode(StoredRow {
        hash: row.get(0)?,
        standard: row.get(1)?,
        edition: row.get(2)?,
        title: row.get(3)?,
        description: row.get(4)?,
        archive_bytes: row.get(5)?,
        location: row.get(6)?,
        imported_at: row.get(7)?,
        last_opened_at: row.get(8)?,
    }))
}

fn decode(row: StoredRow) -> Result<Package, String> {
    let hash = row.hash;
    let bad = |e: &dyn std::fmt::Display| format!("package {hash}: {e}");
    let standard =
        serde_json::from_value(serde_json::Value::String(row.standard)).map_err(|e| bad(&e))?;
    let edition = row
        .edition
        .map(|edition| serde_json::from_value(serde_json::Value::String(edition)))
        .transpose()
        .map_err(|e| bad(&e))?;
    let description = serde_json::from_str(&row.description).map_err(|e| bad(&e))?;
    let archive_bytes = u64::try_from(row.archive_bytes).map_err(|e| bad(&e))?;
    let location = ArchiveLocation::parse(&row.location)
        .ok_or_else(|| bad(&format!("unknown location {}", row.location)))?;
    Ok(Package {
        hash,
        standard,
        edition,
        title: row.title,
        description,
        archive_bytes,
        location,
        imported_at: row.imported_at,
        last_opened_at: row.last_opened_at,
    })
}
