//! Revocation status lists: one Bitstring Status List per issuer per host.
//!
//! Issuing a credential takes the next slot in its issuer's list
//! ([`ensure`], [`allocate`]); revoking it sets that slot's bit. A list
//! named at a host is a URL the host serves, so a verifier can fetch it,
//! and every version the host has not acknowledged is [`pending`] until
//! [`push`] delivers it signed and [`mark_published`] records that.

use ed25519_dalek::SigningKey;
use rusqlite::{params, Connection};

use crate::crypto::did::Did;
use crate::domain::vc::VerifiableCredential;

/// Slots per list: a 16 KiB bitmap.
pub const BITS: usize = alexandria_verify::vc::status::MIN_BITS;
/// The `credentialStatus` type of an entry in a list.
pub const ENTRY_TYPE: &str = "BitstringStatusListEntry";

/// The origin this identity's status lists are served from, if any.
///
/// The `credentials.status_host` setting wins; the Alexandria Cloud the app
/// talks to stands in when it is empty.
/// Only `https`, or `http` on loopback, counts — a list named at a plain
/// `http` host could be swapped on the wire, and a verifier would believe it.
pub fn host(conn: &Connection) -> Option<String> {
    use crate::settings::{registry::keys, SettingsStore};
    let configured = SettingsStore::get(conn, keys::CREDENTIAL_STATUS_HOST);
    let candidate = if !configured.trim().is_empty() {
        Some(configured.trim().to_string())
    } else {
        crate::cloud::origin(conn)
    };
    candidate
        .map(|origin| origin.trim_end_matches('/').to_string())
        .filter(|origin| {
            (origin.starts_with("https://") || crate::cloud::is_loopback(origin))
                && alexandria_verify::vc::status::parse_list_url(
                    &alexandria_verify::vc::status::list_url(
                        origin,
                        &Did("did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK".into()),
                        1,
                    ),
                )
                .is_some()
        })
}

/// The list a credential issued now by `issuer_did` belongs to.
///
/// One list per issuer per host. With a host configured the id is the URL the
/// host serves it at (§14.11.2), so any verifier can fetch it; without one it
/// is a URN and the list travels only in exported bundles. A host that
/// changes later starts a fresh list — ids already written into credentials
/// are never rewritten.
pub fn ensure(conn: &Connection, issuer_did: &Did) -> Result<String, String> {
    let list_id = match host(conn) {
        Some(origin) => alexandria_verify::vc::status::list_url(&origin, issuer_did, 1),
        None => format!("urn:alexandria:status-list:{}:1", issuer_did.as_str()),
    };
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM credential_status_lists WHERE list_id = ?1",
            params![list_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists == 0 {
        let bits = vec![0u8; BITS / 8];
        conn.execute(
            "INSERT INTO credential_status_lists \
             (list_id, issuer_did, version, status_purpose, bits, bit_length) \
             VALUES (?1, ?2, 1, 'revocation', ?3, ?4)",
            params![list_id, issuer_did.as_str(), bits, BITS as i64],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(list_id)
}

pub fn allocate(conn: &Connection, list_id: &str) -> Result<i64, String> {
    // Next free index = max allocated + 1. We read from `credentials`
    // rather than scanning the bitmap because gaps from revocations
    // shouldn't be reused (the revoked state is permanent evidence).
    let next: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(status_list_index), -1) + 1 FROM credentials \
             WHERE status_list_id = ?1",
            params![list_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if next >= BITS as i64 {
        return Err(format!("status list {list_id} is full"));
    }
    Ok(next)
}

/// A URL-addressed list whose host has not yet seen its current version.
#[derive(Debug, Clone)]
pub struct PendingStatusList {
    pub list_id: String,
    pub status_purpose: String,
    pub bits: Vec<u8>,
    pub version: i64,
}

/// Lists this issuer owns that are ahead of what their host acknowledged.
///
/// URN-named lists are never pending: nothing serves them.
pub fn pending(conn: &Connection, issuer_did: &Did) -> Result<Vec<PendingStatusList>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT list_id, status_purpose, bits, version FROM credential_status_lists \
             WHERE issuer_did = ?1 AND version > published_version ORDER BY list_id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![issuer_did.as_str()], |r| {
            Ok(PendingStatusList {
                list_id: r.get(0)?,
                status_purpose: r.get(1)?,
                bits: r.get(2)?,
                version: r.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut pending = Vec::new();
    for row in rows {
        let list = row.map_err(|e| e.to_string())?;
        if alexandria_verify::vc::status::parse_list_url(&list.list_id).is_some() {
            pending.push(list);
        }
    }
    Ok(pending)
}

/// The signed `BitstringStatusListCredential` a host serves for `list`.
pub fn signed(
    list: &PendingStatusList,
    key: &SigningKey,
    issuer_did: &Did,
    now: &str,
) -> Result<VerifiableCredential, String> {
    alexandria_verify::vc::status::status_list_credential(
        &list.list_id,
        issuer_did,
        &list.status_purpose,
        &list.bits,
        now,
        key,
    )
    .map_err(|e| format!("sign status list {}: {e}", list.list_id))
}

/// Record that a host acknowledged `version` of each list.
pub fn mark_published(conn: &Connection, published: &[(String, i64)]) -> Result<(), String> {
    for (list_id, version) in published {
        conn.execute(
            "UPDATE credential_status_lists SET published_version = ?2 \
             WHERE list_id = ?1 AND published_version < ?2",
            params![list_id, version],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// What one publication pass did.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct StatusPublishReport {
    /// List ids the host now serves at their current version.
    pub published: Vec<String>,
    /// One line per list that could not be pushed; the list stays pending.
    pub errors: Vec<String>,
}

/// Push each pending list to its host, signed as of `now`.
///
/// Returns the `(list_id, version)` pairs the host accepted, so the caller
/// can mark them, and the failures, which stay pending for the next pass.
pub async fn push(
    pending: &[PendingStatusList],
    key: &SigningKey,
    issuer_did: &Did,
    now: &str,
) -> (Vec<(String, i64)>, Vec<String>) {
    let mut accepted = Vec::new();
    let mut errors = Vec::new();
    for list in pending {
        match push_one(list, key, issuer_did, now).await {
            Ok(()) => accepted.push((list.list_id.clone(), list.version)),
            Err(e) => errors.push(format!("{}: {e}", list.list_id)),
        }
    }
    (accepted, errors)
}

async fn push_one(
    list: &PendingStatusList,
    key: &SigningKey,
    issuer_did: &Did,
    now: &str,
) -> Result<(), String> {
    let location = alexandria_verify::vc::status::parse_list_url(&list.list_id)
        .ok_or("list is not served by a host")?;
    if &location.issuer != issuer_did {
        return Err("list belongs to another issuer".into());
    }
    let document = signed(list, key, issuer_did, now)?;
    let response = crate::cloud::client()?
        .put(&list.list_id)
        .json(&document)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "{} returned {status}: {}",
            location.origin,
            body.chars().take(200).collect::<String>()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::fixtures::{sample_request, setup, test_key, NOW};
    use crate::credentials::{self, lifecycle};
    use crate::crypto::did::derive_did_key;

    #[test]
    fn only_https_or_loopback_hosts_count() {
        use crate::settings::{registry::keys, SettingsStore};
        let (db, _, _, _) = setup();
        for bad in ["http://cloud.example", "ftp://x", "cloud.example", "   "] {
            SettingsStore::set(db.conn(), keys::CREDENTIAL_STATUS_HOST, bad.to_string()).unwrap();
            assert!(host(db.conn()).is_none(), "{bad} accepted");
        }
        SettingsStore::set(
            db.conn(),
            keys::CREDENTIAL_STATUS_HOST,
            "https://cloud.example/".to_string(),
        )
        .unwrap();
        assert_eq!(host(db.conn()).as_deref(), Some("https://cloud.example"));
    }

    #[test]
    fn the_cloud_stands_in_for_an_unset_host() {
        use crate::settings::{registry::keys, SettingsStore};
        let (db, _, _, _) = setup();
        SettingsStore::set(
            db.conn(),
            keys::CLOUD_ORIGIN,
            "http://localhost:8080".to_string(),
        )
        .unwrap();
        assert_eq!(host(db.conn()).as_deref(), Some("http://localhost:8080"));
    }

    /// A one-request HTTP host: records the PUT it receives and answers 200.
    async fn one_shot_host(
        expected_path: String,
    ) -> (String, tokio::sync::oneshot::Receiver<serde_json::Value>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut raw = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = socket.read(&mut buf).await.unwrap();
                raw.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&raw).to_string();
                if let Some(split) = text.find("\r\n\r\n") {
                    let head = &text[..split];
                    let length: usize = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    if raw.len() >= split + 4 + length {
                        let first = head.lines().next().unwrap().to_string();
                        assert_eq!(first, format!("PUT {expected_path} HTTP/1.1"));
                        let body: serde_json::Value =
                            serde_json::from_slice(&raw[split + 4..split + 4 + length]).unwrap();
                        tx.send(body).unwrap();
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 11\r\nconnection: close\r\n\r\n{\"ok\":true}")
                .await
                .unwrap();
        });
        (origin, rx)
    }

    #[tokio::test]
    async fn pending_lists_are_put_to_their_host_as_signed_credentials() {
        use alexandria_verify::vc::status;
        let key = test_key("issuer");
        let issuer = derive_did_key(&key);
        let (origin, received) =
            one_shot_host(format!("/status-lists/{}/1", issuer.as_str())).await;
        let list_id = status::list_url(&origin, &issuer, 1);
        let mut bits = vec![0u8; status::MIN_BITS / 8];
        status::set_bit(&mut bits, 5, true).unwrap();
        let pending = vec![PendingStatusList {
            list_id: list_id.clone(),
            status_purpose: "revocation".into(),
            bits,
            version: 3,
        }];
        let (accepted, errors) = push(&pending, &key, &issuer, NOW).await;
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(accepted, vec![(list_id.clone(), 3)]);
        let document: VerifiableCredential =
            serde_json::from_value(received.await.unwrap()).unwrap();
        let bits = status::verify_fetched_list(&document, &list_id, &issuer, "revocation").unwrap();
        assert_eq!(status::get_bit(&bits, 5), Some(true));
    }

    #[tokio::test]
    async fn a_host_that_refuses_leaves_the_list_pending() {
        use alexandria_verify::vc::status;
        let key = test_key("issuer");
        let issuer = derive_did_key(&key);
        let other = derive_did_key(&test_key("other"));
        // Nothing listens here; and the second list is not ours to push.
        let pending = vec![
            PendingStatusList {
                list_id: status::list_url("http://127.0.0.1:9", &issuer, 1),
                status_purpose: "revocation".into(),
                bits: vec![0; 16],
                version: 1,
            },
            PendingStatusList {
                list_id: status::list_url("http://127.0.0.1:9", &other, 1),
                status_purpose: "revocation".into(),
                bits: vec![0; 16],
                version: 1,
            },
        ];
        let (accepted, errors) = push(&pending, &key, &issuer, NOW).await;
        assert!(accepted.is_empty());
        assert_eq!(errors.len(), 2);
        assert!(errors[1].contains("another issuer"), "{errors:?}");
    }

    #[test]
    fn a_configured_host_makes_status_lists_url_addressed_and_pending() {
        use crate::settings::{registry::keys, SettingsStore};
        use alexandria_verify::vc::status;
        let (db, key, issuer, subject) = setup();
        SettingsStore::set(
            db.conn(),
            keys::CREDENTIAL_STATUS_HOST,
            "http://127.0.0.1:8080/".to_string(),
        )
        .unwrap();
        assert_eq!(host(db.conn()).as_deref(), Some("http://127.0.0.1:8080"));

        let vc =
            credentials::issue(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        let reference = vc.credential_status.expect("status attached");
        let expected = status::list_url("http://127.0.0.1:8080", &issuer, 1);
        assert_eq!(reference.status_list_credential, expected);
        assert_eq!(reference.id, format!("{expected}#0"));

        // A new list is version 1 and nothing has served it: pending.
        let pending = super::pending(db.conn(), &issuer).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].version, 1);
        let signed = super::signed(&pending[0], &key, &issuer, NOW).unwrap();
        let bits = status::verify_fetched_list(&signed, &expected, &issuer, "revocation").unwrap();
        assert_eq!(status::get_bit(&bits, 0), Some(false));

        mark_published(db.conn(), &[(expected.clone(), 1)]).unwrap();
        assert!(super::pending(db.conn(), &issuer).unwrap().is_empty());

        // Revocation bumps the version past what the host has.
        lifecycle::revoke(db.conn(), &issuer, vc.id.as_deref().unwrap(), "test", NOW).unwrap();
        let pending = super::pending(db.conn(), &issuer).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].version, 2);
        let signed = super::signed(&pending[0], &key, &issuer, NOW).unwrap();
        let bits = status::verify_fetched_list(&signed, &expected, &issuer, "revocation").unwrap();
        assert_eq!(status::get_bit(&bits, 0), Some(true));

        // An older acknowledgement never moves the mark backwards.
        mark_published(db.conn(), &[(expected.clone(), 2)]).unwrap();
        mark_published(db.conn(), &[(expected, 1)]).unwrap();
        assert!(super::pending(db.conn(), &issuer).unwrap().is_empty());
    }

    #[test]
    fn without_a_host_lists_are_urns_and_never_pending() {
        let (db, key, issuer, subject) = setup();
        assert!(host(db.conn()).is_none());
        let vc =
            credentials::issue(db.conn(), &key, &issuer, &sample_request(subject), NOW).unwrap();
        lifecycle::revoke(db.conn(), &issuer, vc.id.as_deref().unwrap(), "test", NOW).unwrap();
        assert!(super::pending(db.conn(), &issuer).unwrap().is_empty());
    }
}
