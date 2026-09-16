use std::future::Future;
use std::time::Duration;

use futures::stream::{self, StreamExt};
use serde::Serialize;

use crate::content_store::resolver::ContentResolver;
use crate::db::executor::DatabaseWorkload;
use crate::db::governance_genesis::{load_pinned_genesis, pin_genesis};
use crate::domain::governance_certificate::{
    decode_and_verify_genesis, FoundingGenesisEnvelope, VerifiedFoundingGenesis,
    MAX_GOVERNANCE_GENESIS_BYTES,
};
use crate::domain::governance_locator::GovernanceGenesisLocator;
use crate::profile::scope::ProfileState as State;
use crate::AppState;

const GENESIS_SOURCE_CONCURRENCY: usize = 3;
const GENESIS_SOURCE_TIMEOUT: Duration = Duration::from_secs(10);
const GENESIS_OVERALL_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy)]
struct RetrievalPolicy {
    max_concurrent: usize,
    per_source_timeout: Duration,
    overall_timeout: Duration,
}

const GENESIS_RETRIEVAL_POLICY: RetrievalPolicy = RetrievalPolicy {
    max_concurrent: GENESIS_SOURCE_CONCURRENCY,
    per_source_timeout: GENESIS_SOURCE_TIMEOUT,
    overall_timeout: GENESIS_OVERALL_TIMEOUT,
};

#[derive(Debug, Clone, Serialize)]
pub struct GenesisMemberPreview {
    pub member_id: String,
    pub identity_public_key_hex: String,
    pub consensus_public_key_hex: String,
    pub governance_public_key_hex: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GenesisPreview {
    pub dao_id: String,
    /// Domain-separated hash of the canonical genesis core. Equal to `dao_id`,
    /// and identical for every valid envelope over the same core.
    pub core_hash: String,
    /// BLAKE3 of these exact canonical envelope bytes, including signatures.
    /// A locator's content hash names this value.
    pub envelope_hash: String,
    pub name: String,
    pub scope_type: String,
    pub scope_id: String,
    pub protocol_version: u16,
    pub rules_version: String,
    pub rules_hash: String,
    pub proposal_approval_numerator: u32,
    pub proposal_approval_denominator: u32,
    pub minimum_turnout_count: u64,
    pub committee_size: u8,
    pub receipt_threshold: u8,
    pub outcome_threshold: u8,
    pub qualification_policy_version: String,
    pub accepted_issuers: Vec<String>,
    pub accepted_assessment_evidence: Vec<String>,
    pub cometbft_chain_id: String,
    pub initial_epoch: u64,
    pub initial_height: u64,
    pub activation_time_unix: i64,
    pub members: Vec<GenesisMemberPreview>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PinGenesisResponse {
    pub preview: GenesisPreview,
    pub newly_pinned: bool,
    /// A differently signed envelope over the same core was already pinned.
    /// The stored bytes were kept and the reviewed bytes were not stored.
    pub stored_envelope_differs: bool,
}

/// A parsed locator plus its canonical encoding. Retrieval and sharing use
/// `canonical_uri`, never the text the user typed.
#[derive(Debug, Clone, Serialize)]
pub struct ReviewedGenesisLocator {
    #[serde(flatten)]
    pub locator: GovernanceGenesisLocator,
    pub canonical_uri: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RetrievedGenesisPreview {
    pub locator: GovernanceGenesisLocator,
    pub resolved_from: String,
    pub genesis_json: String,
    pub preview: GenesisPreview,
}

fn checked_genesis(
    genesis_json: &str,
    expected_dao_id: Option<&str>,
) -> Result<(FoundingGenesisEnvelope, VerifiedFoundingGenesis), String> {
    if genesis_json.len() > MAX_GOVERNANCE_GENESIS_BYTES {
        return Err(format!(
            "governance genesis exceeds the {MAX_GOVERNANCE_GENESIS_BYTES}-byte limit"
        ));
    }
    decode_and_verify_genesis(genesis_json.as_bytes(), expected_dao_id)
        .map_err(|error| error.to_string())
}

fn preview(
    envelope: &FoundingGenesisEnvelope,
    verified: &VerifiedFoundingGenesis,
    canonical_bytes: &[u8],
) -> GenesisPreview {
    GenesisPreview {
        dao_id: verified.dao_id().to_owned(),
        core_hash: verified.genesis_hash().to_owned(),
        envelope_hash: blake3::hash(canonical_bytes).to_hex().to_string(),
        name: envelope.core.name.clone(),
        scope_type: envelope.core.scope.scope_type.clone(),
        scope_id: envelope.core.scope.scope_id.clone(),
        protocol_version: envelope.core.protocol_version,
        rules_version: envelope.core.rules.rules_version.clone(),
        rules_hash: verified.rules_hash().to_owned(),
        proposal_approval_numerator: envelope.core.rules.proposal_approval_numerator,
        proposal_approval_denominator: envelope.core.rules.proposal_approval_denominator,
        minimum_turnout_count: envelope.core.rules.minimum_turnout_count,
        committee_size: envelope.core.rules.committee_size,
        receipt_threshold: envelope.core.rules.receipt_threshold,
        outcome_threshold: envelope.core.rules.outcome_threshold,
        qualification_policy_version: envelope.core.qualification_policy.policy_version.clone(),
        accepted_issuers: envelope.core.qualification_policy.accepted_issuers.clone(),
        accepted_assessment_evidence: envelope
            .core
            .qualification_policy
            .accepted_assessment_evidence
            .clone(),
        cometbft_chain_id: envelope.core.activation.cometbft_chain_id.clone(),
        initial_epoch: envelope.core.activation.initial_epoch,
        initial_height: envelope.core.activation.initial_height,
        activation_time_unix: envelope.core.activation.activation_time_unix,
        members: envelope
            .core
            .members
            .iter()
            .map(|member| GenesisMemberPreview {
                member_id: member.member_id.clone(),
                identity_public_key_hex: member.identity_public_key_hex.clone(),
                consensus_public_key_hex: member.consensus_public_key_hex.clone(),
                governance_public_key_hex: member.governance_public_key_hex.clone(),
            })
            .collect(),
    }
}

fn reviewed_locator(locator_uri: &str) -> Result<ReviewedGenesisLocator, String> {
    let locator =
        GovernanceGenesisLocator::parse(locator_uri).map_err(|error| error.to_string())?;
    let canonical_uri = locator.encode().map_err(|error| error.to_string())?;
    Ok(ReviewedGenesisLocator {
        locator,
        canonical_uri,
    })
}

async fn first_success_with_policy<Source, Value, Attempt, AttemptFuture>(
    sources: impl IntoIterator<Item = Source>,
    mut attempt: Attempt,
    policy: RetrievalPolicy,
) -> Result<Option<Value>, tokio::time::error::Elapsed>
where
    Attempt: FnMut(Source) -> AttemptFuture,
    AttemptFuture: Future<Output = Option<Value>>,
{
    debug_assert!(policy.max_concurrent > 0);
    let attempts = stream::iter(sources)
        .map(move |source| {
            let future = attempt(source);
            async move {
                tokio::time::timeout(policy.per_source_timeout, future)
                    .await
                    .ok()
                    .flatten()
            }
        })
        .buffer_unordered(policy.max_concurrent);
    futures::pin_mut!(attempts);

    tokio::time::timeout(policy.overall_timeout, async move {
        while let Some(result) = attempts.next().await {
            if result.is_some() {
                return result;
            }
        }
        None
    })
    .await
}

async fn retrieve_verified_source(
    resolver: &ContentResolver,
    locator: &GovernanceGenesisLocator,
    identifier: String,
) -> Option<(String, String, GenesisPreview)> {
    let result = match resolver
        .resolve_preview_bounded(&identifier, MAX_GOVERNANCE_GENESIS_BYTES)
        .await
    {
        Ok(result) => result,
        Err(error) => {
            log::debug!("governance genesis source unavailable: {error}");
            return None;
        }
    };
    let actual_hash = blake3::hash(&result.bytes).to_hex().to_string();
    if result.bytes.len() > MAX_GOVERNANCE_GENESIS_BYTES
        || result.blake3_hash != locator.content_hash
        || actual_hash != locator.content_hash
    {
        log::warn!("governance genesis source returned content outside its locator binding");
        return None;
    }
    let genesis_json = match String::from_utf8(result.bytes) {
        Ok(genesis_json) => genesis_json,
        Err(_) => {
            log::warn!("governance genesis source returned non-UTF-8 content");
            return None;
        }
    };
    let (envelope, verified) = match checked_genesis(&genesis_json, Some(&locator.dao_id)) {
        Ok(genesis) => genesis,
        Err(_) => {
            log::warn!("governance genesis source returned an invalid canonical envelope");
            return None;
        }
    };
    let preview = preview(&envelope, &verified, genesis_json.as_bytes());
    Some((identifier, genesis_json, preview))
}

/// Verify canonical genesis bytes and return every material trust fact without
/// changing local trust state.
#[tauri::command]
pub async fn governance_preview_genesis(
    _profile: crate::profile::scope::ProfileLease,
    genesis_json: String,
) -> Result<GenesisPreview, String> {
    let (envelope, verified) = checked_genesis(&genesis_json, None)?;
    Ok(preview(&envelope, &verified, genesis_json.as_bytes()))
}

/// Parse and normalize a portable locator without fetching content or
/// changing either the content cache or local governance trust state.
#[tauri::command]
pub async fn governance_preview_genesis_locator(
    _profile: crate::profile::scope::ProfileLease,
    locator_uri: String,
) -> Result<ReviewedGenesisLocator, String> {
    reviewed_locator(&locator_uri)
}

/// Retrieve a locator's canonical JSON and verify both layers of content
/// addressing. This deliberately stops at preview: only
/// `governance_pin_genesis` can create the local trust anchor. Only the
/// locator's own normalized sources are fetched.
#[tauri::command]
pub async fn governance_retrieve_genesis(
    state: State<'_, AppState>,
    locator_uri: String,
) -> Result<RetrievedGenesisPreview, String> {
    let locator =
        GovernanceGenesisLocator::parse(&locator_uri).map_err(|error| error.to_string())?;
    let resolver = {
        let guard = state.resolver.lock().await;
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| "content resolver not initialized".to_string())?
    };

    let identifiers = locator
        .retrieval_identifiers()
        .map_err(|error| error.to_string())?;
    let retrieved = first_success_with_policy(
        identifiers,
        |identifier| retrieve_verified_source(&resolver, &locator, identifier),
        GENESIS_RETRIEVAL_POLICY,
    )
    .await;

    match retrieved {
        Ok(Some((resolved_from, genesis_json, preview))) => Ok(RetrievedGenesisPreview {
            locator,
            resolved_from,
            genesis_json,
            preview,
        }),
        Ok(None) => {
            Err("no locator source returned the exact verified governance genesis".to_string())
        }
        Err(_) => Err("governance genesis retrieval exceeded the 30-second limit".to_string()),
    }
}

/// Persist an explicit local trust decision. The caller must echo the exact
/// derived DAO id shown by `governance_preview_genesis`; a boolean confirmation
/// or a matching display name is deliberately insufficient.
#[tauri::command]
pub async fn governance_pin_genesis(
    state: State<'_, AppState>,
    genesis_json: String,
    expected_dao_id: String,
) -> Result<PinGenesisResponse, String> {
    let (envelope, verified) = checked_genesis(&genesis_json, Some(&expected_dao_id))?;
    let expected_preview = preview(&envelope, &verified, genesis_json.as_bytes());
    let bytes = genesis_json.into_bytes();
    let pinned = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "governance.pin-genesis",
            move |db| pin_genesis(db.conn(), &bytes).map_err(|error| error.to_string()),
        )
        .await?;
    if pinned.verified.dao_id() != expected_preview.dao_id || pinned.envelope.core != envelope.core
    {
        return Err("pinned governance genesis did not match the reviewed envelope".to_string());
    }
    Ok(PinGenesisResponse {
        preview: expected_preview,
        newly_pinned: pinned.newly_pinned,
        stored_envelope_differs: pinned.stored_envelope_differs,
    })
}

/// Return the exact canonical bytes of a locally pinned trust anchor.
#[tauri::command]
pub async fn governance_get_pinned_genesis(
    state: State<'_, AppState>,
    dao_id: String,
) -> Result<Option<String>, String> {
    let bytes = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "governance.get-pinned-genesis",
            move |db| load_pinned_genesis(db.conn(), &dao_id).map_err(|error| error.to_string()),
        )
        .await?;
    bytes
        .map(|bytes| {
            String::from_utf8(bytes)
                .map_err(|_| "pinned governance genesis is not valid UTF-8".to_string())
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use super::*;
    use crate::content_store::http::HttpClient;
    use crate::content_store::node::ContentNode;
    use crate::domain::governance_certificate::test_support::signed_genesis;

    struct ActiveAttempt {
        active: Arc<AtomicUsize>,
    }

    impl Drop for ActiveAttempt {
        fn drop(&mut self) {
            self.active.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn record_peak(peak: &AtomicUsize, active: usize) {
        let mut observed = peak.load(Ordering::SeqCst);
        while active > observed {
            match peak.compare_exchange(observed, active, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => break,
                Err(current) => observed = current,
            }
        }
    }

    #[tokio::test]
    async fn retrieval_races_no_more_than_the_configured_source_limit() {
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let policy = RetrievalPolicy {
            max_concurrent: 3,
            per_source_timeout: Duration::from_millis(100),
            overall_timeout: Duration::from_millis(200),
        };

        let result = first_success_with_policy(
            0..6,
            |source| {
                let active = active.clone();
                let peak = peak.clone();
                async move {
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    record_peak(&peak, current);
                    let _attempt = ActiveAttempt { active };
                    if source == 1 {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        Some(source)
                    } else {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        None
                    }
                }
            },
            policy,
        )
        .await
        .unwrap();

        assert_eq!(result, Some(1));
        assert_eq!(peak.load(Ordering::SeqCst), 3);
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn per_source_timeouts_release_slots_for_later_sources() {
        let policy = RetrievalPolicy {
            max_concurrent: 3,
            per_source_timeout: Duration::from_millis(10),
            overall_timeout: Duration::from_millis(100),
        };

        let result = first_success_with_policy(
            0..5,
            |source| async move {
                if source < 3 {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    None
                } else if source == 3 {
                    Some(source)
                } else {
                    None
                }
            },
            policy,
        )
        .await
        .unwrap();

        assert_eq!(result, Some(3));
    }

    #[tokio::test]
    async fn overall_timeout_cancels_the_source_race() {
        let policy = RetrievalPolicy {
            max_concurrent: 1,
            per_source_timeout: Duration::from_millis(100),
            overall_timeout: Duration::from_millis(10),
        };

        let result = first_success_with_policy(
            0..2,
            |_| async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                None::<usize>
            },
            policy,
        )
        .await;

        assert!(result.is_err());
    }

    /// Connect-time resolver that never answers within any test budget.
    #[derive(Default)]
    struct SlowResolver {
        lookups: AtomicUsize,
    }

    impl reqwest::dns::Resolve for SlowResolver {
        fn resolve(&self, _name: reqwest::dns::Name) -> reqwest::dns::Resolving {
            self.lookups.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Err::<reqwest::dns::Addrs, _>(
                    Box::new(std::io::Error::other("slow resolver")) as Box<_>
                )
            })
        }
    }

    fn slow_dns_resolver() -> (ContentResolver, Arc<SlowResolver>, tempfile::TempDir) {
        let directory = tempfile::TempDir::new().unwrap();
        let dns = Arc::new(SlowResolver::default());
        let http = HttpClient::with_dns_resolver(Duration::from_secs(60), dns.clone()).unwrap();
        let resolver = ContentResolver::new(
            Arc::new(ContentNode::new(directory.path())),
            http,
            Arc::new(Mutex::new(None)),
        );
        (resolver, dns, directory)
    }

    fn slow_sources(count: usize) -> Vec<String> {
        (0..count)
            .map(|index| format!("https://slow-{index}.example.org/genesis.json"))
            .collect()
    }

    // Regression: the preview path used to run a blocking `to_socket_addrs`
    // pre-check inside the race, which no tokio timeout can pre-empt. Every
    // name here would stall DNS; the budgets must still hold, and the lookups
    // must reach the async connect-time resolver rather than a blocking one.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slow_dns_cannot_stretch_the_per_source_budget() {
        let (resolver, dns, _directory) = slow_dns_resolver();
        let policy = RetrievalPolicy {
            max_concurrent: 3,
            per_source_timeout: Duration::from_millis(100),
            overall_timeout: Duration::from_secs(5),
        };

        let started = Instant::now();
        let result = first_success_with_policy(
            slow_sources(8),
            |url| {
                let resolver = resolver.clone();
                async move { resolver.resolve_preview_bounded(&url, 1024).await.ok() }
            },
            policy,
        )
        .await;
        let elapsed = started.elapsed();

        assert!(matches!(result, Ok(None)));
        // Three 100 ms rounds for eight sources; generous slack for CI.
        assert!(elapsed < Duration::from_millis(1_500), "took {elapsed:?}");
        assert_eq!(dns.lookups.load(Ordering::SeqCst), 8);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn slow_dns_cannot_stretch_the_overall_budget() {
        let (resolver, dns, _directory) = slow_dns_resolver();
        let policy = RetrievalPolicy {
            max_concurrent: 3,
            per_source_timeout: Duration::from_secs(10),
            overall_timeout: Duration::from_millis(200),
        };

        let started = Instant::now();
        let result = first_success_with_policy(
            slow_sources(8),
            |url| {
                let resolver = resolver.clone();
                async move { resolver.resolve_preview_bounded(&url, 1024).await.ok() }
            },
            policy,
        )
        .await;
        let elapsed = started.elapsed();

        assert!(result.is_err());
        assert!(elapsed < Duration::from_millis(1_200), "took {elapsed:?}");
        assert!(dns.lookups.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn reviewed_locator_returns_the_canonical_encoding() {
        let hash = "ab".repeat(32);
        let dao = "cd".repeat(32);
        let typed = format!(
            "https://alexandria.ifftu.dev/governance/genesis/{dao}?content={hash}\
             &source=https%3A%2F%2FMirror.Example.ORG%3A443%2F%2567enesis.json%3F%23blake3%3D{hash}\
             &source=iroh%3A%2F%2F{hash}"
        );
        let reviewed = reviewed_locator(&typed).unwrap();
        assert_ne!(reviewed.canonical_uri, typed);
        assert!(reviewed
            .canonical_uri
            .starts_with("alexandria://governance/genesis/"));
        assert_eq!(
            reviewed.locator.locations[0],
            format!("https://mirror.example.org/genesis.json#blake3={hash}")
        );
        let reparsed = reviewed_locator(&reviewed.canonical_uri).unwrap();
        assert_eq!(reparsed.locator, reviewed.locator);
        assert_eq!(reparsed.canonical_uri, reviewed.canonical_uri);
    }

    #[test]
    fn preview_exposes_one_core_hash_and_the_exact_envelope_hash() {
        let bytes = signed_genesis("computer-science");
        let json = String::from_utf8(bytes.clone()).unwrap();
        let (envelope, verified) = checked_genesis(&json, None).unwrap();
        let preview = preview(&envelope, &verified, &bytes);
        assert_eq!(preview.core_hash, preview.dao_id);
        assert_eq!(preview.core_hash, envelope.core.core_hash().unwrap());
        assert_eq!(
            preview.envelope_hash,
            blake3::hash(&bytes).to_hex().to_string()
        );
        assert_ne!(preview.envelope_hash, preview.core_hash);
    }
}
