//! AI access: who may use which model, when, and which lesson text a model
//! may see. Chat, the lesson tutor, Studio runs, provider sign-in, chat
//! actions and course notes all ask here.
//!
//! * [`may_use`]: may the profile's holder send to this connection now. The
//!   age band is computed from the local identity's birthdate at every call,
//!   so a corrected birthdate applies at the next send. Under 13: models on
//!   the device or the learner's own network only, because providers expect
//!   zero data retention or COPPA-grade handling for children's data, which
//!   Alexandria cannot verify on a user's key. 13 to 17: never a vendor whose
//!   terms require users to be 18 or older, and every other remote endpoint
//!   needs a linked guardian's approval for that connection at that endpoint.
//! * [`may_connect`]: the part of those rules no guardian approval can lift,
//!   for setting a connection up (provider sign-in, asking for approval).
//! * [`visible_lesson`]: which lesson a model may see (text lessons only).
//! * Guardian approvals: [`request_approval`], [`approvals`],
//!   [`pending_approvals`] for the guardian sync, and [`record_decision`]
//!   when a guardian's decision arrives. Guardians see a connection's name,
//!   endpoint and model, never chat content.
//!
//! Every refusal is a typed [`Refused`]; chat shows its string code, the
//! tutor and Studio runs convert it to their error once.

use alexandria_ai::endpoint::Location;
use alexandria_studio::model::StudioConnection;
use alexandria_studio::{provider as studio_provider, store as studio_store};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

pub use alexandria_studio::access::{visible_lesson, Reader, Refused};

/// How vendor terms and guardian rules see the profile's holder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgeBand {
    Adult,
    /// 13 to 17.
    Teen,
    /// Under 13.
    Child,
}

fn age_band(conn: &Connection) -> Result<AgeBand, Refused> {
    let birthdate: Option<String> = conn
        .query_row(
            "SELECT birthdate FROM local_identity WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| Refused::Storage("profile"))?
        .flatten();
    let today = chrono::Utc::now().date_naive();
    Ok(match birthdate.as_deref() {
        Some(b) if crate::domain::identity::is_younger_than(b, 13, today) => AgeBand::Child,
        Some(b) if crate::domain::identity::is_minor(b, today) => AgeBand::Teen,
        _ => AgeBand::Adult,
    })
}

fn remote(connection: &StudioConnection) -> Result<bool, Refused> {
    Ok(studio_provider::location(connection)? == Location::Remote)
}

/// May the profile's holder send to `connection` now: it is enabled and
/// answers text, and the age rules allow it (see the module docs).
pub fn may_use(conn: &Connection, connection: &StudioConnection) -> Result<(), Refused> {
    if !connection.enabled {
        return Err(Refused::ConnectionDisabled);
    }
    if connection.capability != "text" {
        return Err(Refused::WrongCapability);
    }
    if !remote(connection)? {
        return Ok(());
    }
    let band = age_band(conn)?;
    if band == AgeBand::Adult {
        return Ok(());
    }
    refuse_for_minors(&connection.endpoint, band)?;
    match latest_decision(conn, &connection.id, &connection.endpoint)?.as_deref() {
        Some("approved") => Ok(()),
        Some("pending") => Err(Refused::GuardianPending),
        Some(_) => Err(Refused::GuardianDeclined),
        None => Err(Refused::GuardianRequired),
    }
}

/// May the profile's holder set `connection` up at all: the refusals no
/// guardian approval can lift. A teen may connect a remote model that is not
/// adults-only and then needs a guardian's approval to use it.
pub fn may_connect(conn: &Connection, connection: &StudioConnection) -> Result<(), Refused> {
    if !remote(connection)? {
        return Ok(());
    }
    match age_band(conn)? {
        AgeBand::Adult => Ok(()),
        band => refuse_for_minors(&connection.endpoint, band),
    }
}

fn refuse_for_minors(endpoint: &str, band: AgeBand) -> Result<(), Refused> {
    if band == AgeBand::Child {
        return Err(Refused::TooYoungForRemote);
    }
    let url = alexandria_ai::endpoint::parse(endpoint, Location::Remote)
        .map_err(|e| Refused::Invalid(e.to_string()))?;
    match alexandria_ai::terms::adults_only(&url) {
        Some(vendor) => Err(Refused::AdultsOnly(vendor)),
        None => Ok(()),
    }
}

// ─── guardian approvals ──────────────────────────────────────────────────

/// A request for a guardian's approval of a remote connection, as the
/// learner sees it.
#[derive(Debug, Clone, Serialize)]
pub struct GuardianApproval {
    pub id: String,
    pub connection_id: String,
    pub connection_name: String,
    pub endpoint: String,
    pub model: String,
    pub status: String,
    pub requested_at: String,
    pub decided_at: Option<String>,
}

/// An open request, as the guardian sync sends it: what the guardian sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingApproval {
    pub id: String,
    pub connection_name: String,
    pub endpoint: String,
    pub model: String,
}

/// The latest guardian decision for a connection at this endpoint:
/// `pending`, `approved` or `declined`.
fn latest_decision(
    conn: &Connection,
    connection_id: &str,
    endpoint: &str,
) -> Result<Option<String>, Refused> {
    conn.query_row(
        "SELECT status FROM ai_connection_approvals
          WHERE connection_id = ?1 AND endpoint = ?2
          ORDER BY requested_at DESC, rowid DESC LIMIT 1",
        [connection_id, endpoint],
        |r| r.get(0),
    )
    .optional()
    .map_err(|_| Refused::Storage("approvals"))
}

/// Every approval request, newest first.
pub fn approvals(conn: &Connection) -> Result<Vec<GuardianApproval>, Refused> {
    let read = || -> rusqlite::Result<Vec<GuardianApproval>> {
        let mut stmt = conn.prepare(
            "SELECT id, connection_id, connection_name, endpoint, model, status, requested_at, decided_at
               FROM ai_connection_approvals ORDER BY requested_at DESC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(GuardianApproval {
                    id: r.get(0)?,
                    connection_id: r.get(1)?,
                    connection_name: r.get(2)?,
                    endpoint: r.get(3)?,
                    model: r.get(4)?,
                    status: r.get(5)?,
                    requested_at: r.get(6)?,
                    decided_at: r.get(7)?,
                })
            })?
            .collect();
        rows
    };
    read().map_err(|_| Refused::Storage("approvals"))
}

/// Asks the learner's guardians to approve a remote connection at its
/// current endpoint. The request travels with the next guardian sync. Asking
/// again while a request is pending or approved returns that request.
pub fn request_approval(
    conn: &Connection,
    connection_id: &str,
) -> Result<GuardianApproval, Refused> {
    let doc = studio_store::connection(conn, connection_id)?;
    if !remote(&doc.value)? {
        return Err(Refused::NotRemote);
    }
    let has_guardian: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM guardian_links WHERE side = 'ward' AND status = 'active')",
            [],
            |r| r.get(0),
        )
        .map_err(|_| Refused::Storage("guardian links"))?;
    if !has_guardian {
        return Err(Refused::NoGuardian);
    }
    may_connect(conn, &doc.value)?;
    let open = matches!(
        latest_decision(conn, &doc.id, &doc.value.endpoint)?.as_deref(),
        Some("pending" | "approved")
    );
    let id = if open {
        None
    } else {
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO ai_connection_approvals (id, connection_id, connection_name, endpoint, model, status, requested_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6)",
            rusqlite::params![id, doc.id, doc.value.name, doc.value.endpoint, doc.value.model, crate::ai_chat::now()],
        )
        .map_err(|_| Refused::Storage("approvals"))?;
        Some(id)
    };
    approvals(conn)?
        .into_iter()
        .find(|a| match &id {
            Some(id) => &a.id == id,
            None => a.connection_id == doc.id && a.endpoint == doc.value.endpoint,
        })
        .ok_or(Refused::Storage("the approval"))
}

/// Ward side: the open requests to send a guardian, oldest first.
pub fn pending_approvals(conn: &Connection, limit: u32) -> Result<Vec<PendingApproval>, Refused> {
    let read = || -> rusqlite::Result<Vec<PendingApproval>> {
        let mut stmt = conn.prepare(
            "SELECT id, connection_name, endpoint, model FROM ai_connection_approvals
              WHERE status = 'pending' ORDER BY requested_at LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit], |r| {
                Ok(PendingApproval {
                    id: r.get(0)?,
                    connection_name: r.get(1)?,
                    endpoint: r.get(2)?,
                    model: r.get(3)?,
                })
            })?
            .collect();
        rows
    };
    read().map_err(|_| Refused::Storage("approvals"))
}

/// Ward side: applies a guardian's decision, arriving on `link_id`, to a
/// request this profile made, for the endpoint it asked about. The latest
/// decision applies, so an approval can be withdrawn. Returns whether a
/// matching request existed.
pub fn record_decision(
    conn: &Connection,
    link_id: &str,
    request_id: &str,
    endpoint: &str,
    approved: bool,
) -> Result<bool, Refused> {
    let status = if approved { "approved" } else { "declined" };
    conn.execute(
        "UPDATE ai_connection_approvals
            SET status = ?3, decided_by_link = ?4, decided_at = datetime('now')
          WHERE id = ?1 AND endpoint = ?2",
        rusqlite::params![request_id, endpoint, status, link_id],
    )
    .map(|changed| changed == 1)
    .map_err(|_| Refused::Storage("approvals"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_chat::turn::fixtures::*;
    use alexandria_studio::model::StudioDocument;
    use chrono::Datelike;

    /// Sets the profile's birthdate `years` years and a day before today.
    fn born_years_ago(conn: &Connection, years: i32) {
        let today = chrono::Utc::now().date_naive();
        let born = chrono::NaiveDate::from_ymd_opt(today.year() - years, today.month(), 1).unwrap()
            - chrono::Duration::days(1);
        conn.execute(
            "UPDATE local_identity SET birthdate = ?1",
            [born.format("%Y-%m-%d").to_string()],
        )
        .unwrap();
    }

    fn link_guardian(conn: &Connection) {
        conn.execute(
            "INSERT INTO guardian_links (id, side, peer_did, shared_key, status)
             VALUES ('link', 'ward', 'did:key:zParent', zeroblob(32), 'active')",
            [],
        )
        .unwrap();
    }

    /// The connection as it is stored now.
    fn current(conn: &Connection, id: &str) -> StudioConnection {
        studio_store::connection(conn, id).unwrap().value
    }

    #[test]
    fn under_13_learners_use_local_models_only_whatever_a_guardian_approves() {
        let conn = db();
        born_years_ago(&conn, 11);
        link_guardian(&conn);
        assert!(may_use(&conn, &current(&conn, "c1")).is_ok());
        connection(&conn, "c1", "cloud", "https://api.openai.com/v1", None);
        assert!(matches!(
            request_approval(&conn, "c1"),
            Err(Refused::TooYoungForRemote)
        ));
        conn.execute(
            "INSERT INTO ai_connection_approvals (id, connection_id, connection_name, endpoint, model, status, requested_at)
             VALUES ('a', 'c1', 'Ollama', 'https://api.openai.com/v1', 'm', 'approved', '2026-10-24T00:00:00Z')",
            [],
        )
        .unwrap();
        let refused = may_use(&conn, &current(&conn, "c1")).unwrap_err();
        assert!(refused.to_string().starts_with("under_13"), "{refused}");
        connection(
            &conn,
            "c1",
            "private_network",
            "http://192.168.1.40:1234/v1",
            None,
        );
        assert!(may_use(&conn, &current(&conn, "c1")).is_ok());
    }

    #[test]
    fn vendors_with_an_18_plus_floor_are_refused_for_minors_and_allowed_for_adults() {
        let conn = db();
        let gemini = "https://generativelanguage.googleapis.com/v1beta/openai";
        connection(&conn, "c1", "cloud", gemini, None);
        assert!(may_use(&conn, &current(&conn, "c1")).is_ok());
        born_years_ago(&conn, 16);
        link_guardian(&conn);
        assert!(matches!(
            request_approval(&conn, "c1"),
            Err(Refused::AdultsOnly(_))
        ));
        conn.execute(
            "INSERT INTO ai_connection_approvals (id, connection_id, connection_name, endpoint, model, status, requested_at)
             VALUES ('a', 'c1', 'Ollama', ?1, 'm', 'approved', '2026-10-24T00:00:00Z')",
            [gemini],
        )
        .unwrap();
        let err = may_use(&conn, &current(&conn, "c1"))
            .unwrap_err()
            .to_string();
        assert!(
            err.starts_with("adults_only") && err.contains("Gemini"),
            "{err}"
        );
    }

    #[test]
    fn consumer_sign_ins_are_never_stored_or_used() {
        let conn = db();
        let doc = |endpoint: &str| StudioDocument {
            id: "c2".into(),
            revision: 0,
            value: StudioConnection {
                id: "c2".into(),
                name: "Mine".into(),
                endpoint: endpoint.into(),
                model: "m".into(),
                location: "cloud".into(),
                capability: "text".into(),
                enabled: true,
                has_key: false,
                context_tokens: None,
                protocol: Default::default(),
                auth: Default::default(),
            },
        };
        let err = studio_store::save_connection(
            &conn,
            &doc("https://api.anthropic.com/v1"),
            Some("sk-ant-oat01-x"),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("subscription sign-in token"), "{err}");
        assert!(
            studio_store::connection(&conn, "c2").is_err(),
            "nothing saved"
        );
        let err = studio_store::save_connection(
            &conn,
            &doc("https://chatgpt.com/backend-api/codex"),
            Some("k"),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("consumer app"), "{err}");
        studio_store::save_connection(
            &conn,
            &doc("https://api.anthropic.com/v1"),
            Some("sk-ant-api03-x"),
        )
        .unwrap();
    }

    #[test]
    fn minors_need_a_guardians_approval_for_remote_models_at_that_endpoint() {
        let conn = db();
        born_years_ago(&conn, 15);
        assert!(may_use(&conn, &current(&conn, "c1")).is_ok());
        connection(&conn, "c1", "cloud", "https://api.openai.com/v1", None);
        assert!(matches!(
            may_use(&conn, &current(&conn, "c1")),
            Err(Refused::GuardianRequired)
        ));
        // Asking needs a linked guardian.
        assert!(matches!(
            request_approval(&conn, "c1"),
            Err(Refused::NoGuardian)
        ));
        link_guardian(&conn);
        let asked = request_approval(&conn, "c1").unwrap();
        assert_eq!(asked.status, "pending");
        assert_eq!(
            request_approval(&conn, "c1").unwrap().id,
            asked.id,
            "no duplicate request"
        );
        assert!(matches!(
            may_use(&conn, &current(&conn, "c1")),
            Err(Refused::GuardianPending)
        ));
        // The guardian sync sends what the guardian sees, and the decision
        // comes back bound to the endpoint asked about.
        assert_eq!(
            pending_approvals(&conn, 20).unwrap(),
            vec![PendingApproval {
                id: asked.id.clone(),
                connection_name: "Ollama".into(),
                endpoint: "https://api.openai.com/v1".into(),
                model: "llama3.1:8b".into(),
            }]
        );
        assert!(
            !record_decision(&conn, "link", &asked.id, "https://evil.example/v1", true).unwrap()
        );
        assert!(
            record_decision(&conn, "link", &asked.id, "https://api.openai.com/v1", true).unwrap()
        );
        assert!(pending_approvals(&conn, 20).unwrap().is_empty());
        assert!(may_use(&conn, &current(&conn, "c1")).is_ok());
        // A different endpoint is a different approval.
        connection(&conn, "c1", "cloud", "https://api.anthropic.com/v1", None);
        assert!(matches!(
            may_use(&conn, &current(&conn, "c1")),
            Err(Refused::GuardianRequired)
        ));
        // Models on the learner's own network never need approval.
        connection(
            &conn,
            "c1",
            "private_network",
            "http://192.168.1.40:1234/v1",
            None,
        );
        assert!(may_use(&conn, &current(&conn, "c1")).is_ok());
        // A declined request says so.
        connection(&conn, "c1", "cloud", "https://api.openai.com/v1", None);
        assert!(
            record_decision(&conn, "link", &asked.id, "https://api.openai.com/v1", false).unwrap()
        );
        assert!(matches!(
            may_use(&conn, &current(&conn, "c1")),
            Err(Refused::GuardianDeclined)
        ));
    }

    #[test]
    fn provider_sign_in_follows_the_same_age_rules() {
        use crate::ai_auth::{chatgpt, openrouter};
        use crate::commands::ai_signin::sign_in_target;
        let conn = db();
        let chatgpt = sign_in_target(chatgpt::API_BASE);
        let openrouter = sign_in_target(openrouter::API_BASE);
        assert!(may_connect(&conn, &chatgpt).is_ok());
        assert!(may_connect(&conn, &openrouter).is_ok());
        // 13 to 17: a remote sign-in is allowed and then needs a guardian's
        // approval to use; an adults-only vendor never.
        born_years_ago(&conn, 15);
        assert!(may_connect(&conn, &chatgpt).is_ok());
        assert!(matches!(
            may_use(&conn, &chatgpt),
            Err(Refused::GuardianRequired)
        ));
        assert!(matches!(
            may_connect(&conn, &openrouter),
            Err(Refused::AdultsOnly("OpenRouter"))
        ));
        // Under 13: no remote sign-in at all.
        born_years_ago(&conn, 12);
        for target in [&chatgpt, &openrouter] {
            assert!(matches!(
                may_connect(&conn, target),
                Err(Refused::TooYoungForRemote)
            ));
        }
    }
}
