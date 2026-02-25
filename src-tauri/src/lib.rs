pub mod cardano;
pub mod commands;
#[cfg(desktop)]
pub mod crypto;
pub mod db;
pub mod domain;
pub mod evidence;
#[cfg(desktop)]
pub mod ipfs;
#[cfg(desktop)]
pub mod p2p;

// Minimal stubs for mobile (no iroh, no stronghold, no libp2p)
#[cfg(mobile)]
pub mod crypto {
    pub mod hash;
    pub mod signing;
    pub mod wallet;
    // keystore not available on mobile (requires iota_stronghold)
}

use db::Database;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

#[cfg(desktop)]
use std::path::PathBuf;

#[cfg(desktop)]
use crypto::keystore::Keystore;
#[cfg(desktop)]
use ipfs::gateway::GatewayClient;
#[cfg(desktop)]
use ipfs::node::ContentNode;
#[cfg(desktop)]
use ipfs::resolver::ContentResolver;
#[cfg(desktop)]
use p2p::network::P2pNode;

/// Shared application state accessible from all Tauri commands.
///
/// The database uses `Mutex` because rusqlite's `Connection` is not
/// safe for concurrent access — `prepare()` mutably borrows an internal
/// `RefCell`, so even concurrent reads will panic. A `Mutex` serializes
/// all database access, preventing the `RefCell already mutably borrowed`
/// panic that occurs when multiple IPC commands run in parallel.
pub struct AppState {
    pub db: Arc<Mutex<Database>>,
    #[cfg(desktop)]
    pub keystore: Arc<Mutex<Option<Keystore>>>,
    #[cfg(desktop)]
    pub vault_dir: PathBuf,
    #[cfg(desktop)]
    pub content_node: Arc<ContentNode>,
    #[cfg(desktop)]
    pub resolver: Arc<Mutex<Option<ContentResolver>>>,
    #[cfg(desktop)]
    pub p2p_node: Arc<Mutex<Option<P2pNode>>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Initialize logging
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Debug)
                        .build(),
                )?;
            }

            // Initialize database
            let app_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data directory");
            std::fs::create_dir_all(&app_dir)
                .expect("failed to create app data directory");

            let db_path = app_dir.join("alexandria.db");
            log::info!("Database path: {}", db_path.display());

            let database =
                Database::open(&db_path).expect("failed to open database");
            database
                .run_migrations()
                .expect("failed to run database migrations");

            // Seed demo data on first launch (skips if tables already populated)
            match db::seed::seed_if_empty(database.conn()) {
                Ok(true) => log::info!("Demo seed data inserted"),
                Ok(false) => log::info!("Database already populated — seed skipped"),
                Err(e) => log::warn!("Seed data failed (non-fatal): {e}"),
            }

            log::info!("Database initialized successfully");

            let db = Arc::new(Mutex::new(database));

            #[cfg(desktop)]
            {
                // Vault directory for Stronghold
                let vault_dir = app_dir.join("stronghold");
                std::fs::create_dir_all(&vault_dir)
                    .expect("failed to create stronghold directory");
                log::info!("Vault directory: {}", vault_dir.display());

                // iroh content node directory
                let iroh_dir = app_dir.join("iroh");
                std::fs::create_dir_all(&iroh_dir)
                    .expect("failed to create iroh data directory");
                log::info!("iroh data directory: {}", iroh_dir.display());

                let content_node = Arc::new(ContentNode::new(&iroh_dir));
                let resolver: Arc<Mutex<Option<ContentResolver>>> =
                    Arc::new(Mutex::new(None));

                // Start the iroh node and initialize the resolver in the background
                let content_node_clone = content_node.clone();
                let db_clone = db.clone();
                let resolver_clone = resolver.clone();
                tauri::async_runtime::spawn(async move {
                    match content_node_clone.start().await {
                        Ok(()) => {
                            log::info!("iroh content node started successfully");

                            // Seed content blobs for dev/testnet elements (idempotent)
                            match db::seed_content::seed_content_if_needed(
                                &db_clone,
                                &content_node_clone,
                            )
                            .await
                            {
                                Ok(n) if n > 0 => {
                                    log::info!("seeded content for {n} elements");
                                }
                                Ok(_) => {}
                                Err(e) => {
                                    log::warn!("content seed failed (non-fatal): {e}");
                                }
                            }

                            // Initialize the content resolver with gateway fallback
                            match GatewayClient::with_defaults() {
                                Ok(gateway) => {
                                    let r = ContentResolver::new(
                                        content_node_clone,
                                        gateway,
                                        db_clone,
                                    );
                                    *resolver_clone.lock().await = Some(r);
                                    log::info!("content resolver initialized with IPFS gateway fallback");
                                }
                                Err(e) => {
                                    log::error!("failed to create gateway client: {e}");
                                }
                            }
                        }
                        Err(e) => log::error!("failed to start iroh content node: {e}"),
                    }
                });

                app.manage(AppState {
                    db,
                    keystore: Arc::new(Mutex::new(None)),
                    vault_dir,
                    content_node,
                    resolver,
                    p2p_node: Arc::new(Mutex::new(None)),
                });
            }

            #[cfg(mobile)]
            {
                app.manage(AppState { db });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::health::check_health,
            // Identity / Wallet (desktop only — requires Stronghold)
            #[cfg(desktop)]
            commands::identity::check_vault_exists,
            #[cfg(desktop)]
            commands::identity::unlock_vault,
            #[cfg(desktop)]
            commands::identity::generate_wallet,
            #[cfg(desktop)]
            commands::identity::restore_wallet,
            #[cfg(desktop)]
            commands::identity::export_mnemonic,
            #[cfg(desktop)]
            commands::identity::lock_vault,
            #[cfg(desktop)]
            commands::identity::get_wallet_info,
            #[cfg(desktop)]
            commands::identity::get_profile,
            #[cfg(desktop)]
            commands::identity::update_profile,
            #[cfg(desktop)]
            commands::identity::publish_profile,
            #[cfg(desktop)]
            commands::identity::resolve_profile,
            // Courses
            commands::courses::list_courses,
            commands::courses::get_course,
            commands::courses::create_course,
            commands::courses::update_course,
            commands::courses::delete_course,
            // Enrollment
            commands::enrollment::list_enrollments,
            commands::enrollment::enroll,
            commands::enrollment::update_progress,
            commands::enrollment::get_progress,
            // Content (desktop only — requires iroh)
            #[cfg(desktop)]
            commands::content::content_add,
            #[cfg(desktop)]
            commands::content::content_get,
            #[cfg(desktop)]
            commands::content::content_has,
            #[cfg(desktop)]
            commands::content::content_node_status,
            #[cfg(desktop)]
            commands::content::content_resolve,
            #[cfg(desktop)]
            commands::content::content_resolve_bytes,
            // Chapters & Elements
            commands::chapters::list_chapters,
            commands::chapters::create_chapter,
            commands::chapters::update_chapter,
            commands::chapters::delete_chapter,
            commands::elements::list_elements,
            commands::elements::create_element,
            commands::elements::update_element,
            commands::elements::delete_element,
            // Course publishing (desktop only — requires iroh)
            #[cfg(desktop)]
            commands::courses::publish_course,
            #[cfg(desktop)]
            commands::courses::fetch_course_document,
            // Evidence
            commands::evidence::list_skill_proofs,
            commands::evidence::list_evidence,
            commands::evidence::list_reputation,
            // Cardano (desktop only — requires Stronghold keystore)
            #[cfg(desktop)]
            commands::cardano::mint_skill_proof_nft,
            #[cfg(desktop)]
            commands::cardano::register_course_onchain,
            // P2P (desktop only — requires libp2p)
            #[cfg(desktop)]
            commands::p2p::p2p_start,
            #[cfg(desktop)]
            commands::p2p::p2p_stop,
            #[cfg(desktop)]
            commands::p2p::p2p_status,
            #[cfg(desktop)]
            commands::p2p::p2p_peers,
            #[cfg(desktop)]
            commands::p2p::p2p_publish,
            // Catalog
            commands::catalog::search_catalog,
            commands::catalog::get_catalog_entry,
            // Governance
            commands::governance::list_daos,
            commands::governance::create_dao,
            commands::governance::get_dao,
            commands::governance::open_election,
            commands::governance::list_elections,
            commands::governance::get_election,
            commands::governance::nominate,
            commands::governance::accept_nomination,
            commands::governance::start_election_voting,
            commands::governance::cast_election_vote,
            commands::governance::finalize_election,
            commands::governance::install_committee,
            commands::governance::submit_proposal,
            commands::governance::list_proposals,
            commands::governance::approve_proposal,
            commands::governance::cancel_proposal,
            commands::governance::cast_proposal_vote,
            commands::governance::resolve_proposal,
            // Reputation
            commands::reputation::get_reputation,
            commands::reputation::compute_reputation,
            commands::reputation::get_instructor_ranking,
            commands::reputation::verify_reputation,
            // Snapshots
            commands::snapshot::snapshot_reputation,
            commands::snapshot::list_snapshots,
            commands::snapshot::get_snapshot,
            commands::snapshot::update_snapshot_status,
            // Taxonomy
            commands::taxonomy::propose_taxonomy_change,
            commands::taxonomy::preview_taxonomy_change,
            commands::taxonomy::publish_taxonomy_ratification,
            commands::taxonomy::get_taxonomy_version,
            commands::taxonomy::list_taxonomy_versions,
            commands::taxonomy::validate_taxonomy_changes,
            commands::taxonomy::list_subject_fields,
            commands::taxonomy::list_subjects,
            commands::taxonomy::list_skills,
            commands::taxonomy::get_skill,
            commands::taxonomy::list_skill_graph_edges,
            commands::taxonomy::tag_element_skill,
            commands::taxonomy::untag_element_skill,
            commands::taxonomy::list_element_skill_tags,
            // Sync (desktop only — requires libp2p)
            #[cfg(desktop)]
            commands::sync::sync_get_device_info,
            #[cfg(desktop)]
            commands::sync::sync_set_device_name,
            #[cfg(desktop)]
            commands::sync::sync_list_devices,
            #[cfg(desktop)]
            commands::sync::sync_remove_device,
            #[cfg(desktop)]
            commands::sync::sync_status,
            #[cfg(desktop)]
            commands::sync::sync_now,
            #[cfg(desktop)]
            commands::sync::sync_set_auto,
            #[cfg(desktop)]
            commands::sync::sync_history,
            // Challenges (desktop only — requires P2P)
            #[cfg(desktop)]
            commands::challenge::submit_evidence_challenge,
            #[cfg(desktop)]
            commands::challenge::list_challenges,
            #[cfg(desktop)]
            commands::challenge::get_challenge,
            #[cfg(desktop)]
            commands::challenge::vote_on_challenge,
            #[cfg(desktop)]
            commands::challenge::resolve_challenge,
            #[cfg(desktop)]
            commands::challenge::list_my_challenges,
            #[cfg(desktop)]
            commands::challenge::list_challenges_against_me,
            // Attestation
            commands::attestation::get_attestation_requirement,
            commands::attestation::list_attestation_requirements,
            commands::attestation::set_attestation_requirement,
            commands::attestation::remove_attestation_requirement,
            commands::attestation::submit_attestation,
            commands::attestation::list_attestations_for_evidence,
            commands::attestation::get_attestation_status,
            commands::attestation::list_unattested_evidence,
            // Integrity
            commands::integrity::integrity_start_session,
            commands::integrity::integrity_submit_snapshot,
            commands::integrity::integrity_end_session,
            commands::integrity::integrity_get_session,
            commands::integrity::integrity_list_sessions,
            commands::integrity::integrity_list_snapshots,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
