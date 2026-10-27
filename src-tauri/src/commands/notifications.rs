// SPDX-License-Identifier: AGPL-3.0-or-later
//! IPC for the notification list and this device's push registration.

use tauri::AppHandle;

use crate::db::executor::DatabaseWorkload;
use crate::notifications::{self, push::PushStatus, NewNotification, NotificationRow};
use crate::profile::scope::ProfileLease;
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
pub async fn notifications_permission_request(
    app: AppHandle,
    lease: ProfileLease,
) -> Result<String, String> {
    if !lease.is_current() {
        return Err("profile session changed".into());
    }
    // Permission is device-global; do not retain a profile while awaiting the OS.
    drop(lease);
    if cfg!(mobile) {
        let granted = tauri_plugin_mobile_push::request_permission(&app)
            .await
            .map_err(|e| e.to_string())?;
        Ok(if granted { "granted" } else { "denied" }.into())
    } else {
        Ok("granted".into())
    }
}

/// Peek at a validated tap even while locked. Only the matching admitted
/// profile can consume it after navigation through notifications_consume_open.
#[tauri::command]
pub async fn notifications_pending_open(
    state: tauri::State<'_, AppState>,
) -> Result<Option<notifications::opens::NotificationOpen>, String> {
    notifications::opens::pending(&state).await
}

#[tauri::command]
pub async fn notifications_consume_open(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    notifications::opens::consume(&state, &state.profile_lease(), &id)
}

#[tauri::command]
pub async fn push_status(state: State<'_, AppState>) -> Result<PushStatus, String> {
    notifications::push::status(&state, state.profile_lease()).await
}

#[tauri::command]
pub async fn push_register(app: AppHandle, lease: ProfileLease) -> Result<PushStatus, String> {
    notifications::push::register(&app, lease).await
}

#[tauri::command]
pub async fn push_unregister(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<PushStatus, String> {
    notifications::push::unregister(&app, state.profile_lease()).await
}

#[tauri::command]
pub async fn push_set_enabled(
    app: AppHandle,
    lease: ProfileLease,
    enabled: bool,
) -> Result<PushStatus, String> {
    use tauri::Manager;
    let status = notifications::push::reconcile(
        &app.state::<AppState>(),
        lease.clone(),
        None,
        Some(enabled),
    )
    .await?;
    use tauri::Emitter;
    if lease.is_current() {
        let _ = app.emit(
            "settings-changed",
            serde_json::json!({ "key": "notifications.push_enabled" }),
        );
    }
    if enabled {
        notifications::push::register(&app, lease).await
    } else {
        Ok(status)
    }
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
