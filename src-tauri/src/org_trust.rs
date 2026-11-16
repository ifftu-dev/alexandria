//! Who is really asking: checking an organisation's proof against its domain.
//!
//! Everything an organisation sends a learner — an access request, an
//! assessment request, an interview invitation, an offer — arrives through
//! Alexandria Cloud with an [`OrgProof`]. The name the app shows comes from
//! the TXT record the organisation publishes on its own domain, looked up
//! here directly, never from the service that relayed the request. A request whose
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

use crate::db::executor::{DatabaseExecutor, DatabaseWorkload};
use crate::profile::scope::ProfileLease;
use crate::profile::scope::ProfileState as State;
use crate::AppState;

/// How long a fetched descriptor is trusted without asking the domain again.
pub const DESCRIPTOR_TTL_SECS: i64 = 3600;

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

/// The cached descriptor for `domain` if it was fetched less than
/// [`DESCRIPTOR_TTL_SECS`] before `now`. A stale or missing copy is `None`:
/// backing judged from an old copy could outlive a key the domain removed.
pub(crate) fn fresh_descriptor(
    conn: &Connection,
    domain: &str,
    now: i64,
) -> Result<Option<OrgDescriptor>, String> {
    let Ok(domain) = org::normalise_domain(domain) else {
        return Ok(None);
    };
    Ok(cached(conn, &domain)?
        .filter(|(_, fetched_at)| now - fetched_at < DESCRIPTOR_TTL_SECS)
        .map(|(descriptor, _)| descriptor))
}

/// Make sure the cache holds a fresh descriptor for `domain` that lists each
/// of `keys`, asking DNS only when the cached copy is missing, stale, or does
/// not list one of them. Afterwards [`fresh_descriptor`] reads it.
pub(crate) async fn refresh_descriptor(
    state: &State<'_, AppState>,
    domain: &str,
    keys: &[&str],
) -> Result<OrgDescriptor, String> {
    refresh_descriptor_for(
        &state.db_executor,
        DatabaseWorkload::Learner,
        state.profile_lease(),
        domain,
        keys,
    )
    .await
}

/// [`refresh_descriptor`] for callers outside a command, such as the
/// background pass, which hold the executor and a lease themselves.
pub(crate) async fn refresh_descriptor_for(
    executor: &DatabaseExecutor,
    workload: DatabaseWorkload,
    lease: ProfileLease,
    domain: &str,
    keys: &[&str],
) -> Result<OrgDescriptor, String> {
    let now = chrono::Utc::now().timestamp();
    descriptor_listing(
        executor,
        workload,
        lease,
        domain,
        keys,
        now,
        |domain| async move { fetch_descriptor(&domain).await },
    )
    .await
}

/// The descriptor `domain` publishes: the cached copy while it is fresh and
/// lists every one of `keys`, else one lookup through `fetch`, cached for
/// the next caller.
///
/// A key the cached copy does not list may be a fresh rotation, so it costs
/// one refetch rather than an hour of refusals. Every descriptor read goes
/// through here: [`check`] for an organisation's proof, and
/// [`refresh_descriptor_for`] for a credential's issuer.
async fn descriptor_listing<F, Fut>(
    executor: &DatabaseExecutor,
    workload: DatabaseWorkload,
    lease: ProfileLease,
    domain: &str,
    keys: &[&str],
    now: i64,
    fetch: F,
) -> Result<OrgDescriptor, String>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<OrgDescriptor, String>>,
{
    let domain = org::normalise_domain(domain).map_err(|e| e.to_string())?;
    let lookup = domain.clone();
    let cached = executor
        .execute(workload, lease.clone(), "org.descriptor.read", move |db| {
            fresh_descriptor(db.conn(), &lookup, now)
        })
        .await
        .unwrap_or_else(|error| {
            log::debug!("org descriptor cache for {domain}: {error}");
            None
        });
    if let Some(descriptor) = cached.filter(|descriptor| {
        keys.iter()
            .all(|key| descriptor.keys.iter().any(|listed| listed == key))
    }) {
        return Ok(descriptor);
    }
    let descriptor = fetch(domain.clone()).await?;
    let stored = descriptor.clone();
    if let Err(error) = executor
        .execute(workload, lease, "org.descriptor.store", move |db| {
            store(db.conn(), &stored, now)
        })
        .await
    {
        log::warn!("org descriptor for {domain} not cached: {error}");
    }
    Ok(descriptor)
}

/// Look up and check `domain`'s descriptor in DNS.
pub(crate) async fn fetch_descriptor(domain: &str) -> Result<OrgDescriptor, String> {
    let name = org::txt_name(domain).map_err(|e| e.to_string())?;
    let records = txt_records(&name).await?;
    org::descriptor_from_txt(domain, &records).map_err(|e| match e {
        OrgError::BadDescriptor(why) => format!("{domain}: {why} ({name})"),
        other => other.to_string(),
    })
}

/// Every TXT record at `name`, each one's strings joined. None found is an
/// empty list, not an error: "this domain says nothing" is an answer.
///
/// The system resolver first; where there is no readable system
/// configuration (Android has no resolv.conf) a public one stands in.
///
/// iOS goes straight to the public one. Its SystemConfiguration symbols are
/// link stubs that return NULL (see `build.rs`), and hickory's Apple reader
/// panics on them instead of failing, which kills the calling command.
async fn txt_records(name: &str) -> Result<Vec<String>, String> {
    use hickory_resolver::config::{ResolverConfig, CLOUDFLARE};
    use hickory_resolver::net::runtime::TokioRuntimeProvider;
    use hickory_resolver::proto::rr::RData;
    use hickory_resolver::TokioResolver;

    let public = || {
        TokioResolver::builder_with_config(
            ResolverConfig::udp_and_tcp(&CLOUDFLARE),
            TokioRuntimeProvider::default(),
        )
        .build()
    };
    #[cfg(target_os = "ios")]
    let resolver = public();
    #[cfg(not(target_os = "ios"))]
    let resolver = TokioResolver::builder_tokio()
        .and_then(|builder| builder.build())
        .or_else(|_| public());
    let resolver = resolver.map_err(|e| format!("could not set up DNS: {e}"))?;
    match resolver.txt_lookup(format!("{name}.")).await {
        Ok(found) => Ok(found
            .answers()
            .iter()
            .filter_map(|record| match &record.data {
                RData::TXT(txt) => Some(
                    txt.txt_data
                        .iter()
                        .map(|part| String::from_utf8_lossy(part).into_owned())
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect()),
        Err(e) if e.is_no_records_found() => Ok(Vec::new()),
        Err(e) => Err(format!("could not look up {name}: {e}")),
    }
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
    let descriptor =
        match refresh_descriptor(state, &domain, &[proof.verification_method.as_str()]).await {
            Ok(descriptor) => descriptor,
            Err(e) => return OrgCheck::refused(Some(domain), e),
        };
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

    #[test]
    fn only_a_fresh_cached_descriptor_resolves() {
        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        store(db.conn(), &descriptor(), 100).unwrap();
        let fresh = |now| fresh_descriptor(db.conn(), "Alexandria.IFFTU.dev", now).unwrap();
        assert_eq!(fresh(100), Some(descriptor()));
        assert_eq!(fresh(100 + DESCRIPTOR_TTL_SECS), None);
        assert_eq!(
            fresh_descriptor(db.conn(), "not a domain", 100).unwrap(),
            None
        );
    }

    /// A key the domain lists only since the descriptor was cached is seen:
    /// the cached copy does not list it, so it is fetched once more, and
    /// the fresh copy then serves later asks without another lookup.
    #[tokio::test]
    async fn a_key_added_to_the_domain_after_caching_is_seen() {
        use crate::db::executor::DatabaseExecutor;
        use crate::profile::operations::ProfileOperations;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Mutex};

        let db = Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        store(db.conn(), &descriptor(), 1_000).unwrap();
        let executor = DatabaseExecutor::new(Arc::new(Mutex::new(Some(db))));
        let operations = ProfileOperations::default();
        operations
            .activate(async { Ok(()) }, async { Ok(()) })
            .await
            .unwrap();
        let lease = operations.admit(&operations.session().unwrap()).unwrap();

        let old_key = descriptor().keys[0].clone();
        let new_key = alexandria_verify::did::derive_did_key(
            &ed25519_dalek::SigningKey::from_bytes(&[4; 32]),
        )
        .as_str()
        .to_string();
        let mut rotated = descriptor();
        rotated.keys.push(new_key.clone());

        let fetches = Arc::new(AtomicUsize::new(0));
        let ask = |key: String| {
            let (fetches, rotated, executor, lease) =
                (fetches.clone(), rotated.clone(), &executor, lease.clone());
            async move {
                descriptor_listing(
                    executor,
                    DatabaseWorkload::Learner,
                    lease,
                    "alexandria.ifftu.dev",
                    &[key.as_str()],
                    1_010,
                    |_| async move {
                        fetches.fetch_add(1, Ordering::SeqCst);
                        Ok(rotated)
                    },
                )
                .await
                .unwrap()
            }
        };

        assert_eq!(
            ask(old_key.clone()).await,
            descriptor(),
            "cached copy lists it"
        );
        assert_eq!(fetches.load(Ordering::SeqCst), 0);

        assert!(ask(new_key.clone()).await.keys.contains(&new_key));
        assert_eq!(fetches.load(Ordering::SeqCst), 1, "one refetch");

        assert!(ask(new_key).await.keys.contains(&old_key));
        assert_eq!(
            fetches.load(Ordering::SeqCst),
            1,
            "the fresh copy was cached"
        );
    }

    /// Needs the network. `cargo test --lib org_trust -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn txt_lookups_reach_real_dns() {
        let found = txt_records("alexandria.ifftu.dev").await.unwrap();
        assert!(found.iter().any(|r| r.starts_with("v=spf1")), "{found:?}");
        assert!(txt_records("_nothing-here.alexandria.ifftu.dev")
            .await
            .unwrap()
            .is_empty());
        // The test organisation's own record.
        let ifftu = fetch_descriptor("alexandria.ifftu.dev").await.unwrap();
        assert_eq!(ifftu.name, "IFFTU");
        assert_eq!(
            ifftu.keys,
            ["did:key:z6MkqWNi44j62UbaZJZnGUfGE2TMoPavjsXD99MFvpuJYkp7"]
        );
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
