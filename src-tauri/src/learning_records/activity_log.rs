//! What the activity log view does with the records: list them a page at a
//! time, export them signed, and delete them.
//!
//! - **Export.** Every record is built for the learner's own log
//!   ([`super::local_recipient`]: the learner's DID as the actor, names
//!   disclosed), oldest first, into a JSON array of xAPI statements. The
//!   learner's key signs the exact bytes of that file, detached, in the
//!   format [`alexandria_verify::record_export`] defines and verifies.
//! - **Delete.** Deleting is local: the record leaves this device's log.
//!   A deleted record that was waiting in an outbox is no longer sent (its
//!   outbox rows go with it); a record a recipient already received stays
//!   with that recipient.

use alexandria_studio::xapi::{Action, BuildOptions, LearningEvent};
use alexandria_verify::record_export;
use ed25519_dalek::SigningKey;
use rusqlite::{params, Connection};

use super::{list, local_recipient, statement, RecordError};
use crate::domain::learning_record::LearningRecordEntry;

/// The most rows one [`page`] returns.
pub const MAX_PAGE: u32 = 200;

/// `limit` rows of the log from `offset`, newest first, with the titles of
/// what each record is about as this device knows them.
pub fn page(
    conn: &Connection,
    offset: u32,
    limit: u32,
) -> Result<Vec<LearningRecordEntry>, RecordError> {
    let mut statement = conn.prepare(
        "SELECT r.id, r.kind, r.verb, r.course_id, c.title, r.element_id, e.title, \
                r.activity_id, r.summary, r.bytes, r.recorded_at, r.event_json \
         FROM learning_records r \
         LEFT JOIN courses c ON c.id = r.course_id \
         LEFT JOIN course_elements e ON e.id = r.element_id \
         ORDER BY r.seq DESC LIMIT ?1 OFFSET ?2",
    )?;
    let rows = statement.query_map(params![limit.min(MAX_PAGE), offset], |row| {
        let bytes: i64 = row.get(9)?;
        let event_json: String = row.get(11)?;
        Ok((
            LearningRecordEntry {
                id: row.get(0)?,
                kind: row.get(1)?,
                verb: row.get(2)?,
                course_id: row.get(3)?,
                course_title: row.get(4)?,
                element_id: row.get(5)?,
                element_title: row.get(6)?,
                activity_id: row.get(7)?,
                summary: row.get(8)?,
                bytes: u64::try_from(bytes).unwrap_or(0),
                recorded_at: row.get(10)?,
                percent: None,
                score: None,
            },
            event_json,
        ))
    })?;
    rows.map(|row| {
        let (mut entry, event_json) = row?;
        let event: LearningEvent = serde_json::from_str(&event_json)?;
        (entry.percent, entry.score) = detail(&event.action);
        Ok(entry)
    })
    .collect()
}

/// The course progress and the scaled score a record carries.
fn detail(action: &Action) -> (Option<u8>, Option<f64>) {
    match action {
        Action::Progressed { percent, .. } => (Some(*percent), None),
        Action::Completed { outcome, .. }
        | Action::Passed { outcome, .. }
        | Action::Failed { outcome, .. }
        | Action::Scored { outcome, .. }
        | Action::Responded { outcome, .. } => {
            (None, outcome.score.as_ref().and_then(|s| s.scaled))
        }
        Action::Registered { .. }
        | Action::Launched { .. }
        | Action::Voided { .. }
        | Action::Abandoned { .. }
        | Action::Satisfied { .. } => (None, None),
    }
}

/// A signed export: the statement file and its detached signature file,
/// byte for byte as they are written to disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    pub statements: Vec<u8>,
    pub signature: Vec<u8>,
}

/// Builds every record into one statement file, oldest first, and signs it
/// with the learner's key.
pub fn export(conn: &Connection, key: &SigningKey) -> Result<Export, RecordError> {
    let secret = key.to_bytes();
    let mut records = list(conn)?;
    records.reverse();
    let options = BuildOptions::default();
    let statements = records
        .iter()
        .map(|record| {
            let recipient = local_recipient(&secret, &record.learner_did)?;
            statement(record, &recipient, &options)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut file = serde_json::to_vec_pretty(&statements)?;
    file.push(b'\n');
    let signature = record_export::signature_file(&record_export::sign(&file, key));
    Ok(Export {
        statements: file,
        signature,
    })
}

/// Deletes the records with these local ids, and with them their outbox
/// rows. Unknown ids are ignored. Returns how many records were removed.
pub fn delete(conn: &Connection, ids: &[String]) -> Result<usize, RecordError> {
    let tx = conn.unchecked_transaction()?;
    let mut removed = 0;
    {
        let mut statement = tx.prepare("DELETE FROM learning_records WHERE id = ?1")?;
        for id in ids {
            removed += statement.execute([id])?;
        }
    }
    tx.commit()?;
    Ok(removed)
}

/// Deletes every record in the log, and with them the whole outbox.
pub fn delete_all(conn: &Connection) -> Result<usize, RecordError> {
    Ok(conn.execute("DELETE FROM learning_records", [])?)
}
