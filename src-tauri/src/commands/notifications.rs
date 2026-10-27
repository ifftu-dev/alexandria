// SPDX-License-Identifier: AGPL-3.0-or-later
//! IPC for the notification list and this device's push registration.

use tauri::AppHandle;
use tauri_plugin_notification::{NotificationExt, PermissionState};

use crate::db::executor::DatabaseWorkload;
use crate::notifications::{self, push::PushStatus, NewNotification, NotificationRow};
use crate::profile::scope::ProfileState as State;
use crate::AppState;

/// Newest first; `limit` defaults to 100.
#[tauri::command]
pub async fn notifications_list(
    state: State<'_, AppState>,
    limit: Option<u32>,
    unread_only: Option<bool>,
) -> Result<Vec<NotificationRow>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "notifications.list",
            move |db| notifications::list(db.conn(), limit, unread_only.unwrap_or(false)),
        )
        .await
}

#[tauri::command]
pub async fn notifications_unread_count(state: State<'_, AppState>) -> Result<u32, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "notifications.unread",
            |db| notifications::unread_count(db.conn()),
        )
        .await
}

#[tauri::command]
pub async fn notifications_mark_read(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<(), String> {
    let unread = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "notifications.mark_read",
            move |db| {
                let now = crate::commands::credentials::now_rfc3339();
                notifications::mark_read(db.conn(), &ids, &now)?;
                notifications::unread_count(db.conn())
            },
        )
        .await?;
    notifications::emit_changed(&app, unread);
    Ok(())
}

#[tauri::command]
pub async fn notifications_mark_all_read(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "notifications.mark_all_read",
            |db| {
                let now = crate::commands::credentials::now_rfc3339();
                notifications::mark_all_read(db.conn(), &now).map(drop)
            },
        )
        .await?;
    notifications::emit_changed(&app, 0);
    Ok(())
}

/// Delete every notification. What they reported stays reported, so the
/// next background pass does not bring them all back.
#[tauri::command]
pub async fn notifications_clear(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "notifications.clear",
            |db| notifications::clear(db.conn()).map(drop),
        )
        .await?;
    notifications::emit_changed(&app, 0);
    Ok(())
}

/// Ask the OS for permission to show banners. The desktop needs none and
/// answers `granted`; a phone shows its prompt the first time.
///
/// `granted` | `denied` | `unavailable`.
#[tauri::command]
pub async fn notifications_permission_request(app: AppHandle) -> Result<String, String> {
    let state = match app.notification().permission_state() {
        Ok(PermissionState::Granted) => PermissionState::Granted,
        Ok(PermissionState::Denied) => PermissionState::Denied,
        Ok(_) => app
            .notification()
            .request_permission()
            .map_err(|e| e.to_string())?,
        Err(error) => return Err(error.to_string()),
    };
    Ok(match state {
        PermissionState::Granted => "granted",
        PermissionState::Denied => "denied",
        _ => "unavailable",
    }
    .to_string())
}

/// The route of a banner or push the learner tapped before the webview was
/// listening, if any. Taken once. The webview calls this when it starts
/// listening, so a tap that launched the app still lands.
#[tauri::command]
pub async fn notifications_pending_open() -> Result<Option<String>, String> {
    Ok(notifications::take_pending_open())
}

#[tauri::command]
pub async fn push_status(
    app: AppHandle,
    _state: State<'_, AppState>,
) -> Result<PushStatus, String> {
    notifications::push::status(&app).await
}

#[tauri::command]
pub async fn push_register(
    app: AppHandle,
    _state: State<'_, AppState>,
) -> Result<PushStatus, String> {
    notifications::push::register(&app).await
}

#[tauri::command]
pub async fn push_unregister(
    app: AppHandle,
    _state: State<'_, AppState>,
) -> Result<PushStatus, String> {
    notifications::push::unregister(&app).await
}

/// Push a notification of `kind` through the whole pipeline, for trying the
/// list, the badge and the banner by hand. Debug builds only.
#[tauri::command]
pub async fn notifications_emit_test(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: String,
) -> Result<NotificationRow, String> {
    if !cfg!(debug_assertions) {
        return Err("test notifications exist in debug builds only".into());
    }
    if !notifications::kind::ALL.contains(&kind.as_str()) {
        return Err(format!("unknown notification kind `{kind}`"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let notification = match notifications::cloud_copy(&kind, &id) {
        Some((title, body, route)) => NewNotification::new(&kind, title, body).route(route),
        None => NewNotification::new(
            &kind,
            format!("Test: {kind}"),
            "A notification produced on request, to see how it looks.",
        )
        .route("/notifications"),
    }
    .payload(serde_json::json!({ "id": id, "test": true }));
    let recorded = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "notifications.emit_test",
            move |db| notifications::record_if_enabled(db.conn(), &notification),
        )
        .await?
        .ok_or("notifications are switched off for this profile")?;
    notifications::announce(&app, &recorded);
    Ok(recorded.row)
}
