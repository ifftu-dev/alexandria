pub mod aggregation;
pub mod cardano;
pub mod classroom;
pub mod commands;
#[cfg(desktop)]
pub mod crypto;
pub mod db;
pub mod diag;
pub mod domain;
pub mod evidence;
pub mod ipfs;
pub mod p2p;
pub mod plugins;
pub mod tutoring;

// Mobile crypto: same modules as desktop but with portable keystore
// (AES-256-GCM + Argon2id instead of IOTA Stronghold)
#[cfg(mobile)]
pub mod crypto {
    pub mod content_crypto;
    pub mod did;
    pub mod group_key;
    pub mod hash;
    #[path = "keystore_portable.rs"]
    pub mod keystore;
    pub mod shamir;
    pub mod signing;
    pub mod wallet;
}

use db::Database;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

use classroom::ClassroomManager;
use crypto::keystore::Keystore;
use ipfs::gateway::GatewayClient;
use ipfs::node::ContentNode;
use ipfs::resolver::ContentResolver;
use p2p::network::P2pNode;
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
    pub db: Arc<std::sync::Mutex<Option<Database>>>,
    pub db_path: PathBuf,
    pub keystore: Arc<Mutex<Option<Keystore>>>,
    pub vault_dir: PathBuf,
    /// Directory where installed plugin bundles live (`<app_data>/plugins/`).
    /// Each plugin is rooted at `plugins_dir/<plugin_cid>/`.
    pub plugins_dir: PathBuf,
    /// Deterministic Wasmtime grader runtime for community plugins (Phase 2).
    /// Cheap to clone; the underlying engine and module cache are shared.
    /// Desktop-only — wasmtime v27 does not support iOS / Android; mobile
    /// builds do not carry the runtime and the corresponding IPC command
    /// returns `GraderUnavailable` on mobile.
    #[cfg(desktop)]
    pub grader_runtime: Arc<plugins::wasm_runtime::GraderRuntime>,
    pub content_node: Arc<ContentNode>,
    pub resolver: Arc<Mutex<Option<ContentResolver>>>,
    pub p2p_node: Arc<Mutex<Option<P2pNode>>>,
    pub tutoring: Arc<TutoringManager>,
    pub classroom: Arc<ClassroomManager>,
    /// Last IPC activity timestamp for session timeout (auto-lock).
    pub last_activity: Arc<std::sync::Mutex<std::time::Instant>>,
    /// IPC rate limiter for sensitive commands.
    pub ipc_limiter: Arc<std::sync::Mutex<crate::commands::ratelimit::IpcRateLimiter>>,
}

impl AppState {
    /// Open the encrypted database using the vault-derived key.
    ///
    /// Handles legacy migration from unencrypted → encrypted on first unlock.
    /// Called during `unlock_vault` and `generate_wallet`.
    pub fn open_database(&self, db_key: &[u8; 32]) -> Result<(), String> {
        // Check if the DB is already open
        {
            let guard = self.db.lock().map_err(|e| e.to_string())?;
            if guard.is_some() {
                return Ok(());
            }
        }

        // Migrate legacy unencrypted DB if needed
        if Database::is_plaintext(&self.db_path) {
            log::info!("Migrating legacy unencrypted database to SQLCipher...");
            Database::migrate_to_encrypted(&self.db_path, db_key)
                .map_err(|e| format!("database migration failed: {e}"))?;
        }

        // Open the encrypted database
        let database = Database::open_encrypted(&self.db_path, db_key)
            .map_err(|e| format!("failed to open encrypted database: {e}"))?;
        database
            .run_migrations()
            .map_err(|e| format!("database migrations failed: {e}"))?;

        // Seed demo data into the encrypted DB if empty
        #[cfg(feature = "dev-seed")]
        {
            if let Err(e) = crate::db::seed::seed_if_empty(database.conn()) {
                log::warn!("seed failed (non-fatal): {e}");
            }
        }

        {
            let mut guard = self.db.lock().map_err(|e| e.to_string())?;
            *guard = Some(database);
        }
        log::info!("Encrypted database initialized successfully");

        // Install built-in plugins (Phase 2). Idempotent — same CID ≠
        // reinstall. Runs synchronously after migrations because both
        // the dispatcher and any pre-existing course_elements pointing
        // at builtin plugin CIDs need them to resolve from the very
        // first render.
        {
            let guard = self.db.lock().map_err(|e| e.to_string())?;
            if let Some(db) = guard.as_ref() {
                let stats = plugins::builtins::install_all(db, &self.plugins_dir);
                log::info!(
                    "builtin plugins: installed={} failed={}",
                    stats.installed,
                    stats.failed
                );
            }
        }

        // Seed iroh content blobs (videos, PDFs, downloadables) in the
        // background. This requires network IO and the iroh node to be
        // up, so we don't block wallet creation on it — the user can
        // explore the app while content fetches.
        #[cfg(feature = "dev-seed")]
        {
            let db_handle = Arc::clone(&self.db);
            let node_handle = Arc::clone(&self.content_node);
            tokio::spawn(async move {
                match crate::db::seed_content::seed_content_if_needed(&db_handle, &node_handle)
                    .await
                {
                    Ok(0) => {}
                    Ok(n) => log::info!("seeded iroh content for {n} elements"),
                    Err(e) => log::warn!("iroh content seed failed (non-fatal): {e}"),
                }
            });
        }

        Ok(())
    }

    /// Remove an orphaned encrypted database when onboarding starts without
    /// a vault, but stale SQLCipher files from an older vault are still present.
    ///
    /// This intentionally preserves legacy plaintext databases, since those can
    /// still be migrated with the newly created vault key.
    pub fn reset_orphaned_encrypted_database(&self) -> Result<bool, String> {
        if !self.db_path.exists() || Database::is_plaintext(&self.db_path) {
            return Ok(false);
        }

        {
            let mut guard = self.db.lock().map_err(|e| e.to_string())?;
            *guard = None;
        }

        let wal_path = PathBuf::from(format!("{}-wal", self.db_path.display()));
        let shm_path = PathBuf::from(format!("{}-shm", self.db_path.display()));

        for path in [&self.db_path, &wal_path, &shm_path] {
            if path.exists() {
                std::fs::remove_file(path).map_err(|e| {
                    format!(
                        "failed to remove stale database file {}: {e}",
                        path.display()
                    )
                })?;
            }
        }

        log::warn!(
            "removed orphaned encrypted database files because no vault existed during onboarding"
        );
        Ok(true)
    }

    /// Remove the local wallet files for this device, including the vault and
    /// the encrypted database, while leaving other app data in place.
    pub fn reset_local_wallet_files(&self) -> Result<(), String> {
        {
            let mut guard = self.db.lock().map_err(|e| e.to_string())?;
            *guard = None;
        }

        let wal_path = PathBuf::from(format!("{}-wal", self.db_path.display()));
        let shm_path = PathBuf::from(format!("{}-shm", self.db_path.display()));

        for path in [&self.db_path, &wal_path, &shm_path] {
            if path.exists() {
                std::fs::remove_file(path).map_err(|e| {
                    format!(
                        "failed to remove local database file {}: {e}",
                        path.display()
                    )
                })?;
            }
        }

        if self.vault_dir.exists() {
            std::fs::remove_dir_all(&self.vault_dir).map_err(|e| {
                format!(
                    "failed to remove local vault directory {}: {e}",
                    self.vault_dir.display()
                )
            })?;
        }

        std::fs::create_dir_all(&self.vault_dir).map_err(|e| {
            format!(
                "failed to recreate local vault directory {}: {e}",
                self.vault_dir.display()
            )
        })?;

        log::warn!(
            "removed local wallet files at {} and {}",
            self.vault_dir.display(),
            self.db_path.display()
        );

        Ok(())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Bridge tracing → log so that tracing events from iroh / iroh-live
    // are forwarded to tauri_plugin_log and become visible in the console.
    // We install a tracing Subscriber that converts tracing events into
    // log::log!() calls.  tauri_plugin_log then picks those up as normal
    // `log` crate records.
    use tracing_log::NormalizeEvent;
    struct TracingToLog;
    impl tracing::Subscriber for TracingToLog {
        fn enabled(&self, _meta: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _attrs: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            let normalized = event.normalized_metadata();
            let meta = normalized.as_ref().unwrap_or_else(|| event.metadata());
            let level = match *meta.level() {
                tracing::Level::ERROR => log::Level::Error,
                tracing::Level::WARN => log::Level::Warn,
                tracing::Level::INFO => log::Level::Info,
                tracing::Level::DEBUG => log::Level::Debug,
                tracing::Level::TRACE => log::Level::Trace,
            };
            // Format the event message + fields
            struct Visitor(String);
            impl tracing::field::Visit for Visitor {
                fn record_debug(
                    &mut self,
                    field: &tracing::field::Field,
                    value: &dyn std::fmt::Debug,
                ) {
                    if field.name() == "message" {
                        self.0.push_str(&format!("{:?}", value));
                    } else {
                        if !self.0.is_empty() {
                            self.0.push(' ');
                        }
                        self.0.push_str(&format!("{}={:?}", field.name(), value));
                    }
                }
            }
            let mut visitor = Visitor(String::new());
            event.record(&mut visitor);
            log::log!(target: meta.target(), level, "{}", visitor.0);

            // Also write iroh-live / moq-media diagnostic events to diag.log
            // so they appear in the in-app diagnostics modal alongside our own logs.
            let target = meta.target();
            if matches!(
                *meta.level(),
                tracing::Level::ERROR | tracing::Level::WARN | tracing::Level::INFO
            ) && (target.starts_with("moq_media")
                || target.starts_with("iroh_live")
                || target.starts_with("hang")
                || target.starts_with("moq_lite"))
            {
                crate::diag::log(&format!("[{target}] {}", visitor.0));
            }
        }
        fn enter(&self, _span: &tracing::span::Id) {}
        fn exit(&self, _span: &tracing::span::Id) {}
    }
    tracing::subscriber::set_global_default(TracingToLog).ok();

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
            diag::log(&format!("app_dir={}", app_dir.display()));

            let db_path = app_dir.join("alexandria.db");
            log::info!("Database path: {}", db_path.display());

            // Database open is deferred until vault unlock, since the
            // encryption key is derived from the vault password.
            // For new installs, the DB is created at unlock time.
            // For legacy unencrypted DBs, migration happens at unlock.
            let db: Arc<std::sync::Mutex<Option<Database>>> =
                Arc::new(std::sync::Mutex::new(None));

            log::info!("Database open deferred until vault unlock");

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
            diag::log("spawning iroh node startup task");
            tauri::async_runtime::spawn(async move {
                crate::diag::log("iroh startup: calling content_node.start()...");
                match content_node_clone.start(None).await {
                    Ok(()) => {
                        crate::diag::log("iroh startup: content_node started OK");
                        log::info!("iroh content node started successfully");

                        // Backfill pins table for users upgrading from older versions
                        if let Ok(guard) = db_clone.lock() {
                            if let Some(db) = guard.as_ref() {
                                ipfs::storage::backfill_pins(db.conn());
                            }
                        }

                        // Initialize the content resolver with gateway fallback
                        match GatewayClient::with_defaults() {
                            Ok(gateway) => {
                                let r = ContentResolver::new(
                                    content_node_clone.clone(),
                                    gateway,
                                    db_clone.clone(),
                                );
                                *resolver_clone.lock().await = Some(r);
                                log::info!("content resolver initialized with IPFS gateway fallback");
                            }
                            Err(e) => {
                                log::error!("failed to create gateway client: {e}");
                            }
                        }

                        // Run eviction at startup to catch incomplete evictions
                        let result = ipfs::storage::maybe_evict(
                            &content_node_clone,
                            &db_clone,
                        )
                        .await;
                        if result.blobs_evicted > 0 {
                            log::info!(
                                "startup eviction: freed {} bytes from {} blobs",
                                result.bytes_freed,
                                result.blobs_evicted
                            );
                        }
                    }
                    Err(e) => {
                        crate::diag::log(&format!("iroh startup: FAILED: {e}"));
                        log::error!("failed to start iroh content node: {e}");
                    }
                }
            });

            // Keystore arc — created early so both the queue processor
            // background task and AppState share the same handle. The
            // keystore starts as None; it's populated when the user
            // unlocks or generates a wallet.
            let keystore: Arc<Mutex<Option<Keystore>>> = Arc::new(Mutex::new(None));

            // Spawn on-chain queue processor (runs every 60s).
            // Processes both the governance tx queue and the credential
            // anchor queue. Both silently skip when BLOCKFROST_PROJECT_ID
            // is unset or the vault isn't unlocked yet.
            {
                let db_for_queue = db.clone();
                let ks_for_queue = keystore.clone();
                diag::log("spawning on-chain queue processor (governance + credential anchors)");
                tauri::async_runtime::spawn(async move {
                    // Wait for app to fully initialize before processing
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;

                    loop {
                        // Try to create a Blockfrost client from env
                        let bf = std::env::var("BLOCKFROST_PROJECT_ID")
                            .ok()
                            .and_then(|id| {
                                cardano::blockfrost::BlockfrostClient::new(id).ok()
                            });

                        // Derive wallet from the unlocked keystore. If the vault
                        // is still locked (keystore is None), wallet will be None
                        // and both queues skip silently.
                        let wallet: Option<crypto::wallet::Wallet> = {
                            let guard = ks_for_queue.lock().await;
                            guard.as_ref().and_then(|ks| {
                                ks.retrieve_mnemonic()
                                    .ok()
                                    .and_then(|m| crypto::wallet::wallet_from_mnemonic(&m).ok())
                            })
                        };

                        // Governance tx queue (elections, proposals, soulbound)
                        match cardano::onchain_queue::process_queue(&db_for_queue, &bf, &wallet).await
                        {
                            Ok(n) if n > 0 => {
                                log::info!("governance queue: processed {n} items");
                            }
                            Err(e) => {
                                log::debug!("governance queue: {e}");
                            }
                            _ => {}
                        }

                        // Credential anchor queue (VC integrity hashes → Cardano metadata-only txs)
                        match cardano::anchor_queue::tick(&db_for_queue, &bf, &wallet).await
                        {
                            Ok(n) if n > 0 => {
                                log::info!("anchor queue: processed {n} items");
                            }
                            Err(e) => {
                                log::debug!("anchor queue: {e}");
                            }
                            _ => {}
                        }

                        // Completion-witness observer + auto-issuance pipeline.
                        // Gated on ALEXANDRIA_COMPLETION_POLICY_ID + the vault
                        // being unlocked (wallet present). Silent no-op otherwise.
                        if let (Some(bf_client), Some(w)) = (bf.as_ref(), wallet.as_ref()) {
                            if let Ok(policy_id_raw) =
                                std::env::var("ALEXANDRIA_COMPLETION_POLICY_ID")
                            {
                                let policy_id = policy_id_raw.trim().to_string();
                                if !policy_id.is_empty() {
                                    // Observer: takes the shared DB handle
                                    // so locks stay short across awaits.
                                    match cardano::completion::tick(
                                        &db_for_queue,
                                        bf_client,
                                        &policy_id,
                                    )
                                    .await
                                    {
                                        Ok(n) if n > 0 => log::info!(
                                            "completion observer: ingested {n} new mint(s)"
                                        ),
                                        Err(e) => log::debug!("completion observer: {e}"),
                                        _ => {}
                                    }

                                    // Auto-issuance over the ingested rows.
                                    let issuance = {
                                        let guard = db_for_queue.lock();
                                        match guard.as_deref() {
                                            Ok(Some(db)) => Some(
                                                commands::auto_issuance::tick(
                                                    db.conn(),
                                                    &w.signing_key,
                                                ),
                                            ),
                                            _ => None,
                                        }
                                    };
                                    match issuance {
                                        Some(Ok(report)) if report.issued > 0 => log::info!(
                                            "auto-issuance: issued {} VC(s), {} waiting on attestation",
                                            report.issued,
                                            report.waiting_on_attestations,
                                        ),
                                        Some(Ok(report)) if !report.errors.is_empty() => {
                                            log::warn!(
                                                "auto-issuance: {} error(s): {:?}",
                                                report.errors.len(),
                                                report.errors
                                            );
                                        }
                                        Some(Err(e)) => log::debug!("auto-issuance: {e}"),
                                        _ => {}
                                    }
                                }
                            }
                        }

                        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    }
                });
            }

            diag::log("creating TutoringManager");
            let tutoring = Arc::new(TutoringManager::new());

            diag::log("creating ClassroomManager");
            let classroom = Arc::new(ClassroomManager::new());

            // Plugin bundle store. Created eagerly so install commands
            // don't race the first install.
            let plugins_dir = app_dir.join("plugins");
            std::fs::create_dir_all(&plugins_dir)
                .expect("failed to create plugins directory");
            log::info!("Plugins directory: {}", plugins_dir.display());

            // Deterministic Wasmtime engine for plugin graders (Phase 2).
            // Construction is cheap; the engine + module cache live for
            // the app's lifetime. Desktop-only because wasmtime v27 does
            // not support iOS / Android targets.
            #[cfg(desktop)]
            let grader_runtime = Arc::new(
                plugins::wasm_runtime::GraderRuntime::new()
                    .expect("failed to create grader runtime"),
            );

            let app_state = AppState {
                db,
                db_path,
                keystore,
                vault_dir,
                plugins_dir,
                #[cfg(desktop)]
                grader_runtime,
                content_node,
                resolver,
                p2p_node: Arc::new(Mutex::new(None)),
                tutoring,
                classroom,
                last_activity: Arc::new(std::sync::Mutex::new(std::time::Instant::now())),
                ipc_limiter: Arc::new(std::sync::Mutex::new(
                    commands::ratelimit::IpcRateLimiter::new(),
                )),
            };

            // Clean up any sessions stuck as 'active' from a previous crash.
            // NOTE: These run at startup, before vault unlock opens the DB.
            // They will be skipped if the DB is not yet initialized; the
            // cleanup will happen on the next vault unlock when the DB opens.
            diag::log("cleaning up orphaned tutoring sessions");
            {
                match app_state.db.lock() {
                    Ok(guard) => {
                        if let Some(db) = guard.as_ref() {
                            match db.conn().execute(
                                "UPDATE tutoring_sessions SET status = 'ended', ended_at = datetime('now') WHERE status = 'active'",
                                [],
                            ) {
                                Ok(count) if count > 0 => {
                                    log::info!("tutoring: cleaned up {count} orphaned session(s) from previous run");
                                    diag::log(&format!("cleaned up {count} orphaned session(s)"));
                                }
                                Ok(_) => {
                                    diag::log("no orphaned sessions to clean up");
                                }
                                Err(e) => {
                                    log::warn!("tutoring: failed to clean up orphaned sessions: {e}");
                                    diag::log(&format!("orphan cleanup error: {e}"));
                                }
                            }
                        } else {
                            diag::log("db not yet initialized — skipping orphan cleanup");
                        }
                    }
                    Err(e) => {
                        log::error!("tutoring: db mutex poisoned during orphan cleanup: {e}");
                        diag::log(&format!("CRITICAL: db mutex poisoned: {e}"));
                    }
                }
            }

            // Clean up classroom calls stuck as 'active' from a previous crash
            diag::log("cleaning up orphaned classroom calls");
            {
                match app_state.db.lock() {
                    Ok(guard) => {
                        if let Some(db) = guard.as_ref() {
                            let _ = db.conn().execute(
                                "UPDATE classroom_calls SET status = 'ended', ended_at = datetime('now') WHERE status = 'active'",
                                [],
                            );
                        }
                    }
                    Err(e) => {
                        log::warn!("classroom: failed to clean up orphaned calls: {e}");
                    }
                }
            }

            diag::log("managing app state in Tauri");
            app.manage(app_state);
            diag::log("app setup complete — webview should be loading");

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
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Community plugin asset protocol — serves files out of
        // `app_data_dir/plugins/<cid>/` with a per-plugin CSP and the
        // alex bootstrap injected into HTML responses. See
        // `src/plugins/asset_protocol.rs` and
        // `/Users/hack/.claude/plans/prancy-bubbling-grove.md`.
        .register_uri_scheme_protocol("plugin", |ctx, request| {
            let plugins_dir = ctx
                .app_handle()
                .state::<AppState>()
                .plugins_dir
                .clone();
            plugins::asset_protocol::handle(&plugins_dir, request)
        })
        .invoke_handler(tauri::generate_handler![
            commands::health::check_health,
            commands::health::read_diag_log,
            // Identity / Wallet (all platforms — portable keystore)
            commands::identity::check_vault_exists,
            commands::identity::unlock_vault,
            commands::identity::generate_wallet,
            commands::identity::restore_wallet,
            commands::identity::export_mnemonic,
            commands::identity::is_biometric_available,
            commands::identity::lock_vault,
            commands::identity::reset_local_wallet,
            commands::identity::get_wallet_info,
            commands::identity::get_local_did,
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
            commands::courses::publish_tutorial,
            commands::courses::fetch_course_document,
            // Opinions (Field Commentary)
            commands::opinions::publish_opinion,
            commands::opinions::list_opinions,
            commands::opinions::get_opinion,
            commands::opinions::list_my_opinions,
            commands::opinions::list_eligible_subject_fields_for_posting,
            commands::opinions::withdraw_own_opinion,
            // Reputation read surface (skill_proofs/evidence listings
            // retired in migration 040; use `list_credentials` instead).
            commands::evidence::list_reputation,
            // P2P (libp2p swarm)
            commands::p2p::p2p_start,
            commands::p2p::p2p_stop,
            commands::p2p::p2p_status,
            commands::p2p::p2p_peers,
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
            commands::governance::get_onchain_queue_status,
            commands::governance::retry_onchain_submission,
            // Reputation (VC-sourced engine — see commands::reputation).
            commands::reputation::list_reputation_rows,
            commands::reputation::recompute_reputation_for_subject,
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
            // Completion attestation (VC-first gate).
            commands::attestation::set_completion_attestation_requirement,
            commands::attestation::remove_completion_attestation_requirement,
            commands::attestation::list_completion_attestation_requirements,
            commands::attestation::submit_completion_attestation,
            commands::attestation::get_completion_attestation_status,
            // Credential challenges (VC-first rebuild).
            commands::challenge::submit_credential_challenge,
            commands::challenge::vote_on_credential_challenge,
            commands::challenge::resolve_credential_challenge,
            commands::challenge::list_credential_challenges,
            commands::challenge::get_credential_challenge,
            commands::challenge::expire_overdue_credential_challenges,
            // Completion-witness flow (Merkle root + tx submission).
            commands::completion::preview_completion_root,
            commands::completion::submit_completion_witness,
            // Integrity
            commands::integrity::integrity_start_session,
            commands::integrity::integrity_submit_snapshot,
            commands::integrity::integrity_end_session,
            commands::integrity::integrity_get_session,
            commands::integrity::integrity_list_sessions,
            commands::integrity::integrity_list_snapshots,
            // Sentinel DAO (adversarial-prior governance)
            commands::sentinel_dao::sentinel_dao_get_info,
            // Sentinel adversarial priors (propose / ratify / list / sync / load)
            commands::sentinel_priors::sentinel_propose_prior,
            commands::sentinel_priors::sentinel_ratify_prior,
            commands::sentinel_priors::sentinel_priors_list,
            commands::sentinel_priors::sentinel_priors_sync,
            commands::sentinel_priors::sentinel_priors_load,
            // Sentinel holdout evaluation (threshold-sealed)
            commands::sentinel_holdout::sentinel_holdout_upload,
            commands::sentinel_holdout::sentinel_holdout_list,
            commands::sentinel_holdout::sentinel_holdout_get_policy,
            commands::sentinel_holdout::sentinel_holdout_unseal_share,
            commands::sentinel_holdout::sentinel_holdout_evaluate,
            // Live Tutoring (iroh-live rooms)
            commands::tutoring::tutoring_create_room,
            commands::tutoring::tutoring_join_room,
            commands::tutoring::tutoring_leave_room,
            commands::tutoring::tutoring_toggle_video,
            commands::tutoring::tutoring_toggle_audio,
            commands::tutoring::tutoring_set_audio_devices,
            commands::tutoring::tutoring_toggle_screen_share,
            commands::tutoring::tutoring_send_chat,
            commands::tutoring::tutoring_status,
            commands::tutoring::tutoring_peers,
            commands::tutoring::tutoring_list_sessions,
            commands::tutoring::tutoring_check_devices,
            commands::tutoring::tutoring_list_devices,
            commands::tutoring::tutoring_get_audio_level,
            commands::tutoring::tutoring_diagnostics,
            // Classrooms
            commands::classroom::classroom_create,
            commands::classroom::classroom_list,
            commands::classroom::classroom_get,
            commands::classroom::classroom_archive,
            commands::classroom::classroom_list_members,
            commands::classroom::classroom_request_join,
            commands::classroom::classroom_approve_member,
            commands::classroom::classroom_deny_member,
            commands::classroom::classroom_leave,
            commands::classroom::classroom_kick_member,
            commands::classroom::classroom_set_role,
            commands::classroom::classroom_list_join_requests,
            commands::classroom::classroom_list_channels,
            commands::classroom::classroom_create_channel,
            commands::classroom::classroom_delete_channel,
            commands::classroom::classroom_get_messages,
            commands::classroom::classroom_send_message,
            commands::classroom::classroom_delete_message,
            commands::classroom::classroom_subscribe,
            commands::classroom::classroom_unsubscribe,
            commands::classroom::classroom_start_call,
            commands::classroom::classroom_join_call,
            commands::classroom::classroom_end_call,
            commands::classroom::classroom_get_active_call,
            // Storage management
            commands::storage::storage_get_quota,
            commands::storage::storage_set_quota,
            commands::storage::storage_stats,
            commands::storage::storage_evict_now,
            // Verifiable Credentials (VC-first migration, PRs 2–12)
            commands::credentials::issue_credential,
            commands::credentials::list_credentials,
            commands::credentials::get_credential,
            commands::credentials::revoke_credential,
            commands::credentials::suspend_credential,
            commands::credentials::reinstate_credential,
            commands::credentials::allow_credential_fetch,
            commands::credentials::disallow_credential_fetch,
            commands::credentials::verify_credential_cmd,
            commands::credentials::export_credentials_bundle,
            // Selective-disclosure presentations (§18)
            commands::presentation::create_presentation,
            commands::presentation::verify_presentation,
            // PinBoard (§12 + §20.4)
            commands::pinning::declare_pinboard_commitment,
            commands::pinning::revoke_pinboard_commitment,
            commands::pinning::list_my_commitments,
            commands::pinning::list_incoming_commitments,
            commands::pinning::get_quota_breakdown,
            // Derived skill state (§14 aggregation)
            commands::aggregation::get_derived_skill_state,
            commands::aggregation::list_derived_states,
            commands::aggregation::recompute_all,
            // Community plugin system (Phase 1 — local-file install,
            // iframe-sandboxed interactive plugins)
            commands::plugins::plugin_install_from_file,
            commands::plugins::plugin_uninstall,
            commands::plugins::plugin_list,
            commands::plugins::plugin_get_manifest,
            commands::plugins::plugin_grant_capability,
            commands::plugins::plugin_revoke_capability,
            commands::plugins::plugin_list_permissions,
            // Phase 2 — submit-and-grade against deterministic WASM graders.
            // Desktop-only: wasmtime v27 lacks iOS / Android support.
            #[cfg(desktop)]
            commands::plugins::plugin_submit_and_grade,
            // Phase 3 — P2P discovery + Plugin DAO attestation
            commands::plugins::plugin_browse_catalog,
            commands::plugins::plugin_attestation_status,
            commands::plugins::plugin_ingest_attestation,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
