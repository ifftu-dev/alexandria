//! IPC commands for the P2P network layer.
//!
//! These commands expose the libp2p swarm to the frontend for
//! starting/stopping the node, querying network status, listing
//! connected peers, and publishing gossip messages.

use crate::profile::scope::ProfileState as State;
use tauri::{AppHandle, Manager};

use crate::crypto::wallet;
use crate::db::executor::DatabaseWorkload;
use crate::diag;
use crate::p2p::device_id;
use crate::p2p::inbound::{self, InboundDatabase};
use crate::p2p::network::{self, derive_libp2p_keypair};
use crate::p2p::types::NetworkStatus;
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

    // Inbound network work outlives this command, so it cannot hold the
    // command's lease without blocking profile locking. Pin the session
    // instead: while the command lease is current, the open session is the
    // one that admitted it (sessions are fresh UUIDs and never reused).
    let command_lease = state.profile_lease();
    let session = state
        .profile_operations
        .session()
        .filter(|_| command_lease.is_current())
        .ok_or("profile session is locked or has changed")?;
    let inbound_db = InboundDatabase::pinned(
        state.db_executor.clone(),
        state.profile_operations.clone(),
        session,
    );

    // Load known peers from the database so we can reconnect to them.
    let known_peers = match state
        .db_executor
        .execute(
            DatabaseWorkload::Background,
            state.profile_lease(),
            "p2p.load-known-peers",
            |db| Ok(load_known_peers(db.conn())),
        )
        .await
    {
        Ok(peers) => peers,
        Err(error) => {
            diag::log(&format!(
                "p2p_start: unable to load known peers, continuing without them: {error}"
            ));
            Vec::new()
        }
    };
    diag::log(&format!(
        "p2p_start: loaded {} known peers from DB",
        known_peers.len()
    ));

    // Clone handles for the spawned task
    let p2p_node = state.p2p_node.clone();
    let peer_id_for_return = peer_id.clone();
    let app_for_events = app.clone();
    let operations = state.profile_operations.clone();

    // Spawn the heavy work (node startup + event loop) in a background task.
    state
        .profile_operations
        .spawn_job(async move {
            diag::log("p2p_bg: background task started");

            // Double-check: another call may have started the node while we waited
            let mut node_slot = p2p_node.lock().await;
            if node_slot.is_some() {
                diag::log("p2p_bg: node already started by another task, skipping");
                return;
            }

            diag::log("p2p_bg: creating event channel...");
            let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(256);

            // Inbound gossip is applied by one dispatcher, in arrival order,
            // as profile-fenced Background-lane executor jobs. The consumer
            // below only queues it (bounded, dropping when full), so a slow
            // database never backs up the swarm's event channel. UI events
            // are emitted after the message's job has committed.
            let app_events = app_for_events.clone();
            let (mut gossip, gossip_dispatcher) =
                inbound::gossip_ingest(inbound_db.clone(), move |event| event.emit(&app_events));
            operations.spawn_job(gossip_dispatcher).await;

            // Spawn event consumer
            operations
                .spawn_job(async move {
                    while let Some(event) = event_rx.recv().await {
                        match event {
                            crate::p2p::types::P2pEvent::PeerConnected { peer_id } => {
                                diag::log(&format!("P2P event: peer connected — {peer_id}"));
                            }
                            crate::p2p::types::P2pEvent::PeerDisconnected { peer_id } => {
                                diag::log(&format!("P2P event: peer disconnected — {peer_id}"));
                            }
                            crate::p2p::types::P2pEvent::GossipMessage { topic, message } => {
                                log::debug!("P2P event: gossip message on {topic}");
                                gossip.offer(topic, message);
                            }
                            crate::p2p::types::P2pEvent::StatusChanged(status) => {
                                log::debug!("P2P: {} peers", status.connected_peers);
                            }
                            _ => {}
                        }
                    }
                })
                .await;

            diag::log("p2p_bg: calling start_node_with_db...");

            // Start the node with the active-profile DB wired in. The DB
            // handle is what activates:
            //   - the registry-backed identity check for privileged-topic
            //     gossip (see `p2p::registry::check_message`)
            //   - inbound `/alexandria/vc-fetch/1.0` responses against
            //     local credentials (otherwise the swarm replies
            //     `FetchResponse::NotFound` to every request).
            //
            // `inbound_db` runs every such lookup as a Background-lane
            // executor job fenced to this profile session, so work for a
            // locked or replaced profile is refused rather than committed.
            // Federation + DHT-server settings, read before the swarm spins
            // up so discovery surfaces and kad mode reflect them.
            let network_settings = inbound_db
                .run("p2p.load-network-settings", |database| {
                    let conn = database.conn();
                    Ok((
                        crate::settings::SettingsStore::get(
                            conn,
                            crate::settings::registry::keys::P2P_EXTRA_RELAYS,
                        )
                        .0,
                        crate::settings::SettingsStore::get(
                            conn,
                            crate::settings::registry::keys::P2P_DHT_SERVER,
                        ),
                        crate::settings::SettingsStore::get(
                            conn,
                            crate::settings::registry::keys::P2P_RELAY_REGISTRY_CACHE,
                        )
                        .0,
                    ))
                })
                .await;
            let (extras_raw, serve, cached) = match network_settings {
                Ok((extras, serve, cached)) => (Some(extras), serve, Some(cached)),
                Err(error) => {
                    diag::log(&format!(
                        "p2p_bg: network settings unavailable, using defaults: {error}"
                    ));
                    (None, false, None)
                }
            };
            let dht_server = {
                let extras: Vec<crate::p2p::discovery::ExtraRelay> = extras_raw
                    .and_then(|raw| serde_json::from_value(raw).ok())
                    .unwrap_or_default();
                crate::p2p::discovery::set_extra_relays(extras);
                // Contributing (relay-serving + DHT-serving) is desktop-only.
                // Mobile stays a pure client regardless of the stored setting:
                // backgrounded apps churn the routing table and can't be
                // reached to relay anyway.
                serve && cfg!(desktop)
            };

            // On-chain relay registry: install the last-known-good authorized
            // issuer set immediately (so receipt verification has it before
            // any network round-trip), then refresh from chain in the
            // background. Genesis issuers stay trusted regardless, so this
            // only ever *adds* — naming works offline / pre-registry.
            {
                if let Some(list) = cached
                    .as_ref()
                    .and_then(|v| v.get("issuers"))
                    .and_then(|v| v.as_array())
                {
                    let issuers: Vec<String> = list
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                    if !issuers.is_empty() {
                        crate::p2p::relay_registry::set_onchain_issuers(issuers);
                    }
                }

                let registry_db = inbound_db.clone();
                operations
                    .spawn_job(async move {
                        // Resolve the Blockfrost project id in its own job; no
                        // database work spans the chain round-trip.
                        let project_id = match registry_db
                            .run("p2p.relay-registry-project-id", |database| {
                                Ok(crate::cardano::blockfrost::resolve_project_id(Some(
                                    database.conn(),
                                )))
                            })
                            .await
                        {
                            Ok(project_id) => project_id,
                            Err(error) => {
                                log::debug!("relay registry refresh skipped: {error}");
                                return;
                            }
                        };
                        let Some(pid) = project_id else {
                            return;
                        };
                        let Ok(bf) = crate::cardano::blockfrost::BlockfrostClient::new(pid) else {
                            return;
                        };
                        if let Some((seq, issuers)) =
                            crate::cardano::relay_registry_chain::refresh_from_chain(&bf).await
                        {
                            let cache = crate::settings::registry::JsonSetting(serde_json::json!({
                                "seq": seq,
                                "issuers": issuers,
                            }));
                            if let Err(error) = registry_db
                                .run("p2p.relay-registry-cache", move |database| {
                                    crate::settings::SettingsStore::set(
                                        database.conn(),
                                        crate::settings::registry::keys::P2P_RELAY_REGISTRY_CACHE,
                                        cache,
                                    )
                                    .map(drop)
                                    .map_err(|e| e.to_string())
                                })
                                .await
                            {
                                log::debug!("relay registry cache not saved: {error}");
                            }
                        }
                    })
                    .await;
            }

            match network::start_node_with_db(
                keypair,
                event_tx,
                known_peers,
                Some(inbound_db.clone()),
                dht_server,
            )
            .await
            {
                Ok(node) => {
                    *node_slot = Some(node);
                    diag::log(&format!("p2p_bg: node started with PeerId: {peer_id}"));
                }
                Err(e) => {
                    diag::log(&format!("p2p_bg: start_node FAILED (non-fatal): {e}"));
                }
            }
        })
        .await;

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

/// Return the user-configured extra (community) relays. These extend the
/// built-in operator relays for *connectivity* (circuit relay + DHT) —
/// they do NOT gain username-receipt trust (that stays with the
/// on-chain registry).
#[tauri::command]
pub async fn get_extra_relays(
    state: State<'_, AppState>,
) -> Result<Vec<crate::p2p::discovery::ExtraRelay>, String> {
    let raw = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "p2p.get-extra-relays",
            |db| {
                Ok(crate::settings::SettingsStore::get(
                    db.conn(),
                    crate::settings::registry::keys::P2P_EXTRA_RELAYS,
                )
                .0)
            },
        )
        .await?;
    Ok(serde_json::from_value(raw).unwrap_or_default())
}

/// Validate + persist the extra-relay set, and apply it to the running
/// node's discovery surface immediately. New circuit reservations are
/// established on the next p2p start. Returns the saved list.
#[tauri::command]
pub async fn save_extra_relays(
    state: State<'_, AppState>,
    relays: Vec<crate::p2p::discovery::ExtraRelay>,
) -> Result<Vec<crate::p2p::discovery::ExtraRelay>, String> {
    use libp2p::PeerId;

    // Validate every entry up front so a single bad row rejects the
    // whole save with an actionable message (rather than silently
    // dropping it the way the discovery filter does).
    for (i, r) in relays.iter().enumerate() {
        if r.peer_id.parse::<PeerId>().is_err() {
            return Err(format!("relay {}: invalid peer id '{}'", i + 1, r.peer_id));
        }
        if r.host.trim().is_empty() {
            return Err(format!("relay {}: host is empty", i + 1));
        }
        if r.port == 0 {
            return Err(format!("relay {}: port must be non-zero", i + 1));
        }
    }

    let setting = crate::settings::registry::JsonSetting(
        serde_json::to_value(&relays).map_err(|e| e.to_string())?,
    );
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "p2p.save-extra-relays",
            move |db| {
                crate::settings::SettingsStore::set(
                    db.conn(),
                    crate::settings::registry::keys::P2P_EXTRA_RELAYS,
                    setting,
                )
                .map_err(|e| e.to_string())
            },
        )
        .await?;

    // Apply to the live discovery surface so availability/bootstrap pick
    // it up without a restart (circuit reservations come on next start).
    crate::p2p::discovery::set_extra_relays(relays.clone());
    Ok(relays)
}

fn load_known_peers(conn: &rusqlite::Connection) -> Vec<network::KnownPeer> {
    conn.prepare(
        "SELECT peer_id, addresses FROM peers WHERE addresses IS NOT NULL AND addresses != '[]'",
    )
    .ok()
    .and_then(|mut stmt| {
        stmt.query_map([], |row| {
            let peer_id = row.get(0)?;
            let addrs_json: String = row.get(1)?;
            let addresses = serde_json::from_str(&addrs_json).unwrap_or_default();
            Ok(network::KnownPeer { peer_id, addresses })
        })
        .ok()
        .map(|rows| rows.filter_map(Result::ok).collect())
    })
    .unwrap_or_default()
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
            // And it must be passed `Some(...)`, with the same
            // profile-fenced handle the gossip dispatcher uses, so the
            // validator and the vc-fetch responder share state with the
            // rest of the app.
            "Some(inbound_db.clone())",
            "inbound::gossip_ingest(inbound_db.clone()",
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

    #[test]
    fn known_peer_bootstrap_keeps_valid_rows_and_tolerates_bad_addresses() {
        let conn = rusqlite::Connection::open_in_memory().expect("database");
        conn.execute_batch(
            "CREATE TABLE peers (peer_id TEXT PRIMARY KEY, addresses TEXT); \
             INSERT INTO peers (peer_id, addresses) VALUES \
               ('peer-valid', '[\"/ip4/127.0.0.1/tcp/4001\"]'), \
               ('peer-malformed', 'not-json'), \
               ('peer-empty', '[]'), \
               ('peer-null', NULL);",
        )
        .expect("peer fixtures");

        let peers = super::load_known_peers(&conn);
        assert_eq!(peers.len(), 2);
        assert_eq!(peers[0].peer_id, "peer-valid");
        assert_eq!(peers[0].addresses, vec!["/ip4/127.0.0.1/tcp/4001"]);
        assert_eq!(peers[1].peer_id, "peer-malformed");
        assert!(peers[1].addresses.is_empty());
    }
}
