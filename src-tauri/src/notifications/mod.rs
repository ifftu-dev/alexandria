// SPDX-License-Identifier: AGPL-3.0-or-later
//! What the app tells the learner about, and how.
//!
//! A notification is a row in the profile's own database first and an OS
//! banner second. The row is what the in-app list shows, what the badge
//! counts, and what survives the banner being swiped away; the banner is only
//! for the moments nobody is looking at the app. Every producer — the
//! background pass that asks Alexandria Cloud what is waiting, a classroom
//! event arriving over gossip, a credential the pipeline just issued, a push
//! the phone received — goes through [`record_if_enabled`] and [`announce`],
//! so there is exactly one place that decides whether to record, whether to
//! interrupt, and what the webview hears.
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
mod copy;
pub mod opens;
pub mod push;
pub(crate) mod recipients;
mod store;

use rusqlite::Connection;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_notification::NotificationExt;

use crate::settings::{registry::keys, SettingsStore};

pub use store::{clear, list, mark_all_read, mark_read, record, unread_count};

/// Why a notification operation failed. Its display is the text the
/// commands return.
#[derive(Debug, thiserror::Error)]
pub enum NotificationError {
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// A file beside the profiles (open intents, recipient routing, the open
    /// key) could not be read or written.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A lock on one of those files was poisoned.
    #[error("poisoned lock: another task failed inside")]
    Poisoned,
    /// The profile session the operation began under ended.
    #[error("profile session changed")]
    SessionChanged,
    #[error("no profile is unlocked")]
    NoProfile,
    #[error("vault is locked — unlock first")]
    VaultLocked,
    /// The vault or the wallet it holds could not give the issuer key.
    #[error("{0}")]
    Vault(String),
    /// A profile-scoped step failed: the database executor (whose jobs
    /// return strings), profile admission, or a lease.
    #[error("{0}")]
    Profile(String),
    #[error(transparent)]
    NetworkProfile(&'static crate::network_profile::NetworkProfileError),
    #[error("push mutation version exhausted")]
    VersionExhausted,
    #[error("notification belongs to another profile")]
    OtherProfile,
    #[error("invalid notification envelope")]
    InvalidEnvelope,
    #[error("unsupported notification open format")]
    UnsupportedOpenFormat,
    #[error("notification key derivation failed")]
    KeyDerivation,
    #[error("invalid notification encryption key")]
    InvalidKey,
    /// Sealing or opening a held notification open failed.
    #[error("{0}")]
    Crypto(String),
}

impl<T> From<std::sync::PoisonError<T>> for NotificationError {
    fn from(_: std::sync::PoisonError<T>) -> Self {
        NotificationError::Poisoned
    }
}

pub use crate::domain::notification::{
    NotificationKind, NotificationOpen, NotificationPermission, NotificationRow,
    NotificationSource, PushErrorCode, PushStatus,
};

/// Emitted with a [`NotificationRow`] when one is recorded.
pub const EVENT_NEW: &str = "notifications:new";
/// Emitted with `{ unread }` whenever the list or its read state changes.
pub const EVENT_CHANGED: &str = "notifications:changed";
/// Emitted with `{ route }` when the learner acts on a banner or a push.
pub const EVENT_OPEN: &str = "notifications:open";

/// The Android channel every Alexandria notification is delivered on. Cloud
/// names the same channel in its FCM payloads.
pub const ANDROID_CHANNEL: &str = "alexandria_default";

/// A notification about to be recorded.
#[derive(Debug, Clone)]
pub struct NewNotification {
    pub kind: NotificationKind,
    pub title: String,
    pub body: String,
    pub route: Option<String>,
    /// Remembered for good once recorded; a second notification with the
    /// same key is dropped.
    pub dedupe_key: Option<String>,
    pub payload: Option<serde_json::Value>,
    pub source: NotificationSource,
}

impl NewNotification {
    pub fn new(kind: NotificationKind, title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            body: body.into(),
            route: None,
            dedupe_key: None,
            payload: None,
            source: NotificationSource::Local,
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
        self.source = NotificationSource::Push;
        self
    }
}

/// Where an interview-room notification leads. Inbox rows, pushes and Cloud
/// discovery all name the room through this.
pub fn interview_room_route(id: &str) -> String {
    format!("/interview-room/{id}")
}

/// English generic copy and destination for a Cloud-originated kind.
/// `None` for a kind Cloud does not send. Recording selects the profile locale
/// from the shared webview catalogs; contract fixtures check Cloud agreement.
pub fn cloud_copy(
    kind: NotificationKind,
    id: &str,
) -> Option<(&'static str, &'static str, String)> {
    use NotificationKind as K;
    let route = match kind {
        K::InterviewRoomOpened => interview_room_route(id),
        K::InterviewInvited | K::OfferSent | K::AccessRequested | K::AssessmentRequested => {
            "/settings/organisations".to_string()
        }
        K::ClassroomCallStarted
        | K::ClassroomJoinRequested
        | K::CredentialIssued
        | K::GuardianRequest
        | K::Generic => return None,
    };
    let (title, body) = copy::localized("en", kind);
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
                    NotificationKind::ClassroomCallStarted,
                    "A classroom call started",
                    "Somebody started a call in one of your classrooms.",
                )
                .route(format!("/classrooms/{}", event.classroom_id))
                .dedupe(format!("classroom_call:{call_id}"))
                .payload(serde_json::json!({
                    "classroom_id": event.classroom_id,
                    "call_id": call_id,
                })),
            )
        }
        "JoinRequest" => {
            let request_id = data.get("request_id")?.as_str()?;
            let display_name = data.get("display_name").and_then(serde_json::Value::as_str);
            Some(
                NewNotification::new(
                    NotificationKind::ClassroomJoinRequested,
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
                    "classroom_id": event.classroom_id,
                    "request_id": request_id,
                })),
            )
        }
        _ => None,
    }
}

/// A credential the auto-issuance pipeline just signed.
pub fn for_issued_credential(credential_id: &str) -> NewNotification {
    NewNotification::new(
        NotificationKind::CredentialIssued,
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
        NotificationKind::GuardianRequest,
        "A guardian linked to your account",
        match guardian_display_name {
            Some(name) => format!("{name} now has oversight of this account."),
            None => "A guardian now has oversight of this account.".to_string(),
        },
    )
    .route("/settings/guardian")
    .dedupe(format!("guardian_link:{link_id}"))
    .payload(serde_json::json!({ "link_id": link_id }))
}

/// A recorded notification and the unread count after it.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub row: NotificationRow,
    pub unread: u32,
}

/// Record a notification unless the learner has switched notifications off or
/// it repeats one already seen. Selects generic copy in the profile language,
/// ignoring producer/provider text. Writes only; nothing is emitted. The producer
/// that holds the database inside a job uses this and announces afterwards.
pub fn record_if_enabled(
    conn: &Connection,
    new: &NewNotification,
) -> Result<Option<Recorded>, NotificationError> {
    if !SettingsStore::get(conn, keys::NOTIFICATIONS_ENABLED) {
        return Ok(None);
    }
    let now = crate::clock::now_rfc3339();
    let locale = SettingsStore::get(conn, keys::USER_LANGUAGE);
    let (title, body) = copy::localized(&locale, new.kind);
    let localized = NewNotification {
        title: title.into(),
        body: body.into(),
        ..new.clone()
    };
    let Some(row) = store::record(conn, &localized, &now)? else {
        return Ok(None);
    };
    let unread = store::unread_count(conn)?;
    Ok(Some(Recorded { row, unread }))
}

/// What goes out for one recorded notification: the session the webview
/// checks and the profile the OS banner is tagged with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    pub session: String,
    pub profile_id: crate::profile::ProfileId,
}

/// The announcement for a notification recorded under `session`, the
/// session of the lease its producer ran with. `None` once that session is no
/// longer current: a lock or profile switch came in between, and the row
/// stays in its own profile's store unannounced. The profile is read while
/// the session is held current, so a banner is never tagged with the profile
/// that took over.
pub fn announcement(state: &crate::AppState, session: &str) -> Option<Announcement> {
    let lease = state.profile_operations.admit(session).ok()?;
    let profile_id = lease
        .with_current(|| {
            state
                .active_id()
                .ok_or_else(|| "no active profile".to_string())
        })
        .ok()?;
    Some(Announcement {
        session: session.to_owned(),
        profile_id,
    })
}

/// Tell the webview, and the OS when nobody is looking at the app.
pub fn announce<R: Runtime>(app: &AppHandle<R>, session: &str, recorded: &Recorded) {
    let Some(state) = app.try_state::<crate::AppState>() else {
        return;
    };
    let Some(announcement) = announcement(&state, session) else {
        return;
    };
    if let Err(error) = app.emit(
        EVENT_NEW,
        serde_json::json!({ "session": announcement.session, "row": &recorded.row }),
    ) {
        log::debug!("notifications: emit failed: {error}");
    }
    emit_unread(app, &announcement.session, recorded.unread);
    if should_interrupt(app, &recorded.row) {
        os_banner(app, &announcement, &recorded.row);
    }
}

/// Emit the unread count after anything that changes it, under `session`,
/// the session of the lease that changed it. Dropped once that session is no
/// longer current, so the next profile's badge is never overwritten.
pub fn emit_changed<R: Runtime>(app: &AppHandle<R>, session: &str, unread: u32) {
    let current = app
        .try_state::<crate::AppState>()
        .is_some_and(|state| state.profile_operations.admit(session).is_ok());
    if current {
        emit_unread(app, session, unread);
    }
}

fn emit_unread<R: Runtime>(app: &AppHandle<R>, session: &str, unread: u32) {
    let _ = app.emit(
        EVENT_CHANGED,
        serde_json::json!({ "session": session, "unread": unread }),
    );
}

/// Publish an already persisted and locally resolved tap.
pub fn emit_open<R: Runtime>(app: &AppHandle<R>, intent: &NotificationOpen) {
    let _ = app.emit(EVENT_OPEN, intent);
}

/// Whether a banner is warranted: on a phone always, on the desktop only
/// while the app's window is not the one in front. A banner over the very
/// screen that just updated is noise.
///
/// A push on iOS is the exception: iOS already showed the system banner
/// before handing the payload over, so showing another would double it.
fn should_interrupt<R: Runtime>(app: &AppHandle<R>, row: &NotificationRow) -> bool {
    if cfg!(target_os = "ios") && row.source == NotificationSource::Push {
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

fn os_banner<R: Runtime>(app: &AppHandle<R>, announcement: &Announcement, row: &NotificationRow) {
    // Absent only where no notification plugin is installed (test apps).
    let Some(notification) = app.try_state::<tauri_plugin_notification::Notification<R>>() else {
        return;
    };
    let Ok(network) = recipients::network() else {
        return;
    };
    let mut builder = notification
        .builder()
        .title(&row.title)
        .body(&row.body)
        .extra("notificationId", &row.id)
        .extra("profileId", announcement.profile_id.as_str())
        .extra("networkId", network);
    if let Some(route) = &row.route {
        // Keep the locally discovered/registered origin even if the learner
        // clears this row before tapping its OS banner.
        let origin = row
            .payload
            .as_ref()
            .and_then(|p| p.get("cloud_origin"))
            .and_then(serde_json::Value::as_str);
        let destination = match origin {
            Some(origin) if row.kind == NotificationKind::InterviewRoomOpened => format!(
                "{route}?cloud={}",
                url::form_urlencoded::byte_serialize(origin.as_bytes()).collect::<String>()
            ),
            _ => route.clone(),
        };
        builder = builder.extra("route", destination);
    }
    #[cfg(target_os = "android")]
    {
        builder = builder.channel_id(ANDROID_CHANNEL);
    }
    if let Err(error) = builder.show() {
        log::debug!("notifications: OS banner failed: {error}");
    }
}

/// The Android channel's name and description, as system notification
/// settings show them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelText {
    pub name: &'static str,
    pub description: &'static str,
}

/// The channel text for a `user.language` preference. `system` (or nothing)
/// follows the device language; an unsupported language reads in English.
pub fn channel_text(preference: &str) -> ChannelText {
    let preference = preference.trim();
    let locale = if preference.is_empty() || preference == "system" {
        tauri_plugin_os::locale().unwrap_or_default()
    } else {
        preference.to_string()
    };
    let (name, description) = copy::channel(&locale);
    ChannelText { name, description }
}

/// Create the Android channel, or rename it in place: Android keeps a
/// channel's importance and the learner's choices for it, and takes a new
/// name and description for the same id. Elsewhere there is no channel.
#[cfg_attr(not(target_os = "android"), allow(unused_variables))]
fn apply_channel<R: Runtime>(app: &AppHandle<R>, text: ChannelText) {
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_notification::{Channel, Importance};
        let channel = Channel::builder(ANDROID_CHANNEL, text.name)
            .description(text.description)
            .importance(Importance::High)
            .build();
        if let Err(error) = app.notification().create_channel(channel) {
            log::warn!("notifications: could not create the Android channel: {error}");
        }
    }
}

/// Write the channel in the unlocked profile's language. Called when a
/// profile unlocks and when its language changes.
pub async fn refresh_channel<R: Runtime>(
    app: &AppHandle<R>,
    lease: crate::profile::scope::ProfileLease,
) -> Result<ChannelText, NotificationError> {
    let preference = app
        .state::<crate::AppState>()
        .db_executor
        .execute(
            crate::db::executor::DatabaseWorkload::Learner,
            lease.clone(),
            "notifications.channel",
            |db| Ok(SettingsStore::get(db.conn(), keys::USER_LANGUAGE)),
        )
        .await
        .map_err(NotificationError::Profile)?;
    let text = channel_text(&preference);
    if lease.is_current() {
        apply_channel(app, text);
    }
    Ok(text)
}

/// Refresh the channel after a settings write that changes its language.
/// `None` when the key does not affect it.
pub async fn settings_changed<R: Runtime>(
    app: &AppHandle<R>,
    lease: crate::profile::scope::ProfileLease,
    key: &str,
) -> Result<Option<ChannelText>, NotificationError> {
    if key != keys::USER_LANGUAGE.key {
        return Ok(None);
    }
    refresh_channel(app, lease).await.map(Some)
}

/// Wire the OS side up once the plugins exist: the Android channel and the
/// push plugin's events. Called from the app's setup hook.
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    // No profile is unlocked yet. A channel left by an earlier run keeps the
    // language of the profile that last wrote it; a first run gets the
    // device language until a profile unlocks.
    #[cfg(target_os = "android")]
    {
        let exists = app
            .notification()
            .list_channels()
            .is_ok_and(|channels| channels.iter().any(|c| c.id() == ANDROID_CHANNEL));
        if !exists {
            apply_channel(app, channel_text("system"));
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

    pub(super) type Heard = std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>;

    /// A test app over an unlocked "Learner" profile, and every
    /// `notifications:new` / `notifications:changed` payload it emits.
    pub(super) async fn listening_app(
        directory: &std::path::Path,
    ) -> (tauri::App<tauri::test::MockRuntime>, Heard) {
        use tauri::Listener;
        let state = push::boundary_tests::active(directory).await;
        let state = std::sync::Arc::try_unwrap(state).unwrap_or_else(|_| panic!("unique state"));
        let app = tauri::test::mock_builder()
            .manage(state)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let heard = Heard::default();
        for event in [EVENT_NEW, EVENT_CHANGED] {
            let heard = heard.clone();
            app.listen_any(event, move |event| {
                let payload = serde_json::from_str(event.payload()).unwrap();
                heard.lock().unwrap().push(payload);
            });
        }
        (app, heard)
    }

    pub(super) async fn in_profile<T: Send + 'static, E: std::fmt::Display>(
        state: &crate::AppState,
        job: impl FnOnce(&crate::db::Database) -> Result<T, E> + Send + 'static,
    ) -> T {
        let lease = state
            .profile_operations
            .admit(&state.profile_operations.session().unwrap())
            .unwrap();
        state
            .db_executor
            .execute(
                crate::db::executor::DatabaseWorkload::Background,
                lease,
                "test.notifications",
                move |db| job(db).map_err(|e| e.to_string()),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_profile_switch_before_announcing_drops_the_event_and_banner_and_keeps_the_row_home()
    {
        let directory = tempfile::tempdir().unwrap();
        let (app, heard) = listening_app(directory.path()).await;
        let state = app.state::<crate::AppState>();
        let home = state.active_id().unwrap();
        let session = state.profile_operations.session().unwrap();
        let recorded = in_profile(&state, |db| {
            record_if_enabled(db.conn(), &for_issued_credential("c1"))
        })
        .await
        .unwrap();
        assert_eq!(
            announcement(&state, &session),
            Some(Announcement {
                session: session.clone(),
                profile_id: home.clone(),
            }),
            "while the recording profile is current, banners are tagged with it"
        );
        announce(app.handle(), &session, &recorded);
        assert_eq!(
            std::mem::take(&mut *heard.lock().unwrap()),
            vec![
                serde_json::json!({ "session": session, "row": recorded.row }),
                serde_json::json!({ "session": session, "unread": 1 }),
            ]
        );

        push::boundary_tests::switch_to(&state, "B").await;
        assert_ne!(state.active_id().unwrap(), home);
        assert_eq!(
            announcement(&state, &session),
            None,
            "no banner may be built for the profile that took over"
        );
        announce(app.handle(), &session, &recorded);
        assert!(
            heard.lock().unwrap().is_empty(),
            "{:?}",
            heard.lock().unwrap()
        );
        assert!(in_profile(&state, |db| list(db.conn(), None, false))
            .await
            .is_empty());

        push::boundary_tests::switch_to(&state, "Learner").await;
        assert_eq!(
            in_profile(&state, |db| list(db.conn(), None, false)).await,
            vec![recorded.row]
        );
    }

    #[tokio::test]
    async fn count_updates_carry_the_acting_session_and_never_the_next_profiles() {
        let directory = tempfile::tempdir().unwrap();
        let (app, heard) = listening_app(directory.path()).await;
        let state = app.state::<crate::AppState>();
        let acting = state.profile_operations.session().unwrap();
        in_profile(&state, |db| {
            record_if_enabled(db.conn(), &for_issued_credential("c1"))
        })
        .await
        .unwrap();
        let unread = in_profile(&state, |db| {
            mark_all_read(db.conn(), "2026-11-04T00:00:00Z")?;
            unread_count(db.conn())
        })
        .await;
        emit_changed(app.handle(), &acting, unread);
        assert_eq!(
            std::mem::take(&mut *heard.lock().unwrap()),
            vec![serde_json::json!({ "session": acting, "unread": 0 })]
        );

        // Cleared, then a switch lands before the count update goes out.
        in_profile(&state, |db| clear(db.conn())).await;
        push::boundary_tests::switch_to(&state, "B").await;
        let next = state.profile_operations.session().unwrap();
        emit_changed(app.handle(), &acting, 0);
        let heard = heard.lock().unwrap();
        assert!(
            heard.iter().all(|event| event["session"] != next.as_str()),
            "the next profile's badge must not be overwritten: {heard:?}"
        );
    }

    #[test]
    fn the_webview_receives_rows_with_snake_case_fields_and_typed_vocabulary() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let row = record_if_enabled(db.conn(), &for_issued_credential("c1"))
            .unwrap()
            .unwrap()
            .row;
        let wire = serde_json::to_value(&row).unwrap();
        let mut keys: Vec<&str> = wire
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "body",
                "created_at",
                "id",
                "kind",
                "payload",
                "read_at",
                "route",
                "source",
                "title"
            ]
        );
        assert_eq!(wire["kind"], "credential_issued");
        assert_eq!(wire["source"], "local");
        assert_eq!(wire["payload"], serde_json::json!({ "id": "c1" }));
    }

    #[test]
    fn the_android_channel_reads_in_the_chosen_language_and_falls_back_to_english() {
        assert_eq!(
            channel_text("es"),
            ChannelText {
                name: "Actividad",
                description: "Entrevistas, ofertas, solicitudes y actividad de las clases",
            }
        );
        assert_eq!(channel_text("zh-Hans").name, "动态");
        assert_eq!(
            channel_text("xx"),
            ChannelText {
                name: "Activity",
                description: "Interviews, offers, requests and classroom activity",
            }
        );
    }

    #[tokio::test]
    async fn changing_the_profile_language_refreshes_the_android_channel_text() {
        let directory = tempfile::tempdir().unwrap();
        let (app, _) = listening_app(directory.path()).await;
        let state = app.state::<crate::AppState>();
        let lease = || {
            state
                .profile_operations
                .admit(&state.profile_operations.session().unwrap())
                .unwrap()
        };
        in_profile(&state, |db| {
            SettingsStore::set(db.conn(), keys::USER_LANGUAGE, "fr".to_string())
                .map_err(|e| e.to_string())
        })
        .await;
        assert_eq!(
            settings_changed(app.handle(), lease(), keys::USER_LANGUAGE.key)
                .await
                .unwrap(),
            Some(ChannelText {
                name: "Activité",
                description: "Entretiens, offres, demandes et activité des classes",
            })
        );
        assert_eq!(
            settings_changed(app.handle(), lease(), keys::UI_THEME.key)
                .await
                .unwrap(),
            None,
            "other settings leave the channel alone"
        );
        assert_eq!(
            refresh_channel(app.handle(), lease()).await.unwrap().name,
            "Activité",
            "unlocking creates the channel in the profile's language"
        );
    }

    #[test]
    fn recording_uses_profile_language_for_generic_banner_copy() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        SettingsStore::set(db.conn(), keys::USER_LANGUAGE, "fr".to_string()).unwrap();
        let recorded = record_if_enabled(db.conn(), &for_issued_credential("credential"))
            .unwrap()
            .unwrap();
        assert_eq!(recorded.row.title, "Justificatif délivré");
        assert_eq!(
            recorded.row.body,
            "Un nouveau justificatif a été ajouté à votre portefeuille."
        );
        assert_eq!(list(db.conn(), None, false).unwrap()[0], recorded.row);
    }

    #[test]
    fn recorded_cloud_copy_matches_contract_with_locale_normalization_and_no_payload_text() {
        let contract: serde_json::Value = serde_json::from_str(include_str!(
            "../../../src/i18n/__tests__/fixtures/notification-copy-contract.json"
        ))
        .unwrap();
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let mut cases: Vec<(String, String)> = contract["locales"]
            .as_object()
            .unwrap()
            .keys()
            .map(|locale| (locale.clone(), locale.clone()))
            .collect();
        cases.extend(
            contract["normalization"]["examples"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(locale, expected)| (locale.clone(), expected.as_str().unwrap().into())),
        );
        for (locale, expected) in cases {
            SettingsStore::set(db.conn(), keys::USER_LANGUAGE, locale.clone()).unwrap();
            for (kind, copy) in contract["locales"][expected].as_object().unwrap() {
                let new = NewNotification::new(
                    NotificationKind::parse(kind).unwrap(),
                    "Private person",
                    "Private organisation did:key:secret",
                )
                .from_push();
                let row = record_if_enabled(db.conn(), &new).unwrap().unwrap().row;
                assert_eq!(
                    row.title,
                    copy["title"].as_str().unwrap(),
                    "{locale}/{kind}"
                );
                assert_eq!(row.body, copy["body"].as_str().unwrap(), "{locale}/{kind}");
            }
        }
    }

    #[test]
    fn every_local_kind_records_generic_copy() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        SettingsStore::set(db.conn(), keys::USER_LANGUAGE, "fr".to_string()).unwrap();
        for (kind, expected) in [
            (
                NotificationKind::ClassroomCallStarted,
                "Un appel de classe a commencé",
            ),
            (
                NotificationKind::ClassroomJoinRequested,
                "Demande d’adhésion",
            ),
            (NotificationKind::CredentialIssued, "Justificatif délivré"),
            (
                NotificationKind::GuardianRequest,
                "Un responsable légal est lié à votre compte",
            ),
            (NotificationKind::Generic, "Nouvelle activité"),
        ] {
            let row = record_if_enabled(
                db.conn(),
                &NewNotification::new(kind, "Private person", "Private organisation"),
            )
            .unwrap()
            .unwrap()
            .row;
            assert_eq!(row.title, expected);
            assert!(!row.body.contains("Private"));
        }
    }

    #[test]
    fn disabled_recording_preserves_history_without_claiming_new_events() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let first = for_issued_credential("first");
        record_if_enabled(db.conn(), &first).unwrap().unwrap();
        SettingsStore::set(db.conn(), keys::NOTIFICATIONS_ENABLED, false).unwrap();
        let next = for_issued_credential("next");
        assert!(record_if_enabled(db.conn(), &next).unwrap().is_none());
        assert_eq!(list(db.conn(), None, false).unwrap().len(), 1);
        assert_eq!(unread_count(db.conn()).unwrap(), 1);
        SettingsStore::set(db.conn(), keys::NOTIFICATIONS_ENABLED, true).unwrap();
        assert!(record_if_enabled(db.conn(), &next).unwrap().is_some());
        mark_all_read(db.conn(), "read").unwrap();
        assert_eq!(unread_count(db.conn()).unwrap(), 0);
        clear(db.conn()).unwrap();
        assert!(record_if_enabled(db.conn(), &first).unwrap().is_none());
        assert!(record_if_enabled(db.conn(), &next).unwrap().is_none());
    }

    #[test]
    fn cloud_copy_names_a_route_per_kind() {
        let (title, _, route) = cloud_copy(NotificationKind::InterviewRoomOpened, "abc").unwrap();
        assert_eq!(title, "Your interview room is open");
        assert_eq!(route, "/interview-room/abc");
        assert_eq!(
            cloud_copy(NotificationKind::OfferSent, "x").unwrap().2,
            "/settings/organisations"
        );
        assert!(cloud_copy(NotificationKind::ClassroomCallStarted, "x").is_none());
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
        assert_eq!(n.kind, NotificationKind::ClassroomCallStarted);
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
