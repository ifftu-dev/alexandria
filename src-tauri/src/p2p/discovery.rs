use std::collections::HashSet;

use libp2p::{Multiaddr, PeerId};

/// Alexandria relay bootstrap nodes.
///
/// These are the entry points into the private Alexandria Kademlia DHT
/// (`/alexandria/kad/1.0`). Relays run Circuit Relay v2 so NATted peers
/// (phones, laptops behind routers) can connect through them.
///
/// Relays have NO special authority — they cannot read encrypted traffic,
/// forge identities, or censor content. They're dumb pipes + phonebooks.
///
/// ## Adding a new relay
///
/// 1. Deploy `alexandria-relay` to a new Fly.io region
/// 2. Run `alexandria-relay --generate-key` to get a deterministic PeerId
/// 3. Set `RELAY_SEED` env var on the server
/// 4. Add the new relay's info to the `RELAYS` array below
///
struct RelayInfo {
    peer_id: &'static str,
    host: &'static str,
    ipv4: &'static str,
    port: u16,
}

/// All known relay nodes. The client bootstraps to all of them and
/// requests circuit relay reservations from each.
const RELAYS: &[RelayInfo] = &[
    // Mumbai (primary)
    RelayInfo {
        peer_id: "12D3KooWENHQjSydcHUXVTuq4wVNvCP4VGXzxueBtdKi1D3mS6wR",
        host: "alexandria-relay.fly.dev",
        ipv4: "168.220.86.30",
        port: 4001,
    },
    // Frankfurt (EU)
    RelayInfo {
        peer_id: "12D3KooWFDVfPBwa6EVEp8v8cqXpgmiksV7qMarHCYLF174XV9xj",
        host: "alexandria-relay-eu.fly.dev",
        ipv4: "66.51.123.68",
        port: 4001,
    },
];

/// Return the set of all configured relay PeerIds.
///
/// Used by the event loop to identify relay peers after Identify handshake
/// and trigger relay reservation + Kademlia bootstrap.
pub fn relay_peer_ids() -> HashSet<PeerId> {
    RELAYS
        .iter()
        .filter(|r| !r.peer_id.starts_with("PLACEHOLDER"))
        .filter_map(|r| r.peer_id.parse().ok())
        .collect()
}

/// Build circuit relay listen addresses for all configured relays.
///
/// Returns multiaddrs like:
/// `/ip4/{ip}/tcp/{port}/p2p/{relay_peer_id}/p2p-circuit`
///
/// When passed to `Swarm::listen_on`, each tells the relay client to
/// connect to that relay and request a circuit reservation so other
/// NATted peers can reach us through it.
pub fn relay_circuit_addrs() -> Vec<Multiaddr> {
    RELAYS
        .iter()
        .filter(|r| !r.peer_id.starts_with("PLACEHOLDER"))
        .filter_map(|r| {
            format!(
                "/ip4/{}/tcp/{}/p2p/{}/p2p-circuit",
                r.ipv4, r.port, r.peer_id
            )
            .parse()
            .ok()
        })
        .collect()
}

/// Build the circuit address for a specific relay peer.
pub fn relay_circuit_addr_for(peer_id: &PeerId) -> Option<Multiaddr> {
    let pid_str = peer_id.to_string();
    RELAYS.iter().find(|r| r.peer_id == pid_str).and_then(|r| {
        format!(
            "/ip4/{}/tcp/{}/p2p/{}/p2p-circuit",
            r.ipv4, r.port, r.peer_id
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
    RELAYS
        .iter()
        .filter(|r| !r.peer_id.starts_with("PLACEHOLDER"))
        .filter_map(|relay| {
            format!(
                "/dns4/{}/tcp/{}/p2p/{}/p2p-circuit/p2p/{}",
                relay.host, relay.port, relay.peer_id, peer_id
            )
            .parse::<Multiaddr>()
            .ok()
        })
        .collect()
}

pub fn bootstrap_peers() -> Vec<Multiaddr> {
    let mut addrs = Vec::new();

    for relay in RELAYS {
        if relay.peer_id.starts_with("PLACEHOLDER") {
            continue;
        }

        // TCP via DNS
        if let Ok(addr) = format!(
            "/dns4/{}/tcp/{}/p2p/{}",
            relay.host, relay.port, relay.peer_id
        )
        .parse::<Multiaddr>()
        {
            addrs.push(addr);
        }

        // QUIC via DNS
        if let Ok(addr) = format!(
            "/dns4/{}/udp/{}/quic-v1/p2p/{}",
            relay.host, relay.port, relay.peer_id
        )
        .parse::<Multiaddr>()
        {
            addrs.push(addr);
        }

        // Direct IPv4 TCP (fallback — DNS resolution can fail on some mobile networks)
        if let Ok(addr) = format!(
            "/ip4/{}/tcp/{}/p2p/{}",
            relay.ipv4, relay.port, relay.peer_id
        )
        .parse::<Multiaddr>()
        {
            addrs.push(addr);
        }

        // Direct IPv4 QUIC (fallback)
        if let Ok(addr) = format!(
            "/ip4/{}/udp/{}/quic-v1/p2p/{}",
            relay.ipv4, relay.port, relay.peer_id
        )
        .parse::<Multiaddr>()
        {
            addrs.push(addr);
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
/// The key is the SHA-256 hash of the namespace string "ifftu.alexandria",
/// which is a valid Kademlia record key. On the private DHT, every node
/// is an Alexandria node, but provider records still allow targeted discovery
/// of nodes that are actively providing content.
pub fn namespace_key() -> libp2p::kad::RecordKey {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(b"ifftu.alexandria");
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
