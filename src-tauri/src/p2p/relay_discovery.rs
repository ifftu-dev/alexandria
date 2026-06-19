//! Client-side policy for auto-discovered circuit relays.
//!
//! Nodes that contribute to the network advertise themselves under
//! [`super::discovery::relay_namespace_key`]; clients query that key and
//! feed the results here. This bounds how many discovered relays we
//! adopt (cap) and tracks a small reputation score so relays that fail
//! to connect are evicted — limiting the eclipse/Sybil surface that
//! open discovery would otherwise expose.
//!
//! These relays provide CONNECTIVITY only (circuit + DHT). They never
//! gain username-receipt trust — that is gated separately by the
//! on-chain registry — so a malicious discovered relay can at worst
//! refuse traffic, not forge identity.

use std::collections::HashMap;

use libp2p::PeerId;

/// Max discovered relays adopted at once. Keeps the set small so a flood
/// of Sybil adverts can't dominate our relay paths.
const DEFAULT_CAP: usize = 8;
/// Score a relay starts with on admission.
const START_SCORE: i32 = 2;
/// Ceiling so a long-lived relay can't accrue unbounded immunity.
const MAX_SCORE: i32 = 5;
/// At or below this score the relay is evicted.
const EVICT_AT: i32 = 0;

/// Capped, reputation-scored set of auto-discovered relays.
pub struct DiscoveredRelays {
    cap: usize,
    scores: HashMap<PeerId, i32>,
}

impl DiscoveredRelays {
    pub fn new() -> Self {
        Self::with_cap(DEFAULT_CAP)
    }

    pub fn with_cap(cap: usize) -> Self {
        Self {
            cap,
            scores: HashMap::new(),
        }
    }

    /// Admit a freshly discovered relay if there is room under the cap.
    /// Returns `true` only when newly admitted — the caller should then
    /// dial it and request a circuit reservation. Already-known relays
    /// and over-cap discoveries return `false`.
    pub fn admit(&mut self, peer: PeerId) -> bool {
        if self.scores.contains_key(&peer) || self.scores.len() >= self.cap {
            return false;
        }
        self.scores.insert(peer, START_SCORE);
        true
    }

    pub fn contains(&self, peer: &PeerId) -> bool {
        self.scores.contains_key(peer)
    }

    /// Reward a relay that connected / reserved successfully.
    pub fn note_success(&mut self, peer: &PeerId) {
        if let Some(s) = self.scores.get_mut(peer) {
            *s = (*s + 1).min(MAX_SCORE);
        }
    }

    /// Penalize a failure. Returns `true` if the relay was evicted (so
    /// the caller can free its reservation / stop using it).
    pub fn note_failure(&mut self, peer: &PeerId) -> bool {
        if let Some(s) = self.scores.get_mut(peer) {
            *s -= 1;
            if *s <= EVICT_AT {
                self.scores.remove(peer);
                return true;
            }
        }
        false
    }

    pub fn peers(&self) -> Vec<PeerId> {
        self.scores.keys().copied().collect()
    }

    pub fn len(&self) -> usize {
        self.scores.len()
    }

    pub fn is_empty(&self) -> bool {
        self.scores.is_empty()
    }
}

impl Default for DiscoveredRelays {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer() -> PeerId {
        libp2p::identity::Keypair::generate_ed25519()
            .public()
            .to_peer_id()
    }

    #[test]
    fn admits_until_cap_then_refuses() {
        let mut d = DiscoveredRelays::with_cap(2);
        assert!(d.admit(peer()));
        assert!(d.admit(peer()));
        assert!(!d.admit(peer())); // over cap
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn admit_is_idempotent_per_peer() {
        let mut d = DiscoveredRelays::with_cap(4);
        let p = peer();
        assert!(d.admit(p));
        assert!(!d.admit(p)); // already known
        assert_eq!(d.len(), 1);
    }

    #[test]
    fn failures_evict_then_free_a_slot() {
        let mut d = DiscoveredRelays::with_cap(1);
        let p = peer();
        assert!(d.admit(p));
        // START_SCORE = 2 → two failures reach EVICT_AT.
        assert!(!d.note_failure(&p));
        assert!(d.note_failure(&p)); // evicted
        assert!(!d.contains(&p));
        // Cap slot freed for a new relay.
        assert!(d.admit(peer()));
    }

    #[test]
    fn success_raises_resilience_to_failure() {
        let mut d = DiscoveredRelays::with_cap(1);
        let p = peer();
        d.admit(p); // score 2
        d.note_success(&p); // 3
        d.note_success(&p); // 4
        for _ in 0..3 {
            assert!(!d.note_failure(&p)); // 3,2,1 — survives
        }
        assert!(d.note_failure(&p)); // 0 → evicted
    }

    #[test]
    fn note_failure_on_unknown_is_noop() {
        let mut d = DiscoveredRelays::new();
        assert!(!d.note_failure(&peer()));
    }
}
