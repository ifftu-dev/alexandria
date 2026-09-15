use alexandria_verify::json::JsonLimits;
use serde::{Deserialize, Serialize};

/// Largest gossip message the node publishes or accepts, in bytes.
pub const MAX_GOSSIP_MESSAGE_BYTES: usize = 64 * 1024;

/// Structural limits for a signed gossip envelope. The payload, signature and
/// public key encode as JSON arrays of bytes, so an array may hold a whole
/// message's worth of elements while nesting stays two levels deep.
pub const GOSSIP_ENVELOPE_JSON_LIMITS: JsonLimits = JsonLimits {
    max_bytes: MAX_GOSSIP_MESSAGE_BYTES,
    max_depth: 2,
    max_array_len: MAX_GOSSIP_MESSAGE_BYTES,
    max_object_entries: 16,
    max_string_bytes: 1024,
};

/// Structural limits for the topic payload inside a gossip envelope, checked
/// before any topic handler decodes it.
pub const GOSSIP_PAYLOAD_JSON_LIMITS: JsonLimits = JsonLimits {
    max_bytes: MAX_GOSSIP_MESSAGE_BYTES,
    max_depth: 32,
    max_array_len: 4096,
    max_object_entries: 256,
    max_string_bytes: 64 * 1024,
};

/// Structural limits for an unsigned peer exchange announcement.
pub const PEER_EXCHANGE_JSON_LIMITS: JsonLimits = JsonLimits {
    max_bytes: MAX_GOSSIP_MESSAGE_BYTES,
    max_depth: 2,
    max_array_len: 64,
    max_object_entries: 8,
    max_string_bytes: 1024,
};

/// Gossip topic identifiers for the Alexandria P2P protocol.
///
/// Each topic uses a versioned path to allow protocol upgrades.
pub const TOPIC_CATALOG: &str = "/alexandria/catalog/1.0";
pub const TOPIC_TAXONOMY: &str = "/alexandria/taxonomy/1.0";
pub const TOPIC_GOVERNANCE: &str = "/alexandria/governance/1.0";
pub const TOPIC_PROFILES: &str = "/alexandria/profiles/1.0";
/// Field Commentary opinions — credentialed-in-domain video takes.
/// Receivers validate the envelope signature AND check that the
/// author's referenced credentials exist locally and cover at least
/// one skill under the target `subject_field_id`. Opinions whose
/// credentials haven't synced yet are held in
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
/// Reserved for the replacement committee certificate protocol. Messages on
/// this topic currently confer no plugin or credential authority.
pub const TOPIC_PLUGIN_ATTESTATIONS: &str = "/alexandria/plugin-attestations/1.0";

/// Retired Sentinel prior library topic. It stays subscribed until the
/// coordinated wire-protocol removal; every inbound message is rejected
/// before any database access.
pub const TOPIC_SENTINEL_PRIORS: &str = "/alexandria/sentinel-priors/1.0";

/// Ratified goal-template versions — a DAO publishes a signed version
/// document (goal → target-skill maps) here after a `goal_template_change`
/// proposal is approved. Receivers apply it into `goal_templates`. Privileged.
pub const TOPIC_GOAL_TEMPLATES: &str = "/alexandria/goal-templates/1.0";
/// Ratified assessment question-bank versions — published after a
/// `question_bank_change` proposal is approved. Receivers apply it into
/// `question_banks` / `bank_questions`. The answer key travels inside the
/// signed doc but is never re-exposed to the client. Privileged.
pub const TOPIC_QUESTION_BANKS: &str = "/alexandria/question-banks/1.0";

/// All gossip topics the node subscribes to.
pub const ALL_TOPICS: &[&str] = &[
    TOPIC_CATALOG,
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
    TOPIC_GOAL_TEMPLATES,
    TOPIC_QUESTION_BANKS,
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
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", content = "address")]
pub enum NatState {
    /// NAT status not yet determined (probing in progress).
    #[default]
    Unknown,
    /// Node is publicly reachable at the given address.
    Public(String),
    /// Node is behind a NAT and not directly reachable.
    Private,
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
