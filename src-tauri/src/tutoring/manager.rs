//! TutoringManager — room lifecycle, media controls, video frame
//! bridge, session chat, and peer identity.
//!
//! Manages creation/joining of iroh-live rooms, local media publishing
//! (camera, mic, screen share), bridges decoded remote video frames
//! to the webview via Tauri events, text chat via a gossip topic
//! derived from the room topic, and peer display name exchange via
//! a separate gossip `/names` topic.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use bytes::Bytes;
use image::ImageEncoder;
use iroh::Endpoint;
use iroh_gossip::net::Gossip;
use iroh_live::media::audio::{AudioBackend, DeviceId, InputStream, OutputStream};
use iroh_live::media::av::{AudioPreset, AudioSinkHandle, DecodeConfig, VideoPreset};
use iroh_live::media::capture::{CameraIndex, CameraCapturer, ScreenCapturer};
use iroh_live::media::ffmpeg::{FfmpegDecoders, FfmpegVideoDecoder, H264Encoder, OpusEncoder};
use iroh_live::media::publish::{AudioRenditions, PublishBroadcast, VideoRenditions};
use iroh_live::media::subscribe::WatchTrack;
use iroh_live::rooms::{Room, RoomEvent, RoomHandle, RoomTicket};
use iroh_live::Live;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;

// ── Constants ──────────────────────────────────────────────────────

/// Maximum chat message text length (bytes).
const CHAT_MAX_LENGTH: usize = 2000;

/// Minimum interval between chat messages from the local user (ms).
const CHAT_RATE_LIMIT_MS: u64 = 200;

/// Interval for re-broadcasting our display name (seconds).
const NAME_BROADCAST_INTERVAL_SECS: u64 = 15;

// ── Chat message protocol ──────────────────────────────────────────

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

// ── Name announcement protocol ─────────────────────────────────────

/// A display name announcement broadcast over the `/names` gossip topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct NameAnnouncement {
    /// Sender's iroh node ID (hex).
    node_id: String,
    /// Human-readable display name.
    display_name: String,
}

/// Tauri event payload for a video frame.
#[derive(Debug, Clone, Serialize)]
struct VideoFrameEvent {
    /// Node ID of the peer (or "self" for local preview).
    node_id: String,
    /// Base64-encoded JPEG image data.
    jpeg_b64: String,
    /// Frame width in pixels.
    width: u32,
    /// Frame height in pixels.
    height: u32,
}

/// Tauri event payload for an incoming chat message.
#[derive(Debug, Clone, Serialize)]
struct ChatMessageEvent {
    sender: String,
    sender_name: Option<String>,
    text: String,
    timestamp: u64,
}

/// Tauri event payload when a peer's video track closes.
#[derive(Debug, Clone, Serialize)]
struct PeerVideoEndedEvent {
    node_id: String,
}

/// Tauri event payload when a peer's display name is learned.
#[derive(Debug, Clone, Serialize)]
struct PeerNameEvent {
    node_id: String,
    display_name: String,
}

// ── Public types ───────────────────────────────────────────────────

/// Name used for our broadcast in every room.
const BROADCAST_NAME: &str = "cam";

/// User-selected devices for a tutoring session.
///
/// All fields are optional — `None` means "use the system default".
#[derive(Debug, Clone, Default)]
pub struct DeviceSelection {
    /// Camera index string (as returned by `list_devices` — e.g. "Index(0)").
    pub camera_index: Option<String>,
    /// Audio input device ID string (from `DeviceId::to_string()`).
    pub mic_device_id: Option<String>,
    /// Audio output device ID string (from `DeviceId::to_string()`).
    pub speaker_device_id: Option<String>,
}

/// Parse a camera index string (Debug-formatted `CameraIndex`) back to the enum.
///
/// Accepts formats: "Index(0)", "String(\"FaceTime HD Camera\")", or a plain
/// number like "0" as a fallback.
fn parse_camera_index(s: &str) -> Option<CameraIndex> {
    if let Some(inner) = s.strip_prefix("Index(").and_then(|s| s.strip_suffix(')')) {
        inner.parse::<u32>().ok().map(CameraIndex::Index)
    } else if let Some(inner) = s.strip_prefix("String(\"").and_then(|s| s.strip_suffix("\")")) {
        Some(CameraIndex::String(inner.to_string()))
    } else if let Ok(n) = s.parse::<u32>() {
        Some(CameraIndex::Index(n))
    } else {
        None
    }
}

/// Peer info as seen from room gossip events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TutoringPeer {
    pub node_id: String,
    pub display_name: Option<String>,
    pub broadcasts: Vec<String>,
    pub connected: bool,
}

/// Status of the active tutoring session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatus {
    pub session_id: String,
    pub session_title: String,
    pub ticket: String,
    pub peers: Vec<TutoringPeer>,
    pub video_enabled: bool,
    pub audio_enabled: bool,
    pub screen_sharing: bool,
    /// Session start time (millis since Unix epoch).
    pub started_at: u64,
}

// ── Internal state ─────────────────────────────────────────────────

/// Tauri event payload for audio level updates.
#[derive(Debug, Clone, Serialize)]
struct AudioLevelEvent {
    /// Mic input level 0.0–1.0.
    mic_level: f32,
    /// Output level 0.0–1.0 (if available from output stream).
    output_level: f32,
}

/// Internal state for an active room.
struct ActiveSession {
    session_id: String,
    session_title: String,
    handle: RoomHandle,
    broadcast: PublishBroadcast,
    audio_ctx: Option<AudioBackend>,
    /// Clone of the mic InputStream — kept for peak metering (VU meter).
    mic_input: Option<InputStream>,
    /// Clone of the OutputStream — kept for output peak metering.
    output_stream: Option<OutputStream>,
    peers: HashMap<String, TutoringPeer>,
    video_enabled: bool,
    audio_enabled: bool,
    screen_sharing: bool,
    /// Chat sender for the derived gossip topic.
    chat_sender: Option<iroh_gossip::api::GossipSender>,
    /// Our node ID (for attributing chat messages).
    our_node_id: String,
    /// Our display name (sent to peers via `/names` gossip).
    our_display_name: String,
    /// Session start time (millis since epoch).
    started_at: u64,
    /// Timestamp of last sent chat message (for rate limiting).
    last_chat_sent: Instant,
    /// AppHandle for emitting Tauri events from toggle methods.
    app_handle: AppHandle,
    /// Handle for the self-preview task (aborted on toggle).
    self_preview_task: Option<JoinHandle<()>>,
    /// User's selected devices — preserved for toggle_video/toggle_audio re-creation.
    device_selection: DeviceSelection,
    /// Background tasks to abort on leave.
    _tasks: Vec<JoinHandle<()>>,
}

// ── TutoringManager ────────────────────────────────────────────────

/// Manages live tutoring rooms.
///
/// Thread-safe via `Arc<Mutex<>>`. Stored in Tauri `AppState`.
pub struct TutoringManager {
    inner: Arc<Mutex<Option<ActiveSession>>>,
}

impl TutoringManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }

    /// Try to create the audio backend (Firewheel + cpal).
    ///
    /// Optionally accepts specific input/output device IDs. Pass `None`
    /// for either to use the system default.
    ///
    /// Returns `None` only if initialization panics (shouldn't happen
    /// with the cpal 0.17.x fix for macOS Sequoia).
    fn try_create_audio_backend(
        input_device_id: Option<DeviceId>,
        output_device_id: Option<DeviceId>,
    ) -> Option<AudioBackend> {
        let result = std::panic::catch_unwind(move || {
            AudioBackend::new_with_devices(input_device_id, output_device_id)
        });
        match result {
            Ok(backend) => {
                log::info!("tutoring: audio backend initialized");
                Some(backend)
            }
            Err(e) => {
                log::error!("tutoring: audio backend panicked during init: {e:?}");
                None
            }
        }
    }

    /// Parse an optional device ID string into a `DeviceId`.
    fn parse_device_id(s: &Option<String>) -> Option<DeviceId> {
        s.as_ref().and_then(|id| id.parse::<DeviceId>().ok())
    }

    fn now_millis() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    // ── Room lifecycle ─────────────────────────────────────────────

    /// Create a new tutoring room (host mode).
    pub async fn create_room(
        &self,
        session_id: String,
        title: String,
        display_name: String,
        endpoint: &Endpoint,
        gossip: Gossip,
        live: Live,
        app_handle: AppHandle,
        devices: DeviceSelection,
    ) -> Result<String, String> {
        let mut inner = self.inner.lock().await;
        if inner.is_some() {
            return Err("already in a tutoring session".into());
        }

        let our_node_id = endpoint.id().to_string();

        let ticket = RoomTicket::generate();
        let room = Room::new(endpoint, gossip.clone(), live, ticket)
            .await
            .map_err(|e| format!("failed to create room: {e}"))?;

        let ticket_str = room.ticket().to_string();

        // Try to initialize audio with user-selected devices
        let mic_id = Self::parse_device_id(&devices.mic_device_id);
        let speaker_id = Self::parse_device_id(&devices.speaker_device_id);
        let audio_ctx = Self::try_create_audio_backend(mic_id, speaker_id);
        let has_audio = audio_ctx.is_some();
        if !has_audio {
            log::warn!("tutoring: proceeding without audio (CoreAudio init failed)");
        }

        // Start publishing local media (using selected camera if any)
        let camera_idx = devices.camera_index.as_deref().and_then(parse_camera_index);
        let (mut broadcast, mic_input) =
            Self::create_broadcast(audio_ctx.as_ref(), true, has_audio, camera_idx).await?;
        room.publish(BROADCAST_NAME, broadcast.producer())
            .await
            .map_err(|e| format!("failed to publish broadcast: {e}"))?;

        let (events, handle) = room.split();

        // Start self-preview from local camera source
        let self_preview_task =
            Self::start_self_preview(&mut broadcast, app_handle.clone());

        // Set up chat on a derived gossip topic
        let topic_seed = room_topic_bytes(&ticket_str);
        let chat_sender = Self::setup_chat(
            &gossip,
            &topic_seed,
            &our_node_id,
            app_handle.clone(),
        )
        .await;

        // Set up name announcements on a derived /names gossip topic
        let names_task = Self::setup_names(
            &gossip,
            &topic_seed,
            &our_node_id,
            &display_name,
            self.inner.clone(),
            app_handle.clone(),
        )
        .await;

        // Spawn event loop to track peers and bridge video frames
        let inner_clone = self.inner.clone();
        let audio_ctx_clone = audio_ctx.clone();
        let app_handle_clone = app_handle.clone();
        let event_task = tokio::spawn(async move {
            Self::event_loop(events, inner_clone, audio_ctx_clone, app_handle_clone).await;
        });

        let mut tasks = vec![event_task];
        if let Some(t) = names_task {
            tasks.push(t);
        }

        *inner = Some(ActiveSession {
            session_id,
            session_title: title,
            handle,
            broadcast,
            audio_ctx,
            mic_input,
            output_stream: None, // Set when first remote broadcast is subscribed
            peers: HashMap::new(),
            video_enabled: true,
            audio_enabled: has_audio,
            screen_sharing: false,
            chat_sender,
            our_node_id,
            our_display_name: display_name,
            started_at: Self::now_millis(),
            last_chat_sent: Instant::now() - Duration::from_secs(10),
            app_handle: app_handle.clone(),
            self_preview_task,
            device_selection: devices,
            _tasks: tasks,
        });

        // Spawn audio level emitter after session is stored (reads from inner)
        let audio_level_task = Self::start_audio_level_emitter(
            self.inner.clone(),
            app_handle,
        );
        if let Some(session) = inner.as_mut() {
            session._tasks.push(audio_level_task);
        }

        Ok(ticket_str)
    }

    /// Join an existing tutoring room using a ticket string.
    pub async fn join_room(
        &self,
        session_id: String,
        title: String,
        display_name: String,
        ticket_str: &str,
        endpoint: &Endpoint,
        gossip: Gossip,
        live: Live,
        app_handle: AppHandle,
        devices: DeviceSelection,
    ) -> Result<String, String> {
        let mut inner = self.inner.lock().await;
        if inner.is_some() {
            return Err("already in a tutoring session".into());
        }

        let our_node_id = endpoint.id().to_string();

        let ticket: RoomTicket = ticket_str
            .parse()
            .map_err(|e| format!("invalid room ticket: {e}"))?;

        let room = Room::new(endpoint, gossip.clone(), live, ticket)
            .await
            .map_err(|e| format!("failed to join room: {e}"))?;

        let ticket_str = room.ticket().to_string();

        // Try to initialize audio with user-selected devices
        let mic_id = Self::parse_device_id(&devices.mic_device_id);
        let speaker_id = Self::parse_device_id(&devices.speaker_device_id);
        let audio_ctx = Self::try_create_audio_backend(mic_id, speaker_id);
        let has_audio = audio_ctx.is_some();
        if !has_audio {
            log::warn!("tutoring: proceeding without audio (CoreAudio init failed)");
        }

        // Start publishing local media (using selected camera if any)
        let camera_idx = devices.camera_index.as_deref().and_then(parse_camera_index);
        let (mut broadcast, mic_input) =
            Self::create_broadcast(audio_ctx.as_ref(), true, has_audio, camera_idx).await?;
        room.publish(BROADCAST_NAME, broadcast.producer())
            .await
            .map_err(|e| format!("failed to publish broadcast: {e}"))?;

        let (events, handle) = room.split();

        // Start self-preview from local camera source
        let self_preview_task =
            Self::start_self_preview(&mut broadcast, app_handle.clone());

        // Set up chat on a derived gossip topic
        let topic_seed = room_topic_bytes(&ticket_str);
        let chat_sender = Self::setup_chat(
            &gossip,
            &topic_seed,
            &our_node_id,
            app_handle.clone(),
        )
        .await;

        // Set up name announcements on a derived /names gossip topic
        let names_task = Self::setup_names(
            &gossip,
            &topic_seed,
            &our_node_id,
            &display_name,
            self.inner.clone(),
            app_handle.clone(),
        )
        .await;

        let inner_clone = self.inner.clone();
        let audio_ctx_clone = audio_ctx.clone();
        let app_handle_clone = app_handle.clone();
        let event_task = tokio::spawn(async move {
            Self::event_loop(events, inner_clone, audio_ctx_clone, app_handle_clone).await;
        });

        let mut tasks = vec![event_task];
        if let Some(t) = names_task {
            tasks.push(t);
        }

        *inner = Some(ActiveSession {
            session_id,
            session_title: title,
            handle,
            broadcast,
            audio_ctx,
            mic_input,
            output_stream: None, // Set when first remote broadcast is subscribed
            peers: HashMap::new(),
            video_enabled: true,
            audio_enabled: has_audio,
            screen_sharing: false,
            chat_sender,
            our_node_id,
            our_display_name: display_name,
            started_at: Self::now_millis(),
            last_chat_sent: Instant::now() - Duration::from_secs(10),
            app_handle: app_handle.clone(),
            self_preview_task,
            device_selection: devices,
            _tasks: tasks,
        });

        // Spawn audio level emitter after session is stored (reads from inner)
        let audio_level_task = Self::start_audio_level_emitter(
            self.inner.clone(),
            app_handle,
        );
        if let Some(session) = inner.as_mut() {
            session._tasks.push(audio_level_task);
        }

        Ok(ticket_str)
    }

    /// Leave the current room.
    pub async fn leave_room(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().await;
        let session = inner.take().ok_or("not in a tutoring session")?;

        // Abort all background tasks
        if let Some(t) = &session.self_preview_task {
            t.abort();
        }
        for task in &session._tasks {
            task.abort();
        }
        drop(session);

        log::info!("left tutoring session");
        Ok(())
    }

    // ── Media controls ─────────────────────────────────────────────

    /// Toggle local camera on/off.
    pub async fn toggle_video(&self, enable: bool) -> Result<bool, String> {
        let mut inner = self.inner.lock().await;
        let session = inner.as_mut().ok_or("not in a tutoring session")?;

        if enable == session.video_enabled && !session.screen_sharing {
            return Ok(session.video_enabled);
        }

        // Abort old self-preview
        if let Some(t) = session.self_preview_task.take() {
            t.abort();
        }

        if enable {
            // If screen sharing is active, switch back to camera
            if session.screen_sharing {
                session.screen_sharing = false;
            }
            let camera_idx = session.device_selection.camera_index.as_deref().and_then(parse_camera_index);
            match CameraCapturer::with_index(camera_idx) {
                Ok(camera) => {
                    let renditions =
                        VideoRenditions::new::<H264Encoder>(camera, VideoPreset::all());
                    session
                        .broadcast
                        .set_video(Some(renditions))
                        .map_err(|e| format!("failed to enable video: {e}"))?;
                    session.video_enabled = true;
                    // Restart self-preview
                    session.self_preview_task = Self::start_self_preview(
                        &mut session.broadcast,
                        session.app_handle.clone(),
                    );
                }
                Err(e) => {
                    log::warn!("tutoring: camera unavailable: {e}");
                    return Err(format!("camera unavailable: {e}"));
                }
            }
        } else {
            session
                .broadcast
                .set_video(None)
                .map_err(|e| format!("failed to disable video: {e}"))?;
            session.video_enabled = false;
            // Emit a "self" video ended so frontend can show placeholder
            let _ = session.app_handle.emit(
                "tutoring:peer-video-ended",
                PeerVideoEndedEvent {
                    node_id: "self".into(),
                },
            );
        }

        Ok(session.video_enabled)
    }

    /// Toggle local microphone on/off.
    pub async fn toggle_audio(&self, enable: bool) -> Result<bool, String> {
        let mut inner = self.inner.lock().await;
        let session = inner.as_mut().ok_or("not in a tutoring session")?;

        if enable == session.audio_enabled {
            return Ok(session.audio_enabled);
        }

        if enable {
            if let Some(ref ctx) = session.audio_ctx {
                match ctx.default_input().await {
                    Ok(mic) => {
                        session.mic_input = Some(mic.clone());
                        let renditions =
                            AudioRenditions::new::<OpusEncoder>(mic, [AudioPreset::Hq]);
                        session
                            .broadcast
                            .set_audio(Some(renditions))
                            .map_err(|e| format!("failed to enable audio: {e}"))?;
                        session.audio_enabled = true;
                    }
                    Err(e) => {
                        return Err(format!("microphone unavailable: {e}"));
                    }
                }
            } else {
                return Err("audio backend not available (CoreAudio workaround)".into());
            }
        } else {
            session
                .broadcast
                .set_audio(None)
                .map_err(|e| format!("failed to disable audio: {e}"))?;
            session.audio_enabled = false;
            session.mic_input = None;
        }

        Ok(session.audio_enabled)
    }

    /// Toggle screen sharing on/off.
    ///
    /// Screen sharing replaces the camera video track. When disabled,
    /// the camera is restored if it was previously enabled.
    pub async fn toggle_screen_share(&self, enable: bool) -> Result<bool, String> {
        let mut inner = self.inner.lock().await;
        let session = inner.as_mut().ok_or("not in a tutoring session")?;

        if enable == session.screen_sharing {
            return Ok(session.screen_sharing);
        }

        // Abort old self-preview
        if let Some(t) = session.self_preview_task.take() {
            t.abort();
        }

        if enable {
            match ScreenCapturer::new() {
                Ok(screen) => {
                    let renditions =
                        VideoRenditions::new::<H264Encoder>(screen, VideoPreset::all());
                    session
                        .broadcast
                        .set_video(Some(renditions))
                        .map_err(|e| format!("failed to start screen share: {e}"))?;
                    session.screen_sharing = true;
                    session.video_enabled = true;
                    // Self-preview now shows screen share
                    session.self_preview_task = Self::start_self_preview(
                        &mut session.broadcast,
                        session.app_handle.clone(),
                    );
                }
                Err(e) => {
                    return Err(format!("screen capture unavailable: {e}"));
                }
            }
        } else {
            // Stop screen share, restore camera (using stored device selection)
            session.screen_sharing = false;
            let camera_idx = session.device_selection.camera_index.as_deref().and_then(parse_camera_index);
            match CameraCapturer::with_index(camera_idx) {
                Ok(camera) => {
                    let renditions =
                        VideoRenditions::new::<H264Encoder>(camera, VideoPreset::all());
                    session
                        .broadcast
                        .set_video(Some(renditions))
                        .map_err(|e| format!("failed to restore camera: {e}"))?;
                    session.video_enabled = true;
                    session.self_preview_task = Self::start_self_preview(
                        &mut session.broadcast,
                        session.app_handle.clone(),
                    );
                }
                Err(_) => {
                    let _ = session.broadcast.set_video(None);
                    session.video_enabled = false;
                    let _ = session.app_handle.emit(
                        "tutoring:peer-video-ended",
                        PeerVideoEndedEvent {
                            node_id: "self".into(),
                        },
                    );
                }
            }
        }

        Ok(session.screen_sharing)
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

        let mut inner = self.inner.lock().await;
        let session = inner.as_mut().ok_or("not in a tutoring session")?;

        // Rate limit
        let now = Instant::now();
        let elapsed = now.duration_since(session.last_chat_sent).as_millis() as u64;
        if elapsed < CHAT_RATE_LIMIT_MS {
            return Err("sending too fast, please wait".into());
        }
        session.last_chat_sent = now;

        let sender = session
            .chat_sender
            .as_ref()
            .ok_or("chat not available")?;

        let msg = ChatMessage {
            sender: session.our_node_id.clone(),
            sender_name: Some(session.our_display_name.clone()),
            text,
            timestamp: Self::now_millis(),
        };

        let encoded = postcard::to_stdvec(&msg)
            .map_err(|e| format!("failed to encode chat: {e}"))?;

        sender
            .broadcast(Bytes::from(encoded))
            .await
            .map_err(|e| format!("failed to send chat: {e}"))?;

        Ok(())
    }

    // ── Query ──────────────────────────────────────────────────────

    /// Get the current session status.
    pub async fn status(&self) -> Option<SessionStatus> {
        let inner = self.inner.lock().await;
        let session = inner.as_ref()?;

        Some(SessionStatus {
            session_id: session.session_id.clone(),
            session_title: session.session_title.clone(),
            ticket: session.handle.ticket().to_string(),
            peers: session.peers.values().cloned().collect(),
            video_enabled: session.video_enabled,
            audio_enabled: session.audio_enabled,
            screen_sharing: session.screen_sharing,
            started_at: session.started_at,
        })
    }

    /// Get list of peers in the current room.
    pub async fn peers(&self) -> Vec<TutoringPeer> {
        let inner = self.inner.lock().await;
        match inner.as_ref() {
            Some(session) => session.peers.values().cloned().collect(),
            None => vec![],
        }
    }

    /// Check if currently in a session.
    pub async fn is_active(&self) -> bool {
        let inner = self.inner.lock().await;
        inner.is_some()
    }

    // ── Internal helpers ───────────────────────────────────────────

    /// Create a PublishBroadcast with camera + mic.
    ///
    /// Returns `(broadcast, mic_input)` — the mic_input is a clone of the
    /// InputStream that can be used for peak metering (VU meters).
    ///
    /// `camera_index` selects a specific camera; `None` uses the default.
    async fn create_broadcast(
        audio_ctx: Option<&AudioBackend>,
        video: bool,
        audio: bool,
        camera_index: Option<CameraIndex>,
    ) -> Result<(PublishBroadcast, Option<InputStream>), String> {
        let mut broadcast = PublishBroadcast::new();
        let mut mic_input: Option<InputStream> = None;

        if audio {
            if let Some(ctx) = audio_ctx {
                match ctx.default_input().await {
                    Ok(mic) => {
                        // Clone the input stream so we can monitor peak levels
                        mic_input = Some(mic.clone());
                        let audio_renditions =
                            AudioRenditions::new::<OpusEncoder>(mic, [AudioPreset::Hq]);
                        broadcast
                            .set_audio(Some(audio_renditions))
                            .map_err(|e| format!("failed to set audio: {e}"))?;
                    }
                    Err(e) => {
                        log::warn!(
                            "tutoring: microphone unavailable, continuing without audio: {e}"
                        );
                    }
                }
            }
        }

        if video {
            match CameraCapturer::with_index(camera_index) {
                Ok(camera) => {
                    let video_renditions =
                        VideoRenditions::new::<H264Encoder>(camera, VideoPreset::all());
                    broadcast
                        .set_video(Some(video_renditions))
                        .map_err(|e| format!("failed to set video: {e}"))?;
                }
                Err(e) => {
                    log::warn!("tutoring: camera unavailable, continuing without video: {e}");
                }
            }
        }

        Ok((broadcast, mic_input))
    }

    /// Start a self-preview from the broadcast's local video source.
    ///
    /// Uses `PublishBroadcast::watch_local()` which taps the shared
    /// camera source directly (no encode/decode round-trip). Frames
    /// are emitted as `tutoring:video-frame` events with `node_id = "self"`.
    fn start_self_preview(
        broadcast: &mut PublishBroadcast,
        app_handle: AppHandle,
    ) -> Option<JoinHandle<()>> {
        let config = DecodeConfig::default();
        let watch = broadcast.watch_local(config)?;

        log::info!("tutoring: starting self-preview");
        Some(Self::spawn_frame_bridge(watch, "self".into(), app_handle))
    }

    /// Spawn a background task that periodically reads mic + output peak levels
    /// and emits `tutoring:audio-level` Tauri events for the frontend VU meters.
    fn start_audio_level_emitter(
        inner: Arc<Mutex<Option<ActiveSession>>>,
        app_handle: AppHandle,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(50));
            loop {
                interval.tick().await;
                let (mic_level, output_level) = {
                    let guard = inner.lock().await;
                    match guard.as_ref() {
                        Some(session) => {
                            let mic = session
                                .mic_input
                                .as_ref()
                                .map(|m| m.smoothed_peak_normalized())
                                .unwrap_or(0.0);
                            let out = session
                                .output_stream
                                .as_ref()
                                .and_then(|o| o.smoothed_peak_normalized())
                                .unwrap_or(0.0);
                            (mic, out)
                        }
                        None => break, // Session ended, stop emitting
                    }
                };
                let _ = app_handle.emit(
                    "tutoring:audio-level",
                    AudioLevelEvent {
                        mic_level,
                        output_level,
                    },
                );
            }
        })
    }

    /// Get the current mic peak level (0.0–1.0) if in a session with audio.
    pub async fn get_mic_level(&self) -> f32 {
        let inner = self.inner.lock().await;
        match inner.as_ref().and_then(|s| s.mic_input.as_ref()) {
            Some(mic) => mic.smoothed_peak_normalized(),
            None => 0.0,
        }
    }

    /// Set up a chat channel on a gossip topic derived from the room.
    async fn setup_chat(
        gossip: &Gossip,
        topic_seed: &[u8],
        our_node_id: &str,
        app_handle: AppHandle,
    ) -> Option<iroh_gossip::api::GossipSender> {
        use iroh_gossip::proto::TopicId;

        // Derive a chat topic from the room topic by hashing with a "chat" suffix
        let mut hasher = blake3::Hasher::new();
        hasher.update(topic_seed);
        hasher.update(b"/chat");
        let hash = hasher.finalize();
        let topic_id = TopicId::from_bytes(*hash.as_bytes());

        match gossip.subscribe(topic_id, vec![]).await {
            Ok(topic) => {
                let (sender, mut receiver) = topic.split();
                let our_id = our_node_id.to_string();

                // Spawn task to receive chat messages and forward to webview
                tokio::spawn(async move {
                    use futures::StreamExt;
                    while let Some(Ok(event)) = receiver.next().await {
                        if let iroh_gossip::api::Event::Received(msg) = event {
                            match postcard::from_bytes::<ChatMessage>(&msg.content) {
                                Ok(chat_msg) => {
                                    // Don't echo our own messages
                                    if chat_msg.sender == our_id {
                                        continue;
                                    }
                                    let _ = app_handle.emit(
                                        "tutoring:chat",
                                        ChatMessageEvent {
                                            sender: chat_msg.sender,
                                            sender_name: chat_msg.sender_name,
                                            text: chat_msg.text,
                                            timestamp: chat_msg.timestamp,
                                        },
                                    );
                                }
                                Err(e) => {
                                    log::warn!("tutoring: failed to decode chat message: {e}");
                                }
                            }
                        }
                    }
                    log::info!("tutoring: chat receiver loop ended");
                });

                Some(sender)
            }
            Err(e) => {
                log::warn!("tutoring: failed to set up chat gossip topic: {e}");
                None
            }
        }
    }

    /// Set up a display name exchange channel on a `/names` gossip topic.
    ///
    /// Periodically broadcasts our display name so new joiners learn it,
    /// and listens for name announcements from other peers, updating the
    /// peers map and emitting `tutoring:peer-name` events.
    async fn setup_names(
        gossip: &Gossip,
        topic_seed: &[u8],
        our_node_id: &str,
        our_display_name: &str,
        inner: Arc<Mutex<Option<ActiveSession>>>,
        app_handle: AppHandle,
    ) -> Option<JoinHandle<()>> {
        use iroh_gossip::proto::TopicId;

        let mut hasher = blake3::Hasher::new();
        hasher.update(topic_seed);
        hasher.update(b"/names");
        let hash = hasher.finalize();
        let topic_id = TopicId::from_bytes(*hash.as_bytes());

        match gossip.subscribe(topic_id, vec![]).await {
            Ok(topic) => {
                let (sender, mut receiver) = topic.split();
                let our_id = our_node_id.to_string();
                let our_name = our_display_name.to_string();

                // Spawn a task that:
                // 1. Broadcasts our name immediately and every N seconds
                // 2. Listens for name announcements from peers
                let task = tokio::spawn(async move {
                    // Broadcast our name immediately
                    let announce = NameAnnouncement {
                        node_id: our_id.clone(),
                        display_name: our_name.clone(),
                    };
                    if let Ok(encoded) = postcard::to_stdvec(&announce) {
                        let _ = sender.broadcast(Bytes::from(encoded)).await;
                    }

                    let mut broadcast_interval =
                        tokio::time::interval(Duration::from_secs(NAME_BROADCAST_INTERVAL_SECS));
                    broadcast_interval.tick().await; // consume first immediate tick

                    loop {
                        tokio::select! {
                            _ = broadcast_interval.tick() => {
                                let announce = NameAnnouncement {
                                    node_id: our_id.clone(),
                                    display_name: our_name.clone(),
                                };
                                if let Ok(encoded) = postcard::to_stdvec(&announce) {
                                    let _ = sender.broadcast(Bytes::from(encoded)).await;
                                }
                            }
                            msg = async {
                                use futures::StreamExt;
                                receiver.next().await
                            } => {
                                match msg {
                                    Some(Ok(iroh_gossip::api::Event::Received(msg))) => {
                                        if let Ok(announce) = postcard::from_bytes::<NameAnnouncement>(&msg.content) {
                                            // Skip our own announcements
                                            if announce.node_id == our_id {
                                                continue;
                                            }
                                            log::info!(
                                                "tutoring: learned peer name: {} = {}",
                                                &announce.node_id[..8.min(announce.node_id.len())],
                                                announce.display_name
                                            );

                                            // Update the peers map
                                            let mut guard = inner.lock().await;
                                            if let Some(session) = guard.as_mut() {
                                                session
                                                    .peers
                                                    .entry(announce.node_id.clone())
                                                    .and_modify(|p| {
                                                        p.display_name = Some(announce.display_name.clone());
                                                    })
                                                    .or_insert(TutoringPeer {
                                                        node_id: announce.node_id.clone(),
                                                        display_name: Some(announce.display_name.clone()),
                                                        broadcasts: vec![],
                                                        connected: false,
                                                    });
                                            }

                                            // Emit to frontend
                                            let _ = app_handle.emit(
                                                "tutoring:peer-name",
                                                PeerNameEvent {
                                                    node_id: announce.node_id,
                                                    display_name: announce.display_name,
                                                },
                                            );
                                        }
                                    }
                                    Some(Ok(_)) => {} // other gossip events
                                    Some(Err(e)) => {
                                        log::warn!("tutoring: names gossip error: {e}");
                                    }
                                    None => break, // topic closed
                                }
                            }
                        }
                    }
                    log::info!("tutoring: names loop ended");
                });

                Some(task)
            }
            Err(e) => {
                log::warn!("tutoring: failed to set up names gossip topic: {e}");
                None
            }
        }
    }

    /// Event loop that processes room events (peer announcements,
    /// connections, broadcast subscriptions) and spawns video frame
    /// bridge tasks for each subscribed remote broadcast.
    async fn event_loop(
        mut events: mpsc::Receiver<RoomEvent>,
        inner: Arc<Mutex<Option<ActiveSession>>>,
        audio_ctx: Option<AudioBackend>,
        app_handle: AppHandle,
    ) {
        while let Some(event) = events.recv().await {
            match event {
                RoomEvent::RemoteAnnounced {
                    remote,
                    broadcasts,
                } => {
                    let node_id = remote.to_string();
                    log::info!("tutoring: peer announced: {node_id} with {broadcasts:?}");

                    let mut guard = inner.lock().await;
                    if let Some(session) = guard.as_mut() {
                        session
                            .peers
                            .entry(node_id.clone())
                            .and_modify(|p| {
                                p.broadcasts = broadcasts.clone();
                            })
                            .or_insert(TutoringPeer {
                                node_id,
                                display_name: None,
                                broadcasts,
                                connected: false,
                            });
                    }
                }
                RoomEvent::RemoteConnected {
                    session: moq_session,
                } => {
                    let node_id = moq_session.conn().remote_id().to_string();
                    log::info!("tutoring: peer connected: {node_id}");

                    let mut guard = inner.lock().await;
                    if let Some(session) = guard.as_mut() {
                        session
                            .peers
                            .entry(node_id.clone())
                            .and_modify(|p| {
                                p.connected = true;
                            })
                            .or_insert(TutoringPeer {
                                node_id,
                                display_name: None,
                                broadcasts: vec![],
                                connected: true,
                            });
                    }
                }
                RoomEvent::BroadcastSubscribed {
                    session: moq_session,
                    broadcast,
                } => {
                    let node_id = moq_session.remote_id().to_string();
                    let name = broadcast.broadcast_name().to_string();
                    log::info!("tutoring: subscribed to {node_id}:{name}");

                    // Start audio playback if available
                    let audio_out = match &audio_ctx {
                        Some(ctx) => match ctx.default_output().await {
                            Ok(out) => Some(out),
                            Err(e) => {
                                log::warn!("tutoring: audio output unavailable: {e}");
                                None
                            }
                        },
                        None => None,
                    };

                    if let Some(audio_out) = audio_out {
                        // Clone the output stream for peak metering before passing to watch_and_listen
                        let output_clone = audio_out.clone();

                        // Audio + video: use watch_and_listen
                        match broadcast
                            .watch_and_listen::<FfmpegDecoders>(audio_out, Default::default())
                        {
                            Ok(av_track) => {
                                log::info!(
                                    "tutoring: watching + listening to {node_id}:{name}"
                                );
                                // Store output stream clone for peak metering
                                {
                                    let mut guard = inner.lock().await;
                                    if let Some(session) = guard.as_mut() {
                                        session.output_stream = Some(output_clone);
                                    }
                                }
                                if let Some(video) = av_track.video {
                                    Self::spawn_frame_bridge(
                                        video,
                                        node_id.clone(),
                                        app_handle.clone(),
                                    );
                                }
                            }
                            Err(e) => {
                                log::error!(
                                    "tutoring: failed to watch_and_listen {node_id}:{name}: {e}"
                                );
                            }
                        }
                    } else {
                        // Video only: use watch()
                        match broadcast.watch::<FfmpegVideoDecoder>() {
                            Ok(video) => {
                                log::info!(
                                    "tutoring: watching video from {node_id}:{name}"
                                );
                                Self::spawn_frame_bridge(
                                    video,
                                    node_id.clone(),
                                    app_handle.clone(),
                                );
                            }
                            Err(e) => {
                                log::error!(
                                    "tutoring: failed to watch {node_id}:{name}: {e}"
                                );
                            }
                        }
                    }
                }
            }
        }
        log::info!("tutoring: event loop ended");
    }

    /// Spawn a background task that polls decoded video frames from a
    /// `WatchTrack` (either remote peer or local preview), encodes
    /// them as JPEG, and emits them to the webview via a Tauri event.
    ///
    /// When the track closes (peer left or video disabled), emits a
    /// `tutoring:peer-video-ended` event so the frontend can clean up.
    fn spawn_frame_bridge(
        watch: WatchTrack,
        node_id: String,
        app_handle: AppHandle,
    ) -> JoinHandle<()> {
        let (mut frames, handle) = watch.split();
        // Set a reasonable viewport — the decoder will scale down if needed
        handle.set_viewport(640, 480);

        tokio::spawn(async move {
            // IMPORTANT: Keep `handle` alive for the duration of this task.
            // It holds the shutdown token guard and thread handle — dropping
            // it cancels the video source thread and closes the frame channel.
            let _handle = handle;
            log::info!("tutoring: frame bridge started for {node_id}");

            // Target ~15 fps to avoid overwhelming the webview with events
            let frame_interval = Duration::from_millis(66);
            let mut last_emit = std::time::Instant::now();

            loop {
                match frames.next_frame().await {
                    Some(frame) => {
                        // Rate-limit to ~15 fps
                        let now = std::time::Instant::now();
                        if now.duration_since(last_emit) < frame_interval {
                            continue;
                        }
                        last_emit = now;

                        let img = frame.img();
                        let (width, height) = img.dimensions();

                        // Encode to JPEG (quality 60 for good size/quality tradeoff)
                        let mut jpeg_buf = Vec::with_capacity((width * height) as usize / 4);
                        let mut cursor = Cursor::new(&mut jpeg_buf);
                        let encoder =
                            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 60);
                        match encoder.write_image(
                            img.as_raw(),
                            width,
                            height,
                            image::ExtendedColorType::Rgba8,
                        ) {
                            Ok(()) => {}
                            Err(e) => {
                                log::warn!("tutoring: JPEG encode failed for {node_id}: {e}");
                                continue;
                            }
                        }

                        let jpeg_b64 =
                            base64::engine::general_purpose::STANDARD.encode(&jpeg_buf);

                        let _ = app_handle.emit(
                            "tutoring:video-frame",
                            VideoFrameEvent {
                                node_id: node_id.clone(),
                                jpeg_b64,
                                width,
                                height,
                            },
                        );
                    }
                    None => {
                        log::info!("tutoring: frame bridge ended for {node_id} (track closed)");
                        // Notify frontend that this peer's video has ended
                        let _ = app_handle.emit(
                            "tutoring:peer-video-ended",
                            PeerVideoEndedEvent {
                                node_id: node_id.clone(),
                            },
                        );
                        break;
                    }
                }
            }
        })
    }
}

/// Derive stable bytes from a ticket string for topic derivation.
fn room_topic_bytes(ticket_str: &str) -> Vec<u8> {
    ticket_str.as_bytes().to_vec()
}
