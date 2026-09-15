use std::sync::atomic::Ordering;

use alexandria_studio::{grants::StudioGrant, Error as StudioError};
use serde::Serialize;
use tauri::State;

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
            .map_err(|_| StudioError::Unavailable("Assistant access unavailable".into()))?
            .list(chrono::Utc::now().timestamp());
        Ok(StudioAssistantAccess {
            available: state.studio.broker_available.load(Ordering::SeqCst),
            grants,
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
        state
            .studio
            .grants
            .lock()
            .map_err(|_| StudioError::Unavailable("Assistant access unavailable".into()))?
            .revoke(&grant_id);
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
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let _gate = state.studio.broker_gate.write().await;
        with_db(&state, |_| {
            if !state.studio.broker_available.load(Ordering::SeqCst) {
                return Err(StudioError::Unavailable(
                    "Assistant broker is not running".into(),
                ));
            }
            let dir = private_directory(&state.app_data_dir).map_err(StudioError::Unavailable)?;
            let mut grants = state
                .studio
                .grants
                .lock()
                .map_err(|_| StudioError::Unavailable("Assistant access unavailable".into()))?;
            let (grant, token) = grants.issue(
                client_name,
                scopes,
                state.studio.epoch.load(Ordering::SeqCst),
                chrono::Utc::now().timestamp(),
            )?;
            let file = dir.join(format!("{}.json", grant.id));
            let written = (|| -> Result<(), String> {
                let mut output = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&file)
                    .map_err(|_| "Could not create connection file".to_string())?;
                let credential = serde_json::json!({"socket":socket_path(&dir),"token":token});
                output
                    .write_all(credential.to_string().as_bytes())
                    .map_err(|_| "Could not write connection file".to_string())
            })();
            if let Err(error) = written {
                grants.revoke(&grant.id);
                return Err(StudioError::Unavailable(error));
            }
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
fn private_directory(root: &std::path::Path) -> Result<std::path::PathBuf, String> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let dir = root.join("studio-mcp");
    if !dir.exists() {
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .map_err(|_| "Could not create private assistant directory".to_string())?;
    }
    let metadata = std::fs::symlink_metadata(&dir)
        .map_err(|_| "Assistant directory unavailable".to_string())?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err("Assistant directory must be private to this OS user".into());
    }
    Ok(dir)
}

#[cfg(all(desktop, unix))]
fn socket_path(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join(format!("broker-{}.sock", std::process::id()))
}

#[cfg(all(desktop, unix))]
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BrokerRequest {
    token: String,
    operation: String,
    #[serde(default)]
    course_id: String,
    #[serde(default)]
    element_id: String,
    #[serde(default)]
    fingerprint: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    request_id: String,
}

#[cfg(all(desktop, unix))]
pub fn start(app: tauri::AppHandle) {
    use tauri::Manager;
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let dir = match private_directory(&state.app_data_dir) {
            Ok(dir) => dir,
            Err(error) => {
                log::warn!("MCP broker unavailable: {error}");
                return;
            }
        };
        let listener = match tokio::net::UnixListener::bind(socket_path(&dir)) {
            Ok(listener) => listener,
            Err(_) => {
                log::warn!("MCP broker socket could not be bound");
                return;
            }
        };
        state.studio.broker_available.store(true, Ordering::SeqCst);
        let capacity = std::sync::Arc::new(tokio::sync::Semaphore::new(8));
        while let Ok((stream, _)) = listener.accept().await {
            let Ok(permit) = capacity.clone().try_acquire_owned() else {
                continue;
            };
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let _permit = permit;
                let _ =
                    tokio::time::timeout(std::time::Duration::from_secs(5), handle(app, stream))
                        .await;
            });
        }
        state.studio.broker_available.store(false, Ordering::SeqCst);
    });
}

#[cfg(all(desktop, unix))]
async fn handle(app: tauri::AppHandle, stream: tokio::net::UnixStream) -> Result<(), String> {
    use tauri::Manager;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
    let (reader, mut writer) = stream.into_split();
    let mut reader = tokio::io::BufReader::new(reader.take(262145));
    let mut bytes = Vec::new();
    reader
        .read_until(b'\n', &mut bytes)
        .await
        .map_err(|_| "transport_error".to_string())?;
    if bytes.len() > 262144 || bytes.last() != Some(&b'\n') {
        return Err("invalid_input".into());
    }
    let request: BrokerRequest =
        serde_json::from_slice(&bytes).map_err(|_| "invalid_input".to_string())?;
    let state = app.state::<AppState>();
    let _gate = state.studio.broker_gate.read().await;
    let result = with_db(&state, |db| {
        let scope = match request.operation.as_str() {
            "list_course_drafts" | "read_lesson_draft" => "drafts:read",
            "propose_lesson_draft" => "drafts:propose",
            _ => return Err(StudioError::Permission),
        };
        let grant = state
            .studio
            .grants
            .lock()
            .map_err(|_| StudioError::Permission)?
            .authorize(
                &request.token,
                scope,
                state.studio.epoch.load(Ordering::SeqCst),
                chrono::Utc::now().timestamp(),
            )?;
        match request.operation.as_str() {
            "list_course_drafts" => {
                let mut stmt=db.prepare("SELECT c.id,c.title,e.id,e.title FROM courses c JOIN local_identity i ON i.id=1 AND i.stake_address=c.author_address LEFT JOIN course_chapters ch ON ch.course_id=c.id LEFT JOIN course_elements e ON e.chapter_id=ch.id AND e.element_type='text' ORDER BY c.id,ch.position,e.position LIMIT 101")?;
                let rows=stmt.query_map([],|r|Ok(serde_json::json!({"course_id":r.get::<_,String>(0)?,"course_title":r.get::<_,String>(1)?,"element_id":r.get::<_,Option<String>>(2)?,"element_title":r.get::<_,Option<String>>(3)?})))?;
                let mut items = rows.collect::<Result<Vec<_>, _>>()?;
                let truncated = items.len() > 100;
                items.truncate(100);
                Ok(serde_json::json!({"items":items,"truncated":truncated}))
            }
            "read_lesson_draft" => {
                alexandria_studio::store::read_draft(db, &request.course_id, &request.element_id)
            }
            "propose_lesson_draft" => {
                let proposal = alexandria_studio::store::propose_draft(
                    db,
                    &request.course_id,
                    &request.element_id,
                    &request.fingerprint,
                    &request.text,
                    &grant.client_name,
                    &format!("{}:{}", grant.id, request.request_id),
                )?;
                Ok(
                    serde_json::json!({"run_id":proposal.id,"status":proposal.value.status,"requires_instructor_review":true}),
                )
            }
            _ => Err(StudioError::Permission),
        }
    });
    let response = match result {
        Ok(value) => serde_json::json!({"result":value}),
        Err(error) => serde_json::json!({"error":error}),
    };
    let json = response.to_string();
    writer
        .write_all(json.as_bytes())
        .await
        .map_err(|_| "transport_error".to_string())?;
    writer
        .write_all(b"\n")
        .await
        .map_err(|_| "transport_error".to_string())?;
    Ok(())
}
