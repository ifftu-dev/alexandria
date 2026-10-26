//! The run: a begun turn streamed into its answer.
//!
//! A [`Run`] takes a begun turn and drives it to one terminal state. It emits
//! the events the panel renders in order with their sequence numbers, saves
//! checkpoints at a bounded cadence, stops when asked or when an assessment
//! starts, and completes the answer through the turn, which stores it with
//! its cards. Nothing here knows about Tauri. The run has three seams, each
//! with a production adapter in this file and an in-memory one in its tests:
//!
//! * [`EventSink`]: where events go. The IPC channel in production, a vector
//!   in tests. The run numbers events; the sink only delivers them.
//! * [`StreamSource`]: where text comes from. The AI crate's adapter
//!   ([`ProviderStream`]) in production, a scripted answer in tests. A
//!   source connects first, and that step is never cut short by Stop; only
//!   the stream itself is.
//! * [`RunStore`]: how the profile database is reached. The executor under
//!   the profile lease ([`ExecutorStore`]) in production, the connection held
//!   directly in tests.
//!
//! See `docs/ai-chat-spec.md` section 11 for the event contract, checkpoint
//! cadence, and stop semantics the run keeps.

use std::future::Future;
use std::pin::Pin;
use std::time::{Duration, Instant};

use alexandria_ai::endpoint::{self, Location};
use alexandria_ai::model::Limits;
use alexandria_ai::{Adapter, ChatRequest, Completion, Protocol, StreamEvent, Usage};
use alexandria_studio::gate;
use alexandria_studio::model::StudioConnection;
use rusqlite::Connection;
use serde::Serialize;
use tokio::sync::{mpsc, oneshot};

use super::store::{self as chat_store, Finish};
use super::turn::{self, Begun, Started, StartedRun};
use super::vocab::ChatErrorCode;
use super::{error_code, now, RunGuard};
use crate::db::executor::{DatabaseExecutor, DatabaseWorkload};
use crate::profile::scope::ProfileLease;

/// Saved at least this often while streaming.
const CHECKPOINT_INTERVAL: Duration = Duration::from_millis(500);
const CHECKPOINT_BYTES: usize = 2_048;

// ─── events ──────────────────────────────────────────────────────────────

/// One event of a run, as the panel receives it.
#[derive(Debug, Clone, Serialize)]
pub struct ChatEvent {
    pub run_id: String,
    pub thread_id: String,
    pub seq: u64,
    #[serde(flatten)]
    pub body: ChatEventBody,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatEventBody {
    RunStarted,
    TextDelta {
        text: String,
    },
    UsageUpdated {
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    },
    RunCompleted {
        finish: String,
    },
    RunFailed {
        error: String,
        partial: bool,
    },
    RunCancelled {
        error: Option<String>,
    },
}

// ─── seams ───────────────────────────────────────────────────────────────

/// Where a run's events go. Delivery failures are the sink's to swallow: the
/// panel may be closed, and the answer still completes and is saved.
pub trait EventSink: Send {
    fn send(&mut self, event: ChatEvent);
}

impl<F: FnMut(ChatEvent) + Send> EventSink for F {
    fn send(&mut self, event: ChatEvent) {
        self(event)
    }
}

/// Why a stream ended without an answer.
#[derive(Debug)]
pub enum Failure {
    /// The provider or the connection failed; [`error_code`] names it.
    Provider(alexandria_ai::Error),
    /// The request was never sent; carries the stable code to record.
    NotSent(String),
}

impl Failure {
    fn code(&self) -> String {
        match self {
            Failure::Provider(error) => error_code(error),
            Failure::NotSent(code) => code.clone(),
        }
    }

    fn may_have_been_accepted(&self) -> bool {
        match self {
            Failure::Provider(error) => error.may_have_been_accepted(),
            Failure::NotSent(_) => false,
        }
    }
}

/// A future a seam hands back. Boxed rather than `impl Future`: rustc cannot
/// prove a spawned run `Send` through a trait's opaque futures.
pub type SeamFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Where a run's text comes from: something that delivers deltas as they
/// arrive and then returns the whole answer, or why there is none. The
/// channel's capacity bounds how far decoding runs ahead of the run.
pub trait StreamSource: Send + 'static {
    /// Gets ready to send. The run awaits this to the end before it listens
    /// for Stop, so work that must not be abandoned halfway (renewing a
    /// sign-in, whose rotated token has to be saved) belongs here.
    fn connect(&mut self) -> SeamFuture<'_, Result<(), Failure>> {
        Box::pin(std::future::ready(Ok(())))
    }

    /// Sends the request. Dropping the future ends it.
    fn stream(
        self,
        events: mpsc::Sender<StreamEvent>,
    ) -> SeamFuture<'static, Result<Completion, Failure>>;
}

/// How a run reaches the profile database: one operation on its connection.
pub trait RunStore: Send + Sync {
    fn execute<T, F>(&self, label: &'static str, operation: F) -> SeamFuture<'_, Result<T, String>>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T, String> + Send + 'static;
}

// ─── production adapters ─────────────────────────────────────────────────

/// The profile database behind its executor, under the profile lease.
#[derive(Clone)]
pub struct ExecutorStore {
    pub(crate) executor: DatabaseExecutor,
    pub lease: ProfileLease,
}

impl RunStore for ExecutorStore {
    fn execute<T, F>(&self, label: &'static str, operation: F) -> SeamFuture<'_, Result<T, String>>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T, String> + Send + 'static,
    {
        Box::pin(self.executor.execute(
            DatabaseWorkload::Background,
            self.lease.clone(),
            label,
            move |db| operation(db.conn()),
        ))
    }
}

/// The AI crate's adapter for the begun turn's connection. Connecting
/// resolves the endpoint and renews a sign-in; streaming sends the request.
pub struct ProviderStream {
    db: ExecutorStore,
    request: ChatRequest,
    endpoint: String,
    location: Location,
    protocol: Protocol,
    /// The stored secret until connecting renews it into the bearer to send.
    key: Option<String>,
    connection: StudioConnection,
    /// Set by a successful connect.
    resolved: Option<alexandria_ai::Endpoint>,
}

impl ProviderStream {
    /// Consumes the begun turn: the request and the provider secret move
    /// into the source, and what completing needs comes back as [`Begun`].
    pub fn new(db: ExecutorStore, mut started: Started) -> (Self, Begun) {
        let key = started.take_key();
        let Started {
            run,
            mode,
            request,
            endpoint,
            location,
            protocol,
            connection,
            ..
        } = started;
        let source = Self {
            db,
            request,
            endpoint,
            location,
            protocol,
            key,
            connection,
            resolved: None,
        };
        (source, Begun { run, mode })
    }
}

impl StreamSource for ProviderStream {
    fn connect(&mut self) -> SeamFuture<'_, Result<(), Failure>> {
        Box::pin(async move {
            let endpoint = endpoint::resolve(&self.endpoint, self.location)
                .await
                .map_err(Failure::Provider)?;
            // A ChatGPT sign-in renews its hour-long token here, before
            // sending. The refresh token rotates, so the renewal and its save
            // must finish even if the learner stops the answer meanwhile.
            self.key = crate::ai_auth::credential::bearer_for(
                &self.db.executor,
                &self.db.lease,
                &self.connection,
                self.key.take(),
            )
            .await
            .map_err(|error| {
                Failure::NotSent(error.split(':').next().unwrap_or("auth").to_string())
            })?;
            self.resolved = Some(endpoint);
            Ok(())
        })
    }

    fn stream(
        self,
        events: mpsc::Sender<StreamEvent>,
    ) -> SeamFuture<'static, Result<Completion, Failure>> {
        Box::pin(async move {
            let endpoint = self
                .resolved
                .ok_or_else(|| Failure::NotSent("invalid".into()))?;
            let adapter =
                Adapter::new(self.protocol, endpoint, self.key).with_limits(Limits::default());
            adapter
                .stream(&self.request, events)
                .await
                .map_err(Failure::Provider)
        })
    }
}

// ─── the run ─────────────────────────────────────────────────────────────

/// A begun turn being streamed into its answer. [`Run::drive`] is its one
/// operation; the guard keeps the run registered until it returns.
pub struct Run<Store, Source, Sink> {
    begun: Begun,
    store: Store,
    source: Source,
    sink: Sink,
    cancel: oneshot::Receiver<()>,
    _guard: RunGuard,
}

/// How the stream ended.
enum RunEnd {
    Done(Result<Completion, Failure>),
    Stopped(Option<&'static str>),
}

/// Numbers events and delivers them.
struct Events<Sink> {
    sink: Sink,
    run_id: String,
    thread_id: String,
    seq: u64,
}

impl<Sink: EventSink> Events<Sink> {
    fn emit(&mut self, body: ChatEventBody) {
        self.seq += 1;
        self.sink.send(ChatEvent {
            run_id: self.run_id.clone(),
            thread_id: self.thread_id.clone(),
            seq: self.seq,
            body,
        });
    }

    fn usage(&mut self, usage: Usage) {
        self.emit(ChatEventBody::UsageUpdated {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
        });
    }
}

impl<Store, Source, Sink> Run<Store, Source, Sink>
where
    Store: RunStore,
    Source: StreamSource,
    Sink: EventSink,
{
    /// `slot` is the registration from [`super::ChatRuntime::register`]: the
    /// guard that holds the run's place and the receiver that stops it.
    pub fn new(
        begun: Begun,
        store: Store,
        source: Source,
        sink: Sink,
        slot: (RunGuard, oneshot::Receiver<()>),
    ) -> Self {
        let (guard, cancel) = slot;
        Self {
            begun,
            store,
            source,
            sink,
            cancel,
            _guard: guard,
        }
    }

    /// Drives the run to its end: every event emitted in order, the answer
    /// saved with its usage, or the stop or failure recorded with whatever
    /// text arrived.
    pub async fn drive(self) {
        let Run {
            begun,
            store,
            mut source,
            sink,
            mut cancel,
            _guard,
        } = self;
        let mut events = Events {
            sink,
            run_id: begun.run.run_id.clone(),
            thread_id: begun.run.thread_id.clone(),
            seq: 0,
        };
        let message_id = begun.run.assistant_message.id.clone();
        events.emit(ChatEventBody::RunStarted);
        let ids = (events.run_id.clone(), message_id.clone());

        // Not raced with Stop: a stop that arrives meanwhile is seen as soon
        // as the stream starts.
        if let Err(failure) = source.connect().await {
            let code = failure.code();
            record(
                &store,
                ids,
                "failed",
                String::new(),
                Some(code.clone()),
                None,
            )
            .await;
            events.emit(ChatEventBody::RunFailed {
                error: code,
                partial: false,
            });
            return;
        }

        let (tx, mut rx) = mpsc::channel::<StreamEvent>(64);
        let mut stream = source.stream(tx);

        let mut text = String::new();
        let mut usage: Option<Usage> = None;
        let mut unsaved = 0usize;
        let mut last_save = Instant::now();
        let mut events_open = true;
        let end = loop {
            tokio::select! {
                biased;
                _ = &mut cancel => break RunEnd::Stopped(None),
                event = rx.recv(), if events_open => match event {
                    Some(StreamEvent::TextDelta { text: delta }) => {
                        unsaved += delta.len();
                        text.push_str(&delta);
                        events.emit(ChatEventBody::TextDelta { text: delta });
                        if unsaved >= CHECKPOINT_BYTES || last_save.elapsed() >= CHECKPOINT_INTERVAL {
                            unsaved = 0;
                            last_save = Instant::now();
                            if !checkpoint(&store, message_id.clone(), text.clone()).await {
                                break RunEnd::Stopped(Some(ChatErrorCode::AssessmentActive.as_str()));
                            }
                        }
                    }
                    Some(StreamEvent::Usage { usage: u }) => {
                        usage = Some(u);
                        events.usage(u);
                    }
                    Some(StreamEvent::Finished { .. }) => {}
                    None => events_open = false,
                },
                result = stream.as_mut() => {
                    // Deliver anything decoded before the stream returned.
                    while let Ok(event) = rx.try_recv() {
                        match event {
                            StreamEvent::TextDelta { text: delta } => {
                                text.push_str(&delta);
                                events.emit(ChatEventBody::TextDelta { text: delta });
                            }
                            StreamEvent::Usage { usage: u } => {
                                usage = Some(u);
                                events.usage(u);
                            }
                            StreamEvent::Finished { .. } => {}
                        }
                    }
                    break RunEnd::Done(result);
                }
            }
        };
        // Ends the provider request now, not when the run returns: a stopped
        // answer's request must not stay open while the stop is recorded. The
        // provider may still have computed (and billed) the answer.
        drop(stream);
        drop(rx);

        match end {
            RunEnd::Done(Ok(completion)) => {
                let usage = completion.usage.or(usage);
                let finish = serde_json::to_value(&completion.finish)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "unknown".into());
                match complete(&store, begun, completion.text, usage).await {
                    Ok(Completed::Saved) => {
                        events.emit(ChatEventBody::RunCompleted { finish });
                    }
                    Ok(Completed::AssessmentActive) => {
                        let error = "assessment_active";
                        record(&store, ids, "cancelled", text, Some(error.into()), usage).await;
                        events.emit(ChatEventBody::RunCancelled {
                            error: Some(error.into()),
                        });
                    }
                    Err(error) => {
                        log::warn!("chat: could not save an answer: {error}");
                        let code = ChatErrorCode::StorageError.to_string();
                        let partial = !text.trim().is_empty();
                        record(&store, ids, "failed", text, Some(code.clone()), usage).await;
                        events.emit(ChatEventBody::RunFailed {
                            error: code,
                            partial,
                        });
                    }
                }
            }
            RunEnd::Done(Err(failure)) => {
                let code = failure.code();
                let partial = !text.trim().is_empty();
                let state = if partial && failure.may_have_been_accepted() {
                    "interrupted"
                } else {
                    "failed"
                };
                record(&store, ids, state, text, Some(code.clone()), usage).await;
                events.emit(ChatEventBody::RunFailed {
                    error: code,
                    partial,
                });
            }
            RunEnd::Stopped(reason) => {
                let error = reason.unwrap_or("cancelled").to_string();
                record(&store, ids, "cancelled", text, Some(error), usage).await;
                events.emit(ChatEventBody::RunCancelled {
                    error: reason.map(str::to_owned),
                });
            }
        }
    }
}

/// Records that a begun run will never be driven, so its thread is not left
/// generating.
pub async fn abandon(db: &impl RunStore, run: &StartedRun, code: &'static str) {
    let ids = (run.run_id.clone(), run.assistant_message.id.clone());
    record(db, ids, "failed", String::new(), Some(code.into()), None).await;
}

/// Saves the text so far. False when an assessment has started and the
/// answer must stop.
async fn checkpoint(db: &impl RunStore, id: String, text: String) -> bool {
    db.execute("chat.checkpoint", move |conn| {
        if gate::assessment_in_progress(conn).unwrap_or(true) {
            return Ok(false);
        }
        chat_store::checkpoint(conn, &id, &text).map_err(|e| e.to_string())?;
        Ok(true)
    })
    .await
    .unwrap_or(false)
}

/// What completing a finished answer did.
enum Completed {
    Saved,
    /// An assessment started after the last checkpoint; nothing was saved.
    AssessmentActive,
}

/// Completes the answer through the turn, which stores it with its cards in
/// one transaction, unless an assessment has started since the last
/// checkpoint. A short answer may never reach a checkpoint, so the gate is
/// checked here too, in the same database hop as the write.
async fn complete(
    db: &impl RunStore,
    begun: Begun,
    reply: String,
    usage: Option<Usage>,
) -> Result<Completed, String> {
    db.execute("chat.finish", move |conn| {
        if gate::assessment_in_progress(conn).unwrap_or(true) {
            return Ok(Completed::AssessmentActive);
        }
        turn::complete(conn, &begun, &reply, usage).map(|_| Completed::Saved)
    })
    .await
}

/// Records a run that ended without a completed answer.
/// `ids` are the run and its answer's message. A run that already ended (a
/// recovery marked it interrupted) is left as it is.
async fn record(
    db: &impl RunStore,
    ids: (String, String),
    state: &'static str,
    content: String,
    error: Option<String>,
    usage: Option<Usage>,
) {
    let (run_id, message_id) = ids;
    let ended = run_id.clone();
    let recorded = db
        .execute("chat.finish", move |conn| {
            chat_store::finish_run(
                conn,
                &Finish {
                    run_id: &run_id,
                    assistant_message_id: &message_id,
                    state,
                    content: &content,
                    error: error.as_deref(),
                    input_tokens: usage.and_then(|u| u.input_tokens),
                    output_tokens: usage.and_then(|u| u.output_tokens),
                    now: &now(),
                },
            )
            .map_err(|e| e.to_string())
        })
        .await;
    match recorded {
        Ok(true) => {}
        Ok(false) => log::info!("chat: run {ended} had already ended; kept as it was"),
        Err(error) => log::warn!("chat: could not record how a run ended: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_chat::turn::fixtures::*;
    use crate::ai_chat::{store, ChatRuntime};
    use crate::commands::ai_chat::attach;
    use crate::db::Database;
    use crate::profile::scope::Admission;
    use alexandria_ai::FinishReason;
    use alexandria_studio::store as studio_store;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    type Recorded = Arc<Mutex<Vec<serde_json::Value>>>;

    /// The test sink: every event, as the panel would decode it.
    fn vector_sink() -> (impl FnMut(ChatEvent) + Send, Recorded) {
        let events: Recorded = Arc::default();
        let sink = events.clone();
        (
            move |event: ChatEvent| {
                sink.lock()
                    .unwrap()
                    .push(serde_json::to_value(&event).unwrap());
            },
            events,
        )
    }

    /// The test store: the profile database held directly, no executor.
    struct Direct(Arc<Mutex<Option<Database>>>);

    impl RunStore for Direct {
        fn execute<T, F>(
            &self,
            _label: &'static str,
            operation: F,
        ) -> SeamFuture<'_, Result<T, String>>
        where
            T: Send + 'static,
            F: FnOnce(&Connection) -> Result<T, String> + Send + 'static,
        {
            let result = {
                let guard = self.0.lock().unwrap();
                operation(guard.as_ref().unwrap().conn())
            };
            Box::pin(std::future::ready(result))
        }
    }

    /// The test source: an answer delivered word by word with a pause
    /// between words, then its ending, as the AI crate's adapters do.
    struct Scripted {
        words: &'static [&'static str],
        pause: Duration,
        ending: Result<Completion, Failure>,
        /// A slow connect, like a sign-in renewal, and whether it finished.
        connecting: Option<(Duration, Arc<std::sync::atomic::AtomicBool>)>,
    }

    impl Scripted {
        fn answer(words: &'static [&'static str], pause_ms: u64) -> Self {
            Self {
                words,
                pause: Duration::from_millis(pause_ms),
                ending: Ok(Completion {
                    text: words.concat(),
                    usage: Some(Usage {
                        input_tokens: Some(40),
                        output_tokens: Some(3),
                    }),
                    finish: FinishReason::Stop,
                }),
                connecting: None,
            }
        }

        fn failing(error: alexandria_ai::Error) -> Self {
            Self {
                words: &[],
                pause: Duration::ZERO,
                ending: Err(Failure::Provider(error)),
                connecting: None,
            }
        }
    }

    impl StreamSource for Scripted {
        fn connect(&mut self) -> SeamFuture<'_, Result<(), Failure>> {
            Box::pin(async move {
                if let Some((delay, done)) = &self.connecting {
                    tokio::time::sleep(*delay).await;
                    done.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                Ok(())
            })
        }

        fn stream(
            self,
            events: mpsc::Sender<StreamEvent>,
        ) -> SeamFuture<'static, Result<Completion, Failure>> {
            Box::pin(async move {
                for word in self.words {
                    let delta = StreamEvent::TextDelta {
                        text: word.to_string(),
                    };
                    if events.send(delta).await.is_err() {
                        break;
                    }
                    tokio::time::sleep(self.pause).await;
                }
                if let Ok(completion) = &self.ending {
                    if let Some(usage) = completion.usage {
                        let _ = events.send(StreamEvent::Usage { usage }).await;
                    }
                    let _ = events
                        .send(StreamEvent::Finished {
                            reason: completion.finish.clone(),
                        })
                        .await;
                }
                self.ending
            })
        }
    }

    struct Harness {
        _dir: tempfile::TempDir,
        shared: Arc<Mutex<Option<Database>>>,
        executor: DatabaseExecutor,
        lease: ProfileLease,
        _admission: Admission,
        runtime: Arc<ChatRuntime>,
    }

    impl Harness {
        fn new(endpoint: &str) -> Self {
            let dir = tempfile::TempDir::new().unwrap();
            let database = Database::open(&dir.path().join("profile.db")).unwrap();
            seed(database.conn());
            connection(database.conn(), "c1", "local", endpoint, None);
            let shared = Arc::new(Mutex::new(Some(database)));
            let admission = Admission::default();
            let session = admission.close();
            assert!(admission.open(&session));
            let lease = admission.admit(&session).unwrap();
            Self {
                _dir: dir,
                executor: DatabaseExecutor::new(shared.clone()),
                shared,
                lease,
                _admission: admission,
                runtime: Arc::new(ChatRuntime::default()),
            }
        }

        fn conn<T>(&self, f: impl FnOnce(&Connection) -> T) -> T {
            let guard = self.shared.lock().unwrap();
            f(guard.as_ref().unwrap().conn())
        }

        fn direct(&self) -> Direct {
            Direct(self.shared.clone())
        }

        fn executor_store(&self) -> ExecutorStore {
            ExecutorStore {
                executor: self.executor.clone(),
                lease: self.lease.clone(),
            }
        }

        /// Prepares and begins a turn in thread `t`, with the lesson attached.
        fn begin(&self, text: &str) -> Started {
            self.begin_in("ask", text)
        }

        fn begin_in(&self, mode: &str, text: &str) -> Started {
            self.conn(|conn| {
                if store::thread(conn, "t").is_err() {
                    thread(conn);
                    attach(conn, "t", "course", "lesson").unwrap();
                }
                store::set_thread_mode(conn, "t", mode, &now()).unwrap();
                let prepared = prepare(conn, text).unwrap();
                turn::begin(
                    conn,
                    &self.runtime,
                    &prepared.prepare_id,
                    &prepared.digest,
                    &uuid::Uuid::new_v4().to_string(),
                )
                .unwrap()
            })
        }

        /// Drives a begun turn over the scripted source and the direct
        /// store, and returns the events it emitted.
        async fn drive(&self, started: &Started, source: Scripted) -> Vec<serde_json::Value> {
            self.drive_with(started.begun(), self.direct(), source)
                .await
        }

        /// Drives a begun turn over the production adapters: the AI crate's
        /// adapter and the executor-backed store.
        async fn drive_provider(&self, started: Started) -> Vec<serde_json::Value> {
            let (source, begun) = ProviderStream::new(self.executor_store(), started);
            self.drive_with(begun, self.executor_store(), source).await
        }

        async fn drive_with(
            &self,
            begun: Begun,
            store: impl RunStore,
            source: impl StreamSource,
        ) -> Vec<serde_json::Value> {
            let slot = self.runtime.register(&begun.run.run_id).unwrap();
            let (sink, events) = vector_sink();
            Run::new(begun, store, source, sink, slot).drive().await;
            let events = events.lock().unwrap().clone();
            events
        }
    }

    fn kinds(events: &[serde_json::Value]) -> Vec<String> {
        events
            .iter()
            .map(|e| e["kind"].as_str().unwrap().to_string())
            .collect()
    }

    // ─── over the scripted source ───────────────────────────────────────

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_answer_streams_in_order_and_is_saved_with_its_usage() {
        let harness = Harness::new("http://127.0.0.1:11434/v1");
        let started = harness.begin("Why?");
        let message_id = started.run.assistant_message.id.clone();
        let events = harness
            .drive(
                &started,
                Scripted::answer(&["Because ", "it is ", "rare."], 5),
            )
            .await;

        assert_eq!(
            kinds(&events),
            [
                "run_started",
                "text_delta",
                "text_delta",
                "text_delta",
                "usage_updated",
                "run_completed"
            ]
        );
        let seqs: Vec<u64> = events.iter().map(|e| e["seq"].as_u64().unwrap()).collect();
        assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1));
        let saved = harness.conn(|c| store::message(c, &message_id).unwrap());
        assert_eq!(saved.status, "complete");
        assert_eq!(saved.content, "Because it is rare.");
        assert_eq!(
            (saved.input_tokens, saved.output_tokens),
            (Some(40), Some(3))
        );
        assert_eq!(saved.sources.len(), 1);
        assert!(harness
            .conn(|c| store::active_run(c, "t").unwrap())
            .is_none());
        // The task left the registry, so recovery treats nothing as live.
        assert!(harness.runtime.has_capacity());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stopping_keeps_the_partial_answer_and_records_the_stop() {
        let harness = Harness::new("http://127.0.0.1:11434/v1");
        let started = harness.begin("Count");
        let run_id = started.run.run_id.clone();
        let message_id = started.run.assistant_message.id.clone();
        let runtime = harness.runtime.clone();
        let stopper = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(220)).await;
            assert!(runtime.cancel(&run_id));
        });
        let events = harness
            .drive(
                &started,
                Scripted::answer(&["One ", "two ", "three ", "four ", "five "], 150),
            )
            .await;
        stopper.await.unwrap();
        let saved = harness.conn(|c| store::message(c, &message_id).unwrap());
        assert_eq!(saved.status, "cancelled");
        assert!(saved.content.starts_with("One "));
        assert!(!saved.content.contains("five"));
        assert_eq!(kinds(&events).last().unwrap(), "run_cancelled");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_assessment_starting_mid_answer_stops_it() {
        // The pause between words is longer than the checkpoint interval, so
        // the second word forces a checkpoint, which checks the gate.
        static WORDS: [&str; 3] = ["Start ", "middle ", "end"];
        let harness = Harness::new("http://127.0.0.1:11434/v1");
        let started = harness.begin("Explain");
        let message_id = started.run.assistant_message.id.clone();
        harness.conn(|c| {
            c.execute("INSERT INTO integrity_sessions(id) VALUES ('attempt')", [])
                .unwrap()
        });
        let events = harness.drive(&started, Scripted::answer(&WORDS, 700)).await;
        let saved = harness.conn(|c| store::message(c, &message_id).unwrap());
        assert_eq!(saved.status, "cancelled");
        assert_eq!(saved.error.as_deref(), Some("assessment_active"));
        let last = events.last().unwrap();
        assert_eq!(last["kind"], "run_cancelled");
        assert_eq!(last["error"], "assessment_active");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_unreachable_local_model_fails_without_text() {
        let harness = Harness::new("http://127.0.0.1:11434/v1");
        let started = harness.begin("Hello");
        let message_id = started.run.assistant_message.id.clone();
        let events = harness
            .drive(
                &started,
                Scripted::failing(alexandria_ai::Error::Unreachable),
            )
            .await;
        let saved = harness.conn(|c| store::message(c, &message_id).unwrap());
        assert_eq!(
            (saved.status.as_str(), saved.error.as_deref()),
            ("failed", Some("unreachable"))
        );
        let last = events.last().unwrap();
        assert_eq!(
            (last["kind"].as_str(), last["partial"].as_bool()),
            (Some("run_failed"), Some(false))
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stopping_while_connecting_lets_a_sign_in_renewal_finish() {
        let harness = Harness::new("http://127.0.0.1:11434/v1");
        let started = harness.begin("Hello");
        let run_id = started.run.run_id.clone();
        let message_id = started.run.assistant_message.id.clone();
        let renewed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut source = Scripted::answer(&["Never ", "sent."], 5);
        source.connecting = Some((Duration::from_millis(300), renewed.clone()));
        let runtime = harness.runtime.clone();
        let stopper = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert!(runtime.cancel(&run_id));
        });
        let events = harness.drive(&started, source).await;
        stopper.await.unwrap();

        // The renewal (and the save of its rotated token) ran to the end;
        // the stop then ended the run before anything was sent.
        assert!(renewed.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(kinds(&events), ["run_started", "run_cancelled"]);
        let saved = harness.conn(|c| store::message(c, &message_id).unwrap());
        assert_eq!(
            (saved.status.as_str(), saved.content.as_str()),
            ("cancelled", "")
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_assessment_starting_before_a_short_answer_is_saved_stops_it() {
        // Too short and too quick to reach a checkpoint.
        static WORDS: [&str; 2] = ["Short ", "answer."];
        let harness = Harness::new("http://127.0.0.1:11434/v1");
        let started = harness.begin("Explain");
        let message_id = started.run.assistant_message.id.clone();
        let shared = harness.shared.clone();
        let attempt = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(40)).await;
            let guard = shared.lock().unwrap();
            guard
                .as_ref()
                .unwrap()
                .conn()
                .execute("INSERT INTO integrity_sessions(id) VALUES ('attempt')", [])
                .unwrap();
        });
        let events = harness.drive(&started, Scripted::answer(&WORDS, 100)).await;
        attempt.await.unwrap();

        let saved = harness.conn(|c| store::message(c, &message_id).unwrap());
        assert_eq!(saved.status, "cancelled");
        assert_eq!(saved.error.as_deref(), Some("assessment_active"));
        let last = events.last().unwrap();
        assert_eq!(last["kind"], "run_cancelled");
        assert_eq!(last["error"], "assessment_active");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_answer_that_cannot_be_saved_fails_instead_of_completing() {
        static WORDS: [&str; 2] = [
            "Here is one.\n",
            "```practice\n{\"kind\":\"numeric\",\"prompt\":\"What is 1/2 + 1/4?\",\"answer\":\"3/4\"}\n```",
        ];
        let harness = Harness::new("http://127.0.0.1:11434/v1");
        let started = harness.begin_in("practice", "Quiz me");
        let message_id = started.run.assistant_message.id.clone();
        // The question cannot be stored, so the turn's transaction fails.
        harness.conn(|c| {
            c.execute_batch("ALTER TABLE chat_practice_items RENAME TO practice_gone")
                .unwrap()
        });
        let events = harness.drive(&started, Scripted::answer(&WORDS, 5)).await;

        let last = events.last().unwrap();
        assert_eq!(
            (last["kind"].as_str(), last["error"].as_str()),
            (Some("run_failed"), Some("storage_error"))
        );
        assert_eq!(last["partial"], true);
        let saved = harness.conn(|c| store::message(c, &message_id).unwrap());
        assert_eq!(
            (saved.status.as_str(), saved.error.as_deref()),
            ("failed", Some("storage_error"))
        );
        assert!(harness
            .conn(|c| store::active_run(c, "t").unwrap())
            .is_none());
    }

    // ─── over the provider adapter, against a loopback model ────────────

    /// Serves one streamed answer, word by word, pausing between words.
    async fn model(words: &'static [&'static str], pause_ms: u64) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1 << 16];
            let _ = socket.read(&mut buf).await;
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n")
                .await;
            for word in words {
                let delta = serde_json::json!({"choices": [{"delta": {"content": word}}]});
                let frame = format!("data: {delta}\n\n");
                if socket.write_all(frame.as_bytes()).await.is_err() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(pause_ms)).await;
            }
            let _ = socket
                .write_all(b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":40,\"completion_tokens\":3}}\n\ndata: [DONE]\n\n")
                .await;
        });
        endpoint
    }

    /// Reads one HTTP request whole and reports it.
    async fn read_request(socket: &mut tokio::net::TcpStream) -> String {
        let mut request = Vec::new();
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = socket.read(&mut buf).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            request.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&request);
            if let Some(end) = text.find("\r\n\r\n") {
                let length = text[..end]
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&request).into_owned()
    }

    /// Serves one answer over the Anthropic Messages protocol, with a
    /// thinking block first, and reports the raw request.
    async fn anthropic_model(
        words: &'static [&'static str],
    ) -> (String, tokio::sync::oneshot::Receiver<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let (seen, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let _ = seen.send(read_request(&mut socket).await);
            let mut body = String::from(
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":30,\"cache_read_input_tokens\":10}}}\n\n\
                 event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"hidden\"}}\n\n",
            );
            for word in words {
                let delta = serde_json::json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": word}});
                body.push_str(&format!("event: content_block_delta\ndata: {delta}\n\n"));
            }
            body.push_str(
                "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3}}\n\n\
                 event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
            );
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n")
                .await;
            let _ = socket.write_all(body.as_bytes()).await;
        });
        (endpoint, rx)
    }

    /// Serves one answer over the Responses protocol and reports the raw
    /// request.
    async fn responses_model() -> (String, tokio::sync::oneshot::Receiver<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let (seen, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let _ = seen.send(read_request(&mut socket).await);
            let body = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"On \"}\n\n\
                        data: {\"type\":\"response.output_text.delta\",\"delta\":\"plan.\"}\n\n\
                        data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":12,\"output_tokens\":2}}}\n\n";
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n")
                .await;
            let _ = socket.write_all(body.as_bytes()).await;
        });
        (endpoint, rx)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_provider_adapter_reports_an_unreachable_model_without_text() {
        let port = {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            l.local_addr().unwrap().port()
        };
        let harness = Harness::new(&format!("http://127.0.0.1:{port}/v1"));
        let started = harness.begin("Hello");
        let message_id = started.run.assistant_message.id.clone();
        let events = harness.drive_provider(started).await;
        let saved = harness.conn(|c| store::message(c, &message_id).unwrap());
        assert_eq!(
            (saved.status.as_str(), saved.error.as_deref()),
            ("failed", Some("unreachable"))
        );
        let last = events.last().unwrap();
        assert_eq!(
            (last["kind"].as_str(), last["partial"].as_bool()),
            (Some("run_failed"), Some(false))
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_scripted_source_and_the_provider_adapter_emit_the_same_events() {
        static WORDS: [&str; 3] = ["Because ", "it is ", "rare."];
        let endpoint = model(&WORDS, 5).await;
        let harness = Harness::new(&endpoint);
        let over_provider = harness.drive_provider(harness.begin("Why?")).await;
        let started = harness.begin("Why?");
        let over_script = harness.drive(&started, Scripted::answer(&WORDS, 5)).await;

        // Only the run differs; the thread, the order, the numbering, and
        // every body are the same.
        let without_run = |events: &[serde_json::Value]| -> Vec<serde_json::Value> {
            events
                .iter()
                .cloned()
                .map(|mut e| {
                    e.as_object_mut().unwrap().remove("run_id");
                    e
                })
                .collect()
        };
        assert_eq!(without_run(&over_provider), without_run(&over_script));
        assert_eq!(over_provider.len(), 6);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_chatgpt_sign_in_streams_through_the_responses_route_with_its_token() {
        let (endpoint, seen) = responses_model().await;
        let harness = Harness::new(&endpoint);
        harness.conn(|conn| {
            let mut doc = studio_store::connection(conn, "c1").unwrap();
            doc.value.protocol = Protocol::Responses;
            doc.value.auth = alexandria_studio::model::ConnectionAuth::ChatgptPlan;
            let credential = crate::ai_auth::chatgpt::Credential {
                client_id: "oaiapp_9".into(),
                sub: "u".into(),
                email: None,
                host_id: "h".into(),
                access_token: "plan-token".into(),
                refresh_token: "r".into(),
                id_token: "i".into(),
                expires_at: crate::ai_auth::now_secs() + 3000,
                scopes: vec!["chatgpt.tokens.use.direct".into()],
            };
            studio_store::save_signed_in_connection(conn, &doc, &credential.encode()).unwrap();
            // The generic editor cannot swap the sign-in for a key.
            let doc = studio_store::connection(conn, "c1").unwrap();
            assert!(studio_store::save_connection(conn, &doc, Some("sk-x")).is_err());
        });
        let started = harness.begin("Why?");
        let message_id = started.run.assistant_message.id.clone();
        harness.drive_provider(started).await;

        let saved = harness.conn(|c| store::message(c, &message_id).unwrap());
        assert_eq!(saved.status, "complete");
        assert_eq!(saved.content, "On plan.");
        assert_eq!(
            (saved.input_tokens, saved.output_tokens),
            (Some(12), Some(2))
        );
        let raw = seen.await.unwrap();
        let head = raw[..raw.find("\r\n\r\n").unwrap()].to_ascii_lowercase();
        assert!(head.starts_with("post /v1/responses "));
        assert!(head.contains("authorization: bearer plan-token"));
        let body: serde_json::Value =
            serde_json::from_str(&raw[raw.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert_eq!(body["store"], false);
        assert!(body["instructions"]
            .as_str()
            .unwrap()
            .contains("study assistant"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_anthropic_connection_streams_text_and_lifts_the_instructions() {
        static WORDS: [&str; 2] = ["Rare ", "events."];
        let (endpoint, seen) = anthropic_model(&WORDS).await;
        let harness = Harness::new(&endpoint);
        harness.conn(|conn| {
            let mut doc = studio_store::connection(conn, "c1").unwrap();
            doc.value.protocol = Protocol::Anthropic;
            studio_store::save_connection(conn, &doc, Some("sk-ant-test")).unwrap();
        });
        let started = harness.begin("Why?");
        let message_id = started.run.assistant_message.id.clone();
        harness.drive_provider(started).await;

        let saved = harness.conn(|c| store::message(c, &message_id).unwrap());
        assert_eq!(saved.status, "complete");
        assert_eq!(saved.content, "Rare events.");
        assert_eq!(
            (saved.input_tokens, saved.output_tokens),
            (Some(40), Some(3))
        );
        let raw = seen.await.unwrap();
        let head = raw[..raw.find("\r\n\r\n").unwrap()].to_ascii_lowercase();
        assert!(head.starts_with("post /v1/messages "));
        assert!(head.contains("x-api-key: sk-ant-test"));
        let body: serde_json::Value =
            serde_json::from_str(&raw[raw.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert!(
            body["system"]
                .as_str()
                .unwrap()
                .contains("<<<SOURCE id=\"S1\""),
            "instructions and sources are the system prompt"
        );
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.last().unwrap()["role"], "user");
        assert!(messages.iter().all(|m| m["role"] != "system"));
    }
}
