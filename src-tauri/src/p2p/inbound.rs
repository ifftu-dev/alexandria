//! Profile-fenced database access for inbound P2P work.
//!
//! Messages received from peers used to lock the shared profile database on
//! the swarm event loop or on the gossip consumer task. SQL there stalled
//! Tokio workers, bypassed the executor's lane fairness, and could commit a
//! message received for one profile into the next profile after a switch.
//!
//! Every inbound database operation now runs as a Background-lane executor
//! job under a lease for the profile session the work belongs to. A lease is
//! admitted per job rather than held for the node's lifetime, so locking still
//! drains promptly, and a job for a stale session is refused before it touches
//! the database.
//!
//! Overload policy. The Background lane reserves eight waiting slots and
//! rejects overflow immediately as retryable busy work:
//!
//! - Gossip is best-effort. Accepted messages enter one
//!   bounded queue and are applied one at a time, preserving arrival order. A
//!   full queue or a busy lane drops the message with a rate-limited warning;
//!   the swarm loop never waits for capacity and nothing is spawned per message.
//! - Request/response handlers run in a bounded job set owned by the swarm
//!   loop. When that set or the lane is full, protocols with an error variant
//!   (device-sync, guardian) answer with the retryable busy error. Protocols
//!   without one (vc-fetch, graph-fetch, profile-fetch) omit the response, so
//!   the requester observes a transient failure instead of an authoritative
//!   `NotFound`/`NotOwner`.
//! - Best-effort persistence (known peer addresses, the DHT record mirror) is
//!   dropped when its bounded job set or the lane is full.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::classroom::manager as classroom_manager;
use crate::classroom::types::{
    is_classroom_topic, ClassroomMessageEvent, ClassroomMessagePayload, ClassroomMetaEvent,
    ClassroomMetaTauriEvent,
};
use crate::db::executor::{DatabaseExecutor, DatabaseWorkload};
use crate::db::Database;
use crate::profile::operations::ProfileOperations;
use crate::profile::scope::ProfileLease;

use super::types::{
    SignedGossipMessage, TOPIC_CATALOG, TOPIC_GOAL_TEMPLATES, TOPIC_GOVERNANCE, TOPIC_OPINIONS,
    TOPIC_PINBOARD, TOPIC_QUESTION_BANKS, TOPIC_SENTINEL_PRIORS, TOPIC_TAXONOMY, TOPIC_VC_DID,
    TOPIC_VC_PRESENTATION, TOPIC_VC_STATUS,
};

/// Error text the database executor returns when a lane has no waiting slot.
pub(crate) const DATABASE_BUSY: &str = "database is busy; retry the operation";

/// Historical refusal text for request/response protocols with an error
/// variant when no profile database is available.
const NO_ACTIVE_PROFILE: &str = "no active profile";

/// Accepted gossip waiting for the database. At 64 KiB per message this bounds
/// queued payloads to roughly 4 MiB.
pub(crate) const GOSSIP_QUEUE_CAPACITY: usize = 64;

const DROP_LOG_INTERVAL: Duration = Duration::from_secs(30);

pub(crate) fn is_busy(error: &str) -> bool {
    error == DATABASE_BUSY
}

/// Database handle for work that arrives from the network rather than from
/// an admitted IPC command.
#[derive(Clone)]
pub(crate) struct InboundDatabase {
    executor: DatabaseExecutor,
    operations: ProfileOperations,
    session: Arc<Mutex<Option<String>>>,
}

impl InboundDatabase {
    /// Fence work to a session already known to be current, such as the one
    /// that admitted the command starting the P2P node.
    pub(crate) fn pinned(
        executor: DatabaseExecutor,
        operations: ProfileOperations,
        session: String,
    ) -> Self {
        Self {
            executor,
            operations,
            session: Arc::new(Mutex::new(Some(session))),
        }
    }

    /// Fence work to the first session it is admitted under. For profile jobs
    /// spawned during startup, before admission opens; those jobs are joined by
    /// profile cleanup, so they cannot observe a later session first.
    pub(crate) fn pin_on_first_use(
        executor: DatabaseExecutor,
        operations: ProfileOperations,
    ) -> Self {
        Self {
            executor,
            operations,
            session: Arc::new(Mutex::new(None)),
        }
    }

    /// Loopback and integration fixtures have no profile lifecycle. They still
    /// run inbound work on a dedicated executor, under an admission that stays
    /// open for the fixture's lifetime.
    pub(crate) async fn detached(database: Arc<Mutex<Option<Database>>>) -> Result<Self, String> {
        let operations = ProfileOperations::default();
        operations
            .activate(async { Ok(()) }, async { Ok(()) })
            .await?;
        let session = operations
            .session()
            .ok_or("detached database admission did not open")?;
        Ok(Self::pinned(
            DatabaseExecutor::new(database),
            operations,
            session,
        ))
    }

    fn lease(&self) -> Result<ProfileLease, String> {
        let session = {
            let mut pinned = self
                .session
                .lock()
                .map_err(|_| "inbound profile session poisoned".to_string())?;
            match pinned.as_ref() {
                Some(session) => session.clone(),
                None => {
                    let current = self
                        .operations
                        .session()
                        .ok_or("profile session is locked or has changed")?;
                    *pinned = Some(current.clone());
                    current
                }
            }
        };
        self.operations.admit(&session)
    }

    /// Run `operation` on the Background lane. The lease is admitted when the
    /// returned future is first polled and is refused for a stale session;
    /// a full lane returns [`DATABASE_BUSY`] without waiting.
    pub(crate) async fn run<T, F>(&self, label: &'static str, operation: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&Database) -> Result<T, String> + Send + 'static,
    {
        let lease = self.lease()?;
        self.executor
            .execute(DatabaseWorkload::Background, lease, label, operation)
            .await
    }
}

/// Rate-limited warning for inbound work dropped under load. Owned by a
/// single task, so it needs no synchronization.
pub(crate) struct DropLog {
    what: &'static str,
    last: Option<Instant>,
    suppressed: u64,
}

impl DropLog {
    pub(crate) const fn new(what: &'static str) -> Self {
        Self {
            what,
            last: None,
            suppressed: 0,
        }
    }

    pub(crate) fn record(&mut self, reason: &str) {
        let now = Instant::now();
        if self
            .last
            .is_some_and(|last| now.duration_since(last) < DROP_LOG_INTERVAL)
        {
            self.suppressed = self.suppressed.saturating_add(1);
            return;
        }
        log::warn!(
            "{}: dropped under load ({reason}); {} earlier drop(s) not logged",
            self.what,
            self.suppressed
        );
        self.last = Some(now);
        self.suppressed = 0;
    }
}

/// Spawn `job` only while `jobs` holds fewer than `limit` tasks. Callers treat
/// `false` as overload; nothing waits for capacity.
pub(crate) fn try_spawn_bounded<T: Send + 'static>(
    jobs: &mut JoinSet<T>,
    limit: usize,
    job: impl Future<Output = T> + Send + 'static,
) -> bool {
    if jobs.len() >= limit {
        return false;
    }
    jobs.spawn(job);
    true
}

/// Refusal text for device-sync and guardian, which carry an error variant.
/// Busy work stays distinguishable so the requester retries; any other
/// refusal keeps the historical no-profile answer.
pub(crate) fn refusal_text(error: &str) -> String {
    if is_busy(error) {
        DATABASE_BUSY.to_string()
    } else {
        NO_ACTIVE_PROFILE.to_string()
    }
}

/// vc-fetch, graph-fetch and profile-fetch have no error variant. Busy work
/// omits the response rather than claiming `NotFound`/`NotOwner`; any other
/// refusal answers with the protocol's historical fallback.
pub(crate) fn fallback_unless_busy<R>(error: &str, fallback: R) -> Option<R> {
    (!is_busy(error)).then_some(fallback)
}

/// UI notification for inbound state, emitted only after its job committed.
#[derive(Debug, Clone)]
pub(crate) enum InboundUiEvent {
    ClassroomMessage(ClassroomMessageEvent),
    ClassroomMeta(ClassroomMetaTauriEvent),
}

impl InboundUiEvent {
    pub(crate) fn emit<R: tauri::Runtime>(self, app: &tauri::AppHandle<R>) {
        use tauri::Emitter;
        let result = match self {
            Self::ClassroomMessage(event) => app.emit(classroom_manager::MESSAGE_EVENT, event),
            Self::ClassroomMeta(event) => app.emit(classroom_manager::META_EVENT, event),
        };
        if let Err(error) = result {
            log::debug!("inbound gossip: UI event emission failed: {error}");
        }
    }
}

#[derive(Default)]
pub(crate) struct GossipCounters {
    completed: AtomicU64,
    busy_drops: AtomicU64,
    queue_drops: AtomicU64,
    refused: AtomicU64,
}

/// Swarm-side handle of the inbound gossip queue.
pub(crate) struct GossipIngest {
    queue: mpsc::Sender<(String, SignedGossipMessage)>,
    counters: Arc<GossipCounters>,
    drops: DropLog,
}

impl GossipIngest {
    /// Queue a validated message without waiting. Returns `false` when the
    /// message was dropped because the queue is full or the dispatcher stopped.
    pub(crate) fn offer(&mut self, topic: String, message: SignedGossipMessage) -> bool {
        match self.queue.try_send((topic, message)) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.counters.queue_drops.fetch_add(1, Ordering::Relaxed);
                self.drops.record("inbound gossip queue is full");
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        }
    }

    #[cfg(test)]
    fn counters(&self) -> Arc<GossipCounters> {
        Arc::clone(&self.counters)
    }
}

/// Create the bounded gossip queue and the dispatcher that drains it. The
/// dispatcher applies one message at a time and calls `emit` only after the
/// message's job has returned from the database thread.
pub(crate) fn gossip_ingest<E>(
    database: InboundDatabase,
    mut emit: E,
) -> (GossipIngest, impl Future<Output = ()> + Send + 'static)
where
    E: FnMut(InboundUiEvent) + Send + 'static,
{
    let (queue, mut receiver) =
        mpsc::channel::<(String, SignedGossipMessage)>(GOSSIP_QUEUE_CAPACITY);
    let counters = Arc::new(GossipCounters::default());
    let dispatcher_counters = Arc::clone(&counters);
    let dispatcher = async move {
        let mut busy = DropLog::new("inbound gossip");
        while let Some((topic, message)) = receiver.recv().await {
            match apply_gossip(&database, &topic, message).await {
                Ok(event) => {
                    dispatcher_counters
                        .completed
                        .fetch_add(1, Ordering::Relaxed);
                    if let Some(event) = event {
                        emit(event);
                    }
                }
                // Best-effort delivery: never retry or wait for capacity.
                Err(error) if is_busy(&error) => {
                    dispatcher_counters
                        .busy_drops
                        .fetch_add(1, Ordering::Relaxed);
                    busy.record(&error);
                }
                Err(error) => {
                    dispatcher_counters.refused.fetch_add(1, Ordering::Relaxed);
                    log::debug!("inbound gossip on {topic} refused: {error}");
                }
            }
        }
    };
    (
        GossipIngest {
            queue,
            counters,
            drops: DropLog::new("inbound gossip"),
        },
        dispatcher,
    )
}

/// Decode the payload outside the database job, then run the topic handler as
/// one job. Handlers keep their own validation and transaction boundaries.
async fn apply_gossip(
    database: &InboundDatabase,
    topic: &str,
    message: SignedGossipMessage,
) -> Result<Option<InboundUiEvent>, String> {
    let Some(work) = GossipWork::decode(topic, &message) else {
        return Ok(None);
    };
    let label = work.label();
    database
        .run(label, move |db| Ok(work.apply(db, &message)))
        .await
}

enum GossipWork {
    Catalog,
    Taxonomy,
    Governance,
    Opinion,
    Did,
    Status,
    Presentation,
    Pinboard,
    SentinelPrior,
    ContentVersion,
    ClassroomMessage(ClassroomMessagePayload),
    ClassroomMeta(ClassroomMetaEvent),
}

impl GossipWork {
    fn decode(topic: &str, message: &SignedGossipMessage) -> Option<Self> {
        Some(match topic {
            TOPIC_CATALOG => Self::Catalog,
            TOPIC_TAXONOMY => Self::Taxonomy,
            TOPIC_GOVERNANCE => Self::Governance,
            TOPIC_OPINIONS => Self::Opinion,
            TOPIC_VC_DID => Self::Did,
            TOPIC_VC_STATUS => Self::Status,
            TOPIC_VC_PRESENTATION => Self::Presentation,
            TOPIC_PINBOARD => Self::Pinboard,
            TOPIC_SENTINEL_PRIORS => Self::SentinelPrior,
            TOPIC_GOAL_TEMPLATES | TOPIC_QUESTION_BANKS => Self::ContentVersion,
            _ if is_classroom_topic(topic) && topic.ends_with("/meta/1.0") => {
                Self::ClassroomMeta(classroom_manager::decode_incoming_meta(message)?)
            }
            _ if is_classroom_topic(topic) => {
                Self::ClassroomMessage(classroom_manager::decode_incoming_message(message)?)
            }
            _ => return None,
        })
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Catalog => "p2p.gossip.catalog",
            Self::Taxonomy => "p2p.gossip.taxonomy",
            Self::Governance => "p2p.gossip.governance",
            Self::Opinion => "p2p.gossip.opinion",
            Self::Did => "p2p.gossip.vc-did",
            Self::Status => "p2p.gossip.vc-status",
            Self::Presentation => "p2p.gossip.vc-presentation",
            Self::Pinboard => "p2p.gossip.pinboard",
            Self::SentinelPrior => "p2p.gossip.sentinel-prior",
            Self::ContentVersion => "p2p.gossip.content-version",
            Self::ClassroomMessage(_) => "p2p.gossip.classroom-message",
            Self::ClassroomMeta(_) => "p2p.gossip.classroom-meta",
        }
    }

    fn apply(self, db: &Database, message: &SignedGossipMessage) -> Option<InboundUiEvent> {
        let label = self.label();
        let outcome = match self {
            Self::Catalog => super::catalog::handle_catalog_message(db, message).map(drop),
            Self::Taxonomy => super::taxonomy::handle_taxonomy_message(db, message).map(drop),
            Self::Governance => super::governance::handle_governance_message(db, message).map(drop),
            Self::Opinion => super::opinions::handle_opinion_message(db, message).map(drop),
            Self::Did => super::vc_did::handle_did_message(db, message).map(drop),
            Self::Status => super::vc_status::handle_status_message(db, message).map(drop),
            Self::Presentation => super::presentation::handle_presentation_message(db, message),
            Self::Pinboard => super::pinboard::handle_pinboard_message(db, message),
            Self::SentinelPrior => {
                super::sentinel::handle_sentinel_prior_message(db, message).map(drop)
            }
            Self::ContentVersion => {
                super::content::handle_content_version_message(db, message).map(drop)
            }
            Self::ClassroomMessage(payload) => {
                return classroom_manager::apply_incoming_message(db, message, payload)
                    .map(InboundUiEvent::ClassroomMessage);
            }
            Self::ClassroomMeta(event) => {
                return classroom_manager::apply_incoming_meta(db, message, event)
                    .map(InboundUiEvent::ClassroomMeta);
            }
        };
        if let Err(error) = outcome {
            log::debug!("{label}: message not applied: {error}");
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::Condvar;

    use super::*;
    use crate::p2p::device_sync::SyncResponse;
    use crate::p2p::vc_fetch::FetchResponse;

    fn shared_database() -> Arc<Mutex<Option<Database>>> {
        let database = Database::open_in_memory().expect("in-memory database");
        database.run_migrations().expect("migrations");
        Arc::new(Mutex::new(Some(database)))
    }

    async fn active_operations() -> (ProfileOperations, String) {
        let operations = ProfileOperations::default();
        operations
            .activate(async { Ok(()) }, async { Ok(()) })
            .await
            .expect("activate profile");
        let session = operations.session().expect("open session");
        (operations, session)
    }

    async fn wait_until(mut condition: impl FnMut() -> bool) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !condition() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("inbound test condition was not reached");
    }

    #[derive(Default)]
    struct Gate {
        started: AtomicBool,
        released: Mutex<bool>,
        ready: Condvar,
    }

    impl Gate {
        fn block(&self) {
            self.started.store(true, Ordering::Release);
            let mut released = self.released.lock().expect("gate lock");
            while !*released {
                released = self.ready.wait(released).expect("gate lock");
            }
        }

        fn started(&self) -> bool {
            self.started.load(Ordering::Acquire)
        }

        fn release(&self) {
            *self.released.lock().expect("gate lock") = true;
            self.ready.notify_all();
        }
    }

    /// Occupy the single database thread with learner work.
    fn block_database(
        executor: &DatabaseExecutor,
        operations: &ProfileOperations,
        session: &str,
    ) -> (Arc<Gate>, tokio::task::JoinHandle<Result<(), String>>) {
        let gate = Arc::new(Gate::default());
        let blocker_gate = Arc::clone(&gate);
        let lease = operations.admit(session).expect("blocker lease");
        let executor = executor.clone();
        let blocker = tokio::spawn(async move {
            executor
                .execute(
                    DatabaseWorkload::Learner,
                    lease,
                    "test.inbound-blocker",
                    move |_| {
                        blocker_gate.block();
                        Ok(())
                    },
                )
                .await
        });
        (gate, blocker)
    }

    fn catalog_message(content_cid: &str) -> (String, SignedGossipMessage) {
        let author = "stake_test1uinbounddispatch";
        let announcement = crate::p2p::catalog::build_catalog_announcement(
            &"11".repeat(32),
            author,
            "Inbound fencing",
            None,
            content_cid,
            None,
            &[],
            &[],
            1,
            "course",
        );
        let message = SignedGossipMessage {
            topic: TOPIC_CATALOG.into(),
            payload: serde_json::to_vec(&announcement).expect("catalog payload"),
            signature: vec![0; 64],
            public_key: vec![0; 32],
            stake_address: author.into(),
            timestamp: 0,
            encrypted: false,
            key_id: None,
        };
        (announcement.course_id, message)
    }

    fn catalog_rows(shared: &Arc<Mutex<Option<Database>>>, course_id: &str) -> i64 {
        let guard = shared.lock().expect("database slot");
        guard
            .as_ref()
            .expect("database")
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM catalog WHERE course_id = ?1",
                [course_id],
                |row| row.get(0),
            )
            .expect("catalog count")
    }

    #[tokio::test(flavor = "current_thread")]
    async fn inbound_job_runs_on_the_database_thread_not_the_async_runtime() {
        let shared = shared_database();
        let (operations, session) = active_operations().await;
        let inbound = InboundDatabase::pinned(
            DatabaseExecutor::new(Arc::clone(&shared)),
            operations,
            session,
        );
        let gate = Arc::new(Gate::default());
        let job_gate = Arc::clone(&gate);
        let job_inbound = inbound.clone();
        let job = tokio::spawn(async move {
            job_inbound
                .run("test.inbound-block", move |_| {
                    job_gate.block();
                    Ok(std::thread::current().name().map(str::to_owned))
                })
                .await
        });

        wait_until(|| gate.started()).await;
        tokio::time::timeout(Duration::from_millis(100), tokio::task::yield_now())
            .await
            .expect("runtime remained responsive while inbound SQL was blocked");
        // Queuing gossip behind the blocked job returns without waiting.
        let (mut ingest, _dispatcher) = gossip_ingest(inbound.clone(), |_| {});
        let (_, message) = catalog_message("cid-responsive");
        assert!(ingest.offer(TOPIC_CATALOG.into(), message));

        gate.release();
        assert_eq!(
            job.await.expect("join").expect("inbound result").as_deref(),
            Some("alexandria-database")
        );
    }

    #[tokio::test]
    async fn gossip_for_a_stale_profile_is_refused_and_writes_nothing() {
        let shared = shared_database();
        let (operations, session) = active_operations().await;
        let executor = DatabaseExecutor::new(Arc::clone(&shared));
        let inbound =
            InboundDatabase::pinned(executor.clone(), operations.clone(), session.clone());
        let (mut ingest, dispatcher) = gossip_ingest(inbound.clone(), |_| {});
        let counters = ingest.counters();
        let dispatcher = tokio::spawn(dispatcher);

        // Control: the pinned session is current, so the handler commits.
        let (current_id, current) = catalog_message("cid-current");
        assert!(ingest.offer(TOPIC_CATALOG.into(), current));
        wait_until(|| counters.completed.load(Ordering::Relaxed) == 1).await;
        assert_eq!(catalog_rows(&shared, &current_id), 1);

        // Admit and queue a message while the session is current, then lock.
        let (gate, blocker) = block_database(&executor, &operations, &session);
        wait_until(|| gate.started()).await;
        let (queued_id, queued) = catalog_message("cid-queued");
        let mut queued_job = Box::pin(apply_gossip(&inbound, TOPIC_CATALOG, queued));
        assert!(futures::poll!(&mut queued_job).is_pending());
        let lock_operations = operations.clone();
        let lock = tokio::spawn(async move { lock_operations.lock(async { Ok(()) }).await });
        wait_until(|| operations.session().is_none()).await;
        gate.release();
        blocker
            .await
            .expect("blocker join")
            .expect("blocker result");
        assert_eq!(
            queued_job.await.expect_err("stale job is refused"),
            "profile session changed before database work started"
        );
        lock.await.expect("lock join").expect("lock completes");
        operations
            .activate(async { Ok(()) }, async { Ok(()) })
            .await
            .expect("activate next profile");

        // Arriving after the switch: refused at admission, never queued.
        let (late_id, late) = catalog_message("cid-late");
        assert!(ingest.offer(TOPIC_CATALOG.into(), late));
        wait_until(|| counters.refused.load(Ordering::Relaxed) == 1).await;

        assert_eq!(catalog_rows(&shared, &queued_id), 0);
        assert_eq!(catalog_rows(&shared, &late_id), 0);
        assert_eq!(counters.completed.load(Ordering::Relaxed), 1);
        drop(ingest);
        dispatcher
            .await
            .expect("dispatcher stops when the swarm side closes");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn background_overload_is_rejected_without_waiting_or_spawning() {
        let shared = shared_database();
        let (operations, session) = active_operations().await;
        let executor = DatabaseExecutor::new(Arc::clone(&shared));
        let inbound =
            InboundDatabase::pinned(executor.clone(), operations.clone(), session.clone());
        let (gate, blocker) = block_database(&executor, &operations, &session);
        wait_until(|| gate.started()).await;

        // Fill the eight reserved Background waiting slots.
        let mut waiting = Vec::new();
        for _ in 0..8 {
            let mut job = Box::pin(inbound.run("test.inbound-fill", |_| Ok(())));
            assert!(futures::poll!(&mut job).is_pending());
            waiting.push(job);
        }
        let overflow = tokio::time::timeout(
            Duration::from_millis(100),
            inbound.run("test.inbound-overflow", |_| Ok(())),
        )
        .await
        .expect("overload is reported immediately");
        assert_eq!(overflow, Err(DATABASE_BUSY.to_string()));

        // Gossip on a busy lane is dropped and the dispatcher moves on.
        let (mut ingest, dispatcher) = gossip_ingest(inbound.clone(), |_| {});
        let counters = ingest.counters();
        let dispatcher = tokio::spawn(dispatcher);
        let (dropped_id, dropped) = catalog_message("cid-overloaded");
        assert!(ingest.offer(TOPIC_CATALOG.into(), dropped));
        wait_until(|| counters.busy_drops.load(Ordering::Relaxed) == 1).await;

        // Request/response replies on a busy lane stay retryable.
        assert_eq!(refusal_text(DATABASE_BUSY), DATABASE_BUSY);
        assert!(matches!(
            SyncResponse::Error(refusal_text(DATABASE_BUSY)),
            SyncResponse::Error(text) if is_busy(&text)
        ));
        assert!(fallback_unless_busy(DATABASE_BUSY, FetchResponse::NotFound).is_none());
        assert!(matches!(
            fallback_unless_busy(
                "profile session is locked or has changed",
                FetchResponse::NotFound
            ),
            Some(FetchResponse::NotFound)
        ));
        assert_eq!(
            refusal_text("profile session is locked or has changed"),
            NO_ACTIVE_PROFILE
        );

        // Internal buffering is bounded: a stalled dispatcher never makes the
        // swarm side wait, and bounded job sets refuse instead of growing.
        let (mut stalled, _not_polled) = gossip_ingest(inbound.clone(), |_| {});
        let stalled_counters = stalled.counters();
        for index in 0..GOSSIP_QUEUE_CAPACITY {
            let (_, message) = catalog_message(&format!("cid-queued-{index}"));
            assert!(stalled.offer(TOPIC_CATALOG.into(), message));
        }
        let (_, extra) = catalog_message("cid-queue-overflow");
        assert!(!stalled.offer(TOPIC_CATALOG.into(), extra));
        assert_eq!(stalled_counters.queue_drops.load(Ordering::Relaxed), 1);
        let mut jobs = JoinSet::new();
        assert!(try_spawn_bounded(
            &mut jobs,
            2,
            std::future::pending::<()>()
        ));
        assert!(try_spawn_bounded(
            &mut jobs,
            2,
            std::future::pending::<()>()
        ));
        assert!(!try_spawn_bounded(
            &mut jobs,
            2,
            std::future::pending::<()>()
        ));
        assert_eq!(jobs.len(), 2);
        jobs.abort_all();

        gate.release();
        blocker
            .await
            .expect("blocker join")
            .expect("blocker result");
        for job in waiting {
            job.await.expect("queued background work completes");
        }
        assert_eq!(catalog_rows(&shared, &dropped_id), 0);
        drop(ingest);
        dispatcher.await.expect("dispatcher join");
    }
}
