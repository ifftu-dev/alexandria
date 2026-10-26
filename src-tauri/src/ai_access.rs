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
//!   for setting a connection up (provider sign-in, the connection form,
//!   listing models, asking for approval).
//! * [`may_use_remote_service`]: the under-13 rule for the one fixed remote
//!   service that is not a model connection (Optional AI checks).
//! * [`no_attempt_in_progress`]: every AI surface refuses while an
//!   assessment attempt or interview check runs; chat and the tutor record
//!   the refusal as an integrity flag (see [`Surface`]).
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

/// May the profile's holder send learning data to a fixed remote service
/// that is not a model connection: the Optional AI checks service, which
/// runs under its own terms. Under 13, no: the same reason as remote models.
/// Vendor age floors and guardian approvals belong to model connections and
/// endpoints, so they do not apply here.
pub fn may_use_remote_service(conn: &Connection) -> Result<(), Refused> {
    match age_band(conn)? {
        AgeBand::Child => Err(Refused::TooYoungForRemote),
        AgeBand::Teen | AgeBand::Adult => Ok(()),
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

// ─── the attempt gate ────────────────────────────────────────────────────

/// Which AI surface asks whether an attempt is in progress. They are all
/// refused; they differ in whether the refusal is an integrity flag.
///
/// The webview learns that an attempt began only when it next refreshes the
/// chat status, so a click already in flight can arrive after the attempt
/// started. Only requests that reach a model, or show model or practice
/// content on request, are flagged; everything else that touches AI state
/// is refused quietly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// Chat that reaches a model or shows its content on request: preparing
    /// and starting a turn, practice attempts, hints and reveals, and
    /// allowing a proposal. Flagged.
    ChatModel,
    /// Every other chat command that reads or changes chat state: threads,
    /// attachments, models, memory, notes, approvals. Refused, not flagged.
    ChatState,
    /// Asking the lesson tutor, or having its answer checked. Flagged.
    Tutor,
    /// Saving a chat or tutor answer that was already generating when the
    /// attempt started: discarded, not flagged, since it was asked before.
    LateAnswer,
    /// A Studio run step: refused, not flagged, since a run may have started
    /// before the attempt.
    StudioRun,
    /// Course notes, which a course page may still list as an attempt
    /// starts: refused, not flagged.
    CourseNotes,
}

impl Surface {
    fn flag(self) -> Option<&'static str> {
        match self {
            Surface::ChatModel | Surface::Tutor => Some("in_app_assistant_during_attempt"),
            Surface::ChatState
            | Surface::LateAnswer
            | Surface::StudioRun
            | Surface::CourseNotes => None,
        }
    }
}

/// Refuses while an assessment attempt or interview check is in progress
/// (see `alexandria_studio::gate`), and records the surface's integrity flag
/// into the attempt's committed snapshot chain when it has one.
pub fn no_attempt_in_progress(conn: &Connection, surface: Surface) -> Result<(), Refused> {
    match alexandria_studio::gate::require_no_assessment(conn) {
        Ok(()) => Ok(()),
        Err(alexandria_studio::Error::AssessmentActive) => {
            if let Some(flag) = surface.flag() {
                if let Err(error) = crate::sentinel::snapshots::record_backend_flag(conn, flag) {
                    log::warn!("{surface:?}: could not record an integrity flag: {error}");
                }
            }
            Err(Refused::AttemptInProgress)
        }
        Err(other) => Err(other.into()),
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
    // A refusal no guardian can lift is the answer, before asking for a link.
    may_connect(conn, &doc.value)?;
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
    use crate::ai_chat::vocab::refused;
    use alexandria_studio::model::StudioDocument;

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
        let refused = refused(may_use(&conn, &current(&conn, "c1")).unwrap_err());
        assert!(refused.starts_with("under_13"), "{refused}");
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
        let err = refused(may_use(&conn, &current(&conn, "c1")).unwrap_err());
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
    fn an_attempt_refuses_every_surface_and_flags_only_what_the_open_app_never_sends() {
        let conn = db();
        let surfaces = [
            (Surface::ChatModel, true),
            (Surface::Tutor, true),
            (Surface::ChatState, false),
            (Surface::LateAnswer, false),
            (Surface::StudioRun, false),
            (Surface::CourseNotes, false),
        ];
        for (surface, _) in surfaces {
            assert!(no_attempt_in_progress(&conn, surface).is_ok());
        }
        for (surface, flags) in surfaces {
            let id = format!("{surface:?}");
            attempt(&conn, &id);
            let refused = no_attempt_in_progress(&conn, surface).unwrap_err();
            assert!(matches!(refused, Refused::AttemptInProgress));
            assert!(matches!(
                alexandria_studio::Error::from(refused),
                alexandria_studio::Error::AssessmentActive
            ));
            assert_eq!(flagged(&conn, &id), flags, "{id}");
        }
    }

    #[test]
    fn a_send_is_refused_when_the_rules_changed_after_it_was_prepared() {
        let conn = db();
        thread(&conn);
        born_years_ago(&conn, 15);
        link_guardian(&conn);
        let openai = "https://api.openai.com/v1";
        connection(&conn, "c1", "cloud", openai, None);
        let asked = request_approval(&conn, "c1").unwrap();
        assert!(record_decision(&conn, "link", &asked.id, openai, true).unwrap());
        // The guardian withdraws the approval after the learner reviewed.
        let prepared = prepare(&conn, "Hello").unwrap();
        assert!(record_decision(&conn, "link", &asked.id, openai, false).unwrap());
        let refused = start(&conn, &prepared, "q1").unwrap_err();
        assert!(refused.starts_with("guardian_declined"), "{refused}");
        // A corrected birthdate applies at send time too.
        assert!(record_decision(&conn, "link", &asked.id, openai, true).unwrap());
        let prepared = prepare(&conn, "Hello").unwrap();
        born_years_ago(&conn, 12);
        let refused = start(&conn, &prepared, "q2").unwrap_err();
        assert!(refused.starts_with("under_13"), "{refused}");
        born_years_ago(&conn, 15);
        start(&conn, &prepared, "q3").unwrap();
    }

    #[test]
    fn a_learner_hears_the_final_reason_before_being_asked_to_link_a_guardian() {
        let conn = db();
        connection(&conn, "c1", "cloud", "https://api.openai.com/v1", None);
        born_years_ago(&conn, 11);
        assert!(matches!(
            request_approval(&conn, "c1"),
            Err(Refused::TooYoungForRemote)
        ));
        born_years_ago(&conn, 16);
        assert!(matches!(
            request_approval(&conn, "c1"),
            Err(Refused::NoGuardian)
        ));
        let gemini = "https://generativelanguage.googleapis.com/v1beta/openai";
        connection(&conn, "c1", "cloud", gemini, None);
        assert!(matches!(
            request_approval(&conn, "c1"),
            Err(Refused::AdultsOnly(_))
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
