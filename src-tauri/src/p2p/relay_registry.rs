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
//! Authority = the network-profile receipt issuers plus a Cardano-anchored
//! registry governed by a governance key (later a DAO). The on-chain
//! set is layered in by the relay-registry reader; until it lands,
//! genesis is the sole authority. On-chain entries only *add* issuers —
//! genesis stays trusted so naming keeps working if the chain is
//! unreachable.

use std::collections::HashSet;
use std::sync::RwLock;

use libp2p::PeerId;

use crate::network_profile::embedded_preprod;

/// Cardano transaction-metadata label carrying the relay registry.
/// Sits alongside the credential (1697) and username (1698) anchors.
pub const REGISTRY_LABEL: u64 = 1699;

pub fn governance_anchor_address() -> Option<&'static str> {
    embedded_preprod()
        .expect("embedded network profile is validated during app setup")
        .optional_governance_anchor_address
        .as_deref()
}

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
    if embedded_preprod()
        .expect("embedded network profile is valid")
        .receipt_issuer_keys
        .iter()
        .any(|issuer| issuer == &s)
    {
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
    let mut set: HashSet<String> = embedded_preprod()
        .expect("embedded network profile is valid")
        .receipt_issuer_keys
        .iter()
        .cloned()
        .collect();
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
        for g in &embedded_preprod().unwrap().receipt_issuer_keys {
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
