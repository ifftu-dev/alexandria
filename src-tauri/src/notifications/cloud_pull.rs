// SPDX-License-Identifier: AGPL-3.0-or-later
//! Asking Alexandria Cloud what is waiting for the learner, so the app can
//! say so without the learner opening the organisations page.
//!
//! Runs from the profile's background pass, and on a phone as soon as the app
//! returns to the foreground (see [`discover`]). It reads the same four lists the
//! organisations page reads — interview invitations, offers, access requests
//! and assessment requests — plus currently live accepted rooms. It records
//! a notification for each event it has
//! not reported before. It parses only what it needs (an id, an expiry, a
//! role label) rather than the full signed envelopes: whether an item is
//! *trusted* is the page's job, decided with the organisation's proof when
//! the learner looks; whether it is *new* is all this needs to know.
//!
//! A push from Cloud covers the same events faster when the device is
//! registered for one. This pass is what covers the desktop, a phone that
//! declined push, and anything a push missed.

use ed25519_dalek::SigningKey;
use futures::FutureExt;
use rusqlite::Connection;
use serde_json::Value;
use tauri::{AppHandle, Manager, Runtime};

use super::{
    cloud_copy, interview_room_route, NewNotification, NotificationError, NotificationKind,
};
use crate::cloud::{directories_db, signed_get, Directory};
use crate::db::executor::DatabaseWorkload;
use crate::profile::scope::ProfileLease;

/// One thing Cloud is holding for the learner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingItem {
    pub kind: NotificationKind,
    pub id: String,
    pub role_label: Option<String>,
    pub event_id: String,
    pub cloud_origin: Option<String>,
    pub expires_at: i64,
}

impl PendingItem {
    pub fn dedupe_key(&self) -> String {
        self.event_id.clone()
    }

    pub fn notification(&self) -> Option<NewNotification> {
        let (title, body, route) = cloud_copy(self.kind, &self.id)?;
        let mut payload = serde_json::json!({ "id": self.id });
        payload["event_id"] = Value::String(self.event_id.clone());
        if let Some(origin) = &self.cloud_origin {
            payload["cloud_origin"] = Value::String(origin.clone());
        }
        if let Some(role) = &self.role_label {
            payload["role_label"] = Value::String(role.clone());
        }
        Some(
            NewNotification::new(self.kind, title, body)
                .route(route)
                .dedupe(self.dedupe_key())
                .payload(payload),
        )
    }
}

/// The lists to read: the path under the learner's DID, the kind each item
/// becomes, and the envelope field the item sits under.
const LISTS: &[(&str, NotificationKind, &str)] = &[
    (
        "/api/interviews/for/",
        NotificationKind::InterviewInvited,
        "invite",
    ),
    ("/api/offers/for/", NotificationKind::OfferSent, "offer"),
    (
        "/api/access-requests/for/",
        NotificationKind::AccessRequested,
        "request",
    ),
    (
        "/api/assessment-requests/for/",
        NotificationKind::AssessmentRequested,
        "request",
    ),
    (
        "/api/interviews/rooms/for/",
        NotificationKind::InterviewRoomOpened,
        "",
    ),
];

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
}

/// The items in one list's response that are for `did` and not yet expired.
///
/// Lenient on purpose: an envelope this cannot read is skipped, not an error,
/// because one malformed item must not hide the others.
pub fn parse_items(
    kind: NotificationKind,
    field: &str,
    body: &Value,
    did: &str,
    now: i64,
) -> Vec<PendingItem> {
    let Some(envelopes) = body.as_array() else {
        return Vec::new();
    };
    envelopes
        .iter()
        .take(if kind == NotificationKind::InterviewRoomOpened {
            100
        } else {
            1000
        })
        .filter_map(|envelope| {
            if kind == NotificationKind::InterviewRoomOpened {
                let id = envelope.get("id")?.as_str()?;
                let opening = envelope.get("openingId")?.as_str()?;
                let event_id = envelope.get("eventId")?.as_str()?;
                let opened =
                    chrono::DateTime::parse_from_rfc3339(envelope.get("openedAt")?.as_str()?)
                        .ok()?
                        .timestamp();
                let expires =
                    chrono::DateTime::parse_from_rfc3339(envelope.get("expiresAt")?.as_str()?)
                        .ok()?
                        .timestamp();
                if !canonical_uuid(id)
                    || !canonical_uuid(opening)
                    || event_id != format!("interview_room_opened:{id}:{opening}")
                    || envelope.get("kind")?.as_str()? != kind.as_str()
                    || envelope.get("route")?.as_str()? != interview_room_route(id)
                    || expires <= now
                    || opened > now
                    || opened >= expires
                {
                    return None;
                }
                return Some(PendingItem {
                    kind,
                    id: id.into(),
                    event_id: event_id.into(),
                    role_label: None,
                    cloud_origin: None,
                    expires_at: expires,
                });
            }
            let item = envelope.get(field)?;
            let id = item.get("id")?.as_str()?;
            if !canonical_uuid(id) {
                return None;
            }
            let event_id = envelope.get("eventId")?.as_str()?;
            if event_id != format!("{kind}:{id}") {
                return None;
            }
            if item.get("subject_did")?.as_str()? != did {
                return None;
            }
            let expires_at = item.get("expires_at")?.as_i64()?;
            if expires_at <= now {
                return None;
            }
            Some(PendingItem {
                kind,
                id: id.to_string(),
                event_id: event_id.into(),
                cloud_origin: None,
                expires_at,
                role_label: item
                    .get("role_label")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect()
}

/// Read every list from `origin`. Each list is independent; one that fails
/// is logged and the others still count.
pub async fn fetch(origin: &str, signing_key: &SigningKey, did: &str) -> Vec<PendingItem> {
    let Ok(client) = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
    else {
        return Vec::new();
    };
    let directory = Directory {
        name: "Alexandria Cloud".into(),
        url: origin.to_string(),
    };
    let lists = futures::future::join_all(LISTS.iter().map(|(prefix, kind, field)| {
        let client = &client;
        let directory = &directory;
        async move {
            let path = format!("{prefix}{did}");
            match signed_get(client, directory, signing_key, &path).await {
                Ok(body) => parse_items(*kind, field, &body, did, chrono::Utc::now().timestamp()),
                Err(error) => {
                    log::debug!("notifications: {kind} pull: {error}");
                    Vec::new()
                }
            }
        }
    }))
    .await;
    lists
        .into_iter()
        .flatten()
        .map(|mut item| {
            item.cloud_origin = Some(origin.into());
            item
        })
        .collect()
}

/// The items worth recording: not already reported, and not an invitation
/// or offer the learner has already answered (the page hides those, so a
/// notification about one would point at nothing).
pub fn unreported(conn: &Connection, items: &[PendingItem]) -> Vec<PendingItem> {
    items
        .iter()
        .filter(|item| item.expires_at > chrono::Utc::now().timestamp())
        .filter(|item| !super::store::seen(conn, &item.dedupe_key()).unwrap_or(true))
        .filter(|item| !answered(conn, item))
        .cloned()
        .collect()
}

fn answered(conn: &Connection, item: &PendingItem) -> bool {
    if item.kind != NotificationKind::InterviewInvited && item.kind != NotificationKind::OfferSent {
        return false;
    }
    conn.query_row(
        "SELECT 1 FROM hiring_responses WHERE id = ?1",
        rusqlite::params![item.id],
        |_| Ok(()),
    )
    .is_ok()
}

/// A discovery pass's error. Callers that join a running pass share its
/// result, so the error is shared too.
pub type DiscoveryError = std::sync::Arc<NotificationError>;
type Outcome = Result<usize, DiscoveryError>;
type Pass = futures::future::Shared<futures::future::BoxFuture<'static, Outcome>>;

/// The discovery pass in flight, if any, and the profile session it runs for.
#[derive(Clone, Default)]
pub(crate) struct Passes(std::sync::Arc<std::sync::Mutex<Option<(String, Pass)>>>);

/// Run one discovery pass for the profile `lease` admits, from the background
/// loop or when a phone comes back to the foreground. Returns how many
/// notifications the pass recorded.
///
/// One pass runs per profile session at a time. A caller arriving while a
/// pass for the same session is running joins that pass and shares its
/// result, so Cloud is not asked twice; a pass for another session starts
/// its own and may run beside the earlier one until that one ends. Rows still
/// go through the dedupe keys, so a push and a pass for the same event record
/// it once.
pub async fn discover<R: Runtime>(app: &AppHandle<R>, lease: ProfileLease) -> Outcome {
    let state = app
        .try_state::<crate::AppState>()
        .ok_or(NotificationError::AppStateUnavailable)?;
    let pass = {
        let mut running = state
            .cloud_discovery
            .0
            .lock()
            .map_err(NotificationError::from)?;
        match running.as_ref() {
            Some((session, pass)) if session == lease.session_id() && pass.peek().is_none() => {
                pass.clone()
            }
            _ => {
                // The pass runs as its own task, so it finishes even if every
                // caller stops waiting, and it ends at once when the profile
                // locks: the slot never holds a lease nobody polls.
                let session = lease.session_id().to_owned();
                let task = tokio::spawn({
                    let app = app.clone();
                    let lease = lease.clone();
                    async move {
                        lease
                            .clone()
                            .run_until_closed(run_pass(app, lease))
                            .await
                            .unwrap_or(Err(NotificationError::SessionChanged))
                    }
                });
                let pass = async move {
                    task.await
                        .unwrap_or_else(|error| {
                            Err(NotificationError::DiscoveryTask(error.to_string()))
                        })
                        .map_err(std::sync::Arc::new)
                }
                .boxed()
                .shared();
                *running = Some((session, pass.clone()));
                pass
            }
        }
    };
    drop(lease);
    pass.await
}

/// Ask Cloud what is waiting, record what has not been reported, and announce
/// it under the lease's session. Nothing to do without a Cloud origin (or with
/// organisations off) or while the vault is locked.
async fn run_pass<R: Runtime>(
    app: AppHandle<R>,
    lease: ProfileLease,
) -> Result<usize, NotificationError> {
    let state = app
        .try_state::<crate::AppState>()
        .ok_or(NotificationError::AppStateUnavailable)?;
    let origin = state
        .db_executor
        .execute(
            DatabaseWorkload::Background,
            lease.clone(),
            "notifications.discovery.origin",
            |db| Ok(directories_db(db.conn())?.into_iter().next().map(|d| d.url)),
        )
        .await
        .map_err(NotificationError::Profile)?;
    let Some(origin) = origin else {
        return Ok(0);
    };
    let (signing_key, did) = super::push::issuer(&state).await?.into_parts();
    let items = fetch(&origin, &signing_key, did.as_str()).await;
    if items.is_empty() {
        return Ok(0);
    }
    let recorded = state
        .db_executor
        .execute(
            DatabaseWorkload::Background,
            lease.clone(),
            "notifications.discovery.record",
            move |db| {
                let mut recorded = Vec::new();
                for item in unreported(db.conn(), &items) {
                    let Some(notification) = item.notification() else {
                        continue;
                    };
                    match super::record_if_enabled(db.conn(), &notification) {
                        Ok(Some(row)) => recorded.push(row),
                        Ok(None) => {}
                        Err(error) => log::debug!("cloud notification: {error}"),
                    }
                }
                Ok(recorded)
            },
        )
        .await
        .map_err(NotificationError::Profile)?;
    for row in &recorded {
        super::announce(&app, lease.session_id(), row);
    }
    Ok(recorded.len())
}

#[cfg(test)]
mod tests {
    use super::super::tests::{in_profile, listening_app};
    use super::*;
    use crate::settings::{registry::keys, SettingsStore};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tauri::Manager;

    const INVITE: &str = "33333333-3333-4333-8333-333333333333";

    /// A fake Alexandria Cloud holding one interview invitation for `did`.
    /// It counts the requests it receives and answers none of them until
    /// `release` turns true.
    async fn fake_cloud(
        did: String,
        release: tokio::sync::watch::Receiver<bool>,
    ) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let received = Arc::new(AtomicUsize::new(0));
        let counter = received.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let did = did.clone();
                let mut release = release.clone();
                let counter = counter.clone();
                tokio::spawn(async move {
                    let mut buffer = [0; 8192];
                    let n = socket.read(&mut buffer).await.unwrap();
                    let request = String::from_utf8_lossy(&buffer[..n]).into_owned();
                    counter.fetch_add(1, Ordering::SeqCst);
                    release.wait_for(|released| *released).await.unwrap();
                    let body = if request.starts_with("GET /api/interviews/for/") {
                        serde_json::json!([{
                            "eventId": format!("interview_invited:{INVITE}"),
                            "invite": { "id": INVITE, "subject_did": did, "expires_at": 4_102_444_800_i64 }
                        }])
                    } else {
                        serde_json::json!([])
                    }
                    .to_string();
                    let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    socket.write_all(response.as_bytes()).await.unwrap();
                });
            }
        });
        (origin, received, server)
    }

    /// An unlocked profile pointed at a fake Cloud holding one invitation.
    async fn learner_with_invitation(
        directory: &std::path::Path,
        released: bool,
    ) -> (
        tauri::App<tauri::test::MockRuntime>,
        super::super::tests::Heard,
        Arc<AtomicUsize>,
        tokio::sync::watch::Sender<bool>,
        tokio::task::JoinHandle<()>,
    ) {
        let (app, heard) = listening_app(directory).await;
        let state = app.state::<crate::AppState>();
        let did = super::super::push::issuer(&state)
            .await
            .unwrap()
            .did()
            .clone();
        let (release, gate) = tokio::sync::watch::channel(released);
        let (origin, received, server) = fake_cloud(did.as_str().to_string(), gate).await;
        in_profile(&state, move |db| {
            SettingsStore::set(db.conn(), keys::CLOUD_ORIGIN, origin).map_err(|e| e.to_string())
        })
        .await;
        (app, heard, received, release, server)
    }

    fn admitted(state: &crate::AppState) -> ProfileLease {
        state
            .profile_operations
            .admit(&state.profile_operations.session().unwrap())
            .unwrap()
    }

    #[tokio::test]
    async fn a_resume_pass_records_a_missed_invitation_without_waiting_for_the_loop() {
        let directory = tempfile::tempdir().unwrap();
        let (app, heard, _, _release, server) =
            learner_with_invitation(directory.path(), true).await;
        let state = app.state::<crate::AppState>();
        let session = state.profile_operations.session().unwrap();

        assert_eq!(discover(app.handle(), admitted(&state)).await.unwrap(), 1);

        let rows = in_profile(&state, |db| super::super::list(db.conn(), None, false)).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, NotificationKind::InterviewInvited);
        assert_eq!(rows[0].route.as_deref(), Some("/settings/organisations"));
        assert!(heard
            .lock()
            .unwrap()
            .iter()
            .any(|event| event["session"] == session.as_str()
                && event["row"]["id"] == rows[0].id.as_str()));
        server.abort();
    }

    #[tokio::test]
    async fn a_pass_with_the_vault_locked_fails_with_the_typed_refusal() {
        let directory = tempfile::tempdir().unwrap();
        let (app, _, received, _release, server) =
            learner_with_invitation(directory.path(), true).await;
        let state = app.state::<crate::AppState>();
        *state.keystore.lock().await = None;

        let error = discover(app.handle(), admitted(&state))
            .await
            .expect_err("no issuer key while the vault is locked");
        assert!(
            matches!(*error, NotificationError::VaultLocked),
            "got {error:?}"
        );
        assert_eq!(received.load(Ordering::SeqCst), 0, "Cloud is not asked");
        server.abort();
    }

    #[tokio::test]
    async fn a_resume_during_a_running_pass_joins_it_without_repeating_requests_or_rows() {
        let directory = tempfile::tempdir().unwrap();
        let (app, _, received, release, server) =
            learner_with_invitation(directory.path(), false).await;
        let state = app.state::<crate::AppState>();
        let lists = LISTS.len();

        let background = tokio::spawn({
            let app = app.handle().clone();
            let lease = admitted(&state);
            async move { discover(&app, lease).await }
        });
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while received.load(Ordering::SeqCst) < lists {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the background pass reaches Cloud");
        let resume = tokio::spawn({
            let app = app.handle().clone();
            let lease = admitted(&state);
            async move { discover(&app, lease).await }
        });
        // Long enough for a second pass to reach the fake Cloud if one started.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert_eq!(
            received.load(Ordering::SeqCst),
            lists,
            "the resume pass must join, not ask Cloud again"
        );
        release.send(true).unwrap();
        assert_eq!(background.await.unwrap().unwrap(), 1);
        assert_eq!(
            resume.await.unwrap().unwrap(),
            1,
            "the joiner shares the result"
        );
        assert_eq!(received.load(Ordering::SeqCst), lists);

        // A later pass is a new one, and de-duplication keeps the row single.
        assert_eq!(discover(app.handle(), admitted(&state)).await.unwrap(), 0);
        assert_eq!(received.load(Ordering::SeqCst), 2 * lists);
        let rows = in_profile(&state, |db| super::super::list(db.conn(), None, false)).await;
        assert_eq!(rows.len(), 1);
        server.abort();
    }

    #[tokio::test]
    async fn authenticated_room_recovery_survives_an_independent_list_failure() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut buffer = [0; 8192];
                    let n = socket.read(&mut buffer).await.unwrap();
                    let request = String::from_utf8_lossy(&buffer[..n]);
                    assert!(request.contains("x-alexandria-proof:"));
                    let room = request.starts_with("GET /api/interviews/rooms/for/");
                    let body = if room {
                        serde_json::json!([{
                        "id": "11111111-1111-4111-8111-111111111111",
                        "openingId": "22222222-2222-4222-8222-222222222222",
                        "eventId": "interview_room_opened:11111111-1111-4111-8111-111111111111:22222222-2222-4222-8222-222222222222",
                        "kind": "interview_room_opened",
                        "route": "/interview-room/11111111-1111-4111-8111-111111111111",
                        "openedAt": "2026-01-01T00:00:00Z", "expiresAt": "2099-01-01T00:00:00Z"
                    }]).to_string()
                    } else {
                        let (kind, field) = if request.starts_with("GET /api/interviews/for/") {
                            ("interview_invited", "invite")
                        } else if request.starts_with("GET /api/access-requests/for/") {
                            ("access_requested", "request")
                        } else if request.starts_with("GET /api/assessment-requests/for/") {
                            ("assessment_requested", "request")
                        } else {
                            ("offer_sent", "offer")
                        };
                        serde_json::json!([{
                            "eventId": format!("{kind}:11111111-1111-4111-8111-111111111111"),
                            (field): { "id": "11111111-1111-4111-8111-111111111111", "subject_did": DID, "expires_at": 4_102_444_800_i64 }
                        }]).to_string()
                    };
                    let status = if request.contains("/api/offers/") {
                        "500 Internal Server Error"
                    } else {
                        "200 OK"
                    };
                    let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    socket.write_all(response.as_bytes()).await.unwrap();
                });
            }
        });
        let key = SigningKey::from_bytes(&[7; 32]);
        let items = fetch(&origin, &key, DID).await;
        server.abort();
        assert_eq!(
            items.len(),
            4,
            "room and request recovery must not depend on offers or push"
        );
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        for kind in [
            "interview_invited",
            "access_requested",
            "assessment_requested",
            "interview_room_opened",
        ] {
            assert!(items.iter().any(|item| item.kind.as_str() == kind));
        }
        let new = items
            .iter()
            .find(|item| item.kind == NotificationKind::InterviewRoomOpened)
            .unwrap()
            .notification()
            .unwrap();
        super::super::store::record(db.conn(), &new, "now").unwrap();
        assert_eq!(
            super::super::store::list(db.conn(), None, false).unwrap()[0]
                .route
                .as_deref(),
            Some("/interview-room/11111111-1111-4111-8111-111111111111")
        );
        assert_eq!(new.dedupe_key.as_deref(), Some("interview_room_opened:11111111-1111-4111-8111-111111111111:22222222-2222-4222-8222-222222222222"));
        assert_eq!(new.payload.as_ref().unwrap()["cloud_origin"], origin);
    }

    #[test]
    fn cleared_room_events_stay_cleared_but_a_new_opening_is_reported() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let item = serde_json::json!({
            "id":"11111111-1111-4111-8111-111111111111", "openingId":"22222222-2222-4222-8222-222222222222",
            "eventId":"interview_room_opened:11111111-1111-4111-8111-111111111111:22222222-2222-4222-8222-222222222222",
            "kind":"interview_room_opened", "route":"/interview-room/11111111-1111-4111-8111-111111111111",
            "openedAt":"2026-01-01T00:00:00Z", "expiresAt":"2099-01-01T00:00:00Z"
        });
        let now = chrono::Utc::now().timestamp();
        let parse = |item: Value| {
            parse_items(
                NotificationKind::InterviewRoomOpened,
                "",
                &serde_json::json!([item]),
                DID,
                now,
            )
        };
        let items = parse(item.clone());
        // Accepted invitations must not hide the independently live room.
        db.conn().execute("INSERT INTO hiring_responses (id,kind,directory_url,organization,role_label,decision,responded_at,payload_json) VALUES ('11111111-1111-4111-8111-111111111111','interview','https://cloud.example','Org','Role','accepted','now','{}')", []).unwrap();
        assert_eq!(unreported(db.conn(), &items).len(), 1);
        let pull = items[0].notification().unwrap();
        let push = pull.clone().from_push();
        for (first, second) in [(&pull, &push), (&push, &pull)] {
            let fresh = crate::db::Database::open_in_memory().unwrap();
            fresh.run_migrations().unwrap();
            assert!(super::super::record(fresh.conn(), first, "now")
                .unwrap()
                .is_some());
            assert!(super::super::record(fresh.conn(), second, "now")
                .unwrap()
                .is_none());
            super::super::clear(fresh.conn()).unwrap();
            assert!(super::super::record(fresh.conn(), first, "now")
                .unwrap()
                .is_none());
            assert_eq!(super::super::unread_count(fresh.conn()).unwrap(), 0);
        }
        super::super::record(db.conn(), &pull, "now").unwrap();
        super::super::clear(db.conn()).unwrap();
        assert!(unreported(db.conn(), &items).is_empty());
        let mut next = item.clone();
        next["openingId"] = Value::String("33333333-3333-4333-8333-333333333333".into());
        next["eventId"] = Value::String("interview_room_opened:11111111-1111-4111-8111-111111111111:33333333-3333-4333-8333-333333333333".into());
        assert_eq!(unreported(db.conn(), &parse(next)).len(), 1);
        for (field, bad) in [
            ("expiresAt", "2026-01-01T00:00:00Z"),
            ("openedAt", "2099-01-02T00:00:00Z"),
            ("openingId", "NOT-A-UUID"),
            ("eventId", "interview_room_opened:foreign"),
            ("kind", "offer_sent"),
            (
                "route",
                "/interview-room/11111111-1111-4111-8111-111111111111?cloud=https://evil.example",
            ),
        ] {
            let mut malformed = item.clone();
            malformed[field] = Value::String(bad.into());
            assert!(parse(malformed).is_empty(), "accepted invalid {field}");
        }
    }

    const DID: &str = "did:key:z6MkLearner";

    fn interviews() -> Value {
        serde_json::json!([
            {
                "invite": {
                    "id": "11111111-1111-4111-8111-111111111111", "subject_did": DID, "role_label": "Backend engineer",
                    "expires_at": 4_102_444_800_i64, "organization": "Acme"
                },
                "eventId": "interview_invited:11111111-1111-4111-8111-111111111111",
                "orgProof": null
            },
            {
                // Expired: skipped.
                "invite": { "id": "int-2", "subject_did": DID, "expires_at": 1 }
            },
            {
                // Somebody else's: skipped.
                "invite": { "id": "int-3", "subject_did": "did:key:zOther", "expires_at": 4_102_444_800_i64 }
            },
            { "invite": { "expires_at": 4_102_444_800_i64 } },
            "garbage"
        ])
    }

    #[test]
    fn parses_live_items_for_this_learner_only() {
        let items = parse_items(
            NotificationKind::InterviewInvited,
            "invite",
            &interviews(),
            DID,
            1_000,
        );
        assert_eq!(
            items,
            vec![PendingItem {
                kind: NotificationKind::InterviewInvited,
                id: "11111111-1111-4111-8111-111111111111".into(),
                role_label: Some("Backend engineer".into()),
                event_id: "interview_invited:11111111-1111-4111-8111-111111111111".into(),
                cloud_origin: None,
                expires_at: 4_102_444_800,
            }]
        );
        assert!(parse_items(
            NotificationKind::OfferSent,
            "offer",
            &serde_json::json!({}),
            DID,
            0
        )
        .is_empty());
    }

    #[test]
    fn an_item_becomes_a_deduplicated_notification() {
        let item = PendingItem {
            kind: NotificationKind::AccessRequested,
            id: "req-9".into(),
            role_label: None,
            event_id: "access_requested:req-9".into(),
            cloud_origin: None,
            expires_at: i64::MAX,
        };
        let n = item.notification().unwrap();
        assert_eq!(n.dedupe_key.as_deref(), Some("access_requested:req-9"));
        assert_eq!(n.route.as_deref(), Some("/settings/organisations"));
        assert_eq!(
            n.payload,
            Some(serde_json::json!({ "id": "req-9", "event_id": "access_requested:req-9" }))
        );
    }

    #[test]
    fn unreported_skips_seen_and_answered() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let conn = db.conn();
        let fresh = PendingItem {
            kind: NotificationKind::OfferSent,
            id: "o-1".into(),
            role_label: None,
            event_id: "offer_sent:o-1".into(),
            cloud_origin: None,
            expires_at: i64::MAX,
        };
        let seen = PendingItem {
            kind: NotificationKind::OfferSent,
            id: "o-2".into(),
            role_label: None,
            event_id: "offer_sent:o-2".into(),
            cloud_origin: None,
            expires_at: i64::MAX,
        };
        let answered = PendingItem {
            kind: NotificationKind::OfferSent,
            id: "o-3".into(),
            role_label: None,
            event_id: "offer_sent:o-3".into(),
            cloud_origin: None,
            expires_at: i64::MAX,
        };
        crate::notifications::store::record(conn, &seen.notification().unwrap(), "t").unwrap();
        conn.execute(
            "INSERT INTO hiring_responses (id,kind,directory_url,organization,role_label,decision,responded_at,payload_json) \
             VALUES ('o-3','offer','https://c','Acme','Role','accepted','t','{}')",
            [],
        )
        .unwrap();
        let left = unreported(conn, &[fresh.clone(), seen, answered]);
        assert_eq!(left, vec![fresh]);
    }
}
