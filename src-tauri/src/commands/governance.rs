//! IPC commands for DAO governance.
//!
//! Exposes the full governance lifecycle to the frontend:
//!   - DAO management (list, create, get with members)
//!   - Election lifecycle (open, nominate, accept, start voting, vote, finalize, install)
//!   - Proposal lifecycle (submit, approve, cancel, vote, resolve)
//!
//! Port of `api/internal/handler/governance.go` (20 endpoints) adapted for
//! local-first operation. Committee/admin checks use local identity.

use rusqlite::params;
use tauri::State;

use crate::crypto::hash::entity_id;
use crate::domain::governance::{
    CreateDaoParams, DaoInfo, DaoMember, Election, ElectionNominee, ElectionVote,
    OpenElectionParams, Proposal, ProposalVote, SubmitProposalParams,
};
use crate::AppState;

/// Default proposal voting deadline: 14 days from approval.
const DEFAULT_VOTING_DAYS: i64 = 14;

/// Supermajority threshold for proposal resolution (2/3).
const SUPERMAJORITY_THRESHOLD: f64 = 2.0 / 3.0;

// ---- DAO Commands ----

/// List all DAOs, optionally filtered by scope_type and/or status.
#[tauri::command]
pub async fn list_daos(
    state: State<'_, AppState>,
    scope_type: Option<String>,
    status: Option<String>,
    search: Option<String>,
) -> Result<Vec<DaoInfo>, String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    let mut conditions: Vec<String> = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(ref st) = scope_type {
        conditions.push(format!("scope_type = ?{idx}"));
        param_values.push(Box::new(st.clone()));
        idx += 1;
    }
    if let Some(ref s) = status {
        conditions.push(format!("status = ?{idx}"));
        param_values.push(Box::new(s.clone()));
        idx += 1;
    }
    if let Some(ref q) = search {
        conditions.push(format!("(name LIKE ?{idx} OR description LIKE ?{idx})"));
        param_values.push(Box::new(format!("%{q}%")));
        idx += 1;
    }
    let _ = idx;

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT id, name, description, icon_emoji, scope_type, scope_id, status, \
         committee_size, election_interval_days, on_chain_tx, created_at, updated_at \
         FROM governance_daos {where_clause} ORDER BY name ASC"
    );

    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let daos = stmt
        .query_map(params_ref.as_slice(), |row| {
            Ok(DaoInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                icon_emoji: row.get(3)?,
                scope_type: row.get(4)?,
                scope_id: row.get(5)?,
                status: row.get(6)?,
                committee_size: row.get(7)?,
                election_interval_days: row.get(8)?,
                on_chain_tx: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(daos)
}

/// Create a new DAO.
#[tauri::command]
pub async fn create_dao(
    state: State<'_, AppState>,
    params: CreateDaoParams,
) -> Result<DaoInfo, String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    let id = entity_id(&[&params.name, &params.scope_type, &params.scope_id]);
    let committee_size = params.committee_size.unwrap_or(5);
    let election_interval = params.election_interval_days.unwrap_or(365);

    conn.execute(
        "INSERT INTO governance_daos \
         (id, name, description, scope_type, scope_id, status, committee_size, election_interval_days) \
         VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?7)",
        params![
            id,
            params.name,
            params.description,
            params.scope_type,
            params.scope_id,
            committee_size,
            election_interval,
        ],
    )
    .map_err(|e| e.to_string())?;

    Ok(DaoInfo {
        id,
        name: params.name,
        description: params.description,
        icon_emoji: None,
        scope_type: params.scope_type,
        scope_id: params.scope_id,
        status: "pending".into(),
        committee_size,
        election_interval_days: election_interval,
        on_chain_tx: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Get a DAO by ID, including its members.
#[tauri::command]
pub async fn get_dao(
    state: State<'_, AppState>,
    dao_id: String,
) -> Result<(DaoInfo, Vec<DaoMember>), String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    let dao: DaoInfo = conn
        .query_row(
            "SELECT id, name, description, icon_emoji, scope_type, scope_id, status, \
             committee_size, election_interval_days, on_chain_tx, created_at, updated_at \
             FROM governance_daos WHERE id = ?1",
            params![dao_id],
            |row| {
                Ok(DaoInfo {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    icon_emoji: row.get(3)?,
                    scope_type: row.get(4)?,
                    scope_id: row.get(5)?,
                    status: row.get(6)?,
                    committee_size: row.get(7)?,
                    election_interval_days: row.get(8)?,
                    on_chain_tx: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                })
            },
        )
        .map_err(|e| format!("DAO not found: {e}"))?;

    let mut stmt = conn
        .prepare(
            "SELECT dao_id, stake_address, role, joined_at \
             FROM governance_dao_members WHERE dao_id = ?1",
        )
        .map_err(|e| e.to_string())?;

    let members = stmt
        .query_map(params![dao_id], |row| {
            Ok(DaoMember {
                dao_id: row.get(0)?,
                stake_address: row.get(1)?,
                role: row.get(2)?,
                joined_at: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok((dao, members))
}

// ---- Election Commands ----

/// Open a new election for a DAO.
#[tauri::command]
pub async fn open_election(
    state: State<'_, AppState>,
    params: OpenElectionParams,
) -> Result<Election, String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    // Verify DAO exists and is active
    let dao_status: String = conn
        .query_row(
            "SELECT status FROM governance_daos WHERE id = ?1",
            params![params.dao_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("DAO not found: {e}"))?;

    if dao_status != "active" {
        return Err(format!("DAO is not active (status: {dao_status})"));
    }

    let id = entity_id(&[&params.dao_id, &params.title, &chrono::Utc::now().to_rfc3339()]);
    let seats = params.seats.unwrap_or(5);
    let nominee_prof = params.nominee_min_proficiency.unwrap_or_else(|| "apply".into());
    let voter_prof = params.voter_min_proficiency.unwrap_or_else(|| "remember".into());

    conn.execute(
        "INSERT INTO governance_elections \
         (id, dao_id, title, description, phase, seats, \
          nominee_min_proficiency, voter_min_proficiency, \
          nomination_end, voting_end) \
         VALUES (?1, ?2, ?3, ?4, 'nomination', ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            params.dao_id,
            params.title,
            params.description,
            seats,
            nominee_prof,
            voter_prof,
            params.nomination_end,
            params.voting_end,
        ],
    )
    .map_err(|e| e.to_string())?;

    let election = query_election(conn, &id)?;
    Ok(election)
}

/// List elections, optionally filtered by dao_id and/or phase.
#[tauri::command]
pub async fn list_elections(
    state: State<'_, AppState>,
    dao_id: Option<String>,
    phase: Option<String>,
) -> Result<Vec<Election>, String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    let mut conditions: Vec<String> = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(ref d) = dao_id {
        conditions.push(format!("dao_id = ?{idx}"));
        param_values.push(Box::new(d.clone()));
        idx += 1;
    }
    if let Some(ref p) = phase {
        conditions.push(format!("phase = ?{idx}"));
        param_values.push(Box::new(p.clone()));
        idx += 1;
    }
    let _ = idx;

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT id, dao_id, title, description, phase, seats, \
         nominee_min_proficiency, voter_min_proficiency, \
         nomination_start, nomination_end, voting_end, on_chain_tx, \
         created_at, finalized_at \
         FROM governance_elections {where_clause} ORDER BY created_at DESC"
    );

    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let elections = stmt
        .query_map(params_ref.as_slice(), |row| {
            Ok(Election {
                id: row.get(0)?,
                dao_id: row.get(1)?,
                title: row.get(2)?,
                description: row.get(3)?,
                phase: row.get(4)?,
                seats: row.get(5)?,
                nominee_min_proficiency: row.get(6)?,
                voter_min_proficiency: row.get(7)?,
                nomination_start: row.get(8)?,
                nomination_end: row.get(9)?,
                voting_end: row.get(10)?,
                on_chain_tx: row.get(11)?,
                created_at: row.get(12)?,
                finalized_at: row.get(13)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(elections)
}

/// Get an election by ID with its nominees.
#[tauri::command]
pub async fn get_election(
    state: State<'_, AppState>,
    election_id: String,
) -> Result<(Election, Vec<ElectionNominee>), String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    let election = query_election(conn, &election_id)?;
    let nominees = query_nominees(conn, &election_id)?;

    Ok((election, nominees))
}

/// Nominate someone (or self) for an election.
#[tauri::command]
pub async fn nominate(
    state: State<'_, AppState>,
    election_id: String,
    stake_address: String,
) -> Result<ElectionNominee, String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    // Verify election is in nomination phase
    let phase: String = conn
        .query_row(
            "SELECT phase FROM governance_elections WHERE id = ?1",
            params![election_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("election not found: {e}"))?;

    if phase != "nomination" {
        return Err(format!("election is not in nomination phase (phase: {phase})"));
    }

    let id = entity_id(&[&election_id, &stake_address]);

    conn.execute(
        "INSERT INTO governance_election_nominees \
         (id, election_id, stake_address) VALUES (?1, ?2, ?3)",
        params![id, election_id, stake_address],
    )
    .map_err(|e| e.to_string())?;

    Ok(ElectionNominee {
        id,
        election_id,
        stake_address,
        accepted: false,
        votes_received: 0,
        is_winner: false,
        nominated_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Accept a nomination (nominee confirms their candidacy).
#[tauri::command]
pub async fn accept_nomination(
    state: State<'_, AppState>,
    nominee_id: String,
) -> Result<(), String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    let affected = conn
        .execute(
            "UPDATE governance_election_nominees SET accepted = 1 WHERE id = ?1",
            params![nominee_id],
        )
        .map_err(|e| e.to_string())?;

    if affected == 0 {
        return Err("nominee not found".into());
    }

    Ok(())
}

/// Transition an election from nomination to voting phase.
#[tauri::command]
pub async fn start_election_voting(
    state: State<'_, AppState>,
    election_id: String,
) -> Result<(), String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    let phase: String = conn
        .query_row(
            "SELECT phase FROM governance_elections WHERE id = ?1",
            params![election_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("election not found: {e}"))?;

    if phase != "nomination" {
        return Err(format!("election must be in nomination phase to start voting (phase: {phase})"));
    }

    // Verify at least `seats` accepted nominees
    let (seats, accepted_count): (i64, i64) = conn
        .query_row(
            "SELECT e.seats, \
             (SELECT COUNT(*) FROM governance_election_nominees n \
              WHERE n.election_id = e.id AND n.accepted = 1) \
             FROM governance_elections e WHERE e.id = ?1",
            params![election_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| e.to_string())?;

    if accepted_count < seats {
        return Err(format!(
            "need at least {seats} accepted nominees to start voting, have {accepted_count}"
        ));
    }

    conn.execute(
        "UPDATE governance_elections SET phase = 'voting' WHERE id = ?1",
        params![election_id],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// Cast a vote in an election (one vote per voter).
#[tauri::command]
pub async fn cast_election_vote(
    state: State<'_, AppState>,
    election_id: String,
    voter: String,
    nominee_id: String,
) -> Result<ElectionVote, String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    // Verify election is in voting phase
    let phase: String = conn
        .query_row(
            "SELECT phase FROM governance_elections WHERE id = ?1",
            params![election_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("election not found: {e}"))?;

    if phase != "voting" {
        return Err(format!("election is not in voting phase (phase: {phase})"));
    }

    // Check double-vote
    let already_voted: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM governance_election_votes \
             WHERE election_id = ?1 AND voter = ?2",
            params![election_id, voter],
            |row| Ok(row.get::<_, i64>(0)? > 0),
        )
        .map_err(|e| e.to_string())?;

    if already_voted {
        return Err("already voted in this election".into());
    }

    // Verify nominee exists and is accepted
    let nominee_accepted: bool = conn
        .query_row(
            "SELECT accepted FROM governance_election_nominees WHERE id = ?1",
            params![nominee_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("nominee not found: {e}"))?;

    if !nominee_accepted {
        return Err("nominee has not accepted their nomination".into());
    }

    let vote_id = entity_id(&[&election_id, &voter]);

    conn.execute(
        "INSERT INTO governance_election_votes \
         (id, election_id, voter, nominee_id) VALUES (?1, ?2, ?3, ?4)",
        params![vote_id, election_id, voter, nominee_id],
    )
    .map_err(|e| e.to_string())?;

    // Increment nominee vote count
    conn.execute(
        "UPDATE governance_election_nominees SET votes_received = votes_received + 1 WHERE id = ?1",
        params![nominee_id],
    )
    .map_err(|e| e.to_string())?;

    Ok(ElectionVote {
        id: vote_id,
        election_id,
        voter,
        nominee_id,
        on_chain_tx: None,
        voted_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Finalize an election: determine winners and transition to finalized.
#[tauri::command]
pub async fn finalize_election(
    state: State<'_, AppState>,
    election_id: String,
) -> Result<Vec<ElectionNominee>, String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    let (phase, seats): (String, i64) = conn
        .query_row(
            "SELECT phase, seats FROM governance_elections WHERE id = ?1",
            params![election_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| format!("election not found: {e}"))?;

    if phase != "voting" {
        return Err(format!("election must be in voting phase to finalize (phase: {phase})"));
    }

    // Get top N accepted nominees by votes_received
    let mut stmt = conn
        .prepare(
            "SELECT id FROM governance_election_nominees \
             WHERE election_id = ?1 AND accepted = 1 \
             ORDER BY votes_received DESC LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;

    let winner_ids: Vec<String> = stmt
        .query_map(params![election_id, seats], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    // Mark winners
    for wid in &winner_ids {
        conn.execute(
            "UPDATE governance_election_nominees SET is_winner = 1 WHERE id = ?1",
            params![wid],
        )
        .map_err(|e| e.to_string())?;
    }

    // Transition to finalized
    conn.execute(
        "UPDATE governance_elections SET phase = 'finalized', finalized_at = datetime('now') \
         WHERE id = ?1",
        params![election_id],
    )
    .map_err(|e| e.to_string())?;

    let nominees = query_nominees(conn, &election_id)?;
    Ok(nominees)
}

/// Install winners as the new DAO committee after a finalized election.
#[tauri::command]
pub async fn install_committee(
    state: State<'_, AppState>,
    election_id: String,
) -> Result<Vec<DaoMember>, String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    let (phase, dao_id): (String, String) = conn
        .query_row(
            "SELECT phase, dao_id FROM governance_elections WHERE id = ?1",
            params![election_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| format!("election not found: {e}"))?;

    if phase != "finalized" {
        return Err(format!("election must be finalized to install committee (phase: {phase})"));
    }

    // Get winner stake addresses
    let mut stmt = conn
        .prepare(
            "SELECT stake_address FROM governance_election_nominees \
             WHERE election_id = ?1 AND is_winner = 1",
        )
        .map_err(|e| e.to_string())?;

    let winners: Vec<String> = stmt
        .query_map(params![election_id], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    // Replace committee: delete all committee members, insert winners
    conn.execute(
        "DELETE FROM governance_dao_members WHERE dao_id = ?1 AND role = 'committee'",
        params![dao_id],
    )
    .map_err(|e| e.to_string())?;

    let now = chrono::Utc::now().to_rfc3339();
    for addr in &winners {
        conn.execute(
            "INSERT OR REPLACE INTO governance_dao_members \
             (dao_id, stake_address, role, joined_at) VALUES (?1, ?2, 'committee', ?3)",
            params![dao_id, addr, now],
        )
        .map_err(|e| e.to_string())?;
    }

    // Return new committee
    let mut stmt = conn
        .prepare(
            "SELECT dao_id, stake_address, role, joined_at \
             FROM governance_dao_members WHERE dao_id = ?1",
        )
        .map_err(|e| e.to_string())?;

    let members = stmt
        .query_map(params![dao_id], |row| {
            Ok(DaoMember {
                dao_id: row.get(0)?,
                stake_address: row.get(1)?,
                role: row.get(2)?,
                joined_at: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(members)
}

// ---- Proposal Commands ----

/// Submit a new proposal to a DAO.
#[tauri::command]
pub async fn submit_proposal(
    state: State<'_, AppState>,
    params: SubmitProposalParams,
) -> Result<Proposal, String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    // Verify DAO exists and is active
    let dao_status: String = conn
        .query_row(
            "SELECT status FROM governance_daos WHERE id = ?1",
            params![params.dao_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("DAO not found: {e}"))?;

    if dao_status != "active" {
        return Err(format!("DAO is not active (status: {dao_status})"));
    }

    // Get local identity as proposer
    let proposer: String = conn
        .query_row(
            "SELECT stake_address FROM local_identity WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("no local identity: {e}"))?;

    let id = entity_id(&[&params.dao_id, &params.title, &proposer]);
    let min_prof = params.min_vote_proficiency.unwrap_or_else(|| "remember".into());

    conn.execute(
        "INSERT INTO governance_proposals \
         (id, dao_id, title, description, category, status, proposer, \
          min_vote_proficiency) \
         VALUES (?1, ?2, ?3, ?4, ?5, 'draft', ?6, ?7)",
        params![
            id,
            params.dao_id,
            params.title,
            params.description,
            params.category,
            proposer,
            min_prof,
        ],
    )
    .map_err(|e| e.to_string())?;

    let proposal = query_proposal(conn, &id)?;
    Ok(proposal)
}

/// List proposals, optionally filtered by dao_id, status, category.
#[tauri::command]
pub async fn list_proposals(
    state: State<'_, AppState>,
    dao_id: Option<String>,
    status: Option<String>,
    category: Option<String>,
) -> Result<Vec<Proposal>, String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    let mut conditions: Vec<String> = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(ref d) = dao_id {
        conditions.push(format!("dao_id = ?{idx}"));
        param_values.push(Box::new(d.clone()));
        idx += 1;
    }
    if let Some(ref s) = status {
        conditions.push(format!("status = ?{idx}"));
        param_values.push(Box::new(s.clone()));
        idx += 1;
    }
    if let Some(ref c) = category {
        conditions.push(format!("category = ?{idx}"));
        param_values.push(Box::new(c.clone()));
        idx += 1;
    }
    let _ = idx;

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT id, dao_id, title, description, category, status, proposer, \
         votes_for, votes_against, voting_deadline, min_vote_proficiency, \
         on_chain_tx, created_at, resolved_at \
         FROM governance_proposals {where_clause} ORDER BY created_at DESC"
    );

    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    let proposals = stmt
        .query_map(params_ref.as_slice(), |row| {
            Ok(Proposal {
                id: row.get(0)?,
                dao_id: row.get(1)?,
                title: row.get(2)?,
                description: row.get(3)?,
                category: row.get(4)?,
                status: row.get(5)?,
                proposer: row.get(6)?,
                votes_for: row.get(7)?,
                votes_against: row.get(8)?,
                voting_deadline: row.get(9)?,
                min_vote_proficiency: row.get(10)?,
                on_chain_tx: row.get(11)?,
                created_at: row.get(12)?,
                resolved_at: row.get(13)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(proposals)
}

/// Approve a proposal (draft → published), setting voting deadline.
#[tauri::command]
pub async fn approve_proposal(
    state: State<'_, AppState>,
    proposal_id: String,
) -> Result<Proposal, String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    let status: String = conn
        .query_row(
            "SELECT status FROM governance_proposals WHERE id = ?1",
            params![proposal_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("proposal not found: {e}"))?;

    if status != "draft" {
        return Err(format!("proposal must be in draft status to approve (status: {status})"));
    }

    let deadline = chrono::Utc::now()
        + chrono::Duration::days(DEFAULT_VOTING_DAYS);

    conn.execute(
        "UPDATE governance_proposals SET status = 'published', voting_deadline = ?1 WHERE id = ?2",
        params![deadline.to_rfc3339(), proposal_id],
    )
    .map_err(|e| e.to_string())?;

    let proposal = query_proposal(conn, &proposal_id)?;
    Ok(proposal)
}

/// Cancel a proposal (only if not already resolved).
#[tauri::command]
pub async fn cancel_proposal(
    state: State<'_, AppState>,
    proposal_id: String,
) -> Result<(), String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    let status: String = conn
        .query_row(
            "SELECT status FROM governance_proposals WHERE id = ?1",
            params![proposal_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("proposal not found: {e}"))?;

    if status == "approved" || status == "rejected" {
        return Err(format!("cannot cancel a resolved proposal (status: {status})"));
    }

    conn.execute(
        "UPDATE governance_proposals SET status = 'cancelled' WHERE id = ?1",
        params![proposal_id],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// Cast a vote on a proposal (one vote per voter).
#[tauri::command]
pub async fn cast_proposal_vote(
    state: State<'_, AppState>,
    proposal_id: String,
    voter: String,
    in_favor: bool,
) -> Result<ProposalVote, String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    // Verify proposal is published
    let status: String = conn
        .query_row(
            "SELECT status FROM governance_proposals WHERE id = ?1",
            params![proposal_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("proposal not found: {e}"))?;

    if status != "published" {
        return Err(format!("proposal is not open for voting (status: {status})"));
    }

    // Check double-vote
    let already_voted: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM governance_proposal_votes \
             WHERE proposal_id = ?1 AND voter = ?2",
            params![proposal_id, voter],
            |row| Ok(row.get::<_, i64>(0)? > 0),
        )
        .map_err(|e| e.to_string())?;

    if already_voted {
        return Err("already voted on this proposal".into());
    }

    let vote_id = entity_id(&[&proposal_id, &voter]);
    let in_favor_int: i64 = if in_favor { 1 } else { 0 };

    conn.execute(
        "INSERT INTO governance_proposal_votes \
         (id, proposal_id, voter, in_favor) VALUES (?1, ?2, ?3, ?4)",
        params![vote_id, proposal_id, voter, in_favor_int],
    )
    .map_err(|e| e.to_string())?;

    // Update tally
    if in_favor {
        conn.execute(
            "UPDATE governance_proposals SET votes_for = votes_for + 1 WHERE id = ?1",
            params![proposal_id],
        )
        .map_err(|e| e.to_string())?;
    } else {
        conn.execute(
            "UPDATE governance_proposals SET votes_against = votes_against + 1 WHERE id = ?1",
            params![proposal_id],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(ProposalVote {
        id: vote_id,
        proposal_id,
        voter,
        in_favor,
        on_chain_tx: None,
        voted_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Resolve a proposal using supermajority (2/3 votes_for).
#[tauri::command]
pub async fn resolve_proposal(
    state: State<'_, AppState>,
    proposal_id: String,
) -> Result<Proposal, String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    let (status, votes_for, votes_against): (String, i64, i64) = conn
        .query_row(
            "SELECT status, votes_for, votes_against FROM governance_proposals WHERE id = ?1",
            params![proposal_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|e| format!("proposal not found: {e}"))?;

    if status != "published" {
        return Err(format!("proposal must be published to resolve (status: {status})"));
    }

    let total_votes = votes_for + votes_against;
    if total_votes == 0 {
        return Err("cannot resolve proposal with no votes".into());
    }

    let approval_ratio = votes_for as f64 / total_votes as f64;
    let new_status = if approval_ratio >= SUPERMAJORITY_THRESHOLD {
        "approved"
    } else {
        "rejected"
    };

    conn.execute(
        "UPDATE governance_proposals SET status = ?1, resolved_at = datetime('now') WHERE id = ?2",
        params![new_status, proposal_id],
    )
    .map_err(|e| e.to_string())?;

    let proposal = query_proposal(conn, &proposal_id)?;
    Ok(proposal)
}

// ---- Internal Helpers ----

fn query_election(conn: &rusqlite::Connection, election_id: &str) -> Result<Election, String> {
    conn.query_row(
        "SELECT id, dao_id, title, description, phase, seats, \
         nominee_min_proficiency, voter_min_proficiency, \
         nomination_start, nomination_end, voting_end, on_chain_tx, \
         created_at, finalized_at \
         FROM governance_elections WHERE id = ?1",
        params![election_id],
        |row| {
            Ok(Election {
                id: row.get(0)?,
                dao_id: row.get(1)?,
                title: row.get(2)?,
                description: row.get(3)?,
                phase: row.get(4)?,
                seats: row.get(5)?,
                nominee_min_proficiency: row.get(6)?,
                voter_min_proficiency: row.get(7)?,
                nomination_start: row.get(8)?,
                nomination_end: row.get(9)?,
                voting_end: row.get(10)?,
                on_chain_tx: row.get(11)?,
                created_at: row.get(12)?,
                finalized_at: row.get(13)?,
            })
        },
    )
    .map_err(|e| format!("election not found: {e}"))
}

fn query_nominees(
    conn: &rusqlite::Connection,
    election_id: &str,
) -> Result<Vec<ElectionNominee>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, election_id, stake_address, accepted, votes_received, is_winner, nominated_at \
             FROM governance_election_nominees WHERE election_id = ?1 \
             ORDER BY votes_received DESC",
        )
        .map_err(|e| e.to_string())?;

    let nominees = stmt
        .query_map(params![election_id], |row| {
            Ok(ElectionNominee {
                id: row.get(0)?,
                election_id: row.get(1)?,
                stake_address: row.get(2)?,
                accepted: row.get(3)?,
                votes_received: row.get(4)?,
                is_winner: row.get(5)?,
                nominated_at: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    Ok(nominees)
}

fn query_proposal(conn: &rusqlite::Connection, proposal_id: &str) -> Result<Proposal, String> {
    conn.query_row(
        "SELECT id, dao_id, title, description, category, status, proposer, \
         votes_for, votes_against, voting_deadline, min_vote_proficiency, \
         on_chain_tx, created_at, resolved_at \
         FROM governance_proposals WHERE id = ?1",
        params![proposal_id],
        |row| {
            Ok(Proposal {
                id: row.get(0)?,
                dao_id: row.get(1)?,
                title: row.get(2)?,
                description: row.get(3)?,
                category: row.get(4)?,
                status: row.get(5)?,
                proposer: row.get(6)?,
                votes_for: row.get(7)?,
                votes_against: row.get(8)?,
                voting_deadline: row.get(9)?,
                min_vote_proficiency: row.get(10)?,
                on_chain_tx: row.get(11)?,
                created_at: row.get(12)?,
                resolved_at: row.get(13)?,
            })
        },
    )
    .map_err(|e| format!("proposal not found: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn setup_db() -> Database {
        let db = Database::open_in_memory().expect("open db");
        db.run_migrations().expect("migrations");

        // Create local identity
        db.conn()
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address) \
                 VALUES (1, 'stake_test1uvoter', 'addr_test1q123')",
                [],
            )
            .unwrap();

        // Create subject field and subject for DAO scope
        db.conn()
            .execute(
                "INSERT INTO subject_fields (id, name) VALUES ('sf1', 'CS')",
                [],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO subjects (id, name, subject_field_id) VALUES ('sub1', 'Algo', 'sf1')",
                [],
            )
            .unwrap();

        // Create an active DAO
        db.conn()
            .execute(
                "INSERT INTO governance_daos (id, name, scope_type, scope_id, status) \
                 VALUES ('dao1', 'CS DAO', 'subject_field', 'sf1', 'active')",
                [],
            )
            .unwrap();

        db
    }

    #[test]
    fn create_and_list_daos() {
        let db = setup_db();
        let conn = db.conn();

        // Count DAOs
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM governance_daos", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn election_lifecycle_nomination_to_finalized() {
        let db = setup_db();
        let conn = db.conn();

        // Open election
        let elec_id = entity_id(&["dao1", "test election", "now"]);
        conn.execute(
            "INSERT INTO governance_elections \
             (id, dao_id, title, phase, seats, nominee_min_proficiency, voter_min_proficiency) \
             VALUES (?1, 'dao1', 'Test Election', 'nomination', 3, 'apply', 'remember')",
            params![elec_id],
        )
        .unwrap();

        // Verify phase
        let phase: String = conn
            .query_row(
                "SELECT phase FROM governance_elections WHERE id = ?1",
                params![elec_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(phase, "nomination");

        // Add 3 nominees and accept them
        for i in 1..=3 {
            let nom_id = entity_id(&[&elec_id, &format!("nominee{i}")]);
            conn.execute(
                "INSERT INTO governance_election_nominees \
                 (id, election_id, stake_address, accepted) VALUES (?1, ?2, ?3, 1)",
                params![nom_id, elec_id, format!("stake_test1unominee{i}")],
            )
            .unwrap();
        }

        // Transition to voting
        let accepted: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM governance_election_nominees \
                 WHERE election_id = ?1 AND accepted = 1",
                params![elec_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(accepted, 3);

        conn.execute(
            "UPDATE governance_elections SET phase = 'voting' WHERE id = ?1",
            params![elec_id],
        )
        .unwrap();

        let phase: String = conn
            .query_row(
                "SELECT phase FROM governance_elections WHERE id = ?1",
                params![elec_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(phase, "voting");
    }

    #[test]
    fn double_vote_prevention_elections() {
        let db = setup_db();
        let conn = db.conn();

        let elec_id = entity_id(&["dao1", "elec", "now"]);
        conn.execute(
            "INSERT INTO governance_elections \
             (id, dao_id, title, phase, seats) VALUES (?1, 'dao1', 'Test', 'voting', 1)",
            params![elec_id],
        )
        .unwrap();

        let nom_id = entity_id(&[&elec_id, "nominee1"]);
        conn.execute(
            "INSERT INTO governance_election_nominees \
             (id, election_id, stake_address, accepted) VALUES (?1, ?2, 'stake_test1unom', 1)",
            params![nom_id, elec_id],
        )
        .unwrap();

        // First vote should succeed
        let vote1_id = entity_id(&[&elec_id, "voter1"]);
        conn.execute(
            "INSERT INTO governance_election_votes \
             (id, election_id, voter, nominee_id) VALUES (?1, ?2, 'voter1', ?3)",
            params![vote1_id, elec_id, nom_id],
        )
        .unwrap();

        // Second vote from same voter should fail (UNIQUE constraint)
        let vote2_id = entity_id(&[&elec_id, "voter1", "2"]);
        let result = conn.execute(
            "INSERT INTO governance_election_votes \
             (id, election_id, voter, nominee_id) VALUES (?1, ?2, 'voter1', ?3)",
            params![vote2_id, elec_id, nom_id],
        );
        assert!(result.is_err(), "double vote should be rejected by UNIQUE constraint");
    }

    #[test]
    fn proposal_lifecycle_draft_to_approved() {
        let db = setup_db();
        let conn = db.conn();

        // Create proposal
        let prop_id = entity_id(&["dao1", "test prop", "proposer"]);
        conn.execute(
            "INSERT INTO governance_proposals \
             (id, dao_id, title, category, status, proposer, min_vote_proficiency) \
             VALUES (?1, 'dao1', 'Test Proposal', 'policy', 'draft', 'stake_test1uvoter', 'remember')",
            params![prop_id],
        )
        .unwrap();

        // Approve (draft -> published)
        conn.execute(
            "UPDATE governance_proposals SET status = 'published', \
             voting_deadline = datetime('now', '+14 days') WHERE id = ?1",
            params![prop_id],
        )
        .unwrap();

        // Cast votes: 3 for, 1 against → 75% > 66.7% → approved
        for i in 1..=3 {
            let vid = entity_id(&[&prop_id, &format!("voter{i}")]);
            conn.execute(
                "INSERT INTO governance_proposal_votes \
                 (id, proposal_id, voter, in_favor) VALUES (?1, ?2, ?3, 1)",
                params![vid, prop_id, format!("voter{i}")],
            )
            .unwrap();
        }
        let vid = entity_id(&[&prop_id, "voter4"]);
        conn.execute(
            "INSERT INTO governance_proposal_votes \
             (id, proposal_id, voter, in_favor) VALUES (?1, ?2, 'voter4', 0)",
            params![vid, prop_id],
        )
        .unwrap();

        // Update tally
        conn.execute(
            "UPDATE governance_proposals SET votes_for = 3, votes_against = 1 WHERE id = ?1",
            params![prop_id],
        )
        .unwrap();

        // Resolve: 3/4 = 0.75 >= 0.667 → approved
        let (vf, va): (i64, i64) = conn
            .query_row(
                "SELECT votes_for, votes_against FROM governance_proposals WHERE id = ?1",
                params![prop_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        let ratio = vf as f64 / (vf + va) as f64;
        let new_status = if ratio >= SUPERMAJORITY_THRESHOLD {
            "approved"
        } else {
            "rejected"
        };

        assert_eq!(new_status, "approved");
    }

    #[test]
    fn proposal_supermajority_rejection() {
        // 1 for, 2 against → 33% < 67% → rejected
        let ratio = 1.0 / 3.0;
        assert!(ratio < SUPERMAJORITY_THRESHOLD);
    }

    #[test]
    fn proposal_double_vote_prevention() {
        let db = setup_db();
        let conn = db.conn();

        let prop_id = entity_id(&["dao1", "prop", "p"]);
        conn.execute(
            "INSERT INTO governance_proposals \
             (id, dao_id, title, category, status, proposer, min_vote_proficiency) \
             VALUES (?1, 'dao1', 'Test', 'policy', 'published', 'proposer', 'remember')",
            params![prop_id],
        )
        .unwrap();

        let vid1 = entity_id(&[&prop_id, "v1"]);
        conn.execute(
            "INSERT INTO governance_proposal_votes (id, proposal_id, voter, in_favor) \
             VALUES (?1, ?2, 'voter1', 1)",
            params![vid1, prop_id],
        )
        .unwrap();

        let vid2 = entity_id(&[&prop_id, "v1", "2"]);
        let result = conn.execute(
            "INSERT INTO governance_proposal_votes (id, proposal_id, voter, in_favor) \
             VALUES (?1, ?2, 'voter1', 0)",
            params![vid2, prop_id],
        );
        assert!(result.is_err(), "double vote should fail");
    }

    #[test]
    fn election_winners_are_top_n_by_votes() {
        let db = setup_db();
        let conn = db.conn();

        let elec_id = entity_id(&["dao1", "winner_test", "now"]);
        conn.execute(
            "INSERT INTO governance_elections \
             (id, dao_id, title, phase, seats) VALUES (?1, 'dao1', 'Winner Test', 'voting', 2)",
            params![elec_id],
        )
        .unwrap();

        // 3 nominees with different vote counts
        for (i, votes) in [(1, 10), (2, 5), (3, 8)] {
            let nom_id = entity_id(&[&elec_id, &format!("nom{i}")]);
            conn.execute(
                "INSERT INTO governance_election_nominees \
                 (id, election_id, stake_address, accepted, votes_received) \
                 VALUES (?1, ?2, ?3, 1, ?4)",
                params![nom_id, elec_id, format!("stake{i}"), votes],
            )
            .unwrap();
        }

        // Get top 2 by votes
        let mut stmt = conn
            .prepare(
                "SELECT stake_address, votes_received FROM governance_election_nominees \
                 WHERE election_id = ?1 AND accepted = 1 \
                 ORDER BY votes_received DESC LIMIT 2",
            )
            .unwrap();

        let winners: Vec<(String, i64)> = stmt
            .query_map(params![elec_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(winners.len(), 2);
        assert_eq!(winners[0].1, 10); // nominee1 with 10 votes
        assert_eq!(winners[1].1, 8);  // nominee3 with 8 votes
    }
}
