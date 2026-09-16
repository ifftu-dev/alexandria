//! Reproducible database-contention baseline for remediation work package W7.
//!
//! This is a measurement harness, not a benchmark assertion. It exercises the
//! current single encrypted SQLite connection behind one mutex while a bounded
//! ingest stream competes with representative catalog, credential, and
//! governance reads. Run with `--profile learner`, `--profile instructor`, or
//! the small `--profile smoke` fixture used for harness verification. Sample
//! and concurrency overrides make long-running profiles practical during local
//! investigation without changing their fixture sizes.

use std::{
    env, fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use app_lib::db::Database;
use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use serde::Serialize;
use tokio::time::{Instant as TokioInstant, MissedTickBehavior};

const FIXTURE_KEY: [u8; 32] = [0x5a; 32];

#[derive(Clone, Copy)]
struct Workload {
    name: &'static str,
    catalog_rows: usize,
    credential_rows: usize,
    vote_rows: usize,
    samples: usize,
    concurrency: usize,
    ingest_per_second: u64,
}

struct Arguments {
    profile: String,
    samples: Option<usize>,
    concurrency: Option<usize>,
    mode: RunMode,
}

#[derive(Clone, Copy)]
enum RunMode {
    Direct,
    BoundedBlocking,
}

impl RunMode {
    fn named(name: &str) -> Result<Self> {
        match name {
            "direct" => Ok(Self::Direct),
            "bounded-blocking" => Ok(Self::BoundedBlocking),
            other => Err(anyhow!(
                "unknown mode {other:?}; expected direct or bounded-blocking"
            )),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::BoundedBlocking => "bounded-blocking",
        }
    }
}

#[derive(Clone, Copy)]
enum QueryKind {
    CatalogSearch,
    CredentialLookup,
    GovernanceOutcomeEvidence,
}

impl QueryKind {
    const ALL: [Self; 3] = [
        Self::CatalogSearch,
        Self::CredentialLookup,
        Self::GovernanceOutcomeEvidence,
    ];

    fn for_sample(sample: usize) -> Self {
        Self::ALL[sample % Self::ALL.len()]
    }
}

impl Workload {
    fn named(name: &str) -> Result<Self> {
        match name {
            "smoke" => Ok(Self {
                name: "smoke",
                catalog_rows: 1_000,
                credential_rows: 2_500,
                vote_rows: 5_000,
                samples: 60,
                concurrency: 2,
                ingest_per_second: 100,
            }),
            "learner" => Ok(Self {
                name: "learner",
                catalog_rows: 10_000,
                credential_rows: 25_000,
                vote_rows: 25_000,
                samples: 600,
                concurrency: 4,
                ingest_per_second: 10,
            }),
            "instructor" => Ok(Self {
                name: "instructor",
                catalog_rows: 100_000,
                credential_rows: 250_000,
                vote_rows: 500_000,
                samples: 1_200,
                concurrency: 8,
                ingest_per_second: 100,
            }),
            other => Err(anyhow!(
                "unknown profile {other:?}; expected smoke, learner, or instructor"
            )),
        }
    }
}

#[derive(Debug, Serialize)]
struct Distribution {
    samples: usize,
    min_us: u64,
    p50_us: u64,
    p95_us: u64,
    p99_us: u64,
    max_us: u64,
}

#[derive(Debug, Serialize)]
struct QueryDistribution {
    lock_wait: Distribution,
    execution: Distribution,
}

#[derive(Debug, Serialize)]
struct QueryDistributions {
    catalog_search: QueryDistribution,
    credential_lookup: QueryDistribution,
    governance_outcome_evidence: QueryDistribution,
}

#[derive(Debug, Serialize)]
struct QueryPlans {
    catalog_search: Vec<String>,
    credential_lookup: Vec<String>,
    governance_outcome_evidence: Vec<String>,
}

struct Measurement {
    kind: QueryKind,
    lock_wait_us: u64,
    execution_us: u64,
}

#[derive(Debug, Serialize)]
struct Report {
    schema_version: u8,
    profile: &'static str,
    mode: &'static str,
    encrypted: bool,
    catalog_rows: usize,
    credential_rows: usize,
    vote_rows: usize,
    concurrency: usize,
    requested_ingest_per_second: u64,
    completed_ingest_events: usize,
    fixture_build_ms: u128,
    measured_wall_ms: u128,
    database_bytes: u64,
    wal_bytes: u64,
    lock_wait: Distribution,
    query_execution: Distribution,
    queries: QueryDistributions,
    query_plans: QueryPlans,
    event_loop_lateness: Distribution,
    limitations: Vec<&'static str>,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<()> {
    let arguments = parse_arguments()?;
    let mut workload = Workload::named(&arguments.profile)?;
    if let Some(samples) = arguments.samples {
        workload.samples = samples;
    }
    if let Some(concurrency) = arguments.concurrency {
        workload.concurrency = concurrency;
    }
    let fixture_dir = tempfile::tempdir().context("create temporary benchmark directory")?;
    let db_path = fixture_dir.path().join("alexandria-performance.db");
    let fixture_started = Instant::now();
    let database = Database::open_encrypted(&db_path, &FIXTURE_KEY)
        .context("open temporary encrypted database")?;
    database.run_migrations().context("apply schema")?;
    populate_fixture(database.conn(), workload).context("populate benchmark fixture")?;
    let fixture_build_ms = fixture_started.elapsed().as_millis();
    let query_plans = query_plans(database.conn()).context("inspect benchmark query plans")?;

    let database = Arc::new(Mutex::new(database));
    warm_queries(&database)?;
    let running = Arc::new(AtomicBool::new(true));
    let ingest_count = Arc::new(AtomicUsize::new(0));

    let ingest = tokio::spawn(run_ingest(
        Arc::clone(&database),
        Arc::clone(&running),
        Arc::clone(&ingest_count),
        workload.ingest_per_second,
        arguments.mode,
    ));
    let timer = tokio::spawn(measure_event_loop(Arc::clone(&running)));

    let measured_started = Instant::now();
    let next_sample = Arc::new(AtomicUsize::new(0));
    let mut workers = Vec::with_capacity(workload.concurrency);
    for _ in 0..workload.concurrency {
        let database = Arc::clone(&database);
        let next_sample = Arc::clone(&next_sample);
        let mode = arguments.mode;
        workers.push(tokio::spawn(async move {
            let mut measurements = Vec::new();
            loop {
                let sample = next_sample.fetch_add(1, Ordering::Relaxed);
                if sample >= workload.samples {
                    break;
                }
                let kind = QueryKind::for_sample(sample);
                let (lock_wait_us, execution_us, rows) = match mode {
                    RunMode::Direct => measure_query(&database, kind)?,
                    RunMode::BoundedBlocking => {
                        let database = Arc::clone(&database);
                        tokio::task::spawn_blocking(move || measure_query(&database, kind))
                            .await
                            .context("blocking query task panicked")??
                    }
                };
                std::hint::black_box(rows);
                measurements.push(Measurement {
                    kind,
                    lock_wait_us,
                    execution_us,
                });
                tokio::task::yield_now().await;
            }
            Ok::<_, anyhow::Error>(measurements)
        }));
    }

    let mut lock_wait = Vec::with_capacity(workload.samples);
    let mut query_execution = Vec::with_capacity(workload.samples);
    let mut per_query = PerQueryMeasurements::default();
    for worker in workers {
        for measurement in worker.await.context("query worker panicked")?? {
            lock_wait.push(measurement.lock_wait_us);
            query_execution.push(measurement.execution_us);
            per_query.push(measurement);
        }
    }
    let measured_wall_ms = measured_started.elapsed().as_millis();
    running.store(false, Ordering::Release);
    ingest.await.context("ingest task panicked")??;
    let event_loop_lateness = timer.await.context("timer task panicked")?;
    drop(database);

    let report = Report {
        schema_version: 1,
        profile: workload.name,
        mode: arguments.mode.name(),
        encrypted: true,
        catalog_rows: workload.catalog_rows,
        credential_rows: workload.credential_rows,
        vote_rows: workload.vote_rows,
        concurrency: workload.concurrency,
        requested_ingest_per_second: workload.ingest_per_second,
        completed_ingest_events: ingest_count.load(Ordering::Acquire),
        fixture_build_ms,
        measured_wall_ms,
        database_bytes: file_size(&db_path),
        wal_bytes: file_size(&wal_path(&db_path)),
        lock_wait: distribution(lock_wait),
        query_execution: distribution(query_execution),
        queries: per_query.into_distributions(),
        query_plans,
        event_loop_lateness: distribution(event_loop_lateness),
        limitations: vec![
            "host-only synthetic fixture; not a physical mobile-device result",
            "measures the Rust/SQLite boundary, not webview rendering or IPC serialization",
            "credential payloads are structurally representative but are not cryptographically verified in timed reads",
            "background ingest models database writes, not network transfer or signature verification",
        ],
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    drop(fixture_dir);
    Ok(())
}

fn parse_arguments() -> Result<Arguments> {
    let mut args = env::args().skip(1);
    let mut profile = "smoke".to_owned();
    let mut samples = None;
    let mut concurrency = None;
    let mut mode = RunMode::Direct;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--profile" => {
                profile = args
                    .next()
                    .ok_or_else(|| anyhow!("--profile requires a value"))?;
            }
            "--samples" => samples = Some(positive_usize(&mut args, "--samples")?),
            "--concurrency" => {
                concurrency = Some(positive_usize(&mut args, "--concurrency")?);
            }
            "--mode" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow!("--mode requires a value"))?;
                mode = RunMode::named(&value)?;
            }
            "--help" | "-h" => {
                println!(
                    "Usage: cargo run --release -p alexandria-node --example db_contention_baseline -- --profile smoke|learner|instructor [--mode direct|bounded-blocking] [--samples N] [--concurrency N]"
                );
                std::process::exit(0);
            }
            other => return Err(anyhow!("unknown argument {other:?}")),
        }
    }
    Ok(Arguments {
        profile,
        samples,
        concurrency,
        mode,
    })
}

fn positive_usize(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<usize> {
    let raw = args
        .next()
        .ok_or_else(|| anyhow!("{flag} requires a value"))?;
    let value = raw
        .parse::<usize>()
        .with_context(|| format!("{flag} must be a positive integer"))?;
    if value == 0 {
        return Err(anyhow!("{flag} must be greater than zero"));
    }
    Ok(value)
}

fn populate_fixture(conn: &Connection, workload: Workload) -> Result<()> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    {
        let mut catalog = tx.prepare_cached(
            "INSERT INTO catalog (course_id, title, description, author_address, content_cid, \
             tags, skill_ids, version, published_at, received_at, signature, kind) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?8, ?9, 'course')",
        )?;
        for index in 0..workload.catalog_rows {
            let topic = index % 100;
            catalog.execute(params![
                format!("catalog-{index:09}"),
                format!("Distributed systems course {topic:03}"),
                format!("Offline-first learning material for topic {topic:03}"),
                format!("stake_test_author_{:04}", index % 1_000),
                format!("bafy-content-{index:09}"),
                format!(r#"["distributed","topic-{topic:03}"]"#),
                format!(r#"["skill-{:04}"]"#, index % 2_000),
                format!("2026-01-{:02}T{:02}:00:00Z", index % 28 + 1, index % 24),
                format!("signature-{index:09}"),
            ])?;
        }
    }
    {
        let mut credential = tx.prepare_cached(
            "INSERT INTO credentials (id, issuer_did, subject_did, credential_type, claim_kind, \
             skill_id, issuance_date, signed_vc_json, integrity_hash, revoked, received_at) \
             VALUES (?1, ?2, ?3, 'AssessmentCredential', 'skill', ?4, ?5, ?6, ?7, 0, ?5)",
        )?;
        for index in 0..workload.credential_rows {
            let subject = format!("did:key:learner-{:05}", index % 5_000);
            let issuer = format!("did:key:issuer-{:04}", index % 500);
            let skill = format!("skill-{:04}", index % 2_000);
            let timestamp = format!("2026-02-{:02}T{:02}:00:00Z", index % 28 + 1, index % 24);
            let vc = serde_json::json!({
                "@context": ["https://www.w3.org/ns/credentials/v2"],
                "id": format!("urn:uuid:credential-{index:09}"),
                "type": ["VerifiableCredential", "AssessmentCredential"],
                "issuer": issuer,
                "validFrom": timestamp,
                "credentialSubject": {
                    "id": subject,
                    "skill": { "skillId": skill, "level": 0.75, "score": 0.8 }
                },
                "proof": {
                    "type": "Ed25519Signature2020",
                    "created": timestamp,
                    "verificationMethod": "did:key:fixture#key-1",
                    "proofPurpose": "assertionMethod",
                    "jws": "fixture-signature"
                }
            })
            .to_string();
            credential.execute(params![
                format!("urn:uuid:credential-{index:09}"),
                format!("did:key:issuer-{:04}", index % 500),
                format!("did:key:learner-{:05}", index % 5_000),
                format!("skill-{:04}", index % 2_000),
                format!("2026-02-{:02}T{:02}:00:00Z", index % 28 + 1, index % 24),
                vc,
                format!("{index:064x}"),
            ])?;
        }
    }

    tx.execute(
        "INSERT INTO governance_daos (id, name, scope_type, scope_id, status) \
         VALUES ('fixture-dao', 'Fixture DAO', 'subject', 'fixture-subject', 'active')",
        [],
    )?;
    let proposal_count = workload.vote_rows.clamp(1, 100);
    {
        let mut proposal = tx.prepare_cached(
            "INSERT INTO governance_proposals \
             (id, dao_id, title, category, proposer, status, voting_deadline) \
             VALUES (?1, 'fixture-dao', ?2, 'policy', 'stake_test_proposer', 'published', \
             '2026-12-31T00:00:00Z')",
        )?;
        for index in 0..proposal_count {
            proposal.execute(params![
                format!("proposal-{index:03}"),
                format!("Proposal {index:03}"),
            ])?;
        }
    }
    {
        let mut vote = tx.prepare_cached(
            "INSERT INTO governance_proposal_votes \
             (id, proposal_id, voter, in_favor, voted_at, signature, public_key) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for index in 0..workload.vote_rows {
            let proposal = index % proposal_count;
            vote.execute(params![
                format!("vote-{index:09}"),
                format!("proposal-{proposal:03}"),
                format!("stake_test_voter-{index:09}"),
                i64::from(index % 3 != 0),
                format!("2026-03-{:02}T{:02}:00:00Z", index % 28 + 1, index % 24),
                format!("signature-{index:09}"),
                format!("public-key-{:04}", index % 5_000),
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

fn warm_queries(database: &Arc<Mutex<Database>>) -> Result<()> {
    let db = database
        .lock()
        .map_err(|_| anyhow!("database mutex poisoned"))?;
    for query in QueryKind::ALL {
        std::hint::black_box(run_query(db.conn(), query)?);
    }
    Ok(())
}

fn run_query(conn: &Connection, query: QueryKind) -> Result<usize> {
    match query {
        QueryKind::CatalogSearch => {
            let mut statement = conn.prepare(
                "SELECT course_id, title, description, tags FROM catalog \
                 WHERE title LIKE ?1 OR description LIKE ?1 OR tags LIKE ?1 \
                 ORDER BY published_at DESC LIMIT 50",
            )?;
            let rows = statement.query_map(["%topic-042%"], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?.len())
        }
        QueryKind::CredentialLookup => {
            let mut statement = conn.prepare(
                "SELECT signed_vc_json FROM credentials \
                 WHERE subject_did = ?1 AND skill_id = ?2 AND revoked = 0 \
                 ORDER BY received_at DESC LIMIT 50",
            )?;
            let rows = statement
                .query_map(params!["did:key:learner-00042", "skill-0042"], |row| {
                    row.get::<_, String>(0)
                })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?.len())
        }
        QueryKind::GovernanceOutcomeEvidence => {
            let mut statement = conn.prepare(
                "SELECT voter, in_favor, signature FROM governance_proposal_votes \
                 WHERE proposal_id = ?1 AND signature IS NOT NULL ORDER BY id",
            )?;
            let rows = statement.query_map(["proposal-042"], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?.len())
        }
    }
}

fn measure_query(database: &Arc<Mutex<Database>>, kind: QueryKind) -> Result<(u64, u64, usize)> {
    let lock_started = Instant::now();
    let db = database
        .lock()
        .map_err(|_| anyhow!("database mutex poisoned"))?;
    let lock_wait_us = micros(lock_started.elapsed());
    let query_started = Instant::now();
    let rows = run_query(db.conn(), kind)?;
    Ok((lock_wait_us, micros(query_started.elapsed()), rows))
}

fn query_plans(conn: &Connection) -> Result<QueryPlans> {
    Ok(QueryPlans {
        catalog_search: explain_query_plan(
            conn,
            "EXPLAIN QUERY PLAN SELECT course_id, title, description, tags FROM catalog \
             WHERE title LIKE ?1 OR description LIKE ?1 OR tags LIKE ?1 \
             ORDER BY published_at DESC LIMIT 50",
            &["%topic-042%"],
        )?,
        credential_lookup: explain_query_plan(
            conn,
            "EXPLAIN QUERY PLAN SELECT signed_vc_json FROM credentials \
             WHERE subject_did = ?1 AND skill_id = ?2 AND revoked = 0 \
             ORDER BY received_at DESC LIMIT 50",
            &["did:key:learner-00042", "skill-0042"],
        )?,
        governance_outcome_evidence: explain_query_plan(
            conn,
            "EXPLAIN QUERY PLAN SELECT voter, in_favor, signature \
             FROM governance_proposal_votes WHERE proposal_id = ?1 \
             AND signature IS NOT NULL ORDER BY id",
            &["proposal-042"],
        )?,
    })
}

fn explain_query_plan(conn: &Connection, sql: &str, parameters: &[&str]) -> Result<Vec<String>> {
    let mut statement = conn.prepare(sql)?;
    let rows = statement.query_map(rusqlite::params_from_iter(parameters), |row| {
        row.get::<_, String>(3)
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

async fn run_ingest(
    database: Arc<Mutex<Database>>,
    running: Arc<AtomicBool>,
    completed: Arc<AtomicUsize>,
    events_per_second: u64,
    mode: RunMode,
) -> Result<()> {
    let interval_duration = Duration::from_micros(1_000_000 / events_per_second.max(1));
    let mut interval = tokio::time::interval(interval_duration);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    while running.load(Ordering::Acquire) {
        interval.tick().await;
        if !running.load(Ordering::Acquire) {
            break;
        }
        let sequence = completed.load(Ordering::Acquire);
        match mode {
            RunMode::Direct => ingest_one(&database, sequence)?,
            RunMode::BoundedBlocking => {
                let database = Arc::clone(&database);
                tokio::task::spawn_blocking(move || ingest_one(&database, sequence))
                    .await
                    .context("blocking ingest task panicked")??;
            }
        }
        completed.fetch_add(1, Ordering::AcqRel);
    }
    Ok(())
}

fn ingest_one(database: &Arc<Mutex<Database>>, sequence: usize) -> Result<()> {
    let db = database
        .lock()
        .map_err(|_| anyhow!("database mutex poisoned"))?;
    db.conn().execute(
        "INSERT INTO sync_log (entity_type, entity_id, direction, peer_id, signature) \
         VALUES ('catalog', ?1, 'received', ?2, ?3)",
        params![
            format!("ingest-{sequence:09}"),
            format!("peer-{:02}", sequence % 64),
            format!("signature-{sequence:09}"),
        ],
    )?;
    Ok(())
}

async fn measure_event_loop(running: Arc<AtomicBool>) -> Vec<u64> {
    const TICK: Duration = Duration::from_millis(10);
    let mut expected = TokioInstant::now() + TICK;
    let mut lateness = Vec::new();
    while running.load(Ordering::Acquire) {
        tokio::time::sleep_until(expected).await;
        lateness.push(micros(
            TokioInstant::now().saturating_duration_since(expected),
        ));
        expected += TICK;
    }
    lateness
}

fn distribution(mut samples: Vec<u64>) -> Distribution {
    samples.sort_unstable();
    Distribution {
        samples: samples.len(),
        min_us: samples.first().copied().unwrap_or(0),
        p50_us: percentile(&samples, 50),
        p95_us: percentile(&samples, 95),
        p99_us: percentile(&samples, 99),
        max_us: samples.last().copied().unwrap_or(0),
    }
}

#[derive(Default)]
struct PerQueryMeasurements {
    catalog_search: Vec<Measurement>,
    credential_lookup: Vec<Measurement>,
    governance_outcome_evidence: Vec<Measurement>,
}

impl PerQueryMeasurements {
    fn push(&mut self, measurement: Measurement) {
        match measurement.kind {
            QueryKind::CatalogSearch => self.catalog_search.push(measurement),
            QueryKind::CredentialLookup => self.credential_lookup.push(measurement),
            QueryKind::GovernanceOutcomeEvidence => {
                self.governance_outcome_evidence.push(measurement);
            }
        }
    }

    fn into_distributions(self) -> QueryDistributions {
        QueryDistributions {
            catalog_search: query_distribution(self.catalog_search),
            credential_lookup: query_distribution(self.credential_lookup),
            governance_outcome_evidence: query_distribution(self.governance_outcome_evidence),
        }
    }
}

fn query_distribution(measurements: Vec<Measurement>) -> QueryDistribution {
    let mut waits = Vec::with_capacity(measurements.len());
    let mut executions = Vec::with_capacity(measurements.len());
    for measurement in measurements {
        waits.push(measurement.lock_wait_us);
        executions.push(measurement.execution_us);
    }
    QueryDistribution {
        lock_wait: distribution(waits),
        execution: distribution(executions),
    }
}

fn percentile(samples: &[u64], percentile: usize) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let index = (samples.len() * percentile).div_ceil(100).saturating_sub(1);
    samples[index.min(samples.len() - 1)]
}

fn micros(duration: Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}

fn file_size(path: &Path) -> u64 {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

fn wal_path(database: &Path) -> std::path::PathBuf {
    let mut name = database.as_os_str().to_owned();
    name.push("-wal");
    name.into()
}

#[cfg(test)]
mod tests {
    use super::{distribution, percentile};

    #[test]
    fn percentile_uses_nearest_rank() {
        let samples = [1, 2, 3, 4, 5];
        assert_eq!(percentile(&samples, 50), 3);
        assert_eq!(percentile(&samples, 95), 5);
    }

    #[test]
    fn distribution_handles_no_samples() {
        let result = distribution(Vec::new());
        assert_eq!(result.samples, 0);
        assert_eq!(result.min_us, 0);
        assert_eq!(result.p99_us, 0);
        assert_eq!(result.max_us, 0);
    }
}
