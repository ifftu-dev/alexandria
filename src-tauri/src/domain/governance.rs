//! Governance domain values that remain after retiring the obsolete local and
//! operator DAO authority.
//!
//! Local elections, nominations, committee installation, proposals and the
//! operator Cardano governance queue are deleted. The replacement protocol
//! consumes verified committee certificates bound to an explicitly pinned
//! genesis (see `governance_certificate`). Until it lands, inbound governance
//! gossip is rejected before any database access.

/// Returned for inbound governance gossip while the replacement committee
/// certificate protocol is not implemented.
pub const LEGACY_GOVERNANCE_DISABLED: &str =
    "legacy governance is disabled pending verified committee certificates";
