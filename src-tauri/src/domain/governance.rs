//! Governance domain values that remain after retiring the obsolete local and
//! operator DAO authority.
//!
//! Local elections, nominations, committee installation, proposals and the
//! operator Cardano governance queue are deleted. The replacement protocol
//! consumes verified committee certificates bound to an explicitly pinned
//! genesis (see `governance_certificate`). Until it lands, inbound governance
//! gossip is rejected before any database access.

use serde::{Deserialize, Serialize};

/// Returned for inbound governance gossip while the replacement committee
/// certificate protocol is not implemented.
pub const LEGACY_GOVERNANCE_DISABLED: &str =
    "legacy governance is disabled pending verified committee certificates";

/// A DAO scope row from the local `governance_daos` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaoInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon_emoji: Option<String>,
    pub scope_type: String,
    pub scope_id: String,
    pub status: String,
    pub committee_size: i64,
    pub election_interval_days: i64,
    pub on_chain_tx: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A DAO member row from the local `governance_dao_members` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaoMember {
    pub dao_id: String,
    pub stake_address: String,
    pub role: String,
    pub joined_at: String,
}
