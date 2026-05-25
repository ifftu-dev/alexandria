//! Chain-side ingestion for the stake-pubkey registry.
//!
//! Reconciles the `stake_pubkey_registry` table against on-chain
//! registrations. The chain side is the canonical source of truth
//! per `docs/stake-pubkey-registry.md` §4.1 — entries here override
//! anything pulled from the bundled snapshot.
//!
//! Four pieces live in this module:
//!
//! - [`ChainEntry`] — the wire shape that comes back from Blockfrost
//!   after parsing the registration UTxO's inline datum.
//! - [`apply_chain_entries`] — pure function that applies a batch of
//!   chain entries to the local registry, calling
//!   [`crate::p2p::registry::upsert_chain_entry`] and
//!   [`crate::p2p::registry::evict_contradicted_snapshot`] as needed.
//! - [`BlockfrostFetcher`] — production implementation of
//!   [`ChainFetcher`]. Walks the `stake_pubkey_registration` script
//!   address, decodes each inline datum, witness-verifies the
//!   creating tx, and emits one [`ChainEntry`] per surviving UTxO.
//! - [`spawn_refresh_task`] — Tokio task that polls the fetcher at a
//!   configurable interval and feeds [`apply_chain_entries`]. Both
//!   the fetcher and the interval are resolved through factory
//!   closures, invoked **per tick**, so the task survives operators
//!   unlocking a profile or tuning settings after boot.
//!
//! Before-profile-unlock the task uses [`BOOTSTRAP_REFRESH_SECS`] as
//! the inter-tick sleep instead of the configured interval, so a
//! newly-set `cardano.blockfrost_project_id` is picked up within
//! tens of seconds rather than waiting out the default hour.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;

use super::registry::{evict_contradicted_snapshot, upsert_chain_entry};
use crate::db::Database;

/// Default cadence for the background refresh task. Used when no
/// per-profile `registry.refresh_secs` setting is present. Production
/// reads the setting fresh on every tick via [`spawn_refresh_task`]'s
/// `interval_factory` closure so a value change takes effect on the
/// next refresh without a restart.
pub const DEFAULT_REFRESH_SECS: u64 = 3600;

/// Lower bound on the per-tick sleep. Stops a misconfigured setting
/// (e.g. 0 or 1) from hot-looping Blockfrost.
pub const MIN_REFRESH_SECS: u64 = 60;

/// Inter-tick sleep used while the refresh task is still in its
/// "bootstrap" phase — either no Blockfrost project id is configured
/// or no profile has been unlocked yet. Shorter than `MIN_REFRESH_SECS`
/// because the loop produces zero Blockfrost traffic in this state
/// (the factory returns `None`), so a faster poll just means a
/// newly-unlocked profile picks up its setting in tens of seconds
/// rather than waiting out the full refresh interval.
pub const BOOTSTRAP_REFRESH_SECS: u64 = 30;

// Compile-time guard: the fast-bootstrap path only wins if its sleep
// is strictly less than the floor we'd otherwise hit. If someone
// raises `MIN_REFRESH_SECS` without raising `BOOTSTRAP_REFRESH_SECS`
// past it (or vice-versa), the bootstrap optimisation collapses.
const _: () = assert!(
    BOOTSTRAP_REFRESH_SECS < MIN_REFRESH_SECS,
    "BOOTSTRAP_REFRESH_SECS must be strictly less than MIN_REFRESH_SECS",
);

/// One on-chain registration as parsed from a UTxO at the
/// `stake_pubkey_registration` script address. Mirrors the fields of
/// the Aiken `StakePubkeyRegistrationDatum`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainEntry {
    pub stake_address: String,
    pub public_key_hex: String,
    pub valid_from: u64,
    /// `None` here matches `valid_until = 0` on-chain (open-ended).
    pub valid_until: Option<u64>,
    /// Hash of the transaction that created the registration UTxO.
    pub on_chain_tx: String,
}

/// Apply a batch of chain entries to the local registry. Returns the
/// number of rows upserted plus the number of contradicted snapshot
/// rows evicted.
pub fn apply_chain_entries(
    conn: &Connection,
    entries: &[ChainEntry],
    now: u64,
) -> Result<ApplyStats, rusqlite::Error> {
    let mut stats = ApplyStats::default();
    for entry in entries {
        upsert_chain_entry(
            conn,
            &entry.stake_address,
            &entry.public_key_hex,
            entry.valid_from,
            entry.valid_until,
            &entry.on_chain_tx,
            now,
        )?;
        stats.upserted += 1;
        let evicted = evict_contradicted_snapshot(
            conn,
            &entry.stake_address,
            &entry.public_key_hex,
            entry.valid_from,
            entry.valid_until,
        )?;
        stats.evicted += evicted;
    }
    Ok(stats)
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ApplyStats {
    pub upserted: usize,
    pub evicted: usize,
}

/// Abstraction over the chain fetch so production can call Blockfrost
/// while tests provide a canned response. The future is `Send` so the
/// refresh task can `.await` it across threads.
pub trait ChainFetcher: Send + Sync {
    fn fetch<'a>(
        &'a self,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<ChainEntry>, String>> + Send + 'a>,
    >;
}

/// Real Blockfrost-backed fetcher. Walks the
/// `stake_pubkey_registration` script address, decodes inline datums,
/// verifies the creating tx was signed by the claimed stake key, and
/// emits one [`ChainEntry`] per surviving registration UTxO.
///
/// Entries whose witness check fails are dropped with a `WARN` log —
/// they can't bind privileged authority because possession of the
/// stake key wasn't proven on chain.
pub struct BlockfrostFetcher {
    pub client: std::sync::Arc<crate::cardano::blockfrost::BlockfrostClient>,
    pub network: crate::cardano::stake_pubkey::Network,
}

impl BlockfrostFetcher {
    pub fn new(
        client: std::sync::Arc<crate::cardano::blockfrost::BlockfrostClient>,
        network: crate::cardano::stake_pubkey::Network,
    ) -> Self {
        Self { client, network }
    }

    async fn fetch_inner(&self) -> Result<Vec<ChainEntry>, String> {
        use crate::cardano::stake_pubkey;

        let script_addr = stake_pubkey::script_address(self.network)
            .map_err(|e| format!("script address: {e}"))?;

        let utxos = self
            .client
            .get_utxos(&script_addr)
            .await
            .map_err(|e| format!("get_utxos: {e}"))?;
        if utxos.is_empty() {
            return Ok(vec![]);
        }

        let mut out = Vec::with_capacity(utxos.len());
        for utxo in utxos {
            // Pull the full tx outputs so we can read the inline datum
            // at this output index.
            let tx_utxos = match self.client.get_tx_utxos(&utxo.tx_hash).await {
                Ok(t) => t,
                Err(e) => {
                    log::warn!(
                        "registry refresh: get_tx_utxos({}) failed: {e}",
                        utxo.tx_hash
                    );
                    continue;
                }
            };
            let target = tx_utxos
                .outputs
                .into_iter()
                .find(|o| o.output_index as u64 == utxo.tx_index);
            let Some(out_struct) = target else {
                log::warn!(
                    "registry refresh: no output {} in tx {} — skipping",
                    utxo.tx_index,
                    utxo.tx_hash
                );
                continue;
            };
            let Some(datum_hex) = out_struct.inline_datum else {
                log::debug!(
                    "registry refresh: UTxO {}:{} has no inline datum — skipping",
                    utxo.tx_hash,
                    utxo.tx_index
                );
                continue;
            };
            let datum_bytes = match hex::decode(&datum_hex) {
                Ok(b) => b,
                Err(e) => {
                    log::warn!("registry refresh: bad datum hex on {}: {e}", utxo.tx_hash);
                    continue;
                }
            };
            let datum = match stake_pubkey::decode_datum(&datum_bytes) {
                Ok(d) => d,
                Err(e) => {
                    log::warn!("registry refresh: bad datum CBOR on {}: {e}", utxo.tx_hash);
                    continue;
                }
            };
            let stake_address = match stake_pubkey::stake_address_from_key_hash(
                &datum.stake_key_hash,
                self.network,
            ) {
                Ok(s) => s,
                Err(e) => {
                    log::warn!(
                        "registry refresh: bad stake key hash on {}: {e}",
                        utxo.tx_hash
                    );
                    continue;
                }
            };
            // Witness check: fetch raw tx CBOR and confirm the
            // creating tx was signed by the claimed stake key.
            // Without this an attacker can post a UTxO at the script
            // address with any datum they like.
            let tx_cbor = match self.client.get_tx_cbor(&utxo.tx_hash).await {
                Ok(b) => b,
                Err(e) => {
                    log::warn!(
                        "registry refresh: get_tx_cbor({}) failed: {e} — skipping entry",
                        utxo.tx_hash
                    );
                    continue;
                }
            };
            match stake_pubkey::tx_witnesses_include_stake_key(&tx_cbor, &datum.stake_key_hash) {
                stake_pubkey::WitnessCheck::SignedByStakeKey => {}
                other => {
                    log::warn!(
                        "registry refresh: rejecting {} on tx {} — witness check: {:?}",
                        stake_address,
                        utxo.tx_hash,
                        other
                    );
                    continue;
                }
            }
            out.push(ChainEntry {
                stake_address,
                public_key_hex: hex::encode(datum.public_key),
                valid_from: datum.valid_from.max(0) as u64,
                valid_until: if datum.valid_until > 0 {
                    Some(datum.valid_until as u64)
                } else {
                    None
                },
                on_chain_tx: utxo.tx_hash,
            });
        }
        Ok(out)
    }
}

impl ChainFetcher for BlockfrostFetcher {
    fn fetch<'a>(
        &'a self,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<ChainEntry>, String>> + Send + 'a>,
    > {
        Box::pin(self.fetch_inner())
    }
}

/// Spawn the background refresh task.
///
/// `fetcher_factory` is invoked **once per tick** so the task picks
/// up configuration changes (Blockfrost project id arriving, network
/// switching, etc.) without an app restart. A `None` return value
/// means "not configured this tick" — the loop logs at debug and
/// continues. Once the operator sets the
/// `cardano.blockfrost_project_id` setting (or exports
/// `BLOCKFROST_PROJECT_ID`) the very next tick will build a fresh
/// fetcher and start pulling chain state.
///
/// `interval_factory` is likewise invoked per tick so the
/// `registry.refresh_secs` setting can be tuned at runtime. The
/// returned value is clamped up to [`MIN_REFRESH_SECS`].
///
/// Failures inside the loop are logged and swallowed — registry
/// state is unchanged on error, and the next tick retries.
///
/// Returns a `JoinHandle` so callers can abort during shutdown.
pub fn spawn_refresh_task<F, I>(
    db: Arc<Mutex<Option<Database>>>,
    fetcher_factory: F,
    interval_factory: I,
) -> tokio::task::JoinHandle<()>
where
    F: Fn() -> Option<Arc<dyn ChainFetcher>> + Send + Sync + 'static,
    I: Fn() -> u64 + Send + Sync + 'static,
{
    tokio::spawn(async move {
        // First iteration runs immediately so a freshly-started node
        // catches up before privileged-topic traffic flows. Sleep is
        // at the end of the loop so the interval applies *between*
        // ticks, not before the first one. The labelled-block
        // pattern keeps every skip path inside the same sleep, so a
        // misconfigured factory can't hot-spin.
        //
        // `bootstrap_mode` is set inside the labelled block whenever
        // the tick produced zero Blockfrost traffic (no fetcher, or
        // no profile DB yet). In that case the inter-tick sleep
        // shortens to `BOOTSTRAP_REFRESH_SECS` so a late profile
        // unlock or a newly-set Blockfrost project id is picked up
        // within tens of seconds — without this, the default 1-hour
        // interval gates the first useful tick after unlock.
        loop {
            let mut bootstrap_mode = false;
            'tick: {
                let Some(fetcher) = fetcher_factory() else {
                    log::debug!(
                        "registry refresh: skipping tick — no Blockfrost project id configured \
                         (set in Settings → Cardano or via BLOCKFROST_PROJECT_ID); \
                         polling again in {BOOTSTRAP_REFRESH_SECS}s"
                    );
                    bootstrap_mode = true;
                    break 'tick;
                };
                let entries = match fetcher.fetch().await {
                    Ok(e) => e,
                    Err(e) => {
                        log::warn!("registry refresh: fetch failed: {e}");
                        break 'tick;
                    }
                };
                if entries.is_empty() {
                    break 'tick;
                }
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let apply_result = {
                    let guard = match db.lock() {
                        Ok(g) => g,
                        Err(e) => {
                            log::warn!("registry refresh: db lock poisoned: {e}");
                            break 'tick;
                        }
                    };
                    let Some(db_ref) = guard.as_ref() else {
                        // No active profile yet — drop this tick
                        // AND mark the tick as bootstrap so the next
                        // wait is short. We did spend a Blockfrost
                        // call here (above), so a fast retry costs
                        // one call per BOOTSTRAP_REFRESH_SECS until
                        // unlock; acceptable trade for prompt
                        // pickup. The refresh task outlives any
                        // single profile.
                        bootstrap_mode = true;
                        break 'tick;
                    };
                    apply_chain_entries(db_ref.conn(), &entries, now)
                };
                match apply_result {
                    Ok(stats) => log::info!(
                        "registry refresh: upserted {} chain rows, evicted {} snapshot rows",
                        stats.upserted,
                        stats.evicted
                    ),
                    Err(e) => log::warn!("registry refresh: apply failed: {e}"),
                }
            }
            let secs = if bootstrap_mode {
                BOOTSTRAP_REFRESH_SECS
            } else {
                interval_factory().max(MIN_REFRESH_SECS)
            };
            tokio::time::sleep(Duration::from_secs(secs)).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p::registry::{lookup, upsert_snapshot_entry, SnapshotEntry};

    fn test_db() -> Database {
        let db = Database::open_in_memory().expect("in-memory db");
        db.run_migrations().expect("migrations");
        db
    }

    fn entry(stake: &str, pubkey: &str, from: u64, until: Option<u64>, tx: &str) -> ChainEntry {
        ChainEntry {
            stake_address: stake.into(),
            public_key_hex: pubkey.into(),
            valid_from: from,
            valid_until: until,
            on_chain_tx: tx.into(),
        }
    }

    #[test]
    fn apply_inserts_fresh_chain_entries() {
        let db = test_db();
        let entries = vec![
            entry("stake1u_a", "aa", 0, None, "tx1"),
            entry("stake1u_b", "bb", 0, Some(100), "tx2"),
        ];
        let stats = apply_chain_entries(db.conn(), &entries, 50).unwrap();
        assert_eq!(stats.upserted, 2);
        assert_eq!(stats.evicted, 0);
        assert!(lookup(db.conn(), "stake1u_a", "aa", 10).unwrap());
        assert!(lookup(db.conn(), "stake1u_b", "bb", 50).unwrap());
    }

    #[test]
    fn apply_upgrades_matching_snapshot_row() {
        let db = test_db();
        // Pre-seed with a snapshot row.
        upsert_snapshot_entry(
            db.conn(),
            &SnapshotEntry {
                stake_address: "stake1u_a".into(),
                public_key_hex: "aa".into(),
                valid_from: 0,
                valid_until: Some(100),
                on_chain_tx: None,
            },
            None,
        )
        .unwrap();

        let entries = vec![entry("stake1u_a", "aa", 0, Some(100), "tx-chain")];
        apply_chain_entries(db.conn(), &entries, 999).unwrap();

        // Row exists with source upgraded to 'chain'.
        let (source, tx): (String, Option<String>) = db
            .conn()
            .query_row(
                "SELECT source, on_chain_tx FROM stake_pubkey_registry \
                 WHERE stake_address = 'stake1u_a' AND public_key_hex = 'aa'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(source, "chain");
        assert_eq!(tx.as_deref(), Some("tx-chain"));
    }

    #[test]
    fn apply_evicts_contradicting_snapshot() {
        let db = test_db();
        // Snapshot says alice's key is `aaaa`; chain says it's `bbbb`.
        upsert_snapshot_entry(
            db.conn(),
            &SnapshotEntry {
                stake_address: "stake1u_alice".into(),
                public_key_hex: "aaaa".into(),
                valid_from: 0,
                valid_until: Some(100),
                on_chain_tx: None,
            },
            None,
        )
        .unwrap();
        let entries = vec![entry("stake1u_alice", "bbbb", 0, Some(100), "tx-real")];
        let stats = apply_chain_entries(db.conn(), &entries, 999).unwrap();
        assert_eq!(stats.upserted, 1);
        assert_eq!(stats.evicted, 1);
        // Old snapshot binding gone; chain binding wins.
        assert!(!lookup(db.conn(), "stake1u_alice", "aaaa", 50).unwrap());
        assert!(lookup(db.conn(), "stake1u_alice", "bbbb", 50).unwrap());
    }

    /// Tiny canned fetcher used by the refresh-task tests below.
    struct StaticFetcher(Vec<ChainEntry>);
    impl ChainFetcher for StaticFetcher {
        fn fetch<'a>(
            &'a self,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<ChainEntry>, String>> + Send + 'a>,
        > {
            let entries = self.0.clone();
            Box::pin(async move { Ok(entries) })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn refresh_task_uses_bootstrap_cadence_when_unconfigured() {
        // Regression for the P2 round-2 finding: without
        // `BOOTSTRAP_REFRESH_SECS`, a profile unlocked after the
        // first tick had to wait the full `DEFAULT_REFRESH_SECS`
        // (~1h) before the next tick picked up its Blockfrost
        // project id. We assert here that the inter-tick wait when
        // the factory returns `None` is `BOOTSTRAP_REFRESH_SECS`
        // (30s) by advancing virtual time *just past* that mark and
        // observing that the apply lands before the normal-mode
        // `MIN_REFRESH_SECS` (60s) would have allowed.
        use std::sync::atomic::{AtomicUsize, Ordering};

        let db = test_db();
        let handle = Arc::new(Mutex::new(Some(db)));
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_factory = calls.clone();
        let canned: Arc<dyn ChainFetcher> = Arc::new(StaticFetcher(vec![entry(
            "stake1u_boot",
            "ee",
            0,
            None,
            "tx-boot",
        )]));
        let factory = move || -> Option<Arc<dyn ChainFetcher>> {
            let n = calls_for_factory.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                None
            } else {
                Some(canned.clone())
            }
        };
        // Normal-mode interval would be 3600s (default); if the
        // bootstrap path is broken we'd wait that long. The test
        // would either hang or fail.
        let h = spawn_refresh_task(handle.clone(), factory, || {
            crate::p2p::registry_chain::DEFAULT_REFRESH_SECS
        });
        // First tick (factory → None) runs immediately.
        tokio::task::yield_now().await;
        // Advance past BOOTSTRAP_REFRESH_SECS but well under
        // MIN_REFRESH_SECS — proves the path uses the bootstrap
        // const, not the default-interval clamp.
        tokio::time::advance(Duration::from_secs(BOOTSTRAP_REFRESH_SECS + 5)).await;
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
        let found = {
            let guard = handle.lock().unwrap();
            guard
                .as_ref()
                .and_then(|db| lookup(db.conn(), "stake1u_boot", "ee", 0).ok())
                .unwrap_or(false)
        };
        h.abort();
        assert!(
            found,
            "bootstrap-mode tick must fire within BOOTSTRAP_REFRESH_SECS, \
             not the configured (default 1h) refresh interval"
        );
        // The const guard at module top enforces this at compile time;
        // the runtime check here documents the relationship for
        // readers of the test alone.
    }

    #[tokio::test(start_paused = true)]
    async fn refresh_task_picks_up_fetcher_after_late_arrival() {
        // Regression for the P2 bug where the refresh launcher
        // resolved Blockfrost once at boot and returned forever if
        // the project id wasn't set yet. The fetcher_factory is
        // queried on every tick — a `None` early and a `Some` later
        // means the entry should still land without restart.
        //
        // Tokio time is paused (`start_paused = true`) so we can
        // time-travel past the MIN_REFRESH_SECS sleep deterministically
        // without making the suite slow.
        use std::sync::atomic::{AtomicUsize, Ordering};

        let db = test_db();
        let handle = Arc::new(Mutex::new(Some(db)));
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_factory = calls.clone();
        let canned: Arc<dyn ChainFetcher> = Arc::new(StaticFetcher(vec![entry(
            "stake1u_late",
            "fe",
            0,
            None,
            "tx-late",
        )]));
        // First call returns None (operator hasn't configured
        // Blockfrost yet); subsequent calls return the real fetcher.
        let factory = move || -> Option<Arc<dyn ChainFetcher>> {
            let n = calls_for_factory.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                None
            } else {
                Some(canned.clone())
            }
        };
        let h = spawn_refresh_task(handle.clone(), factory, || 60);

        // Yield so the spawned task runs its first tick (factory→None).
        tokio::task::yield_now().await;
        // Advance past the inter-tick sleep so the second tick fires.
        tokio::time::advance(std::time::Duration::from_secs(61)).await;
        // Yield repeatedly to give the task a chance to fetch, apply,
        // and release the DB mutex.
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }

        let found = {
            let guard = handle.lock().unwrap();
            guard
                .as_ref()
                .and_then(|db| lookup(db.conn(), "stake1u_late", "fe", 0).ok())
                .unwrap_or(false)
        };
        h.abort();
        assert!(
            found,
            "refresh task must apply entries after a late config arrival"
        );
        assert!(
            calls.load(Ordering::SeqCst) >= 2,
            "fetcher_factory should have been invoked at least twice (none-then-some), got {}",
            calls.load(Ordering::SeqCst)
        );
    }

    #[tokio::test]
    async fn refresh_task_applies_fetcher_output() {
        let db = test_db();
        let handle = Arc::new(Mutex::new(Some(db)));
        let fetcher: Arc<dyn ChainFetcher> = Arc::new(StaticFetcher(vec![entry(
            "stake1u_x",
            "ff",
            0,
            None,
            "tx-x",
        )]));
        // Use a 60s interval — clamped to MIN_REFRESH_SECS; the
        // tokio runtime advances immediately on the sleep so the
        // first apply lands inside the spin budget below.
        let factory_fetcher = fetcher.clone();
        let h = spawn_refresh_task(handle.clone(), move || Some(factory_fetcher.clone()), || 60);
        // Spin until the row lands or we time out (~1s budget). The
        // guard is scoped inside braces so clippy doesn't think it
        // crosses the `await` below — we already explicitly drop it
        // before sleeping, but the scope is clearer + lint-clean.
        let mut found = false;
        for _ in 0..50 {
            {
                let guard = handle.lock().unwrap();
                if let Some(db) = guard.as_ref() {
                    if lookup(db.conn(), "stake1u_x", "ff", 0).unwrap_or(false) {
                        found = true;
                        break;
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        h.abort();
        assert!(found, "refresh task should have applied the entry");
    }
}
