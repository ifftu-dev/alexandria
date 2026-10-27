// SPDX-License-Identifier: AGPL-3.0-or-later
//! Asking Alexandria Cloud what is waiting for the learner, so the app can
//! say so without the learner opening the organisations page.
//!
//! Runs from the profile's background pass. It reads the same four lists the
//! organisations page reads — interview invitations, offers, access requests
//! and assessment requests — and records a notification for each item it has
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
}

impl PendingItem {
    pub fn dedupe_key(&self) -> String {
        format!("{}:{}", self.kind, self.id)
    }

    pub fn notification(&self) -> Option<NewNotification> {
        let (title, body, route) = cloud_copy(self.kind, &self.id)?;
        let mut payload = serde_json::json!({ "id": self.id });
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
];

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
        .filter_map(|envelope| {
            let item = envelope.get(field)?;
            let id = item.get("id")?.as_str()?;
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
    let client = reqwest::Client::new();
    let directory = Directory {
        name: "Alexandria Cloud".into(),
        url: origin.to_string(),
    };
    let now = chrono::Utc::now().timestamp();
    let mut items = Vec::new();
    for (prefix, kind, field) in LISTS {
        let path = format!("{prefix}{did}");
        match signed_get(&client, &directory, signing_key, &path).await {
            Ok(body) => items.extend(parse_items(kind, field, &body, did, now)),
            Err(error) => log::debug!("notifications: {kind} pull: {error}"),
        }
    }
    items
}

/// The items worth recording: not already reported, and not an invitation
/// or offer the learner has already answered (the page hides those, so a
/// notification about one would point at nothing).
pub fn unreported(conn: &Connection, items: &[PendingItem]) -> Vec<PendingItem> {
    items
        .iter()
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

    const DID: &str = "did:key:z6MkLearner";

    fn interviews() -> Value {
        serde_json::json!([
            {
                "invite": {
                    "id": "int-1", "subject_did": DID, "role_label": "Backend engineer",
                    "expires_at": 4_102_444_800_i64, "organization": "Acme"
                },
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
                id: "int-1".into(),
                role_label: Some("Backend engineer".into()),
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
        };
        let n = item.notification().unwrap();
        assert_eq!(n.dedupe_key.as_deref(), Some("access_requested:req-9"));
        assert_eq!(n.route.as_deref(), Some("/settings/organisations"));
        assert_eq!(n.payload, Some(serde_json::json!({ "id": "req-9" })));
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
        };
        let seen = PendingItem {
            kind: kind::OFFER_SENT,
            id: "o-2".into(),
            role_label: None,
        };
        let answered = PendingItem {
            kind: kind::OFFER_SENT,
            id: "o-3".into(),
            role_label: None,
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
