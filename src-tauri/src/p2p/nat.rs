//! NAT traversal configuration and status tracking.
//!
//! Per spec §7.5: "Most users are behind NATs. The network uses:
//! 1. QUIC hole punching (built into libp2p).
//! 2. AutoNAT — peers help each other determine if they're publicly reachable.
//! 3. Circuit relay v2 — if direct connection fails, relay through a willing peer."
//!
//! This module configures AutoNAT probing and provides types for
//! tracking and reporting NAT status to the frontend.

use std::time::Duration;

use libp2p::autonat;

/// Build AutoNAT configuration for the Alexandria node.
///
/// AutoNAT lets peers discover whether they're behind a NAT by
/// requesting other peers to dial back to them. The result determines
/// whether we listen for relay reservations or attempt direct connections.
pub fn build_autonat_config() -> autonat::Config {
    let mut config = autonat::Config::default();

    // Probe every 60 seconds to detect NAT changes
    // (default is 15s, but we don't need aggressive probing)
    config.retry_interval = Duration::from_secs(60);

    // After 2 successful probes, consider ourselves public
    // (default is 3, lowered for faster convergence in small networks)
    config.confidence_max = 2;

    // Throttle inbound probe requests: max 2 per peer per minute
    // (prevents abuse as an amplification vector)
    config.throttle_server_period = Duration::from_secs(30);

    // Only probe 3 servers maximum per cycle
    config.max_peer_addresses = 3;

    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autonat_config_has_reasonable_defaults() {
        let config = build_autonat_config();
        assert_eq!(config.retry_interval, Duration::from_secs(60));
        assert_eq!(config.confidence_max, 2);
    }

    #[test]
    fn autonat_config_throttles_inbound_probes() {
        let config = build_autonat_config();
        assert_eq!(config.throttle_server_period, Duration::from_secs(30));
    }
}
