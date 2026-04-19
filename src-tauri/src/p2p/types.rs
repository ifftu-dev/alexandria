use serde::{Deserialize, Serialize};

/// Gossip topic identifiers for the Alexandria P2P protocol.
///
/// Each topic uses a versioned path to allow protocol upgrades.
pub const TOPIC_CATALOG: &str = "/alexandria/catalog/1.0";
pub const TOPIC_EVIDENCE: &str = "/alexandria/evidence/1.0";
pub const TOPIC_TAXONOMY: &str = "/alexandria/taxonomy/1.0";
pub const TOPIC_GOVERNANCE: &str = "/alexandria/governance/1.0";
pub const TOPIC_PROFILES: &str = "/alexandria/profiles/1.0";
/// Field Commentary opinions — credentialed-in-domain video takes.
/// Receivers validate the envelope signature AND check that the
/// author's referenced `skill_proof` IDs exist locally and cover at
/// least one skill under the target `subject_field_id`. Opinions
/// whose credentials haven't synced yet are held in
/// `opinions_pending_verification`.
pub const TOPIC_OPINIONS: &str = "/alexandria/opinions/1.0";
/// Peer exchange topic — nodes broadcast their PeerId + listen addresses
/// so that peers-of-peers can discover each other transitively.
pub const TOPIC_PEER_EXCHANGE: &str = "/alexandria/peer-exchange/1.0";

// ---- VC-first migration (PRs 2–13) --------------------------------------
/// DID document announcements + key rotation records (spec §5.3).
/// Receivers reflect the DID registry into their local `key_registry`
/// so historical verification survives across peers.
pub const TOPIC_VC_DID: &str = "/alexandria/vc-did/1.0";
/// RevocationList2020-style status list snapshots / deltas (§11.2).
/// Versioned — receivers refuse older versions to prevent rollback.
pub const TOPIC_VC_STATUS: &str = "/alexandria/vc-status/1.0";
/// Subject-authored selective-disclosure presentations (§18). Opt-in;
/// a subject broadcasts a presentation to a specific audience and
/// network members relay it.
pub const TOPIC_VC_PRESENTATION: &str = "/alexandria/vc-presentation/1.0";
/// PinBoard pinning commitments (§12 + §20.4). Peers broadcast opt-in
/// commitments to pin specific subjects' content for community
/// redundancy.
pub const TOPIC_PINBOARD: &str = "/alexandria/pinboard/1.0";

// ---- Community plugin system (Phase 3) -----------------------------------
/// Plugin announcements — authors broadcast a manifest CID + metadata so
/// other nodes can discover and (optionally) install. Receivers cache the
/// announcement in `plugin_catalog` for opinion-weighted browse. The full
/// bundle bytes are *not* on this topic — they're fetched on demand from
/// the iroh blob store via the manifest CID.
pub const TOPIC_PLUGINS: &str = "/alexandria/plugins/1.0";
/// Plugin DAO attestations — the canonical Alexandria Plugin DAO
/// publishes threshold-signed (plugin_cid, grader_cid) attestations on
/// this topic. Verifiers cross-reference attestations from this topic
/// to decide whether a graded plugin's submissions are credential-eligible.
pub const TOPIC_PLUGIN_ATTESTATIONS: &str = "/alexandria/plugin-attestations/1.0";

/// Ratified Sentinel adversarial priors — the Sentinel DAO broadcasts
/// metadata for each prior the committee has approved so every client
/// can mirror the library locally. The blob itself is content-addressed
/// and fetched separately on demand; this topic carries only the
/// envelope metadata plus the approved proposal reference.
/// See docs/sentinel-adversarial-priors.md.
pub const TOPIC_SENTINEL_PRIORS: &str = "/alexandria/sentinel-priors/1.0";

/// All gossip topics the node subscribes to.
pub const ALL_TOPICS: &[&str] = &[
    TOPIC_CATALOG,
    TOPIC_EVIDENCE,
    TOPIC_TAXONOMY,
    TOPIC_GOVERNANCE,
    TOPIC_PROFILES,
    TOPIC_OPINIONS,
    TOPIC_PEER_EXCHANGE,
    TOPIC_VC_DID,
    TOPIC_VC_STATUS,
    TOPIC_VC_PRESENTATION,
    TOPIC_PINBOARD,
    TOPIC_PLUGINS,
    TOPIC_PLUGIN_ATTESTATIONS,
    TOPIC_SENTINEL_PRIORS,
];

/// Peer exchange message — broadcast on TOPIC_PEER_EXCHANGE.
///
/// Contains the sender's PeerId and all known listen addresses so
/// other nodes can dial them directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerExchangeMessage {
    /// The PeerId of the broadcasting node.
    pub peer_id: String,
    /// Multiaddresses the node is listening on.
    pub addresses: Vec<String>,
}

/// A signed gossip message envelope.
///
/// Every message broadcast on the P2P network is wrapped in this
/// envelope. The sender signs the payload with their Cardano Ed25519
/// key, enabling receivers to verify authenticity and link the
/// message to an on-chain identity (stake address).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedGossipMessage {
    /// The topic this message was published on.
    pub topic: String,
    /// The actual message payload (JSON-encoded, topic-specific).
    /// If `encrypted` is true, this is ciphertext (encrypt-then-sign).
    pub payload: Vec<u8>,
    /// Ed25519 signature over `payload` by the sender's Cardano signing key.
    pub signature: Vec<u8>,
    /// The sender's Ed25519 public key (32 bytes).
    /// This is the Cardano payment verification key, not the libp2p peer key.
    pub public_key: Vec<u8>,
    /// Sender's Cardano stake address (bech32).
    pub stake_address: String,
    /// Unix timestamp (seconds) when the message was created.
    pub timestamp: u64,
    /// Whether the payload is encrypted (for private topics).
    #[serde(default)]
    pub encrypted: bool,
    /// Key identifier for encrypted payloads (e.g., classroom group key version).
    #[serde(default)]
    pub key_id: Option<String>,
}

/// Information about a known peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    /// libp2p PeerId (base58 encoded).
    pub peer_id: String,
    /// Cardano stake address (if known).
    pub stake_address: Option<String>,
    /// Display name (if known).
    pub display_name: Option<String>,
    /// Last seen timestamp (ISO 8601).
    pub last_seen: String,
    /// Known multiaddresses (JSON array).
    pub addresses: Vec<String>,
    /// Peer roles (e.g., ["instructor", "learner"]).
    pub roles: Vec<String>,
    /// Cached reputation score.
    pub reputation: Option<f64>,
}

/// NAT reachability status.
///
/// Determined by AutoNAT probing — peers try to dial us back
/// to determine if we're publicly reachable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", content = "address")]
pub enum NatState {
    /// NAT status not yet determined (probing in progress).
    Unknown,
    /// Node is publicly reachable at the given address.
    Public(String),
    /// Node is behind a NAT and not directly reachable.
    Private,
}

impl Default for NatState {
    fn default() -> Self {
        Self::Unknown
    }
}

/// P2P network status reported to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStatus {
    /// Whether the P2P node is running.
    pub is_running: bool,
    /// The local PeerId (base58).
    pub peer_id: Option<String>,
    /// Number of connected peers.
    pub connected_peers: usize,
    /// Multiaddresses the node is listening on.
    pub listening_addresses: Vec<String>,
    /// Topics the node is subscribed to.
    pub subscribed_topics: Vec<String>,
    /// NAT traversal status (public, private, or unknown).
    pub nat_status: NatState,
    /// Addresses we are reachable at via circuit relay.
    pub relay_addresses: Vec<String>,
}

/// Events emitted by the P2P layer to the application.
#[derive(Debug, Clone)]
pub enum P2pEvent {
    /// A new peer connected.
    PeerConnected { peer_id: String },
    /// A peer disconnected.
    PeerDisconnected { peer_id: String },
    /// Received a gossip message on a topic.
    GossipMessage {
        topic: String,
        message: SignedGossipMessage,
    },
    /// Network status changed.
    StatusChanged(NetworkStatus),
    /// NAT status changed (as determined by AutoNAT probing).
    NatStatusChanged(NatState),
    /// A relay reservation was accepted (we can be reached via relay).
    RelayReservation { relay_peer: String },
    /// A relayed connection was upgraded to a direct connection via DCUtR.
    DirectConnectionUpgraded { peer_id: String },
}
