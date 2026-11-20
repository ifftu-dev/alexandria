//! The activity log's size limit.
//!
//! Past [`LIMIT_BYTES`] the oldest interaction-level records go first. If the
//! log is still over, completion-level records are kept as one summary per
//! attempt at each activity: for each registration (an enrolment or a
//! package attempt) and activity, the record with the strongest result
//! (pass or fail, then completion or score, then progress, then launch or
//! enrolment), newest first among equals. A record still waiting in an
//! outbox is never removed. If the log is over its limit after that, it stays
//! over: the summaries are the learner's history and are not dropped
//! silently.

use std::collections::BTreeMap;

use rusqlite::{params, Connection};

use super::RecordError;
use crate::domain::learning_record::LearningRecordUsage;

/// The activity log's size limit: the total size of the stored events.
pub const LIMIT_BYTES: u64 = 16 * 1024 * 1024;

/// The log's size, its limit and how many records it holds.
pub fn usage(conn: &Connection) -> Result<LearningRecordUsage, RecordError> {
    let (bytes, records, interaction_records): (i64, i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(bytes), 0), COUNT(*), \
                COALESCE(SUM(kind = 'interaction'), 0) FROM learning_records",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    Ok(LearningRecordUsage {
        bytes: u64::try_from(bytes).unwrap_or(0),
        limit_bytes: LIMIT_BYTES,
        records: u64::try_from(records).unwrap_or(0),
        interaction_records: u64::try_from(interaction_records).unwrap_or(0),
    })
}

/// Brings the log within `limit_bytes` by the rule in the module header.
/// Returns how many records were removed.
pub fn enforce_limit(conn: &Connection, limit_bytes: u64) -> Result<usize, RecordError> {
    let mut total = usage(conn)?.bytes;
    if total <= limit_bytes {
        return Ok(0);
    }
    let mut removed = 0;

    // Oldest interaction-level records first.
    let interactions: Vec<(i64, i64)> = {
        let mut statement = conn.prepare(&format!(
            "SELECT seq, bytes FROM learning_records \
             WHERE kind = 'interaction' AND {REMOVABLE} ORDER BY seq"
        ))?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    for (seq, bytes) in interactions {
        if total <= limit_bytes {
            return Ok(removed);
        }
        conn.execute("DELETE FROM learning_records WHERE seq = ?1", params![seq])?;
        total = total.saturating_sub(u64::try_from(bytes).unwrap_or(0));
        removed += 1;
    }
    if total <= limit_bytes {
        return Ok(removed);
    }

    // Then one summary per attempt at each activity.
    let completions: Vec<(i64, String, String, String, bool)> = {
        let mut statement = conn.prepare(&format!(
            "SELECT seq, COALESCE(registration, ''), activity_id, verb, {REMOVABLE} \
             FROM learning_records WHERE kind = 'completion' ORDER BY seq"
        ))?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?;
        rows.collect::<Result<_, _>>()?
    };
    let mut groups: BTreeMap<(String, String), Vec<(i64, String, bool)>> = BTreeMap::new();
    for (seq, registration, activity, verb, removable) in completions {
        groups
            .entry((registration, activity))
            .or_default()
            .push((seq, verb, removable));
    }
    for records in groups.into_values().filter(|records| records.len() > 1) {
        let kept = records
            .iter()
            .max_by_key(|(seq, verb, _)| (rank(verb), *seq))
            .map(|(seq, _, _)| *seq)
            .expect("a group is never empty");
        for (seq, _, removable) in &records {
            if *seq != kept && *removable {
                removed +=
                    conn.execute("DELETE FROM learning_records WHERE seq = ?1", params![seq])?;
            }
        }
        conn.execute(
            "UPDATE learning_records SET summary = 1 WHERE seq = ?1",
            params![kept],
        )?;
    }
    Ok(removed)
}

/// A record not waiting in any outbox.
const REMOVABLE: &str = "NOT EXISTS (SELECT 1 FROM learning_record_outbox o \
                         WHERE o.record_id = learning_records.id AND o.state <> 'sent')";

/// How much a record says about an attempt: the summary keeps the highest.
fn rank(verb: &str) -> u8 {
    match verb {
        "passed" | "failed" => 3,
        "completed" | "scored" => 2,
        "progressed" => 1,
        _ => 0,
    }
}
