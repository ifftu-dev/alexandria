//! IPC commands for the P2P network layer.
//!
//! These commands expose the libp2p swarm to the frontend for
//! starting/stopping the node, querying network status, listing
//! connected peers, and publishing gossip messages.

use std::sync::Arc;

use tauri::State;
use tokio::sync::Mutex;

use crate::crypto::wallet;
use crate::db::Database;
use crate::p2p::catalog as p2p_catalog;
use crate::p2p::evidence as p2p_evidence;
use crate::p2p::governance as p2p_governance;
use crate::p2p::network::{self, keypair_from_cardano_key};
use crate::p2p::taxonomy as p2p_taxonomy;
use crate::p2p::types::{
    NetworkStatus, TOPIC_CATALOG, TOPIC_EVIDENCE, TOPIC_GOVERNANCE, TOPIC_TAXONOMY,
};
use crate::AppState;

/// Start the P2P network node.
///
/// Derives the libp2p identity from the Cardano payment key (creating
/// a cryptographic link between P2P PeerId and on-chain identity).
/// Requires an unlocked wallet.
#[tauri::command]
pub async fn p2p_start(state: State<'_, AppState>) -> Result<String, String> {
    // Check if already running
    {
        let node = state.p2p_node.lock().await;
        if node.is_some() {
            return Err("P2P node is already running".to_string());
        }
    }

    // Get the payment key from the unlocked wallet
    let payment_key_bytes: [u8; 32] = {
        let ks_guard = state.keystore.lock().await;
        let ks = ks_guard
            .as_ref()
            .ok_or_else(|| "wallet is locked — unlock first".to_string())?;
        let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
        let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
        // First 32 bytes of the extended key are the Ed25519 scalar
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&w.payment_key_extended[..32]);
        bytes
    };

    // Derive libp2p keypair from Cardano key
    let keypair =
        keypair_from_cardano_key(&payment_key_bytes).map_err(|e| e.to_string())?;
    let peer_id = keypair.public().to_peer_id().to_string();

    // Create event channel (events are logged for now; frontend emission in a later PR)
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(256);

    // Clone the database handle for the event consumer task
    let db_for_events: Arc<Mutex<Database>> = state.db.clone();

    // Spawn event consumer — handles incoming gossip messages by topic
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match &event {
                crate::p2p::types::P2pEvent::PeerConnected { peer_id } => {
                    log::info!("P2P event: peer connected — {peer_id}");
                }
                crate::p2p::types::P2pEvent::PeerDisconnected { peer_id } => {
                    log::info!("P2P event: peer disconnected — {peer_id}");
                }
                crate::p2p::types::P2pEvent::GossipMessage { topic, message } => {
                    log::debug!("P2P event: gossip message on {topic}");
                    let db = db_for_events.lock().await;
                    if topic == TOPIC_CATALOG {
                        match p2p_catalog::handle_catalog_message(&db, message) {
                            Ok(ann) => {
                                log::info!(
                                    "Catalog: indexed '{}' (v{}) from {}",
                                    ann.title,
                                    ann.version,
                                    ann.author_address,
                                );
                            }
                            Err(e) => {
                                log::warn!("Failed to handle catalog message: {e}");
                            }
                        }
                    } else if topic == TOPIC_EVIDENCE {
                        match p2p_evidence::handle_evidence_message(&db, message) {
                            Ok(ann) => {
                                log::info!(
                                    "Evidence: stored '{}' for skill '{}' from {}",
                                    ann.evidence_id,
                                    ann.skill_id,
                                    ann.learner_address,
                                );
                            }
                            Err(e) => {
                                log::warn!("Failed to handle evidence message: {e}");
                            }
                        }
                    } else if topic == TOPIC_TAXONOMY {
                        match p2p_taxonomy::handle_taxonomy_message(&db, message) {
                            Ok(update) => {
                                log::info!(
                                    "Taxonomy: applied v{} (cid: {})",
                                    update.version,
                                    update.cid,
                                );
                            }
                            Err(e) => {
                                log::warn!("Failed to handle taxonomy message: {e}");
                            }
                        }
                    } else if topic == TOPIC_GOVERNANCE {
                        match p2p_governance::handle_governance_message(&db, message) {
                            Ok(ann) => {
                                log::info!(
                                    "Governance: processed event for DAO '{}'",
                                    ann.dao_id,
                                );
                            }
                            Err(e) => {
                                log::warn!("Failed to handle governance message: {e}");
                            }
                        }
                    }
                }
                crate::p2p::types::P2pEvent::StatusChanged(status) => {
                    log::debug!(
                        "P2P event: status changed — {} peers",
                        status.connected_peers
                    );
                }
            }
        }
    });

    // Start the node
    let node = network::start_node(keypair, event_tx)
        .await
        .map_err(|e| e.to_string())?;

    *state.p2p_node.lock().await = Some(node);

    log::info!("P2P node started with PeerId: {peer_id}");
    Ok(peer_id)
}

/// Stop the P2P network node.
#[tauri::command]
pub async fn p2p_stop(state: State<'_, AppState>) -> Result<(), String> {
    let mut node_lock = state.p2p_node.lock().await;
    match node_lock.as_mut() {
        Some(node) => {
            node.shutdown().await;
            *node_lock = None;
            log::info!("P2P node stopped");
            Ok(())
        }
        None => Err("P2P node is not running".to_string()),
    }
}

/// Get the current P2P network status.
#[tauri::command]
pub async fn p2p_status(state: State<'_, AppState>) -> Result<NetworkStatus, String> {
    let node_lock = state.p2p_node.lock().await;
    match node_lock.as_ref() {
        Some(node) => node.status().await.map_err(|e| e.to_string()),
        None => Ok(NetworkStatus {
            is_running: false,
            peer_id: None,
            connected_peers: 0,
            listening_addresses: vec![],
            subscribed_topics: vec![],
        }),
    }
}

/// Get the list of connected peer IDs.
#[tauri::command]
pub async fn p2p_peers(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let node_lock = state.p2p_node.lock().await;
    match node_lock.as_ref() {
        Some(node) => node.connected_peers().await.map_err(|e| e.to_string()),
        None => Ok(vec![]),
    }
}

/// Publish a raw message to a gossip topic.
///
/// This is a low-level command. Higher-level typed publish commands
/// (catalog, evidence, etc.) will be added in subsequent PRs with
/// proper message signing and validation.
#[tauri::command]
pub async fn p2p_publish(
    state: State<'_, AppState>,
    topic: String,
    data: Vec<u8>,
) -> Result<(), String> {
    let node_lock = state.p2p_node.lock().await;
    let node = node_lock
        .as_ref()
        .ok_or_else(|| "P2P node is not running".to_string())?;
    node.publish(&topic, data)
        .await
        .map_err(|e| e.to_string())
}
