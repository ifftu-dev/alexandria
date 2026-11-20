//! cmi5 units: launch one under an open package launch, end its session,
//! and on desktop bring a unit's own window to the front. The logic is in
//! [`crate::packages::attempts::cmi5`].
//!
//! A unit that asks for its own window gets a separate webview window on
//! desktop. Its label is listed in no capability, so it reaches no command;
//! closing it ends the session (abandoned unless the unit terminated), and
//! the app closes it when the unit returns to its return page.

use std::sync::Arc;

use crate::credentials::Issuer;
use crate::db::executor::DatabaseWorkload;
use crate::domain::package_attempt::{Cmi5Launch, PackageAttemptProgress};
use crate::packages::attempts::cmi5::{self, LaunchRequest};
use crate::packages::{ExecutorDb, OwnedExecutorDb};
use crate::profile::scope::ProfileState as State;
use crate::settings::{registry::keys, SettingsStore};
use crate::AppState;

/// The event the player hears when a session ends without it asking: the
/// unit's own window closed, or its endpoint could not come back after the
/// app resumed. Payload: `{ session_id, progress? }`.
pub const SESSION_ENDED_EVENT: &str = "package-cmi5-ended";

fn db<'a>(state: &'a State<'_, AppState>) -> ExecutorDb<'a> {
    ExecutorDb {
        executor: &state.db_executor,
        lease: state.profile_lease(),
        workload: DatabaseWorkload::Learner,
    }
}

/// The window label of a unit's own window.
#[cfg(desktop)]
fn window_label(session_id: &str) -> String {
    format!("cmi5-unit-{}", session_id.replace('-', ""))
}

/// Launches one cmi5 unit of an attempt under the open package launch
/// `prefix`. Needs the unlocked vault: the unit's actor and registration are
/// derived from the learner's key. On desktop a unit that asks for its own
/// window opens in one here; the player shows that instead of the frame.
#[tauri::command]
pub async fn package_cmi5_launch(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    attempt_id: String,
    activity_id: String,
    prefix: String,
) -> Result<Cmi5Launch, String> {
    let (key, _) = Issuer::from_keystore(&state.keystore)
        .await
        .map_err(|e| e.to_string())?
        .into_parts();
    let language = {
        use crate::packages::PackageDb;
        db(&state)
            .run("packages.cmi5-language", |db| {
                Ok(SettingsStore::get(db.conn(), keys::USER_LANGUAGE))
            })
            .await
            .unwrap_or_default()
    };
    let languages = match language.as_str() {
        "" | "system" => "en-US".to_string(),
        other => other.to_string(),
    };
    let owned = Arc::new(OwnedExecutorDb {
        executor: state.db_executor.clone(),
        lease: state.profile_lease(),
        workload: DatabaseWorkload::Learner,
    });
    let launched = cmi5::launch(
        owned,
        &state.cmi5_sessions,
        &state.package_launches,
        &key.to_bytes(),
        LaunchRequest {
            attempt_id,
            activity_id,
            prefix: prefix.clone(),
            languages,
        },
    )
    .await
    .map_err(|e| e.to_string())?;
    #[cfg(desktop)]
    if launched.own_window {
        open_unit_window(&app, &state, &prefix, &launched)?;
    }
    #[cfg(not(desktop))]
    let _ = prefix;
    #[cfg(not(desktop))]
    let _ = app;
    Ok(launched)
}

#[cfg(desktop)]
fn open_unit_window(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    prefix: &str,
    launched: &Cmi5Launch,
) -> Result<(), String> {
    use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

    let url = url::Url::parse(&launched.url).map_err(|e| e.to_string())?;
    let label = window_label(&launched.session_id);
    let prefix = prefix.to_string();
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .title(launched.activity_id.as_str())
        .inner_size(1024.0, 768.0)
        // The unit stays under its own launch: anywhere else would leave
        // the content policy and the bootstrap behind.
        .on_navigation(move |target| cmi5::unit_window_may_navigate(&prefix, target))
        .on_page_load(|window, payload| {
            // The unit returned: its own window has done its job.
            if payload.url().path().ends_with(cmi5::RETURN_PAGE) {
                let _ = window.close();
            }
        })
        .build()
        .map_err(|e| e.to_string())?;
    let session_id = launched.session_id.clone();
    let handle = app.clone();
    let sessions = state.cmi5_sessions.clone();
    let launches = state.package_launches.clone();
    let owned = OwnedExecutorDb {
        executor: state.db_executor.clone(),
        lease: state.profile_lease(),
        workload: DatabaseWorkload::Learner,
    };
    let owned = Arc::new(owned);
    window.on_window_event(move |event| {
        if !matches!(event, tauri::WindowEvent::Destroyed) {
            return;
        }
        if !sessions.is_open(&session_id) {
            return;
        }
        let (handle, sessions, launches, owned, session_id) = (
            handle.clone(),
            sessions.clone(),
            launches.clone(),
            owned.clone(),
            session_id.clone(),
        );
        tauri::async_runtime::spawn(async move {
            let progress = cmi5::end(&*owned, &sessions, &launches, &session_id)
                .await
                .map_err(|e| log::warn!("cmi5 session not ended: {e}"))
                .ok();
            if let Some(main) = handle.get_webview_window("main") {
                let _ = main.emit(
                    SESSION_ENDED_EVENT,
                    serde_json::json!({ "session_id": session_id, "progress": progress }),
                );
            }
        });
    });
    Ok(())
}

/// Ends a unit's session: its endpoint stops, a session the unit did not
/// terminate is recorded abandoned, and the attempt's progress comes back.
/// On desktop its own window, if it has one, closes.
#[tauri::command]
pub async fn package_cmi5_end(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<PackageAttemptProgress, String> {
    let ended = cmi5::end(
        &db(&state),
        &state.cmi5_sessions,
        &state.package_launches,
        &session_id,
    )
    .await
    .map_err(|e| e.to_string())?;
    #[cfg(desktop)]
    {
        use tauri::Manager;
        if let Some(window) = app.get_webview_window(&window_label(&session_id)) {
            let _ = window.close();
        }
    }
    #[cfg(not(desktop))]
    let _ = app;
    Ok(ended)
}

/// After the app resumes (mobile): binds again any open session's endpoint
/// the system reclaimed. A session whose endpoint cannot come back ends, and
/// the player hears about it.
#[cfg(mobile)]
pub fn revive_endpoints(app: tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let operations = &state.profile_operations;
        let Some(lease) = operations
            .session()
            .and_then(|session| operations.admit(&session).ok())
        else {
            return;
        };
        let owned = Arc::new(OwnedExecutorDb {
            executor: state.db_executor.clone(),
            lease,
            workload: DatabaseWorkload::Learner,
        });
        for session_id in cmi5::revive(owned, &state.cmi5_sessions, &state.package_launches).await {
            let _ = app.emit(
                SESSION_ENDED_EVENT,
                serde_json::json!({ "session_id": session_id }),
            );
        }
    });
}

/// Brings a unit's own window to the front (desktop). Returns whether the
/// window is still open.
#[tauri::command]
pub async fn package_cmi5_focus(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<bool, String> {
    if !state.cmi5_sessions.is_open(&session_id) {
        return Ok(false);
    }
    #[cfg(desktop)]
    {
        use tauri::Manager;
        if let Some(window) = app.get_webview_window(&window_label(&session_id)) {
            let _ = window.unminimize();
            window.set_focus().map_err(|e| e.to_string())?;
            return Ok(true);
        }
        Ok(false)
    }
    #[cfg(not(desktop))]
    {
        let _ = (app, session_id);
        Ok(false)
    }
}
