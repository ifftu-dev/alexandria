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

/// All known relay nodes. The client will bootstrap to all of them.
/// The first relay is used as the primary circuit relay for NAT traversal.
const RELAYS: &[RelayInfo] = &[
    // Mumbai (primary)
    RelayInfo {
        peer_id: "12D3KooWENHQjSydcHUXVTuq4wVNvCP4VGXzxueBtdKi1D3mS6wR",
        host: "alexandria-relay.fly.dev",
        ipv4: "168.220.86.30",
        port: 4001,
    },
    // Frankfurt (EU) — placeholder PeerId, update after first deploy
    // RelayInfo {
    //     peer_id: "PLACEHOLDER_EU_PEER_ID",
    //     host: "alexandria-relay-eu.fly.dev",
    //     ipv4: "PLACEHOLDER_EU_IPV4",
    //     port: 4001,
    // },
];

/// Legacy constants for backward compatibility with code that references them directly.
const RELAY_PEER_ID: &str = "12D3KooWENHQjSydcHUXVTuq4wVNvCP4VGXzxueBtdKi1D3mS6wR";
const RELAY_IPV4: &str = "168.220.86.30";
const RELAY_PORT: u16 = 4001;

/// Return the relay's PeerId if configured (not placeholder).
///
/// Used by the event loop to identify the relay after Identify handshake
/// and trigger relay reservation + Kademlia bootstrap.
pub fn relay_peer_id() -> Option<PeerId> {
    if RELAY_PEER_ID == "PLACEHOLDER_PEER_ID" {
        return None;
    }
    RELAY_PEER_ID.parse().ok()
}

/// Build the relay circuit listen address for requesting a reservation.
///
/// Returns a full multiaddr like:
/// `/ip4/{ip}/tcp/4001/p2p/{relay_peer_id}/p2p-circuit`
///
/// When passed to `Swarm::listen_on`, this tells the relay client to
/// connect to the relay and request a circuit reservation so other
/// NATted peers can reach us through the relay.
pub fn relay_circuit_addr() -> Option<Multiaddr> {
    let _relay_pid = relay_peer_id()?;
    // Use the direct IPv4 address for the circuit (most reliable)
    format!("/ip4/{RELAY_IPV4}/tcp/{RELAY_PORT}/p2p/{RELAY_PEER_ID}/p2p-circuit")
        .parse()
        .ok()
}

pub fn bootstrap_peers() -> Vec<Multiaddr> {
    let mut addrs = Vec::new();

    for relay in RELAYS {
        if relay.peer_id.starts_with("PLACEHOLDER") {
            continue;
        }

        // TCP via DNS
        if let Ok(addr) =
            format!("/dns4/{}/tcp/{}/p2p/{}", relay.host, relay.port, relay.peer_id)
                .parse::<Multiaddr>()
        {
            addrs.push(addr);
        }

        // QUIC via DNS
        if let Ok(addr) =
            format!("/dns4/{}/udp/{}/quic-v1/p2p/{}", relay.host, relay.port, relay.peer_id)
                .parse::<Multiaddr>()
        {
            addrs.push(addr);
        }

        // Direct IPv4 TCP (fallback — DNS resolution can fail on some mobile networks)
        if let Ok(addr) =
            format!("/ip4/{}/tcp/{}/p2p/{}", relay.ipv4, relay.port, relay.peer_id)
                .parse::<Multiaddr>()
        {
            addrs.push(addr);
        }

        // Direct IPv4 QUIC (fallback)
        if let Ok(addr) =
            format!("/ip4/{}/udp/{}/quic-v1/p2p/{}", relay.ipv4, relay.port, relay.peer_id)
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
    fn bootstrap_peers_returns_relay_addrs() {
        let peers = bootstrap_peers();
        assert_eq!(peers.len(), 4, "should return DNS TCP/QUIC + IPv4 TCP/QUIC");
        for addr in &peers {
            let s = addr.to_string();
            assert!(s.contains(RELAY_PEER_ID), "should contain relay PeerId");
        }
        // First two are DNS, last two are IPv4 fallback
        assert!(peers[0].to_string().contains("alexandria-relay.fly.dev"));
        assert!(peers[2].to_string().contains("168.220.86.30"));
    }

    #[test]
    fn namespace_key_is_deterministic() {
        let k1 = namespace_key();
        let k2 = namespace_key();
        assert_eq!(k1, k2);
    }

    #[test]
    fn relay_peer_id_returns_valid_peer() {
        let pid = relay_peer_id().expect("relay PeerId should be configured");
        assert!(
            pid.to_string().starts_with("12D3KooW"),
            "should be a valid Ed25519 PeerId"
        );
    }

    #[test]
    fn relay_circuit_addr_is_valid() {
        let addr = relay_circuit_addr().expect("relay circuit addr should be configured");
        let s = addr.to_string();
        assert!(s.contains("p2p-circuit"), "should contain p2p-circuit");
        assert!(s.contains(RELAY_PEER_ID), "should contain relay PeerId");
        assert!(s.contains(RELAY_IPV4), "should contain relay IP");
    }
}
