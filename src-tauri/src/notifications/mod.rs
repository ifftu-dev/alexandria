// SPDX-License-Identifier: AGPL-3.0-or-later
//! What the app tells the learner about, and how.
//!
//! A notification is a row in the profile's own database first and an OS
//! banner second. The row is what the in-app list shows, what the badge
//! counts, and what survives the banner being swiped away; the banner is only
//! for the moments nobody is looking at the app. Every producer — the
//! background pass that asks Alexandria Cloud what is waiting, a classroom
//! event arriving over gossip, a credential the pipeline just issued, a push
//! the phone received — goes through [`deliver`], so there is exactly one
//! place that decides whether to record, whether to interrupt, and what the
//! webview hears.
//!
//! # Not repeating ourselves
//!
//! Producers that observe state rather than events (the Cloud pull sees the
//! same pending invitation every minute) name what they saw with a dedupe
//! key. A key is remembered for good in `notification_dedupe`, separately
//! from the row, so clearing the list does not resurrect everything still
//! pending on the next pass.
//!
//! # The learner's switch
//!
//! `notifications.enabled` off means nothing is recorded and nothing is
//! shown — not "recorded quietly". A list that fills up while the learner
//! asked for silence is a list they did not ask for.
//!
//! # What a push carries
//!
//! Only a kind, an id and a route. Alexandria Cloud never puts a name, an
//! organisation or a DID into a push payload: Apple and Google carry that
//! payload and would read it. The copy in [`cloud_copy`] is therefore generic
//! by design, and it is the same copy Cloud sends, so a banner reads the same
//! whether the app or the phone's push service produced it.

pub mod cloud_pull;
pub mod opens;
pub mod push;
pub(crate) mod recipients;
mod store;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_notification::NotificationExt;

use crate::settings::{registry::keys, SettingsStore};

pub use store::{clear, list, mark_all_read, mark_read, record, unread_count};

/// The kinds the app and Alexandria Cloud share. A kind is a string on the
/// wire and in the database; these constants are the vocabulary.
pub mod kind {
    pub const INTERVIEW_INVITED: &str = "interview_invited";
    pub const INTERVIEW_ROOM_OPENED: &str = "interview_room_opened";
    pub const OFFER_SENT: &str = "offer_sent";
    pub const ACCESS_REQUESTED: &str = "access_requested";
    pub const ASSESSMENT_REQUESTED: &str = "assessment_requested";
    pub const CLASSROOM_CALL_STARTED: &str = "classroom_call_started";
    pub const CLASSROOM_JOIN_REQUESTED: &str = "classroom_join_requested";
    pub const CREDENTIAL_ISSUED: &str = "credential_issued";
    pub const GUARDIAN_REQUEST: &str = "guardian_request";
    pub const GENERIC: &str = "generic";

    /// Every kind the app knows, for validating what arrives from outside.
    pub const ALL: &[&str] = &[
        INTERVIEW_INVITED,
        INTERVIEW_ROOM_OPENED,
        OFFER_SENT,
        ACCESS_REQUESTED,
        ASSESSMENT_REQUESTED,
        CLASSROOM_CALL_STARTED,
        CLASSROOM_JOIN_REQUESTED,
        CREDENTIAL_ISSUED,
        GUARDIAN_REQUEST,
        GENERIC,
    ];
}

/// Emitted with a [`NotificationRow`] when one is recorded.
pub const EVENT_NEW: &str = "notifications:new";
/// Emitted with `{ unread }` whenever the list or its read state changes.
pub const EVENT_CHANGED: &str = "notifications:changed";
/// Emitted with `{ route }` when the learner acts on a banner or a push.
pub const EVENT_OPEN: &str = "notifications:open";

/// The Android channel every Alexandria notification is delivered on. Cloud
/// names the same channel in its FCM payloads.
pub const ANDROID_CHANNEL: &str = "alexandria_default";

/// Where a notification came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Produced by this app from something it observed itself.
    Local,
    /// Produced from a remote push the platform delivered.
    Push,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Push => "push",
        }
    }
}

/// A notification as the webview sees it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NotificationRow {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub route: Option<String>,
    pub source: String,
    pub created_at: String,
    pub read_at: Option<String>,
    pub payload: Option<serde_json::Value>,
}

/// A notification about to be recorded.
#[derive(Debug, Clone)]
pub struct NewNotification {
    pub kind: String,
    pub title: String,
    pub body: String,
    pub route: Option<String>,
    /// Remembered for good once recorded; a second notification with the
    /// same key is dropped.
    pub dedupe_key: Option<String>,
    pub payload: Option<serde_json::Value>,
    pub source: Source,
}

impl NewNotification {
    pub fn new(kind: &str, title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            kind: kind.to_string(),
            title: title.into(),
            body: body.into(),
            route: None,
            dedupe_key: None,
            payload: None,
            source: Source::Local,
        }
    }

    pub fn route(mut self, route: impl Into<String>) -> Self {
        self.route = Some(route.into());
        self
    }

    pub fn dedupe(mut self, key: impl Into<String>) -> Self {
        self.dedupe_key = Some(key.into());
        self
    }

    pub fn payload(mut self, payload: serde_json::Value) -> Self {
        self.payload = Some(payload);
        self
    }

    pub fn from_push(mut self) -> Self {
        self.source = Source::Push;
        self
    }
}

/// The generic copy for a Cloud-originated kind: title, body and the route a
/// tap opens. `None` for a kind Cloud does not send.
///
/// Mirrors the table in Alexandria Cloud's push sender. The two are kept the
/// same by hand; the point of generic copy is that it never needs a name.
pub fn cloud_copy(kind: &str, id: &str) -> Option<(&'static str, &'static str, String)> {
    let (title, body, route) = match kind {
        kind::INTERVIEW_INVITED => (
            "New interview invitation",
            "An organisation invited you to interview. Open Alexandria to respond.",
            "/settings/organisations".to_string(),
        ),
        kind::INTERVIEW_ROOM_OPENED => (
            "Your interview room is open",
            "The panel is waiting. Open Alexandria to join.",
            format!("/interview-room/{id}"),
        ),
        kind::OFFER_SENT => (
            "New offer",
            "An organisation sent you an offer.",
            "/settings/organisations".to_string(),
        ),
        kind::ACCESS_REQUESTED => (
            "Access request",
            "An organisation is asking to see your credentials.",
            "/settings/organisations".to_string(),
        ),
        kind::ASSESSMENT_REQUESTED => (
            "New assessment request",
            "An organisation asked you to take an assessment.",
            "/settings/organisations".to_string(),
        ),
        _ => return None,
    };
    Some((title, body, route))
}

/// The notification a committed classroom meta event warrants for this
/// learner, if any: a call somebody else started, or a request to join a
/// classroom this learner moderates. `apply_incoming_meta` has already
/// dropped join requests for non-moderators, so a request that reaches here
/// is one the learner can act on.
pub fn for_classroom_meta(
    event: &crate::classroom::types::ClassroomMetaTauriEvent,
    local_address: Option<&str>,
) -> Option<NewNotification> {
    let data = event.data.get("data")?;
    match event.event_type.as_str() {
        "CallStarted" => {
            let call_id = data.get("call_id")?.as_str()?;
            let started_by = data.get("started_by")?.as_str()?;
            if Some(started_by) == local_address {
                return None;
            }
            Some(
                NewNotification::new(
                    kind::CLASSROOM_CALL_STARTED,
                    "A classroom call started",
                    "Somebody started a call in one of your classrooms.",
                )
                .route(format!("/classrooms/{}", event.classroom_id))
                .dedupe(format!("classroom_call:{call_id}"))
                .payload(serde_json::json!({
                    "classroomId": event.classroom_id,
                    "callId": call_id,
                })),
            )
        }
        "JoinRequest" => {
            let request_id = data.get("request_id")?.as_str()?;
            let display_name = data.get("display_name").and_then(serde_json::Value::as_str);
            Some(
                NewNotification::new(
                    kind::CLASSROOM_JOIN_REQUESTED,
                    "Someone wants to join your classroom",
                    match display_name {
                        Some(name) => format!("{name} asked to join."),
                        None => "A learner asked to join.".to_string(),
                    },
                )
                .route(format!("/classrooms/{}/requests", event.classroom_id))
                .dedupe(format!(
                    "classroom_join:{}:{request_id}",
                    event.classroom_id
                ))
                .payload(serde_json::json!({
                    "classroomId": event.classroom_id,
                    "requestId": request_id,
                })),
            )
        }
        _ => None,
    }
}

/// A credential the auto-issuance pipeline just signed.
pub fn for_issued_credential(credential_id: &str) -> NewNotification {
    NewNotification::new(
        kind::CREDENTIAL_ISSUED,
        "New credential",
        "A completion was verified on-chain and a credential was issued to you.",
    )
    .route(format!("/credentials/{credential_id}"))
    .dedupe(format!("credential:{credential_id}"))
    .payload(serde_json::json!({ "id": credential_id }))
}

/// A guardian completed a link to this (ward) profile.
pub fn for_guardian_linked(link_id: &str, guardian_display_name: Option<&str>) -> NewNotification {
    NewNotification::new(
        kind::GUARDIAN_REQUEST,
        "A guardian linked to your account",
        match guardian_display_name {
            Some(name) => format!("{name} now has oversight of this account."),
            None => "A guardian now has oversight of this account.".to_string(),
        },
    )
    .route("/settings/guardian")
    .dedupe(format!("guardian_link:{link_id}"))
    .payload(serde_json::json!({ "linkId": link_id }))
}

/// A recorded notification and the unread count after it.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub row: NotificationRow,
    pub unread: u32,
}

/// Record a notification unless the learner has switched notifications off or
/// it repeats one already seen. Writes only; nothing is emitted. The producer
/// that holds the database inside a job uses this and announces afterwards.
pub fn record_if_enabled(
    conn: &Connection,
    new: &NewNotification,
) -> Result<Option<Recorded>, String> {
    if !SettingsStore::get(conn, keys::NOTIFICATIONS_ENABLED) {
        return Ok(None);
    }
    let now = crate::commands::credentials::now_rfc3339();
    let Some(row) = store::record(conn, new, &now)? else {
        return Ok(None);
    };
    let unread = store::unread_count(conn)?;
    Ok(Some(Recorded { row, unread }))
}

/// Tell the webview, and the OS when nobody is looking at the app.
pub fn announce<R: Runtime>(app: &AppHandle<R>, recorded: &Recorded) {
    let Some(session) = app
        .try_state::<crate::AppState>()
        .and_then(|s| s.profile_operations.session())
    else {
        return;
    };
    if let Err(error) = app.emit(
        EVENT_NEW,
        serde_json::json!({ "session": session, "row": &recorded.row }),
    ) {
        log::debug!("notifications: emit failed: {error}");
    }
    emit_changed(app, recorded.unread);
    if should_interrupt(app, &recorded.row) {
        os_banner(app, &recorded.row);
    }
}

/// Record and announce in one step, for producers that hold the connection
/// outside an executor job.
pub fn deliver<R: Runtime>(
    app: &AppHandle<R>,
    conn: &Connection,
    new: &NewNotification,
) -> Result<Option<Recorded>, String> {
    let recorded = record_if_enabled(conn, new)?;
    if let Some(recorded) = &recorded {
        announce(app, recorded);
    }
    Ok(recorded)
}

/// Emit the unread count, after anything that changes it.
pub fn emit_changed<R: Runtime>(app: &AppHandle<R>, unread: u32) {
    let Some(session) = app
        .try_state::<crate::AppState>()
        .and_then(|s| s.profile_operations.session())
    else {
        return;
    };
    let _ = app.emit(
        EVENT_CHANGED,
        serde_json::json!({ "session": session, "unread": unread }),
    );
}

/// Publish an already persisted and locally resolved tap.
pub fn emit_open<R: Runtime>(app: &AppHandle<R>, intent: &opens::NotificationOpen) {
    let _ = app.emit(EVENT_OPEN, intent);
}

/// Whether a banner is warranted: on a phone always, on the desktop only
/// while the app's window is not the one in front. A banner over the very
/// screen that just updated is noise.
///
/// A push on iOS is the exception: iOS already showed the system banner
/// before handing the payload over, so showing another would double it.
fn should_interrupt<R: Runtime>(app: &AppHandle<R>, row: &NotificationRow) -> bool {
    if cfg!(target_os = "ios") && row.source == Source::Push.as_str() {
        return false;
    }
    if cfg!(mobile) {
        return true;
    }
    let focused = app
        .get_webview_window("main")
        .and_then(|window| window.is_focused().ok())
        .unwrap_or(false);
    !focused
}

fn os_banner<R: Runtime>(app: &AppHandle<R>, row: &NotificationRow) {
    let Some(state) = app.try_state::<crate::AppState>() else {
        return;
    };
    let Some(profile) = state.active_id() else {
        return;
    };
    let Ok(network) = recipients::network() else {
        return;
    };
    let mut builder = app
        .notification()
        .builder()
        .title(&row.title)
        .body(&row.body)
        .extra("notificationId", &row.id)
        .extra("profileId", profile.as_str())
        .extra("networkId", network);
    if let Some(route) = &row.route {
        builder = builder.extra("route", route);
    }
    #[cfg(target_os = "android")]
    {
        builder = builder.channel_id(ANDROID_CHANNEL);
    }
    if let Err(error) = builder.show() {
        log::debug!("notifications: OS banner failed: {error}");
    }
}

/// Wire the OS side up once the plugins exist: the Android channel and the
/// push plugin's events. Called from the app's setup hook.
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_notification::{Channel, Importance};
        let channel = Channel::builder(ANDROID_CHANNEL, "Alexandria")
            .description("Interviews, offers, requests and classroom activity")
            .importance(Importance::High)
            .build();
        if let Err(error) = app.notification().create_channel(channel) {
            log::warn!("notifications: could not create the Android channel: {error}");
        }
    }

    // A tap on a banner the app itself showed. The notification plugin
    // reports it with the `extra` map the banner was built with, and the
    // route is in there (see `os_banner`). On iOS the push plugin takes over
    // the notification centre's delegate once a token has been requested,
    // and the same tap then arrives through `push::install` instead, with
    // the route under `__EXTRA__`; the two never both fire for one tap.
    let on_banner = app.clone();
    if let Err(error) = app.notification().on_action(move |performed| {
        if route_of_action(performed).is_none() {
            return;
        }
        if let Some(notification) = performed.notification() {
            let extras = serde_json::to_value(notification.extra()).ok();
            if let Some(extras) = extras.as_ref().and_then(serde_json::Value::as_object) {
                if let Ok(Some(intent)) =
                    opens::local_tap(&on_banner.state::<crate::AppState>(), extras)
                {
                    emit_open(&on_banner, &intent);
                }
            }
        }
    }) {
        log::warn!("notifications: banner taps will not route: {error}");
    }

    push::install(app);
}

/// The route a banner tap names: a click on the banner itself (`tap`), not a
/// dismissal or an action button, of a banner that carried a route.
fn route_of_action(performed: &tauri_plugin_notification::ActionPerformed) -> Option<String> {
    if performed.action_id() != "tap" {
        return None;
    }
    performed
        .notification()?
        .extra()
        .get("route")?
        .as_str()
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_copy_names_a_route_per_kind() {
        let (title, _, route) = cloud_copy(kind::INTERVIEW_ROOM_OPENED, "abc").unwrap();
        assert_eq!(title, "Your interview room is open");
        assert_eq!(route, "/interview-room/abc");
        assert_eq!(
            cloud_copy(kind::OFFER_SENT, "x").unwrap().2,
            "/settings/organisations"
        );
        assert!(cloud_copy(kind::CLASSROOM_CALL_STARTED, "x").is_none());
    }

    #[test]
    fn classroom_events_become_notifications_for_the_right_people() {
        use crate::classroom::types::{ClassroomMetaEvent, ClassroomMetaTauriEvent};
        let tauri_event = |event: ClassroomMetaEvent| ClassroomMetaTauriEvent {
            classroom_id: event.classroom_id().to_string(),
            event_type: event.event_type().to_string(),
            data: serde_json::to_value(&event).unwrap(),
        };
        let call = tauri_event(ClassroomMetaEvent::CallStarted {
            classroom_id: "c1".into(),
            call_id: "call-1".into(),
            ticket: "t".into(),
            started_by: "stake_other".into(),
        });
        let n = for_classroom_meta(&call, Some("stake_me")).unwrap();
        assert_eq!(n.kind, kind::CLASSROOM_CALL_STARTED);
        assert_eq!(n.route.as_deref(), Some("/classrooms/c1"));
        assert_eq!(n.dedupe_key.as_deref(), Some("classroom_call:call-1"));
        // The learner who started the call is not told about it.
        assert!(for_classroom_meta(&call, Some("stake_other")).is_none());

        let join = tauri_event(ClassroomMetaEvent::JoinRequest {
            classroom_id: "c1".into(),
            request_id: "r1".into(),
            display_name: Some("Ada".into()),
            message: None,
        });
        let n = for_classroom_meta(&join, Some("stake_me")).unwrap();
        assert_eq!(n.body, "Ada asked to join.");
        assert_eq!(n.route.as_deref(), Some("/classrooms/c1/requests"));

        let ended = tauri_event(ClassroomMetaEvent::CallEnded {
            classroom_id: "c1".into(),
            call_id: "call-1".into(),
        });
        assert!(for_classroom_meta(&ended, None).is_none());
    }

    #[test]
    fn a_banner_tap_names_its_route_and_nothing_else_does() {
        let tap = serde_json::from_value::<tauri_plugin_notification::ActionPerformed>(
            serde_json::json!({
                "actionId": "tap",
                "notification": { "id": 7, "extra": { "route": "/credentials/1" } }
            }),
        )
        .unwrap();
        assert_eq!(route_of_action(&tap).as_deref(), Some("/credentials/1"));

        let dismiss = serde_json::from_value::<tauri_plugin_notification::ActionPerformed>(
            serde_json::json!({
                "actionId": "dismiss",
                "notification": { "id": 7, "extra": { "route": "/credentials/1" } }
            }),
        )
        .unwrap();
        assert_eq!(route_of_action(&dismiss), None);

        let bare = serde_json::from_value::<tauri_plugin_notification::ActionPerformed>(
            serde_json::json!({ "actionId": "tap", "notification": { "id": 7 } }),
        )
        .unwrap();
        assert_eq!(route_of_action(&bare), None);
    }
}
