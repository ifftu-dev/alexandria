use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use libp2p::gossipsub::{self, IdentTopic, MessageAuthenticity, MessageId, ValidationMode};
use libp2p::identity::Keypair;
use libp2p::kad::store::MemoryStore;
use libp2p::swarm::NetworkBehaviour;
use libp2p::{autonat, dcutr, identify, kad, noise, relay, yamux, PeerId, Swarm, SwarmBuilder};
use thiserror::Error;
use tokio::sync::mpsc;

use crate::db::Database;

use crate::crypto::hash::blake2b_256;
use crate::diag;

use super::nat::build_autonat_config;
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

fn identify_agent_version() -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!("alexandria-node/{version} ({})", device_label())
}

fn device_label() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Some(model) = apple_sysctl("hw.model") {
            return humanize_apple_model(&model);
        }
        return "Mac".to_string();
    }

    #[cfg(target_os = "ios")]
    {
        if let Some(model) = apple_sysctl("hw.machine") {
            return humanize_apple_model(&model);
        }
        if let Ok(sim_model) = std::env::var("SIMULATOR_MODEL_IDENTIFIER") {
            if !sim_model.trim().is_empty() {
                return humanize_apple_model(sim_model.trim());
            }
        }
        return "iPhone or iPad".to_string();
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(label) = std::env::var("ALEXANDRIA_DEVICE_LABEL") {
            let label = label.trim();
            if !label.is_empty() {
                return label.to_string();
            }
        }
        if let Ok(product) = std::fs::read_to_string("/sys/devices/virtual/dmi/id/product_name") {
            let product = product.trim();
            if !product.is_empty() {
                return product.to_string();
            }
        }
        return "Linux device".to_string();
    }

    #[cfg(target_os = "android")]
    {
        if let Ok(label) = std::env::var("ALEXANDRIA_DEVICE_LABEL") {
            let label = label.trim();
            if !label.is_empty() {
                return label.to_string();
            }
        }
        return "Android device".to_string();
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(label) = std::env::var("ALEXANDRIA_DEVICE_LABEL") {
            let label = label.trim();
            if !label.is_empty() {
                return label.to_string();
            }
        }
        return "Windows device".to_string();
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
    /// Get the current network status.
    Status {
        reply: mpsc::Sender<NetworkStatus>,
    },
    /// Get the list of connected peers.
    Peers {
        reply: mpsc::Sender<Vec<PeerId>>,
    },
    /// Shutdown the node.
    Shutdown,
}

/// Derive a libp2p Ed25519 keypair from the Cardano payment key.
///
/// The architecture spec requires: "PeerId = Ed25519 public key derived
/// from Cardano signing key for linkability." This creates a cryptographic
/// link between P2P identity and on-chain identity.
///
/// We use the first 32 bytes of the extended payment key (the Ed25519 scalar)
/// as the libp2p identity key.
pub fn keypair_from_cardano_key(payment_key_bytes: &[u8; 32]) -> Result<Keypair, NetworkError> {
    let mut seed = *payment_key_bytes;
    let keypair = Keypair::ed25519_from_bytes(&mut seed)
        .map_err(|e| NetworkError::Identity(e.to_string()))?;
    Ok(keypair)
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

pub async fn start_node(
    keypair: Keypair,
    event_tx: mpsc::Sender<P2pEvent>,
    known_peers: Vec<KnownPeer>,
) -> Result<P2pNode, NetworkError> {
    let peer_id = keypair.public().to_peer_id();
    diag::log(&format!("start_node: PeerId: {peer_id}"));

    // Build the swarm with transport + relay client.
    //
    // Desktop: QUIC transport (UDP, built-in TLS 1.3)
    // Mobile:  TCP + Noise + Yamux (QUIC causes SIGSEGV on iOS)
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
            build_behaviour(key, peer_id, relay_behaviour)
        })
        .map_err(|e| NetworkError::SwarmBuild(e.to_string()))?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(300)))
        .build();

    #[cfg(mobile)]
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
                build_behaviour(key, peer_id, relay_behaviour)
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
            .map_err(|e| NetworkError::Subscribe(
                format!("topic '{}': {}", topic_str, e),
            ))?;
    }

    diag::log("start_node: topics subscribed, binding listener...");

    // Listen on all interfaces, OS-assigned port.
    // Desktop: QUIC (UDP). Mobile: TCP.
    #[cfg(desktop)]
    {
        // Listen on both TCP and QUIC so desktop can connect to mobile (TCP) and other desktops (QUIC)
        let tcp_addr: libp2p::Multiaddr = "/ip4/0.0.0.0/tcp/0"
            .parse()
            .expect("valid multiaddr");
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

    #[cfg(mobile)]
    {
        diag::log("start_node: listen_on /ip4/0.0.0.0/tcp/0...");
        let listen_addr: libp2p::Multiaddr = "/ip4/0.0.0.0/tcp/0"
            .parse()
            .expect("valid multiaddr");
        swarm
            .listen_on(listen_addr)
            .map_err(|e| {
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
        diag::log(&format!("start_node: dialing {} known peers...", known_peers.len()));
        for kp in &known_peers {
            if let Ok(pid) = kp.peer_id.parse::<PeerId>() {
                if pid == peer_id {
                    continue; // skip self
                }
                for addr_str in &kp.addresses {
                    if let Ok(addr) = addr_str.parse::<libp2p::Multiaddr>() {
                        let dial_addr = addr.with(libp2p::multiaddr::Protocol::P2p(pid));
                        match swarm.dial(dial_addr.clone()) {
                            Ok(_) => diag::log(&format!("start_node: dialing known peer {}", kp.peer_id)),
                            Err(e) => diag::log(&format!("start_node: known peer dial failed {}: {e}", kp.peer_id)),
                        }
                    }
                }
            }
        }
    }

    diag::log("start_node: spawning event loop...");

    // Create command channel
    let (command_tx, command_rx) = mpsc::channel::<SwarmCommand>(256);

    // Create the message validator (shared via Arc for the event loop)
    let validator = Arc::new(MessageValidator::new());

    // Spawn the swarm event loop
    tokio::spawn(swarm_event_loop(swarm, command_rx, event_tx, validator, None));

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
    let mut kademlia_config =
        kad::Config::new(libp2p::StreamProtocol::new("/alexandria/kad/1.0"));
    kademlia_config.set_query_timeout(Duration::from_secs(60));
    // Lower replication factor for small-network friendliness (default is 20)
    kademlia_config.set_replication_factor(
        std::num::NonZeroUsize::new(8).unwrap(),
    );
    // Provider records expire after 24h (default). Re-publication every
    // 3 min in the event loop keeps them alive. This ensures stale peers
    // are cleaned up if they go offline without graceful shutdown.
    kademlia_config.set_provider_record_ttl(Some(Duration::from_secs(24 * 3600)));
    // Republish provider records every 12h to refresh TTL on other nodes.
    kademlia_config.set_provider_publication_interval(Some(Duration::from_secs(12 * 3600)));
    let kademlia = kad::Behaviour::with_config(peer_id, MemoryStore::new(peer_id), kademlia_config);

    diag::log("build_behaviour: creating identify...");

    // Identify protocol (needed by GossipSub and Kademlia)
    let identify = identify::Behaviour::new(
        identify::Config::new(
            "/alexandria/id/1.0".to_string(),
            keypair.public(),
        )
        .with_agent_version(identify_agent_version())
        .with_push_listen_addr_updates(true),
    );

    diag::log("build_behaviour: creating autonat...");

    // AutoNAT — peer-assisted NAT detection (spec §7.5)
    let autonat = autonat::Behaviour::new(
        peer_id,
        build_autonat_config(),
    );

    diag::log("build_behaviour: creating relay server...");

    // Relay server — any publicly reachable node automatically relays
    // traffic for NATted peers. The server is inert on NATted nodes
    // (nobody can reach it to make HOP requests). When AutoNAT detects
    // public reachability, the relay server naturally becomes active.
    let relay_server = relay::Behaviour::new(peer_id, relay::Config::default());

    diag::log("build_behaviour: creating dcutr...");

    // DCUtR — upgrade relayed connections to direct via hole punching
    let dcutr = dcutr::Behaviour::new(peer_id);

    diag::log("build_behaviour: all sub-behaviours created OK");

    Ok(AlexandriaBehaviour {
        gossipsub,
        kademlia,
        identify,
        autonat,
        relay_server,
        relay_client: relay_behaviour,
        dcutr,
    })
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
    db: Option<Arc<std::sync::Mutex<Database>>>,
) {
    use libp2p::swarm::SwarmEvent;

    use super::types::{PeerExchangeMessage, TOPIC_PEER_EXCHANGE};

    // Track NAT state locally for the Status command
    let mut current_nat_state = NatState::Unknown;
    let mut relay_addrs: Vec<String> = Vec::new();

    // Track whether we've already requested a relay reservation.
    // We only need to do this once per session — after the Identify
    // handshake confirms we're connected to the relay.
    let relay_peer_id = super::discovery::relay_peer_id();
    let mut relay_reservation_requested = false;

    // Periodic Kademlia bootstrap — re-run every 5 minutes to keep
    // the routing table fresh and discover new peers.
    let mut kad_bootstrap_interval = tokio::time::interval(Duration::from_secs(300));
    kad_bootstrap_interval.tick().await; // consume the immediate first tick

    // Periodic provider publish + query — every 3 minutes we:
    // 1. Re-publish our provider record so other Alexandria nodes can find us
    // 2. Query for other providers to discover new Alexandria peers
    let namespace_key = super::discovery::namespace_key();
    let mut provider_interval = tokio::time::interval(Duration::from_secs(180));
    // Initial publish after a short delay (30s) to let bootstrap connections establish
    let provider_initial = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(provider_initial);
    let mut initial_provider_done = false;

    // Periodic peer exchange — broadcast our addresses every 60s
    // so peers-of-peers can discover us transitively via GossipSub.
    let mut peer_exchange_interval = tokio::time::interval(Duration::from_secs(60));
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
                    diag::log(&format!(
                        "Peer exchange: broadcast {addr_count} addresses"
                    ));
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
    fn save_peer_to_db(db: &Option<Arc<std::sync::Mutex<Database>>>, peer_id: &str, addresses: &[String]) {
        if addresses.is_empty() {
            return;
        }
        let Some(db_arc) = db else { return };
        let Ok(db_lock) = db_arc.lock() else { return };
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
            // Initial provider publish (30s after start)
            _ = &mut provider_initial, if !initial_provider_done => {
                initial_provider_done = true;
                diag::log("Publishing provider record for ifftu.alexandria namespace");
                let _ = swarm.behaviour_mut().kademlia.start_providing(namespace_key.clone());
                diag::log("Querying providers for ifftu.alexandria namespace");
                let _ = swarm.behaviour_mut().kademlia.get_providers(namespace_key.clone());
            }
            // Periodic provider refresh
            _ = provider_interval.tick(), if initial_provider_done => {
                let _ = swarm.behaviour_mut().kademlia.start_providing(namespace_key.clone());
                let _ = swarm.behaviour_mut().kademlia.get_providers(namespace_key.clone());
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
                        gossipsub::Event::Message { message, .. }
                    )) => {
                        let topic = message.topic.to_string();
                        log::debug!(
                            "Gossip message on {topic} from {:?} ({} bytes)",
                            message.source,
                            message.data.len()
                        );

                        // Peer exchange messages are NOT signed envelopes —
                        // handle them separately before the validation pipeline.
                        if topic == TOPIC_PEER_EXCHANGE {
                            handle_peer_exchange(&mut swarm, &message.data);
                            continue;
                        }

                        // Step 1: Deserialize the signed envelope
                        let envelope = match serde_json::from_slice::<SignedGossipMessage>(
                            &message.data,
                        ) {
                            Ok(env) => env,
                            Err(e) => {
                                log::debug!(
                                    "Dropping message on {topic}: invalid envelope: {e}"
                                );
                                continue;
                            }
                        };

                        // Step 2: Run the full validation pipeline
                        // (signature, freshness, dedup, schema, authority)
                        if let Err(e) = validator.validate(&envelope) {
                            log::debug!(
                                "Dropping message on {topic} from {}: {e}",
                                envelope.stake_address
                            );
                            continue;
                        }

                        // Step 3: Forward validated message to the application layer
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

                        // If this is the relay peer, perform relay-based discovery:
                        // 1. Request a relay reservation so other NATted peers can reach us
                        // 2. Bootstrap Kademlia now that we have the relay in our routing table
                        // 3. Start providing on the namespace key for peer discovery
                        if Some(peer_id) == relay_peer_id && !relay_reservation_requested {
                            relay_reservation_requested = true;
                            diag::log(&format!(
                                "Identified relay peer {peer_id} — requesting reservation"
                            ));

                            // Request relay reservation via listen_on with the circuit address.
                            if let Some(circuit_addr) = super::discovery::relay_circuit_addr() {
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

                            // Start providing on the namespace key so other peers can find us.
                            let _ = swarm.behaviour_mut().kademlia.start_providing(
                                namespace_key.clone(),
                            );
                            diag::log("Kademlia: started providing on namespace key");
                        }
                    }
                    // Kademlia events — DHT routing + provider discovery
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::Kademlia(event)) => {
                        match event {
                            kad::Event::RoutingUpdated { peer, addresses, .. } => {
                                diag::log(&format!(
                                    "Kademlia: routing updated for {peer} ({} addrs)",
                                    addresses.len()
                                ));
                            }
                            kad::Event::OutboundQueryProgressed {
                                result: kad::QueryResult::GetProviders(Ok(
                                    kad::GetProvidersOk::FoundProviders { providers, .. }
                                )),
                                ..
                            } => {
                                // Found Alexandria peers in the DHT!
                                // `providers` is a HashSet<PeerId>
                                let local = *swarm.local_peer_id();
                                diag::log(&format!(
                                    "DHT: GetProviders returned {} provider(s)",
                                    providers.len()
                                ));
                                for peer in &providers {
                                    diag::log(&format!(
                                        "DHT: provider peer={peer} is_self={} is_connected={}",
                                        *peer == local,
                                        swarm.is_connected(peer)
                                    ));
                                }
                                for peer in providers {
                                    if peer != local && !swarm.is_connected(&peer) {
                                        diag::log(&format!(
                                            "DHT: discovered Alexandria peer {peer}, dialing..."
                                        ));
                                        // Add to GossipSub so we exchange messages
                                        swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer);
                                        // Try to dial them
                                        if let Err(e) = swarm.dial(peer) {
                                            diag::log(&format!("DHT: failed to dial {peer}: {e}"));
                                        }
                                    }
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
                        match &event {
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
                                    relay_peer: relay_str,
                                }).await;
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
                    SwarmEvent::NewListenAddr { address, .. } => {
                        log::info!("Listening on {address}");
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
                    }
                    SwarmEvent::ConnectionClosed { peer_id, .. } => {
                        log::debug!("Disconnected from peer: {peer_id}");
                        diag::log(&format!("P2P event: peer disconnected — {peer_id}"));
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
        reply_rx.recv().await.unwrap_or(Err(NetworkError::NotRunning))
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

    /// Shutdown the node.
    pub async fn shutdown(&mut self) {
        self.running = false;
        let _ = self.command_tx.send(SwarmCommand::Shutdown).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keypair_from_cardano_key_deterministic() {
        let key_bytes = [0x42u8; 32];
        let kp1 = keypair_from_cardano_key(&key_bytes).unwrap();
        let kp2 = keypair_from_cardano_key(&key_bytes).unwrap();
        assert_eq!(
            kp1.public().to_peer_id(),
            kp2.public().to_peer_id(),
            "same key should produce same PeerId"
        );
    }

    #[test]
    fn different_keys_produce_different_peer_ids() {
        let kp1 = keypair_from_cardano_key(&[0x01u8; 32]).unwrap();
        let kp2 = keypair_from_cardano_key(&[0x02u8; 32]).unwrap();
        assert_ne!(
            kp1.public().to_peer_id(),
            kp2.public().to_peer_id(),
            "different keys should produce different PeerIds"
        );
    }

    #[test]
    fn peer_id_is_valid_base58() {
        let kp = keypair_from_cardano_key(&[0xABu8; 32]).unwrap();
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
        let keypair = keypair_from_cardano_key(&[0x42u8; 32]).unwrap();
        let (event_tx, _event_rx) = mpsc::channel(16);

        let mut node = start_node(keypair, event_tx, vec![])
            .await
            .expect("node should start");

        // Check status
        let status = node.status().await.expect("should get status");
        assert!(status.is_running);
        assert!(status.peer_id.is_some());
        assert_eq!(status.subscribed_topics.len(), 6); // All 6 topics (5 app + 1 peer exchange)
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
        let keypair = keypair_from_cardano_key(&key_bytes).expect("derive keypair");
        let peer_id = keypair.public().to_peer_id();
        assert!(
            peer_id.to_string().starts_with("12D3KooW"),
            "PeerId should be Ed25519, got: {}",
            peer_id
        );

        // Deterministic: same mnemonic → same PeerId
        let w2 = wallet::wallet_from_mnemonic(&mnemonic).expect("wallet 2");
        let mut kb2 = [0u8; 32];
        kb2.copy_from_slice(&w2.payment_key_extended[..32]);
        let kp2 = keypair_from_cardano_key(&kb2).expect("kp2");
        assert_eq!(
            kp2.public().to_peer_id(),
            peer_id,
            "same mnemonic should produce same PeerId"
        );
    }
}
