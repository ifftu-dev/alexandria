// SPDX-License-Identifier: AGPL-3.0-or-later
//! Asking Alexandria Cloud what organisations hold about you, and answering
//! when they ask.
//!
//! # Where requests come from
//!
//! Organisations reach a learner through the network's Alexandria Cloud. The
//! address comes from the network profile (or a developer override in
//! [`keys::CLOUD_ORIGIN`]), and the learner has one switch,
//! [`keys::ORGANISATIONS_ENABLED`], that stops the app contacting it at all.
//! The service is not what the learner has to trust about who is asking: each
//! organisation signs what it sends with a key its own domain publishes, and
//! the app checks that itself (see `crate::org_trust`).
//!
//! # Why the client is here and the server is not
//!
//! Whether to answer a disclosure request is a decision about the learner's own
//! data, made on their machine, with their key. The code that makes it has to
//! be readable by the person it affects, which means it belongs in this
//! repository under this licence. The service on the other end holds records
//! about *other* people and does not. See `docs/enterprise-boundary.md`.
//!
//! # The proof
//!
//! Everything a directory holds about a person is answered only to that person,
//! so each read carries a signature over `METHOD\npath\ntimestamp` made with
//! the key behind the DID. A DID is public — it is published to a talent index
//! on purpose — so an endpoint keyed by DID alone would tell anybody who had
//! seen a listing who was asking about that person and who had looked at them.
//!
//! The same property that makes this work is why there is no account: the key
//! resolves out of the `did:key` itself, so a directory needs to store nothing
//! about the holder in order to authenticate them.

use crate::credentials::Issuer;
use crate::profile::scope::ProfileState as State;
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};

use crate::cloud::{self, is_allowed_origin};
use crate::db::executor::DatabaseWorkload;
use crate::settings::registry::keys;
use crate::settings::SettingsStore;
use crate::AppState;

/// The Cloud service the app asks. Kept as a list so callers that fan out
/// over services keep working; today it holds at most one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Directory {
    pub name: String,
    /// Base URL, https only (http on loopback for local testing).
    pub url: String,
}

/// One entry in a directory's record of who looked at this person.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessEntry {
    pub directory: String,
    pub institution: String,
    pub role: String,
    pub module_id: Option<String>,
    /// `named` or `aggregate`.
    pub granularity: String,
    pub at: String,
}

/// What a directory answered, or why it did not.
///
/// Failures are per directory and never abort the others. One unreachable
/// server must not hide what a second one is holding, and "we could not ask" is
/// something the learner should see rather than something that looks like
/// "nobody asked about you".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullResult<T> {
    pub items: Vec<T>,
    /// One entry per directory that could not be reached or refused.
    pub problems: Vec<Problem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Problem {
    pub directory: String,
    pub detail: String,
}

pub(crate) async fn directories(state: &State<'_, AppState>) -> Result<Vec<Directory>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "holder.directories.list",
            |db| directories_db(db.conn()),
        )
        .await
}

pub(crate) fn directories_db(conn: &rusqlite::Connection) -> Result<Vec<Directory>, String> {
    if !SettingsStore::get(conn, keys::ORGANISATIONS_ENABLED) {
        return Ok(Vec::new());
    }
    Ok(cloud::origin(conn)
        .into_iter()
        .map(|url| Directory {
            name: "Alexandria Cloud".into(),
            url,
        })
        .collect())
}

/// A value used once, so two identical requests do not sign identically.
///
/// Ed25519 is deterministic (RFC 8032), so without this a second request to the
/// same path in the same second produces the same 64 bytes as the first. The
/// service treats a proof as single-use — that is what stops a captured header
/// being replayed — and cannot tell "asked twice" from "replayed" unless the
/// client makes them different. Releasing evidence in several batches is
/// exactly that case.
pub(crate) fn nonce() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Sign the challenge a directory will check.
///
/// The path is signed along with the method, the timestamp and a per-request
/// nonce, so a proof handed to one endpoint cannot be replayed against another
/// — including another person's record on the same server — and cannot be
/// replayed against the same one twice.
pub(crate) fn proof(
    sk: &SigningKey,
    method: &str,
    path: &str,
    timestamp: i64,
    nonce: &str,
) -> String {
    let challenge = format!("{method}\n{path}\n{timestamp}\n{nonce}");
    let sig = sk.sign(challenge.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sig.to_bytes())
}

/// GET a path from a directory with a fresh proof attached.
async fn signed_get(
    client: &reqwest::Client,
    dir: &Directory,
    sk: &SigningKey,
    path: &str,
) -> Result<serde_json::Value, String> {
    // https only. This carries a list of who is interested in a named person,
    // and the proof header is a bearer credential for five minutes — neither
    // belongs on a plaintext connection.
    if !is_allowed_origin(&dir.url) {
        return Err("a directory must be https".into());
    }

    let timestamp = chrono::Utc::now().timestamp();
    let n = nonce();
    let base = dir.url.trim_end_matches('/');
    let resp = client
        .get(format!("{base}{path}"))
        .header("x-alexandria-timestamp", timestamp.to_string())
        .header("x-alexandria-nonce", &n)
        .header("x-alexandria-proof", proof(sk, "GET", path, timestamp, &n))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let code = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("{code}: {body}"));
    }
    resp.json().await.map_err(|e| e.to_string())
}

// --- tauri command handlers ----------------------------------------------

#[tauri::command]
pub async fn fetch_access_log(
    state: State<'_, AppState>,
) -> Result<PullResult<AccessEntry>, String> {
    let dirs = directories(&state).await?;
    let (sk, did) = Issuer::from_keystore(&state.keystore).await?.into_parts();
    let client = reqwest::Client::new();

    let mut items = Vec::new();
    let mut problems = Vec::new();

    for dir in &dirs {
        let path = format!("/api/cohorts/access-log/{}", did.as_str());
        match signed_get(&client, dir, &sk, &path).await {
            Ok(value) => {
                let rows: Vec<serde_json::Value> = value
                    .get("accesses")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                for r in rows {
                    items.push(AccessEntry {
                        directory: dir.name.clone(),
                        institution: string_at(&r, "institution"),
                        role: string_at(&r, "role"),
                        module_id: r
                            .get("moduleId")
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                        granularity: string_at(&r, "granularity"),
                        at: string_at(&r, "at"),
                    });
                }
            }
            Err(detail) => problems.push(Problem {
                directory: dir.name.clone(),
                detail,
            }),
        }
    }

    Ok(PullResult { items, problems })
}

fn string_at(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string()
}

/// One institution's standing permission to see this person by name.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Grant {
    pub directory: String,
    /// Needed to withdraw it. A learner who can see the grant but cannot name
    /// the institution back to the endpoint has been shown a control they
    /// cannot reach.
    pub organisation_id: String,
    pub institution: String,
    pub visible_to_administrators: bool,
}

/// What this person has granted, per institution, across every directory.
#[tauri::command]
pub async fn fetch_visibility(state: State<'_, AppState>) -> Result<PullResult<Grant>, String> {
    let dirs = directories(&state).await?;
    let (sk, did) = Issuer::from_keystore(&state.keystore).await?.into_parts();
    let client = reqwest::Client::new();

    let mut items = Vec::new();
    let mut problems = Vec::new();

    for dir in &dirs {
        let path = format!("/api/cohorts/visibility/{}", did.as_str());
        match signed_get(&client, dir, &sk, &path).await {
            Ok(value) => {
                let rows: Vec<serde_json::Value> = value
                    .get("grants")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                for r in rows {
                    items.push(Grant {
                        directory: dir.name.clone(),
                        organisation_id: string_at(&r, "organisationId"),
                        institution: string_at(&r, "institution"),
                        visible_to_administrators: r
                            .get("visibleToAdministrators")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                    });
                }
            }
            Err(detail) => problems.push(Problem {
                directory: dir.name.clone(),
                detail,
            }),
        }
    }

    Ok(PullResult { items, problems })
}

/// Grant or withdraw an institution's administrators named visibility.
///
/// Signed by the learner, because an institution that could set this for its
/// own students would make the consent decorative. Withdrawal goes through the
/// same call — a control that can only be switched on is not a control.
#[tauri::command]
pub async fn set_visibility(
    state: State<'_, AppState>,
    directory_url: String,
    organisation_id: String,
    visible: bool,
) -> Result<(), String> {
    if !is_allowed_origin(&directory_url) {
        return Err("a directory must be https".into());
    }

    let (sk, did) = Issuer::from_keystore(&state.keystore).await?.into_parts();
    let now = crate::clock::now_rfc3339();

    // A record with no skills in it. The endpoint checks the signature and the
    // subject, and nothing else; sending the published listing would attach a
    // skill disclosure to what is only a consent change.
    let record = crate::domain::talent_index::TalentIndexRecord {
        subject_did: did.as_str().to_string(),
        skills: vec![],
        display_name: None,
        bio: None,
        valid_until: (chrono::Utc::now() + chrono::Duration::minutes(10))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    };
    let signed = crate::domain::talent_index::sign_record(&record, &sk, &now)?;

    let base = directory_url.trim_end_matches('/');
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/cohorts/visibility/{}", did.as_str()))
        .json(&serde_json::json!({
            "org_id": organisation_id,
            "visible": visible,
            "presentation": signed,
        }))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let code = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("{code}: {body}"));
    }
    Ok(())
}

/// Publish the consented listing to Alexandria Cloud, where employers search.
///
/// What goes is exactly what the consent screen previews, so the preview stays
/// the audit surface and this is only delivery. Recorded afterwards so the
/// automatic pass does not send the same listing again.
#[tauri::command]
pub async fn publish_listing(state: State<'_, AppState>) -> Result<(), String> {
    let cloud = directories(&state)
        .await?
        .into_iter()
        .next()
        .ok_or("requests from organisations are switched off, or no Alexandria Cloud is set")?;
    let (key, did) = Issuer::from_keystore(&state.keystore).await?.into_parts();
    let now = crate::clock::now_rfc3339();
    let now_unix = chrono::Utc::now().timestamp();
    let (sign_key, subject, at) = (key.clone(), did.as_str().to_string(), now.clone());
    let (digest, signed) = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "holder.listing.sign",
            move |db| {
                let record = crate::commands::talent_index::get_talent_index_preview_impl(
                    db.conn(),
                    &subject,
                    &at,
                )?
                .record
                .ok_or(
                    "you have not chosen any skills to publish yet — choose what to share first",
                )?;
                Ok((
                    crate::commands::organisations::snapshot_digest(&record)?,
                    crate::domain::talent_index::sign_record(&record, &sign_key, &at)?,
                ))
            },
        )
        .await?;
    crate::commands::organisations::send_listing(&cloud.url, &signed).await?;
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "holder.listing.record",
            move |db| {
                crate::commands::organisations::record_listing(
                    db.conn(),
                    &cloud.url,
                    &digest,
                    now_unix,
                )
            },
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::did::derive_did_key;
    use crate::db::Database;

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    /// The proof the directory checks is a signature over the exact challenge
    /// it reconstructs. Pinned here because the two halves live in different
    /// repositories, and a format described in prose in both of them drifts.
    #[test]
    fn a_proof_verifies_against_the_challenge_the_server_rebuilds() {
        use ed25519_dalek::Verifier;

        let sk = key();
        let encoded = proof(
            &sk,
            "GET",
            "/api/cohorts/access-log/did:key:zAbc",
            1_786_600_000,
            "n0nce",
        );
        let sig_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded.as_bytes())
            .expect("proofs are unpadded url-safe base64");
        let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes.try_into().unwrap());

        let rebuilt = "GET\n/api/cohorts/access-log/did:key:zAbc\n1786600000\nn0nce";
        assert!(sk.verifying_key().verify(rebuilt.as_bytes(), &sig).is_ok());
    }

    /// The same pinning for the two methods that change something.
    ///
    /// `holder_release` signs a POST to send evidence and a DELETE to take it
    /// back, and the service binds a proof to its method precisely so that one
    /// cannot be replayed as the other. A release replayable as a withdrawal —
    /// or worse, the reverse — would be a proof that does the opposite of what
    /// the person holding it intended.
    #[test]
    fn a_release_and_a_withdrawal_verify_against_what_the_server_rebuilds() {
        use ed25519_dalek::Verifier;

        let sk = key();
        let path = "/api/runs/0b1e9f4a-0000-4000-8000-000000000000/evidence";

        for method in ["POST", "DELETE"] {
            let encoded = proof(&sk, method, path, 1_786_600_000, "n0nce");
            let sig_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(encoded.as_bytes())
                .expect("proofs are unpadded url-safe base64");
            let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes.try_into().unwrap());

            let rebuilt = format!("{method}\n{path}\n1786600000\nn0nce");
            assert!(
                sk.verifying_key().verify(rebuilt.as_bytes(), &sig).is_ok(),
                "{method} proof did not verify against the challenge the service rebuilds"
            );
        }
    }

    /// Two identical requests must not sign identically.
    ///
    /// Ed25519 is deterministic, and the service refuses a proof it has already
    /// seen. Without a per-request nonce, a release sent in several batches
    /// signs the same bytes every time and every batch after the first is
    /// refused as a replay — at the moment somebody is contesting a flag.
    #[test]
    fn the_same_request_twice_produces_different_proofs() {
        let sk = key();
        let path = "/api/runs/abc/evidence";
        assert_ne!(
            proof(&sk, "POST", path, 1, &nonce()),
            proof(&sk, "POST", path, 1, &nonce()),
            "asking twice must not look like a replay"
        );
    }

    /// And a nonce is a value used once, not a constant.
    #[test]
    fn a_nonce_does_not_repeat() {
        assert_ne!(nonce(), nonce());
    }

    /// Sending and taking back are different acts and must not share a proof.
    #[test]
    fn a_proof_is_specific_to_its_method() {
        let sk = key();
        let path = "/api/runs/abc/evidence";
        assert_ne!(
            proof(&sk, "POST", path, 1, "n"),
            proof(&sk, "DELETE", path, 1, "n"),
            "a proof to release must not also withdraw"
        );
    }

    #[test]
    fn a_proof_is_specific_to_its_path() {
        let sk = key();
        let a = proof(&sk, "GET", "/api/a", 1, "n");
        let b = proof(&sk, "GET", "/api/b", 1, "n");
        assert_ne!(a, b, "a proof for one path must not open another");
    }

    #[test]
    fn a_proof_is_specific_to_its_moment() {
        let sk = key();
        assert_ne!(
            proof(&sk, "GET", "/api/a", 1, "n"),
            proof(&sk, "GET", "/api/a", 2, "n")
        );
    }

    /// Only the holder's own key produces a proof their DID accepts.
    #[test]
    fn another_key_produces_a_different_proof() {
        let mine = key();
        let theirs = SigningKey::from_bytes(&[9u8; 32]);
        assert_ne!(
            proof(&mine, "GET", "/api/a", 1, "n"),
            proof(&theirs, "GET", "/api/a", 1, "n")
        );
        assert_ne!(
            derive_did_key(&mine).as_str(),
            derive_did_key(&theirs).as_str()
        );
    }

    #[test]
    fn cloud_is_the_override_else_the_profile_and_the_switch_stops_it() {
        let db = Database::open_in_memory().expect("database");
        db.run_migrations().expect("migrations");
        let profile_origin = crate::network_profile::embedded_preprod()
            .ok()
            .and_then(|p| p.cloud_https_origin.clone());
        assert_eq!(cloud::origin(db.conn()), profile_origin);

        SettingsStore::set(
            db.conn(),
            keys::CLOUD_ORIGIN,
            "http://127.0.0.1:8787/".into(),
        )
        .expect("override");
        assert_eq!(
            cloud::origin(db.conn()).as_deref(),
            Some("http://127.0.0.1:8787")
        );
        assert_eq!(directories_db(db.conn()).expect("services").len(), 1);

        // Plaintext to anywhere but loopback is not a Cloud the app will use.
        SettingsStore::set(db.conn(), keys::CLOUD_ORIGIN, "http://cloud.example".into())
            .expect("override");
        assert_eq!(cloud::origin(db.conn()), None);

        SettingsStore::set(
            db.conn(),
            keys::CLOUD_ORIGIN,
            "https://cloud.example".into(),
        )
        .expect("override");
        SettingsStore::set(db.conn(), keys::ORGANISATIONS_ENABLED, false).expect("switch");
        assert!(directories_db(db.conn()).expect("services").is_empty());
        // Status lists still have a home: the switch is about requests.
        assert_eq!(
            cloud::origin(db.conn()).as_deref(),
            Some("https://cloud.example")
        );
    }
}
