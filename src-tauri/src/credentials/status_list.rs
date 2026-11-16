//! Status lists: per issuer per host, one Bitstring Status List for
//! revocation and one for suspension.
//!
//! Issuing a credential takes the next slot, the same index in both of its
//! issuer's lists ([`ensure`], [`allocate`]); revoking it sets that slot's
//! bit in the revocation list, suspending it sets the bit in the suspension
//! list and reinstating clears it. A list named at a host is a URL the host
//! serves, so a verifier can fetch it, and every version the host has not
//! acknowledged is [`pending`] until [`push`] delivers it signed and
//! [`mark_published`] records that.

use rusqlite::{params, Connection};

use super::{CredentialError, Issuer};
use crate::clock::now_rfc3339;
use crate::crypto::did::Did;
use crate::db::executor::{DatabaseExecutor, DatabaseWorkload};
use crate::domain::vc::{
    CredentialStatus, CredentialStatuses, VerifiableCredential, STATUS_PURPOSE_REVOCATION,
    STATUS_PURPOSE_SUSPENSION,
};
use crate::profile::scope::ProfileLease;

/// Slots per list: a 16 KiB bitmap.
pub const BITS: usize = alexandria_verify::vc::status::MIN_BITS;
/// The `credentialStatus` type of an entry in a list.
pub const ENTRY_TYPE: &str = "BitstringStatusListEntry";

/// The origin this identity's status lists are served from, if any.
///
/// The `credentials.status_host` setting wins; the Alexandria Cloud the app
/// talks to ([`crate::cloud::origin`], already checked) stands in when it is
/// empty. A configured host must be an allowed origin
/// ([`crate::cloud::is_allowed_origin`]) — a list named at a plain `http`
/// host could be swapped on the wire, and a verifier would believe it.
pub fn host(conn: &Connection) -> Option<String> {
    use crate::settings::{registry::keys, SettingsStore};
    let configured = SettingsStore::get(conn, keys::CREDENTIAL_STATUS_HOST);
    if configured.trim().is_empty() {
        return crate::cloud::origin(conn);
    }
    Some(configured.trim().trim_end_matches('/').to_string()).filter(|origin| {
        crate::cloud::is_allowed_origin(origin)
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

/// The list number of an issuer's revocation list at a host.
pub const REVOCATION_LIST_NUMBER: u32 = 1;
/// The list number of the issuer's suspension list at the same host.
pub const SUSPENSION_LIST_NUMBER: u32 = 2;

/// The pair of lists a credential issued now takes a slot in: one per
/// status purpose, the same index in each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLists {
    pub revocation: String,
    pub suspension: String,
}

impl StatusLists {
    /// The credential's `credentialStatus`: a revocation entry and a
    /// suspension entry, each a `BitstringStatusListEntry` at `index`.
    pub fn entries(&self, index: i64) -> CredentialStatuses {
        let entry = |purpose: &str, list_id: &str| CredentialStatus {
            id: format!("{list_id}#{index}"),
            type_: ENTRY_TYPE.into(),
            status_purpose: purpose.into(),
            status_list_index: index.to_string(),
            status_list_credential: list_id.to_string(),
        };
        vec![
            entry(STATUS_PURPOSE_REVOCATION, &self.revocation),
            entry(STATUS_PURPOSE_SUSPENSION, &self.suspension),
        ]
        .into()
    }
}

/// The lists a credential issued now by `issuer_did` belongs to.
///
/// One revocation list and one suspension list per issuer per host, list
/// numbers [`REVOCATION_LIST_NUMBER`] and [`SUSPENSION_LIST_NUMBER`]. With a
/// host configured each id is the URL the host serves it at (§14.11.2), so
/// any verifier can fetch it; without one it is a URN and the list travels
/// only in exported bundles. A host that changes later starts a fresh pair —
/// ids already written into credentials are never rewritten.
pub fn ensure(conn: &Connection, issuer_did: &Did) -> Result<StatusLists, CredentialError> {
    let origin = host(conn);
    let ensure_one = |number: u32, purpose: &str| -> Result<String, CredentialError> {
        let list_id = match &origin {
            Some(origin) => alexandria_verify::vc::status::list_url(origin, issuer_did, number),
            None => format!(
                "urn:alexandria:status-list:{}:{number}",
                issuer_did.as_str()
            ),
        };
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM credential_status_lists WHERE list_id = ?1",
            params![list_id],
            |r| r.get(0),
        )?;
        if exists == 0 {
            let bits = vec![0u8; BITS / 8];
            conn.execute(
                "INSERT INTO credential_status_lists \
                 (list_id, issuer_did, version, status_purpose, bits, bit_length) \
                 VALUES (?1, ?2, 1, ?3, ?4, ?5)",
                params![list_id, issuer_did.as_str(), purpose, bits, BITS as i64],
            )?;
        }
        Ok(list_id)
    };
    Ok(StatusLists {
        revocation: ensure_one(REVOCATION_LIST_NUMBER, STATUS_PURPOSE_REVOCATION)?,
        suspension: ensure_one(SUSPENSION_LIST_NUMBER, STATUS_PURPOSE_SUSPENSION)?,
    })
}

pub fn allocate(conn: &Connection, list_id: &str) -> Result<i64, CredentialError> {
    // Next free index = max allocated + 1. We read from `credentials`
    // rather than scanning the bitmap because gaps from revocations
    // shouldn't be reused (the revoked state is permanent evidence).
    let next: i64 = conn.query_row(
        "SELECT COALESCE(MAX(status_list_index), -1) + 1 FROM credentials \
             WHERE status_list_id = ?1",
        params![list_id],
        |r| r.get(0),
    )?;
    if next >= BITS as i64 {
        return Err(CredentialError::StatusListFull(list_id.to_string()));
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
pub fn pending(
    conn: &Connection,
    issuer_did: &Did,
) -> Result<Vec<PendingStatusList>, CredentialError> {
    let mut stmt = conn.prepare(
        "SELECT list_id, status_purpose, bits, version FROM credential_status_lists \
         WHERE issuer_did = ?1 AND version > published_version ORDER BY list_id",
    )?;
    let rows = stmt.query_map(params![issuer_did.as_str()], |r| {
        Ok(PendingStatusList {
            list_id: r.get(0)?,
            status_purpose: r.get(1)?,
            bits: r.get(2)?,
            version: r.get(3)?,
        })
    })?;
    let mut pending = Vec::new();
    for row in rows {
        let list = row?;
        if alexandria_verify::vc::status::parse_list_url(&list.list_id).is_some() {
            pending.push(list);
        }
    }
    Ok(pending)
}

/// The signed `BitstringStatusListCredential` a host serves for `list`.
pub fn signed(
    list: &PendingStatusList,
    issuer: &Issuer,
    now: &str,
) -> Result<VerifiableCredential, CredentialError> {
    let (key, issuer_did) = (issuer.key(), issuer.did());
    alexandria_verify::vc::status::status_list_credential(
        &list.list_id,
        issuer_did,
        &list.status_purpose,
        &list.bits,
        now,
        key,
    )
    .map_err(|source| CredentialError::SignStatusList {
        list_id: list.list_id.clone(),
        source,
    })
}

/// Record that a host acknowledged `version` of each list.
pub fn mark_published(
    conn: &Connection,
    published: &[(String, i64)],
) -> Result<(), CredentialError> {
    for (list_id, version) in published {
        conn.execute(
            "UPDATE credential_status_lists SET published_version = ?2 \
             WHERE list_id = ?1 AND published_version < ?2",
            params![list_id, version],
        )?;
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
    issuer: &Issuer,
    now: &str,
) -> (Vec<(String, i64)>, Vec<String>) {
    let mut accepted = Vec::new();
    let mut errors = Vec::new();
    for list in pending {
        match push_one(list, issuer, now).await {
            Ok(()) => accepted.push((list.list_id.clone(), list.version)),
            Err(e) => errors.push(format!("{}: {e}", list.list_id)),
        }
    }
    (accepted, errors)
}

async fn push_one(
    list: &PendingStatusList,
    issuer: &Issuer,
    now: &str,
) -> Result<(), CredentialError> {
    let issuer_did = issuer.did();
    let location = alexandria_verify::vc::status::parse_list_url(&list.list_id)
        .ok_or(CredentialError::ListNotHosted)?;
    if &location.issuer != issuer_did {
        return Err(CredentialError::ListOfAnotherIssuer);
    }
    let document = signed(list, issuer, now)?;
    let response = crate::cloud::client()
        .map_err(CredentialError::Cloud)?
        .put(&list.list_id)
        .json(&document)
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(CredentialError::HostRefused {
            origin: location.origin,
            status,
            body: body.chars().take(200).collect(),
        });
    }
    Ok(())
}

/// Publish every pending list `issuer` owns, and record what landed.
///
/// Called after anything that creates or changes a list, and on demand.
/// Timed suspensions whose time has come are lifted first, so the lists
/// pushed carry the cleared bits. A failed push is reported and left
/// pending; the local change already happened and stands regardless.
pub(crate) async fn publish(
    executor: &DatabaseExecutor,
    lease: &ProfileLease,
    issuer: &Issuer,
) -> Result<StatusPublishReport, CredentialError> {
    let issuer_for_read = issuer.did().clone();
    let now = now_rfc3339();
    let pending = executor
        .execute(
            DatabaseWorkload::Instructor,
            lease.clone(),
            "credentials.status_lists.pending",
            move |db| {
                super::lifecycle::lift_expired_suspensions(db.conn(), &issuer_for_read, &now)
                    .and_then(|_| pending(db.conn(), &issuer_for_read))
                    .map_err(|e| e.to_string())
            },
        )
        .await
        .map_err(CredentialError::Profile)?;
    if pending.is_empty() {
        return Ok(StatusPublishReport::default());
    }
    let (accepted, errors) = push(&pending, issuer, &now_rfc3339()).await;
    let published = accepted.iter().map(|(id, _)| id.clone()).collect();
    if !accepted.is_empty() {
        executor
            .execute(
                DatabaseWorkload::Instructor,
                lease.clone(),
                "credentials.status_lists.mark",
                move |db| mark_published(db.conn(), &accepted).map_err(|e| e.to_string()),
            )
            .await
            .map_err(CredentialError::Profile)?;
    }
    Ok(StatusPublishReport { published, errors })
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
        let (accepted, errors) = push(&pending, &Issuer::from_key(key.clone()), NOW).await;
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
        let (accepted, errors) = push(&pending, &Issuer::from_key(key.clone()), NOW).await;
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

        let vc = credentials::issue(db.conn(), &key, &sample_request(subject), NOW).unwrap();
        let expected = status::list_url("http://127.0.0.1:8080", &issuer, 1);
        let suspension = status::list_url("http://127.0.0.1:8080", &issuer, 2);
        let reference = vc.status_entry("revocation").expect("revocation entry");
        assert_eq!(reference.status_list_credential, expected);
        assert_eq!(reference.id, format!("{expected}#0"));
        let reference = vc.status_entry("suspension").expect("suspension entry");
        assert_eq!(reference.status_list_credential, suspension);
        assert_eq!(reference.id, format!("{suspension}#0"));

        // New lists are version 1 and nothing has served them: pending, each
        // signed with its own purpose.
        let pending = super::pending(db.conn(), &issuer).unwrap();
        assert_eq!(pending.len(), 2);
        for (list, (id, purpose)) in pending
            .iter()
            .zip([(&expected, "revocation"), (&suspension, "suspension")])
        {
            assert_eq!((&list.list_id, list.version), (id, 1));
            let signed = super::signed(list, &key, NOW).unwrap();
            let bits = status::verify_fetched_list(&signed, id, &issuer, purpose).unwrap();
            assert_eq!(status::get_bit(&bits, 0), Some(false));
        }

        mark_published(db.conn(), &[(expected.clone(), 1), (suspension, 1)]).unwrap();
        assert!(super::pending(db.conn(), &issuer).unwrap().is_empty());

        // Revocation bumps the version past what the host has.
        lifecycle::revoke(db.conn(), &issuer, vc.id.as_deref().unwrap(), "test", NOW).unwrap();
        let pending = super::pending(db.conn(), &issuer).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].version, 2);
        let signed = super::signed(&pending[0], &key, NOW).unwrap();
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
        let vc = credentials::issue(db.conn(), &key, &sample_request(subject), NOW).unwrap();
        lifecycle::revoke(db.conn(), &issuer, vc.id.as_deref().unwrap(), "test", NOW).unwrap();
        assert!(super::pending(db.conn(), &issuer).unwrap().is_empty());
    }
}
