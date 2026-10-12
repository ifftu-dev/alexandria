//! Who is really asking: checking an organisation's proof against its domain.
//!
//! Everything an organisation sends a learner — an access request, an
//! assessment request, an interview invitation, an offer — arrives through
//! Alexandria Cloud with an [`OrgProof`]. The name the app shows comes from
//! the file the organisation publishes on its own domain, fetched here
//! directly, never from the service that relayed the request. A request whose
//! proof does not check out is still shown, so the learner knows it arrived,
//! but nothing can be sent back to it.
//!
//! Descriptors are cached per domain for an hour. A proof made with a key the
//! cached copy does not list triggers one fresh fetch before it is refused, so
//! an organisation that rotates keys is not locked out for the hour.

use alexandria_verify::org::{self, OrgDescriptor, OrgError, OrgProof};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::executor::DatabaseWorkload;
use crate::profile::scope::ProfileState as State;
use crate::AppState;

/// How long a fetched descriptor is trusted without asking the domain again.
pub const DESCRIPTOR_TTL_SECS: i64 = 3600;

const MAX_DESCRIPTOR_BYTES: usize = 64 * 1024;

/// What the learner is shown about who sent something.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgCheck {
    /// The domain the organisation claims, when it claimed one.
    pub domain: Option<String>,
    /// The name its domain publishes. Only set when verified.
    pub name: Option<String>,
    pub verified: bool,
    /// Why it could not be verified, in words a learner can act on.
    pub problem: Option<String>,
}

impl OrgCheck {
    fn verified(descriptor: &OrgDescriptor) -> Self {
        Self {
            domain: Some(descriptor.domain.clone()),
            name: Some(descriptor.name.clone()),
            verified: true,
            problem: None,
        }
    }

    fn refused(domain: Option<String>, problem: impl Into<String>) -> Self {
        Self {
            domain,
            name: None,
            verified: false,
            problem: Some(problem.into()),
        }
    }

    /// The verified name, or an error to refuse an answer with.
    pub fn require(&self) -> Result<&str, String> {
        match (&self.verified, &self.name) {
            (true, Some(name)) => Ok(name),
            _ => Err(format!(
                "this organisation could not be verified{}",
                self.problem
                    .as_deref()
                    .map(|p| format!(": {p}"))
                    .unwrap_or_default()
            )),
        }
    }
}

fn cached(conn: &Connection, domain: &str) -> Result<Option<(OrgDescriptor, i64)>, String> {
    let row: Option<(String, i64)> = conn
        .query_row(
            "SELECT descriptor_json, fetched_at FROM org_descriptors WHERE domain = ?1",
            params![domain],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(row.and_then(|(json, at)| Some((serde_json::from_str(&json).ok()?, at))))
}

fn store(conn: &Connection, descriptor: &OrgDescriptor, now: i64) -> Result<(), String> {
    conn.execute(
        "INSERT INTO org_descriptors (domain, descriptor_json, fetched_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(domain) DO UPDATE SET descriptor_json = excluded.descriptor_json,
                                           fetched_at = excluded.fetched_at",
        params![
            descriptor.domain,
            serde_json::to_string(descriptor).map_err(|e| e.to_string())?,
            now
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Fetch and check `domain`'s descriptor over HTTPS.
pub(crate) async fn fetch_descriptor(domain: &str) -> Result<OrgDescriptor, String> {
    let url = org::descriptor_url(domain).map_err(|e| e.to_string())?;
    fetch_descriptor_from(&url, domain).await
}

/// The same, from an explicit URL. Split out so tests can serve a descriptor
/// from loopback; production only ever passes [`org::descriptor_url`].
pub(crate) async fn fetch_descriptor_from(
    url: &str,
    domain: &str,
) -> Result<OrgDescriptor, String> {
    let mut response = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?
        .get(url)
        .send()
        .await
        .map_err(|e| format!("could not reach {domain}: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "{domain} does not publish an organisation file ({})",
            response.status()
        ));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if body.len() + chunk.len() > MAX_DESCRIPTOR_BYTES {
            return Err(format!("{domain}'s organisation file is too large"));
        }
        body.extend_from_slice(&chunk);
    }
    let descriptor: OrgDescriptor = serde_json::from_slice(&body)
        .map_err(|e| format!("{domain}'s organisation file is unreadable: {e}"))?;
    org::check_descriptor(&descriptor, domain).map_err(|e| e.to_string())?;
    Ok(descriptor)
}

fn learner_problem(error: &OrgError) -> String {
    match error {
        OrgError::UnlistedKey => "it was signed with a key its domain does not list".to_string(),
        OrgError::WrongName { claimed, published } => {
            format!("it calls itself {claimed}, but its domain says {published}")
        }
        OrgError::WrongAudience => "it is addressed for a different organisation".to_string(),
        other => other.to_string(),
    }
}

/// Check one org-authored payload and its proof.
pub(crate) async fn check(
    state: &State<'_, AppState>,
    payload: &Value,
    proof: Option<&OrgProof>,
) -> OrgCheck {
    let Some(proof) = proof else {
        return OrgCheck::refused(None, "it is not signed by the organisation");
    };
    let domain = match org::normalise_domain(&proof.domain) {
        Ok(d) => d,
        Err(e) => return OrgCheck::refused(None, e.to_string()),
    };
    let now = chrono::Utc::now().timestamp();

    let lookup = domain.clone();
    let cached = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "org.descriptor.read",
            move |db| cached(db.conn(), &lookup),
        )
        .await
        .ok()
        .flatten();

    if let Some((descriptor, fetched_at)) = &cached {
        if now - fetched_at < DESCRIPTOR_TTL_SECS {
            match org::verify(payload, proof, descriptor) {
                Ok(()) => return OrgCheck::verified(descriptor),
                // A key the cached copy does not list may be a fresh rotation.
                Err(OrgError::UnlistedKey) => {}
                Err(e) => return OrgCheck::refused(Some(domain), learner_problem(&e)),
            }
        }
    }

    let descriptor = match fetch_descriptor(&domain).await {
        Ok(d) => d,
        Err(e) => return OrgCheck::refused(Some(domain), e),
    };
    let stored = descriptor.clone();
    let _ = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "org.descriptor.store",
            move |db| store(db.conn(), &stored, now),
        )
        .await;
    match org::verify(payload, proof, &descriptor) {
        Ok(()) => OrgCheck::verified(&descriptor),
        Err(e) => OrgCheck::refused(Some(domain), learner_problem(&e)),
    }
}

/// Check a serialisable payload. Convenience over [`check`].
pub(crate) async fn check_item<T: Serialize>(
    state: &State<'_, AppState>,
    payload: &T,
    proof: Option<&OrgProof>,
) -> OrgCheck {
    match serde_json::to_value(payload) {
        Ok(value) => check(state, &value, proof).await,
        Err(e) => OrgCheck::refused(None, e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    fn descriptor() -> OrgDescriptor {
        let key = ed25519_dalek::SigningKey::from_bytes(&[3; 32]);
        OrgDescriptor {
            format: org::DESCRIPTOR_FORMAT.into(),
            domain: "alexandria.ifftu.dev".into(),
            name: "IFFTU".into(),
            keys: vec![alexandria_verify::did::derive_did_key(&key)
                .as_str()
                .to_string()],
        }
    }

    #[test]
    fn descriptors_cache_per_domain_and_refresh_in_place() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        assert!(cached(db.conn(), "alexandria.ifftu.dev").unwrap().is_none());
        store(db.conn(), &descriptor(), 10).unwrap();
        let mut renamed = descriptor();
        renamed.name = "IFFTU Labs".into();
        store(db.conn(), &renamed, 20).unwrap();
        let (found, at) = cached(db.conn(), "alexandria.ifftu.dev").unwrap().unwrap();
        assert_eq!((found.name.as_str(), at), ("IFFTU Labs", 20));
    }

    #[tokio::test]
    async fn a_descriptor_is_fetched_checked_and_bounded() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        async fn serve(body: String) -> String {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!(
                "http://{}/.well-known/alexandria-org.json",
                listener.local_addr().unwrap()
            );
            tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                let reply = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(reply.as_bytes()).await.unwrap();
            });
            url
        }

        let good = serde_json::to_string(&descriptor()).unwrap();
        let url = serve(good).await;
        let found = fetch_descriptor_from(&url, "alexandria.ifftu.dev")
            .await
            .unwrap();
        assert_eq!(found.name, "IFFTU");

        // A file served for another domain does not speak for this one.
        let url = serve(serde_json::to_string(&descriptor()).unwrap()).await;
        assert!(fetch_descriptor_from(&url, "example.com").await.is_err());

        let url = serve("x".repeat(MAX_DESCRIPTOR_BYTES + 1)).await;
        assert!(fetch_descriptor_from(&url, "alexandria.ifftu.dev")
            .await
            .unwrap_err()
            .contains("too large"));
    }

    #[test]
    fn an_unverified_organisation_cannot_be_answered() {
        let refused = OrgCheck::refused(Some("example.com".into()), "no file");
        assert!(refused.require().unwrap_err().contains("no file"));
        assert_eq!(
            OrgCheck::verified(&descriptor()).require().unwrap(),
            "IFFTU"
        );
    }
}
