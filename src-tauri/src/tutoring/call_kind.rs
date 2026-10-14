//! What a call is for.
//!
//! Every call rides the same engine, but a tutoring session, a classroom
//! call and an interview are different things to the people in them: who may
//! join, whether there is an invite to share, where leaving takes you. The
//! kind is recorded with the call when it is created or joined, and the rest
//! of the app reads it from there.
//!
//! What this does NOT do is keep anyone out. A ticket is still a bearer pass:
//! a device that has never seen the call can join it as plain tutoring. The
//! check below only stops a device from re-entering a call it already knows
//! under a different kind. Admission — publishers refusing peers that are not
//! entitled to the call — is separate work.
use live::rooms::RoomTicket;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallKind {
    /// A standalone tutoring session; anyone with the ticket may join.
    Tutoring,
    /// A call inside a classroom, for its members.
    Classroom,
    /// A panelist's seat in an interview held in Alexandria.
    InterviewPanel,
    /// The candidate's side of an interview held in Alexandria.
    InterviewCandidate,
    /// The in-person interview assistant on a single device.
    InterviewAssistant,
    /// A live experience hosted by a plugin.
    Plugin,
}

impl CallKind {
    pub const ALL: [CallKind; 6] = [
        CallKind::Tutoring,
        CallKind::Classroom,
        CallKind::InterviewPanel,
        CallKind::InterviewCandidate,
        CallKind::InterviewAssistant,
        CallKind::Plugin,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            CallKind::Tutoring => "tutoring",
            CallKind::Classroom => "classroom",
            CallKind::InterviewPanel => "interview_panel",
            CallKind::InterviewCandidate => "interview_candidate",
            CallKind::InterviewAssistant => "interview_assistant",
            CallKind::Plugin => "plugin",
        }
    }

    /// The kind a command was asked for. Absent means tutoring, which is what
    /// every call was before kinds existed.
    pub fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.map(str::trim).filter(|v| !v.is_empty()) {
            None => Ok(CallKind::Tutoring),
            Some(v) => CallKind::ALL
                .into_iter()
                .find(|k| k.as_str() == v)
                .ok_or_else(|| format!("unknown call kind: {v}")),
        }
    }

    /// Whether the call offers an invite link or QR code to share. Only open
    /// tutoring does: every other kind decides who joins somewhere else.
    pub fn shares_invites(self) -> bool {
        self == CallKind::Tutoring
    }
}

/// The room a ticket opens, as stored in `tutoring_sessions.room_id`.
pub(crate) fn room_of(ticket: &str) -> Result<String, String> {
    let parsed: RoomTicket = ticket
        .parse()
        .map_err(|e| format!("invalid room ticket: {e}"))?;
    Ok(hex::encode(parsed.topic_id.as_bytes()))
}

/// Refuse to join a room this device already knows under another kind, so a
/// classroom or interview ticket cannot be reopened as plain tutoring here.
pub(crate) fn check_join_kind(
    conn: &Connection,
    ticket: &str,
    kind: CallKind,
) -> Result<(), String> {
    let room = room_of(ticket)?;
    let known: Option<String> = conn
        .query_row(
            "SELECT kind FROM tutoring_sessions WHERE room_id = ?1 AND kind <> ?2
             ORDER BY created_at DESC LIMIT 1",
            params![room, kind.as_str()],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(other) = known {
        return Err(refusal(&other));
    }
    if kind != CallKind::Classroom {
        let mut stmt = conn
            .prepare("SELECT ticket FROM classroom_calls WHERE ticket IS NOT NULL")
            .map_err(|e| e.to_string())?;
        let tickets = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        for classroom_ticket in tickets {
            let classroom_ticket = classroom_ticket.map_err(|e| e.to_string())?;
            if room_of(&classroom_ticket).ok().as_deref() == Some(room.as_str()) {
                return Err(refusal(CallKind::Classroom.as_str()));
            }
        }
    }
    Ok(())
}

fn refusal(kind: &str) -> String {
    let what = match kind {
        "classroom" => "a classroom call; join it from the classroom",
        "interview_panel" | "interview_candidate" | "interview_assistant" => {
            "an interview; join it from the interview"
        }
        "plugin" => "a call inside a course element; join it from there",
        _ => "a different kind of call",
    };
    format!("CALL_KIND_MISMATCH: This room is {what}.")
}

/// Record a call this device created or joined.
pub(crate) fn record_session(
    conn: &Connection,
    id: &str,
    title: &str,
    ticket: &str,
    kind: CallKind,
    context_id: Option<&str>,
) -> Result<(), String> {
    // Rejoining keeps the same id (a classroom call is recorded under its
    // call id), so a second join reopens the row rather than failing.
    conn.execute(
        "INSERT INTO tutoring_sessions (id, title, ticket, status, kind, context_id)
         VALUES (?1, ?2, ?3, 'active', ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET title = excluded.title, ticket = excluded.ticket,
             status = 'active', ended_at = NULL, kind = excluded.kind,
             context_id = excluded.context_id",
        params![id, title, ticket, kind.as_str(), context_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_round_trip_and_default_to_tutoring() {
        for kind in CallKind::ALL {
            assert_eq!(CallKind::parse(Some(kind.as_str())).unwrap(), kind);
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{}\"", kind.as_str()));
        }
        assert_eq!(CallKind::parse(None).unwrap(), CallKind::Tutoring);
        assert_eq!(CallKind::parse(Some("  ")).unwrap(), CallKind::Tutoring);
        assert!(CallKind::parse(Some("party")).is_err());
    }

    #[test]
    fn only_tutoring_shares_invites() {
        for kind in CallKind::ALL {
            assert_eq!(
                kind.shares_invites(),
                kind == CallKind::Tutoring,
                "{kind:?}"
            );
        }
    }

    fn db() -> crate::db::Database {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db
    }

    #[test]
    fn a_known_room_cannot_be_rejoined_as_another_kind() {
        let db = db();
        let conn = db.conn();
        let ticket = RoomTicket::generate().to_string();
        record_session(
            conn,
            "s1",
            "Interview",
            &ticket,
            CallKind::InterviewCandidate,
            Some("int-1"),
        )
        .unwrap();
        conn.execute(
            "UPDATE tutoring_sessions SET room_id = ?2 WHERE id = ?1",
            params!["s1", room_of(&ticket).unwrap()],
        )
        .unwrap();

        let refused = check_join_kind(conn, &ticket, CallKind::Tutoring).unwrap_err();
        assert!(refused.starts_with("CALL_KIND_MISMATCH"), "{refused}");
        assert!(refused.contains("interview"), "{refused}");
        // The same kind may rejoin its own room.
        check_join_kind(conn, &ticket, CallKind::InterviewCandidate).unwrap();
        // An unknown room is not this check's business.
        check_join_kind(
            conn,
            &RoomTicket::generate().to_string(),
            CallKind::Tutoring,
        )
        .unwrap();
    }

    #[test]
    fn a_classroom_ticket_cannot_be_joined_as_tutoring() {
        let db = db();
        let conn = db.conn();
        conn.execute(
            "INSERT INTO classrooms (id, name, owner_address) VALUES ('c1', 'Algebra', 'addr')",
            [],
        )
        .unwrap();
        let ticket = RoomTicket::generate().to_string();
        conn.execute(
            "INSERT INTO classroom_calls (id, classroom_id, title, ticket, started_by)
             VALUES ('k1', 'c1', 'Call', ?1, 'addr')",
            params![ticket],
        )
        .unwrap();
        let refused = check_join_kind(conn, &ticket, CallKind::Tutoring).unwrap_err();
        assert!(refused.contains("classroom"), "{refused}");
        check_join_kind(conn, &ticket, CallKind::Classroom).unwrap();
    }
}
