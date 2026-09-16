use std::sync::atomic::Ordering;

use crate::profile::scope::ProfileState as State;
use alexandria_studio::{grants::StudioGrant, Error as StudioError};
use serde::Serialize;

use super::studio::with_db;
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct StudioAssistantAccess {
    pub available: bool,
    pub grants: Vec<StudioGrant>,
}

#[derive(Debug, Serialize)]
pub struct StudioAssistantGrant {
    pub grant: StudioGrant,
    pub connection_file: String,
}

#[tauri::command]
pub async fn studio_assistant_access(
    state: State<'_, AppState>,
) -> Result<StudioAssistantAccess, String> {
    with_db(&state, |_| {
        let grants = state
            .studio
            .grants
            .lock()
            .map_err(|_| StudioError::Unavailable("Assistant access unavailable".into()))?;
        let now = chrono::Utc::now().timestamp();
        #[cfg(all(desktop, unix))]
        if let Ok(dir) = alexandria_studio::broker::private_directory(&state.app_data_dir) {
            alexandria_studio::broker::prune_connections(&dir, &grants, now);
        }
        Ok(StudioAssistantAccess {
            available: state.studio.broker_available.load(Ordering::SeqCst),
            grants: grants.list(now),
        })
    })
}

#[tauri::command]
pub async fn studio_revoke_assistant(
    state: State<'_, AppState>,
    grant_id: String,
) -> Result<(), String> {
    let _gate = state.studio.broker_gate.write().await;
    with_db(&state, |_| {
        let mut grants = state
            .studio
            .grants
            .lock()
            .map_err(|_| StudioError::Unavailable("Assistant access unavailable".into()))?;
        #[cfg(all(desktop, unix))]
        if let Ok(dir) = alexandria_studio::broker::private_directory(&state.app_data_dir) {
            alexandria_studio::broker::revoke_connection(&dir, &mut grants, &grant_id);
            return Ok(());
        }
        grants.revoke(&grant_id);
        Ok(())
    })
}

#[tauri::command]
pub async fn studio_grant_assistant(
    state: State<'_, AppState>,
    client_name: String,
    scopes: Vec<String>,
) -> Result<StudioAssistantGrant, String> {
    #[cfg(all(desktop, unix))]
    {
        use alexandria_studio::broker;
        let _gate = state.studio.broker_gate.write().await;
        with_db(&state, |_| {
            if !state.studio.broker_available.load(Ordering::SeqCst) {
                return Err(StudioError::Unavailable(
                    "Assistant broker is not running".into(),
                ));
            }
            let dir =
                broker::private_directory(&state.app_data_dir).map_err(StudioError::Unavailable)?;
            let mut grants = state
                .studio
                .grants
                .lock()
                .map_err(|_| StudioError::Unavailable("Assistant access unavailable".into()))?;
            let (grant, file) = broker::issue_connection(
                &dir,
                &mut grants,
                client_name,
                scopes,
                state.studio.epoch.load(Ordering::SeqCst),
                chrono::Utc::now().timestamp(),
            )?;
            Ok(StudioAssistantGrant {
                grant,
                connection_file: file.to_string_lossy().into_owned(),
            })
        })
    }
    #[cfg(not(all(desktop, unix)))]
    {
        let _ = (state, client_name, scopes);
        Err("unavailable: local assistant access currently requires macOS or Linux".into())
    }
}

#[cfg(all(desktop, unix))]
struct AppBroker(tauri::AppHandle);

#[cfg(all(desktop, unix))]
impl AppBroker {
    fn state(&self) -> &AppState {
        use tauri::Manager;
        self.0.state::<AppState>().inner()
    }
}

#[cfg(all(desktop, unix))]
impl alexandria_studio::broker::BrokerHost for AppBroker {
    fn gate(&self) -> &tokio::sync::RwLock<()> {
        &self.state().studio.broker_gate
    }

    fn grants(&self) -> &std::sync::Mutex<alexandria_studio::grants::Grants> {
        &self.state().studio.grants
    }

    fn epoch(&self) -> u64 {
        self.state().studio.epoch.load(Ordering::SeqCst)
    }

    fn with_db(
        &self,
        f: &mut dyn FnMut(&rusqlite::Connection) -> alexandria_studio::Result<serde_json::Value>,
    ) -> Result<serde_json::Value, String> {
        with_db(self.state(), |db| f(db))
    }

    fn fetch_content(&self, blob: &str) -> alexandria_studio::broker::ContentFuture<'_> {
        let blob = blob.to_string();
        Box::pin(async move {
            let resolver = self
                .state()
                .resolver
                .lock()
                .await
                .as_ref()
                .cloned()
                .ok_or_else(|| "content resolver unavailable".to_string())?;
            let resolved = resolver
                .resolve(&blob)
                .await
                .map_err(|error| error.to_string())?;
            Ok(resolved.bytes.to_vec())
        })
    }
}

#[cfg(all(desktop, unix))]
pub fn start(app: tauri::AppHandle) {
    use alexandria_studio::broker;
    use tauri::Manager;
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let dir = match broker::private_directory(&state.app_data_dir) {
            Ok(dir) => dir,
            Err(error) => {
                log::warn!("MCP broker unavailable: {error}");
                return;
            }
        };
        // Grants live only in memory, so earlier runs' files and sockets are unusable.
        broker::sweep(&dir, &[], None);
        let listener = match tokio::net::UnixListener::bind(broker::socket_path(&dir)) {
            Ok(listener) => listener,
            Err(_) => {
                log::warn!("MCP broker socket could not be bound");
                return;
            }
        };
        state.studio.broker_available.store(true, Ordering::SeqCst);
        broker::serve(listener, std::sync::Arc::new(AppBroker(app.clone()))).await;
        state.studio.broker_available.store(false, Ordering::SeqCst);
    });
}

/// Removes every connection file after profile invalidation cleared the grants.
#[cfg(all(desktop, unix))]
pub fn remove_connection_files(app_data_dir: &std::path::Path) {
    use alexandria_studio::broker;
    if let Ok(dir) = broker::private_directory(app_data_dir) {
        broker::sweep(&dir, &[], Some(&broker::socket_path(&dir)));
    }
}
