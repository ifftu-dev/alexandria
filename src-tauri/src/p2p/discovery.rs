use libp2p::Multiaddr;

/// Hardcoded bootstrap nodes for the Alexandria P2P network.
///
/// These are well-known peers run by the Alexandria non-profit and
/// community volunteers. They serve as initial contact points for
/// Kademlia DHT queries and GossipSub relay.
///
/// Bootstrap nodes have NO special authority — they are peers like
/// any other. They can be turned off without affecting the network
/// once enough organic peers exist.
///
/// For preprod/development, this list is empty. In production, it
/// will contain multiaddresses of community-run bootstrap nodes.
pub fn bootstrap_peers() -> Vec<Multiaddr> {
    // TODO: Add community bootstrap nodes for preprod testnet.
    // Format: "/ip4/<IP>/udp/<PORT>/quic-v1/p2p/<PEER_ID>"
    //
    // Example:
    // "/ip4/34.123.45.67/udp/4001/quic-v1/p2p/12D3KooW..."
    //     .parse()
    //     .expect("valid bootstrap addr"),
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_peers_returns_valid_list() {
        let peers = bootstrap_peers();
        // Currently empty for development
        for addr in &peers {
            // Each address should be a valid multiaddr
            let s = addr.to_string();
            assert!(s.starts_with('/'), "multiaddr should start with /");
        }
        // Intentionally no assertion on length — empty is valid for dev
    }
}
