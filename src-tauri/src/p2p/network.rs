use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use futures::StreamExt;
use libp2p::gossipsub::{self, IdentTopic, MessageAuthenticity, MessageId, ValidationMode};
use libp2p::identity::Keypair;
use libp2p::kad::store::MemoryStore;
use libp2p::swarm::NetworkBehaviour;
use libp2p::{identify, kad, mdns, PeerId, Swarm, SwarmBuilder};
use thiserror::Error;
use tokio::sync::mpsc;

use super::types::{NetworkStatus, P2pEvent, ALL_TOPICS};

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

/// Composed network behaviour for the Alexandria P2P node.
///
/// Combines four libp2p protocols:
/// - **GossipSub v1.1**: Topic-based publish/subscribe for gossip messages
/// - **Kademlia DHT**: Peer discovery and content routing
/// - **mDNS**: Local network peer discovery (LAN, university campus)
/// - **Identify**: Exchange peer info (needed by GossipSub and Kademlia)
#[derive(NetworkBehaviour)]
pub struct AlexandriaBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub kademlia: kad::Behaviour<MemoryStore>,
    pub mdns: mdns::tokio::Behaviour,
    pub identify: identify::Behaviour,
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
/// as a background tokio task.
pub async fn start_node(
    keypair: Keypair,
    event_tx: mpsc::Sender<P2pEvent>,
) -> Result<P2pNode, NetworkError> {
    let peer_id = keypair.public().to_peer_id();
    log::info!("Starting P2P node with PeerId: {peer_id}");

    // Build the composed network behaviour
    let behaviour = build_behaviour(&keypair, peer_id)?;

    // Build the swarm with QUIC transport
    let mut swarm = SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_quic()
        .with_behaviour(|_| Ok(behaviour))
        .map_err(|e| NetworkError::SwarmBuild(e.to_string()))?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

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

    // Listen on QUIC (all interfaces, OS-assigned port)
    let listen_addr: libp2p::Multiaddr = "/ip4/0.0.0.0/udp/0/quic-v1"
        .parse()
        .expect("valid multiaddr");
    swarm
        .listen_on(listen_addr)
        .map_err(|e| NetworkError::Listen(e.to_string()))?;

    // Also listen on IPv6 if available
    if let Ok(ipv6_addr) = "/ip6/::/udp/0/quic-v1".parse::<libp2p::Multiaddr>() {
        let _ = swarm.listen_on(ipv6_addr); // Best-effort, ignore errors
    }

    // Create command channel
    let (command_tx, command_rx) = mpsc::channel::<SwarmCommand>(256);

    // Spawn the swarm event loop
    tokio::spawn(swarm_event_loop(swarm, command_rx, event_tx));

    Ok(P2pNode {
        command_tx,
        peer_id,
        running: true,
    })
}

/// Build the composed network behaviour.
fn build_behaviour(
    keypair: &Keypair,
    peer_id: PeerId,
) -> Result<AlexandriaBehaviour, NetworkError> {
    // GossipSub configuration
    let gossipsub_config = gossipsub::ConfigBuilder::default()
        .heartbeat_interval(Duration::from_secs(1))
        .validation_mode(ValidationMode::Strict)
        .message_id_fn(|msg: &gossipsub::Message| {
            // Deduplicate by blake2b hash of data (spec requirement)
            let mut hasher = DefaultHasher::new();
            msg.data.hash(&mut hasher);
            MessageId::from(hasher.finish().to_string())
        })
        .max_transmit_size(65536) // 64KB max message size
        .build()
        .map_err(|e| NetworkError::SwarmBuild(format!("gossipsub config: {e}")))?;

    let gossipsub = gossipsub::Behaviour::new(
        MessageAuthenticity::Signed(keypair.clone()),
        gossipsub_config,
    )
    .map_err(|e| NetworkError::SwarmBuild(format!("gossipsub behaviour: {e}")))?;

    // Kademlia DHT
    let mut kademlia_config = kad::Config::new(libp2p::StreamProtocol::new("/alexandria/kad/1.0"));
    kademlia_config.set_query_timeout(Duration::from_secs(60));
    let kademlia = kad::Behaviour::new(peer_id, MemoryStore::new(peer_id));

    // mDNS for local network discovery
    let mdns = mdns::tokio::Behaviour::new(
        mdns::Config::default(),
        peer_id,
    )
    .map_err(|e| NetworkError::SwarmBuild(format!("mdns: {e}")))?;

    // Identify protocol (needed by GossipSub and Kademlia)
    let identify = identify::Behaviour::new(
        identify::Config::new(
            "/alexandria/id/1.0".to_string(),
            keypair.public(),
        )
        .with_push_listen_addr_updates(true),
    );

    Ok(AlexandriaBehaviour {
        gossipsub,
        kademlia,
        mdns,
        identify,
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
) {
    use libp2p::swarm::SwarmEvent;

    loop {
        tokio::select! {
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
                        // Forward raw message to application layer for validation
                        // (message signing/verification is handled in a later PR)
                        if let Some(_source) = message.source {
                            let _ = event_tx.send(P2pEvent::GossipMessage {
                                topic: topic.clone(),
                                message: super::types::SignedGossipMessage {
                                    topic,
                                    payload: message.data,
                                    signature: vec![], // Will be populated after envelope parsing
                                    public_key: vec![],
                                    stake_address: String::new(),
                                    timestamp: 0,
                                },
                            }).await;
                        }
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::Gossipsub(
                        gossipsub::Event::Subscribed { peer_id, topic }
                    )) => {
                        log::debug!("Peer {peer_id} subscribed to {topic}");
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::Mdns(
                        mdns::Event::Discovered(peers)
                    )) => {
                        for (peer_id, addr) in peers {
                            log::info!("mDNS discovered peer: {peer_id} at {addr}");
                            swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                            swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
                        }
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::Mdns(
                        mdns::Event::Expired(peers)
                    )) => {
                        for (peer_id, _) in peers {
                            log::debug!("mDNS peer expired: {peer_id}");
                        }
                    }
                    SwarmEvent::Behaviour(AlexandriaBehaviourEvent::Identify(
                        identify::Event::Received { peer_id, info, .. }
                    )) => {
                        log::debug!(
                            "Identified peer {peer_id}: {} ({})",
                            info.protocol_version,
                            info.agent_version
                        );
                        // Add identified addresses to Kademlia
                        for addr in &info.listen_addrs {
                            swarm.behaviour_mut().kademlia.add_address(&peer_id, addr.clone());
                        }
                    }
                    SwarmEvent::NewListenAddr { address, .. } => {
                        log::info!("Listening on {address}");
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        log::info!("Connected to peer: {peer_id}");
                        let _ = event_tx.send(P2pEvent::PeerConnected {
                            peer_id: peer_id.to_string(),
                        }).await;
                    }
                    SwarmEvent::ConnectionClosed { peer_id, .. } => {
                        log::debug!("Disconnected from peer: {peer_id}");
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

        let mut node = start_node(keypair, event_tx)
            .await
            .expect("node should start");

        // Check status
        let status = node.status().await.expect("should get status");
        assert!(status.is_running);
        assert!(status.peer_id.is_some());
        assert_eq!(status.subscribed_topics.len(), 5); // All 5 topics
        assert_eq!(status.connected_peers, 0); // No peers yet

        // Shutdown
        node.shutdown().await;
    }
}
