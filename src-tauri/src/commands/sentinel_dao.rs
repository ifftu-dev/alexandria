//! Sentinel DAO — thin wrapper exposing the seeded Sentinel DAO row with
//! committee membership and the list of proposal categories it recognizes.
//!
//! The Sentinel DAO governs adversarial-prior curation for federated
//! anti-cheat training (see `docs/sentinel-adversarial-priors.md`). The
//! underlying DAO machinery (propose / vote / ratify / elect) is reused
//! verbatim from `commands::governance`; this module only provides a
//! convenient read path for UI code that wants DAO metadata without caring
//! about generic DAO listing.

use rusqlite::params;
use serde::Serialize;
use tauri::State;

use crate::domain::governance::{DaoInfo, DaoMember};
use crate::AppState;

/// Stable identifier for the Sentinel DAO row seeded by migration 037.
pub const SENTINEL_DAO_ID: &str = "sentinel-dao";

/// Proposal category clients must use when submitting adversarial-prior
/// candidates to the Sentinel DAO. Enforced on the typed propose path
/// (Phase 2); `governance::submit_proposal` itself stores free-form
/// categories, but UI code should always use this constant.
pub const SENTINEL_PRIOR_CATEGORY: &str = "sentinel_prior";

#[derive(Debug, Serialize)]
pub struct SentinelDaoInfo {
    pub dao: DaoInfo,
    pub committee: Vec<DaoMember>,
    /// Canonical list of `governance_proposals.category` values the
    /// Sentinel DAO will accept. Clients should reject anything else
    /// before submission to avoid wasted ratification rounds.
    pub recognized_categories: Vec<String>,
}

/// Read the seeded Sentinel DAO row plus its current committee.
///
/// Returns an error if migration 037 has not been applied. Committee may
/// be empty if the bootstrap election has not yet run — that's an
/// expected transient state, not an error.
#[tauri::command]
pub async fn sentinel_dao_get_info(state: State<'_, AppState>) -> Result<SentinelDaoInfo, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let conn = db.conn();

    let dao = conn
        .query_row(
            "SELECT id, name, description, icon_emoji, scope_type, scope_id, status,
                    committee_size, election_interval_days, on_chain_tx, created_at, updated_at
             FROM governance_daos WHERE id = ?1",
            params![SENTINEL_DAO_ID],
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
        .map_err(|e| format!("Sentinel DAO not found (has migration 037 run?): {e}"))?;

    let mut stmt = conn
        .prepare(
            "SELECT dao_id, stake_address, role, joined_at
             FROM governance_dao_members WHERE dao_id = ?1
             ORDER BY joined_at ASC",
        )
        .map_err(|e| e.to_string())?;

    let committee = stmt
        .query_map(params![SENTINEL_DAO_ID], |row| {
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

    Ok(SentinelDaoInfo {
        dao,
        committee,
        recognized_categories: vec![SENTINEL_PRIOR_CATEGORY.to_string()],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_stable() {
        // Guard against accidental rename — clients hard-code these.
        assert_eq!(SENTINEL_DAO_ID, "sentinel-dao");
        assert_eq!(SENTINEL_PRIOR_CATEGORY, "sentinel_prior");
    }
}
