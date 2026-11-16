// SPDX-License-Identifier: AGPL-3.0-or-later
//! Collecting the credentials organisations issued to the learner through
//! Alexandria Cloud.
//!
//! An organisation's hosted service signs a role credential for a candidate
//! as the organisation and holds it until the candidate's app asks with a
//! signed GET, `GET /api/credentials/for/{did}`, answered with
//! `[{"credential": …}]`. [`fetch`] asks; [`store`] decides what to keep;
//! [`run`] does both on the profile's background pass to Cloud
//! (`crate::notifications::cloud_pull`).
//!
//! Cloud is not trusted for any of it. Each credential is verified here with
//! the verifier library before it is stored, exactly as a credential handed
//! over in a file is ([`super::receive`]). Its status lists are fetched from
//! where the credential names them and are applied only when the issuer
//! signed them. Organisation backing is judged against the descriptor the
//! organisation publishes on its own domain, never against what Cloud says.

use alexandria_verify::org::{self, OrgDescriptor, OrganisationBacking};
use ed25519_dalek::SigningKey;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::receive::{self, Receipt};
use crate::cloud::Directory;
use crate::crypto::did::Did;
use crate::db::executor::{DatabaseExecutor, DatabaseWorkload};
use crate::domain::vc::{status, VerifiableCredential};
use crate::profile::scope::ProfileLease;

/// Where Cloud answers with the credentials issued to a DID.
pub const ISSUED_CREDENTIALS_PATH: &str = "/api/credentials/for/";

/// Resolves the descriptor an organisation publishes on a domain, or `None`
/// when none is available.
pub type DescriptorResolver<'a> = dyn Fn(&str) -> Result<Option<OrgDescriptor>, String> + 'a;

/// What one fetch from Cloud brought back, not yet verified.
#[derive(Debug, Clone, Default)]
pub struct Fetched {
    /// One entry per item Cloud listed: the credential, or why it could not
    /// be read as one.
    pub credentials: Vec<Result<VerifiableCredential, String>>,
    /// Status list documents the credentials name, as served.
    pub status_lists: Vec<FetchedList>,
}

/// A status list document fetched from the URL a credential named.
#[derive(Debug, Clone)]
pub struct FetchedList {
    pub url: String,
    /// The issuer of the credential that named it.
    pub issuer: Did,
    pub status_purpose: String,
    pub document: VerifiableCredential,
}

/// What happened to one collected credential.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// Verified and newly stored.
    Stored,
    /// Verified and already in the store.
    AlreadyPresent,
    /// The signature holds but something verification needs is missing, such
    /// as a status list; queued in the pending-verification queue and tried
    /// again on the next pull.
    Pending { reasons: Vec<String> },
    /// Not stored.
    Rejected { reason: String },
}

/// One credential's outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collected {
    /// The credential's id, else its position in Cloud's answer.
    pub credential_id: String,
    pub outcome: Outcome,
    /// The organisation backing of the issuer key, for a verified credential.
    pub organisation: Option<OrganisationBacking>,
}

/// The most credentials one answer is read for; Cloud sends at most 200.
const MAX_CREDENTIALS: usize = 200;

/// Ask Cloud for the credentials issued to `did`, and fetch the status lists
/// they name.
///
/// A list is fetched only from the Cloud origin itself and only at the path
/// that names the credential's own issuer ([`status::parse_list_url`]), so a
/// credential cannot send the app to an arbitrary host or have its issuer's
/// bits written over another issuer's list. A list that cannot be fetched is
/// left out; verification then reads it as missing and the credential waits.
/// Items Cloud lists that are not credentials are kept as errors, so one bad
/// item does not hide the others.
pub async fn fetch(
    client: &reqwest::Client,
    directory: &Directory,
    key: &SigningKey,
    did: &Did,
) -> Result<Fetched, String> {
    let path = format!("{ISSUED_CREDENTIALS_PATH}{}", did.as_str());
    let body = crate::cloud::signed_get(client, directory, key, &path).await?;
    let entries = body
        .as_array()
        .ok_or("Alexandria Cloud did not answer with a list of credentials")?;
    let credentials: Vec<Result<VerifiableCredential, String>> = entries
        .iter()
        .take(MAX_CREDENTIALS)
        .map(|entry| {
            let credential = entry
                .get("credential")
                .ok_or_else(|| "the entry carries no credential".to_string())?;
            super::store::credential_entry(credential).map_err(|e| format!("not a credential: {e}"))
        })
        .collect();

    let mut status_lists = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for credential in credentials.iter().flatten() {
        let issuer = credential.issuer.id();
        for entry in credential.status_entries() {
            let url = &entry.status_list_credential;
            if !names_own_list(url, issuer) || !same_origin(url, &directory.url) {
                continue;
            }
            if !seen.insert(url.clone()) {
                continue;
            }
            match fetch_list(client, url).await {
                Ok(document) => status_lists.push(FetchedList {
                    url: url.clone(),
                    issuer: issuer.clone(),
                    status_purpose: entry.status_purpose.clone(),
                    document,
                }),
                Err(error) => log::debug!("collect: status list {url}: {error}"),
            }
        }
    }
    Ok(Fetched {
        credentials,
        status_lists,
    })
}

/// One collection pass for the background pull: fetch from `origin`, make
/// sure each issuing organisation's domain descriptor is freshly cached and
/// lists the keys that signed (one TXT lookup per domain at most, see
/// [`crate::org_trust`]), then verify and store under `lease`.
pub(crate) async fn run(
    executor: &DatabaseExecutor,
    lease: &ProfileLease,
    origin: &str,
    key: &SigningKey,
    did: &Did,
) -> Result<Vec<Collected>, String> {
    let client = crate::cloud::client()?;
    let directory = Directory {
        name: "Alexandria Cloud".into(),
        url: origin.to_string(),
    };
    let fetched = fetch(&client, &directory, key, did).await?;
    if fetched.credentials.is_empty() {
        return Ok(Vec::new());
    }
    let mut domains: std::collections::BTreeMap<&str, Vec<&str>> = Default::default();
    for credential in fetched.credentials.iter().flatten() {
        if let Some(domain) = credential.issuer.domain() {
            domains
                .entry(domain)
                .or_default()
                .push(credential.issuer.as_str());
        }
    }
    for (domain, keys) in &domains {
        // An unresolvable domain reads as descriptor_not_supplied.
        if let Err(error) = crate::org_trust::refresh_descriptor_for(
            executor,
            DatabaseWorkload::Background,
            lease.clone(),
            domain,
            keys,
        )
        .await
        {
            log::debug!("collect: descriptor for {domain}: {error}");
        }
    }
    let holder = did.clone();
    let now = crate::clock::now_rfc3339();
    let now_unix = chrono::Utc::now().timestamp();
    executor
        .execute(
            DatabaseWorkload::Background,
            lease.clone(),
            "credentials.collect",
            move |db| {
                let conn = db.conn();
                Ok(store(conn, &holder, &fetched, &now, &|domain| {
                    crate::org_trust::fresh_descriptor(conn, domain, now_unix)
                }))
            },
        )
        .await
}

/// Whether `url` is a hosted status list URL whose path names `issuer`.
fn names_own_list(url: &str, issuer: &Did) -> bool {
    status::parse_list_url(url).is_some_and(|location| &location.issuer == issuer)
}

fn same_origin(a: &str, b: &str) -> bool {
    match (url::Url::parse(a), url::Url::parse(b)) {
        (Ok(a), Ok(b)) => a.origin() == b.origin(),
        _ => false,
    }
}

/// GET a status list document. Status lists are public, so no proof is sent.
async fn fetch_list(client: &reqwest::Client, url: &str) -> Result<VerifiableCredential, String> {
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    let value = crate::cloud::response_json(response).await?;
    serde_json::from_value(value).map_err(|e| format!("not a status list credential: {e}"))
}

/// Verify what [`fetch`] brought back and keep what verifies.
///
/// Status lists come first, each applied only when the credential's issuer
/// signed it for the URL it was fetched from, it is not one `holder` issues,
/// and it is newer than the copy already held. Then each credential, for `holder` only, goes through
/// [`receive::receive`]: accepted ones are stored once, pending ones wait in
/// the pending-verification queue until a later pull can decide them, and
/// refused ones are dropped (and leave the queue). Organisation backing is
/// reported for verified credentials whose issuer names a domain, with the
/// descriptor `resolve` supplies; it never decides whether one is stored.
pub fn store(
    conn: &Connection,
    holder: &Did,
    fetched: &Fetched,
    now: &str,
    resolve: &DescriptorResolver<'_>,
) -> Vec<Collected> {
    for list in &fetched.status_lists {
        if let Err(error) = apply_list(conn, holder, list, now) {
            log::debug!("collect: status list {} not applied: {error}", list.url);
        }
    }
    fetched
        .credentials
        .iter()
        .enumerate()
        .map(|(index, entry)| collect_one(conn, holder, index, entry, now, resolve))
        .collect()
}

fn collect_one(
    conn: &Connection,
    holder: &Did,
    index: usize,
    entry: &Result<VerifiableCredential, String>,
    now: &str,
    resolve: &DescriptorResolver<'_>,
) -> Collected {
    let position = || format!("entry {}", index + 1);
    let rejected = |credential_id: String, reason: String| Collected {
        credential_id,
        outcome: Outcome::Rejected { reason },
        organisation: None,
    };
    let credential = match entry {
        Ok(credential) => credential,
        Err(reason) => return rejected(position(), reason.clone()),
    };
    let credential_id = credential.id.clone().unwrap_or_else(position);
    if credential.credential_subject.id.as_str() != holder.as_str() {
        return rejected(credential_id, "issued to someone else".into());
    }
    let outcome = match receive::receive(conn, credential, now) {
        Ok(Receipt::Stored) => Outcome::Stored,
        Ok(Receipt::AlreadyPresent) => Outcome::AlreadyPresent,
        Ok(Receipt::Pending(reasons)) => {
            let queued = serde_json::to_string(credential)
                .map_err(|e| e.to_string())
                .and_then(|json| {
                    crate::p2p::vc_did::queue_pending(
                        conn,
                        &credential_id,
                        credential.issuer.as_str(),
                        holder.as_str(),
                        &json,
                    )
                });
            if let Err(error) = queued {
                log::warn!("collect: {credential_id} not queued: {error}");
            }
            Outcome::Pending {
                reasons: reasons.into_iter().map(str::to_string).collect(),
            }
        }
        Ok(Receipt::Rejected(why)) => Outcome::Rejected {
            reason: format!("did not verify: {}", why.join(", ")),
        },
        Err(reason) => Outcome::Rejected { reason },
    };
    if !matches!(outcome, Outcome::Pending { .. }) {
        if let Err(error) = crate::p2p::vc_did::unqueue_pending(conn, &credential_id) {
            log::warn!("collect: {credential_id} left in the pending queue: {error}");
        }
    }
    let organisation = matches!(outcome, Outcome::Stored | Outcome::AlreadyPresent).then(|| {
        let descriptor = credential
            .issuer
            .domain()
            .and_then(|domain| resolve(domain).ok().flatten());
        org::organisation_backing(credential, descriptor.as_ref())
    });
    Collected {
        credential_id,
        outcome,
        organisation,
    }
}

/// Apply one fetched status list to the local store.
///
/// The list must name its issuer in its URL, be the list it was fetched as,
/// be signed by that issuer and carry the purpose the credential used it for
/// ([`status::verify_fetched_list`]). A list `holder` issues is never written
/// from a fetch: this profile's own bits are the source of truth for it, and
/// a genuine older copy it once pushed to its host could otherwise be
/// replayed over a later revocation. Fetched copies are ordered by their own
/// signed `validFrom` against the `valid_from` of the copy held, never
/// against the `version` counter, so a replayed older list cannot undo a
/// revocation. A row held under another issuer is never overwritten.
fn apply_list(
    conn: &Connection,
    holder: &Did,
    list: &FetchedList,
    now: &str,
) -> Result<(), String> {
    if &list.issuer == holder {
        return Err("the list is one this profile issues".into());
    }
    if !names_own_list(&list.url, &list.issuer) {
        return Err("the list URL does not name the credential's issuer".into());
    }
    let bits = status::verify_fetched_list(
        &list.document,
        &list.url,
        &list.issuer,
        &list.status_purpose,
    )
    .map_err(|e| e.to_string())?;
    if bits.len() > crate::p2p::vc_status::MAX_BITS_BYTES {
        return Err("status list bitmap is over the cap".into());
    }
    let valid_from = chrono::DateTime::parse_from_rfc3339(&list.document.valid_from)
        .map_err(|e| format!("status list validFrom: {e}"))?
        .timestamp();
    conn.execute(
        "INSERT INTO credential_status_lists \
         (list_id, issuer_did, version, status_purpose, bits, bit_length, updated_at, valid_from) \
         VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT(list_id) DO UPDATE SET \
            status_purpose = excluded.status_purpose, \
            bits = excluded.bits, \
            bit_length = excluded.bit_length, \
            updated_at = excluded.updated_at, \
            valid_from = excluded.valid_from \
         WHERE credential_status_lists.issuer_did = excluded.issuer_did \
           AND (credential_status_lists.valid_from IS NULL \
                OR excluded.valid_from > credential_status_lists.valid_from)",
        rusqlite::params![
            list.url,
            list.issuer.as_str(),
            list.status_purpose,
            bits,
            (bits.len() as i64) * 8,
            now,
            valid_from,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::store as credential_store;
    use crate::credentials::trust::trust_view;
    use crate::crypto::did::derive_did_key;
    use crate::db::Database;
    use crate::domain::credential_trust::{Provenance, SuspensionState};
    use crate::domain::vc::sign::{sign_credential, UnsignedCredential};
    use crate::domain::vc::{
        status, CredentialIssuer, CredentialStatus, CredentialSubject, EvidenceEntry,
        IntegrityAssertion, Proof, RoleClaim, SqliteVerificationStore, BITSTRING_STATUS_LIST_ENTRY,
        STATUS_PURPOSE_REVOCATION, STATUS_PURPOSE_SUSPENSION,
    };
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const NOW: &str = "2026-11-16T12:00:00Z";
    const ISSUED: &str = "2026-11-16T10:00:00Z";
    const DOMAIN: &str = "example.org";
    const NAME: &str = "Example Org";

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn org_key() -> SigningKey {
        key(41)
    }

    fn learner_key() -> SigningKey {
        key(42)
    }

    fn open_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db
    }

    fn descriptor() -> OrgDescriptor {
        OrgDescriptor {
            format: alexandria_verify::org::DESCRIPTOR_FORMAT.into(),
            domain: DOMAIN.into(),
            name: NAME.into(),
            keys: vec![derive_did_key(&org_key()).as_str().to_string()],
        }
    }

    fn resolve_example(domain: &str) -> Result<Option<OrgDescriptor>, String> {
        Ok((domain == DOMAIN).then(descriptor))
    }

    /// A fake Alexandria Cloud. It answers the signed fetch with whatever
    /// `issued` holds at the time, and serves the status lists in `lists` by
    /// path; anything else is a 404.
    struct FakeCloud {
        origin: String,
        issued: Arc<Mutex<serde_json::Value>>,
        lists: Arc<Mutex<HashMap<String, serde_json::Value>>>,
        proofless_fetches: Arc<Mutex<u32>>,
    }

    async fn fake_cloud() -> FakeCloud {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let issued = Arc::new(Mutex::new(serde_json::json!([])));
        let lists: Arc<Mutex<HashMap<String, serde_json::Value>>> = Arc::default();
        let proofless = Arc::new(Mutex::new(0));
        let (served_issued, served_lists, counter) =
            (issued.clone(), lists.clone(), proofless.clone());
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let (issued, lists, counter) =
                    (served_issued.clone(), served_lists.clone(), counter.clone());
                tokio::spawn(async move {
                    let mut buffer = vec![0; 16 * 1024];
                    let n = socket.read(&mut buffer).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buffer[..n]).to_lowercase();
                    let path = request
                        .lines()
                        .next()
                        .and_then(|line| line.split(' ').nth(1))
                        .unwrap_or_default()
                        .to_string();
                    let body = if path.starts_with(ISSUED_CREDENTIALS_PATH) {
                        if !request.contains("x-alexandria-proof:") {
                            *counter.lock().unwrap() += 1;
                        }
                        Some(issued.lock().unwrap().clone())
                    } else {
                        lists.lock().unwrap().get(&path).cloned()
                    };
                    let response = match body {
                        Some(body) => {
                            let body = body.to_string();
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            )
                        }
                        None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\
                                 Connection: close\r\n\r\n"
                            .to_string(),
                    };
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        FakeCloud {
            origin,
            issued,
            lists,
            proofless_fetches: proofless,
        }
    }

    impl FakeCloud {
        fn directory(&self) -> Directory {
            Directory {
                name: "Alexandria Cloud".into(),
                url: self.origin.clone(),
            }
        }

        fn issue(&self, credentials: &[&VerifiableCredential]) {
            *self.issued.lock().unwrap() = serde_json::json!(credentials
                .iter()
                .map(|credential| serde_json::json!({ "credential": credential }))
                .collect::<Vec<_>>());
        }

        /// Publish `signer`'s revocation and suspension lists (all clear) at
        /// the URLs `organisation_credential` names.
        fn publish_lists(&self, signer: &SigningKey) {
            let issuer = derive_did_key(signer);
            let mut lists = self.lists.lock().unwrap();
            for (number, purpose) in [
                (1, STATUS_PURPOSE_REVOCATION),
                (2, STATUS_PURPOSE_SUSPENSION),
            ] {
                let url = status::list_url(&self.origin, &issuer, number);
                let document = status::status_list_credential(
                    &url, &issuer, purpose, &[0u8; 16], ISSUED, signer,
                )
                .unwrap();
                let path = url.strip_prefix(&self.origin).unwrap().to_lowercase();
                lists.insert(path, serde_json::json!(document));
            }
        }

        async fn pull(&self, db: &Database) -> Vec<Collected> {
            let learner = learner_key();
            let did = derive_did_key(&learner);
            let fetched = fetch(
                &crate::cloud::client().unwrap(),
                &self.directory(),
                &learner,
                &did,
            )
            .await
            .unwrap();
            store(db.conn(), &did, &fetched, NOW, &resolve_example)
        }
    }

    /// A role credential as Cloud issues it: signed by the organisation's
    /// hosted key, an issuer object naming the organisation, the candidate's
    /// integrity assertion, revocation and suspension entries in lists hosted
    /// at `origin`, and the `AssessedCredentialEvidence` entry
    /// naming the candidate's own credential.
    fn organisation_credential(
        origin: &str,
        signer: &SigningKey,
        subject: &Did,
    ) -> VerifiableCredential {
        let issuer = derive_did_key(signer);
        let entry = |purpose: &str, number: u32| {
            let list = status::list_url(origin, &issuer, number);
            CredentialStatus {
                id: format!("{list}#7"),
                type_: BITSTRING_STATUS_LIST_ENTRY.into(),
                status_purpose: purpose.into(),
                status_list_index: "7".into(),
                status_list_credential: list,
            }
        };
        let integrity: IntegrityAssertion = serde_json::from_value(serde_json::json!({
            "sessionId": "session-1",
            "status": "completed",
            "integrityScore": 0.93,
            "criticalCount": 0,
            "warningCount": 1,
            "assuranceLevel": "device_attested",
            "generatedAt": ISSUED,
            "signalsRun": ["core", "device_attestation"],
        }))
        .unwrap();
        let serde_json::Value::Object(assessed) = serde_json::json!({
            "type": "AssessedCredentialEvidence",
            "credentialId": "urn:uuid:11111111-1111-4111-8111-111111111111",
            "integrityHash": "b3a1c0ffee",
            "attestation": { "outcome": "verified_credential_bound", "platform": "ios" },
            "verifiedAssuranceLevel": "device_attested",
        }) else {
            unreachable!()
        };
        let unsigned = VerifiableCredential {
            context: vec!["https://www.w3.org/ns/credentials/v2".into()],
            id: Some("urn:uuid:22222222-2222-4222-8222-222222222222".into()),
            type_: vec!["VerifiableCredential".into(), "RoleCredential".into()],
            issuer: CredentialIssuer::organisation(issuer.clone(), NAME, DOMAIN),
            valid_from: ISSUED.into(),
            valid_until: None,
            credential_subject: CredentialSubject {
                id: subject.clone(),
                properties: RoleClaim {
                    role: "Backend engineer".into(),
                    scope: Some(NAME.into()),
                }
                .into_properties(),
            },
            credential_status: Some(
                vec![
                    entry(STATUS_PURPOSE_REVOCATION, 1),
                    entry(STATUS_PURPOSE_SUSPENSION, 2),
                ]
                .into(),
            ),
            terms_of_use: None,
            witness: None,
            integrity: Some(integrity),
            evidence: vec![EvidenceEntry(assessed)],
            related_resource: Vec::new(),
            proof: Proof::unsigned(ISSUED),
        };
        sign_credential(
            UnsignedCredential {
                credential: unsigned,
            },
            signer,
            &issuer,
        )
        .unwrap()
    }

    fn listed(db: &Database) -> Vec<VerifiableCredential> {
        let holder = derive_did_key(&learner_key());
        credential_store::list(db.conn(), Some(holder.as_str()), None).unwrap()
    }

    fn pending_rows(db: &Database) -> i64 {
        db.conn()
            .query_row(
                "SELECT COUNT(*) FROM credentials_pending_verification",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// The organisation-issued credential, assessed-credential entry and all,
    /// verifies and lands in the credential list, and its trust view says
    /// who issued it.
    #[tokio::test]
    async fn a_valid_organisation_credential_is_stored_with_organisation_provenance() {
        let db = open_db();
        let cloud = fake_cloud().await;
        let holder = derive_did_key(&learner_key());
        let vc = organisation_credential(&cloud.origin, &org_key(), &holder);
        cloud.publish_lists(&org_key());
        cloud.issue(&[&vc]);

        let collected = cloud.pull(&db).await;

        assert_eq!(
            collected,
            vec![Collected {
                credential_id: vc.id.clone().unwrap(),
                outcome: Outcome::Stored,
                organisation: Some(OrganisationBacking::Verified {
                    name: NAME.into(),
                    domain: DOMAIN.into(),
                }),
            }]
        );
        assert_eq!(*cloud.proofless_fetches.lock().unwrap(), 0);
        let stored = listed(&db);
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].id, vc.id);
        assert!(
            stored[0]
                .evidence
                .iter()
                .any(|entry| entry.has_type("AssessedCredentialEvidence")),
            "the evidence entry the verifier does not know is kept, not refused"
        );

        let view = trust_view(
            &SqliteVerificationStore(db.conn()),
            &alexandria_verify::attestation::AttestationTrust::default(),
            &stored[0],
            NOW,
            "test-network",
            None,
            Some(&descriptor()),
        );
        assert_eq!(
            view.provenance,
            Some(Provenance::Organisation {
                name: NAME.into(),
                domain: DOMAIN.into(),
            })
        );
        assert_eq!(view.suspension, SuspensionState::NotSuspended);
    }

    #[tokio::test]
    async fn a_tampered_credential_is_rejected_and_not_stored() {
        let db = open_db();
        let cloud = fake_cloud().await;
        let holder = derive_did_key(&learner_key());
        let mut vc = organisation_credential(&cloud.origin, &org_key(), &holder);
        vc.credential_subject
            .properties
            .insert("role".into(), "Chief executive".into());
        cloud.publish_lists(&org_key());
        cloud.issue(&[&vc]);

        let collected = cloud.pull(&db).await;

        assert_eq!(collected.len(), 1);
        assert!(
            matches!(collected[0].outcome, Outcome::Rejected { .. }),
            "{collected:?}"
        );
        assert!(listed(&db).is_empty());
        assert_eq!(pending_rows(&db), 0);
    }

    #[tokio::test]
    async fn fetching_the_same_credential_twice_stores_it_once() {
        let db = open_db();
        let cloud = fake_cloud().await;
        let holder = derive_did_key(&learner_key());
        let vc = organisation_credential(&cloud.origin, &org_key(), &holder);
        cloud.publish_lists(&org_key());
        cloud.issue(&[&vc]);

        let first = cloud.pull(&db).await;
        let second = cloud.pull(&db).await;

        assert_eq!(first[0].outcome, Outcome::Stored);
        assert_eq!(second[0].outcome, Outcome::AlreadyPresent);
        assert_eq!(listed(&db).len(), 1);
    }

    /// A credential is stored only for the learner it was issued to.
    #[tokio::test]
    async fn a_credential_issued_to_someone_else_is_rejected() {
        let db = open_db();
        let cloud = fake_cloud().await;
        let stranger = derive_did_key(&key(43));
        let vc = organisation_credential(&cloud.origin, &org_key(), &stranger);
        cloud.publish_lists(&org_key());
        cloud.issue(&[&vc]);

        let collected = cloud.pull(&db).await;

        assert!(
            matches!(collected[0].outcome, Outcome::Rejected { .. }),
            "{collected:?}"
        );
        let all = credential_store::list(db.conn(), None, None).unwrap();
        assert!(all.is_empty());
    }

    /// Without its status lists a credential cannot be accepted yet: it waits
    /// in the pending-verification queue, and the next pull that can read the
    /// lists stores it and empties the queue.
    #[tokio::test]
    async fn a_credential_whose_status_lists_are_unavailable_waits_in_the_pending_queue() {
        let db = open_db();
        let cloud = fake_cloud().await;
        let holder = derive_did_key(&learner_key());
        let vc = organisation_credential(&cloud.origin, &org_key(), &holder);
        cloud.issue(&[&vc]);

        let first = cloud.pull(&db).await;
        assert!(
            matches!(first[0].outcome, Outcome::Pending { .. }),
            "{first:?}"
        );
        assert!(listed(&db).is_empty());
        assert_eq!(pending_rows(&db), 1);

        let again = cloud.pull(&db).await;
        assert!(matches!(again[0].outcome, Outcome::Pending { .. }));
        assert_eq!(pending_rows(&db), 1, "queued once");

        cloud.publish_lists(&org_key());
        let later = cloud.pull(&db).await;
        assert_eq!(later[0].outcome, Outcome::Stored);
        assert_eq!(listed(&db).len(), 1);
        assert_eq!(pending_rows(&db), 0);
    }

    /// `signer`'s list `number` at `origin`, signed as of `valid_from`.
    fn fetched_list(
        origin: &str,
        signer: &SigningKey,
        number: u32,
        bits: &[u8],
        valid_from: &str,
    ) -> FetchedList {
        let issuer = derive_did_key(signer);
        let url = status::list_url(origin, &issuer, number);
        let document = status::status_list_credential(
            &url,
            &issuer,
            STATUS_PURPOSE_REVOCATION,
            bits,
            valid_from,
            signer,
        )
        .unwrap();
        FetchedList {
            url,
            issuer,
            status_purpose: STATUS_PURPOSE_REVOCATION.into(),
            document,
        }
    }

    /// The first 16 bytes of the bits held for `list_id` (a signed list
    /// pads its bitmap to the minimum length).
    fn list_bits(db: &Database, list_id: &str) -> Vec<u8> {
        let mut bits: Vec<u8> = db
            .conn()
            .query_row(
                "SELECT bits FROM credential_status_lists WHERE list_id = ?1",
                [list_id],
                |r| r.get(0),
            )
            .unwrap();
        bits.truncate(16);
        bits
    }

    fn store_lists(db: &Database, lists: Vec<FetchedList>) {
        let holder = derive_did_key(&learner_key());
        let fetched = Fetched {
            credentials: Vec::new(),
            status_lists: lists,
        };
        store(db.conn(), &holder, &fetched, NOW, &resolve_example);
    }

    /// A list this profile issues is never written from a fetch, however
    /// genuinely signed: an older copy the device itself once pushed to its
    /// host, replayed, must not undo a revocation made since.
    #[test]
    fn a_fetched_copy_of_a_locally_issued_list_never_replaces_local_bits() {
        let db = open_db();
        let origin = "https://cloud.example.org";
        let learner = learner_key();
        let holder = derive_did_key(&learner);
        let list_id = status::list_url(origin, &holder, 1);
        let mut revoked = vec![0u8; 16];
        revoked[0] = 0x80;
        db.conn()
            .execute(
                "INSERT INTO credential_status_lists \
                 (list_id, issuer_did, version, status_purpose, bits, bit_length) \
                 VALUES (?1, ?2, 2, 'revocation', ?3, 128)",
                rusqlite::params![list_id, holder.as_str(), revoked],
            )
            .unwrap();

        store_lists(
            &db,
            vec![fetched_list(origin, &learner, 1, &[0u8; 16], ISSUED)],
        );

        assert_eq!(list_bits(&db, &list_id), revoked);
    }

    /// Fetched copies of another issuer's list are ordered by their own
    /// `validFrom`: an older copy served after a newer one changes nothing.
    #[test]
    fn an_older_fetched_copy_does_not_replace_a_newer_one() {
        let db = open_db();
        let origin = "https://cloud.example.org";
        let org = org_key();
        let list_id = status::list_url(origin, &derive_did_key(&org), 1);
        let mut revoked = vec![0u8; 16];
        revoked[0] = 0x80;

        store_lists(
            &db,
            vec![fetched_list(
                origin,
                &org,
                1,
                &revoked,
                "2026-11-16T11:00:00Z",
            )],
        );
        store_lists(
            &db,
            vec![fetched_list(
                origin,
                &org,
                1,
                &[0u8; 16],
                "2026-11-16T09:00:00Z",
            )],
        );
        assert_eq!(list_bits(&db, &list_id), revoked);

        let mut both = revoked.clone();
        both[0] |= 0x40;
        store_lists(
            &db,
            vec![fetched_list(origin, &org, 1, &both, "2026-11-16T11:30:00Z")],
        );
        assert_eq!(list_bits(&db, &list_id), both);
    }

    /// A row a gossiped list wrote carries the issuer's own counter in
    /// `version`, which says nothing about a fetched copy's age: a fetched
    /// copy is ordered against the stored `validFrom` alone, so a large
    /// counter cannot pin the list against the issuer's later copies.
    #[test]
    fn a_fetched_copy_is_not_ordered_against_a_gossip_version_counter() {
        let db = open_db();
        let origin = "https://cloud.example.org";
        let org = org_key();
        let list_id = status::list_url(origin, &derive_did_key(&org), 1);
        let gossiped = crate::p2p::vc_status::build_signed_status_document(
            &org,
            Some(&list_id),
            STATUS_PURPOSE_REVOCATION,
            4_000_000_000,
            &[0u8; 16],
        )
        .unwrap();
        let envelope = crate::p2p::types::SignedGossipMessage {
            topic: "/alexandria/vc-status/1.0".into(),
            payload: gossiped,
            signature: vec![0u8; 64],
            public_key: vec![0u8; 32],
            stake_address: String::new(),
            timestamp: 0,
            encrypted: false,
            key_id: None,
        };
        crate::p2p::vc_status::handle_status_message(&db, &envelope).unwrap();

        let mut revoked = vec![0u8; 16];
        revoked[0] = 0x80;
        store_lists(
            &db,
            vec![fetched_list(
                origin,
                &org,
                1,
                &revoked,
                "2026-11-16T11:00:00Z",
            )],
        );
        assert_eq!(list_bits(&db, &list_id), revoked);
    }

    /// A credential cannot name another issuer's list and have its own
    /// issuer's bits written over that list in this store.
    #[tokio::test]
    async fn a_status_list_url_naming_another_issuer_is_not_applied() {
        let db = open_db();
        let cloud = fake_cloud().await;
        let holder = derive_did_key(&learner_key());
        let victim = derive_did_key(&key(44));
        let attacker = key(45);
        let mut vc = organisation_credential(&cloud.origin, &attacker, &holder);
        // Point the revocation entry at the victim's list URL, and serve the
        // attacker's own signed list there.
        let victim_url = status::list_url(&cloud.origin, &victim, 1);
        if let Some(statuses) = vc.credential_status.take() {
            let mut entries = statuses.entries().to_vec();
            entries[0].status_list_credential = victim_url.clone();
            entries[0].id = format!("{victim_url}#7");
            vc.credential_status = Some(entries.into());
        }
        vc = sign_credential(
            UnsignedCredential { credential: vc },
            &attacker,
            &derive_did_key(&attacker),
        )
        .unwrap();
        let forged = status::status_list_credential(
            &victim_url,
            &derive_did_key(&attacker),
            STATUS_PURPOSE_REVOCATION,
            &[0u8; 16],
            ISSUED,
            &attacker,
        )
        .unwrap();
        cloud.publish_lists(&attacker);
        cloud.lists.lock().unwrap().insert(
            victim_url
                .strip_prefix(&cloud.origin)
                .unwrap()
                .to_lowercase(),
            serde_json::json!(forged),
        );
        cloud.issue(&[&vc]);

        cloud.pull(&db).await;

        let written: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM credential_status_lists WHERE list_id = ?1",
                [&victim_url],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(written, 0);
    }
}
