//! IPC commands for the P2P network layer.
//!
//! These commands expose the libp2p swarm to the frontend for
//! starting/stopping the node, querying network status, listing
//! connected peers, and publishing gossip messages.

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::classroom::manager as classroom_manager;
use crate::classroom::types::is_classroom_topic;
use crate::crypto::wallet;
use crate::db::Database;
use crate::diag;
use crate::p2p::catalog as p2p_catalog;
use crate::p2p::evidence as p2p_evidence;
use crate::p2p::governance as p2p_governance;
use crate::p2p::network::{self, keypair_from_cardano_key};
use crate::p2p::taxonomy as p2p_taxonomy;
use crate::p2p::types::{
    NetworkStatus, TOPIC_CATALOG, TOPIC_EVIDENCE, TOPIC_GOVERNANCE, TOPIC_TAXONOMY,
};
use crate::AppState;

/// Start the P2P network node in the background.
///
/// Derives the libp2p identity from the Cardano payment key (creating
/// a cryptographic link between P2P PeerId and on-chain identity).
/// Requires an unlocked wallet.
///
/// Returns immediately ("ok"). The actual node startup is spawned as a
/// background task so it never blocks the IPC handler (and cannot crash
/// it). The frontend detects the node coming online via `p2p_status` polling.
#[tauri::command]
pub async fn p2p_start(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    diag::log("p2p_start called");
    log::info!("[p2p_start] called");

    // Check if already running
    {
        let node = state.p2p_node.lock().await;
        if node.is_some() {
            diag::log("p2p_start: already running");
            return Ok("already running".to_string());
        }
    }

    diag::log("p2p_start: retrieving keystore...");

    // Get the payment key from the unlocked wallet
    let payment_key_bytes: [u8; 32] = {
        let ks_guard = state.keystore.lock().await;
        let ks = ks_guard
            .as_ref()
            .ok_or_else(|| "wallet is locked — unlock first".to_string())?;
        let mnemonic = ks.retrieve_mnemonic().map_err(|e| e.to_string())?;
        let w = wallet::wallet_from_mnemonic(&mnemonic).map_err(|e| e.to_string())?;
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&w.payment_key_extended[..32]);
        bytes
    };

    diag::log("p2p_start: deriving keypair...");

    // Derive libp2p keypair from Cardano key
    let keypair = keypair_from_cardano_key(&payment_key_bytes).map_err(|e| e.to_string())?;
    let peer_id = keypair.public().to_peer_id().to_string();

    diag::log(&format!(
        "p2p_start: PeerId={peer_id}, spawning background task..."
    ));

    // Load known peers from the database so we can reconnect to them.
    let known_peers: Vec<network::KnownPeer> = {
        match state.db.lock() {
            Ok(db) => {
                db.conn().prepare(
                    "SELECT peer_id, addresses FROM peers WHERE addresses IS NOT NULL AND addresses != '[]'"
                ).ok().map(|mut stmt| {
                    stmt.query_map([], |row| {
                        let peer_id: String = row.get(0)?;
                        let addrs_json: String = row.get(1)?;
                        let addresses: Vec<String> = serde_json::from_str(&addrs_json).unwrap_or_default();
                        Ok(network::KnownPeer { peer_id, addresses })
                    })
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default()
                }).unwrap_or_default()
            }
            Err(e) => {
                diag::log(&format!("p2p_start: DB mutex poisoned, skipping known peers: {e}"));
                vec![]
            }
        }
    };
    diag::log(&format!(
        "p2p_start: loaded {} known peers from DB",
        known_peers.len()
    ));

    // Clone handles for the spawned task
    let p2p_node = state.p2p_node.clone();
    let db_for_events: Arc<std::sync::Mutex<Database>> = state.db.clone();
    let peer_id_for_return = peer_id.clone();
    let app_for_events = app.clone();

    // Spawn the heavy work (node startup + event loop) in a background task.
    tokio::spawn(async move {
        diag::log("p2p_bg: background task started");

        // Double-check: another call may have started the node while we waited
        {
            let node = p2p_node.lock().await;
            if node.is_some() {
                diag::log("p2p_bg: node already started by another task, skipping");
                return;
            }
        }

        diag::log("p2p_bg: creating event channel...");
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(256);

        // Spawn event consumer
        let db_events = db_for_events.clone();
        let app_events = app_for_events.clone();
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                match &event {
                    crate::p2p::types::P2pEvent::PeerConnected { peer_id } => {
                        diag::log(&format!("P2P event: peer connected — {peer_id}"));
                    }
                    crate::p2p::types::P2pEvent::PeerDisconnected { peer_id } => {
                        diag::log(&format!("P2P event: peer disconnected — {peer_id}"));
                    }
                    crate::p2p::types::P2pEvent::GossipMessage { topic, message } => {
                        log::debug!("P2P event: gossip message on {topic}");
                        let db = match db_events.lock() {
                            Ok(db) => db,
                            Err(e) => {
                                log::error!("P2P gossip handler: DB mutex poisoned: {e}");
                                continue;
                            }
                        };
                        if topic == TOPIC_CATALOG {
                            let _ = p2p_catalog::handle_catalog_message(&db, message);
                        } else if topic == TOPIC_EVIDENCE {
                            let _ = p2p_evidence::handle_evidence_message(&db, message);
                        } else if topic == TOPIC_TAXONOMY {
                            let _ = p2p_taxonomy::handle_taxonomy_message(&db, message);
                        } else if topic == TOPIC_GOVERNANCE {
                            let _ = p2p_governance::handle_governance_message(&db, message);
                        } else if is_classroom_topic(topic) {
                            if topic.ends_with("/meta/1.0") {
                                classroom_manager::handle_classroom_meta(&db, message, &app_events);
                            } else {
                                classroom_manager::handle_classroom_message(
                                    &db,
                                    message,
                                    &app_events,
                                );
                            }
                        }
                    }
                    crate::p2p::types::P2pEvent::StatusChanged(status) => {
                        log::debug!("P2P: {} peers", status.connected_peers);
                    }
                    _ => {}
                }
            }
        });

        diag::log("p2p_bg: calling start_node...");

        // Start the node
        match network::start_node(keypair, event_tx, known_peers).await {
            Ok(node) => {
                *p2p_node.lock().await = Some(node);
                diag::log(&format!("p2p_bg: node started with PeerId: {peer_id}"));
            }
            Err(e) => {
                diag::log(&format!("p2p_bg: start_node FAILED (non-fatal): {e}"));
            }
        }
    });

    Ok(peer_id_for_return)
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
            nat_status: crate::p2p::types::NatState::Unknown,
            relay_addresses: vec![],
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

