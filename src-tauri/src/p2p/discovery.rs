use std::collections::HashSet;
use std::sync::RwLock;

use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};

use crate::network_profile::{embedded_preprod, RelayProfile};

/// User-configured additional relays (federation step 1): anyone can
/// run `alexandria-relay` and point their node at it via the
/// `p2p.extra_relays` setting. Loaded at `p2p_start`; merged into every
/// discovery surface below alongside the relays in the active network profile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExtraRelay {
    pub peer_id: String,
    pub host: String,
    pub port: u16,
}

static EXTRA_RELAYS: RwLock<Vec<ExtraRelay>> = RwLock::new(Vec::new());

/// Replace the extra-relay set (called from `p2p_start` with the
/// parsed `p2p.extra_relays` setting). Invalid entries are dropped.
pub fn set_extra_relays(relays: Vec<ExtraRelay>) {
    let valid: Vec<ExtraRelay> = relays
        .into_iter()
        .filter(|r| r.peer_id.parse::<PeerId>().is_ok() && !r.host.is_empty() && r.port > 0)
        .collect();
    if let Ok(mut guard) = EXTRA_RELAYS.write() {
        *guard = valid;
    }
}

fn extra_relays() -> Vec<ExtraRelay> {
    EXTRA_RELAYS.read().map(|g| g.clone()).unwrap_or_default()
}

fn built_in_relays() -> &'static [RelayProfile] {
    &embedded_preprod()
        .expect("embedded network profile is validated during app setup")
        .relays
}

/// Relay HTTPS registry origins. Used by
/// the signup-time username availability check, which runs before the
/// profile (and thus the P2P identity) exists.
pub fn relay_http_endpoints() -> Vec<String> {
    built_in_relays()
        .iter()
        .map(|relay| relay.registry_https_origin.clone())
        .collect()
}

/// Return the set of all configured relay PeerIds.
///
/// Used by the event loop to identify relay peers after Identify handshake
/// and trigger relay reservation + Kademlia bootstrap.
pub fn relay_peer_ids() -> HashSet<PeerId> {
    built_in_relays()
        .iter()
        .filter_map(|r| r.peer_id.parse().ok())
        .chain(extra_relays().iter().filter_map(|r| r.peer_id.parse().ok()))
        .collect()
}

/// Build circuit relay listen addresses for all configured relays.
///
/// Returns multiaddrs like:
/// `/dns4/{host}/tcp/{port}/p2p/{relay_peer_id}/p2p-circuit`
///
/// When passed to `Swarm::listen_on`, each tells the relay client to
/// connect to that relay and request a circuit reservation so other
/// NATted peers can reach us through it.
pub fn relay_circuit_addrs() -> Vec<Multiaddr> {
    built_in_relays()
        .iter()
        .filter_map(|r| {
            format!(
                "/dns4/{}/tcp/{}/p2p/{}/p2p-circuit",
                r.dns_name, r.port, r.peer_id
            )
            .parse()
            .ok()
        })
        .chain(extra_relays().iter().filter_map(|r| {
            format!(
                "/dns4/{}/tcp/{}/p2p/{}/p2p-circuit",
                r.host, r.port, r.peer_id
            )
            .parse()
            .ok()
        }))
        .collect()
}

/// Build the circuit address for a specific relay peer.
pub fn relay_circuit_addr_for(peer_id: &PeerId) -> Option<Multiaddr> {
    let pid_str = peer_id.to_string();
    if let Some(r) = built_in_relays().iter().find(|r| r.peer_id == pid_str) {
        return format!(
            "/dns4/{}/tcp/{}/p2p/{}/p2p-circuit",
            r.dns_name, r.port, r.peer_id
        )
        .parse()
        .ok();
    }
    extra_relays()
        .iter()
        .find(|r| r.peer_id == pid_str)
        .and_then(|r| {
            format!(
                "/dns4/{}/tcp/{}/p2p/{}/p2p-circuit",
                r.host, r.port, r.peer_id
            )
            .parse()
            .ok()
        })
}

/// Build relay-circuit dial addresses for a discovered destination peer.
///
/// These are the addresses other peers can use when the destination has an
/// active relay reservation but has not yet advertised a directly dialable
/// public address.
///
/// Emits one address per relay (DNS variant only), not one-per-transport-
/// variant. Each address triggers a separate circuit reservation attempt
/// against the relay, which counts toward the relay's per-source-peer
/// circuit limit. Earlier versions also fanned out an `/ip4/.../p2p-circuit`
/// variant for every relay; that doubled the load on the relay for no
/// real benefit (libp2p already has an open connection to the relay by the
/// time we reach this codepath, so the circuit dial reuses it). DNS-only
/// keeps relay pressure proportional to the number of *relays*, not
/// `relays × transport_variants`.
pub fn relay_circuit_dial_addrs(peer_id: &PeerId) -> Vec<Multiaddr> {
    built_in_relays()
        .iter()
        .filter_map(|relay| {
            format!(
                "/dns4/{}/tcp/{}/p2p/{}/p2p-circuit/p2p/{}",
                relay.dns_name, relay.port, relay.peer_id, peer_id
            )
            .parse::<Multiaddr>()
            .ok()
        })
        .chain(extra_relays().into_iter().filter_map(|relay| {
            format!(
                "/dns4/{}/tcp/{}/p2p/{}/p2p-circuit/p2p/{}",
                relay.host, relay.port, relay.peer_id, peer_id
            )
            .parse::<Multiaddr>()
            .ok()
        }))
        .collect()
}

pub fn bootstrap_peers() -> Vec<Multiaddr> {
    let mut addrs = Vec::new();

    for relay in built_in_relays() {
        // TCP via DNS
        if let Ok(addr) = format!(
            "/dns4/{}/tcp/{}/p2p/{}",
            relay.dns_name, relay.port, relay.peer_id
        )
        .parse::<Multiaddr>()
        {
            addrs.push(addr);
        }

        // QUIC via DNS
        if let Ok(addr) = format!(
            "/dns4/{}/udp/{}/quic-v1/p2p/{}",
            relay.dns_name, relay.port, relay.peer_id
        )
        .parse::<Multiaddr>()
        {
            addrs.push(addr);
        }

        for ip in &relay.fallback_ips {
            let family = if ip.is_ipv4() { "ip4" } else { "ip6" };
            for transport in [
                format!("/{family}/{ip}/tcp/{}/p2p/{}", relay.port, relay.peer_id),
                format!(
                    "/{family}/{ip}/udp/{}/quic-v1/p2p/{}",
                    relay.port, relay.peer_id
                ),
            ] {
                if let Ok(addr) = transport.parse::<Multiaddr>() {
                    addrs.push(addr);
                }
            }
        }
    }

    for relay in extra_relays() {
        for proto in [
            format!(
                "/dns4/{}/tcp/{}/p2p/{}",
                relay.host, relay.port, relay.peer_id
            ),
            format!(
                "/dns4/{}/udp/{}/quic-v1/p2p/{}",
                relay.host, relay.port, relay.peer_id
            ),
        ] {
            if let Ok(addr) = proto.parse::<Multiaddr>() {
                addrs.push(addr);
            }
        }
    }

    if addrs.is_empty() {
        log::warn!("No relay peers configured — no bootstrap peers available");
    }

    addrs
}

/// Derive the CID key used for Kademlia provider records.
///
/// All Alexandria nodes publish a provider record for this key.
/// To discover other Alexandria peers, query `get_providers(namespace_key())`.
///
/// The key is a SHA-256 hash bound to the configured network namespace.
/// On the private DHT, every node
/// is an Alexandria node, but provider records still allow targeted discovery
/// of nodes that are actively providing content.
pub fn namespace_key() -> libp2p::kad::RecordKey {
    use sha2::{Digest, Sha256};
    let profile = embedded_preprod().expect("embedded network profile is valid");
    let hash = Sha256::digest(format!("{}.providers", profile.protocol_namespace));
    libp2p::kad::RecordKey::new(&hash)
}

/// Provider-record key under which contributing (publicly reachable)
/// nodes advertise themselves as circuit relays. Distinct from
/// [`namespace_key`] so clients can discover *relays* specifically and
/// request reservations from them, rather than learning every peer.
pub fn relay_namespace_key() -> libp2p::kad::RecordKey {
    use sha2::{Digest, Sha256};
    let profile = embedded_preprod().expect("embedded network profile is valid");
    let hash = Sha256::digest(format!("{}.relays", profile.protocol_namespace));
    libp2p::kad::RecordKey::new(&hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_peers_returns_all_relay_addrs() {
        let peers = bootstrap_peers();
        // 4 addrs per relay (DNS TCP/QUIC + IPv4 TCP/QUIC), 2 relays
        assert_eq!(peers.len(), 8, "should return 4 addrs per relay * 2 relays");
        // First relay addresses
        assert!(peers[0].to_string().contains("alexandria-relay.fly.dev"));
        // Second relay addresses
        assert!(peers[4].to_string().contains("alexandria-relay-eu.fly.dev"));
    }

    #[test]
    fn registry_lookup_uses_https_profile_origins_only() {
        let endpoints = relay_http_endpoints();
        assert_eq!(endpoints.len(), 2);
        assert!(endpoints
            .iter()
            .all(|endpoint| endpoint.starts_with("https://")));
        assert!(endpoints.iter().all(|endpoint| !endpoint.contains(":9090")));
    }

    #[test]
    fn namespace_key_is_deterministic() {
        let k1 = namespace_key();
        let k2 = namespace_key();
        assert_eq!(k1, k2);
    }

    #[test]
    fn relay_peer_ids_returns_all() {
        let ids = relay_peer_ids();
        assert_eq!(ids.len(), 2, "should have 2 relay peer IDs");
        for id in &ids {
            assert!(
                id.to_string().starts_with("12D3KooW"),
                "should be a valid Ed25519 PeerId"
            );
        }
    }

    #[test]
    fn relay_circuit_addrs_returns_all() {
        let addrs = relay_circuit_addrs();
        assert_eq!(addrs.len(), 2, "should have circuit addr per relay");
        for addr in &addrs {
            let s = addr.to_string();
            assert!(s.contains("p2p-circuit"), "should contain p2p-circuit");
        }
    }

    #[test]
    fn relay_circuit_addr_for_known_peer() {
        let ids = relay_peer_ids();
        for id in &ids {
            let addr = relay_circuit_addr_for(id);
            assert!(addr.is_some(), "should find circuit addr for known relay");
            assert!(addr.unwrap().to_string().contains("p2p-circuit"));
        }
    }

    #[test]
    fn relay_circuit_addr_for_unknown_peer() {
        let unknown: PeerId = "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
            .parse()
            .unwrap();
        assert!(relay_circuit_addr_for(&unknown).is_none());
    }

    #[test]
    fn relay_circuit_dial_addrs_returns_one_per_relay() {
        let peer: PeerId = "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
            .parse()
            .unwrap();
        let addrs = relay_circuit_dial_addrs(&peer);
        assert_eq!(
            addrs.len(),
            2,
            "should return one DNS circuit addr per relay (no IPv4 fanout — keeps relay per-peer circuit limit headroom)"
        );
        assert!(addrs
            .iter()
            .all(|addr| addr.to_string().contains("p2p-circuit")));
        assert!(addrs.iter().all(|addr| addr.to_string().contains("/dns4/")));
        assert!(addrs
            .iter()
            .any(|addr| addr.to_string().contains("alexandria-relay.fly.dev")));
        assert!(addrs
            .iter()
            .any(|addr| addr.to_string().contains("alexandria-relay-eu.fly.dev")));
    }
}
