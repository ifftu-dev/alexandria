//! Authorized username-receipt issuers.
//!
//! A relay's username receipt only counts toward a claim's trust tier
//! and conflict ordering if the relay is an *authorized issuer*. This
//! set is deliberately NARROWER than the connectivity relay set
//! ([`super::discovery::relay_peer_ids`], which anyone may extend via
//! `extra_relays`): circuit-relay and DHT service are open to any
//! community relay, but naming trust is not — otherwise a Sybil relay
//! could forge handle ownership simply by issuing receipts.
//!
//! Authority = the GENESIS operator relays plus a Cardano-anchored
//! registry governed by a governance key (later a DAO). The on-chain
//! set is layered in by the relay-registry reader; until it lands,
//! genesis is the sole authority. On-chain entries only *add* issuers —
//! genesis stays trusted so naming keeps working if the chain is
//! unreachable.

use std::collections::HashSet;
use std::sync::RwLock;

use libp2p::PeerId;

/// Project-operated relays trusted to issue receipts before the
/// on-chain registry exists. MUST stay a subset of the hardcoded
/// `discovery::RELAYS` operator nodes — never `extra_relays`.
const GENESIS_ISSUERS: &[&str] = &[
    "12D3KooWENHQjSydcHUXVTuq4wVNvCP4VGXzxueBtdKi1D3mS6wR", // Mumbai
    "12D3KooWFDVfPBwa6EVEp8v8cqXpgmiksV7qMarHCYLF174XV9xj", // Frankfurt
];

/// Cardano transaction-metadata label carrying the relay registry.
/// Sits alongside the credential (1697) and username (1698) anchors.
pub const REGISTRY_LABEL: u64 = 1699;

/// Governance address (preprod). A label-[`REGISTRY_LABEL`] metadata tx
/// is only honoured as a registry update if it was authored by this
/// address (one of its UTxOs is spent as an input — only the gov key
/// can do that). Phase 1: a dedicated gov key; migrates to a DAO script
/// address later. Clients pin it so no single relay can rewrite the set.
pub const GOV_ADDRESS: &str = "addr_test1vzdrft6lj8p2ca7t0ru0wc3tsjtgcws2cyhaa3zemw7hgechm5qry";

/// On-chain / cached authorized issuers, layered over genesis.
/// Populated by the Cardano relay-registry reader once it has fetched
/// and verified the governance-signed registry. Empty == genesis only.
static ONCHAIN_ISSUERS: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// Replace the on-chain issuer set. Called by the relay-registry reader
/// after it verifies the governance-signed registry transaction. Entries
/// are relay PeerId strings.
pub fn set_onchain_issuers(issuers: Vec<String>) {
    if let Ok(mut g) = ONCHAIN_ISSUERS.write() {
        *g = issuers;
    }
}

/// `true` if this relay PeerId may issue authoritative username receipts
/// (genesis ∪ on-chain registry). Connectivity trust is a separate,
/// broader decision — see [`super::discovery::relay_peer_ids`].
pub fn is_authorized_issuer(peer_id: &PeerId) -> bool {
    let s = peer_id.to_string();
    if GENESIS_ISSUERS.contains(&s.as_str()) {
        return true;
    }
    ONCHAIN_ISSUERS
        .read()
        .map(|g| g.iter().any(|x| x == &s))
        .unwrap_or(false)
}

/// The full authorized-issuer set (genesis ∪ on-chain), for diagnostics
/// and UI surfaces.
pub fn authorized_issuers() -> HashSet<String> {
    let mut set: HashSet<String> = GENESIS_ISSUERS.iter().map(|s| s.to_string()).collect();
    if let Ok(g) = ONCHAIN_ISSUERS.read() {
        set.extend(g.iter().cloned());
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn random_peer() -> PeerId {
        libp2p::identity::Keypair::generate_ed25519()
            .public()
            .to_peer_id()
    }

    #[test]
    fn genesis_relays_are_authorized() {
        for g in GENESIS_ISSUERS {
            let pid: PeerId = g.parse().expect("genesis peer id parses");
            assert!(is_authorized_issuer(&pid));
        }
    }

    #[test]
    fn random_relay_is_not_authorized() {
        // A community / extra relay (not genesis, not on-chain) must not
        // be trusted to issue receipts even though it may serve as a
        // connectivity relay.
        assert!(!is_authorized_issuer(&random_peer()));
    }

    #[test]
    fn onchain_issuer_becomes_authorized() {
        let pid = random_peer();
        assert!(!is_authorized_issuer(&pid));
        set_onchain_issuers(vec![pid.to_string()]);
        assert!(is_authorized_issuer(&pid));
        // Cleanup so other tests see a clean on-chain set.
        set_onchain_issuers(Vec::new());
        assert!(!is_authorized_issuer(&pid));
    }
}
