use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::RwLock;
use std::time::Duration;

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

/// An extra relay with the IP addresses its host resolved to.
///
/// The swarm has no DNS transport (libp2p's resolver reads the system
/// resolver configuration file, which mobile platforms do not have), so
/// every extra-relay address is built from these IPs, the way built-in
/// relays are reached through the profile's `fallback_ips`.
#[derive(Debug, Clone, PartialEq)]
struct ResolvedRelay {
    relay: ExtraRelay,
    ips: Vec<IpAddr>,
}

/// Addresses kept per extra relay: enough for an IPv4 and an IPv6 path,
/// few enough that a hostile DNS answer cannot flood the dialer.
const MAX_IPS_PER_RELAY: usize = 4;

/// How long one extra relay's host lookup may take.
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(10);

static EXTRA_RELAYS: RwLock<Vec<ResolvedRelay>> = RwLock::new(Vec::new());

/// Replace the extra-relay set (called from `p2p_start` with the parsed
/// `p2p.extra_relays` setting, and when the setting is saved). Invalid
/// entries are dropped. Each host is resolved here, once: an IP literal is
/// used as is, a DNS name goes through the system resolver. A host that
/// does not resolve keeps its relay in the set with no addresses until the
/// next apply.
pub async fn apply_extra_relays(relays: Vec<ExtraRelay>) {
    let mut resolved = Vec::new();
    for relay in relays.into_iter().filter(is_valid_extra_relay) {
        let ips = resolve_host(&relay.host, relay.port).await;
        if ips.is_empty() {
            log::warn!(
                "extra relay {}: host '{}' did not resolve; it gets no addresses until the setting is applied again",
                relay.peer_id,
                relay.host
            );
        }
        resolved.push(ResolvedRelay { relay, ips });
    }
    if let Ok(mut guard) = EXTRA_RELAYS.write() {
        *guard = resolved;
    }
}

fn is_valid_extra_relay(r: &ExtraRelay) -> bool {
    r.peer_id.parse::<PeerId>().is_ok() && !r.host.trim().is_empty() && r.port > 0
}

/// Resolve an extra relay's host to at most [`MAX_IPS_PER_RELAY`]
/// distinct IPs, in the resolver's order. An IP literal (IPv6 with or
/// without brackets) needs no lookup.
async fn resolve_host(host: &str, port: u16) -> Vec<IpAddr> {
    let host = host.trim();
    if let Some(ip) = ip_literal(host) {
        return vec![ip];
    }
    match tokio::time::timeout(RESOLVE_TIMEOUT, tokio::net::lookup_host((host, port))).await {
        Ok(Ok(addrs)) => {
            let mut ips: Vec<IpAddr> = Vec::new();
            for addr in addrs {
                if !ips.contains(&addr.ip()) {
                    ips.push(addr.ip());
                }
                if ips.len() == MAX_IPS_PER_RELAY {
                    break;
                }
            }
            ips
        }
        Ok(Err(error)) => {
            log::warn!("extra relay host '{host}' lookup failed: {error}");
            Vec::new()
        }
        Err(_) => {
            log::warn!("extra relay host '{host}' lookup timed out");
            Vec::new()
        }
    }
}

fn ip_literal(host: &str) -> Option<IpAddr> {
    let bare = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    bare.parse().ok()
}

fn extra_relays() -> Vec<ResolvedRelay> {
    EXTRA_RELAYS.read().map(|g| g.clone()).unwrap_or_default()
}

/// `/ip4/{ip}` or `/ip6/{ip}`.
fn ip_prefix(ip: &IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => format!("/ip4/{v4}"),
        IpAddr::V6(v6) => format!("/ip6/{v6}"),
    }
}

/// The TCP address circuits through an extra relay are built on: its first
/// resolved IP, one address per relay (see [`relay_circuit_dial_addrs`]).
fn extra_relay_circuit_base(r: &ResolvedRelay) -> Option<String> {
    let ip = r.ips.first()?;
    Some(format!(
        "{}/tcp/{}/p2p/{}",
        ip_prefix(ip),
        r.relay.port,
        r.relay.peer_id
    ))
}

/// Bootstrap addresses for extra relays: TCP and QUIC on every resolved IP.
fn extra_bootstrap_addrs(relays: &[ResolvedRelay]) -> Vec<Multiaddr> {
    let mut addrs = Vec::new();
    for r in relays {
        for ip in &r.ips {
            let prefix = ip_prefix(ip);
            for transport in [
                format!("{prefix}/tcp/{}/p2p/{}", r.relay.port, r.relay.peer_id),
                format!(
                    "{prefix}/udp/{}/quic-v1/p2p/{}",
                    r.relay.port, r.relay.peer_id
                ),
            ] {
                if let Ok(addr) = transport.parse::<Multiaddr>() {
                    addrs.push(addr);
                }
            }
        }
    }
    addrs
}

/// Circuit listen addresses for extra relays, one per relay with an IP.
fn extra_circuit_addrs(relays: &[ResolvedRelay]) -> Vec<Multiaddr> {
    relays
        .iter()
        .filter_map(extra_relay_circuit_base)
        .filter_map(|base| format!("{base}/p2p-circuit").parse().ok())
        .collect()
}

/// Circuit dial addresses through extra relays to `peer_id`, one per relay.
fn extra_circuit_dial_addrs(relays: &[ResolvedRelay], peer_id: &PeerId) -> Vec<Multiaddr> {
    relays
        .iter()
        .filter_map(extra_relay_circuit_base)
        .filter_map(|base| format!("{base}/p2p-circuit/p2p/{peer_id}").parse().ok())
        .collect()
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
        .chain(
            extra_relays()
                .iter()
                .filter_map(|r| r.relay.peer_id.parse().ok()),
        )
        .collect()
}

/// Build circuit relay listen addresses for all configured relays.
///
/// Returns multiaddrs like
/// `/dns4/{host}/tcp/{port}/p2p/{relay_peer_id}/p2p-circuit` for built-in
/// relays (the bootstrap connection over a fallback IP carries the
/// reservation) and `/ip4/{ip}/tcp/{port}/p2p/{relay_peer_id}/p2p-circuit`
/// (or `/ip6`) for extra relays.
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
        .chain(extra_circuit_addrs(&extra_relays()))
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
        .find(|r| r.relay.peer_id == pid_str)
        .and_then(extra_relay_circuit_base)
        .and_then(|base| format!("{base}/p2p-circuit").parse().ok())
}

/// Build relay-circuit dial addresses for a discovered destination peer.
///
/// These are the addresses other peers can use when the destination has an
/// active relay reservation but has not yet advertised a directly dialable
/// public address.
///
/// Emits one address per relay (DNS variant for built-in relays, the first
/// resolved IP for extra relays), not one-per-transport-
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
        .chain(extra_circuit_dial_addrs(&extra_relays(), peer_id))
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

    addrs.extend(extra_bootstrap_addrs(&extra_relays()));

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

    const EXTRA_PEER: &str = "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";

    fn resolved(host: &str, ips: &[&str]) -> ResolvedRelay {
        ResolvedRelay {
            relay: ExtraRelay {
                peer_id: EXTRA_PEER.to_string(),
                host: host.to_string(),
                port: 4001,
            },
            ips: ips.iter().map(|ip| ip.parse().unwrap()).collect(),
        }
    }

    #[tokio::test]
    async fn ip_literal_hosts_need_no_lookup() {
        assert_eq!(
            resolve_host("203.0.113.7", 4001).await,
            vec!["203.0.113.7".parse::<IpAddr>().unwrap()]
        );
        assert_eq!(
            resolve_host(" 2001:db8::1 ", 4001).await,
            vec!["2001:db8::1".parse::<IpAddr>().unwrap()]
        );
        assert_eq!(
            resolve_host("[2001:db8::1]", 4001).await,
            vec!["2001:db8::1".parse::<IpAddr>().unwrap()]
        );
    }

    #[tokio::test]
    async fn dns_hosts_resolve_through_the_system_resolver() {
        let ips = resolve_host("localhost", 4001).await;
        assert!(!ips.is_empty(), "localhost should resolve");
        assert!(ips.iter().all(|ip| ip.is_loopback()));
        assert!(ips.len() <= MAX_IPS_PER_RELAY);
    }

    #[tokio::test]
    async fn unresolvable_hosts_get_no_addresses() {
        // `.invalid` never resolves (RFC 6761).
        assert!(resolve_host("relay.invalid", 4001).await.is_empty());
    }

    #[test]
    fn extra_relay_addresses_use_resolved_ips_not_dns() {
        let relays = [
            resolved("relay.example", &["203.0.113.7", "2001:db8::1"]),
            resolved("203.0.113.9", &["203.0.113.9"]),
        ];

        let bootstrap: Vec<String> = extra_bootstrap_addrs(&relays)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            bootstrap,
            vec![
                format!("/ip4/203.0.113.7/tcp/4001/p2p/{EXTRA_PEER}"),
                format!("/ip4/203.0.113.7/udp/4001/quic-v1/p2p/{EXTRA_PEER}"),
                format!("/ip6/2001:db8::1/tcp/4001/p2p/{EXTRA_PEER}"),
                format!("/ip6/2001:db8::1/udp/4001/quic-v1/p2p/{EXTRA_PEER}"),
                format!("/ip4/203.0.113.9/tcp/4001/p2p/{EXTRA_PEER}"),
                format!("/ip4/203.0.113.9/udp/4001/quic-v1/p2p/{EXTRA_PEER}"),
            ]
        );

        let circuits: Vec<String> = extra_circuit_addrs(&relays)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            circuits,
            vec![
                format!("/ip4/203.0.113.7/tcp/4001/p2p/{EXTRA_PEER}/p2p-circuit"),
                format!("/ip4/203.0.113.9/tcp/4001/p2p/{EXTRA_PEER}/p2p-circuit"),
            ],
            "one circuit address per relay, on its first IP"
        );

        let dest: PeerId = "12D3KooWENHQjSydcHUXVTuq4wVNvCP4VGXzxueBtdKi1D3mS6wR"
            .parse()
            .unwrap();
        let dials = extra_circuit_dial_addrs(&relays, &dest);
        assert_eq!(dials.len(), 2, "one dial address per relay");
        assert!(dials.iter().all(|addr| addr
            .to_string()
            .ends_with(&format!("/p2p-circuit/p2p/{dest}"))));
    }

    #[test]
    fn extra_relays_without_ips_get_no_addresses() {
        let relays = [resolved("relay.invalid", &[])];
        let dest: PeerId = EXTRA_PEER.parse().unwrap();
        assert!(extra_bootstrap_addrs(&relays).is_empty());
        assert!(extra_circuit_addrs(&relays).is_empty());
        assert!(extra_circuit_dial_addrs(&relays, &dest).is_empty());
    }
}
