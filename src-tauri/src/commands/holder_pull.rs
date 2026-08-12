// SPDX-License-Identifier: AGPL-3.0-or-later
//! Asking a directory what it holds about you, and answering when it asks.
//!
//! # What a directory is
//!
//! A service somebody else runs that holds records about this person: an
//! institution's registry, an employer's index, anything that has been told a
//! DID. None is configured by default and none is discovered automatically —
//! the list lives in [`keys::HOLDER_DIRECTORIES`] and is empty until the
//! learner puts something in it. An offline-first application does not decide
//! on its user's behalf to start talking to a server.
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

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::settings::registry::{keys, JsonSetting};
use crate::settings::SettingsStore;
use crate::AppState;

/// A directory the learner has chosen to check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Directory {
    /// What the learner calls it. Their label, not the server's — a service
    /// that names itself in a list of things watching you is a service that
    /// gets to choose how it is described.
    pub name: String,
    /// Base URL, https only.
    pub url: String,
}

/// One request for a disclosure, as the holder sees it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisclosureRequest {
    /// Which directory it came from, so a learner with several can tell.
    pub directory: String,
    pub id: String,
    /// The organisation that is asking, by name.
    pub from: String,
    pub skill_ids: Vec<String>,
    pub purpose: String,
    pub expires_at: String,
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

fn directories(state: &State<'_, AppState>) -> Result<Vec<Directory>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    let JsonSetting(raw) = SettingsStore::get(db.conn(), keys::HOLDER_DIRECTORIES);
    serde_json::from_value(raw).map_err(|e| format!("the directory list is unreadable: {e}"))
}

/// Sign the challenge a directory will check.
///
/// The path is signed along with the method and the timestamp, so a proof
/// handed to one endpoint cannot be replayed against another — including
/// another person's record on the same server.
fn proof(sk: &SigningKey, method: &str, path: &str, timestamp: i64) -> String {
    let challenge = format!("{method}\n{path}\n{timestamp}");
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
    if !dir.url.starts_with("https://") && !is_loopback(&dir.url) {
        return Err("a directory must be https".into());
    }

    let timestamp = chrono::Utc::now().timestamp();
    let base = dir.url.trim_end_matches('/');
    let resp = client
        .get(format!("{base}{path}"))
        .header("x-alexandria-timestamp", timestamp.to_string())
        .header("x-alexandria-proof", proof(sk, "GET", path, timestamp))
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

/// Loopback is allowed so somebody can run a directory locally and try this
/// without a certificate. Nothing else plaintext is.
///
/// The host has to *end* there. A prefix check alone would accept
/// `http://127.0.0.1.example.com`, which is a domain somebody else controls
/// wearing a loopback address as a costume — and it would arrive over
/// plaintext carrying a proof header.
fn is_loopback(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    for host in ["127.0.0.1", "localhost", "[::1]"] {
        if let Some(after) = rest.strip_prefix(host) {
            if after.is_empty() || after.starts_with(':') || after.starts_with('/') {
                return true;
            }
        }
    }
    false
}

// --- tauri command handlers ----------------------------------------------

#[tauri::command]
pub async fn list_directories(state: State<'_, AppState>) -> Result<Vec<Directory>, String> {
    directories(&state)
}

#[tauri::command]
pub async fn set_directories(
    state: State<'_, AppState>,
    directories: Vec<Directory>,
) -> Result<(), String> {
    for d in &directories {
        if !d.url.starts_with("https://") && !is_loopback(&d.url) {
            return Err(format!("{} must be an https URL", d.url));
        }
    }

    let db_guard = state
        .db
        .lock()
        .map_err(|_| "database lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("database not initialized")?;
    SettingsStore::set(
        db.conn(),
        keys::HOLDER_DIRECTORIES,
        JsonSetting(serde_json::to_value(&directories).map_err(|e| e.to_string())?),
    )
    .map_err(|e| e.to_string())
}

/// Who is asking for a disclosure, across every configured directory.
#[tauri::command]
pub async fn fetch_disclosure_requests(
    state: State<'_, AppState>,
) -> Result<PullResult<DisclosureRequest>, String> {
    let dirs = directories(&state)?;
    let (sk, did) = crate::commands::credentials::load_issuer_key(&state).await?;
    let client = reqwest::Client::new();

    let mut items = Vec::new();
    let mut problems = Vec::new();

    for dir in &dirs {
        let path = format!("/api/presentations/for/{}", did.as_str());
        match signed_get(&client, dir, &sk, &path).await {
            Ok(value) => {
                let rows: Vec<serde_json::Value> =
                    serde_json::from_value(value).unwrap_or_default();
                for r in rows {
                    items.push(DisclosureRequest {
                        directory: dir.name.clone(),
                        id: string_at(&r, "id"),
                        from: string_at(&r, "from"),
                        skill_ids: r
                            .get("skillIds")
                            .and_then(|v| serde_json::from_value(v.clone()).ok())
                            .unwrap_or_default(),
                        purpose: string_at(&r, "purpose"),
                        expires_at: string_at(&r, "expiresAt"),
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

/// Who has looked at this person's outcomes, across every configured directory.
#[tauri::command]
pub async fn fetch_access_log(
    state: State<'_, AppState>,
) -> Result<PullResult<AccessEntry>, String> {
    let dirs = directories(&state)?;
    let (sk, did) = crate::commands::credentials::load_issuer_key(&state).await?;
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

/// Answer a disclosure request by sharing a signed record.
///
/// The record is the one the talent-index consent screen already builds and
/// signs, so what leaves the device here is exactly what the learner has
/// already agreed can leave it, and there is one place to audit rather than
/// two.
///
/// There is no counterpart that declines. A request that is not answered
/// expires, and adding a "refused" call would turn a consent mechanism into a
/// record of who said no to whom.
#[tauri::command]
pub async fn share_disclosure(
    state: State<'_, AppState>,
    directory_url: String,
    request_id: String,
) -> Result<(), String> {
    if !directory_url.starts_with("https://") && !is_loopback(&directory_url) {
        return Err("a directory must be https".into());
    }

    let signed = crate::commands::talent_index::sign_talent_index_record(state)
        .await?
        .ok_or_else(|| {
            "you have not consented to publish any skills yet — choose what to share first"
                .to_string()
        })?;

    let base = directory_url.trim_end_matches('/');
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/presentations/{request_id}/share"))
        .json(&serde_json::json!({ "presentation": signed }))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::did::derive_did_key;

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
        );
        let sig_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded.as_bytes())
            .expect("proofs are unpadded url-safe base64");
        let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes.try_into().unwrap());

        let rebuilt = "GET\n/api/cohorts/access-log/did:key:zAbc\n1786600000";
        assert!(sk.verifying_key().verify(rebuilt.as_bytes(), &sig).is_ok());
    }

    #[test]
    fn a_proof_is_specific_to_its_path() {
        let sk = key();
        let a = proof(&sk, "GET", "/api/a", 1);
        let b = proof(&sk, "GET", "/api/b", 1);
        assert_ne!(a, b, "a proof for one path must not open another");
    }

    #[test]
    fn a_proof_is_specific_to_its_moment() {
        let sk = key();
        assert_ne!(
            proof(&sk, "GET", "/api/a", 1),
            proof(&sk, "GET", "/api/a", 2)
        );
    }

    /// Only the holder's own key produces a proof their DID accepts.
    #[test]
    fn another_key_produces_a_different_proof() {
        let mine = key();
        let theirs = SigningKey::from_bytes(&[9u8; 32]);
        assert_ne!(
            proof(&mine, "GET", "/api/a", 1),
            proof(&theirs, "GET", "/api/a", 1)
        );
        assert_ne!(
            derive_did_key(&mine).as_str(),
            derive_did_key(&theirs).as_str()
        );
    }

    #[test]
    fn plaintext_is_refused_except_on_loopback() {
        assert!(is_loopback("http://127.0.0.1:8787"));
        assert!(is_loopback("http://localhost:8787"));
        assert!(!is_loopback("http://registry.example.com"));
        assert!(!is_loopback("http://127.0.0.1.example.com"));
    }
}
