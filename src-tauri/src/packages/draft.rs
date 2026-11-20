//! The draft course a package file opened from disk lives in.

use rusqlite::{params, Connection, OptionalExtension};

use crate::crypto::hash::entity_id;
use crate::domain::course_document::PACKAGE_ELEMENT_TYPE;
use crate::domain::package::Package;

/// Marks a course made by opening a package file, in `courses.provenance`.
pub(super) const PACKAGE_FILE_PROVENANCE: &str = "package_file";

pub(super) struct DraftIds {
    pub course_id: String,
    pub chapter_id: String,
    pub element_id: String,
}

/// The local author's draft course for this package file, made on first
/// open: one chapter holding one package element, both titled after the
/// package. A draft is never in the catalog and never announced.
pub(super) fn find_or_create(conn: &Connection, package: &Package) -> Result<DraftIds, String> {
    crate::db::with_transaction(conn, || {
        let author: String = conn
            .query_row(
                "SELECT stake_address FROM local_identity WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("no identity found — generate a wallet first: {e}"))?;

        let existing = conn
            .query_row(
                "SELECT c.id, ch.id, e.id FROM course_elements e \
                 JOIN course_chapters ch ON ch.id = e.chapter_id \
                 JOIN courses c ON c.id = ch.course_id \
                 WHERE e.element_type = ?1 AND e.content_cid = ?2 \
                   AND c.status = 'draft' AND c.provenance = ?3 AND c.author_address = ?4 \
                 ORDER BY c.created_at, c.id LIMIT 1",
                params![
                    PACKAGE_ELEMENT_TYPE,
                    package.hash,
                    PACKAGE_FILE_PROVENANCE,
                    author
                ],
                |row| {
                    Ok(DraftIds {
                        course_id: row.get(0)?,
                        chapter_id: row.get(1)?,
                        element_id: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(ids) = existing {
            return Ok(ids);
        }

        let course_id = entity_id(&[
            &author,
            PACKAGE_ELEMENT_TYPE,
            &package.hash,
            &chrono::Utc::now().to_rfc3339(),
        ]);
        conn.execute(
            "INSERT INTO courses (id, title, author_address, status, provenance) \
             VALUES (?1, ?2, ?3, 'draft', ?4)",
            params![course_id, package.title, author, PACKAGE_FILE_PROVENANCE],
        )
        .map_err(|e| e.to_string())?;

        let chapter_id = entity_id(&[&course_id, &package.title, "0"]);
        conn.execute(
            "INSERT INTO course_chapters (id, course_id, title, position) VALUES (?1, ?2, ?3, 0)",
            params![chapter_id, course_id, package.title],
        )
        .map_err(|e| e.to_string())?;

        let element_id = entity_id(&[&chapter_id, &package.title, PACKAGE_ELEMENT_TYPE, "0"]);
        conn.execute(
            "INSERT INTO course_elements (id, chapter_id, title, element_type, content_cid, position) \
             VALUES (?1, ?2, ?3, ?4, ?5, 0)",
            params![
                element_id,
                chapter_id,
                package.title,
                PACKAGE_ELEMENT_TYPE,
                package.hash
            ],
        )
        .map_err(|e| e.to_string())?;

        Ok(DraftIds {
            course_id,
            chapter_id,
            element_id,
        })
    })
}
