//! The `packages` table.

use rusqlite::{params, Connection, OptionalExtension, Row};

use super::PackageError;
use crate::domain::package::{Package, PackageArchiveLocation};

fn database(message: impl Into<String>) -> PackageError {
    PackageError::Database(message.into())
}

const COLUMNS: &str = "hash, standard, edition, title, description_json, archive_bytes, \
                       location, imported_at, last_opened_at";

/// Records a package unless one with the same hash is already known, and
/// returns the stored row. An existing row wins: a package that has moved
/// into the served store stays there when the same archive is imported again.
pub(super) fn insert(conn: &Connection, package: &Package) -> Result<Package, PackageError> {
    let description =
        serde_json::to_string(&package.description).map_err(|e| database(e.to_string()))?;
    let archive_bytes = i64::try_from(package.archive_bytes)
        .map_err(|_| database("archive size exceeds SQLite range"))?;
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
    )?;
    get(conn, &package.hash)?.ok_or_else(|| database(format!("package {} vanished", package.hash)))
}

pub(super) fn get(conn: &Connection, hash: &str) -> Result<Option<Package>, PackageError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM packages WHERE hash = ?1"),
        params![hash],
        from_row,
    )
    .optional()?
    .transpose()
}

pub(super) fn mark_opened(conn: &Connection, hash: &str, now: &str) -> Result<(), PackageError> {
    conn.execute(
        "UPDATE packages SET last_opened_at = ?2 WHERE hash = ?1",
        params![hash, now],
    )?;
    Ok(())
}

/// Moves the record to the served store and pins the archive so eviction
/// never removes it.
pub(super) fn record_published(
    conn: &Connection,
    hash: &str,
    size: u64,
) -> Result<(), PackageError> {
    crate::content_store::storage::upsert_pin(conn, hash, "course", size, false)
        .map_err(database)?;
    conn.execute(
        "UPDATE packages SET location = 'store' WHERE hash = ?1",
        params![hash],
    )?;
    Ok(())
}

/// Whether the hash is the content of a package element in a published
/// course on this device.
pub(super) fn in_published_course(conn: &Connection, hash: &str) -> Result<bool, PackageError> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM course_elements e \
           JOIN course_chapters ch ON ch.id = e.chapter_id \
           JOIN courses c ON c.id = ch.course_id \
           WHERE e.element_type = 'package' AND e.content_cid = ?1 AND c.status = 'published')",
        params![hash],
        |row| row.get(0),
    )
    .map_err(PackageError::from)
}

/// The distinct package hashes of a course's package elements.
pub(super) fn course_packages(
    conn: &Connection,
    course_id: &str,
) -> Result<Vec<String>, PackageError> {
    let mut statement = conn.prepare(
        "SELECT e.content_cid FROM course_elements e \
             JOIN course_chapters ch ON ch.id = e.chapter_id \
             WHERE ch.course_id = ?1 AND e.element_type = 'package' AND e.content_cid IS NOT NULL \
             GROUP BY e.content_cid ORDER BY MIN(ch.position), MIN(e.position)",
    )?;
    let hashes = statement
        .query_map(params![course_id], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(hashes)
}

/// Pins an archive received with a course as that course's content. Unlike
/// a published archive, eviction may remove it once no active enrolment
/// needs it; it is fetched again when wanted. A pin already held (the
/// publisher's own, never evicted) is left as it is.
pub(super) fn record_received(
    conn: &Connection,
    hash: &str,
    size: u64,
) -> Result<(), PackageError> {
    let held: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM pins WHERE cid = ?1)",
        params![hash],
        |row| row.get(0),
    )?;
    if held {
        crate::content_store::storage::touch_pin(conn, hash).map_err(database)
    } else {
        crate::content_store::storage::upsert_pin(conn, hash, "course", size, true)
            .map_err(database)
    }
}

/// Packages not extracted for a launch since `cutoff` (RFC 3339, UTC).
pub(super) fn idle_since(conn: &Connection, cutoff: &str) -> Result<Vec<String>, PackageError> {
    let mut statement = conn.prepare(
        "SELECT hash FROM packages \
             WHERE COALESCE(last_opened_at, imported_at) < ?1 ORDER BY hash",
    )?;
    let hashes = statement
        .query_map(params![cutoff], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
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

fn from_row(row: &Row<'_>) -> rusqlite::Result<Result<Package, PackageError>> {
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

fn decode(row: StoredRow) -> Result<Package, PackageError> {
    let hash = row.hash;
    let bad = |e: &dyn std::fmt::Display| database(format!("package {hash}: {e}"));
    let standard =
        serde_json::from_value(serde_json::Value::String(row.standard)).map_err(|e| bad(&e))?;
    let edition = row
        .edition
        .map(|edition| serde_json::from_value(serde_json::Value::String(edition)))
        .transpose()
        .map_err(|e| bad(&e))?;
    let description = serde_json::from_str(&row.description).map_err(|e| bad(&e))?;
    let archive_bytes = u64::try_from(row.archive_bytes).map_err(|e| bad(&e))?;
    let location = PackageArchiveLocation::parse(&row.location)
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
