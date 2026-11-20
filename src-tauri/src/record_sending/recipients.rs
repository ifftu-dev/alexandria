//! The recipients the learner approved, and the identifier each one sees.

use alexandria_studio::xapi::{Account, Recipient, StatementError};
use rusqlite::{params, Connection, OptionalExtension, Row};

use super::{Identity, RecipientEntry, RecipientKind, Scope, SendError};

/// The statement recipient for `identity`: the actor it names and the key
/// its statement and registration identifiers come from, both derived from
/// the learner's key and `label`. `learner_secret` is the learner's Ed25519
/// signing key seed.
pub fn recipient(
    identity: &Identity,
    learner_secret: &[u8; 32],
    learner_did: &str,
    label: &str,
) -> Result<Recipient, StatementError> {
    match identity {
        Identity::Pairwise => Recipient::pairwise(learner_secret, label),
        Identity::RealDid => Recipient::real_did(learner_secret, learner_did, label),
        Identity::Proposed(account) => Recipient::proposed(learner_secret, account.clone(), label),
    }
}

impl RecipientEntry {
    /// The statement recipient this entry's records are built for.
    pub fn recipient(
        &self,
        learner_secret: &[u8; 32],
        learner_did: &str,
    ) -> Result<Recipient, StatementError> {
        recipient(&self.identity, learner_secret, learner_did, &self.label)
    }
}

/// Stores an approved recipient. A label already in use is refused: two
/// rows would share every identifier.
pub fn insert(conn: &Connection, entry: &RecipientEntry) -> Result<(), SendError> {
    entry.scope.validate()?;
    let (home_page, name) = match &entry.identity {
        Identity::Proposed(Account { home_page, name }) => (Some(home_page), Some(name)),
        _ => (None, None),
    };
    let taken: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM record_recipients WHERE label = ?1 OR id = ?2)",
        params![entry.label, entry.id],
        |row| row.get(0),
    )?;
    if taken {
        return Err(SendError::AlreadyAdded(
            "this recipient has already been approved".into(),
        ));
    }
    conn.execute(
        "INSERT INTO record_recipients \
         (id, kind, label, identifier, proposed_home_page, proposed_name, actor_name, \
          course_ids, include_interactions, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            entry.id,
            entry.kind.as_str(),
            entry.label,
            entry.identity.as_str(),
            home_page,
            name,
            entry.actor_name,
            serde_json::to_string(&entry.scope.course_ids)?,
            entry.scope.include_interactions,
            entry.created_at,
        ],
    )?;
    Ok(())
}

const COLUMNS: &str = "id, kind, label, identifier, proposed_home_page, proposed_name, \
                       actor_name, course_ids, include_interactions, created_at";

pub fn get(conn: &Connection, id: &str) -> Result<Option<RecipientEntry>, SendError> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM record_recipients WHERE id = ?1"),
        params![id],
        from_row,
    )
    .optional()?
    .transpose()
}

/// Every recipient of `kind`, oldest first.
pub fn list(conn: &Connection, kind: RecipientKind) -> Result<Vec<RecipientEntry>, SendError> {
    let mut statement = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM record_recipients WHERE kind = ?1 ORDER BY created_at, id"
    ))?;
    let rows = statement.query_map(params![kind.as_str()], from_row)?;
    rows.map(|row| row?).collect()
}

/// Removes a recipient and everything queued for it, so nothing more is
/// sent. Its access log rows stay. Returns whether it existed.
pub fn delete(conn: &Connection, id: &str) -> Result<bool, SendError> {
    conn.execute(
        "DELETE FROM learning_record_outbox WHERE recipient_id = ?1",
        params![id],
    )?;
    Ok(conn.execute("DELETE FROM record_recipients WHERE id = ?1", params![id])? > 0)
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Result<RecipientEntry, SendError>> {
    let kind: String = row.get(1)?;
    let identifier: String = row.get(3)?;
    let home_page: Option<String> = row.get(4)?;
    let name: Option<String> = row.get(5)?;
    let course_ids: String = row.get(7)?;
    let id: String = row.get(0)?;
    let identity = match (identifier.as_str(), home_page, name) {
        ("pairwise", _, _) => Identity::Pairwise,
        ("real_did", _, _) => Identity::RealDid,
        ("proposed", Some(home_page), Some(name)) => {
            Identity::Proposed(Account { home_page, name })
        }
        (other, _, _) => {
            return Ok(Err(SendError::Database(format!(
                "recipient {id}: unusable identifier {other}"
            ))))
        }
    };
    let kind = match RecipientKind::parse(&kind) {
        Ok(kind) => kind,
        Err(error) => return Ok(Err(error)),
    };
    let course_ids = match serde_json::from_str(&course_ids) {
        Ok(ids) => ids,
        Err(error) => return Ok(Err(error.into())),
    };
    Ok(Ok(RecipientEntry {
        id,
        kind,
        label: row.get(2)?,
        identity,
        actor_name: row.get(6)?,
        scope: Scope {
            course_ids,
            include_interactions: row.get(8)?,
        },
        created_at: row.get(9)?,
    }))
}
