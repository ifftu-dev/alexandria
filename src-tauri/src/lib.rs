pub mod cardano;
pub mod commands;
#[cfg(desktop)]
pub mod crypto;
pub mod db;
pub mod diag;
pub mod domain;
pub mod evidence;
pub mod ipfs;
pub mod p2p;
#[cfg(desktop)]
pub mod tutoring;

// Mobile crypto: same modules as desktop but with portable keystore
// (AES-256-GCM + Argon2id instead of IOTA Stronghold)
#[cfg(mobile)]
pub mod crypto {
    pub mod hash;
    #[path = "keystore_portable.rs"]
    pub mod keystore;
    pub mod signing;
    pub mod wallet;
}

use db::Database;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

use crypto::keystore::Keystore;
use ipfs::gateway::GatewayClient;
use ipfs::node::ContentNode;
use ipfs::resolver::ContentResolver;
use p2p::network::P2pNode;
#[cfg(desktop)]
use tutoring::TutoringManager;

/// Shared application state accessible from all Tauri commands.
///
/// The database uses `std::sync::Mutex` (not `tokio::sync::Mutex`)
/// because rusqlite's `Connection` is `!Sync`.  A blocking mutex
/// ensures that the OS thread holding the lock is the *only* thread
/// touching the `Connection`'s internal `RefCell`.  With tokio's async
/// mutex, a `MutexGuard` can migrate between OS threads across
/// `.await` points, and two concurrent tasks could end up calling into
/// the `RefCell` from different OS threads — causing a SIGSEGV on iOS
/// where the tokio thread pool is more aggressive about work-stealing.
pub struct AppState {
    pub db: Arc<std::sync::Mutex<Database>>,
    pub keystore: Arc<Mutex<Option<Keystore>>>,
    pub vault_dir: PathBuf,
    pub content_node: Arc<ContentNode>,
    pub resolver: Arc<Mutex<Option<ContentResolver>>>,
    pub p2p_node: Arc<Mutex<Option<P2pNode>>>,
    #[cfg(desktop)]
    pub tutoring: Arc<TutoringManager>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Initialize logging (always enabled so we can diagnose mobile crashes)
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .build(),
            )?;

            // Initialize database
            let app_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data directory");
            std::fs::create_dir_all(&app_dir)
                .expect("failed to create app data directory");

            // Initialize diagnostic file logger + panic hook (for iOS debugging)
            diag::init(&app_dir);
            diag::install_panic_hook();
            diag::log("app setup started");

            let db_path = app_dir.join("alexandria.db");
            log::info!("Database path: {}", db_path.display());

            let database =
                Database::open(&db_path).expect("failed to open database");
            database
                .run_migrations()
                .expect("failed to run database migrations");

            log::info!("Database initialized successfully");

            let db = Arc::new(std::sync::Mutex::new(database));

            // Vault directory (Stronghold on desktop, AES-GCM portable vault on mobile)
            #[cfg(desktop)]
            let vault_dir = app_dir.join("stronghold");
            #[cfg(mobile)]
            let vault_dir = app_dir.join("vault");
            std::fs::create_dir_all(&vault_dir)
                .expect("failed to create vault directory");
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

            #[cfg(desktop)]
            let tutoring = Arc::new(TutoringManager::new());

            let app_state = AppState {
                db,
                keystore: Arc::new(Mutex::new(None)),
                vault_dir,
                content_node,
                resolver,
                p2p_node: Arc::new(Mutex::new(None)),
                #[cfg(desktop)]
                tutoring,
            };

            // Clean up any sessions stuck as 'active' from a previous crash
            #[cfg(desktop)]
            {
                let db = app_state.db.lock().unwrap();
                match db.conn().execute(
                    "UPDATE tutoring_sessions SET status = 'ended', ended_at = datetime('now') WHERE status = 'active'",
                    [],
                ) {
                    Ok(count) if count > 0 => {
                        log::info!("tutoring: cleaned up {count} orphaned session(s) from previous run");
                    }
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!("tutoring: failed to clean up orphaned sessions: {e}");
                    }
                }
            }

            app.manage(app_state);

            // iOS: disable automatic scroll view content inset adjustment so the
            // webview truly renders edge-to-edge.  Without this, WKWebView's
            // UIScrollView adds content insets for the safe area (status bar,
            // home indicator) which creates a visible gap at the bottom even
            // though the webview frame itself covers the full screen.
            // CSS `env(safe-area-inset-*)` handles the insets instead.
            #[cfg(target_os = "ios")]
            {
                if let Some(wv) = app.get_webview_window("main") {
                    wv.with_webview(|platform_wv| {
                        use objc2::rc::Retained;
                        use objc2::runtime::AnyObject;

                        let wk_webview = platform_wv.inner();

                        // Safety: wk_webview is a valid WKWebView pointer from WRY.
                        // WKWebView responds to `scrollView` (inherited from UIView
                        // category added by WebKit).
                        unsafe {
                            let wk: &AnyObject = &*(wk_webview as *const AnyObject);

                            // UIScrollView *scrollView = [wkWebView scrollView];
                            let scroll_view: Retained<AnyObject> =
                                objc2::msg_send![wk, scrollView];

                            // UIScrollViewContentInsetAdjustmentNever = 2
                            let never: isize = 2;
                            let _: () = objc2::msg_send![
                                &*scroll_view,
                                setContentInsetAdjustmentBehavior: never
                            ];
                        }

                        log::info!("iOS: set scrollView.contentInsetAdjustmentBehavior = .never");
                    })
                    .unwrap_or_else(|e| {
                        log::warn!("iOS: failed to configure webview scroll insets: {e}");
                    });
                }
            }

            Ok(())
        })
        .plugin(tauri_plugin_biometry::init())
        .invoke_handler(tauri::generate_handler![
            commands::health::check_health,
            commands::health::read_diag_log,
            // Identity / Wallet (all platforms — portable keystore)
            commands::identity::check_vault_exists,
            commands::identity::unlock_vault,
            commands::identity::generate_wallet,
            commands::identity::restore_wallet,
            commands::identity::export_mnemonic,
            commands::identity::lock_vault,
            commands::identity::get_wallet_info,
            commands::identity::get_profile,
            commands::identity::update_profile,
            commands::identity::publish_profile,
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
            // Content (iroh blob store)
            commands::content::content_add,
            commands::content::content_get,
            commands::content::content_has,
            commands::content::content_node_status,
            commands::content::content_resolve,
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
            // Course publishing (iroh)
            commands::courses::publish_course,
            commands::courses::fetch_course_document,
            // Evidence
            commands::evidence::list_skill_proofs,
            commands::evidence::list_evidence,
            commands::evidence::list_reputation,
            // Cardano (all platforms — uses portable keystore)
            commands::cardano::mint_skill_proof_nft,
            commands::cardano::register_course_onchain,
            // P2P (libp2p swarm)
            commands::p2p::p2p_start,
            commands::p2p::p2p_stop,
            commands::p2p::p2p_status,
            commands::p2p::p2p_peers,
            commands::p2p::p2p_publish,
            // Catalog
            commands::catalog::search_catalog,
            commands::catalog::get_catalog_entry,
            commands::catalog::bootstrap_public_catalog,
            commands::catalog::hydrate_catalog_courses,
            // Governance
            commands::governance::list_daos,
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
            commands::taxonomy::bootstrap_public_taxonomy,
            commands::taxonomy::list_subject_fields,
            commands::taxonomy::list_subjects,
            commands::taxonomy::list_skills,
            commands::taxonomy::get_skill,
            commands::taxonomy::list_skill_graph_edges,
            commands::taxonomy::tag_element_skill,
            commands::taxonomy::untag_element_skill,
            commands::taxonomy::list_element_skill_tags,
            // Sync
            commands::sync::sync_get_device_info,
            commands::sync::sync_set_device_name,
            commands::sync::sync_list_devices,
            commands::sync::sync_remove_device,
            commands::sync::sync_status,
            commands::sync::sync_now,
            commands::sync::sync_set_auto,
            commands::sync::sync_history,
            // Challenges (all platforms — pure DB operations)
            commands::challenge::submit_evidence_challenge,
            commands::challenge::list_challenges,
            commands::challenge::get_challenge,
            commands::challenge::vote_on_challenge,
            commands::challenge::resolve_challenge,
            commands::challenge::list_my_challenges,
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
            // Live Tutoring (iroh-live rooms)
            commands::tutoring::tutoring_create_room,
            commands::tutoring::tutoring_join_room,
            commands::tutoring::tutoring_leave_room,
            commands::tutoring::tutoring_toggle_video,
            commands::tutoring::tutoring_toggle_audio,
            commands::tutoring::tutoring_toggle_screen_share,
            commands::tutoring::tutoring_send_chat,
            commands::tutoring::tutoring_status,
            commands::tutoring::tutoring_peers,
            commands::tutoring::tutoring_list_sessions,
            commands::tutoring::tutoring_check_devices,
            commands::tutoring::tutoring_list_devices,
            commands::tutoring::tutoring_get_audio_level,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
