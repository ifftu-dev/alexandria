//! The call manager: one implementation of a call for every platform.
//!
//! A call is a `live` room (gossip peer discovery plus MoQ media), the local
//! broadcast published into it, the remote broadcasts watched and listened
//! to, a chat and transcript topic, a `/names` topic for display names, the
//! frame bridge that turns decoded video into webview events, the session
//! row in the profile's database, and the controls (camera, mic, screen,
//! audio route) the learner has over it. All of that is the same on a Mac,
//! an iPhone and an Android phone and lives here once.
//!
//! What differs per platform sits behind [`MediaDevices`]: which camera and
//! screen open, which encoder and decoder run, how audio opens and routes,
//! and which events the device raises on its own. The manager is generic
//! over that adapter and is constructed once at app start with the
//! platform's; tests construct it with the synthetic one.
//!
//! Locking: `inner` guards the active session. It is never held across a
//! device open, a database write or a cleanup wait, so every call command
//! keeps answering while a camera takes its time (14 Oct) and leaving never
//! waits on stuck media work (commit 555535c7).

use std::collections::{HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use iroh::{Endpoint, EndpointId};
use iroh_gossip::net::Gossip;
#[cfg(tutoring_video)]
use live::media::av::DecodeConfig;
use live::media::av::{AudioPreset, AudioSinkHandle};
use live::media::opus::{PureOpusDecoder, PureOpusEncoder};
#[cfg(tutoring_video)]
use live::media::publish::VideoRenditions;
use live::media::publish::{AudioRenditions, PublishBroadcast};
use live::media::subscribe::SubscribeBroadcast;
#[cfg(tutoring_video)]
use live::media::subscribe::WatchTrack;
use live::moq::MoqSession;
use live::rooms::{Room, RoomEvent, RoomHandle, RoomTicket};
use live::Live;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, Mutex};

use super::call_kind::{self, CallKind};
use super::events::{emit, CallEvents};
pub use super::media::DeviceCheckResult;
#[cfg(not(tutoring_video))]
use super::media::VIDEO_DISABLED_ERROR;
use super::media::{
    AudioDeviceInfo, AudioSession, CallAudioInput, CallAudioOutput, CameraDeviceInfo,
    DeviceSelection, MediaDevices, MediaEvent, MediaProfile,
};
#[cfg(tutoring_video)]
use super::media::{FrameTuning, OpenedVideo};
use super::records::SessionRecords;
use super::tasks::{SessionTasks, TaskHandle};

// ── Constants ──────────────────────────────────────────────────────

/// Maximum chat message text length (bytes).
const CHAT_MAX_LENGTH: usize = 2000;
/// Minimum interval between chat messages from the local user (ms).
const CHAT_RATE_LIMIT_MS: u64 = 200;
const TRANSCRIPT_PREFIX: &[u8] = b"ALXTR1";
/// Interval for re-broadcasting our display name (seconds).
const NAME_BROADCAST_INTERVAL_SECS: u64 = 15;
/// Name used for our broadcast in every room.
const BROADCAST_NAME: &str = "cam";
/// Maximum number of log entries retained in the ring buffer.
const MAX_LOG_ENTRIES: usize = 50;
/// How long a camera or screen may take to open mid-call, on every
/// platform. The start of a call follows the platform's
/// [`MediaProfile::start_device_timeout`] instead.
#[cfg(tutoring_video)]
const SOURCE_OPEN_TIMEOUT: Duration = Duration::from_secs(10);
/// How long leaving waits for media cleanup before letting it finish in the
/// background.
const LEAVE_CLEANUP_GRACE: Duration = Duration::from_secs(3);

// ── Wire protocol (gossip) ─────────────────────────────────────────

/// Chat message sent over the gossip channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Sender's iroh node ID (hex).
    pub sender: String,
    /// Display name (optional, from session title or profile).
    pub sender_name: Option<String>,
    /// Message text.
    pub text: String,
    /// Unix timestamp (millis since epoch).
    pub timestamp: u64,
}

/// A participant-controlled, on-device transcript segment. It shares the
/// encrypted room gossip channel with chat but carries a prefix so older chat
/// decoders never mistake transcript text for a chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TranscriptMessage {
    sender: String,
    sender_name: Option<String>,
    text: String,
    confidence: Option<f64>,
    timestamp: u64,
}

/// A display name announcement broadcast over the `/names` gossip topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct NameAnnouncement {
    node_id: String,
    display_name: String,
}

// ── Webview events ─────────────────────────────────────────────────

/// `tutoring:video-frame`
#[cfg(tutoring_video)]
#[derive(Debug, Clone, Serialize)]
struct VideoFrameEvent {
    /// Node ID of the peer (or "self" for local preview).
    node_id: String,
    /// Base64-encoded JPEG image data.
    jpeg_b64: String,
    width: u32,
    height: u32,
}

/// `tutoring:chat`
#[derive(Debug, Clone, Serialize)]
struct ChatMessageEvent {
    sender: String,
    sender_name: Option<String>,
    text: String,
    timestamp: u64,
}

/// `tutoring:transcript`
#[derive(Debug, Clone, Serialize)]
struct TranscriptMessageEvent {
    sender: String,
    sender_name: Option<String>,
    text: String,
    confidence: Option<f64>,
    timestamp: u64,
}

/// `tutoring:peer-video-ended`
#[cfg(tutoring_video)]
#[derive(Debug, Clone, Serialize)]
struct PeerVideoEndedEvent {
    node_id: String,
}

/// `tutoring:peer-name`
#[derive(Debug, Clone, Serialize)]
struct PeerNameEvent {
    node_id: String,
    display_name: String,
}

/// `tutoring:audio-level`
#[derive(Debug, Clone, Serialize)]
struct AudioLevelEvent {
    /// Mic input level 0.0–1.0.
    mic_level: f32,
    /// Output level 0.0–1.0 (if available from output stream).
    output_level: f32,
}

// ── Public types ───────────────────────────────────────────────────

/// Peer info as seen from room gossip events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TutoringPeer {
    pub node_id: String,
    pub display_name: Option<String>,
    pub broadcasts: Vec<String>,
    pub connected: bool,
}

/// Status of the active session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatus {
    pub session_id: String,
    pub session_title: String,
    pub ticket: String,
    pub peers: Vec<TutoringPeer>,
    pub video_enabled: bool,
    pub audio_enabled: bool,
    pub screen_sharing: bool,
    /// The camera peers are seeing, by the system's name for it. `None` when
    /// the camera is off, the screen is shared instead, or the platform does
    /// not name its camera.
    pub camera_name: Option<String>,
    /// Session start time (millis since Unix epoch).
    pub started_at: u64,
}

/// Diagnostic info for debugging the A/V pipeline.
#[derive(Debug, Clone, Serialize)]
pub struct SessionDiagnostics {
    pub session_id: String,
    pub our_node_id: String,
    pub video_enabled: bool,
    pub audio_enabled: bool,
    pub has_audio_ctx: bool,
    pub has_mic_input: bool,
    pub has_output_stream: bool,
    pub has_self_preview: bool,
    pub peer_count: usize,
    pub peers: Vec<PeerDiagnostics>,
    pub task_count: usize,
    /// Recent log entries from the tutoring subsystem (ring buffer), followed
    /// by the app's diag.log.
    pub recent_logs: Vec<String>,
    /// Home relay URL (if connected).
    pub home_relay: Option<String>,
}

/// Per-peer diagnostic info.
#[derive(Debug, Clone, Serialize)]
pub struct PeerDiagnostics {
    pub node_id: String,
    pub display_name: Option<String>,
    pub broadcasts: Vec<String>,
    pub connected: bool,
}

/// Available devices for audio and video.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceList {
    pub audio_inputs: Vec<AudioDeviceInfo>,
    pub audio_outputs: Vec<AudioDeviceInfo>,
    pub cameras: Vec<CameraDeviceInfo>,
    pub selected_audio_input: Option<String>,
    pub selected_audio_output: Option<String>,
}

/// What a learner asks for when creating or joining a call.
pub struct RoomRequest {
    pub session_id: String,
    pub title: String,
    pub display_name: String,
    pub kind: CallKind,
    /// The classroom, interview or plugin element the call belongs to.
    pub context_id: Option<String>,
    pub devices: DeviceSelection,
    /// A classroom call being started: its `classroom_calls` row is written
    /// in the same transaction as this call's session row, so neither exists
    /// without the other. A failed write stops the call.
    pub classroom_call: Option<call_kind::ClassroomCallRow>,
}

/// The iroh node a call rides on; every handle is a cheap clone.
pub struct Network {
    pub endpoint: Endpoint,
    pub gossip: Gossip,
    pub live: Live,
}

// ── Internal state ─────────────────────────────────────────────────

type Input<M> = <<M as MediaDevices>::Audio as AudioSession>::Input;
type Output<M> = <<M as MediaDevices>::Audio as AudioSession>::Output;
type Shared<M> = Arc<Mutex<Option<ActiveSession<M>>>>;

/// Internal state for an active room.
pub(super) struct ActiveSession<M: MediaDevices> {
    /// Cloud panel presence for this exact room, when it is a panel
    /// interview; see [`CallManager::attach_panel_presence`].
    panel_presence: Option<TaskHandle>,
    session_id: String,
    /// Which start of the platform media this session owns; see
    /// [`Core::generation`].
    generation: u64,
    session_title: String,
    handle: RoomHandle,
    broadcast: PublishBroadcast,
    audio: Option<M::Audio>,
    /// Clone of the mic stream — kept for peak metering (VU meter).
    mic_input: Option<Input<M>>,
    /// Clone of the output stream — kept for output peak metering.
    output_stream: Option<Output<M>>,
    /// Stable output streams for remote broadcasts keyed by `node_id:broadcast`.
    ///
    /// Reusing the same sink across resubscribe events avoids repeatedly
    /// mutating the audio graph while a call is already in progress.
    remote_output_streams: HashMap<String, Output<M>>,
    /// MoQ sessions for subscribed broadcasts — kept alive to prevent QUIC close.
    _moq_sessions: Vec<MoqSession>,
    peers: HashMap<String, TutoringPeer>,
    video_enabled: bool,
    audio_enabled: bool,
    screen_sharing: bool,
    camera_name: Option<String>,
    /// Chat sender for the derived gossip topic.
    chat_sender: Option<iroh_gossip::api::GossipSender>,
    /// Names sender for the /names gossip topic. Kept alongside `chat_sender`
    /// so the event loop can seed both topics with peer node IDs as the MoQ
    /// room discovers them (without this, gossip subscriptions start with an
    /// empty neighbour set and messages never propagate).
    names_sender: Option<iroh_gossip::api::GossipSender>,
    our_node_id: String,
    our_display_name: String,
    /// Session start time (millis since epoch).
    started_at: u64,
    /// Timestamp of last sent chat message (for rate limiting).
    last_chat_sent: Instant,
    /// Handle for the self-preview task (aborted on toggle).
    self_preview_task: Option<TaskHandle>,
    /// User's selected devices — kept for toggles and route changes.
    devices: DeviceSelection,
    /// The camera was on and the app took it away for the background; only
    /// such a camera comes back in the foreground, never one the learner
    /// turned off.
    #[cfg(tutoring_video)]
    camera_released_for_background: bool,
    /// Bumped on every trip to the background, so a foreground restore that
    /// finishes after the app left again does not reopen the camera.
    #[cfg(tutoring_video)]
    background_epoch: u64,
    /// Background tasks to abort on leave.
    _tasks: Vec<TaskHandle>,
    /// Ring buffer of recent log entries for diagnostics.
    recent_logs: Vec<String>,
    /// Home relay URL at time of session creation.
    home_relay: Option<String>,
    /// SubscribeBroadcast handles — kept alive so the BroadcastConsumer doesn't
    /// drop and close the MoQ track subscriptions (audio/video).
    _subscribe_broadcasts: Vec<SubscribeBroadcast>,
    _repair_broadcast_keys: HashSet<String>,
    _subscribed_audio_keys: HashSet<String>,
    _subscribed_video_keys: HashSet<String>,
}

impl<M: MediaDevices> ActiveSession<M> {
    fn push_log(&mut self, msg: String) {
        if self.recent_logs.len() >= MAX_LOG_ENTRIES {
            self.recent_logs.remove(0);
        }
        self.recent_logs.push(msg);
    }

    fn upsert_peer(&mut self, node_id: &str, update: impl FnOnce(&mut TutoringPeer)) {
        let peer = self
            .peers
            .entry(node_id.to_string())
            .or_insert_with(|| TutoringPeer {
                node_id: node_id.to_string(),
                display_name: None,
                broadcasts: vec![],
                connected: false,
            });
        update(peer);
    }

    /// Every output we may be playing through, for a resume after a route
    /// change.
    fn outputs(&self) -> Vec<Output<M>> {
        let mut outputs: Vec<_> = self.remote_output_streams.values().cloned().collect();
        if let Some(output) = self.output_stream.clone() {
            outputs.push(output);
        }
        outputs
    }
}

/// The handles every background task of a call shares.
struct Core<M: MediaDevices> {
    media: Arc<M>,
    profile: MediaProfile,
    events: Arc<dyn CallEvents>,
    tasks: Arc<SessionTasks>,
    inner: Shared<M>,
    /// Bumped, under `inner`, each time a session starts the platform
    /// media. Leaving ends the media only if no later session has started
    /// it again, so a slow leave never ends the next call's media.
    generation: Arc<AtomicU64>,
}

impl<M: MediaDevices> Clone for Core<M> {
    fn clone(&self) -> Self {
        Self {
            media: self.media.clone(),
            profile: self.profile.clone(),
            events: self.events.clone(),
            tasks: self.tasks.clone(),
            inner: self.inner.clone(),
            generation: self.generation.clone(),
        }
    }
}

// ── CallManager ────────────────────────────────────────────────────

/// Manages live calls over one platform's media.
///
/// Thread-safe; stored in Tauri `AppState` as `Arc<TutoringManager>`.
pub struct CallManager<M: MediaDevices> {
    core: Core<M>,
}

impl<M: MediaDevices> CallManager<M> {
    pub fn new(media: M, events: impl CallEvents) -> Self {
        let profile = media.profile();
        Self {
            core: Core {
                media: Arc::new(media),
                profile,
                events: Arc::new(events),
                tasks: Arc::new(SessionTasks::default()),
                inner: Arc::new(Mutex::new(None)),
                generation: Arc::new(AtomicU64::new(0)),
            },
        }
    }

    /// The platform's media, for callers that need the device list.
    pub fn media(&self) -> &M {
        &self.core.media
    }

    #[cfg(test)]
    pub(super) fn tasks(&self) -> &Arc<SessionTasks> {
        &self.core.tasks
    }

    #[cfg(test)]
    pub(super) async fn has_session(&self) -> bool {
        self.core.inner.lock().await.is_some()
    }

    fn now_millis() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    // ── Room lifecycle ─────────────────────────────────────────────

    /// Create a new room (host mode). Returns its ticket.
    pub async fn create_room(
        &self,
        request: RoomRequest,
        network: Network,
        records: &impl SessionRecords,
    ) -> Result<String, String> {
        self.start_room(request, None, network, records).await
    }

    /// Refuse a room this device already knows under another kind. The
    /// command runs this before any waiting on the network; [`Self::join_room`]
    /// runs it again as its own guard.
    pub async fn check_join_kind(
        &self,
        records: &impl SessionRecords,
        ticket: &str,
        kind: CallKind,
    ) -> Result<(), String> {
        let ticket = ticket.to_string();
        records
            .run("tutoring.join.check-kind", move |db| {
                call_kind::check_join_kind(db.conn(), &ticket, kind)
            })
            .await
    }

    /// Join an existing room using a ticket string. Returns the resolved
    /// ticket.
    pub async fn join_room(
        &self,
        request: RoomRequest,
        ticket: &str,
        network: Network,
        records: &impl SessionRecords,
    ) -> Result<String, String> {
        self.check_join_kind(records, ticket, request.kind).await?;
        self.start_room(request, Some(ticket), network, records)
            .await
    }

    async fn start_room(
        &self,
        request: RoomRequest,
        join_ticket: Option<&str>,
        network: Network,
        records: &impl SessionRecords,
    ) -> Result<String, String> {
        let core = &self.core;
        let verb = if join_ticket.is_some() {
            "join_room"
        } else {
            "create_room"
        };
        crate::diag::log(&format!("{verb}: starting"));
        let mut inner = core.inner.lock().await;
        if inner.is_some() {
            return Err("already in a tutoring session".into());
        }
        core.tasks.shutdown().await;
        core.tasks.open()?;
        let generation = core.generation.fetch_add(1, Ordering::SeqCst) + 1;
        core.media.session_started();
        let mut startup = core.tasks.startup_guard();
        let RoomRequest {
            session_id,
            title,
            display_name,
            kind,
            context_id,
            devices,
            classroom_call,
        } = request;
        let result = async {
            let endpoint = &network.endpoint;
            let our_node_id = endpoint.id().to_string();
            let home_relay = endpoint.addr().relay_urls().next().map(|u| u.to_string());

            let ticket = match join_ticket {
                Some(ticket) => ticket
                    .parse::<RoomTicket>()
                    .map_err(|e| format!("invalid room ticket: {e}"))?,
                None => RoomTicket::generate(),
            };
            let room = within(
                core.profile.room_open_timeout,
                Room::new(endpoint, network.gossip.clone(), network.live, ticket),
            )
            .await
            .map_err(|limit| format!("Room::new timed out after {}s", limit.as_secs()))?
            .map_err(|e| format!("failed to {} room: {e}", verb.trim_end_matches("_room")))?;
            let ticket_str = room.ticket().to_string();
            crate::diag::log(&format!("{verb}: room open"));

            let audio = core.open_audio(&devices).await;
            let has_audio = audio.is_some();
            if !has_audio {
                log::warn!("tutoring: proceeding without audio (audio engine failed to start)");
            }

            let (mut broadcast, mic_input, camera_name, has_video) = core
                .create_broadcast(audio.as_ref(), devices.camera_index.clone())
                .await?;
            room.publish(BROADCAST_NAME, broadcast.producer())
                .await
                .map_err(|e| format!("failed to publish broadcast: {e}"))?;
            crate::diag::log(&format!(
                "{verb}: published audio={has_audio} video={has_video}"
            ));

            let (events, handle) = room.split();
            let self_preview_task = core.start_self_preview(&mut broadcast);

            // Every participant derives the chat and names topics from the
            // room's gossip topic. The ticket string is not shared: each
            // device's copy names itself as the bootstrap peer.
            let topic_seed = handle.ticket().topic_id.as_bytes().to_vec();
            let chat_sender = core
                .setup_chat(&network.gossip, &topic_seed, &our_node_id)
                .await;
            let (names_sender, names_task) = match core
                .setup_names(&network.gossip, &topic_seed, &our_node_id, &display_name)
                .await
            {
                Some((s, t)) => (Some(s), Some(t)),
                None => (None, None),
            };

            let event_task = {
                let core = core.clone();
                let audio = audio.clone();
                core.tasks
                    .clone()
                    .spawn(async move { core.event_loop(events, audio).await })
            };
            let mut session_tasks = vec![event_task];
            session_tasks.extend(names_task);
            let session_output = core.create_session_output(&audio).await;

            let init_logs = vec![
                format!("{verb}: audio={has_audio}, video={has_video}"),
                format!("home_relay={}", home_relay.as_deref().unwrap_or("none")),
            ];

            *inner = Some(ActiveSession {
                panel_presence: None,
                session_id: session_id.clone(),
                generation,
                session_title: title.clone(),
                handle,
                broadcast,
                audio,
                mic_input,
                output_stream: session_output,
                remote_output_streams: HashMap::new(),
                _moq_sessions: Vec::new(),
                peers: HashMap::new(),
                video_enabled: has_video,
                audio_enabled: has_audio,
                screen_sharing: false,
                camera_name,
                chat_sender,
                names_sender,
                our_node_id,
                our_display_name: display_name,
                started_at: Self::now_millis(),
                last_chat_sent: Instant::now() - Duration::from_secs(10),
                self_preview_task,
                devices,
                #[cfg(tutoring_video)]
                camera_released_for_background: false,
                #[cfg(tutoring_video)]
                background_epoch: 0,
                _tasks: session_tasks,
                recent_logs: init_logs,
                home_relay,
                _subscribe_broadcasts: Vec::new(),
                _repair_broadcast_keys: HashSet::new(),
                _subscribed_audio_keys: HashSet::new(),
                _subscribed_video_keys: HashSet::new(),
            });

            // Tasks that read the stored session start after it is stored.
            let audio_level_task = core.start_audio_level_emitter();
            let media_event_task = core.start_media_event_listener();
            if let Some(session) = inner.as_mut() {
                session._tasks.push(audio_level_task);
                session._tasks.push(media_event_task);
            }

            log::info!(
                "tutoring: room {} — audio={has_audio}, video={has_video}, ticket={}...",
                if join_ticket.is_some() {
                    "joined"
                } else {
                    "created"
                },
                &ticket_str[..ticket_str.len().min(20)]
            );
            Ok(ticket_str)
        }
        .await;
        let ticket_str = match result {
            Ok(ticket_str) => {
                startup.commit();
                ticket_str
            }
            Err(error) => {
                drop(startup);
                core.tasks.shutdown().await;
                core.media.session_ended();
                return Err(error);
            }
        };
        drop(inner);

        // The row is written once the media and network work has completed,
        // with the lock released. A call without its row would be invisible
        // to presence and to the session list, so it is not left running.
        let record = {
            let (session_id, title, ticket_str) =
                (session_id.clone(), title.clone(), ticket_str.clone());
            records.run(
                if join_ticket.is_some() {
                    "tutoring.join.persist-session"
                } else {
                    "tutoring.create.persist-session"
                },
                move |db| {
                    let tx = rusqlite::Transaction::new_unchecked(
                        db.conn(),
                        rusqlite::TransactionBehavior::Immediate,
                    )
                    .map_err(|e| e.to_string())?;
                    call_kind::record_session(
                        &tx,
                        &session_id,
                        &title,
                        &ticket_str,
                        kind,
                        context_id.as_deref(),
                    )?;
                    super::presence::register(&tx, &session_id, &ticket_str)?;
                    if let Some(row) = &classroom_call {
                        call_kind::record_classroom_call(
                            &tx,
                            &session_id,
                            context_id.as_deref(),
                            &title,
                            &ticket_str,
                            row,
                        )?;
                    }
                    tx.commit().map_err(|e| e.to_string())
                },
            )
        }
        .await;
        if let Err(error) = record {
            log::error!("tutoring: session {session_id} could not be recorded: {error}");
            self.stop_room(false, Some(&session_id)).await?;
            return Err(error);
        }
        Ok(ticket_str)
    }

    /// Attach Cloud presence to this exact connected runtime. Page navigation
    /// does not own this task; room shutdown and profile admission do.
    pub(crate) async fn attach_panel_presence(
        &self,
        room_session_id: &str,
        link: crate::interview_panel::PanelLink,
        publication: super::panel_presence::Publication,
        lease: crate::profile::scope::ProfileLease,
        endpoint: Endpoint,
    ) -> Result<serde_json::Value, String> {
        let receive = {
            let mut inner = self.core.inner.lock().await;
            let session = inner
                .as_mut()
                .filter(|s| s.session_id == room_session_id)
                .ok_or("the connected panel room has changed")?;
            super::panel_presence::attach(
                &self.core.tasks,
                &session.handle,
                &mut session.panel_presence,
                endpoint,
                lease,
                link,
                publication,
            )?
        };
        receive
            .await
            .map_err(|_| "the panel room stopped before admission completed".to_string())?
    }

    /// Leave the current room and note when it was last occupied.
    pub async fn leave_room(&self, records: &impl SessionRecords) -> Result<(), String> {
        let session_id = self.status().await.map(|s| s.session_id);
        self.stop_room(true, None).await?;
        if let Some(id) = session_id {
            records
                .run("tutoring.leave.persist-session", move |db| {
                    db.conn()
                        .execute(
                            "UPDATE tutoring_sessions SET last_occupied_at = unixepoch() WHERE id = ?1",
                            rusqlite::params![id],
                        )
                        .map_err(|e| e.to_string())?;
                    Ok(())
                })
                .await?;
        }
        Ok(())
    }

    /// Stop whatever is running, on profile lock or app exit.
    pub async fn shutdown(&self) -> Result<(), String> {
        self.stop_room(false, None).await
    }

    /// Leave the room without letting a stuck media task hold leaving hostage.
    ///
    /// The session leaves the lock first, so every other call command keeps
    /// answering. Cleanup then gets a few seconds; a task that cannot be
    /// aborted (one blocked in native code, which only yields at its next
    /// await) finishes in the background. Starting another room is still
    /// refused until it does, because [`SessionTasks::open`] waits for an
    /// empty registry: no second camera opens on top of the first.
    ///
    /// `only` stops the room only if it is still that session, for a start
    /// that failed after releasing the lock.
    async fn stop_room(&self, require_active: bool, only: Option<&str>) -> Result<(), String> {
        let core = &self.core;
        crate::diag::log("leave: waiting for the session lock");
        let session = {
            let mut inner = core.inner.lock().await;
            if let Some(only) = only {
                if inner.as_ref().map(|s| s.session_id.as_str()) != Some(only) {
                    return Ok(());
                }
            }
            if require_active && inner.is_none() {
                return Err("not in a tutoring session".into());
            }
            inner.take()
        };
        let ended = session.as_ref().map(|s| s.generation);
        crate::diag::log("leave: session taken, stopping media");
        if let Some(session) = session.as_ref() {
            if let Some(preview) = &session.self_preview_task {
                preview.abort();
            }
            for task in &session._tasks {
                task.abort();
            }
        }
        // Stop admitting work and abort what can be aborted now, so nothing
        // new starts even if the cleanup below has to finish later.
        core.tasks.close();
        let tasks = core.tasks.clone();
        let mut cleanup = tokio::spawn(async move {
            tasks.shutdown().await;
            drop(session);
        });
        if tokio::time::timeout(LEAVE_CLEANUP_GRACE, &mut cleanup)
            .await
            .is_err()
        {
            log::warn!(
                "tutoring: media cleanup still running {}s after leaving; finishing in the background",
                LEAVE_CLEANUP_GRACE.as_secs()
            );
            crate::diag::log("leave: media cleanup still running, continuing");
        } else {
            crate::diag::log("leave: media stopped");
        }
        // End the platform media only if it is still this session's: a call
        // started while cleanup ran owns it now.
        if let Some(generation) = ended {
            let _inner = core.inner.lock().await;
            if core.generation.load(Ordering::SeqCst) == generation {
                core.media.session_ended();
            } else {
                crate::diag::log("leave: a newer call owns the media; leaving it running");
            }
        }
        Ok(())
    }

    // ── Media controls ─────────────────────────────────────────────

    /// Toggle local camera on/off.
    ///
    /// Turning it on opens the camera with the call's lock released, then
    /// swaps it in only if the session is still the same one and the camera
    /// is still wanted.
    #[cfg(tutoring_video)]
    pub async fn toggle_video(&self, enable: bool) -> Result<bool, String> {
        let core = &self.core;
        let (session_id, camera) = {
            let mut inner = core.inner.lock().await;
            let session = core.active(&mut inner)?;
            // The learner's choice replaces whatever the app did for the
            // background.
            session.camera_released_for_background = false;
            if enable == session.video_enabled && !session.screen_sharing {
                return Ok(session.video_enabled);
            }
            if !enable {
                core.remove_video(session)?;
                return Ok(session.video_enabled);
            }
            (
                session.session_id.clone(),
                session.devices.camera_index.clone(),
            )
        };

        let opened = core
            .open_video_detached(false, camera)
            .await
            .inspect_err(|e| log::warn!("tutoring: {e}"))?;

        let mut inner = core.inner.lock().await;
        let session = core.same_session(&mut inner, &session_id)?;
        if session.video_enabled && !session.screen_sharing {
            // Another change turned the camera on meanwhile; keep that one.
            return Ok(true);
        }
        core.install_video(session, opened, false)?;
        Ok(session.video_enabled)
    }

    #[cfg(not(tutoring_video))]
    pub async fn toggle_video(&self, enable: bool) -> Result<bool, String> {
        let mut inner = self.core.inner.lock().await;
        let session = self.core.active(&mut inner)?;
        if !enable {
            return Ok(session.video_enabled);
        }
        Err(VIDEO_DISABLED_ERROR.into())
    }

    /// Toggle local microphone on/off.
    pub async fn toggle_audio(&self, enable: bool) -> Result<bool, String> {
        let mut inner = self.core.inner.lock().await;
        let session = self.core.active(&mut inner)?;

        if enable == session.audio_enabled {
            return Ok(session.audio_enabled);
        }

        if enable {
            if session.mic_input.is_some() {
                session.broadcast.set_audio_muted(false);
                session.audio_enabled = true;
            } else if let Some(audio) = session.audio.clone() {
                let mic = audio
                    .input()
                    .await
                    .map_err(|e| format!("microphone unavailable: {e}"))?;
                session.mic_input = Some(mic.clone());
                let renditions = AudioRenditions::new::<PureOpusEncoder>(mic, [AudioPreset::Hq]);
                session
                    .broadcast
                    .set_audio(Some(renditions))
                    .map_err(|e| format!("failed to enable audio: {e}"))?;
                session.broadcast.set_audio_muted(false);
                session.audio_enabled = true;
            } else {
                return Err("audio backend not available".into());
            }
        } else {
            session.broadcast.set_audio_muted(true);
            session.audio_enabled = false;
        }

        Ok(session.audio_enabled)
    }

    /// Use a different camera. Remembered for the rest of the session; when
    /// the camera is on (and the screen is not being shared) it switches now.
    #[cfg(tutoring_video)]
    pub async fn set_camera(&self, camera_index: Option<String>) -> Result<Option<String>, String> {
        let core = &self.core;
        if !core.profile.camera_switch {
            return Err("switching cameras is not available on this device".into());
        }
        let (session_id, camera) = {
            let mut inner = core.inner.lock().await;
            let session = core.active(&mut inner)?;
            session.devices.camera_index = camera_index;
            if !session.video_enabled || session.screen_sharing {
                return Ok(session.camera_name.clone());
            }
            (
                session.session_id.clone(),
                session.devices.camera_index.clone(),
            )
        };

        let opened = core.open_video_detached(false, camera).await?;

        let mut inner = core.inner.lock().await;
        let session = core.same_session(&mut inner, &session_id)?;
        if !session.video_enabled || session.screen_sharing {
            // The camera was turned off or the screen shared meanwhile; the
            // choice is remembered for the next time the camera turns on.
            return Ok(session.camera_name.clone());
        }
        core.install_video(session, opened, false)?;
        Ok(session.camera_name.clone())
    }

    #[cfg(not(tutoring_video))]
    pub async fn set_camera(
        &self,
        _camera_index: Option<String>,
    ) -> Result<Option<String>, String> {
        Err(VIDEO_DISABLED_ERROR.into())
    }

    /// Toggle screen sharing on/off.
    ///
    /// Screen sharing replaces the camera video track. When disabled,
    /// the camera is restored if it was previously enabled.
    #[cfg(tutoring_video)]
    pub async fn toggle_screen_share(&self, enable: bool) -> Result<bool, String> {
        let core = &self.core;
        let (session_id, camera) = {
            let mut inner = core.inner.lock().await;
            let session = core.active(&mut inner)?;
            session.camera_released_for_background = false;
            if enable == session.screen_sharing {
                return Ok(session.screen_sharing);
            }
            (
                session.session_id.clone(),
                session.devices.camera_index.clone(),
            )
        };

        // The screen (to share) or the camera (to restore) opens with the
        // lock released; sharing continues until the camera is ready.
        let opened = if enable {
            core.open_video_detached(true, None).await?
        } else {
            match core.open_video_detached(false, camera).await {
                Ok(opened) => opened,
                Err(e) => {
                    log::warn!("tutoring: {e}; screen share stopped without a camera");
                    let mut inner = core.inner.lock().await;
                    let session = core.same_session(&mut inner, &session_id)?;
                    if session.screen_sharing {
                        core.remove_video(session)?;
                    }
                    return Ok(session.screen_sharing);
                }
            }
        };

        let mut inner = core.inner.lock().await;
        let session = core.same_session(&mut inner, &session_id)?;
        if session.screen_sharing == enable {
            // Another change got there first; keep it.
            return Ok(session.screen_sharing);
        }
        core.install_video(session, opened, enable)?;
        Ok(session.screen_sharing)
    }

    #[cfg(not(tutoring_video))]
    pub async fn toggle_screen_share(&self, enable: bool) -> Result<bool, String> {
        let mut inner = self.core.inner.lock().await;
        let session = self.core.active(&mut inner)?;
        if !enable {
            return Ok(session.screen_sharing);
        }
        Err(VIDEO_DISABLED_ERROR.into())
    }

    /// Move the call's audio to other devices or another route, where the
    /// platform allows it mid-call.
    pub async fn set_audio_devices(
        &self,
        mic_device_id: Option<String>,
        speaker_device_id: Option<String>,
    ) -> Result<(), String> {
        let core = &self.core;
        if !core.profile.audio_route_switch {
            return Err("Changing audio devices during a live tutoring session is currently supported on iPhone and iPad only".into());
        }
        let selection = {
            let mut inner = core.inner.lock().await;
            let session = core.active(&mut inner)?;
            session.devices.mic_device_id = mic_device_id;
            session.devices.speaker_device_id = speaker_device_id;
            session.devices.clone()
        };
        let media = core.media.clone();
        tokio::task::spawn_blocking(move || media.set_audio_route(&selection))
            .await
            .map_err(|e| format!("audio route change failed: {e}"))??;
        core.resume_outputs().await;
        Ok(())
    }

    // ── Chat ───────────────────────────────────────────────────────

    /// Send a text chat message to all peers in the room.
    ///
    /// Enforces a maximum message length and a minimum interval between
    /// sends to prevent accidental spam.
    pub async fn send_chat(&self, text: String) -> Result<(), String> {
        if text.len() > CHAT_MAX_LENGTH {
            return Err(format!(
                "message too long ({} bytes, max {CHAT_MAX_LENGTH})",
                text.len()
            ));
        }

        let mut inner = self.core.inner.lock().await;
        let session = self.core.active(&mut inner)?;

        let now = Instant::now();
        let elapsed = now.duration_since(session.last_chat_sent).as_millis() as u64;
        if elapsed < CHAT_RATE_LIMIT_MS {
            return Err("sending too fast, please wait".into());
        }
        session.last_chat_sent = now;

        let sender = session.chat_sender.as_ref().ok_or("chat not available")?;
        let msg = ChatMessage {
            sender: session.our_node_id.clone(),
            sender_name: Some(session.our_display_name.clone()),
            text,
            timestamp: Self::now_millis(),
        };
        let encoded =
            postcard::to_stdvec(&msg).map_err(|e| format!("failed to encode chat: {e}"))?;
        sender
            .broadcast(Bytes::from(encoded))
            .await
            .map_err(|e| format!("failed to send chat: {e}"))?;
        Ok(())
    }

    /// Broadcast a final transcript segment produced locally by a participant
    /// who explicitly enabled room captions.
    pub async fn send_transcript(
        &self,
        text: String,
        confidence: Option<f64>,
    ) -> Result<(), String> {
        if text.trim().is_empty() {
            return Err("transcript text is required".into());
        }
        if text.len() > CHAT_MAX_LENGTH {
            return Err(format!(
                "transcript too long ({} bytes, max {CHAT_MAX_LENGTH})",
                text.len()
            ));
        }
        let inner = self.core.inner.lock().await;
        let session = inner.as_ref().ok_or("not in a tutoring session")?;
        let sender = session.chat_sender.as_ref().ok_or("chat not available")?;
        let message = TranscriptMessage {
            sender: session.our_node_id.clone(),
            sender_name: Some(session.our_display_name.clone()),
            text,
            confidence: confidence.map(|value| value.clamp(0.0, 1.0)),
            timestamp: Self::now_millis(),
        };
        let payload = postcard::to_stdvec(&message)
            .map_err(|e| format!("failed to encode transcript: {e}"))?;
        let mut encoded = Vec::with_capacity(TRANSCRIPT_PREFIX.len() + payload.len());
        encoded.extend_from_slice(TRANSCRIPT_PREFIX);
        encoded.extend_from_slice(&payload);
        sender
            .broadcast(Bytes::from(encoded))
            .await
            .map_err(|e| format!("failed to send transcript: {e}"))
    }

    // ── Query ──────────────────────────────────────────────────────

    /// Get the current session status.
    pub async fn status(&self) -> Option<SessionStatus> {
        let inner = self.core.inner.lock().await;
        let session = inner.as_ref().filter(|_| self.core.tasks.is_open())?;
        Some(SessionStatus {
            session_id: session.session_id.clone(),
            session_title: session.session_title.clone(),
            ticket: session.handle.ticket().to_string(),
            peers: session.peers.values().cloned().collect(),
            video_enabled: session.video_enabled,
            audio_enabled: session.audio_enabled,
            screen_sharing: session.screen_sharing,
            camera_name: session.camera_name.clone(),
            started_at: session.started_at,
        })
    }

    /// Get list of peers in the current room.
    pub async fn peers(&self) -> Vec<TutoringPeer> {
        let inner = self.core.inner.lock().await;
        match inner.as_ref() {
            Some(session) => session.peers.values().cloned().collect(),
            None => vec![],
        }
    }

    /// Check if currently in a session.
    pub async fn is_active(&self) -> bool {
        let inner = self.core.inner.lock().await;
        inner.is_some() && self.core.tasks.is_open()
    }

    /// Get diagnostic info about the current session for debugging A/V pipeline.
    pub async fn diagnostics(&self) -> Option<SessionDiagnostics> {
        let inner = self.core.inner.lock().await;
        let session = inner.as_ref().filter(|_| self.core.tasks.is_open())?;

        let mut recent_logs = session.recent_logs.clone();
        recent_logs.push("--- diag.log ---".to_string());
        recent_logs.extend(crate::diag::read().lines().map(str::to_string));

        Some(SessionDiagnostics {
            session_id: session.session_id.clone(),
            our_node_id: session.our_node_id.clone(),
            video_enabled: session.video_enabled,
            audio_enabled: session.audio_enabled,
            has_audio_ctx: session.audio.is_some(),
            has_mic_input: session.mic_input.is_some(),
            has_output_stream: session.output_stream.is_some(),
            has_self_preview: session.self_preview_task.is_some(),
            peer_count: session.peers.len(),
            peers: session
                .peers
                .values()
                .map(|p| PeerDiagnostics {
                    node_id: p.node_id.clone(),
                    display_name: p.display_name.clone(),
                    broadcasts: p.broadcasts.clone(),
                    connected: p.connected,
                })
                .collect(),
            task_count: session._tasks.len(),
            recent_logs,
            home_relay: session.home_relay.clone(),
        })
    }

    /// Get the current mic peak level (0.0–1.0) if in a session with audio.
    pub async fn get_mic_level(&self) -> f32 {
        let inner = self.core.inner.lock().await;
        match inner.as_ref().and_then(|s| s.mic_input.as_ref()) {
            Some(mic) => mic.level(),
            None => 0.0,
        }
    }

    /// Check device availability (camera + audio) before joining a session.
    /// Enumeration only: no stream, capture or audio session is started.
    pub async fn check_devices(&self) -> DeviceCheckResult {
        self.core.media.check_devices().await
    }

    /// List all available audio and camera devices.
    pub async fn list_devices(&self) -> DeviceList {
        let inventory = self.core.media.audio_devices();
        let cameras = self.core.media.cameras().await;
        DeviceList {
            audio_inputs: inventory.inputs,
            audio_outputs: inventory.outputs,
            cameras,
            selected_audio_input: inventory.selected_input,
            selected_audio_output: inventory.selected_output,
        }
    }
}

// ── Shared machinery ───────────────────────────────────────────────

impl<M: MediaDevices> Core<M> {
    /// The running session, if admission is still open.
    fn active<'a>(
        &self,
        inner: &'a mut Option<ActiveSession<M>>,
    ) -> Result<&'a mut ActiveSession<M>, String> {
        inner
            .as_mut()
            .filter(|_| self.tasks.is_open())
            .ok_or_else(|| "not in a tutoring session".to_string())
    }

    /// The session a change started on, if it is still the one running.
    /// The lock is released while a source opens, so the call may have ended
    /// or been replaced in the meantime.
    #[cfg(tutoring_video)]
    fn same_session<'a>(
        &self,
        inner: &'a mut Option<ActiveSession<M>>,
        session_id: &str,
    ) -> Result<&'a mut ActiveSession<M>, String> {
        inner
            .as_mut()
            .filter(|session| self.tasks.is_open() && session.session_id == session_id)
            .ok_or_else(|| "the call ended while its video source was opening".to_string())
    }

    fn emit<T: Serialize>(&self, event: &str, payload: T) {
        emit(self.events.as_ref(), event, payload);
    }

    async fn push_log(&self, msg: String) {
        if let Some(session) = self.inner.lock().await.as_mut() {
            session.push_log(msg);
        }
    }

    // ── Devices ────────────────────────────────────────────────────

    /// Start the audio engine on a blocking thread the session owns. Where
    /// the platform sets a start timeout, startup continues without audio
    /// once it passes; cleanup still joins an initializer that has not
    /// returned, since a timeout cannot stop it.
    async fn open_audio(&self, devices: &DeviceSelection) -> Option<M::Audio> {
        let media = self.media.clone();
        let devices = devices.clone();
        let result = within(
            self.profile.start_device_timeout,
            self.tasks.run_blocking(move || {
                catch_unwind(AssertUnwindSafe(|| media.open_audio(&devices)))
            }),
        )
        .await;
        match result {
            Ok(Ok(Ok(Ok(audio)))) => {
                log::info!("tutoring: audio engine started");
                Some(audio)
            }
            Ok(Ok(Ok(Err(error)))) => {
                log::error!("tutoring: audio engine failed to start: {error}");
                None
            }
            Ok(Ok(Err(panic))) => {
                log::error!("tutoring: audio engine panicked during init: {panic:?}");
                None
            }
            Ok(Err(error)) => {
                log::warn!("tutoring: audio initialization cancelled: {error}");
                None
            }
            Err(limit) => {
                log::error!(
                    "tutoring: audio engine init timed out after {}s",
                    limit.as_secs()
                );
                None
            }
        }
    }

    /// Open a camera or the screen on a detached blocking thread, away from
    /// the call's lock.
    ///
    /// Opening a camera can block for seconds, or forever when the device is
    /// gone; doing it under `inner` froze every call command (14 Oct). A
    /// detached thread, not the session's task set, means a source that
    /// never answers stalls neither the call's other controls nor leaving
    /// the call. On timeout the thread is abandoned and whatever it
    /// eventually opens is dropped.
    #[cfg(tutoring_video)]
    async fn open_video_detached(
        &self,
        screen: bool,
        camera: Option<String>,
    ) -> Result<OpenedVideo<M::VideoSource>, String> {
        let media = self.media.clone();
        let open = tokio::task::spawn_blocking(move || {
            if screen {
                media.open_screen()
            } else {
                media.open_camera(camera.as_deref())
            }
        });
        match tokio::time::timeout(SOURCE_OPEN_TIMEOUT, open).await {
            Ok(Ok(opened)) => opened,
            Ok(Err(join)) => Err(format!("the video source failed to open: {join}")),
            Err(_) => Err(format!(
                "the video source did not open within {}s",
                SOURCE_OPEN_TIMEOUT.as_secs()
            )),
        }
    }

    /// Open the camera at session start, on a blocking thread the session
    /// owns so cleanup joins it. `Ok(None)` is a call without video.
    #[cfg(tutoring_video)]
    async fn open_camera_at_start(
        &self,
        camera: Option<String>,
    ) -> Result<Option<OpenedVideo<M::VideoSource>>, String> {
        let media = self.media.clone();
        let result = within(
            self.profile.start_device_timeout,
            self.tasks.run_blocking(move || {
                catch_unwind(AssertUnwindSafe(|| media.open_camera(camera.as_deref())))
            }),
        )
        .await;
        match result {
            Ok(Ok(Ok(Ok(opened)))) => Ok(Some(opened)),
            Ok(Ok(Ok(Err(error)))) => {
                log::warn!("tutoring: camera unavailable, continuing without video: {error}");
                Ok(None)
            }
            Ok(Ok(Err(panic))) => {
                log::error!("tutoring: camera init panicked: {panic:?}");
                Ok(None)
            }
            Ok(Err(error)) => Err(format!("camera initialization failed: {error}")),
            Err(limit) => {
                log::error!("tutoring: camera init timed out after {}s", limit.as_secs());
                Ok(None)
            }
        }
    }

    /// Build the broadcast's renditions from an opened source.
    #[cfg(tutoring_video)]
    fn renditions(opened: OpenedVideo<M::VideoSource>) -> (Option<String>, VideoRenditions) {
        (
            opened.camera_name,
            VideoRenditions::new::<M::VideoEncoder>(opened.source, opened.presets),
        )
    }

    /// Create the local broadcast with mic and camera.
    ///
    /// Returns `(broadcast, mic_input, camera_name, has_video)`; the mic
    /// stream is a clone kept for peak metering.
    async fn create_broadcast(
        &self,
        audio: Option<&M::Audio>,
        camera: Option<String>,
    ) -> Result<(PublishBroadcast, Option<Input<M>>, Option<String>, bool), String> {
        if !self.tasks.is_open() {
            return Err("media session is closing".into());
        }
        let mut broadcast = PublishBroadcast::new();
        let mut mic_input = None;

        if let Some(audio) = audio {
            match audio.input().await {
                Ok(mic) => {
                    mic_input = Some(mic.clone());
                    let renditions =
                        AudioRenditions::new::<PureOpusEncoder>(mic, [AudioPreset::Hq]);
                    broadcast
                        .set_audio(Some(renditions))
                        .map_err(|e| format!("failed to set audio: {e}"))?;
                    log::info!("tutoring: microphone initialized");
                }
                Err(e) => {
                    log::warn!("tutoring: microphone unavailable, continuing without audio: {e}");
                }
            }
        }

        #[cfg(tutoring_video)]
        if self.profile.video {
            log::info!("tutoring: initializing camera...");
            if let Some(opened) = self.open_camera_at_start(camera).await? {
                let (camera_name, renditions) = Self::renditions(opened);
                broadcast
                    .set_video(Some(renditions))
                    .map_err(|e| format!("failed to set video: {e}"))?;
                log::info!("tutoring: camera initialized");
                return Ok((broadcast, mic_input, camera_name, true));
            }
        }
        #[cfg(not(tutoring_video))]
        if camera.is_some() {
            log::info!("tutoring: ignoring requested camera — {VIDEO_DISABLED_ERROR}");
        }

        Ok((broadcast, mic_input, None, false))
    }

    /// Swap an opened source into the broadcast and restart the self view.
    #[cfg(tutoring_video)]
    fn install_video(
        &self,
        session: &mut ActiveSession<M>,
        opened: OpenedVideo<M::VideoSource>,
        screen: bool,
    ) -> Result<(), String> {
        if let Some(task) = session.self_preview_task.take() {
            task.abort();
        }
        let (camera_name, renditions) = Self::renditions(opened);
        session
            .broadcast
            .set_video(Some(renditions))
            .map_err(|e| format!("failed to enable video: {e}"))?;
        session.video_enabled = true;
        session.screen_sharing = screen;
        session.camera_name = camera_name;
        session.self_preview_task = self.start_self_preview(&mut session.broadcast);
        Ok(())
    }

    /// Stop sending video and tell the webview the self view ended.
    #[cfg(tutoring_video)]
    fn remove_video(&self, session: &mut ActiveSession<M>) -> Result<(), String> {
        if let Some(task) = session.self_preview_task.take() {
            task.abort();
        }
        session
            .broadcast
            .set_video(None)
            .map_err(|e| format!("failed to disable video: {e}"))?;
        session.video_enabled = false;
        session.screen_sharing = false;
        session.camera_name = None;
        self.emit(
            "tutoring:peer-video-ended",
            PeerVideoEndedEvent {
                node_id: "self".into(),
            },
        );
        Ok(())
    }

    /// Start a self-preview from the broadcast's local video source.
    ///
    /// Uses `PublishBroadcast::watch_local()` which taps the shared camera
    /// source directly (no encode/decode round-trip). Frames are emitted as
    /// `tutoring:video-frame` events with `node_id = "self"`.
    #[cfg(tutoring_video)]
    fn start_self_preview(&self, broadcast: &mut PublishBroadcast) -> Option<TaskHandle> {
        let watch = broadcast.watch_local(DecodeConfig::default())?;
        log::info!("tutoring: starting self-preview");
        crate::diag::log("self_preview: STARTED");
        Some(self.spawn_frame_bridge(watch, "self".into(), self.profile.self_view, None))
    }

    #[cfg(not(tutoring_video))]
    fn start_self_preview(&self, _broadcast: &mut PublishBroadcast) -> Option<TaskHandle> {
        None
    }

    // ── Audio outputs ──────────────────────────────────────────────

    async fn create_session_output(&self, audio: &Option<M::Audio>) -> Option<Output<M>> {
        let audio = audio.as_ref()?;
        match audio.output().await {
            Ok(output) => {
                output.resume();
                Some(output)
            }
            Err(err) => {
                log::warn!("tutoring: session audio output unavailable: {err}");
                None
            }
        }
    }

    async fn get_or_create_remote_output(
        &self,
        audio: &Option<M::Audio>,
        remote_key: &str,
    ) -> Option<Output<M>> {
        {
            let mut guard = self.inner.lock().await;
            if let Some(session) = guard.as_mut() {
                if let Some(existing) = session.output_stream.clone() {
                    existing.resume();
                    if existing.is_active() {
                        session
                            .remote_output_streams
                            .insert(remote_key.to_string(), existing.clone());
                        return Some(existing);
                    }
                    session.output_stream = None;
                }
                if let Some(existing) = session.remote_output_streams.get(remote_key).cloned() {
                    existing.resume();
                    if existing.is_active() {
                        session.output_stream = Some(existing.clone());
                        return Some(existing);
                    }
                    log::warn!(
                        "tutoring: cached audio output for {remote_key} is inactive, recreating"
                    );
                    session.remote_output_streams.remove(remote_key);
                }
            }
        }

        let audio = audio.as_ref()?;
        let output = match audio.output().await {
            Ok(output) => output,
            Err(err) => {
                log::warn!("tutoring: audio output unavailable for {remote_key}: {err}");
                self.push_log(format!("ERR audio_output[{remote_key}]: {err}"))
                    .await;
                return None;
            }
        };
        output.resume();

        let mut guard = self.inner.lock().await;
        let session = guard.as_mut()?;
        if session.output_stream.is_none() {
            session.output_stream = Some(output.clone());
        }
        let entry = session
            .remote_output_streams
            .entry(remote_key.to_string())
            .or_insert_with(|| output.clone())
            .clone();
        if let Some(shared) = session.output_stream.clone() {
            shared.resume();
            return Some(shared);
        }
        entry.resume();
        Some(entry)
    }

    /// Resume every output after the device moved the audio route or gave
    /// the session back.
    async fn resume_outputs(&self) {
        let outputs = match self.inner.lock().await.as_ref() {
            Some(session) => session.outputs(),
            None => Vec::new(),
        };
        for output in outputs {
            output.resume();
        }
    }

    // ── Device events ──────────────────────────────────────────────

    /// React to what the device does on its own for the length of the
    /// session.
    fn start_media_event_listener(&self) -> TaskHandle {
        let mut events = self.media.events();
        let core = self.clone();
        self.tasks.clone().spawn(async move {
            loop {
                match events.recv().await {
                    // Restoring the camera waits a second and then on the
                    // camera itself; audio events must not queue behind it.
                    Ok(MediaEvent::AppForegrounded) => {
                        log::info!("tutoring: device event AppForegrounded");
                        let restore = core.clone();
                        core.tasks
                            .spawn(async move { restore.restore_camera_after_foreground().await });
                    }
                    Ok(event) => core.handle_media_event(event).await,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    }

    async fn handle_media_event(&self, event: MediaEvent) {
        log::info!("tutoring: device event {event:?}");
        crate::diag::log(&format!("device event {event:?}"));
        match event {
            MediaEvent::AudioResumed | MediaEvent::AudioRouteChanged => {
                self.resume_outputs().await;
            }
            MediaEvent::AppBackgrounded => self.release_camera_for_background().await,
            MediaEvent::AppForegrounded => self.restore_camera_after_foreground().await,
        }
    }

    /// Leaving the foreground: stop the camera to release the hardware, and
    /// remember that the app did it.
    #[cfg(tutoring_video)]
    async fn release_camera_for_background(&self) {
        let mut guard = self.inner.lock().await;
        let Some(session) = guard.as_mut() else {
            return;
        };
        session.background_epoch += 1;
        if !session.video_enabled {
            return;
        }
        match self.remove_video(session) {
            Ok(()) => {
                session.camera_released_for_background = true;
                log::info!("tutoring: camera stopped (background)");
            }
            Err(e) => log::warn!("tutoring: failed to stop the camera for the background: {e}"),
        }
    }

    #[cfg(not(tutoring_video))]
    async fn release_camera_for_background(&self) {}

    /// Back in the foreground: give connections a moment, reopen the camera
    /// the app took away for the background (never one the learner turned
    /// off), and let remote video be watched afresh.
    #[cfg(tutoring_video)]
    async fn restore_camera_after_foreground(&self) {
        let epoch = match self.inner.lock().await.as_ref() {
            Some(session) => session.background_epoch,
            None => return,
        };
        tokio::time::sleep(Duration::from_secs(1)).await;
        let (session_id, camera) = {
            let guard = self.inner.lock().await;
            let Some(session) = guard.as_ref() else {
                return;
            };
            let wanted = session.camera_released_for_background
                && !session.video_enabled
                && session.background_epoch == epoch;
            (
                session.session_id.clone(),
                wanted.then(|| session.devices.camera_index.clone()),
            )
        };
        let reopened = match camera {
            Some(camera) => Some(self.open_video_detached(false, camera).await),
            None => None,
        };
        let mut guard = self.inner.lock().await;
        let Ok(session) = self.same_session(&mut guard, &session_id) else {
            return;
        };
        // Still wanted: the learner did not choose meanwhile, and the app
        // did not go back to the background.
        let still_wanted = session.camera_released_for_background
            && !session.video_enabled
            && session.background_epoch == epoch;
        match reopened {
            Some(Ok(opened)) if still_wanted => {
                session.camera_released_for_background = false;
                match self.install_video(session, opened, false) {
                    Ok(()) => log::info!("tutoring: camera restarted (foreground)"),
                    Err(e) => log::warn!("tutoring: failed to restart camera: {e}"),
                }
            }
            Some(Err(e)) => log::warn!("tutoring: failed to restart camera after unlock: {e}"),
            _ => {}
        }
        session._subscribed_video_keys.clear();
    }

    #[cfg(not(tutoring_video))]
    async fn restore_camera_after_foreground(&self) {}

    // ── Audio levels ───────────────────────────────────────────────

    /// Periodically read mic and output peak levels and emit
    /// `tutoring:audio-level` events for the webview's VU meters.
    fn start_audio_level_emitter(&self) -> TaskHandle {
        let core = self.clone();
        self.tasks.clone().spawn(async move {
            let mut interval = tokio::time::interval(core.profile.audio_level_interval);
            loop {
                interval.tick().await;
                let (mic_level, output_level) = {
                    let guard = core.inner.lock().await;
                    match guard.as_ref() {
                        Some(session) => (
                            session.mic_input.as_ref().map(|m| m.level()).unwrap_or(0.0),
                            session
                                .output_stream
                                .as_ref()
                                .and_then(|o| o.smoothed_peak_normalized())
                                .unwrap_or(0.0),
                        ),
                        None => break,
                    }
                };
                core.emit(
                    "tutoring:audio-level",
                    AudioLevelEvent {
                        mic_level,
                        output_level,
                    },
                );
            }
        })
    }

    // ── Remote broadcasts ──────────────────────────────────────────

    async fn clear_remote_audio_subscription(&self, audio_key: &str, remote_key: &str) {
        if let Some(session) = self.inner.lock().await.as_mut() {
            session._subscribed_audio_keys.remove(audio_key);
            session.remote_output_streams.remove(remote_key);
        }
    }

    #[cfg(tutoring_video)]
    async fn clear_remote_video_subscription(&self, video_key: &str) {
        if let Some(session) = self.inner.lock().await.as_mut() {
            session._subscribed_video_keys.remove(video_key);
        }
    }

    async fn repair_remote_audio_subscription(
        &self,
        audio: &Option<M::Audio>,
        broadcast: &SubscribeBroadcast,
        node_id: &str,
        name: &str,
    ) {
        // hang 0.19: `Catalog.audio` is a plain `Audio` (not `Option`); an empty
        // renditions map means "no audio".
        if broadcast.catalog().audio.renditions.is_empty() {
            return;
        }

        let short_id = node_id[..node_id.len().min(12)].to_string();
        let audio_key = format!("{node_id}:{name}:audio");
        let remote_audio_key = format!("{node_id}:{name}");
        let should_subscribe = match self.inner.lock().await.as_mut() {
            Some(session) => session._subscribed_audio_keys.insert(audio_key.clone()),
            None => false,
        };
        if !should_subscribe {
            return;
        }

        let Some(audio_out) = self
            .get_or_create_remote_output(audio, &remote_audio_key)
            .await
        else {
            log::warn!("tutoring: no audio output available, skipping audio for {short_id}:{name}");
            self.clear_remote_audio_subscription(&audio_key, &remote_audio_key)
                .await;
            return;
        };

        match broadcast.listen::<PureOpusDecoder>(audio_out) {
            Ok(audio_track) => {
                log::info!("tutoring: listening to audio from {short_id}:{name}");
                self.push_log(format!("audio_listen OK: {short_id}:{name}"))
                    .await;
                let core = self.clone();
                let nid = node_id.to_string();
                let bname = name.to_string();
                let keepalive = self.tasks.clone().spawn(async move {
                    audio_track.stopped().await;
                    log::warn!("tutoring: audio track stopped for {nid}:{bname}");
                    core.clear_remote_audio_subscription(&audio_key, &remote_audio_key)
                        .await;
                });
                if let Some(session) = self.inner.lock().await.as_mut() {
                    session._tasks.push(keepalive);
                }
            }
            Err(e) => {
                log::error!("tutoring: failed to listen to audio from {short_id}:{name}: {e}");
                self.clear_remote_audio_subscription(&audio_key, &remote_audio_key)
                    .await;
                self.push_log(format!("ERR audio_listen: {short_id}:{name}: {e}"))
                    .await;
            }
        }
    }

    #[cfg(tutoring_video)]
    async fn repair_remote_video_subscription(
        &self,
        broadcast: &SubscribeBroadcast,
        node_id: &str,
        name: &str,
    ) {
        // hang 0.19: `Catalog.video` is a plain `Video` (not `Option`); empty
        // renditions means "no video".
        if broadcast.catalog().video.renditions.is_empty() {
            return;
        }

        let short_id = node_id[..node_id.len().min(12)].to_string();
        let video_key = format!("{node_id}:{name}:video");
        let should_watch = match self.inner.lock().await.as_mut() {
            Some(session) => session._subscribed_video_keys.insert(video_key.clone()),
            None => false,
        };
        if !should_watch {
            return;
        }

        match broadcast
            .watch_with::<M::VideoDecoder>(&DecodeConfig::default(), self.profile.remote_quality)
        {
            Ok(video) => {
                log::info!("tutoring: watching video from {short_id}:{name}");
                self.push_log(format!("video_watch OK: {short_id}:{name}"))
                    .await;
                self.spawn_frame_bridge(
                    video,
                    node_id.to_string(),
                    self.profile.remote_view,
                    Some(video_key),
                );
            }
            Err(e) => {
                log::error!("tutoring: failed to watch {short_id}:{name}: {e:#}");
                self.clear_remote_video_subscription(&video_key).await;
                self.push_log(format!("ERR video_watch: {short_id}:{name}: {e:#}"))
                    .await;
            }
        }
    }

    fn spawn_remote_broadcast_repair_task(
        &self,
        audio: Option<M::Audio>,
        broadcast: SubscribeBroadcast,
        node_id: String,
        name: String,
    ) -> TaskHandle {
        let core = self.clone();
        self.tasks.clone().spawn(async move {
            let repair_key = format!("{node_id}:{name}:repair");
            let closed = broadcast.closed();
            tokio::pin!(closed);
            let mut interval = tokio::time::interval(Duration::from_secs(2));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    _ = &mut closed => break,
                    _ = interval.tick() => {
                        if core.inner.lock().await.is_none() {
                            break;
                        }
                        core.repair_remote_audio_subscription(&audio, &broadcast, &node_id, &name).await;
                        #[cfg(tutoring_video)]
                        core.repair_remote_video_subscription(&broadcast, &node_id, &name).await;
                    }
                }
            }

            if let Some(session) = core.inner.lock().await.as_mut() {
                session._repair_broadcast_keys.remove(&repair_key);
            }
        })
    }

    // ── Gossip: chat, transcript, names ────────────────────────────

    /// Seed the /chat and /names gossip topics with a freshly-discovered
    /// peer's node ID. Without this, a subscription created with an empty
    /// bootstrap list has no neighbours and never propagates messages, even
    /// though the MoQ room already has a live connection to the peer.
    async fn bootstrap_gossip_peer(&self, peer: EndpointId) {
        let (chat, names) = match self.inner.lock().await.as_ref() {
            Some(session) => (session.chat_sender.clone(), session.names_sender.clone()),
            None => (None, None),
        };
        for (label, sender) in [("chat", chat), ("names", names)] {
            if let Some(sender) = sender {
                if let Err(e) = sender.join_peers(vec![peer]).await {
                    log::warn!("tutoring: {label} join_peers failed: {e}");
                }
            }
        }
    }

    /// Set up a chat channel on a gossip topic derived from the room.
    async fn setup_chat(
        &self,
        gossip: &Gossip,
        topic_seed: &[u8],
        our_node_id: &str,
    ) -> Option<iroh_gossip::api::GossipSender> {
        let topic_id = derived_topic(topic_seed, b"/chat");
        let topic = match gossip.subscribe(topic_id, vec![]).await {
            Ok(topic) => topic,
            Err(e) => {
                log::warn!("tutoring: failed to set up chat gossip topic: {e}");
                return None;
            }
        };
        let (sender, mut receiver) = topic.split();
        let our_id = our_node_id.to_string();
        let core = self.clone();
        self.tasks.clone().spawn(async move {
            use futures::StreamExt;
            while let Some(Ok(event)) = receiver.next().await {
                let iroh_gossip::api::Event::Received(msg) = event else {
                    continue;
                };
                if let Some(payload) = msg.content.strip_prefix(TRANSCRIPT_PREFIX) {
                    match postcard::from_bytes::<TranscriptMessage>(payload) {
                        Ok(transcript) if transcript.sender != our_id => core.emit(
                            "tutoring:transcript",
                            TranscriptMessageEvent {
                                sender: transcript.sender,
                                sender_name: transcript.sender_name,
                                text: transcript.text,
                                confidence: transcript.confidence,
                                timestamp: transcript.timestamp,
                            },
                        ),
                        Ok(_) => {}
                        Err(e) => log::warn!("tutoring: failed to decode transcript message: {e}"),
                    }
                    continue;
                }
                match postcard::from_bytes::<ChatMessage>(&msg.content) {
                    // Don't echo our own messages
                    Ok(chat) if chat.sender != our_id => core.emit(
                        "tutoring:chat",
                        ChatMessageEvent {
                            sender: chat.sender,
                            sender_name: chat.sender_name,
                            text: chat.text,
                            timestamp: chat.timestamp,
                        },
                    ),
                    Ok(_) => {}
                    Err(e) => log::warn!("tutoring: failed to decode chat message: {e}"),
                }
            }
            log::info!("tutoring: chat receiver loop ended");
        });
        Some(sender)
    }

    /// Set up a display name exchange channel on a `/names` gossip topic.
    ///
    /// Periodically broadcasts our display name so new joiners learn it,
    /// and listens for name announcements from other peers, updating the
    /// peers map and emitting `tutoring:peer-name` events.
    async fn setup_names(
        &self,
        gossip: &Gossip,
        topic_seed: &[u8],
        our_node_id: &str,
        our_display_name: &str,
    ) -> Option<(iroh_gossip::api::GossipSender, TaskHandle)> {
        let topic_id = derived_topic(topic_seed, b"/names");
        let topic = match gossip.subscribe(topic_id, vec![]).await {
            Ok(topic) => topic,
            Err(e) => {
                log::warn!("tutoring: failed to set up names gossip topic: {e}");
                return None;
            }
        };
        let (sender, mut receiver) = topic.split();
        let sender_ret = sender.clone();
        let announce = NameAnnouncement {
            node_id: our_node_id.to_string(),
            display_name: our_display_name.to_string(),
        };
        let core = self.clone();
        let task = self.tasks.clone().spawn(async move {
            use futures::StreamExt;
            let encoded = postcard::to_stdvec(&announce).ok();
            if let Some(encoded) = &encoded {
                let _ = sender.broadcast(Bytes::from(encoded.clone())).await;
            }
            let mut broadcast_interval =
                tokio::time::interval(Duration::from_secs(NAME_BROADCAST_INTERVAL_SECS));
            broadcast_interval.tick().await; // consume first immediate tick

            loop {
                tokio::select! {
                    _ = broadcast_interval.tick() => {
                        if let Some(encoded) = &encoded {
                            let _ = sender.broadcast(Bytes::from(encoded.clone())).await;
                        }
                    }
                    msg = receiver.next() => {
                        match msg {
                            Some(Ok(iroh_gossip::api::Event::Received(msg))) => {
                                let Ok(learned) = postcard::from_bytes::<NameAnnouncement>(&msg.content) else {
                                    continue;
                                };
                                if learned.node_id == announce.node_id {
                                    continue;
                                }
                                log::info!(
                                    "tutoring: learned peer name: {} = {}",
                                    &learned.node_id[..8.min(learned.node_id.len())],
                                    learned.display_name
                                );
                                if let Some(session) = core.inner.lock().await.as_mut() {
                                    let display_name = learned.display_name.clone();
                                    session.upsert_peer(&learned.node_id, |p| {
                                        p.display_name = Some(display_name);
                                    });
                                }
                                core.emit(
                                    "tutoring:peer-name",
                                    PeerNameEvent {
                                        node_id: learned.node_id,
                                        display_name: learned.display_name,
                                    },
                                );
                            }
                            Some(Ok(_)) => {}
                            Some(Err(e)) => log::warn!("tutoring: names gossip error: {e}"),
                            None => break,
                        }
                    }
                }
            }
            log::info!("tutoring: names loop ended");
        });
        Some((sender_ret, task))
    }

    // ── Room events ────────────────────────────────────────────────

    /// Process room events (peer announcements, connections, broadcast
    /// subscriptions) and start the audio and video watchers for each
    /// subscribed remote broadcast.
    async fn event_loop(&self, mut events: mpsc::Receiver<RoomEvent>, audio: Option<M::Audio>) {
        while let Some(event) = events.recv().await {
            match event {
                RoomEvent::RemoteAnnounced { remote, broadcasts } => {
                    let node_id = remote.to_string();
                    let short_id = node_id[..node_id.len().min(12)].to_string();
                    log::info!("tutoring: peer announced: {short_id} with {broadcasts:?}");
                    if let Some(session) = self.inner.lock().await.as_mut() {
                        session.upsert_peer(&node_id, |p| p.broadcasts = broadcasts.clone());
                        session.push_log(format!(
                            "peer_announced: {short_id} broadcasts={broadcasts:?}"
                        ));
                    }
                    self.bootstrap_gossip_peer(remote).await;
                }
                RoomEvent::RemoteConnected {
                    session: moq_session,
                } => {
                    let remote = moq_session.conn().remote_id();
                    let node_id = remote.to_string();
                    let short_id = node_id[..node_id.len().min(12)].to_string();
                    log::info!("tutoring: peer connected (MoQ): {short_id}");
                    if let Some(session) = self.inner.lock().await.as_mut() {
                        session.upsert_peer(&node_id, |p| p.connected = true);
                        session.push_log(format!("peer_connected_moq: {short_id}"));
                    }
                    self.bootstrap_gossip_peer(remote).await;
                }
                RoomEvent::BroadcastSubscribed {
                    session: moq_session,
                    broadcast,
                } => {
                    let node_id = moq_session.remote_id().to_string();
                    let name = broadcast.broadcast_name().to_string();
                    let short_id = node_id[..node_id.len().min(12)].to_string();
                    log::info!("tutoring: subscribed to {short_id}:{name}");

                    let repair_key = format!("{node_id}:{name}:repair");
                    let spawn_repair_task = match self.inner.lock().await.as_mut() {
                        Some(session) => {
                            session.upsert_peer(&node_id, |p| {
                                if !p.broadcasts.contains(&name) {
                                    p.broadcasts.push(name.clone());
                                }
                                p.connected = true;
                            });
                            session._moq_sessions.push(moq_session);
                            session.push_log(format!("broadcast_subscribed: {short_id}:{name}"));
                            session._repair_broadcast_keys.insert(repair_key)
                        }
                        None => false,
                    };

                    let catalog = broadcast.catalog();
                    log::info!(
                        "tutoring: {short_id}:{name} catalog: video={}, audio={}",
                        !catalog.video.renditions.is_empty(),
                        !catalog.audio.renditions.is_empty()
                    );
                    for (rname, vcfg) in &catalog.video.renditions {
                        log::info!(
                            "tutoring: {short_id} video '{rname}': {}x{}, desc={}B",
                            vcfg.coded_width.unwrap_or(0),
                            vcfg.coded_height.unwrap_or(0),
                            vcfg.description.as_ref().map(|d| d.len()).unwrap_or(0),
                        );
                    }
                    for (rname, acfg) in &catalog.audio.renditions {
                        log::info!(
                            "tutoring: {short_id} audio '{rname}': {}Hz {}ch",
                            acfg.sample_rate,
                            acfg.channel_count,
                        );
                    }

                    self.repair_remote_audio_subscription(&audio, &broadcast, &node_id, &name)
                        .await;
                    #[cfg(tutoring_video)]
                    self.repair_remote_video_subscription(&broadcast, &node_id, &name)
                        .await;
                    #[cfg(not(tutoring_video))]
                    log::info!(
                        "tutoring: skipping remote video subscribe for {short_id}:{name} — {VIDEO_DISABLED_ERROR}"
                    );

                    let broadcast_for_repair = broadcast.clone();
                    if let Some(session) = self.inner.lock().await.as_mut() {
                        session._subscribe_broadcasts.push(broadcast);
                    }
                    if spawn_repair_task {
                        let repair_task = self.spawn_remote_broadcast_repair_task(
                            audio.clone(),
                            broadcast_for_repair,
                            node_id,
                            name,
                        );
                        if let Some(session) = self.inner.lock().await.as_mut() {
                            session._tasks.push(repair_task);
                        }
                    }
                }
            }
        }
        log::info!("tutoring: event loop ended");
    }

    // ── Frame bridge ───────────────────────────────────────────────

    /// Spawn a background task that polls decoded video frames from a
    /// `WatchTrack` (either remote peer or local preview), encodes them as
    /// JPEG, and emits them to the webview as `tutoring:video-frame`.
    ///
    /// When the track closes (peer left or video disabled), emits
    /// `tutoring:peer-video-ended` so the frontend can clean up, and clears
    /// the remote subscription key so the repair task may watch again.
    #[cfg(tutoring_video)]
    fn spawn_frame_bridge(
        &self,
        watch: WatchTrack,
        node_id: String,
        tuning: FrameTuning,
        subscription_key: Option<String>,
    ) -> TaskHandle {
        let (mut frames, handle) = watch.split();
        handle.set_viewport(tuning.viewport.0, tuning.viewport.1);
        let core = self.clone();

        self.tasks.clone().spawn(async move {
            let _handle = handle;
            log::info!(
                "tutoring: frame bridge started for {node_id} ({}x{} q={} every {:?})",
                tuning.viewport.0,
                tuning.viewport.1,
                tuning.jpeg_quality,
                tuning.interval
            );
            crate::diag::log(&format!("frame_bridge[{node_id}]: STARTED"));

            // Frames arrive on the source's clock; a gate exactly one
            // interval wide drops every other one whenever they arrive a
            // hair early.
            let gate = tuning.interval.saturating_sub(Duration::from_millis(8));
            let mut last_emit = Instant::now();
            let mut frame_count: u64 = 0;
            let mut stats = BridgeStats::default();
            let initial_timeout = Duration::from_secs(10);
            let stall_timeout = Duration::from_secs(30);

            loop {
                let timeout = if frame_count == 0 {
                    initial_timeout
                } else {
                    stall_timeout
                };
                let maybe_frame = match tokio::time::timeout(timeout, frames.next_frame()).await {
                    Ok(f) => f,
                    Err(_) => {
                        if frame_count == 0 {
                            log::warn!(
                                "tutoring: frame bridge {node_id}: no frames after {initial_timeout:?} — track likely dead"
                            );
                        } else {
                            log::warn!(
                                "tutoring: frame bridge {node_id}: stalled after {frame_count} frames ({stall_timeout:?} with no new frame) — treating as dead"
                            );
                        }
                        None
                    }
                };

                let Some(frame) = maybe_frame else {
                    log::info!("tutoring: frame bridge ended for {node_id} (track closed)");
                    crate::diag::log(&format!(
                        "frame_bridge[{node_id}]: ENDED, total frames={frame_count}"
                    ));
                    core.emit(
                        "tutoring:peer-video-ended",
                        PeerVideoEndedEvent {
                            node_id: node_id.clone(),
                        },
                    );
                    if let Some(key) = &subscription_key {
                        core.clear_remote_video_subscription(key).await;
                        log::info!("tutoring: cleared video subscription key {key}");
                    }
                    break;
                };

                frame_count += 1;
                if frame_count == 1 || frame_count.is_multiple_of(100) {
                    let img = frame.img();
                    let (w, h) = img.dimensions();
                    log::info!(
                        "tutoring: frame bridge {node_id}: frame #{frame_count} {w}x{h} buf={}",
                        img.as_raw().len()
                    );
                }
                let now = Instant::now();
                if now.duration_since(last_emit) < gate {
                    continue;
                }
                last_emit = now;

                // Scaling and encoding run on a blocking thread, not on the
                // async workers the call's networking shares.
                let encode_started = Instant::now();
                let img = frame.img().clone();
                let encoded = tokio::task::spawn_blocking(move || {
                    super::preview_jpeg::encode(img, tuning.jpeg_max, tuning.jpeg_quality)
                })
                .await;
                let (jpeg_b64, width, height) = match encoded {
                    Ok(Ok(frame)) => frame,
                    Ok(Err(e)) => {
                        log::warn!("tutoring: JPEG encode failed for {node_id}: {e}");
                        continue;
                    }
                    Err(_) => continue,
                };

                let emit_started = Instant::now();
                core.emit(
                    "tutoring:video-frame",
                    VideoFrameEvent {
                        node_id: node_id.clone(),
                        jpeg_b64,
                        width,
                        height,
                    },
                );
                if let Some(summary) =
                    stats.record(emit_started - encode_started, emit_started.elapsed())
                {
                    log::info!("tutoring: frame bridge {node_id}: {summary}");
                }
            }
        })
    }
}

/// Rolling numbers for a frame bridge, so a slow preview can be pinned on
/// the JPEG encode, the hand-off to the webview, or the frames arriving.
#[cfg(tutoring_video)]
#[derive(Default)]
struct BridgeStats {
    since: Option<Instant>,
    sent: u32,
    encode: Duration,
    emit: Duration,
}

#[cfg(tutoring_video)]
impl BridgeStats {
    const WINDOW: u32 = 150;

    /// Count one sent frame; every [`Self::WINDOW`] frames, describe them.
    fn record(&mut self, encode: Duration, emit: Duration) -> Option<String> {
        let since = *self.since.get_or_insert_with(Instant::now);
        self.sent += 1;
        self.encode += encode;
        self.emit += emit;
        if self.sent < Self::WINDOW {
            return None;
        }
        let summary = format!(
            "sent {:.1} fps, encode {:.1} ms, emit {:.1} ms",
            f64::from(self.sent) / since.elapsed().as_secs_f64(),
            self.encode.as_secs_f64() * 1000.0 / f64::from(self.sent),
            self.emit.as_secs_f64() * 1000.0 / f64::from(self.sent),
        );
        *self = Self::default();
        Some(summary)
    }
}

/// Run `work`, giving up after `limit` if there is one. `Err` carries the
/// limit that passed.
async fn within<F: std::future::Future>(
    limit: Option<Duration>,
    work: F,
) -> Result<F::Output, Duration> {
    match limit {
        Some(limit) => tokio::time::timeout(limit, work).await.map_err(|_| limit),
        None => Ok(work.await),
    }
}

/// A gossip topic derived from the room's by a suffix.
fn derived_topic(topic_seed: &[u8], suffix: &[u8]) -> iroh_gossip::proto::TopicId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(topic_seed);
    hasher.update(suffix);
    iroh_gossip::proto::TopicId::from_bytes(*hasher.finalize().as_bytes())
}
