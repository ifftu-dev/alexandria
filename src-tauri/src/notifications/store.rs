// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `notifications` and `notification_dedupe` tables.

use rusqlite::{params, Connection, OptionalExtension};

use super::{NewNotification, NotificationRow};

const DEFAULT_LIMIT: u32 = 100;

/// Insert a notification. `None` when its dedupe key was seen before: the
/// key is claimed first, in the same transaction, so two producers racing on
/// the same observation record it once.
pub fn record(
    conn: &Connection,
    new: &NewNotification,
    now: &str,
) -> Result<Option<NotificationRow>, String> {
    crate::db::with_transaction(conn, || {
        if let Some(key) = &new.dedupe_key {
            conn.execute(
                "INSERT OR IGNORE INTO notification_dedupe (dedupe_key, seen_at) VALUES (?1, ?2)",
                params![key, now],
            )
            .map_err(|e| e.to_string())?;
            if conn.changes() == 0 {
                return Ok(None);
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        let payload_json = new
            .payload
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO notifications \
             (id, kind, title, body, route, source, dedupe_key, payload_json, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                new.kind,
                new.title,
                new.body,
                new.route,
                new.source.as_str(),
                new.dedupe_key,
                payload_json,
                now,
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(Some(NotificationRow {
            id,
            kind: new.kind.clone(),
            title: new.title.clone(),
            body: new.body.clone(),
            route: new.route.clone(),
            source: new.source.as_str().to_string(),
            created_at: now.to_string(),
            read_at: None,
            payload: new.payload.clone(),
        }))
    })
}

/// Newest first.
pub fn list(
    conn: &Connection,
    limit: Option<u32>,
    unread_only: bool,
) -> Result<Vec<NotificationRow>, String> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, 1000);
    let sql = if unread_only {
        "SELECT id, kind, title, body, route, source, created_at, read_at, payload_json \
         FROM notifications WHERE read_at IS NULL ORDER BY created_at DESC, rowid DESC LIMIT ?1"
    } else {
        "SELECT id, kind, title, body, route, source, created_at, read_at, payload_json \
         FROM notifications ORDER BY created_at DESC, rowid DESC LIMIT ?1"
    };
    let mut statement = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = statement
        .query_map(params![limit], |row| {
            let payload_json: Option<String> = row.get(8)?;
            Ok(NotificationRow {
                id: row.get(0)?,
                kind: row.get(1)?,
                title: row.get(2)?,
                body: row.get(3)?,
                route: row.get(4)?,
                source: row.get(5)?,
                created_at: row.get(6)?,
                read_at: row.get(7)?,
                payload: payload_json.and_then(|json| serde_json::from_str(&json).ok()),
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn unread_count(conn: &Connection) -> Result<u32, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM notifications WHERE read_at IS NULL",
        [],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| u32::try_from(count).unwrap_or(u32::MAX))
    .map_err(|e| e.to_string())
}

/// Mark the given notifications read. Already-read rows keep their first
/// `read_at`. Returns how many changed.
pub fn mark_read(conn: &Connection, ids: &[String], now: &str) -> Result<usize, String> {
    let mut changed = 0;
    for id in ids {
        changed += conn
            .execute(
                "UPDATE notifications SET read_at = ?2 WHERE id = ?1 AND read_at IS NULL",
                params![id, now],
            )
            .map_err(|e| e.to_string())?;
    }
    Ok(changed)
}

pub fn mark_all_read(conn: &Connection, now: &str) -> Result<usize, String> {
    conn.execute(
        "UPDATE notifications SET read_at = ?1 WHERE read_at IS NULL",
        params![now],
    )
    .map_err(|e| e.to_string())
}

/// Delete every notification. The dedupe keys stay: what was already
/// reported stays reported.
pub fn clear(conn: &Connection) -> Result<usize, String> {
    conn.execute("DELETE FROM notifications", [])
        .map_err(|e| e.to_string())
}

/// Whether a dedupe key has been claimed, for producers that want to skip
/// work (a network round trip, say) rather than record and be ignored.
pub fn seen(conn: &Connection, dedupe_key: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT 1 FROM notification_dedupe WHERE dedupe_key = ?1",
        params![dedupe_key],
        |_| Ok(()),
    )
    .optional()
    .map(|found| found.is_some())
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::notifications::{kind, Source};

    fn db() -> Database {
        let db = Database::open_in_memory().expect("in-memory database");
        db.run_migrations().expect("migrations");
        db
    }

    fn sample() -> NewNotification {
        NewNotification::new(
            kind::OFFER_SENT,
            "New offer",
            "An organisation sent you an offer.",
        )
        .route("/settings/organisations")
        .dedupe("offer_sent:abc")
        .payload(serde_json::json!({ "id": "abc" }))
    }

    #[test]
    fn records_lists_and_counts() {
        let db = db();
        let row = record(db.conn(), &sample(), "2026-10-20T10:00:00Z")
            .unwrap()
            .expect("recorded");
        assert_eq!(row.kind, kind::OFFER_SENT);
        assert_eq!(row.source, Source::Local.as_str());
        assert_eq!(row.payload, Some(serde_json::json!({ "id": "abc" })));

        let listed = list(db.conn(), None, false).unwrap();
        assert_eq!(listed, vec![row.clone()]);
        assert_eq!(unread_count(db.conn()).unwrap(), 1);
        assert!(seen(db.conn(), "offer_sent:abc").unwrap());
        assert!(!seen(db.conn(), "offer_sent:other").unwrap());
    }

    #[test]
    fn a_repeated_dedupe_key_is_dropped_even_after_clearing() {
        let db = db();
        assert!(record(db.conn(), &sample(), "t1").unwrap().is_some());
        assert!(record(db.conn(), &sample(), "t2").unwrap().is_none());
        assert_eq!(clear(db.conn()).unwrap(), 1);
        assert!(list(db.conn(), None, false).unwrap().is_empty());
        // Still remembered: the learner cleared the list, not the fact.
        assert!(record(db.conn(), &sample(), "t3").unwrap().is_none());
        // Without a key, every record is new.
        let mut keyless = sample();
        keyless.dedupe_key = None;
        assert!(record(db.conn(), &keyless, "t4").unwrap().is_some());
        assert!(record(db.conn(), &keyless, "t5").unwrap().is_some());
    }

    #[test]
    fn marking_read_keeps_the_first_timestamp() {
        let db = db();
        let first = record(db.conn(), &sample(), "t1").unwrap().unwrap();
        let mut second_new = sample();
        second_new.dedupe_key = Some("offer_sent:def".into());
        let second = record(db.conn(), &second_new, "t2").unwrap().unwrap();

        let first_only = std::slice::from_ref(&first.id);
        assert_eq!(mark_read(db.conn(), first_only, "r1").unwrap(), 1);
        assert_eq!(mark_read(db.conn(), first_only, "r2").unwrap(), 0);
        let unread = list(db.conn(), None, true).unwrap();
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].id, second.id);
        assert_eq!(unread_count(db.conn()).unwrap(), 1);

        assert_eq!(mark_all_read(db.conn(), "r3").unwrap(), 1);
        assert_eq!(unread_count(db.conn()).unwrap(), 0);
        let all = list(db.conn(), None, false).unwrap();
        // Newest first, and the first row keeps `r1`.
        assert_eq!(all[0].id, second.id);
        assert_eq!(all[1].read_at.as_deref(), Some("r1"));
    }

    #[test]
    fn list_is_newest_first_and_bounded() {
        let db = db();
        for i in 0..5 {
            let mut n = sample();
            n.dedupe_key = Some(format!("k{i}"));
            record(db.conn(), &n, &format!("2026-10-20T10:00:0{i}Z")).unwrap();
        }
        let two = list(db.conn(), Some(2), false).unwrap();
        assert_eq!(two.len(), 2);
        assert_eq!(two[0].created_at, "2026-10-20T10:00:04Z");
    }
}
