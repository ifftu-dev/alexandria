// SPDX-License-Identifier: AGPL-3.0-or-later
//! Asking Alexandria Cloud what is waiting for the learner, so the app can
//! say so without the learner opening the organisations page.
//!
//! Runs from the profile's background pass. It reads the same four lists the
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
use rusqlite::Connection;
use serde_json::Value;

use super::{cloud_copy, kind, NewNotification};
use crate::commands::holder_pull::{signed_get, Directory};

/// One thing Cloud is holding for the learner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingItem {
    pub kind: &'static str,
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
        payload["eventId"] = Value::String(self.event_id.clone());
        if let Some(origin) = &self.cloud_origin {
            payload["cloudOrigin"] = Value::String(origin.clone());
        }
        if let Some(role) = &self.role_label {
            payload["roleLabel"] = Value::String(role.clone());
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
const LISTS: &[(&str, &str, &str)] = &[
    ("/api/interviews/for/", kind::INTERVIEW_INVITED, "invite"),
    ("/api/offers/for/", kind::OFFER_SENT, "offer"),
    (
        "/api/access-requests/for/",
        kind::ACCESS_REQUESTED,
        "request",
    ),
    (
        "/api/assessment-requests/for/",
        kind::ASSESSMENT_REQUESTED,
        "request",
    ),
    (
        "/api/interviews/rooms/for/",
        kind::INTERVIEW_ROOM_OPENED,
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
    kind: &'static str,
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
        .take(if kind == kind::INTERVIEW_ROOM_OPENED {
            100
        } else {
            1000
        })
        .filter_map(|envelope| {
            if kind == kind::INTERVIEW_ROOM_OPENED {
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
                    || envelope.get("kind")?.as_str()? != kind
                    || envelope.get("route")?.as_str()? != format!("/interview-room/{id}")
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
                Ok(body) => parse_items(kind, field, &body, did, chrono::Utc::now().timestamp()),
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
    if item.kind != kind::INTERVIEW_INVITED && item.kind != kind::OFFER_SENT {
        return false;
    }
    conn.query_row(
        "SELECT 1 FROM hiring_responses WHERE id = ?1",
        rusqlite::params![item.id],
        |_| Ok(()),
    )
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert!(items.iter().any(|item| item.kind == kind));
        }
        let new = items
            .iter()
            .find(|item| item.kind == kind::INTERVIEW_ROOM_OPENED)
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
        assert_eq!(new.payload.as_ref().unwrap()["cloudOrigin"], origin);
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
                kind::INTERVIEW_ROOM_OPENED,
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
        let items = parse_items(kind::INTERVIEW_INVITED, "invite", &interviews(), DID, 1_000);
        assert_eq!(
            items,
            vec![PendingItem {
                kind: kind::INTERVIEW_INVITED,
                id: "11111111-1111-4111-8111-111111111111".into(),
                role_label: Some("Backend engineer".into()),
                event_id: "interview_invited:11111111-1111-4111-8111-111111111111".into(),
                cloud_origin: None,
                expires_at: 4_102_444_800,
            }]
        );
        assert!(parse_items(kind::OFFER_SENT, "offer", &serde_json::json!({}), DID, 0).is_empty());
    }

    #[test]
    fn an_item_becomes_a_deduplicated_notification() {
        let item = PendingItem {
            kind: kind::ACCESS_REQUESTED,
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
            Some(serde_json::json!({ "id": "req-9", "eventId": "access_requested:req-9" }))
        );
    }

    #[test]
    fn unreported_skips_seen_and_answered() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let conn = db.conn();
        let fresh = PendingItem {
            kind: kind::OFFER_SENT,
            id: "o-1".into(),
            role_label: None,
            event_id: "offer_sent:o-1".into(),
            cloud_origin: None,
            expires_at: i64::MAX,
        };
        let seen = PendingItem {
            kind: kind::OFFER_SENT,
            id: "o-2".into(),
            role_label: None,
            event_id: "offer_sent:o-2".into(),
            cloud_origin: None,
            expires_at: i64::MAX,
        };
        let answered = PendingItem {
            kind: kind::OFFER_SENT,
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
