//! Content provider discovery over iroh-gossip.
//!
//! Answers "I have a BLAKE3 hash — which endpoints can serve it?" by producing
//! a set of candidate [`EndpointAddr`]s for [`super::fetch`] to try.
//!
//! Mechanism: a single well-known gossip topic. A node that holds content
//! broadcasts an [`Announce`] — "endpoint E holds hashes [H…]" — and every
//! subscriber folds it into a TTL'd provider table. Announcements are *hints*,
//! not authority: the fetch is BLAKE3-verified, so a lying announcer simply
//! fails to serve and is skipped. No signature is required at this layer.
//!
//! The table is also directly seedable ([`ContentDiscovery::seed`]). That is
//! how the PinBoard layer injects known pinners for a subject, and how tests
//! inject providers deterministically without depending on gossip
//! swarm-formation timing.
//!
//! Scope note: one global topic is fine at current scale but means every
//! subscriber sees every announcement. Sharding the topic by content namespace
//! (e.g. by subject DID) is the natural next step and is intentionally left as
//! future work.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use iroh::{Endpoint, EndpointAddr};
use iroh_blobs::Hash;
use iroh_gossip::net::Gossip;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// How long a learned provider entry stays valid before it must be re-announced.
const PROVIDER_TTL: Duration = Duration::from_secs(15 * 60);

/// Domain-separated seed for the shared content-discovery gossip topic.
const DISCOVERY_TOPIC_SEED: &[u8] = b"alexandria/content-discovery/v1";

/// A wire announcement: the sender endpoint holds these hashes.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Announce {
    /// Full dialable address of the announcing provider.
    addr: EndpointAddr,
    /// Hashes the provider is offering to serve.
    hashes: Vec<Hash>,
}

/// One known provider for a hash, with its expiry.
#[derive(Debug, Clone)]
struct ProviderEntry {
    addr: EndpointAddr,
    expires_at: Instant,
}

/// Provider table plus the gossip plumbing that keeps it fed.
///
/// Cheap to clone-wrap in an `Arc`; the resolver holds one `Arc<ContentDiscovery>`.
#[derive(Clone)]
pub struct ContentDiscovery {
    table: Arc<Mutex<HashMap<Hash, Vec<ProviderEntry>>>>,
    /// Set once gossip is started; used to broadcast our own announcements.
    sender: Arc<Mutex<Option<iroh_gossip::api::GossipSender>>>,
}

impl Default for ContentDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

impl ContentDiscovery {
    pub fn new() -> Self {
        Self {
            table: Arc::new(Mutex::new(HashMap::new())),
            sender: Arc::new(Mutex::new(None)),
        }
    }

    /// The shared discovery topic id (BLAKE3 of the domain-separated seed).
    fn topic_id() -> iroh_gossip::proto::TopicId {
        let hash = blake3::hash(DISCOVERY_TOPIC_SEED);
        iroh_gossip::proto::TopicId::from_bytes(*hash.as_bytes())
    }

    /// Directly record that `addr` can serve `hash` (fresh TTL).
    ///
    /// Used by the PinBoard layer to inject known pinners, and by tests to seed
    /// providers without waiting on a gossip swarm.
    pub async fn seed(&self, hash: Hash, addr: EndpointAddr) {
        let mut table = self.table.lock().await;
        insert_entry(&mut table, hash, addr);
    }

    /// Return the currently-known, unexpired providers for `hash`.
    ///
    /// Prunes expired entries as a side effect.
    pub async fn find_providers(&self, hash: Hash) -> Vec<EndpointAddr> {
        let now = Instant::now();
        let mut table = self.table.lock().await;
        let Some(entries) = table.get_mut(&hash) else {
            return Vec::new();
        };
        entries.retain(|e| e.expires_at > now);
        if entries.is_empty() {
            table.remove(&hash);
            return Vec::new();
        }
        entries.iter().map(|e| e.addr.clone()).collect()
    }

    /// Subscribe to the discovery topic and spawn the ingest loop.
    ///
    /// Inbound [`Announce`] messages are folded into the provider table. Call
    /// once when the node starts. `bootstrap` are endpoints already known to be
    /// on the topic (e.g. relays / registry peers); an empty list still works
    /// once neighbors are learned by other means.
    pub async fn start(
        &self,
        gossip: &Gossip,
        bootstrap: Vec<iroh::EndpointId>,
    ) -> Result<(), String> {
        let topic = gossip
            .subscribe(Self::topic_id(), bootstrap)
            .await
            .map_err(|e| format!("subscribe discovery topic: {e}"))?;
        let (sender, mut receiver) = topic.split();
        *self.sender.lock().await = Some(sender);

        let table = self.table.clone();
        tokio::spawn(async move {
            use futures::StreamExt;
            while let Some(Ok(event)) = receiver.next().await {
                let iroh_gossip::api::Event::Received(msg) = event else {
                    continue;
                };
                match postcard::from_bytes::<Announce>(&msg.content) {
                    Ok(announce) => {
                        let mut table = table.lock().await;
                        for hash in announce.hashes {
                            insert_entry(&mut table, hash, announce.addr.clone());
                        }
                    }
                    Err(e) => {
                        log::warn!("discovery: undecodable announce: {e}");
                    }
                }
            }
            log::info!("discovery: ingest loop ended");
        });
        Ok(())
    }

    /// Broadcast that `my_addr` holds `hashes` so other subscribers can fetch
    /// from us. No-op if [`start`](Self::start) has not run yet.
    pub async fn announce(&self, hashes: Vec<Hash>, my_addr: EndpointAddr) -> Result<(), String> {
        if hashes.is_empty() {
            return Ok(());
        }
        let guard = self.sender.lock().await;
        let Some(sender) = guard.as_ref() else {
            log::debug!("discovery: announce before gossip start; skipping");
            return Ok(());
        };
        let announce = Announce {
            addr: my_addr,
            hashes,
        };
        let bytes = postcard::to_stdvec(&announce).map_err(|e| format!("encode announce: {e}"))?;
        sender
            .broadcast(bytes.into())
            .await
            .map_err(|e| format!("broadcast announce: {e}"))?;
        Ok(())
    }

    /// Convenience: announce a single hash using the node's current address.
    pub async fn announce_have(&self, hash: Hash, endpoint: &Endpoint) -> Result<(), String> {
        self.announce(vec![hash], endpoint.addr()).await
    }
}

/// Insert-or-refresh a provider entry for `hash` with a fresh TTL.
fn insert_entry(table: &mut HashMap<Hash, Vec<ProviderEntry>>, hash: Hash, addr: EndpointAddr) {
    let entries = table.entry(hash).or_default();
    let expires_at = Instant::now() + PROVIDER_TTL;
    if let Some(existing) = entries.iter_mut().find(|e| e.addr.id == addr.id) {
        existing.addr = addr;
        existing.expires_at = expires_at;
    } else {
        entries.push(ProviderEntry { addr, expires_at });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_hash(b: u8) -> Hash {
        Hash::from_bytes([b; 32])
    }

    fn test_addr() -> EndpointAddr {
        // A bare EndpointAddr (id only) is enough for table bookkeeping tests;
        // dialing is exercised in the e2e transfer tests.
        let secret = iroh::SecretKey::from_bytes(&[7u8; 32]);
        EndpointAddr::from(secret.public())
    }

    #[tokio::test]
    async fn seed_then_find_returns_provider() {
        let d = ContentDiscovery::new();
        let h = test_hash(1);
        d.seed(h, test_addr()).await;
        let found = d.find_providers(h).await;
        assert_eq!(found.len(), 1, "seeded provider must be discoverable");
    }

    #[tokio::test]
    async fn find_unknown_hash_is_empty() {
        let d = ContentDiscovery::new();
        assert!(d.find_providers(test_hash(2)).await.is_empty());
    }

    #[tokio::test]
    async fn seeding_same_provider_twice_dedupes() {
        let d = ContentDiscovery::new();
        let h = test_hash(3);
        d.seed(h, test_addr()).await;
        d.seed(h, test_addr()).await;
        assert_eq!(
            d.find_providers(h).await.len(),
            1,
            "same endpoint id must not create duplicate entries"
        );
    }
}
