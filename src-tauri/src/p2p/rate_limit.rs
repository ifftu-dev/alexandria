use libp2p::PeerId;
use std::collections::HashMap;
use std::time::Instant;

/// Maximum tokens (messages) per peer.
const MAX_TOKENS: u32 = 20;

/// How often a token is replenished (one every 3 seconds).
const REFILL_INTERVAL_SECS: u64 = 3;

/// Simple per-peer token-bucket rate limiter for gossip messages.
///
/// Each peer starts with `MAX_TOKENS` tokens. Consuming a token allows
/// one message through. Tokens are replenished at a rate of 1 per
/// `REFILL_INTERVAL_SECS`, up to `MAX_TOKENS`.
pub struct PeerRateLimiter {
    peers: HashMap<PeerId, (u32, Instant)>,
}

impl PeerRateLimiter {
    pub fn new() -> Self {
        Self {
            peers: HashMap::new(),
        }
    }

    /// Returns `true` if the peer is within rate limits (message allowed).
    /// Returns `false` if the peer has exceeded its rate limit (drop the message).
    pub fn check(&mut self, peer: &PeerId) -> bool {
        let now = Instant::now();

        let (tokens, last_refill) = self
            .peers
            .entry(*peer)
            .or_insert((MAX_TOKENS, now));

        // Refill tokens based on elapsed time
        let elapsed = now.duration_since(*last_refill).as_secs();
        if elapsed > 0 {
            let refill = (elapsed / REFILL_INTERVAL_SECS) as u32;
            if refill > 0 {
                *tokens = (*tokens + refill).min(MAX_TOKENS);
                *last_refill = now;
            }
        }

        // Try to consume a token
        if *tokens > 0 {
            *tokens -= 1;
            true
        } else {
            false
        }
    }

    /// Remove tracking state for a disconnected peer.
    pub fn remove_peer(&mut self, peer: &PeerId) {
        self.peers.remove(peer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_max_tokens() {
        let mut limiter = PeerRateLimiter::new();
        let peer = PeerId::random();

        for _ in 0..MAX_TOKENS {
            assert!(limiter.check(&peer));
        }
        // Next one should be rejected
        assert!(!limiter.check(&peer));
    }

    #[test]
    fn different_peers_independent() {
        let mut limiter = PeerRateLimiter::new();
        let peer_a = PeerId::random();
        let peer_b = PeerId::random();

        // Exhaust peer_a
        for _ in 0..MAX_TOKENS {
            limiter.check(&peer_a);
        }
        assert!(!limiter.check(&peer_a));

        // peer_b should still be fine
        assert!(limiter.check(&peer_b));
    }

    #[test]
    fn remove_peer_resets() {
        let mut limiter = PeerRateLimiter::new();
        let peer = PeerId::random();

        for _ in 0..MAX_TOKENS {
            limiter.check(&peer);
        }
        assert!(!limiter.check(&peer));

        limiter.remove_peer(&peer);
        // After removal, peer gets fresh tokens
        assert!(limiter.check(&peer));
    }
}
