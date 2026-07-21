pub mod aggregation;
pub mod assessment;
pub mod cardano;
pub mod classroom;
pub mod commands;
pub mod crypto;
pub mod db;
pub mod diag;
pub mod domain;
pub mod evidence;
pub mod goals;
pub mod ipfs;
pub mod p2p;
pub mod plugins;
pub mod profile;
pub mod sentinel;
pub mod settings;
pub mod tutoring;

#[cfg(target_os = "macos")]
mod macos_media_delegate;

#[cfg(target_os = "macos")]
mod macos_secure_input;

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
use profile::{ProfileId, ProfileManager, ProfilePaths};
use tutoring::TutoringManager;

/// Metadata for the currently-unlocked profile. Held inside
/// [`AppState::active`]; `None` while the picker / unlock screen is
/// showing.
#[derive(Debug, Clone)]
pub struct ActiveProfile {
    pub id: ProfileId,
    pub paths: ProfilePaths,
}

/// Shared application state accessible from all Tauri commands.
///
/// State is split into two tiers:
///
/// * **Per-device singletons** — initialized once at app launch and
///   shared across every profile (`ProfileManager`, `TutoringManager`,
///   `ClassroomManager`, `last_activity`, `ipc_limiter`, grader runtime).
/// * **Per-profile resources** — created when a profile is unlocked
///   via [`AppState::start_active_profile`] and torn down on
///   [`AppState::stop_active_profile`]. The `Option`-wrapped fields
///   below (`db`, `keystore`, `content_node`, `resolver`, `p2p_node`)
///   are populated only while a profile is active.
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
    // ─── per-device singletons ──────────────────────────────────────
    pub app_data_dir: PathBuf,
    pub profile_manager: Arc<ProfileManager>,
    pub active: Arc<std::sync::RwLock<Option<ActiveProfile>>>,
    pub tutoring: Arc<TutoringManager>,
    pub classroom: Arc<ClassroomManager>,
    /// Deterministic Wasmtime grader runtime for community plugins (Phase 2).
    /// Cheap to clone; the underlying engine and module cache are shared.
    /// Desktop-only — wasmtime v27 does not support iOS / Android; mobile
    /// builds do not carry the runtime and the corresponding IPC command
    /// returns `GraderUnavailable` on mobile.
    #[cfg(desktop)]
    pub grader_runtime: Arc<plugins::wasm_runtime::GraderRuntime>,
    /// Last IPC activity timestamp for session timeout (auto-lock).
    pub last_activity: Arc<std::sync::Mutex<std::time::Instant>>,
    /// IPC rate limiter for sensitive commands.
    pub ipc_limiter: Arc<std::sync::Mutex<crate::commands::ratelimit::IpcRateLimiter>>,

    // ─── per-profile resources (populated while active) ─────────────
    pub db: Arc<std::sync::Mutex<Option<Database>>>,
    pub keystore: Arc<Mutex<Option<Keystore>>>,
    /// Singleton iroh node — repointed at the active profile's blob
    /// directory on each unlock. Never replaced, so existing call sites
    /// that take `&state.content_node` keep working unchanged.
    pub content_node: Arc<ContentNode>,
    pub resolver: Arc<Mutex<Option<ContentResolver>>>,
    /// iroh content-provider discovery, shared with the resolver so cache-misses
    /// are served from peers (pinners) over iroh before the IPFS gateway.
    pub discovery: Arc<ipfs::discovery::ContentDiscovery>,
    pub p2p_node: Arc<Mutex<Option<P2pNode>>>,
}

impl AppState {
    // ─── active-profile accessors ───────────────────────────────────

    /// Identifier of the currently-active profile, if any.
    pub fn active_id(&self) -> Option<ProfileId> {
        self.active
            .read()
            .expect("active profile lock poisoned")
            .as_ref()
            .map(|a| a.id.clone())
    }

    /// Path bundle for the active profile, or `Err` when none is unlocked.
    pub fn active_paths(&self) -> Result<ProfilePaths, String> {
        self.active
            .read()
            .map_err(|e| e.to_string())?
            .as_ref()
            .map(|a| a.paths.clone())
            .ok_or_else(|| "no active profile".to_string())
    }

    pub fn vault_dir(&self) -> Result<PathBuf, String> {
        self.active_paths().map(|p| p.vault_dir)
    }

    pub fn db_path(&self) -> Result<PathBuf, String> {
        self.active_paths().map(|p| p.db_path)
    }

    pub fn plugins_dir(&self) -> Result<PathBuf, String> {
        self.active_paths().map(|p| p.plugins_dir)
    }

    pub fn video_cache_dir(&self) -> Result<PathBuf, String> {
        self.active_paths().map(|p| p.video_cache_dir)
    }

    pub fn iroh_dir(&self) -> Result<PathBuf, String> {
        self.active_paths().map(|p| p.iroh_dir)
    }

    /// Convenience accessor used by callers that want to fail fast when
    /// no profile is active. The singleton `content_node` itself is
    /// always present; this asserts that it has been wired up to a
    /// concrete profile's blob directory and started.
    pub async fn content_node_required(&self) -> Result<Arc<ContentNode>, String> {
        if self.active_id().is_none() {
            return Err("no active profile".to_string());
        }
        Ok(self.content_node.clone())
    }

    // ─── per-profile lifecycle ──────────────────────────────────────

    /// Bring a freshly-unlocked profile online: open its SQLCipher
    /// database, start its iroh node, install builtin plugins, and
    /// register the keystore. Idempotent if the profile is already
    /// active.
    pub async fn start_active_profile(
        &self,
        paths: ProfilePaths,
        keystore: Keystore,
    ) -> Result<(), String> {
        // Refuse to switch into a profile while another is active —
        // callers must stop the prior one first.
        {
            let guard = self.active.read().map_err(|e| e.to_string())?;
            if let Some(current) = guard.as_ref() {
                if current.id == paths.id {
                    log::debug!("profile {} already active", paths.id);
                } else {
                    return Err(format!(
                        "profile {} is already active — lock it first",
                        current.id
                    ));
                }
            }
        }

        // 1. Open the encrypted DB and run migrations.
        let db_key = keystore.derive_db_key();
        self.open_database(&paths, &db_key)?;

        // 2. Stash keystore in shared state so background workers see it.
        {
            let mut guard = self.keystore.lock().await;
            *guard = Some(keystore);
        }

        // 3. Repoint and start the singleton iroh content node.
        self.content_node.set_data_dir(paths.iroh_dir.clone()).await;
        let (node_enc_key, content_key) = {
            let ks = self.keystore.lock().await;
            let k = ks.as_ref().ok_or("keystore vanished mid-start")?;
            (k.derive_node_key(), k.derive_content_key())
        };
        self.content_node.set_content_key(content_key).await;
        self.content_node
            .start(Some(&node_enc_key))
            .await
            .map_err(|e| format!("failed to start iroh content node: {e}"))?;
        log::info!("iroh content node started for profile {}", paths.id);

        // 4. Start iroh content discovery (gossip ingest) so cache-misses can be
        //    served from peers, then build the resolver wired to it (peer fetch
        //    first, IPFS gateway as last resort).
        if let Some(gossip) = self.content_node.gossip().await {
            if let Err(e) = self.discovery.start(&gossip, vec![]).await {
                log::warn!("content discovery gossip start failed: {e}");
            }
        }
        match GatewayClient::with_defaults() {
            Ok(gateway) => {
                let r = ContentResolver::with_discovery(
                    self.content_node.clone(),
                    gateway,
                    self.db.clone(),
                    self.discovery.clone(),
                );
                *self.resolver.lock().await = Some(r);
            }
            Err(e) => log::error!("failed to create gateway client: {e}"),
        }

        // 5. Best-effort startup eviction.
        let result = ipfs::storage::maybe_evict(&self.content_node, &self.db).await;
        if result.blobs_evicted > 0 {
            log::info!(
                "startup eviction: freed {} bytes from {} blobs",
                result.bytes_freed,
                result.blobs_evicted
            );
        }

        // 6. Backfill pins for older DBs.
        if let Ok(guard) = self.db.lock() {
            if let Some(db) = guard.as_ref() {
                ipfs::storage::backfill_pins(db.conn());
            }
        }

        // 7. Seed iroh content blobs (demo video/pdf media) in the background on
        // every platform. Downloads the public seed-asset URLs once into iroh and
        // fills in each element's content_cid so the demo videos actually play.
        // Best-effort + non-blocking — a network-less first launch just leaves the
        // media unresolved until a later boot with connectivity.
        {
            let db_handle = Arc::clone(&self.db);
            let node_handle = self.content_node.clone();
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

        // 8. Publish active profile metadata.
        {
            let mut guard = self.active.write().map_err(|e| e.to_string())?;
            *guard = Some(ActiveProfile {
                id: paths.id.clone(),
                paths: paths.clone(),
            });
        }

        log::info!("profile {} fully initialized", paths.id);
        Ok(())
    }

    /// Tear down the active profile's resources. Safe to call when no
    /// profile is active (becomes a no-op).
    pub async fn stop_active_profile(&self) -> Result<(), String> {
        // 1. Stop p2p first so its sync workers do not race the DB close.
        {
            let mut guard = self.p2p_node.lock().await;
            if let Some(mut node) = guard.take() {
                node.shutdown().await;
            }
        }

        // 2. Stop iroh + clear its content key. The singleton instance
        // is preserved; `set_data_dir` repoints it on the next unlock.
        if self.content_node.is_running().await {
            if let Err(e) = self.content_node.shutdown().await {
                log::warn!("iroh shutdown error (continuing): {e}");
            }
        }
        self.content_node.clear_content_key().await;

        // 3. Drop resolver (holds Arc<ContentNode>).
        {
            let mut guard = self.resolver.lock().await;
            *guard = None;
        }

        // 4. Lock keystore — zeroizes the in-memory password.
        {
            let mut guard = self.keystore.lock().await;
            if let Some(ks) = guard.take() {
                if let Err(e) = ks.lock() {
                    log::warn!("keystore lock error: {e}");
                }
            }
        }

        // 5. Close DB.
        {
            let mut guard = self.db.lock().map_err(|e| e.to_string())?;
            *guard = None;
        }

        // 6. Clear active profile metadata last.
        {
            let mut guard = self.active.write().map_err(|e| e.to_string())?;
            *guard = None;
        }

        log::info!("active profile stopped");
        Ok(())
    }

    /// Open the encrypted database for the given profile.
    ///
    /// Runs migrations, installs builtin plugins, and seeds dev fixtures.
    fn open_database(&self, paths: &ProfilePaths, db_key: &[u8; 32]) -> Result<(), String> {
        {
            let guard = self.db.lock().map_err(|e| e.to_string())?;
            if guard.is_some() {
                return Ok(());
            }
        }

        // Pre-launch: no legacy plaintext DBs exist in the wild. SQLCipher is the
        // only supported on-disk format. If we ever see a plaintext file, refuse
        // to touch it — silent deletion would lose data, silent open as encrypted
        // would corrupt the keystore mapping. Surface the error and let the user
        // (or `alex db reset`) deal with it explicitly.
        if Database::is_plaintext(&paths.db_path) {
            return Err(format!(
                "refusing to open: {} is an unencrypted SQLite database. \
                 This build only supports SQLCipher-encrypted profiles. \
                 Move or delete the file and run onboarding again.",
                paths.db_path.display()
            ));
        }

        let database = Database::open_encrypted(&paths.db_path, db_key)
            .map_err(|e| format!("failed to open encrypted database: {e}"))?;
        database
            .run_migrations()
            .map_err(|e| format!("database migrations failed: {e}"))?;

        // Seed `stake_pubkey_registry` from the bundled
        // bootstrap snapshot. No-op when the placeholder file is still
        // empty (pre-launch). See `docs/stake-pubkey-registry.md`.
        match crate::p2p::registry::load_embedded_bootstrap(database.conn()) {
            Ok(n) if n > 0 => log::info!("stake-pubkey registry: seeded {n} rows from bootstrap"),
            Ok(_) => {}
            Err(e) => log::warn!("stake-pubkey registry: bootstrap seed failed: {e}"),
        }

        // Seed the skill taxonomy, goal templates, and browsable demo courses
        // on every platform (mobile/release included) — these are product data
        // the goals + skill-graph features need, not dev-only fixtures. The
        // heavy iroh content-blob seeding stays behind `dev-seed` above. Fresh
        // profiles are NOT auto-enrolled (see BACKFILL_SQL) and course plugins
        // install through the enrollment pre-flight (see builtins::install_all).
        if let Err(e) = crate::db::seed::seed_if_empty(database.conn()) {
            log::warn!("seed failed (non-fatal): {e}");
        }

        {
            let mut guard = self.db.lock().map_err(|e| e.to_string())?;
            *guard = Some(database);
        }
        log::info!("encrypted database opened for profile {}", paths.id);

        {
            let guard = self.db.lock().map_err(|e| e.to_string())?;
            if let Some(db) = guard.as_ref() {
                let stats = plugins::builtins::install_all(db, &paths.plugins_dir);
                log::info!(
                    "builtin plugins: installed={} failed={}",
                    stats.installed,
                    stats.failed
                );

                // Demo course exercising both first-party plugins.
                // Idempotent and silent if the builtins haven't landed yet
                // (e.g. a corrupt embedded bundle); see seed_plugin_demo.
                if let Err(e) = crate::db::seed_plugin_demo::seed_plugin_demo_course(db.conn()) {
                    log::warn!("plugin demo course seed failed: {e}");
                }

                // Clean up any sessions stuck as 'active' from a previous crash.
                match db.conn().execute(
                    "UPDATE tutoring_sessions SET status = 'ended', ended_at = datetime('now') WHERE status = 'active'",
                    [],
                ) {
                    Ok(count) if count > 0 => {
                        log::info!("tutoring: cleaned up {count} orphaned session(s) from previous run");
                    }
                    Err(e) => log::warn!("tutoring: failed to clean up orphaned sessions: {e}"),
                    _ => {}
                }
                if let Err(e) = db.conn().execute(
                    "UPDATE classroom_calls SET status = 'ended', ended_at = datetime('now') WHERE status = 'active'",
                    [],
                ) {
                    log::warn!("classroom: failed to clean up orphaned calls: {e}");
                }
            }
        }

        Ok(())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Load a local `.env` if present (dev convenience) so config like
    // BLOCKFROST_PROJECT_ID is picked up by `resolve_project_id`'s env
    // fallback. No-op in a packaged build with no `.env`.
    let _ = dotenvy::dotenv();

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

    let builder = tauri::Builder::default().on_window_event(|window, event| {
        // Native app-focus signal for Sentinel: when the assessment
        // window loses focus, report which OS app took the
        // foreground (webview can't see this). Emitted to the
        // frontend, which folds it into the integrity snapshot.
        if let tauri::WindowEvent::Focused(focused) = event {
            use tauri::Emitter;
            let app = if *focused {
                None
            } else {
                crate::sentinel::active_app::frontmost_app()
            };
            let _ = window.emit(
                "sentinel://focus",
                serde_json::json!({ "focused": *focused, "app": app }),
            );
        }
    });

    // Native menus (and their events) exist only on desktop — tauri has
    // no `menu` module on iOS. Gate the whole block so the mobile build
    // keeps a valid builder type.
    #[cfg(desktop)]
    let builder = builder
        // Single-instance MUST be the first plugin registered. With the
        // `deep-link` feature it re-emits an OnOpenUrl when a second launch
        // carries an `alexandria://` URL (how Windows/Linux deliver deep
        // links); here we just surface the already-running window.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            use tauri::Manager;
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .menu(|handle| {
            // Standard menus plus a Developer submenu available in ALL builds:
            // "Reload Webviews" (reload without restarting), "Open DevTools"
            // (web inspector), and "Sentinel Live View" (toggle the debug PiP).
            let menu = tauri::menu::Menu::default(handle)?;
            let reload =
                tauri::menu::MenuItemBuilder::with_id("reload_webviews", "Reload Webviews")
                    .accelerator("CmdOrCtrl+Shift+R")
                    .build(handle)?;
            let devtools = tauri::menu::MenuItemBuilder::with_id("open_devtools", "Open DevTools")
                .accelerator("CmdOrCtrl+Alt+I")
                .build(handle)?;
            let pip =
                tauri::menu::MenuItemBuilder::with_id("toggle_sentinel_pip", "Sentinel Live View")
                    .accelerator("CmdOrCtrl+Shift+S")
                    .build(handle)?;
            let develop = tauri::menu::SubmenuBuilder::new(handle, "Developer")
                .item(&reload)
                .item(&devtools)
                .item(&pip)
                .build()?;
            menu.append(&develop)?;
            Ok(menu)
        })
        .on_menu_event(|app, event| {
            use tauri::{Emitter, Manager};
            match event.id().0.as_str() {
                "reload_webviews" => {
                    for (_, w) in app.webview_windows() {
                        let _ = w.eval("window.location.reload()");
                    }
                }
                "open_devtools" => {
                    for (_, w) in app.webview_windows() {
                        w.open_devtools();
                    }
                }
                "toggle_sentinel_pip" => {
                    let _ = app.emit("develop://toggle-sentinel", ());
                }
                _ => {}
            }
        });

    builder
        .setup(|app| {
            // Initialize logging (always enabled so we can diagnose mobile crashes)
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .build(),
            )?;

            // Deep links (`alexandria://…` + https app-links) are consumed on
            // the frontend via the plugin's JS `onOpenUrl`/`getCurrent` API,
            // which parses + routes them and queues until a profile unlocks
            // (see `src/deeplink/`). In dev the app isn't installed, so the OS
            // has no scheme association — register it at runtime so
            // `open alexandria://…` reaches this instance. Release installs
            // register the scheme via the app bundle.
            #[cfg(all(desktop, debug_assertions))]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let _ = app.deep_link().register_all();
            }

            // Initialize app-data dir + diagnostic logger.
            //
            // `ALEXANDRIA_DATA_DIR` overrides the OS app-data location. This
            // lets several instances run side by side on one host with fully
            // isolated vaults / DBs / P2P identities — used for local
            // cross-device testing (e.g. a "child" and a "parent" instance
            // that pair over LAN mDNS). Desktop-only; ignored on mobile.
            let app_dir = match std::env::var_os("ALEXANDRIA_DATA_DIR") {
                Some(dir) if !dir.is_empty() => std::path::PathBuf::from(dir),
                _ => app
                    .path()
                    .app_data_dir()
                    .expect("failed to resolve app data directory"),
            };
            std::fs::create_dir_all(&app_dir)
                .expect("failed to create app data directory");

            diag::init(&app_dir);
            diag::install_panic_hook();
            diag::log("app setup started");
            diag::log(&format!("app_dir={}", app_dir.display()));

            // Migrate any legacy single-vault layout into the new
            // per-profile layout. Runs at most once; no-op if a
            // profiles/ dir already exists.
            let profile_manager = Arc::new(
                profile::ProfileManager::open(&app_dir)
                    .expect("failed to open profile manager"),
            );
            let legacy = profile::migration::LegacyLayout::at(&app_dir);
            match profile::migration::migrate_if_needed(&profile_manager, &legacy) {
                Ok(profile::migration::MigrationReport::Migrated { id, .. }) => {
                    log::info!("migrated legacy single-vault layout into profile {id}");
                    diag::log(&format!("legacy migration: created profile {id}"));
                }
                Ok(profile::migration::MigrationReport::Failed { error, moved }) => {
                    log::error!("legacy migration failed: {error}");
                    diag::log(&format!("legacy migration FAILED: {error}"));
                    if let Err(e) = profile::migration::rollback(&moved) {
                        log::error!("legacy migration rollback also failed: {e}");
                    }
                }
                Ok(_) => {}
                Err(e) => log::error!("legacy migration error: {e}"),
            }

            // Per-profile resources start empty — populated when a
            // profile is unlocked via `start_active_profile`.
            let db: Arc<std::sync::Mutex<Option<Database>>> =
                Arc::new(std::sync::Mutex::new(None));
            let keystore: Arc<Mutex<Option<Keystore>>> = Arc::new(Mutex::new(None));
            // Singleton iroh node — initialized to a sentinel path inside
            // app_dir/iroh-staging. The real per-profile blob directory is
            // installed by `start_active_profile` before the first start.
            let content_node = Arc::new(ContentNode::new(&app_dir.join("iroh-staging")));
            let resolver: Arc<Mutex<Option<ContentResolver>>> =
                Arc::new(Mutex::new(None));
            let discovery = Arc::new(ipfs::discovery::ContentDiscovery::new());
            let p2p_node: Arc<Mutex<Option<P2pNode>>> = Arc::new(Mutex::new(None));
            let active: Arc<std::sync::RwLock<Option<ActiveProfile>>> =
                Arc::new(std::sync::RwLock::new(None));

            // Spawn on-chain queue processor (runs every 60s).
            // Processes both the governance tx queue and the credential
            // anchor queue. Both silently skip when BLOCKFROST_PROJECT_ID
            // is unset or the vault isn't unlocked yet.
            {
                let db_for_queue = db.clone();
                let ks_for_queue = keystore.clone();
                let node_for_sync = p2p_node.clone();
                diag::log("spawning on-chain queue processor (governance + credential anchors)");
                tauri::async_runtime::spawn(async move {
                    // Wait for app to fully initialize before processing
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;

                    loop {
                        // Try to create a Blockfrost client from the
                        // active profile's `cardano.blockfrost_project_id`
                        // setting (read fresh each tick so the queue
                        // picks up changes without restart). Falls
                        // back to the `BLOCKFROST_PROJECT_ID` env var.
                        let project_id = {
                            let guard = db_for_queue.lock().ok();
                            let conn = guard
                                .as_deref()
                                .and_then(|opt| opt.as_ref())
                                .map(|db| db.conn());
                            cardano::blockfrost::resolve_project_id(conn)
                        };
                        let bf = project_id
                            .and_then(|id| cardano::blockfrost::BlockfrostClient::new(id).ok());

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

                        // Username claim batch anchoring (registry phase 3):
                        // one metadata tx (label 1698) anchors up to 80
                        // unanchored claims. Silent no-op without chain creds.
                        match cardano::username_anchor::tick(&db_for_queue, &bf, &wallet).await {
                            Ok(anchored) if !anchored.is_empty() => {
                                log::info!(
                                    "username anchors: {} claims anchored",
                                    anchored.len()
                                );
                                // Republish enriched (tier 2) claims to the DHT.
                                let node_guard = node_for_sync.lock().await;
                                if let Some(node) = node_guard.as_ref() {
                                    for claim in anchored {
                                        if let Ok(payload) = serde_json::to_vec(&claim) {
                                            let key = crate::domain::username_claim::dht_key(
                                                &claim.username,
                                            );
                                            let _ = node.put_dht_record(key, payload).await;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                log::debug!("username anchors: {e}");
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

                        // Cross-device sync: when auto-sync is enabled,
                        // reconcile with every paired device. No-op if the
                        // toggle is off, no profile is unlocked, or the
                        // node isn't running.
                        let merged =
                            commands::sync::auto_sync_all(&db_for_queue, &node_for_sync).await;
                        if merged > 0 {
                            log::info!("auto-sync: merged {merged} row(s) from paired devices");
                        }

                        // Guardian oversight: wards push activity to
                        // their guardians, guardians pull from wards,
                        // and pending link exchanges retry. No-op
                        // without guardian links.
                        match commands::guardian::guardian_sync_all(&db_for_queue, &node_for_sync)
                            .await
                        {
                            Ok(rows) if rows > 0 => {
                                log::info!("guardian-sync: exchanged {rows} row(s)");
                            }
                            Ok(_) => {}
                            Err(e) => log::debug!("guardian-sync: {e}"),
                        }

                        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    }
                });
            }

            // Stake-address → pubkey registry refresh.
            //
            // Polls the on-chain `stake_pubkey_registration` script
            // address every `registry.refresh_secs` (default 3600) and
            // reconciles entries into the per-profile
            // `stake_pubkey_registry` table.
            //
            // Both the Blockfrost project id and the refresh
            // interval are resolved **fresh on every tick** via the
            // factory closures handed to `spawn_refresh_task`. That
            // means:
            //   - the refresh task starts immediately at app boot
            //     even if no profile has been unlocked yet;
            //   - once the operator unlocks a profile and sets
            //     `cardano.blockfrost_project_id`, the next tick
            //     picks it up without an app restart;
            //   - tuning `registry.refresh_secs` is live.
            // Previous shape returned `forever` at app start if the
            // env var was unset, permanently disabling the chain
            // refresh for the session.
            {
                let db_for_registry = db.clone();
                diag::log("spawning stake-pubkey registry refresh task");
                tauri::async_runtime::spawn(async move {
                    // Tiny startup-grace delay so this task doesn't
                    // race the profile bootstrap path for the DB
                    // mutex on first launch. After this, the inner
                    // loop's `BOOTSTRAP_REFRESH_SECS` cadence handles
                    // a still-locked profile — first useful tick
                    // fires within ~30 s of unlock.
                    tokio::time::sleep(std::time::Duration::from_secs(15)).await;

                    let db_for_resolve = db_for_registry.clone();
                    let fetcher_factory = move || -> Option<Arc<dyn p2p::registry_chain::ChainFetcher>> {
                        let project_id = {
                            let guard = db_for_resolve.lock().ok();
                            let conn = guard
                                .as_deref()
                                .and_then(|opt| opt.as_ref())
                                .map(|db| db.conn());
                            cardano::blockfrost::resolve_project_id(conn)
                        }?;
                        let bf = cardano::blockfrost::BlockfrostClient::new(project_id).ok()?;
                        Some(Arc::new(p2p::registry_chain::BlockfrostFetcher::new(
                            Arc::new(bf),
                            // Preprod for the launch window; revisit
                            // when mainnet config selection lands.
                            cardano::stake_pubkey::Network::Preprod,
                        )))
                    };

                    let db_for_interval = db_for_registry.clone();
                    let interval_factory = move || -> u64 {
                        let guard = db_for_interval.lock().ok();
                        let conn = guard
                            .as_deref()
                            .and_then(|opt| opt.as_ref())
                            .map(|db| db.conn());
                        match conn {
                            Some(c) => settings::store::SettingsStore::get(
                                c,
                                settings::registry::keys::REGISTRY_REFRESH_SECS,
                            ),
                            None => p2p::registry_chain::DEFAULT_REFRESH_SECS,
                        }
                    };

                    // Refresh task owns itself for the app lifetime; we
                    // don't hold the JoinHandle because the runtime
                    // tears it down on shutdown anyway. The interior
                    // loop in spawn_refresh_task swallows transient
                    // Blockfrost errors.
                    let _handle = p2p::registry_chain::spawn_refresh_task(
                        db_for_registry,
                        fetcher_factory,
                        interval_factory,
                    );
                });
            }

            diag::log("creating TutoringManager");
            let tutoring = Arc::new(TutoringManager::new());

            diag::log("creating ClassroomManager");
            let classroom = Arc::new(ClassroomManager::new());

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
                app_data_dir: app_dir.clone(),
                profile_manager,
                active,
                tutoring,
                classroom,
                #[cfg(desktop)]
                grader_runtime,
                last_activity: Arc::new(std::sync::Mutex::new(std::time::Instant::now())),
                ipc_limiter: Arc::new(std::sync::Mutex::new(
                    commands::ratelimit::IpcRateLimiter::new(),
                )),
                db,
                keystore,
                content_node,
                resolver,
                discovery,
                p2p_node,
            };

            // Orphaned tutoring/classroom session cleanup is now per-profile
            // and runs from `start_active_profile` once the DB is open.

            diag::log("managing app state in Tauri");
            app.manage(app_state);
            diag::log("app setup complete — webview should be loading");

            // macOS: WKWebView ships with WKPreferences' `fullScreenEnabled`
            // turned off by default, so HTML5 `Element.requestFullscreen()` is
            // a silent no-op (or throws). Flip the preference on at startup so
            // the video player can fullscreen the <video> wrapper directly,
            // without taking the whole app window into native fullscreen.
            #[cfg(target_os = "macos")]
            {
                if let Some(wv) = app.get_webview_window("main") {
                    wv.with_webview(|platform_wv| {
                        use objc2::class;
                        use objc2::rc::Retained;
                        use objc2::runtime::AnyObject;

                        let wk_webview = platform_wv.inner();
                        unsafe {
                            let wk: &AnyObject = &*(wk_webview as *const AnyObject);

                            // WKWebViewConfiguration *config = [wkWebView configuration];
                            let config: Retained<AnyObject> =
                                objc2::msg_send![wk, configuration];
                            // WKPreferences *prefs = [config preferences];
                            let prefs: Retained<AnyObject> =
                                objc2::msg_send![&*config, preferences];

                            let yes: Retained<AnyObject> = objc2::msg_send![
                                class!(NSNumber),
                                numberWithBool: true
                            ];
                            let no: Retained<AnyObject> = objc2::msg_send![
                                class!(NSNumber),
                                numberWithBool: false
                            ];

                            // Helper closure to set a WKPreferences private
                            // value-for-key entry. Most of the WebRTC + media
                            // capture flags live on WKPreferences' KVC surface.
                            let set_pref = |k: &core::ffi::CStr, val: &AnyObject| {
                                let key: Retained<AnyObject> = objc2::msg_send![
                                    class!(NSString),
                                    stringWithUTF8String: k.as_ptr()
                                ];
                                let _: () = objc2::msg_send![
                                    &*prefs,
                                    setValue: val,
                                    forKey: &*key
                                ];
                            };

                            set_pref(c"fullScreenEnabled", &yes);
                            // Enable getUserMedia + RTCPeerConnection so plugin
                            // iframes can capture audio/video locally for the
                            // Music Reviews / future camera-based plugins.
                            set_pref(c"mediaDevicesEnabled", &yes);
                            set_pref(c"peerConnectionEnabled", &yes);
                            set_pref(c"mediaStreamEnabled", &yes);
                            set_pref(c"mediaCaptureRequiresSecureConnection", &no);

                            // Install a UIDelegate that auto-grants
                            // media-capture requests. WKWebView denies by
                            // default when no UIDelegate implements
                            // `_webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:`,
                            // which blocks getUserMedia inside plugin iframes
                            // even though the plugin's own consent flow has
                            // already gone through PermissionPrompt.
                            crate::macos_media_delegate::install(wk);
                        }

                        log::info!(
                            "macOS: enabled WKPreferences fullScreen + media-capture + UIDelegate"
                        );
                    })
                    .unwrap_or_else(|e| {
                        log::warn!("macOS: failed to configure webview: {e}");
                    });
                }
            }

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
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        // Community plugin asset protocol — serves files out of
        // `app_data_dir/plugins/<cid>/` with a per-plugin CSP and the
        // alex bootstrap injected into HTML responses. See
        // `src/plugins/asset_protocol.rs` and
        // `/Users/hack/.claude/plans/prancy-bubbling-grove.md`.
        .register_uri_scheme_protocol("plugin", |ctx, request| {
            let plugins_dir = match ctx
                .app_handle()
                .state::<AppState>()
                .plugins_dir()
            {
                Ok(p) => p,
                Err(_) => {
                    // No profile unlocked — refuse the asset request so the
                    // webview falls back to its standard error page.
                    return tauri::http::Response::builder()
                        .status(tauri::http::StatusCode::NOT_FOUND)
                        .body(b"no active profile".to_vec())
                        .expect("static response is well-formed");
                }
            };
            plugins::asset_protocol::handle(&plugins_dir, request)
        })
        .invoke_handler(tauri::generate_handler![
            commands::health::check_health,
            commands::health::read_diag_log,
            commands::health::frontend_log,
            commands::health::release_secure_input,
            commands::updater::fetch_update_manifest,
            // App settings (per-profile, scope=sync|device)
            commands::settings::list_settings,
            commands::settings::set_setting,
            commands::settings::reset_setting,
            // Profile lifecycle (multi-user)
            commands::profile::list_profiles,
            commands::profile::get_active_profile_id,
            commands::profile::create_profile,
            commands::profile::restore_profile_with_mnemonic,
            commands::profile::unlock_profile,
            commands::profile::lock_profile,
            commands::profile::rename_profile,
            commands::profile::set_profile_avatar,
            commands::profile::delete_profile,
            // Identity / wallet (operate on active profile)
            commands::identity::export_mnemonic,
            commands::identity::is_biometric_available,
            commands::identity::get_wallet_info,
            commands::identity::get_local_did,
            commands::identity::get_account_status,
            commands::identity::set_account_role,
            commands::identity::resolve_display_names,
            commands::users::resolve_profiles,
            commands::users::fetch_user_profile,
            commands::username_registry::claim_username,
            commands::username_registry::check_username_availability,
            commands::username_registry::resolve_username,
            commands::username_registry::check_my_username_conflict,
            commands::username_registry::set_username,
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
            commands::enrollment::record_element_submission,
            commands::enrollment::get_element_submission,
            // Content (iroh blob store)
            commands::content::content_add,
            commands::content::content_get,
            commands::content::content_has,
            commands::content::content_node_status,
            commands::content::content_resolve,
            commands::content::content_resolve_bytes,
            commands::content::content_cache_file,
            // Chapters & Elements
            commands::chapters::list_chapters,
            commands::chapters::create_chapter,
            commands::chapters::update_chapter,
            commands::chapters::delete_chapter,
            commands::chapters::reorder_chapters,
            commands::elements::list_elements,
            commands::elements::create_element,
            commands::elements::update_element,
            commands::elements::delete_element,
            commands::elements::reorder_elements,
            commands::elements::move_element,
            commands::elements::set_video_chapters,
            commands::elements::list_video_chapters,
            // Guardian links (parental oversight)
            commands::guardian::guardian_create_invite,
            commands::guardian::guardian_accept_invite,
            commands::guardian::guardian_list_links,
            commands::guardian::guardian_sync_now,
            commands::guardian::guardian_revoke_link,
            commands::guardian::guardian_get_child_activity,
            // Instructor dashboard + inbox
            commands::instructor::instructor_overview,
            commands::instructor::instructor_course_learners,
            commands::instructor::instructor_inbox,
            // Course publishing (iroh)
            commands::courses::publish_course,
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
            commands::p2p::get_extra_relays,
            commands::p2p::save_extra_relays,
            // Catalog
            commands::catalog::search_catalog,
            commands::catalog::get_catalog_entry,
            commands::catalog::bootstrap_public_catalog,
            commands::catalog::hydrate_catalog_courses,
            // Governance
            commands::governance::list_daos,
            commands::governance::get_dao,
            commands::governance::create_dao,
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
            commands::reputation::get_reputation,
            commands::reputation::recompute_reputation_for_subject,
            // Skill graph + learning path
            commands::graph::get_my_skill_graph,
            commands::graph::fetch_public_graph,
            commands::graph::compute_learning_path,
            // Goal templates + goal resolution
            commands::goal_templates::list_goal_templates,
            commands::goal_templates::get_goal_template,
            commands::goal_templates::resolve_goal,
            // Skill-graph bootstrap from uploaded documents
            commands::skill_bootstrap::bootstrap_extract,
            commands::skill_bootstrap::bootstrap_confirm,
            commands::skill_bootstrap::bootstrap_extract_text,
            // Dynamic assessments
            commands::assessment::assessment_start_attempt,
            commands::assessment::assessment_grade,
            // Community-content DAO ratification (propose→publish→apply)
            commands::content_governance::propose_goal_template_change,
            commands::content_governance::publish_goal_template_ratification,
            commands::content_governance::propose_question_bank_change,
            commands::content_governance::publish_question_bank_ratification,
            commands::content_governance::apply_content_version,
            // Snapshots
            commands::snapshot::snapshot_reputation,
            commands::snapshot::submit_snapshot_tx,
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
            // Device pairing (bootstraps cross-device sync).
            commands::pairing::pairing_generate_code,
            commands::pairing::pairing_accept_code,
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
            commands::challenge::lock_challenge_stake,
            commands::challenge::settle_challenge_stake,
            // Completion-witness flow (Merkle root + tx submission).
            commands::completion::preview_completion_root,
            commands::completion::submit_completion_witness,
            commands::completion::get_course_completion_status,
            commands::completion::claim_course_completion,
            // Integrity
            commands::integrity::integrity_start_session,
            commands::integrity::integrity_submit_snapshot,
            commands::integrity::integrity_end_session,
            commands::integrity::integrity_get_session,
            commands::integrity::integrity_list_sessions,
            commands::integrity::integrity_list_snapshots,
            commands::integrity::integrity_record_attestation,
            commands::integrity::integrity_set_anchor,
            commands::integrity::integrity_get_assurance,
            commands::role_assessment::create_organization,
            commands::role_assessment::list_organizations,
            commands::role_assessment::create_role_assessment,
            commands::role_assessment::list_role_assessments,
            commands::role_assessment::get_role_assessment,
            commands::role_assessment::set_role_assessment_status,
            commands::role_assessment::issue_role_credential,
            // Sentinel DAO (adversarial-prior governance)
            commands::sentinel_dao::sentinel_dao_get_info,
            // Sentinel adversarial priors (propose / ratify / list / sync / load)
            commands::sentinel_priors::sentinel_propose_prior,
            commands::sentinel_priors::sentinel_ratify_prior,
            commands::sentinel_priors::sentinel_priors_list,
            commands::sentinel_priors::sentinel_priors_sync,
            commands::sentinel_priors::sentinel_priors_load,
            commands::sentinel_priors::sentinel_get_active_paste_classifier,
            commands::sentinel_priors::sentinel_set_kill_switch,
            commands::sentinel_priors::sentinel_get_kill_switch,
            commands::sentinel_priors::sentinel_blocklist_version,
            commands::sentinel_priors::sentinel_unblocklist_version,
            // Sentinel ML — paste classifier (tract) + per-user models (candle)
            commands::sentinel_ml::sentinel_score_paste,
            commands::sentinel_ml::sentinel_paste_classifier_info,
            commands::sentinel_ml::sentinel_load_dao_classifier,
            commands::sentinel_ml::sentinel_revert_classifier_to_bundled,
            commands::sentinel_ml::sentinel_train_keystroke_ae,
            commands::sentinel_ml::sentinel_score_keystroke_ae,
            commands::sentinel_ml::sentinel_extract_digraphs,
            commands::sentinel_ml::sentinel_train_mouse_cnn,
            commands::sentinel_ml::sentinel_score_mouse_cnn,
            commands::sentinel_ml::sentinel_user_models_status,
            commands::sentinel_ml::sentinel_reset_user_models,
            // Sentinel gaze / second-device detection
            commands::sentinel_gaze::sentinel_detect_face,
            commands::sentinel_gaze::sentinel_extract_gaze_features,
            commands::sentinel_gaze::sentinel_score_gaze,
            commands::sentinel_gaze::sentinel_train_gaze_calib,
            commands::sentinel_gaze::sentinel_frontmost_app,
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
            commands::plugins::plugin_list_dependencies,
            commands::plugins::plugin_save_element_state,
            commands::plugins::plugin_load_element_state,
            commands::plugins::plugin_grant_capability,
            commands::plugins::plugin_revoke_capability,
            commands::plugins::plugin_list_permissions,
            commands::plugins::plugin_set_enabled,
            commands::plugins::plugin_get_docs,
            commands::plugins::plugin_read_asset_data_url,
            // Enrollment-gated install: course-scoped plugins install on first
            // enrollment, with a per-plugin progress event.
            commands::plugins::course_required_plugins,
            commands::plugins::install_course_plugins,
            // IRL Review — local instructor inbox
            commands::plugins::irl_submit_for_review,
            commands::plugins::irl_list_my_submissions,
            commands::plugins::irl_list_pending,
            commands::plugins::irl_get_submission,
            commands::plugins::irl_post_review,
            // Phase 2 — submit-and-grade against deterministic WASM graders.
            // Desktop runs the real wasmtime grader; on mobile (wasmtime v27
            // lacks iOS / Android support) a stub returns a catchable
            // GraderUnavailable marker so the UI shows a clean message instead
            // of an "unknown command" failure.
            commands::plugins::plugin_submit_and_grade,
            // Phase 3 — P2P discovery + Plugin DAO attestation
            commands::plugins::plugin_browse_catalog,
            commands::plugins::plugin_attestation_status,
            commands::plugins::plugin_ingest_attestation,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
