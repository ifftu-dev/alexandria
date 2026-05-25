//! IPC commands for the P2P network layer.
//!
//! These commands expose the libp2p swarm to the frontend for
//! starting/stopping the node, querying network status, listing
//! connected peers, and publishing gossip messages.

use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use crate::classroom::manager as classroom_manager;
use crate::classroom::types::is_classroom_topic;
use crate::crypto::wallet;
use crate::db::Database;
use crate::diag;
use crate::p2p::catalog as p2p_catalog;
use crate::p2p::device_id;
use crate::p2p::governance as p2p_governance;
use crate::p2p::network::{self, derive_libp2p_keypair};
use crate::p2p::opinions as p2p_opinions;
use crate::p2p::pinboard as p2p_pinboard;
use crate::p2p::presentation as p2p_presentation;
use crate::p2p::sentinel as p2p_sentinel;
use crate::p2p::taxonomy as p2p_taxonomy;
use crate::p2p::types::{
    NetworkStatus, TOPIC_CATALOG, TOPIC_GOVERNANCE, TOPIC_OPINIONS, TOPIC_PINBOARD,
    TOPIC_SENTINEL_PRIORS, TOPIC_TAXONOMY, TOPIC_VC_DID, TOPIC_VC_PRESENTATION, TOPIC_VC_STATUS,
};
use crate::p2p::vc_did as p2p_vc_did;
use crate::p2p::vc_status as p2p_vc_status;
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

    diag::log("p2p_start: loading device id...");

    // Per-device secret keeps the libp2p PeerId distinct across installs
    // unlocked with the same vault. Generated on first launch, persisted
    // under app_data_dir.
    let device_id_bytes = {
        let app_data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("app_data_dir: {e}"))?;
        device_id::load_or_create(&app_data_dir).map_err(|e| format!("device_id: {e}"))?
    };

    diag::log("p2p_start: deriving keypair...");

    // Derive a per-device libp2p keypair from the wallet payment key + device id
    let keypair =
        derive_libp2p_keypair(&payment_key_bytes, &device_id_bytes).map_err(|e| e.to_string())?;
    let peer_id = keypair.public().to_peer_id().to_string();

    diag::log(&format!(
        "p2p_start: PeerId={peer_id}, spawning background task..."
    ));

    // Load known peers from the database so we can reconnect to them.
    let known_peers: Vec<network::KnownPeer> = {
        match state.db.lock() {
            Ok(guard) => {
                match guard.as_ref() {
                    Some(db) => {
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
                    None => {
                        diag::log("p2p_start: DB not initialized, skipping known peers");
                        vec![]
                    }
                }
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
    let db_for_events: Arc<std::sync::Mutex<Option<Database>>> = state.db.clone();
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
                        let guard = match db_events.lock() {
                            Ok(g) => g,
                            Err(e) => {
                                log::error!("P2P gossip handler: DB mutex poisoned: {e}");
                                continue;
                            }
                        };
                        let db = match guard.as_ref() {
                            Some(db) => db,
                            None => {
                                log::error!("P2P gossip handler: DB not initialized");
                                continue;
                            }
                        };
                        if topic == TOPIC_CATALOG {
                            let _ = p2p_catalog::handle_catalog_message(db, message);
                        } else if topic == TOPIC_TAXONOMY {
                            let _ = p2p_taxonomy::handle_taxonomy_message(db, message);
                        } else if topic == TOPIC_GOVERNANCE {
                            let _ = p2p_governance::handle_governance_message(db, message);
                        } else if topic == TOPIC_OPINIONS {
                            let _ = p2p_opinions::handle_opinion_message(db, message);
                        } else if topic == TOPIC_VC_DID {
                            let _ = p2p_vc_did::handle_did_message(db, message);
                        } else if topic == TOPIC_VC_STATUS {
                            let _ = p2p_vc_status::handle_status_message(db, message);
                        } else if topic == TOPIC_VC_PRESENTATION {
                            let _ = p2p_presentation::handle_presentation_message(db, message);
                        } else if topic == TOPIC_PINBOARD {
                            let _ = p2p_pinboard::handle_pinboard_message(db, message);
                        } else if topic == TOPIC_SENTINEL_PRIORS {
                            let _ = p2p_sentinel::handle_sentinel_prior_message(db, message);
                        } else if is_classroom_topic(topic) {
                            if topic.ends_with("/meta/1.0") {
                                classroom_manager::handle_classroom_meta(db, message, &app_events);
                            } else {
                                classroom_manager::handle_classroom_message(
                                    db,
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

        diag::log("p2p_bg: calling start_node_with_db...");

        // Start the node with the active-profile DB wired in. The DB
        // handle is what activates:
        //   - the registry-backed identity check for privileged-topic
        //     gossip (see `MessageValidator::with_db` + `p2p::registry`)
        //   - inbound `/alexandria/vc-fetch/1.0` responses against
        //     local credentials (otherwise the swarm replies
        //     `FetchResponse::NotFound` to every request).
        //
        // `db_for_events` is `Arc<Mutex<Option<Database>>>`; the
        // validator and fetch responder both lock-then-check on each
        // use, so it is safe to hand them the same handle even if a
        // future revision lets the DB go away.
        match network::start_node_with_db(
            keypair,
            event_tx,
            known_peers,
            Some(db_for_events.clone()),
        )
        .await
        {
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

#[cfg(test)]
mod wiring_tests {
    //! Source-level regression test for the bug where the production
    //! `p2p_start` command silently constructed a swarm without a DB
    //! handle, leaving the registry-backed privileged-topic identity
    //! check fail-open and disabling inbound `vc-fetch` responses.
    //!
    //! This test is grep-style on purpose: a behavioural test would
    //! have to spin up a real swarm + DB + multiple Tauri State
    //! shims, which is impractical for an IPC handler. Reading the
    //! source and asserting the wiring shape is cheap and catches the
    //! exact regression we saw.

    const P2P_RS: &str = include_str!("p2p.rs");

    #[test]
    fn p2p_start_passes_db_to_start_node_with_db() {
        // The body of `p2p_start`'s spawned task MUST hand a real DB
        // handle into the swarm constructor. Two positive checks pin
        // the exact shape so a future refactor that accidentally
        // reintroduces `None` here fails CI loudly.
        //
        // We deliberately avoid a negative grep for the no-DB
        // `start_node` wrapper because the assertion error message
        // would itself contain that string and trigger the check —
        // the wrapper was deleted from `network.rs`, so any caller
        // reaching for it would already fail to compile.
        let needles = [
            // The constructor must be the DB-aware variant.
            "start_node_with_db(",
            // And it must be passed `Some(...)`, with the same DB
            // handle the gossip-event consumer uses, so the validator
            // and the vc-fetch responder share state with the rest
            // of the app.
            "Some(db_for_events.clone())",
        ];
        for needle in needles {
            assert!(
                P2P_RS.contains(needle),
                "regression: commands/p2p.rs no longer wires the active-profile DB \
                 into the swarm — privileged-topic gossip and inbound vc-fetch \
                 will silently misbehave (see the bug fixed in PR B)"
            );
        }
    }
}
