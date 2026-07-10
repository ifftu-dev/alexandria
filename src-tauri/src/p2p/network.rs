use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use libp2p::gossipsub::{self, IdentTopic, MessageAuthenticity, MessageId, ValidationMode};
use libp2p::identity::Keypair;
use libp2p::kad::store::MemoryStore;
use libp2p::request_response::{self, ProtocolSupport, ResponseChannel};
use libp2p::swarm::NetworkBehaviour;
use libp2p::{
    autonat, dcutr, identify, kad, noise, relay, yamux, PeerId, StreamProtocol, Swarm, SwarmBuilder,
};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use super::device_sync::{SyncRequest, SyncResponse};
use super::graph_fetch::{GraphFetchRequest, GraphFetchResponse};
use super::guardian::{GuardianRequest, GuardianResponse};
use super::profile_fetch::{ProfileFetchRequest, ProfileFetchResponse};
use super::username_reg::{ReceiptRequest, ReceiptResponse};
use super::vc_fetch::{FetchRequest, FetchResponse};

use crate::db::Database;

use crate::crypto::hash::blake2b_256;
use crate::diag;
use std::sync::Mutex as StdMutex;

use super::nat::build_autonat_config;
use super::rate_limit::PeerRateLimiter;
use super::scoring::{build_peer_score_params, build_peer_score_thresholds};
use super::types::{NatState, NetworkStatus, P2pEvent, SignedGossipMessage, ALL_TOPICS};
use super::validation::MessageValidator;

#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("failed to build swarm: {0}")]
    SwarmBuild(String),
    #[error("failed to subscribe to topic: {0}")]
    Subscribe(String),
    #[error("node is not running")]
    NotRunning,
    #[error("failed to publish message: {0}")]
    Publish(String),
    #[error("failed to listen on address: {0}")]
    Listen(String),
    #[error("identity error: {0}")]
    Identity(String),
}

const PROVIDER_INITIAL_DELAY: Duration = Duration::from_secs(5);
const PROVIDER_WARMUP_INTERVAL: Duration = Duration::from_secs(15);
const PROVIDER_WARMUP_WINDOW: Duration = Duration::from_secs(120);
const PROVIDER_REFRESH_INTERVAL: Duration = Duration::from_secs(180);
const PEER_EXCHANGE_INTERVAL: Duration = Duration::from_secs(30);

fn identify_agent_version() -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!("alexandria-node/{version} ({})", device_label())
}

fn device_label() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Some(model) = apple_sysctl("hw.model") {
            humanize_apple_model(&model)
        } else {
            "Mac".to_string()
        }
    }

    #[cfg(target_os = "ios")]
    {
        if let Some(model) = apple_sysctl("hw.machine") {
            humanize_apple_model(&model)
        } else if let Ok(sim_model) = std::env::var("SIMULATOR_MODEL_IDENTIFIER") {
            if !sim_model.trim().is_empty() {
                humanize_apple_model(sim_model.trim())
            } else {
                "iPhone or iPad".to_string()
            }
        } else {
            "iPhone or iPad".to_string()
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(label) = std::env::var("ALEXANDRIA_DEVICE_LABEL") {
            let label = label.trim();
            if !label.is_empty() {
                label.to_string()
            } else if let Ok(product) =
                std::fs::read_to_string("/sys/devices/virtual/dmi/id/product_name")
            {
                let product = product.trim();
                if !product.is_empty() {
                    product.to_string()
                } else {
                    "Linux device".to_string()
                }
            } else {
                "Linux device".to_string()
            }
        } else if let Ok(product) =
            std::fs::read_to_string("/sys/devices/virtual/dmi/id/product_name")
        {
            let product = product.trim();
            if !product.is_empty() {
                product.to_string()
            } else {
                "Linux device".to_string()
            }
        } else {
            "Linux device".to_string()
        }
    }

    #[cfg(target_os = "android")]
    {
        if let Ok(label) = std::env::var("ALEXANDRIA_DEVICE_LABEL") {
            let label = label.trim();
            if !label.is_empty() {
                label.to_string()
            } else {
                "Android device".to_string()
            }
        } else {
            "Android device".to_string()
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(label) = std::env::var("ALEXANDRIA_DEVICE_LABEL") {
            let label = label.trim();
            if !label.is_empty() {
                label.to_string()
            } else {
                "Windows device".to_string()
            }
        } else {
            "Windows device".to_string()
        }
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "android",
        target_os = "windows"
    )))]
    {
        "Unknown device".to_string()
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn humanize_apple_model(model: &str) -> String {
    let model = model.trim();
    if model.is_empty() {
        return "Apple device".to_string();
    }

    if model.starts_with("MacBookPro") {
        return format!("MacBook Pro ({model})");
    }
    if model.starts_with("MacBookAir") {
        return format!("MacBook Air ({model})");
    }
    if model.starts_with("MacBook") {
        return format!("MacBook ({model})");
    }
    if model.starts_with("Macmini") {
        return format!("Mac mini ({model})");
    }
    if model.starts_with("MacStudio") {
        return format!("Mac Studio ({model})");
    }
    if model.starts_with("MacPro") {
        return format!("Mac Pro ({model})");
    }
    if model.starts_with("iPhone") {
        if let Some(gen) = extract_apple_generation(model, "iPhone") {
            return format!("iPhone {gen} ({model})");
        }
        return format!("iPhone ({model})");
    }
    if model.starts_with("iPad") {
        if let Some(gen) = extract_apple_generation(model, "iPad") {
            return format!("iPad {gen} ({model})");
        }
        return format!("iPad ({model})");
    }

    format!("Apple device ({model})")
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn extract_apple_generation(model: &str, prefix: &str) -> Option<u32> {
    let tail = model.strip_prefix(prefix)?;
    let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u32>().ok()
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn apple_sysctl(name: &str) -> Option<String> {
    use std::ffi::CString;
    use std::os::raw::c_void;

    let key = CString::new(name).ok()?;
    let mut size: usize = 0;

    unsafe {
        if libc::sysctlbyname(
            key.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return None;
        }

        if size == 0 {
            return None;
        }

        let mut buffer = vec![0u8; size];
        if libc::sysctlbyname(
            key.as_ptr(),
            buffer.as_mut_ptr() as *mut c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return None;
        }

        if let Some(end) = buffer.iter().position(|b| *b == 0) {
            buffer.truncate(end);
        }

        String::from_utf8(buffer).ok()
    }
}

/// Composed network behaviour for the Alexandria P2P node.
///
/// Combines seven libp2p protocols:
/// - **GossipSub v1.1**: Topic-based publish/subscribe with peer scoring
/// - **Kademlia DHT**: Private Alexandria DHT (`/alexandria/kad/1.0`)
/// - **Identify**: Exchange peer info (needed by GossipSub and Kademlia)
/// - **AutoNAT**: Determine NAT reachability via peer probing
/// - **Relay Server**: Any publicly reachable node relays traffic for NATted peers
/// - **Relay Client**: Use circuit relay v2 when behind NAT
/// - **DCUtR**: Upgrade relayed connections to direct connections
#[derive(NetworkBehaviour)]
pub struct AlexandriaBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub kademlia: kad::Behaviour<MemoryStore>,
    pub identify: identify::Behaviour,
    pub autonat: autonat::Behaviour,
    pub relay_server: relay::Behaviour,
    pub relay_client: relay::client::Behaviour,
    pub dcutr: dcutr::Behaviour,
    /// Pull-based credential fetch — `/alexandria/vc-fetch/1.0`
    /// request-response protocol with CBOR codec. Inbound requests
    /// are forwarded to the application layer via
    /// `P2pEvent::FetchRequestReceived`; the application calls
    /// `P2pNode::send_fetch_response` to reply.
    pub vc_fetch: request_response::cbor::Behaviour<FetchRequest, FetchResponse>,
    /// Cross-device sync — `/alexandria/sync/1.0` request-response
    /// protocol between explicitly paired devices. Payloads are sealed
    /// with the pair's shared key (see [`super::device_sync`]).
    pub device_sync: request_response::cbor::Behaviour<SyncRequest, SyncResponse>,
    /// Pull-based public skill-graph fetch — `/alexandria/graph-fetch/1.0`
    /// request-response protocol with CBOR codec. A node serves its own
    /// owner's public skill graph; inbound requests are answered inline
    /// against the local DB (see [`super::graph_fetch`]).
    pub graph_fetch: request_response::cbor::Behaviour<GraphFetchRequest, GraphFetchResponse>,
    /// Pull-based public profile fetch — `/alexandria/profile-fetch/1.0`
    /// request-response protocol with CBOR codec. A node serves its own
    /// owner's public profile by DID or username (see
    /// [`super::profile_fetch`]).
    pub profile_fetch: request_response::cbor::Behaviour<ProfileFetchRequest, ProfileFetchResponse>,
    /// Username receipt requests to the relay —
    /// `/alexandria/username-reg/1.0`. Outbound only: clients ask, the
    /// relay countersigns.
    pub username_reg: request_response::cbor::Behaviour<ReceiptRequest, ReceiptResponse>,
    /// Guardian link + oversight sync — `/alexandria/guardian/1.0`
    /// request-response protocol between a minor ward and their
    /// parent/guardian. Cross-user; payloads sealed under the per-link
    /// key (see [`super::guardian`]).
    pub guardian: request_response::cbor::Behaviour<GuardianRequest, GuardianResponse>,
}

/// The running P2P network node.
///
/// Wraps a libp2p Swarm and provides high-level methods for
/// publishing messages, querying peers, and managing the node lifecycle.
pub struct P2pNode {
    /// Channel to send commands to the swarm event loop.
    command_tx: mpsc::Sender<SwarmCommand>,
    /// The local PeerId.
    peer_id: PeerId,
    /// Whether the node is running.
    running: bool,
}

/// Commands sent to the swarm event loop from the application layer.
pub enum SwarmCommand {
    /// Publish a message on a gossip topic.
    Publish {
        topic: String,
        data: Vec<u8>,
        reply: mpsc::Sender<Result<(), NetworkError>>,
    },
    /// Dynamically subscribe to a gossip topic.
    Subscribe {
        topic: String,
        reply: mpsc::Sender<Result<(), NetworkError>>,
    },
    /// Dynamically unsubscribe from a gossip topic.
    Unsubscribe {
        topic: String,
        reply: mpsc::Sender<Result<(), NetworkError>>,
    },
    /// Get the current network status.
    Status { reply: mpsc::Sender<NetworkStatus> },
    /// Get the list of connected peers.
    Peers { reply: mpsc::Sender<Vec<PeerId>> },
    /// Get every peer in the Kademlia routing table (connected or not).
    /// Request-response sends to these auto-dial via their known addrs.
    KnownPeers { reply: mpsc::Sender<Vec<PeerId>> },
    /// Kick an on-demand provider-discovery sweep: republish our own
    /// provider record + query for Alexandria peers. Discovered peers
    /// are dialed by the existing GetProviders handler. Fire-and-forget
    /// — the caller settles, then reads `KnownPeers`.
    DiscoverPeers { reply: mpsc::Sender<()> },
    /// Send a vc-fetch request to a peer. The reply oneshot
    /// resolves when the peer's response (or an outbound failure)
    /// comes back.
    SendFetchRequest {
        peer: PeerId,
        request: FetchRequest,
        reply: oneshot::Sender<Result<FetchResponse, NetworkError>>,
    },
    /// Reply to an inbound vc-fetch request. The application layer
    /// receives the request via `P2pEvent::FetchRequestReceived`
    /// and calls back through this command.
    SendFetchResponse {
        channel: ResponseChannel<FetchResponse>,
        response: FetchResponse,
    },
    /// Send a sync request to a paired peer. The reply oneshot
    /// resolves with the peer's sealed response (or a failure).
    SendSyncRequest {
        peer: PeerId,
        request: SyncRequest,
        reply: oneshot::Sender<Result<SyncResponse, NetworkError>>,
    },
    /// Reply to an inbound sync request (handled inline against the DB
    /// in the event loop, like vc-fetch).
    SendSyncResponse {
        channel: ResponseChannel<SyncResponse>,
        response: SyncResponse,
    },
    /// Send a guardian request (link / activity push / pull / revoke)
    /// to the counterparty. The reply oneshot resolves with the peer's
    /// [`GuardianResponse`] (or an outbound failure).
    SendGuardianRequest {
        peer: PeerId,
        request: GuardianRequest,
        reply: oneshot::Sender<Result<GuardianResponse, NetworkError>>,
    },
    /// Send a graph-fetch request to a peer. The reply oneshot resolves
    /// with the peer's [`GraphFetchResponse`] (or an outbound failure).
    SendGraphFetchRequest {
        peer: PeerId,
        request: GraphFetchRequest,
        reply: oneshot::Sender<Result<GraphFetchResponse, NetworkError>>,
    },
    /// Reply to an inbound graph-fetch request (handled inline against
    /// the DB in the event loop, like vc-fetch).
    SendGraphFetchResponse {
        channel: ResponseChannel<GraphFetchResponse>,
        response: GraphFetchResponse,
    },
    /// Send a profile-fetch request to a peer.
    SendProfileFetchRequest {
        peer: PeerId,
        request: ProfileFetchRequest,
        reply: oneshot::Sender<Result<ProfileFetchResponse, NetworkError>>,
    },
    /// Reply to an inbound profile-fetch request.
    SendProfileFetchResponse {
        channel: ResponseChannel<ProfileFetchResponse>,
        response: ProfileFetchResponse,
    },
    /// Request a username receipt from a relay.
    SendReceiptRequest {
        peer: PeerId,
        request: ReceiptRequest,
        reply: oneshot::Sender<Result<ReceiptResponse, NetworkError>>,
    },
    /// Store a record in the Kademlia DHT (username claims, etc.).
    PutDhtRecord {
        key: Vec<u8>,
        value: Vec<u8>,
        reply: oneshot::Sender<Result<(), NetworkError>>,
    },
    /// Fetch all records for a DHT key. Conflicting writers can leave
    /// multiple records — the caller picks the winner.
    GetDhtRecords {
        key: Vec<u8>,
        reply: oneshot::Sender<Result<Vec<Vec<u8>>, NetworkError>>,
    },
    /// Dial a peer at the given addresses (used to reach a freshly
    /// paired device whose addresses came in via the pairing code).
    ConnectPeer {
        peer: PeerId,
        addrs: Vec<libp2p::Multiaddr>,
        reply: mpsc::Sender<Result<(), NetworkError>>,
    },
    /// Shutdown the node.
    Shutdown,
}

/// Derive a per-device libp2p Ed25519 keypair from the Cardano payment key
/// and a device-local secret.
///
/// The DID/wallet identity stays linked to the payment key, but each device
/// install picks up its own 32-byte `device_id` (see `super::device_id`),
/// so two devices unlocked with the same vault end up with distinct PeerIds.
/// This avoids the libp2p-layer collision where two installs would otherwise
/// race for one connection slot under a single PeerId.
///
/// HKDF-SHA256(ikm = payment_key, salt = device_id, info = "alexandria-libp2p-v1")
/// → 32 bytes → Ed25519 scalar.
pub fn derive_libp2p_keypair(
    payment_key_bytes: &[u8; 32],
    device_id: &[u8; 32],
) -> Result<Keypair, NetworkError> {
    use hkdf::Hkdf;
    use sha2::Sha256;

    let hk = Hkdf::<Sha256>::new(Some(device_id), payment_key_bytes);
    let mut seed = [0u8; 32];
    hk.expand(b"alexandria-libp2p-v1", &mut seed)
        .map_err(|e| NetworkError::Identity(format!("hkdf expand: {e}")))?;
    Keypair::ed25519_from_bytes(&mut seed).map_err(|e| NetworkError::Identity(e.to_string()))
}

/// Build and start the P2P node.
///
/// Returns a `P2pNode` handle and spawns the swarm event loop
/// as a background tokio task. Includes NAT traversal (AutoNAT,
/// circuit relay v2, DCUtR) and GossipSub peer scoring.
/// A known peer to dial on startup (loaded from the `peers` table).
pub struct KnownPeer {
    pub peer_id: String,
    pub addresses: Vec<String>,
}

/// Start the libp2p swarm.
///
/// `db` is the active-profile database handle. Pass `Some(...)` in
/// production so that:
///
/// - the gossip validator's registry-backed identity check for
///   privileged topics is active (without it the check fail-opens
///   and any peer can publish taxonomy/governance/Sentinel-prior
///   traffic),
/// - inbound `/alexandria/vc-fetch/1.0` requests are answered against
///   local credentials (without it the swarm replies
///   `FetchResponse::NotFound` to every request).
///
/// Pass `None` only from tests / dev tooling that intentionally want
/// the no-DB behaviour — `start_node_with_db` logs a `WARN` in that
/// case so a misconfigured production build is loud rather than
/// silently dormant. The previous `start_node(...)` convenience
/// wrapper that hard-coded `None` was deleted in 2026-05 after it
/// shipped silently as the production path for a release window.
pub async fn start_node_with_db(
    keypair: Keypair,
    event_tx: mpsc::Sender<P2pEvent>,
    known_peers: Vec<KnownPeer>,
    db: Option<Arc<StdMutex<Option<Database>>>>,
    dht_server: bool,
) -> Result<P2pNode, NetworkError> {
    let peer_id = keypair.public().to_peer_id();
    diag::log(&format!("start_node: PeerId: {peer_id}"));

    // Build the swarm with transport + relay client.
    //
    // Desktop: QUIC transport (UDP, built-in TLS 1.3)
    // Android: TCP + QUIC + relay client
    // iOS:     TCP + relay client (QUIC still crashes there)
    //
    // The relay client transport enables connecting via circuit relay v2
    // when behind NAT. It requires noise + yamux for the relay hop.
    diag::log("start_node: building swarm...");

    #[cfg(desktop)]
    let mut swarm = SwarmBuilder::with_existing_identity(keypair.clone())
        .with_tokio()
        .with_tcp(
            libp2p::tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )
        .map_err(|e| NetworkError::SwarmBuild(format!("tcp: {e}")))?
        .with_quic()
        .with_relay_client(noise::Config::new, yamux::Config::default)
        .map_err(|e| NetworkError::SwarmBuild(format!("relay client: {e}")))?
        .with_behaviour(|key, relay_behaviour| {
            build_behaviour(key, peer_id, relay_behaviour, dht_server)
        })
        .map_err(|e| NetworkError::SwarmBuild(e.to_string()))?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(300)))
        .build();

    #[cfg(target_os = "android")]
    let mut swarm = {
        diag::log("start_node: with_tcp...");
        let builder = SwarmBuilder::with_existing_identity(keypair.clone())
            .with_tokio()
            .with_tcp(
                libp2p::tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )
            .map_err(|e| {
                diag::log(&format!("start_node: with_tcp FAILED: {e}"));
                NetworkError::SwarmBuild(format!("tcp: {e}"))
            })?;
        diag::log("start_node: with_tcp OK");

        diag::log("start_node: with_quic...");
        let builder = builder.with_quic();
        diag::log("start_node: with_quic OK");

        diag::log("start_node: with_relay_client...");
        let builder = builder
            .with_relay_client(noise::Config::new, yamux::Config::default)
            .map_err(|e| {
                diag::log(&format!("start_node: with_relay_client FAILED: {e}"));
                NetworkError::SwarmBuild(format!("relay client: {e}"))
            })?;
        diag::log("start_node: with_relay_client OK");

        diag::log("start_node: with_behaviour...");
        let builder = builder
            .with_behaviour(|key, relay_behaviour| {
                diag::log("start_node: inside build_behaviour callback");
                build_behaviour(key, peer_id, relay_behaviour, dht_server)
            })
            .map_err(|e| {
                diag::log(&format!("start_node: with_behaviour FAILED: {e}"));
                NetworkError::SwarmBuild(e.to_string())
            })?;
        diag::log("start_node: with_behaviour OK");

        diag::log("start_node: building final swarm...");
        builder
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(300)))
            .build()
    };

    #[cfg(all(mobile, not(target_os = "android")))]
    let mut swarm = {
        diag::log("start_node: with_tcp...");
        let builder = SwarmBuilder::with_existing_identity(keypair.clone())
            .with_tokio()
            .with_tcp(
                libp2p::tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )
            .map_err(|e| {
                diag::log(&format!("start_node: with_tcp FAILED: {e}"));
                NetworkError::SwarmBuild(format!("tcp: {e}"))
            })?;
        diag::log("start_node: with_tcp OK");

        diag::log("start_node: with_relay_client...");
        let builder = builder
            .with_relay_client(noise::Config::new, yamux::Config::default)
            .map_err(|e| {
                diag::log(&format!("start_node: with_relay_client FAILED: {e}"));
                NetworkError::SwarmBuild(format!("relay client: {e}"))
            })?;
        diag::log("start_node: with_relay_client OK");

        diag::log("start_node: with_behaviour...");
        let builder = builder
            .with_behaviour(|key, relay_behaviour| {
                diag::log("start_node: inside build_behaviour callback");
                build_behaviour(key, peer_id, relay_behaviour, dht_server)
            })
            .map_err(|e| {
                diag::log(&format!("start_node: with_behaviour FAILED: {e}"));
                NetworkError::SwarmBuild(e.to_string())
            })?;
        diag::log("start_node: with_behaviour OK");

        diag::log("start_node: building final swarm...");
        builder
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(300)))
            .build()
    };

    diag::log("start_node: swarm built, subscribing to topics...");

    // Subscribe to all gossip topics
    for topic_str in ALL_TOPICS {
        let topic = IdentTopic::new(*topic_str);
        swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&topic)
            .map_err(|e| NetworkError::Subscribe(format!("topic '{}': {}", topic_str, e)))?;
    }

    diag::log("start_node: topics subscribed, binding listener...");

    // Listen on all interfaces, OS-assigned port.
    // Desktop: QUIC (UDP). Mobile: TCP.
    #[cfg(desktop)]
    {
        // Listen on both TCP and QUIC so desktop can connect to mobile (TCP) and other desktops (QUIC)
        let tcp_addr: libp2p::Multiaddr = "/ip4/0.0.0.0/tcp/0".parse().expect("valid multiaddr");
        swarm
            .listen_on(tcp_addr)
            .map_err(|e| NetworkError::Listen(e.to_string()))?;

        let quic_addr: libp2p::Multiaddr = "/ip4/0.0.0.0/udp/0/quic-v1"
            .parse()
            .expect("valid multiaddr");
        swarm
            .listen_on(quic_addr)
            .map_err(|e| NetworkError::Listen(e.to_string()))?;

        if let Ok(ipv6_addr) = "/ip6/::/tcp/0".parse::<libp2p::Multiaddr>() {
            let _ = swarm.listen_on(ipv6_addr);
        }
        if let Ok(ipv6_addr) = "/ip6/::/udp/0/quic-v1".parse::<libp2p::Multiaddr>() {
            let _ = swarm.listen_on(ipv6_addr);
        }
    }

    #[cfg(target_os = "android")]
    {
        diag::log("start_node: listen_on /ip4/0.0.0.0/tcp/0...");
        let listen_addr: libp2p::Multiaddr = "/ip4/0.0.0.0/tcp/0".parse().expect("valid multiaddr");
        swarm.listen_on(listen_addr).map_err(|e| {
            diag::log(&format!("start_node: listen_on FAILED: {e}"));
            NetworkError::Listen(e.to_string())
        })?;
        diag::log("start_node: TCP listen_on OK");

        diag::log("start_node: listen_on /ip4/0.0.0.0/udp/0/quic-v1...");
        let quic_addr: libp2p::Multiaddr = "/ip4/0.0.0.0/udp/0/quic-v1"
            .parse()
            .expect("valid multiaddr");
        swarm.listen_on(quic_addr).map_err(|e| {
            diag::log(&format!("start_node: QUIC listen_on FAILED: {e}"));
            NetworkError::Listen(e.to_string())
        })?;
        diag::log("start_node: QUIC listen_on OK");

        if let Ok(ipv6_addr) = "/ip6/::/tcp/0".parse::<libp2p::Multiaddr>() {
            let _ = swarm.listen_on(ipv6_addr);
        }
        if let Ok(ipv6_addr) = "/ip6/::/udp/0/quic-v1".parse::<libp2p::Multiaddr>() {
            let _ = swarm.listen_on(ipv6_addr);
        }
        diag::log("start_node: IPv6 TCP/QUIC listen attempted");
    }

    #[cfg(all(mobile, not(target_os = "android")))]
    {
        diag::log("start_node: listen_on /ip4/0.0.0.0/tcp/0...");
        let listen_addr: libp2p::Multiaddr = "/ip4/0.0.0.0/tcp/0".parse().expect("valid multiaddr");
        swarm.listen_on(listen_addr).map_err(|e| {
            diag::log(&format!("start_node: listen_on FAILED: {e}"));
            NetworkError::Listen(e.to_string())
        })?;
        diag::log("start_node: listen_on OK");

        if let Ok(ipv6_addr) = "/ip6/::/tcp/0".parse::<libp2p::Multiaddr>() {
            let _ = swarm.listen_on(ipv6_addr);
        }
        diag::log("start_node: IPv6 listen attempted");
    }

    diag::log("start_node: listener bound, dialing bootstrap peers...");

    // Dial bootstrap/relay peers for internet-wide discovery.
    // These are public relay nodes that all peers connect to first.
    // Through Kademlia DHT on the relay, peers discover each other.
    let bootstrap_addrs = super::discovery::bootstrap_peers();
    for addr in &bootstrap_addrs {
        diag::log(&format!("start_node: dialing bootstrap {addr}"));
        match swarm.dial(addr.clone()) {
            Ok(_) => diag::log(&format!("start_node: dial initiated for {addr}")),
            Err(e) => diag::log(&format!("start_node: dial failed for {addr}: {e}")),
        }
    }

    // Kick off Kademlia bootstrap to populate the routing table.
    // This queries the DHT for our own PeerId, discovering nearby peers.
    if !bootstrap_addrs.is_empty() {
        match swarm.behaviour_mut().kademlia.bootstrap() {
            Ok(_) => diag::log("start_node: kademlia bootstrap started"),
            Err(e) => diag::log(&format!("start_node: kademlia bootstrap failed: {e}")),
        }
    }

    // Dial known Alexandria peers from previous sessions.
    // This is the primary discovery mechanism — once two nodes connect
    // by any means (DHT, peer exchange, relay), they remember each other
    // and reconnect on next startup.
    if !known_peers.is_empty() {
        diag::log(&format!(
            "start_node: dialing {} known peers...",
            known_peers.len()
        ));
        for kp in &known_peers {
            if let Ok(pid) = kp.peer_id.parse::<PeerId>() {
                if pid == peer_id {
                    continue; // skip self
                }
                for addr_str in &kp.addresses {
                    if let Ok(addr) = addr_str.parse::<libp2p::Multiaddr>() {
                        let dial_addr = addr.with(libp2p::multiaddr::Protocol::P2p(pid));
                        match swarm.dial(dial_addr.clone()) {
                            Ok(_) => {
                                diag::log(&format!("start_node: dialing known peer {}", kp.peer_id))
                            }
                            Err(e) => diag::log(&format!(
                                "start_node: known peer dial failed {}: {e}",
                                kp.peer_id
                            )),
                        }
                    }
                }
            }
        }
    }

    diag::log("start_node: spawning event loop...");

    // Create command channel
    let (command_tx, command_rx) = mpsc::channel::<SwarmCommand>(256);

    // Create the message validator (shared via Arc for the event
    // loop). When a DB handle is available, wire it so privileged-
    // topic messages can be authorized against
    // `stake_pubkey_registry`; otherwise the validator fails-open on
    // the identity check.
    //
    // Production callers MUST pass a DB. We log a `WARN` on the
    // no-DB path so a misconfigured release is loud at the very
    // first line of every node startup — silent dormancy is exactly
    // how a prior release shipped with the registry check
    // accidentally disabled.
    let validator = Arc::new(match db.clone() {
        Some(handle) => MessageValidator::with_db(handle),
        None => {
            log::warn!(
                "start_node_with_db: no DB handle provided — privileged-topic identity \
                 binding will fail-open AND inbound /alexandria/vc-fetch/1.0 requests \
                 will all reply NotFound. Production callers must pass Some(db); \
                 the no-DB path exists for tests / dev tooling only."
            );
            MessageValidator::new()
        }
    });

    // Spawn the swarm event loop. The db handle (None for tests /
    // dev tooling, populated by every production call site) lets
    // the loop answer inbound vc-fetch requests synchronously.
    tokio::spawn(swarm_event_loop(
        swarm, command_rx, event_tx, validator, db, dht_server,
    ));

    diag::log("start_node: event loop spawned, node running");

    Ok(P2pNode {
        command_tx,
        peer_id,
        running: true,
    })
}

/// Build the composed network behaviour.
///
/// Includes GossipSub with peer scoring, Kademlia (private Alexandria DHT),
/// Identify, AutoNAT, relay client, and DCUtR.
fn build_behaviour(
    keypair: &Keypair,
    peer_id: PeerId,
    relay_behaviour: relay::client::Behaviour,
    contribute: bool,
) -> Result<AlexandriaBehaviour, Box<dyn std::error::Error + Send + Sync>> {
    diag::log("build_behaviour: creating gossipsub config...");

    // GossipSub configuration — tuned for a learning platform with:
    // - Low message frequency (not real-time chat)
    // - High message value (credentials, evidence, taxonomy)
    // - Small-to-medium network size (hundreds to low thousands)
    let gossipsub_config = gossipsub::ConfigBuilder::default()
        .heartbeat_interval(Duration::from_secs(1))
        .validation_mode(ValidationMode::Strict)
        .message_id_fn(|msg: &gossipsub::Message| {
            // Deduplicate by Blake2b-256 hash of data (spec §7.3)
            let hash = blake2b_256(&msg.data);
            MessageId::from(hex::encode(hash))
        })
        .max_transmit_size(65536) // 64KB max message size
        // Mesh parameters: target 4 peers in mesh (small network),
        // allow down to 2 before grafting, up to 8 before pruning.
        .mesh_n(4)
        .mesh_n_low(2)
        .mesh_n_high(8)
        // Publish to ALL peers (not just mesh members) to ensure
        // delivery in sparse networks. Critical for early deployment
        // when mesh membership may be incomplete.
        .flood_publish(true)
        // History: keep 5 heartbeats of message IDs and gossip 3
        // (default is 5/3). Ensures good dedup without memory bloat.
        .history_length(5)
        .history_gossip(3)
        .build()
        .map_err(|e| format!("gossipsub config: {e}"))?;

    diag::log("build_behaviour: creating gossipsub behaviour...");

    let mut gossipsub = gossipsub::Behaviour::new(
        MessageAuthenticity::Signed(keypair.clone()),
        gossipsub_config,
    )
    .map_err(|e| format!("gossipsub behaviour: {e}"))?;

    diag::log("build_behaviour: setting peer scoring...");

    // Enable peer scoring per spec §7.3: "Peers that repeatedly send
    // invalid messages are scored down, eventually disconnected."
    let score_params = build_peer_score_params();
    let score_thresholds = build_peer_score_thresholds();
    gossipsub
        .with_peer_score(score_params, score_thresholds)
        .map_err(|e| format!("gossipsub peer scoring: {e}"))?;

    diag::log("build_behaviour: creating kademlia...");

    // Kademlia DHT — private Alexandria DHT for peer discovery.
    // Using `/alexandria/kad/1.0` isolates us from the public IPFS DHT.
    // All nodes on this DHT are Alexandria nodes. The relay server is
    // the bootstrap node and always runs in Kademlia server mode.
    let mut kademlia_config = kad::Config::new(libp2p::StreamProtocol::new("/alexandria/kad/1.0"));
    kademlia_config.set_query_timeout(Duration::from_secs(60));
    // Lower replication factor for small-network friendliness (default is 20)
    kademlia_config.set_replication_factor(std::num::NonZeroUsize::new(8).unwrap());
    // Provider records expire after 24h (default). Re-publication every
    // 3 min in the event loop keeps them alive. This ensures stale peers
    // are cleaned up if they go offline without graceful shutdown.
    kademlia_config.set_provider_record_ttl(Some(Duration::from_secs(24 * 3600)));
    // Republish provider records every 12h to refresh TTL on other nodes.
    kademlia_config.set_provider_publication_interval(Some(Duration::from_secs(12 * 3600)));
    // Surface inbound PutRecord requests so DHT-server nodes can store
    // and mirror them explicitly (no-op for client-mode nodes — they
    // never receive puts).
    kademlia_config.set_record_filtering(kad::StoreInserts::FilterBoth);
    // Mobile is ALWAYS a pure DHT client: backgrounded apps churn the
    // routing table (peers route into dead entries) and serving over
    // CGNAT rides relay circuits anyway. Desktop stays in auto mode
    // unless the owner opts into serving (handled in the event loop).
    #[cfg(any(target_os = "ios", target_os = "android"))]
    let kademlia = {
        let mut k =
            kad::Behaviour::with_config(peer_id, MemoryStore::new(peer_id), kademlia_config);
        k.set_mode(Some(kad::Mode::Client));
        k
    };
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    let kademlia = kad::Behaviour::with_config(peer_id, MemoryStore::new(peer_id), kademlia_config);

    diag::log("build_behaviour: creating identify...");

    // Identify protocol (needed by GossipSub and Kademlia)
    let identify = identify::Behaviour::new(
        identify::Config::new("/alexandria/id/1.0".to_string(), keypair.public())
            .with_agent_version(identify_agent_version())
            .with_push_listen_addr_updates(true),
    );

    diag::log("build_behaviour: creating autonat...");

    // AutoNAT — peer-assisted NAT detection (spec §7.5)
    let autonat = autonat::Behaviour::new(peer_id, build_autonat_config());

    diag::log("build_behaviour: creating relay server...");

    // Relay server — any publicly reachable node automatically relays
    // traffic for NATted peers. The server is inert on NATted nodes
    // (nobody can reach it to make HOP requests). When AutoNAT detects
    // public reachability, the relay server naturally becomes active.
    //
    // When the user opts out of contributing (`contribute == false`),
    // grant no reservations or circuits so a publicly-reachable node
    // still refuses to carry others' traffic. The behaviour stays in the
    // swarm (the type is fixed) but becomes a no-op responder.
    let relay_config = {
        let mut cfg = relay::Config::default();
        if !contribute {
            cfg.max_reservations = 0;
            cfg.max_circuits = 0;
        }
        cfg
    };
    let relay_server = relay::Behaviour::new(peer_id, relay_config);

    diag::log("build_behaviour: creating dcutr...");

    // DCUtR — upgrade relayed connections to direct via hole punching
    let dcutr = dcutr::Behaviour::new(peer_id);

    // Pull-based credential fetch (§9 vc-fetch). CBOR codec is
    // ergonomic + small over the wire. Full bidirectional support so
    // every node can both serve and request credentials.
    let vc_fetch = request_response::cbor::Behaviour::<FetchRequest, FetchResponse>::new(
        [(
            StreamProtocol::new("/alexandria/vc-fetch/1.0"),
            ProtocolSupport::Full,
        )],
        request_response::Config::default(),
    );

    // Cross-device sync (§ device pairing). CBOR codec, full
    // bidirectional support — every node both serves and initiates
    // sync with its own paired devices.
    let device_sync = request_response::cbor::Behaviour::<SyncRequest, SyncResponse>::new(
        [(
            StreamProtocol::new("/alexandria/sync/1.0"),
            ProtocolSupport::Full,
        )],
        request_response::Config::default(),
    );

    // Guardian link + oversight sync (§ parental controls). CBOR
    // codec, full bidirectional support — a ward serves pulls and
    // link requests; a guardian serves activity pushes.
    let guardian = request_response::cbor::Behaviour::<GuardianRequest, GuardianResponse>::new(
        [(
            StreamProtocol::new("/alexandria/guardian/1.0"),
            ProtocolSupport::Full,
        )],
        request_response::Config::default(),
    );

    // Public skill-graph fetch (§ graph-fetch). CBOR codec, full
    // bidirectional support — every node both serves its owner's graph
    // and requests other owners' graphs.
    let graph_fetch =
        request_response::cbor::Behaviour::<GraphFetchRequest, GraphFetchResponse>::new(
            [(
                StreamProtocol::new("/alexandria/graph-fetch/1.0"),
                ProtocolSupport::Full,
            )],
            request_response::Config::default(),
        );

    // Username receipt protocol — outbound to the relay only.
    let username_reg = request_response::cbor::Behaviour::<ReceiptRequest, ReceiptResponse>::new(
        [(
            StreamProtocol::new("/alexandria/username-reg/1.0"),
            ProtocolSupport::Outbound,
        )],
        request_response::Config::default(),
    );

    // Public profile fetch (§ profile-fetch). CBOR codec, full
    // bidirectional support — every node serves its owner's profile and
    // requests other owners' profiles by DID or username.
    let profile_fetch =
        request_response::cbor::Behaviour::<ProfileFetchRequest, ProfileFetchResponse>::new(
            [(
                StreamProtocol::new("/alexandria/profile-fetch/1.0"),
                ProtocolSupport::Full,
            )],
            request_response::Config::default(),
        );

    diag::log("build_behaviour: all sub-behaviours created OK");

    Ok(AlexandriaBehaviour {
        gossipsub,
        kademlia,
        identify,
        autonat,
        relay_server,
        relay_client: relay_behaviour,
        vc_fetch,
        device_sync,
        graph_fetch,
        profile_fetch,
        username_reg,
        guardian,
        dcutr,
    })
}

fn refresh_provider_records(
    swarm: &mut Swarm<AlexandriaBehaviour>,
    namespace_key: &kad::RecordKey,
    reason: &str,
) {
    diag::log(&format!("Kademlia: refreshing providers ({reason})"));
    if let Err(e) = swarm
        .behaviour_mut()
        .kademlia
        .start_providing(namespace_key.clone())
    {
        diag::log(&format!("Kademlia: start_providing failed ({reason}): {e}"));
    }
    let _ = swarm
        .behaviour_mut()
        .kademlia
        .get_providers(namespace_key.clone());
}

fn dial_peer_with_relay_fallbacks(
    swarm: &mut Swarm<AlexandriaBehaviour>,
    peer: &PeerId,
    reason: &str,
) {
    if *peer == *swarm.local_peer_id() || swarm.is_connected(peer) {
        return;
    }

    diag::log(&format!(
        "{reason}: dialing peer {peer} via direct and relay-circuit paths"
    ));
    swarm.behaviour_mut().gossipsub.add_explicit_peer(peer);

    if let Err(e) = swarm.dial(*peer) {
        diag::log(&format!("{reason}: direct dial failed for {peer}: {e}"));
    }

    for addr in super::discovery::relay_circuit_dial_addrs(peer) {
        if let Err(e) = swarm.dial(addr.clone()) {
            log::debug!("{reason}: relay-circuit dial {addr} failed: {e}");
        }
    }
}

fn is_circuit_listener(address: &libp2p::Multiaddr) -> bool {
    address.to_string().contains("p2p-circuit")
}

/// The main swarm event loop.
///
/// Runs as a background task, processing both swarm events and
/// application commands. Events are forwarded to the application
/// via the `event_tx` channel.
async fn swarm_event_loop(
    mut swarm: Swarm<AlexandriaBehaviour>,
    mut command_rx: mpsc::Receiver<SwarmCommand>,
    event_tx: mpsc::Sender<P2pEvent>,
    validator: Arc<MessageValidator>,
    db: Option<Arc<std::sync::Mutex<Option<Database>>>>,
    dht_server: bool,
) {
    use libp2p::swarm::SwarmEvent;

    use super::types::{PeerExchangeMessage, TOPIC_PEER_EXCHANGE};

    // Desktop DHT server (opt-in): announce server mode and warm-load
    // the persistent record mirror so this node's slice of the DHT
    // survives restarts.
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    if dht_server {
        swarm
            .behaviour_mut()
            .kademlia
            .set_mode(Some(kad::Mode::Server));
        if let Some(db_arc) = db.as_ref() {
            if let Ok(guard) = db_arc.lock() {
                if let Some(database) = guard.as_ref() {
                    let records: Vec<(Vec<u8>, Vec<u8>)> = database
                        .conn()
                        .prepare("SELECT key, value FROM dht_records")
                        .ok()
                        .map(|mut stmt| {
                            stmt.query_map([], |r| {
                                Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?))
                            })
                            .map(|rows| rows.filter_map(|r| r.ok()).collect())
                            .unwrap_or_default()
                        })
                        .unwrap_or_default();
                    let n = records.len();
                    for (key, value) in records {
                        let record = kad::Record {
                            key: kad::RecordKey::new(&key),
                            value,
                            publisher: None,
                            expires: None,
                        };
                        use libp2p::kad::store::RecordStore;
                        let _ = swarm.behaviour_mut().kademlia.store_mut().put(record);
                    }
                    if n > 0 {
                        diag::log(&format!("DHT server: warm-loaded {n} records"));
                    }
                }
            }
        }
    }
    #[cfg(any(target_os = "ios", target_os = "android"))]
    let _ = dht_server; // mobile never serves

    // Per-peer gossip rate limiter (token bucket: 20 msgs, 1 refill/3s)
    let mut rate_limiter = PeerRateLimiter::new();

    // vc-fetch outbound request bookkeeping. When the application
    // calls `P2pNode::fetch_credential`, we get back a libp2p
    // OutboundRequestId; we stash the caller's reply oneshot here
    // and resolve it when the matching Response/Failure event fires.
    let mut outbound_fetch_replies: HashMap<
        libp2p::request_response::OutboundRequestId,
        oneshot::Sender<Result<FetchResponse, NetworkError>>,
    > = HashMap::new();

    // Same bookkeeping for outbound device-sync requests.
    let mut outbound_sync_replies: HashMap<
        libp2p::request_response::OutboundRequestId,
        oneshot::Sender<Result<SyncResponse, NetworkError>>,
    > = HashMap::new();

    // Same bookkeeping for outbound guardian requests.
    let mut outbound_guardian_replies: HashMap<
        libp2p::request_response::OutboundRequestId,
        oneshot::Sender<Result<GuardianResponse, NetworkError>>,
    > = HashMap::new();

    // Same bookkeeping for outbound graph-fetch requests.
    let mut outbound_graph_fetch_replies: HashMap<
        libp2p::request_response::OutboundRequestId,
        oneshot::Sender<Result<GraphFetchResponse, NetworkError>>,
    > = HashMap::new();

    // Same bookkeeping for outbound profile-fetch requests.
    let mut outbound_profile_fetch_replies: HashMap<
        libp2p::request_response::OutboundRequestId,
        oneshot::Sender<Result<ProfileFetchResponse, NetworkError>>,
    > = HashMap::new();

    // Outbound username receipt requests to the relay.
    let mut outbound_receipt_replies: HashMap<
        libp2p::request_response::OutboundRequestId,
        oneshot::Sender<Result<ReceiptResponse, NetworkError>>,
    > = HashMap::new();

    // Pending DHT record queries (username registry). Get queries
    // accumulate records until the query finishes.
    type GetRecordsReply = oneshot::Sender<Result<Vec<Vec<u8>>, NetworkError>>;
    let mut pending_put_queries: HashMap<kad::QueryId, oneshot::Sender<Result<(), NetworkError>>> =
        HashMap::new();
    let mut pending_get_queries: HashMap<kad::QueryId, (GetRecordsReply, Vec<Vec<u8>>)> =
        HashMap::new();

    // Track NAT state locally for the Status command
    let mut current_nat_state = NatState::Unknown;
    let mut relay_addrs: Vec<String> = Vec::new();

    // Track which relays we've already requested reservations from.
    // We request a reservation from each relay after the Identify
    // handshake confirms we're connected to it.
    let relay_peer_ids = super::discovery::relay_peer_ids();
    let mut relay_reservations_requested: std::collections::HashSet<libp2p::PeerId> =
        std::collections::HashSet::new();

    // Auto-discovered relays (connectivity only): nodes that contribute
    // advertise under the relay namespace; we adopt a capped, reputation
    // -scored subset and request reservations from them via the same
    // identify flow as the built-in relays.
    let relay_namespace_key = super::discovery::relay_namespace_key();
    let mut discovered_relays = super::relay_discovery::DiscoveredRelays::new();
    let mut relay_providers_query: Option<kad::QueryId> = None;

    // Periodic Kademlia bootstrap — re-run every 5 minutes to keep
    // the routing table fresh and discover new peers.
    let mut kad_bootstrap_interval = tokio::time::interval(Duration::from_secs(300));
    kad_bootstrap_interval.tick().await; // consume the immediate first tick

    // Provider publish + query cadence:
    // 1. Start quickly after boot so fresh mobile sessions become visible fast
    // 2. Stay aggressive for the first couple of minutes while peers join
    // 3. Fall back to a quieter steady-state interval afterward
    let namespace_key = super::discovery::namespace_key();
    let mut provider_interval = tokio::time::interval(PROVIDER_REFRESH_INTERVAL);
    provider_interval.tick().await; // consume the immediate first tick
    let mut provider_warmup_interval = tokio::time::interval(PROVIDER_WARMUP_INTERVAL);
    provider_warmup_interval.tick().await; // consume the immediate first tick
    let provider_warmup_deadline = tokio::time::Instant::now() + PROVIDER_WARMUP_WINDOW;
    let provider_initial = tokio::time::sleep(PROVIDER_INITIAL_DELAY);
    tokio::pin!(provider_initial);
    let mut initial_provider_done = false;

    // Periodic peer exchange — broadcast our addresses every 30s
    // so peers-of-peers can discover us transitively via GossipSub.
    let mut peer_exchange_interval = tokio::time::interval(PEER_EXCHANGE_INTERVAL);
    peer_exchange_interval.tick().await; // consume the immediate first tick

    /// Helper: build and publish a peer exchange message.
    fn publish_peer_exchange(swarm: &mut Swarm<AlexandriaBehaviour>) {
        let addresses: Vec<String> = swarm.listeners().map(|a| a.to_string()).collect();
        if addresses.is_empty() {
            return;
        }
        let addr_count = addresses.len();
        let msg = PeerExchangeMessage {
            peer_id: swarm.local_peer_id().to_string(),
            addresses,
        };
        if let Ok(data) = serde_json::to_vec(&msg) {
            let topic = IdentTopic::new(TOPIC_PEER_EXCHANGE);
            match swarm.behaviour_mut().gossipsub.publish(topic, data) {
                Ok(_) => {
                    diag::log(&format!("Peer exchange: broadcast {addr_count} addresses"));
                }
                Err(e) => {
                    // InsufficientPeers is expected when no gossipsub peers are
                    // subscribed to this topic yet — not an error.
                    log::debug!("Peer exchange: publish failed (expected if no peers): {e}");
                }
            }
        }
    }

    /// Helper: handle an incoming peer exchange message.
    fn handle_peer_exchange(swarm: &mut Swarm<AlexandriaBehaviour>, data: &[u8]) {
        let msg: PeerExchangeMessage = match serde_json::from_slice(data) {
            Ok(m) => m,
            Err(e) => {
                log::debug!("Peer exchange: invalid message: {e}");
                return;
            }
        };
        let peer_id: PeerId = match msg.peer_id.parse() {
            Ok(p) => p,
            Err(_) => return,
        };
        if peer_id == *swarm.local_peer_id() {
            return; // ignore our own messages
        }
        if swarm.is_connected(&peer_id) {
            return; // already connected
        }
        diag::log(&format!(
            "Peer exchange: discovered {} with {} addrs, dialing...",
            msg.peer_id,
            msg.addresses.len()
        ));
        // Try to dial each address
        for addr_str in &msg.addresses {
            if let Ok(addr) = addr_str.parse::<libp2p::Multiaddr>() {
                let dial_addr = addr.with(libp2p::multiaddr::Protocol::P2p(peer_id));
                if let Err(e) = swarm.dial(dial_addr.clone()) {
                    log::debug!("Peer exchange: dial {dial_addr} failed: {e}");
                }
            }
        }
    }

    /// Helper: persist a peer's addresses to the DB so we can reconnect on next startup.
    fn save_peer_to_db(
        db: &Option<Arc<std::sync::Mutex<Option<Database>>>>,
        peer_id: &str,
        addresses: &[String],
    ) {
        if addresses.is_empty() {
            return;
        }
        let Some(db_arc) = db else { return };
        let Ok(db_guard) = db_arc.lock() else { return };
        let Some(db_lock) = db_guard.as_ref() else {
            return;
        };
        let addrs_json = serde_json::to_string(addresses).unwrap_or_default();
        let now = chrono::Utc::now().to_rfc3339();
        let _ = db_lock.conn().execute(
            "INSERT INTO peers (peer_id, addresses, last_seen)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(peer_id) DO UPDATE SET
               addresses = ?2,
               last_seen = ?3",
            rusqlite::params![peer_id, addrs_json, now],
        );
    }

    loop {
        tokio::select! {
            // Periodic Kademlia bootstrap
            _ = kad_bootstrap_interval.tick() => {
                let _ = swarm.behaviour_mut().kademlia.bootstrap();
            }
            // Initial provider publish shortly after startup
            _ = &mut provider_initial, if !initial_provider_done => {
                initial_provider_done = true;
                refresh_provider_records(&mut swarm, &namespace_key, "initial warm-up");
            }
            // Fast provider refresh during the startup window
            _ = provider_warmup_interval.tick(), if initial_provider_done && tokio::time::Instant::now() < provider_warmup_deadline => {
                refresh_provider_records(&mut swarm, &namespace_key, "warm-up interval");
            }
            // Periodic provider refresh
            _ = provider_interval.tick(), if initial_provider_done => {
                refresh_provider_records(&mut swarm, &namespace_key, "steady-state interval");
                // Advertise as a relay when contributing, and always look
                // for relays to adopt (capped + reputation-scored).
                if dht_server {
                    if let Err(e) = swarm
                        .behaviour_mut()
                        .kademlia
                        .start_providing(relay_namespace_key.clone())
                    {
                        diag::log(&format!("Kademlia: relay start_providing failed: {e}"));
                    }
                }
                relay_providers_query = Some(
                    swarm
                        .behaviour_mut()
                        .kademlia
                        .get_providers(relay_namespace_key.clone()),
                );
            }
            // Periodic peer exchange broadcast
            _ = peer_exchange_interval.tick() => {
                publish_peer_exchange(&mut swarm);
            }
            // Process commands from the application
            cmd = command_rx.recv() => {
                match cmd {
                    Some(SwarmCommand::Publish { topic, data, reply }) => {
                        let gossip_topic = IdentTopic::new(&topic);
                        let result = swarm
                            .behaviour_mut()
                            .gossipsub
                            .publish(gossip_topic, data)
                            .map(|_| ())
                            .map_err(|e| NetworkError::Publish(e.to_string()));
                        let _ = reply.send(result).await;
                    }
                    Some(SwarmCommand::Subscribe { topic, reply }) => {
                        let t = IdentTopic::new(&topic);
                        let result = swarm
                            .behaviour_mut()
                            .gossipsub
                            .subscribe(&t)
                            .map(|_| ())
                            .map_err(|e| NetworkError::Subscribe(e.to_string()));
                        let _ = reply.send(result).await;
                    }
                    Some(SwarmCommand::Unsubscribe { topic, reply }) => {
                        let t = IdentTopic::new(&topic);
                        swarm
                            .behaviour_mut()
                            .gossipsub
                            .unsubscribe(&t);
                        let _ = reply.send(Ok(())).await;
                    }
                    Some(SwarmCommand::Status { reply }) => {
                        let peer_id = *swarm.local_peer_id();
                        let connected = swarm.connected_peers().count();
                        let listeners: Vec<String> = swarm
                            .listeners()
                            .map(|a| a.to_string())
                            .collect();
                        let topics: Vec<String> = swarm
                            .behaviour()
                            .gossipsub
                            .topics()
                            .map(|t| t.to_string())
                            .collect();
                        let _ = reply.send(NetworkStatus {
                            is_running: true,
                            peer_id: Some(peer_id.to_string()),
                            connected_peers: connected,
                            listening_addresses: listeners,
                            subscribed_topics: topics,
                            nat_status: current_nat_state.clone(),
                            relay_addresses: relay_addrs.clone(),
                        }).await;
                    }
                    Some(SwarmCommand::Peers { reply }) => {
                        let peers: Vec<PeerId> = swarm
                            .connected_peers()
                            .cloned()
                            .collect();
                        let _ = reply.send(peers).await;
                    }
                    Some(SwarmCommand::KnownPeers { reply }) => {
                        let mut peers: Vec<PeerId> = swarm
                            .connected_peers()
                            .cloned()
                            .collect();
                        let in_buckets: Vec<PeerId> = swarm
                            .behaviour_mut()
                            .kademlia
                            .kbuckets()
                            .flat_map(|bucket| {
                                bucket
                                    .iter()
                                    .map(|entry| *entry.node.key.preimage())
                                    .collect::<Vec<_>>()
                            })
                            .collect();
                        for p in in_buckets {
                            if !peers.contains(&p) {
                                peers.push(p);
                            }
                        }
                        let _ = reply.send(peers).await;
                    }
                    Some(SwarmCommand::DiscoverPeers { reply }) => {
                        // Re-announce + query. Results flow to the
                        // GetProviders arm, which dials each discovered
                        // peer (direct + relay circuit). Also nudge a
                        // bootstrap so the routing table widens.
                        refresh_provider_records(
                            &mut swarm,
                            &namespace_key,
                            "on-demand fetch discovery",
                        );
                        let _ = swarm.behaviour_mut().kademlia.bootstrap();
                        let _ = reply.send(()).await;
                    }
                    Some(SwarmCommand::SendFetchRequest { peer, request, reply }) => {
                        let request_id = swarm
                            .behaviour_mut()
                            .vc_fetch
                            .send_request(&peer, request);
                        outbound_fetch_replies.insert(request_id, reply);
                    }
                    Some(SwarmCommand::SendFetchResponse { channel, response }) => {
                        // Best-effort — if the peer disconnected mid-flight
                        // the channel returns `Err(response)` and we drop it.
                        let _ = swarm
                            .behaviour_mut()
                            .vc_fetch
                            .send_response(channel, response);
                    }
                    Some(SwarmCommand::SendSyncRequest { peer, request, reply }) => {
                        let request_id = swarm
                            .behaviour_mut()
                            .device_sync
                            .send_request(&peer, request);
                        outbound_sync_replies.insert(request_id, reply);
                    }
                    Some(SwarmCommand::SendSyncResponse { channel, response }) => {
                        let _ = swarm
                            .behaviour_mut()
                            .device_sync
                            .send_response(channel, response);
                    }
                    Some(SwarmCommand::SendGuardianRequest { peer, request, reply }) => {
                        let request_id = swarm
                            .behaviour_mut()
                            .guardian
                            .send_request(&peer, request);
                        outbound_guardian_replies.insert(request_id, reply);
                    }
                    Some(SwarmCommand::SendGraphFetchRequest { peer, request, reply }) => {
                        let request_id = swarm
                            .behaviour_mut()
                            .graph_fetch
                            .send_request(&peer, request);
                        outbound_graph_fetch_replies.insert(request_id, reply);
                    }
                    Some(SwarmCommand::SendGraphFetchResponse { channel, response }) => {
                        let _ = swarm
                            .behaviour_mut()
                            .graph_fetch
                            .send_response(channel, response);
                    }
                    Some(SwarmCommand::SendProfileFetchRequest { peer, request, reply }) => {
                        let request_id = swarm
                            .behaviour_mut()
                            .profile_fetch
                            .send_request(&peer, request);
                        outbound_profile_fetch_replies.insert(request_id, reply);
                    }
                    Some(SwarmCommand::SendProfileFetchResponse { channel, response }) => {
                        let _ = swarm
                            .behaviour_mut()
                            .profile_fetch
                            .send_response(channel, response);
                    }
                    Some(SwarmCommand::SendReceiptRequest { peer, request, reply }) => {
                        let request_id = swarm
                            .behaviour_mut()
                            .username_reg
                            .send_request(&peer, request);
                        outbound_receipt_replies.insert(request_id, reply);
                    }
                    Some(SwarmCommand::PutDhtRecord { key, value, reply }) => {
                        let record = kad::Record::new(key, value);
                        match swarm
                            .behaviour_mut()
                            .kademlia
                            .put_record(record, kad::Quorum::One)
                        {
                            Ok(qid) => {
                                pending_put_queries.insert(qid, reply);
                            }
                            Err(e) => {
                                let _ = reply.send(Err(NetworkError::Publish(e.to_string())));
                            }
                        }
                    }
                    Some(SwarmCommand::GetDhtRecords { key, reply }) => {
                        let qid = swarm
                            .behaviour_mut()
                            .kademlia
                            .get_record(kad::RecordKey::new(&key));
                        pending_get_queries.insert(qid, (reply, Vec::new()));
                    }
                    Some(SwarmCommand::ConnectPeer { peer, addrs, reply }) => {
                        let mut dialed = false;
                        for addr in addrs {
                            let dial_addr =
                                addr.with(libp2p::multiaddr::Protocol::P2p(peer));
                            match swarm.dial(dial_addr.clone()) {
                                Ok(()) => dialed = true,
                                Err(e) => log::debug!("connect_peer: dial {dial_addr} failed: {e}"),
                            }
                        }
                        let result = if dialed {
                            Ok(())
                        } else {
                            Err(NetworkError::Publish("no dialable address for peer".into()))
                        };
                        let _ = reply.send(result).await;
                    }
                    Some(SwarmCommand::Shutdown) | None => {
                        log::info!("P2P node shutting down");
                        break;
                    }
                }
            }
            // Process swarm events
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::Gossipsub(
                        gossipsub::Event::Message {
                            message,
                            propagation_source,
                            message_id,
                        }
                    )) => {
                        let topic = message.topic.to_string();
                        log::debug!(
                            "Gossip message on {topic} from {:?} ({} bytes)",
                            message.source,
                            message.data.len()
                        );

                        // Rate-limit incoming gossip per source peer. A
                        // rate-limited peer is not a protocol violation —
                        // report Ignore so gossipsub drops the message from
                        // mcache without scoring the sender down.
                        if let Some(source) = message.source {
                            if !rate_limiter.check(&source) {
                                log::debug!(
                                    "Rate-limited gossip from {source} on {topic} — dropping"
                                );
                                let _ = swarm
                                    .behaviour_mut()
                                    .gossipsub
                                    .report_message_validation_result(
                                        &message_id,
                                        &propagation_source,
                                        gossipsub::MessageAcceptance::Ignore,
                                    );
                                continue;
                            }
                        }

                        // Peer exchange messages are NOT signed envelopes —
                        // handle them separately before the validation
                        // pipeline. We Accept them so gossipsub propagates
                        // and the peer-exchange handler does the rest.
                        if topic == TOPIC_PEER_EXCHANGE {
                            handle_peer_exchange(&mut swarm, &message.data);
                            let _ = swarm
                                .behaviour_mut()
                                .gossipsub
                                .report_message_validation_result(
                                    &message_id,
                                    &propagation_source,
                                    gossipsub::MessageAcceptance::Accept,
                                );
                            continue;
                        }

                        // Step 1: Deserialize the signed envelope. A
                        // malformed envelope is a protocol violation —
                        // Reject so gossipsub scores the source down via
                        // the topic's invalid_message_deliveries weight.
                        let envelope = match serde_json::from_slice::<SignedGossipMessage>(
                            &message.data,
                        ) {
                            Ok(env) => env,
                            Err(e) => {
                                log::debug!(
                                    "Dropping message on {topic}: invalid envelope: {e}"
                                );
                                let _ = swarm
                                    .behaviour_mut()
                                    .gossipsub
                                    .report_message_validation_result(
                                        &message_id,
                                        &propagation_source,
                                        gossipsub::MessageAcceptance::Reject,
                                    );
                                continue;
                            }
                        };

                        // Step 2: Run the full validation pipeline
                        // (signature, freshness, dedup, schema, authority).
                        // Failure here is also a protocol violation; Reject
                        // feeds the per-topic P4 (invalid_message_deliveries)
                        // weight in `p2p::scoring`.
                        if let Err(e) = validator.validate(&envelope) {
                            log::debug!(
                                "Dropping message on {topic} from {}: {e}",
                                envelope.stake_address
                            );
                            let _ = swarm
                                .behaviour_mut()
                                .gossipsub
                                .report_message_validation_result(
                                    &message_id,
                                    &propagation_source,
                                    gossipsub::MessageAcceptance::Reject,
                                );
                            continue;
                        }

                        // Step 3: Accept so gossipsub propagates the
                        // message and rewards first-delivery scoring,
                        // then forward to the application layer.
                        let _ = swarm
                            .behaviour_mut()
                            .gossipsub
                            .report_message_validation_result(
                                &message_id,
                                &propagation_source,
                                gossipsub::MessageAcceptance::Accept,
                            );
                        let _ = event_tx.send(P2pEvent::GossipMessage {
                            topic,
                            message: envelope,
                        }).await;
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::Gossipsub(
                        gossipsub::Event::Subscribed { peer_id, topic }
                    )) => {
                        log::debug!("Peer {peer_id} subscribed to {topic}");
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::Identify(
                        identify::Event::Received { peer_id, info, .. }
                    )) => {
                        log::debug!(
                            "Identified peer {peer_id}: {} ({})",
                            info.protocol_version,
                            info.agent_version
                        );
                        // Add identified addresses to Kademlia routing table.
                        for addr in &info.listen_addrs {
                            swarm.behaviour_mut().kademlia.add_address(&peer_id, addr.clone());
                        }

                        // If this is a relay peer we haven't reserved on yet:
                        // 1. Request a relay reservation so other NATted peers can reach us
                        // 2. Bootstrap Kademlia now that we have a relay in our routing table
                        // 3. Start providing on the namespace key for peer discovery
                        let is_known_relay = relay_peer_ids.contains(&peer_id);
                        let is_discovered_relay = discovered_relays.contains(&peer_id);
                        if (is_known_relay || is_discovered_relay)
                            && !relay_reservations_requested.contains(&peer_id)
                        {
                            relay_reservations_requested.insert(peer_id);
                            diag::log(&format!(
                                "Identified relay peer {peer_id} — requesting reservation"
                            ));

                            // Built-in relays have explicit circuit addresses;
                            // discovered relays use a relative circuit addr
                            // (resolved via the connection we just dialed).
                            let circuit_addr = super::discovery::relay_circuit_addr_for(&peer_id)
                                .or_else(|| {
                                    format!("/p2p/{peer_id}/p2p-circuit")
                                        .parse::<libp2p::Multiaddr>()
                                        .ok()
                                });
                            if let Some(circuit_addr) = circuit_addr {
                                match swarm.listen_on(circuit_addr.clone()) {
                                    Ok(_) => diag::log(&format!(
                                        "Relay: listening on circuit {circuit_addr}"
                                    )),
                                    Err(e) => diag::log(&format!(
                                        "Relay: failed to listen on circuit: {e}"
                                    )),
                                }
                            }

                            // Re-bootstrap Kademlia now that relay is in the routing table.
                            match swarm.behaviour_mut().kademlia.bootstrap() {
                                Ok(_) => diag::log("Kademlia: bootstrap after relay identify"),
                                Err(e) => diag::log(&format!(
                                    "Kademlia: bootstrap after relay identify failed: {e}"
                                )),
                            }

                            // Refresh our provider record now that the relay path is
                            // actively coming online, so other peers do not wait for
                            // the next scheduled query cycle to find us.
                            refresh_provider_records(&mut swarm, &namespace_key, "relay identify");
                        }
                    }
                    // Kademlia events — DHT routing + provider discovery
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::Kademlia(event)) => {
                        match event {
                            kad::Event::InboundRequest {
                                request:
                                    kad::InboundRequest::PutRecord {
                                        record: Some(record),
                                        ..
                                    },
                            } => {
                                if dht_server {
                                    if let Some(db_arc) = db.as_ref() {
                                        if let Ok(guard) = db_arc.lock() {
                                            if let Some(database) = guard.as_ref() {
                                                let _ = database.conn().execute(
                                                    "INSERT INTO dht_records (key, value, updated_at)
                                                     VALUES (?1, ?2, datetime('now'))
                                                     ON CONFLICT(key) DO UPDATE SET
                                                         value = excluded.value,
                                                         updated_at = excluded.updated_at",
                                                    rusqlite::params![
                                                        record.key.as_ref(),
                                                        record.value
                                                    ],
                                                );
                                            }
                                        }
                                    }
                                    use libp2p::kad::store::RecordStore;
                                    let _ = swarm
                                        .behaviour_mut()
                                        .kademlia
                                        .store_mut()
                                        .put(record.clone());
                                }
                            }
                            kad::Event::RoutingUpdated { peer, addresses, .. } => {
                                diag::log(&format!(
                                    "Kademlia: routing updated for {peer} ({} addrs)",
                                    addresses.len()
                                ));
                                if !relay_peer_ids.contains(&peer) {
                                    dial_peer_with_relay_fallbacks(
                                        &mut swarm,
                                        &peer,
                                        "Kademlia routing update",
                                    );
                                }
                            }
                            kad::Event::OutboundQueryProgressed {
                                id,
                                result: kad::QueryResult::GetProviders(Ok(
                                    kad::GetProvidersOk::FoundProviders { providers, .. }
                                )),
                                ..
                            } => {
                                let local = *swarm.local_peer_id();
                                let is_relay_query = Some(id) == relay_providers_query;
                                diag::log(&format!(
                                    "DHT: GetProviders returned {} provider(s){}",
                                    providers.len(),
                                    if is_relay_query { " [relay]" } else { "" }
                                ));
                                for peer in providers {
                                    if peer == local {
                                        continue;
                                    }
                                    // Relay-namespace providers: adopt a
                                    // capped subset as circuit relays. The
                                    // built-in relays already self-reserve,
                                    // so skip those. Dialing a newly-admitted
                                    // relay triggers the identify flow, which
                                    // requests the reservation.
                                    if is_relay_query
                                        && !relay_peer_ids.contains(&peer)
                                        && discovered_relays.admit(peer)
                                    {
                                        diag::log(&format!(
                                            "DHT: adopting discovered relay {peer}"
                                        ));
                                    }
                                    dial_peer_with_relay_fallbacks(
                                        &mut swarm,
                                        &peer,
                                        if is_relay_query {
                                            "relay discovery"
                                        } else {
                                            "DHT discovery"
                                        },
                                    );
                                }
                            }
                            kad::Event::OutboundQueryProgressed {
                                result: kad::QueryResult::StartProviding(Ok(
                                    kad::AddProviderOk { key }
                                )),
                                ..
                            } => {
                                diag::log(&format!(
                                    "Kademlia: now providing {:?}",
                                    key
                                ));
                            }
                            kad::Event::OutboundQueryProgressed {
                                result: kad::QueryResult::Bootstrap(Ok(
                                    kad::BootstrapOk { num_remaining, .. }
                                )),
                                ..
                            } => {
                                diag::log(&format!(
                                    "Kademlia: bootstrap progress, {num_remaining} remaining"
                                ));
                            }
                            kad::Event::OutboundQueryProgressed {
                                result: kad::QueryResult::GetProviders(Ok(
                                    kad::GetProvidersOk::FinishedWithNoAdditionalRecord { .. }
                                )),
                                ..
                            } => {
                                diag::log("DHT: GetProviders query finished (no more records)");
                            }
                            kad::Event::OutboundQueryProgressed {
                                id,
                                result: kad::QueryResult::PutRecord(result),
                                ..
                            } => {
                                if let Some(reply) = pending_put_queries.remove(&id) {
                                    let _ = reply.send(match result {
                                        Ok(_) => Ok(()),
                                        Err(e) => Err(NetworkError::Publish(e.to_string())),
                                    });
                                }
                            }
                            kad::Event::OutboundQueryProgressed {
                                id,
                                result: kad::QueryResult::GetRecord(result),
                                step,
                                ..
                            } => {
                                match result {
                                    Ok(kad::GetRecordOk::FoundRecord(peer_record)) => {
                                        if let Some((_, acc)) = pending_get_queries.get_mut(&id) {
                                            acc.push(peer_record.record.value.clone());
                                        }
                                        if step.last {
                                            if let Some((reply, acc)) =
                                                pending_get_queries.remove(&id)
                                            {
                                                let _ = reply.send(Ok(acc));
                                            }
                                        }
                                    }
                                    Ok(kad::GetRecordOk::FinishedWithNoAdditionalRecord {
                                        ..
                                    }) => {
                                        if let Some((reply, acc)) = pending_get_queries.remove(&id)
                                        {
                                            let _ = reply.send(Ok(acc));
                                        }
                                    }
                                    Err(e) => {
                                        if let Some((reply, acc)) = pending_get_queries.remove(&id)
                                        {
                                            // NotFound with nothing accumulated is an
                                            // empty result, not a failure.
                                            if acc.is_empty()
                                                && !matches!(
                                                    e,
                                                    kad::GetRecordError::NotFound { .. }
                                                )
                                            {
                                                let _ = reply.send(Err(NetworkError::Publish(
                                                    e.to_string(),
                                                )));
                                            } else {
                                                let _ = reply.send(Ok(acc));
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {
                                log::debug!("Kademlia: {event:?}");
                            }
                        }
                    }
                    // AutoNAT events — track NAT reachability
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::Autonat(event)) => {
                        match &event {
                            autonat::Event::InboundProbe(probe) => {
                                log::debug!("AutoNAT: inbound probe: {probe:?}");
                            }
                            autonat::Event::OutboundProbe(probe) => {
                                log::debug!("AutoNAT: outbound probe: {probe:?}");
                            }
                            autonat::Event::StatusChanged { old, new } => {
                                log::info!("AutoNAT: status changed from {old:?} to {new:?}");
                                let new_state = match new {
                                    autonat::NatStatus::Public(ref addr) => {
                                        // We're publicly reachable! Add external address.
                                        // This triggers:
                                        // - Kademlia switches from Client → Server mode
                                        // - Relay server becomes active (peers can relay through us)
                                        diag::log(&format!(
                                            "AutoNAT: PUBLIC — adding external addr {addr}, enabling relay server"
                                        ));
                                        swarm.add_external_address(addr.clone());
                                        NatState::Public(addr.to_string())
                                    }
                                    autonat::NatStatus::Private => {
                                        diag::log("AutoNAT: PRIVATE (behind NAT)");
                                        NatState::Private
                                    }
                                    autonat::NatStatus::Unknown => NatState::Unknown,
                                };
                                current_nat_state = new_state.clone();
                                let _ = event_tx.send(P2pEvent::NatStatusChanged(new_state)).await;
                            }
                        }
                    }
                    // Relay server events — this node relays traffic for NATted peers
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::RelayServer(event)) => {
                        diag::log(&format!("Relay server: {event:?}"));
                    }
                    // Relay client events — circuit relay v2
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::RelayClient(event)) => {
                        match event {
                            relay::client::Event::ReservationReqAccepted {
                                relay_peer_id, ..
                            } => {
                                let relay_str = relay_peer_id.to_string();
                                log::info!(
                                    "Relay: reservation accepted by {relay_str}"
                                );
                                if !relay_addrs.iter().any(|a| a.contains(&relay_str)) {
                                    relay_addrs.push(format!(
                                        "/p2p/{relay_str}/p2p-circuit"
                                    ));
                                }
                                let _ = event_tx.send(P2pEvent::RelayReservation {
                                    relay_peer: relay_str.clone(),
                                }).await;
                                refresh_provider_records(
                                    &mut swarm,
                                    &namespace_key,
                                    "relay reservation accepted",
                                );
                                publish_peer_exchange(&mut swarm);
                            }
                            _ => {
                                log::debug!("Relay client event: {event:?}");
                            }
                        }
                    }
                    // DCUtR events — direct connection upgrade through relay
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::Dcutr(event)) => {
                        let peer_str = event.remote_peer_id.to_string();
                        match event.result {
                            Ok(_conn_id) => {
                                log::info!(
                                    "DCUtR: direct connection upgrade succeeded with {peer_str}"
                                );
                                let _ = event_tx.send(P2pEvent::DirectConnectionUpgraded {
                                    peer_id: peer_str,
                                }).await;
                            }
                            Err(error) => {
                                log::debug!(
                                    "DCUtR: upgrade failed with {peer_str}: {error}"
                                );
                            }
                        }
                    }
                    // ---- vc-fetch (request-response) -----------------
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::VcFetch(
                        request_response::Event::Message { peer, message, .. }
                    )) => {
                        match message {
                            request_response::Message::Request { request, channel, .. } => {
                                log::debug!(
                                    "vc-fetch: inbound request from {peer} for {}",
                                    request.credential_id
                                );
                                // Synchronously answer using the local DB.
                                // Without a DB handle we MUST respond
                                // (the libp2p contract requires it) so
                                // we fall back to NotFound.
                                let response = match db.as_ref() {
                                    Some(db_arc) => match db_arc.lock() {
                                        Ok(guard) => match guard.as_ref() {
                                            Some(database) => super::vc_fetch::handle_fetch_request(
                                                database.conn(),
                                                &request,
                                            )
                                            .unwrap_or(super::vc_fetch::FetchResponse::NotFound),
                                            None => super::vc_fetch::FetchResponse::NotFound,
                                        },
                                        Err(_) => super::vc_fetch::FetchResponse::NotFound,
                                    },
                                    None => super::vc_fetch::FetchResponse::NotFound,
                                };
                                let _ = swarm
                                    .behaviour_mut()
                                    .vc_fetch
                                    .send_response(channel, response);
                            }
                            request_response::Message::Response { request_id, response } => {
                                if let Some(reply) = outbound_fetch_replies.remove(&request_id) {
                                    let _ = reply.send(Ok(response));
                                } else {
                                    log::debug!(
                                        "vc-fetch: unmatched response for {request_id} from {peer}"
                                    );
                                }
                            }
                        }
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::VcFetch(
                        request_response::Event::OutboundFailure { request_id, error, peer, .. }
                    )) => {
                        log::warn!("vc-fetch: outbound to {peer} failed: {error}");
                        if let Some(reply) = outbound_fetch_replies.remove(&request_id) {
                            let _ = reply.send(Err(NetworkError::Publish(error.to_string())));
                        }
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::VcFetch(
                        request_response::Event::InboundFailure { error, peer, .. }
                    )) => {
                        log::debug!("vc-fetch: inbound from {peer} failed: {error}");
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::VcFetch(
                        request_response::Event::ResponseSent { .. }
                    )) => {
                        // Nothing to do — the outbound peer already got it.
                    }
                    // ---- device-sync (request-response) --------------
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::DeviceSync(
                        request_response::Event::Message { peer, message, .. }
                    )) => {
                        match message {
                            request_response::Message::Request { request, channel, .. } => {
                                log::debug!("device-sync: inbound request from {peer}");
                                // Answer against the local DB. Without a DB
                                // handle we MUST still respond (libp2p
                                // contract) — fall back to an error response.
                                let response = match db.as_ref() {
                                    Some(db_arc) => match db_arc.lock() {
                                        Ok(guard) => match guard.as_ref() {
                                            Some(database) => super::device_sync::handle_sync_request(
                                                database.conn(),
                                                &peer.to_string(),
                                                &request,
                                            ),
                                            None => super::device_sync::SyncResponse::Error(
                                                "no active profile".into(),
                                            ),
                                        },
                                        Err(_) => super::device_sync::SyncResponse::Error(
                                            "db lock poisoned".into(),
                                        ),
                                    },
                                    None => super::device_sync::SyncResponse::Error(
                                        "no database".into(),
                                    ),
                                };
                                let _ = swarm
                                    .behaviour_mut()
                                    .device_sync
                                    .send_response(channel, response);
                            }
                            request_response::Message::Response { request_id, response } => {
                                if let Some(reply) = outbound_sync_replies.remove(&request_id) {
                                    let _ = reply.send(Ok(response));
                                } else {
                                    log::debug!(
                                        "device-sync: unmatched response for {request_id} from {peer}"
                                    );
                                }
                            }
                        }
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::DeviceSync(
                        request_response::Event::OutboundFailure { request_id, error, peer, .. }
                    )) => {
                        log::warn!("device-sync: outbound to {peer} failed: {error}");
                        if let Some(reply) = outbound_sync_replies.remove(&request_id) {
                            let _ = reply.send(Err(NetworkError::Publish(error.to_string())));
                        }
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::DeviceSync(
                        request_response::Event::InboundFailure { error, peer, .. }
                    )) => {
                        log::debug!("device-sync: inbound from {peer} failed: {error}");
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::DeviceSync(
                        request_response::Event::ResponseSent { .. }
                    )) => {}
                    // ---- guardian (request-response) -----------------
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::Guardian(
                        request_response::Event::Message { peer, message, .. }
                    )) => {
                        match message {
                            request_response::Message::Request { request, channel, .. } => {
                                log::debug!("guardian: inbound request from {peer}");
                                let response = match db.as_ref() {
                                    Some(db_arc) => match db_arc.lock() {
                                        Ok(guard) => match guard.as_ref() {
                                            Some(database) => super::guardian::handle_guardian_request(
                                                database.conn(),
                                                &peer.to_string(),
                                                &request,
                                            ),
                                            None => super::guardian::GuardianResponse::Error(
                                                "no active profile".into(),
                                            ),
                                        },
                                        Err(_) => super::guardian::GuardianResponse::Error(
                                            "db lock poisoned".into(),
                                        ),
                                    },
                                    None => super::guardian::GuardianResponse::Error(
                                        "no database".into(),
                                    ),
                                };
                                let _ = swarm
                                    .behaviour_mut()
                                    .guardian
                                    .send_response(channel, response);
                            }
                            request_response::Message::Response { request_id, response } => {
                                if let Some(reply) = outbound_guardian_replies.remove(&request_id) {
                                    let _ = reply.send(Ok(response));
                                } else {
                                    log::debug!(
                                        "guardian: unmatched response for {request_id} from {peer}"
                                    );
                                }
                            }
                        }
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::Guardian(
                        request_response::Event::OutboundFailure { request_id, error, peer, .. }
                    )) => {
                        log::warn!("guardian: outbound to {peer} failed: {error}");
                        if let Some(reply) = outbound_guardian_replies.remove(&request_id) {
                            let _ = reply.send(Err(NetworkError::Publish(error.to_string())));
                        }
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::Guardian(
                        request_response::Event::InboundFailure { error, peer, .. }
                    )) => {
                        log::debug!("guardian: inbound from {peer} failed: {error}");
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::Guardian(
                        request_response::Event::ResponseSent { .. }
                    )) => {}
                    // ---- graph-fetch (request-response) --------------
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::GraphFetch(
                        request_response::Event::Message { peer, message, .. }
                    )) => {
                        match message {
                            request_response::Message::Request { request, channel, .. } => {
                                log::info!(
                                    "graph-fetch: inbound request from {peer} for {}",
                                    request.subject_did
                                );
                                // Answer against the local DB. Without a DB
                                // handle we MUST still respond (libp2p
                                // contract) — fall back to NotOwner.
                                let response = match db.as_ref() {
                                    Some(db_arc) => match db_arc.lock() {
                                        Ok(guard) => match guard.as_ref() {
                                            Some(database) => {
                                                super::graph_fetch::handle_graph_fetch_request(
                                                    database.conn(),
                                                    &request,
                                                )
                                                .unwrap_or(
                                                    super::graph_fetch::GraphFetchResponse::NotOwner,
                                                )
                                            }
                                            None => super::graph_fetch::GraphFetchResponse::NotOwner,
                                        },
                                        Err(_) => super::graph_fetch::GraphFetchResponse::NotOwner,
                                    },
                                    None => super::graph_fetch::GraphFetchResponse::NotOwner,
                                };
                                let _ = swarm
                                    .behaviour_mut()
                                    .graph_fetch
                                    .send_response(channel, response);
                            }
                            request_response::Message::Response { request_id, response } => {
                                if let Some(reply) =
                                    outbound_graph_fetch_replies.remove(&request_id)
                                {
                                    let _ = reply.send(Ok(response));
                                } else {
                                    log::debug!(
                                        "graph-fetch: unmatched response for {request_id} from {peer}"
                                    );
                                }
                            }
                        }
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::GraphFetch(
                        request_response::Event::OutboundFailure { request_id, error, peer, .. }
                    )) => {
                        log::warn!("graph-fetch: outbound to {peer} failed: {error}");
                        if let Some(reply) = outbound_graph_fetch_replies.remove(&request_id) {
                            let _ = reply.send(Err(NetworkError::Publish(error.to_string())));
                        }
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::GraphFetch(
                        request_response::Event::InboundFailure { error, peer, .. }
                    )) => {
                        log::debug!("graph-fetch: inbound from {peer} failed: {error}");
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::GraphFetch(
                        request_response::Event::ResponseSent { .. }
                    )) => {}
                    // ---- profile-fetch (request-response) ------------
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::ProfileFetch(
                        request_response::Event::Message { peer, message, .. }
                    )) => {
                        match message {
                            request_response::Message::Request { request, channel, .. } => {
                                log::info!("profile-fetch: inbound request from {peer}");
                                let response = match db.as_ref() {
                                    Some(db_arc) => match db_arc.lock() {
                                        Ok(guard) => match guard.as_ref() {
                                            Some(database) => {
                                                super::profile_fetch::handle_profile_fetch_request(
                                                    database.conn(),
                                                    &request,
                                                )
                                                .unwrap_or(
                                                    super::profile_fetch::ProfileFetchResponse::NotOwner,
                                                )
                                            }
                                            None => super::profile_fetch::ProfileFetchResponse::NotOwner,
                                        },
                                        Err(_) => super::profile_fetch::ProfileFetchResponse::NotOwner,
                                    },
                                    None => super::profile_fetch::ProfileFetchResponse::NotOwner,
                                };
                                let _ = swarm
                                    .behaviour_mut()
                                    .profile_fetch
                                    .send_response(channel, response);
                            }
                            request_response::Message::Response { request_id, response } => {
                                if let Some(reply) =
                                    outbound_profile_fetch_replies.remove(&request_id)
                                {
                                    let _ = reply.send(Ok(response));
                                }
                            }
                        }
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::ProfileFetch(
                        request_response::Event::OutboundFailure { request_id, error, peer, .. }
                    )) => {
                        log::warn!("profile-fetch: outbound to {peer} failed: {error}");
                        if let Some(reply) = outbound_profile_fetch_replies.remove(&request_id) {
                            let _ = reply.send(Err(NetworkError::Publish(error.to_string())));
                        }
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::ProfileFetch(
                        request_response::Event::InboundFailure { error, peer, .. }
                    )) => {
                        log::debug!("profile-fetch: inbound from {peer} failed: {error}");
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::ProfileFetch(
                        request_response::Event::ResponseSent { .. }
                    )) => {}
                    // ---- username-reg (outbound receipts) ------------
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::UsernameReg(
                        request_response::Event::Message {
                            message: request_response::Message::Response { request_id, response },
                            ..
                        }
                    )) => {
                        if let Some(reply) = outbound_receipt_replies.remove(&request_id) {
                            let _ = reply.send(Ok(response));
                        }
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::UsernameReg(
                        request_response::Event::OutboundFailure { request_id, error, peer, .. }
                    )) => {
                        log::debug!("username-reg: outbound to {peer} failed: {error}");
                        if let Some(reply) = outbound_receipt_replies.remove(&request_id) {
                            let _ = reply.send(Err(NetworkError::Publish(error.to_string())));
                        }
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::UsernameReg(_)) => {}
                    SwarmEvent::NewListenAddr { address, .. } => {
                        log::info!("Listening on {address}");
                        if is_circuit_listener(&address) {
                            diag::log("Peer exchange: rebroadcasting after relay circuit listen address appeared");
                            publish_peer_exchange(&mut swarm);
                        }
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                        log::info!("Connected to peer: {peer_id}");
                        diag::log(&format!("P2P event: peer connected — {peer_id}"));
                        let _ = event_tx.send(P2pEvent::PeerConnected {
                            peer_id: peer_id.to_string(),
                        }).await;
                        // Broadcast our addresses via peer exchange so this
                        // new peer (and its peers) can discover us.
                        publish_peer_exchange(&mut swarm);
                        // Persist the peer's address so we reconnect on next startup.
                        let addr = endpoint.get_remote_address().to_string();
                        save_peer_to_db(&db, &peer_id.to_string(), &[addr]);
                        // Reward a discovered relay that connected.
                        if discovered_relays.contains(&peer_id) {
                            discovered_relays.note_success(&peer_id);
                        }
                    }
                    SwarmEvent::ConnectionClosed { peer_id, .. } => {
                        log::debug!("Disconnected from peer: {peer_id}");
                        diag::log(&format!("P2P event: peer disconnected — {peer_id}"));
                        rate_limiter.remove_peer(&peer_id);
                        // Decay a discovered relay's reputation; evict if it
                        // has proven unreliable so its slot frees up.
                        if discovered_relays.contains(&peer_id)
                            && discovered_relays.note_failure(&peer_id)
                        {
                            relay_reservations_requested.remove(&peer_id);
                            diag::log(&format!(
                                "Relay: evicted unreliable discovered relay {peer_id}"
                            ));
                        }
                        let _ = event_tx.send(P2pEvent::PeerDisconnected {
                            peer_id: peer_id.to_string(),
                        }).await;
                    }
                    _ => {}
                }
            }
        }
    }
}

impl P2pNode {
    /// Get the local PeerId.
    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    /// Publish a message on a gossip topic.
    pub async fn publish(&self, topic: &str, data: Vec<u8>) -> Result<(), NetworkError> {
        if !self.running {
            return Err(NetworkError::NotRunning);
        }
        let (reply_tx, mut reply_rx) = mpsc::channel(1);
        self.command_tx
            .send(SwarmCommand::Publish {
                topic: topic.to_string(),
                data,
                reply: reply_tx,
            })
            .await
            .map_err(|_| NetworkError::NotRunning)?;
        reply_rx
            .recv()
            .await
            .unwrap_or(Err(NetworkError::NotRunning))
    }

    /// Get the current network status.
    pub async fn status(&self) -> Result<NetworkStatus, NetworkError> {
        if !self.running {
            return Ok(NetworkStatus {
                is_running: false,
                peer_id: Some(self.peer_id.to_string()),
                connected_peers: 0,
                listening_addresses: vec![],
                subscribed_topics: vec![],
                nat_status: NatState::Unknown,
                relay_addresses: vec![],
            });
        }
        let (reply_tx, mut reply_rx) = mpsc::channel(1);
        self.command_tx
            .send(SwarmCommand::Status { reply: reply_tx })
            .await
            .map_err(|_| NetworkError::NotRunning)?;
        reply_rx.recv().await.ok_or(NetworkError::NotRunning)
    }

    /// Get the list of connected peers.
    pub async fn connected_peers(&self) -> Result<Vec<String>, NetworkError> {
        if !self.running {
            return Ok(vec![]);
        }
        let (reply_tx, mut reply_rx) = mpsc::channel(1);
        self.command_tx
            .send(SwarmCommand::Peers { reply: reply_tx })
            .await
            .map_err(|_| NetworkError::NotRunning)?;
        let peers = reply_rx.recv().await.ok_or(NetworkError::NotRunning)?;
        Ok(peers.iter().map(|p| p.to_string()).collect())
    }

    /// Dynamically subscribe to a gossip topic (e.g., a per-classroom topic).
    pub async fn subscribe_topic(&self, topic: &str) -> Result<(), NetworkError> {
        if !self.running {
            return Err(NetworkError::NotRunning);
        }
        let (reply_tx, mut reply_rx) = mpsc::channel(1);
        self.command_tx
            .send(SwarmCommand::Subscribe {
                topic: topic.to_string(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| NetworkError::NotRunning)?;
        reply_rx
            .recv()
            .await
            .unwrap_or(Err(NetworkError::NotRunning))
    }

    /// Dynamically unsubscribe from a gossip topic.
    pub async fn unsubscribe_topic(&self, topic: &str) -> Result<(), NetworkError> {
        if !self.running {
            return Err(NetworkError::NotRunning);
        }
        let (reply_tx, mut reply_rx) = mpsc::channel(1);
        self.command_tx
            .send(SwarmCommand::Unsubscribe {
                topic: topic.to_string(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| NetworkError::NotRunning)?;
        reply_rx
            .recv()
            .await
            .unwrap_or(Err(NetworkError::NotRunning))
    }

    /// Send a vc-fetch request to a specific peer and await the
    /// reply (or an outbound failure). Resolves the protocol-level
    /// `FetchResponse` directly — the caller can match on
    /// `Ok(vc) | Unauthorized | NotFound`.
    pub async fn fetch_credential(
        &self,
        peer: PeerId,
        request: FetchRequest,
    ) -> Result<FetchResponse, NetworkError> {
        if !self.running {
            return Err(NetworkError::NotRunning);
        }
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(SwarmCommand::SendFetchRequest {
                peer,
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| NetworkError::NotRunning)?;
        reply_rx.await.map_err(|_| NetworkError::NotRunning)?
    }

    /// Dial a peer at the given multiaddresses. Best-effort — returns
    /// once the dials have been issued, not once connected.
    pub async fn connect_peer(
        &self,
        peer: PeerId,
        addrs: Vec<libp2p::Multiaddr>,
    ) -> Result<(), NetworkError> {
        if !self.running {
            return Err(NetworkError::NotRunning);
        }
        let (reply_tx, mut reply_rx) = mpsc::channel(1);
        self.command_tx
            .send(SwarmCommand::ConnectPeer {
                peer,
                addrs,
                reply: reply_tx,
            })
            .await
            .map_err(|_| NetworkError::NotRunning)?;
        reply_rx
            .recv()
            .await
            .unwrap_or(Err(NetworkError::NotRunning))
    }

    /// Run a sync exchange with a paired peer over `/alexandria/sync/1.0`.
    /// Resolves with the peer's sealed [`SyncResponse`].
    pub async fn sync_with_peer(
        &self,
        peer: PeerId,
        request: SyncRequest,
    ) -> Result<SyncResponse, NetworkError> {
        if !self.running {
            return Err(NetworkError::NotRunning);
        }
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(SwarmCommand::SendSyncRequest {
                peer,
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| NetworkError::NotRunning)?;
        reply_rx.await.map_err(|_| NetworkError::NotRunning)?
    }

    /// Run one guardian exchange (link / push / pull / revoke) with the
    /// counterparty over `/alexandria/guardian/1.0`.
    pub async fn guardian_request(
        &self,
        peer: PeerId,
        request: super::guardian::GuardianRequest,
    ) -> Result<super::guardian::GuardianResponse, NetworkError> {
        if !self.running {
            return Err(NetworkError::NotRunning);
        }
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(SwarmCommand::SendGuardianRequest {
                peer,
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| NetworkError::NotRunning)?;
        reply_rx.await.map_err(|_| NetworkError::NotRunning)?
    }

    /// Every peer in the Kademlia routing table plus current
    /// connections. Sending a request-response message to a
    /// not-currently-connected peer auto-dials it via its known addrs,
    /// so these are all valid graph-fetch targets even when idle
    /// connections have been reaped.
    pub async fn known_peers(&self) -> Result<Vec<String>, NetworkError> {
        if !self.running {
            return Err(NetworkError::NotRunning);
        }
        let (reply_tx, mut reply_rx) = mpsc::channel(1);
        self.command_tx
            .send(SwarmCommand::KnownPeers { reply: reply_tx })
            .await
            .map_err(|_| NetworkError::NotRunning)?;
        let peers = reply_rx.recv().await.ok_or(NetworkError::NotRunning)?;
        Ok(peers.iter().map(|p| p.to_string()).collect())
    }

    /// Kick an on-demand provider-discovery sweep and let it settle so
    /// freshly-discovered peers (e.g. a profile/graph owner this node
    /// hadn't met yet) get dialed and added to the routing table before
    /// a broadcast. Behind a user-initiated fetch spinner, so the
    /// settle delay is acceptable.
    pub async fn discover_peers(&self, settle: Duration) -> Result<(), NetworkError> {
        if !self.running {
            return Err(NetworkError::NotRunning);
        }
        let (reply_tx, mut reply_rx) = mpsc::channel(1);
        self.command_tx
            .send(SwarmCommand::DiscoverPeers { reply: reply_tx })
            .await
            .map_err(|_| NetworkError::NotRunning)?;
        let _ = reply_rx.recv().await;
        tokio::time::sleep(settle).await;
        Ok(())
    }

    /// Send a graph-fetch request to a specific peer over
    /// `/alexandria/graph-fetch/1.0`. Resolves the protocol-level
    /// [`GraphFetchResponse`] directly — the caller matches on
    /// `Ok(graph) | NotOwner | Empty`.
    pub async fn fetch_graph(
        &self,
        peer: PeerId,
        request: GraphFetchRequest,
    ) -> Result<GraphFetchResponse, NetworkError> {
        if !self.running {
            return Err(NetworkError::NotRunning);
        }
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(SwarmCommand::SendGraphFetchRequest {
                peer,
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| NetworkError::NotRunning)?;
        reply_rx.await.map_err(|_| NetworkError::NotRunning)?
    }

    /// Send a profile-fetch request to a specific peer over
    /// `/alexandria/profile-fetch/1.0`.
    pub async fn fetch_profile(
        &self,
        peer: PeerId,
        request: ProfileFetchRequest,
    ) -> Result<ProfileFetchResponse, NetworkError> {
        if !self.running {
            return Err(NetworkError::NotRunning);
        }
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(SwarmCommand::SendProfileFetchRequest {
                peer,
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| NetworkError::NotRunning)?;
        reply_rx.await.map_err(|_| NetworkError::NotRunning)?
    }

    /// Store a record in the DHT. Resolves when the put query
    /// completes (quorum one).
    pub async fn put_dht_record(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), NetworkError> {
        if !self.running {
            return Err(NetworkError::NotRunning);
        }
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(SwarmCommand::PutDhtRecord {
                key,
                value,
                reply: reply_tx,
            })
            .await
            .map_err(|_| NetworkError::NotRunning)?;
        reply_rx.await.map_err(|_| NetworkError::NotRunning)?
    }

    /// Fetch all records stored under a DHT key (possibly from
    /// multiple conflicting writers).
    pub async fn get_dht_records(&self, key: Vec<u8>) -> Result<Vec<Vec<u8>>, NetworkError> {
        if !self.running {
            return Err(NetworkError::NotRunning);
        }
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(SwarmCommand::GetDhtRecords {
                key,
                reply: reply_tx,
            })
            .await
            .map_err(|_| NetworkError::NotRunning)?;
        reply_rx.await.map_err(|_| NetworkError::NotRunning)?
    }

    /// Request a username receipt from a relay peer.
    pub async fn request_username_receipt(
        &self,
        peer: PeerId,
        request: ReceiptRequest,
    ) -> Result<ReceiptResponse, NetworkError> {
        if !self.running {
            return Err(NetworkError::NotRunning);
        }
        let (reply_tx, reply_rx) = oneshot::channel();
        self.command_tx
            .send(SwarmCommand::SendReceiptRequest {
                peer,
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| NetworkError::NotRunning)?;
        reply_rx.await.map_err(|_| NetworkError::NotRunning)?
    }

    /// Shutdown the node.
    pub async fn shutdown(&mut self) {
        self.running = false;
        let _ = self.command_tx.send(SwarmCommand::Shutdown).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_DEVICE_ID: [u8; 32] = [0xCCu8; 32];

    #[test]
    fn derive_libp2p_keypair_deterministic() {
        let key_bytes = [0x42u8; 32];
        let kp1 = derive_libp2p_keypair(&key_bytes, &TEST_DEVICE_ID).unwrap();
        let kp2 = derive_libp2p_keypair(&key_bytes, &TEST_DEVICE_ID).unwrap();
        assert_eq!(
            kp1.public().to_peer_id(),
            kp2.public().to_peer_id(),
            "same key + device_id should produce same PeerId"
        );
    }

    #[test]
    fn different_keys_produce_different_peer_ids() {
        let kp1 = derive_libp2p_keypair(&[0x01u8; 32], &TEST_DEVICE_ID).unwrap();
        let kp2 = derive_libp2p_keypair(&[0x02u8; 32], &TEST_DEVICE_ID).unwrap();
        assert_ne!(
            kp1.public().to_peer_id(),
            kp2.public().to_peer_id(),
            "different keys should produce different PeerIds"
        );
    }

    #[test]
    fn different_device_ids_produce_different_peer_ids() {
        let key = [0x42u8; 32];
        let kp1 = derive_libp2p_keypair(&key, &[0x01u8; 32]).unwrap();
        let kp2 = derive_libp2p_keypair(&key, &[0x02u8; 32]).unwrap();
        assert_ne!(
            kp1.public().to_peer_id(),
            kp2.public().to_peer_id(),
            "same key with different device_id should produce different PeerIds"
        );
    }

    #[test]
    fn peer_id_is_valid_base58() {
        let kp = derive_libp2p_keypair(&[0xABu8; 32], &TEST_DEVICE_ID).unwrap();
        let peer_id = kp.public().to_peer_id();
        let peer_id_str = peer_id.to_string();
        // PeerId is base58-encoded (starts with "12D3KooW" for Ed25519)
        assert!(
            peer_id_str.starts_with("12D3KooW"),
            "Ed25519 PeerId should start with 12D3KooW, got: {}",
            peer_id_str
        );
    }

    #[tokio::test]
    async fn start_and_shutdown_node() {
        let keypair = derive_libp2p_keypair(&[0x42u8; 32], &TEST_DEVICE_ID).unwrap();
        let (event_tx, _event_rx) = mpsc::channel(16);

        let mut node = start_node_with_db(keypair, event_tx, vec![], None, false)
            .await
            .expect("node should start");

        // Check status
        let status = node.status().await.expect("should get status");
        assert!(status.is_running);
        assert!(status.peer_id.is_some());
        // 6 pre-VC app topics (catalog, taxonomy, governance, profiles,
        // opinions, peer-exchange) + 4 VC-migration topics (vc-did, vc-status,
        // vc-presentation, pinboard) + plugins + plugin-attestations +
        // sentinel-priors + 2 community-content topics (goal-templates,
        // question-banks) = 15.
        assert_eq!(status.subscribed_topics.len(), 15);
        assert_eq!(status.connected_peers, 0); // No peers yet
        assert_eq!(status.nat_status, NatState::Unknown); // NAT unknown initially
        assert!(status.relay_addresses.is_empty()); // No relays yet

        // Shutdown
        node.shutdown().await;
    }

    #[test]
    fn nat_state_default_is_unknown() {
        assert_eq!(NatState::default(), NatState::Unknown);
    }

    #[test]
    fn wallet_keypair_to_peer_id() {
        // Test the wallet → keypair → PeerId chain without starting a node.
        // (Full node integration test is in the iOS simulator diag test.)
        use crate::crypto::wallet;

        let mnemonic = bip39::Mnemonic::generate_in(bip39::Language::English, 24)
            .expect("generate mnemonic")
            .to_string();

        let w = wallet::wallet_from_mnemonic(&mnemonic).expect("derive wallet");
        assert!(
            w.stake_address.starts_with("stake_test1"),
            "expected preprod stake address"
        );

        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&w.payment_key_extended[..32]);
        let keypair = derive_libp2p_keypair(&key_bytes, &TEST_DEVICE_ID).expect("derive keypair");
        let peer_id = keypair.public().to_peer_id();
        assert!(
            peer_id.to_string().starts_with("12D3KooW"),
            "PeerId should be Ed25519, got: {}",
            peer_id
        );

        // Deterministic: same mnemonic + same device_id → same PeerId
        let w2 = wallet::wallet_from_mnemonic(&mnemonic).expect("wallet 2");
        let mut kb2 = [0u8; 32];
        kb2.copy_from_slice(&w2.payment_key_extended[..32]);
        let kp2 = derive_libp2p_keypair(&kb2, &TEST_DEVICE_ID).expect("kp2");
        assert_eq!(
            kp2.public().to_peer_id(),
            peer_id,
            "same mnemonic + same device_id should produce same PeerId"
        );
    }
}
