//! TutoringManager — room lifecycle and media capture management.
//!
//! Manages creation/joining of iroh-live rooms, local media publishing
//! (camera, mic, screen share), and bridges decoded remote video frames
//! to the webview via Tauri events.

use std::collections::HashMap;
use std::sync::Arc;

use iroh::Endpoint;
use iroh_gossip::Gossip;
use iroh_live::rooms::{Room, RoomEvent, RoomHandle, RoomTicket};
use iroh_live::Live;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, mpsc};

use iroh_live::media::audio::AudioBackend;
use iroh_live::media::av::AudioPreset;
use iroh_live::media::capture::CameraCapturer;
use iroh_live::media::ffmpeg::{FfmpegDecoders, H264Encoder, OpusEncoder};
use iroh_live::media::publish::{AudioRenditions, PublishBroadcast, VideoRenditions};
use iroh_live::media::av::VideoPreset;

// ScreenCapturer will be used in toggle_screen_share (Phase 1.1)
#[allow(unused_imports)]
use iroh_live::media::capture::ScreenCapturer;

/// Name used for our broadcast in every room.
const BROADCAST_NAME: &str = "cam";

/// Peer info as seen from room gossip events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TutoringPeer {
    pub node_id: String,
    pub broadcasts: Vec<String>,
    pub connected: bool,
}

/// Status of the active tutoring session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatus {
    pub session_id: String,
    pub ticket: String,
    pub peers: Vec<TutoringPeer>,
    pub video_enabled: bool,
    pub audio_enabled: bool,
    pub screen_sharing: bool,
}

/// Internal state for an active room.
struct ActiveSession {
    session_id: String,
    handle: RoomHandle,
    #[allow(dead_code)]
    broadcast: PublishBroadcast,
    #[allow(dead_code)]
    audio_ctx: Option<AudioBackend>,
    peers: HashMap<String, TutoringPeer>,
    video_enabled: bool,
    audio_enabled: bool,
    screen_sharing: bool,
    _event_task: tokio::task::JoinHandle<()>,
}

/// Manages live tutoring rooms.
///
/// Thread-safe via `Arc<Mutex<>>`. Stored in Tauri `AppState`.
///
/// The audio backend is lazily initialized when a session starts.
/// If CoreAudio initialization fails (known crash on some macOS
/// configurations), the session continues with video-only.
pub struct TutoringManager {
    inner: Arc<Mutex<Option<ActiveSession>>>,
}

impl TutoringManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }

    /// Audio is currently disabled (Phase 1.0) due to a CoreAudio
    /// segfault in cpal's device enumeration on macOS Sequoia+.
    ///
    /// The crash occurs inside Apple's `HALDeviceList::GetData` when
    /// `AudioBackend::new()` enumerates audio devices — this is not
    /// catchable via `catch_unwind` since it's a SIGSEGV, not a panic.
    ///
    /// Audio will be re-enabled once the upstream cpal/firewheel fix
    /// lands or we add a CoreAudio main-thread trampoline.
    ///
    /// TODO: Re-enable audio when cpal CoreAudio crash is resolved.
    fn try_create_audio_backend() -> Option<AudioBackend> {
        log::info!("tutoring: audio disabled (CoreAudio segfault workaround)");
        None
    }

    /// Create a new tutoring room (host mode).
    ///
    /// Generates a fresh topic, starts camera + mic capture, and
    /// publishes to the room. Returns the serialized room ticket
    /// that others can use to join.
    pub async fn create_room(
        &self,
        session_id: String,
        endpoint: &Endpoint,
        gossip: Gossip,
        live: Live,
    ) -> Result<String, String> {
        let mut inner = self.inner.lock().await;
        if inner.is_some() {
            return Err("already in a tutoring session".into());
        }

        let ticket = RoomTicket::generate();
        let room = Room::new(endpoint, gossip, live, ticket)
            .await
            .map_err(|e| format!("failed to create room: {e}"))?;

        let ticket_str = room.ticket().to_string();

        // Try to initialize audio (may fail on some macOS systems)
        let audio_ctx = Self::try_create_audio_backend();
        let has_audio = audio_ctx.is_some();
        if !has_audio {
            log::warn!("tutoring: proceeding without audio (CoreAudio init failed)");
        }

        // Start publishing local media (video always, audio if available)
        let broadcast =
            Self::create_broadcast(audio_ctx.as_ref(), true, has_audio).await?;
        room.publish(BROADCAST_NAME, broadcast.producer())
            .await
            .map_err(|e| format!("failed to publish broadcast: {e}"))?;

        let (events, handle) = room.split();

        // Spawn event loop to track peers
        let inner_clone = self.inner.clone();
        let audio_ctx_clone = audio_ctx.clone();
        let event_task = tokio::spawn(async move {
            Self::event_loop(events, inner_clone, audio_ctx_clone).await;
        });

        *inner = Some(ActiveSession {
            session_id,
            handle,
            broadcast,
            audio_ctx,
            peers: HashMap::new(),
            video_enabled: true,
            audio_enabled: has_audio,
            screen_sharing: false,
            _event_task: event_task,
        });

        Ok(ticket_str)
    }

    /// Join an existing tutoring room using a ticket string.
    pub async fn join_room(
        &self,
        session_id: String,
        ticket_str: &str,
        endpoint: &Endpoint,
        gossip: Gossip,
        live: Live,
    ) -> Result<String, String> {
        let mut inner = self.inner.lock().await;
        if inner.is_some() {
            return Err("already in a tutoring session".into());
        }

        let ticket: RoomTicket = ticket_str
            .parse()
            .map_err(|e| format!("invalid room ticket: {e}"))?;

        let room = Room::new(endpoint, gossip, live, ticket)
            .await
            .map_err(|e| format!("failed to join room: {e}"))?;

        let ticket_str = room.ticket().to_string();

        // Try to initialize audio (may fail on some macOS systems)
        let audio_ctx = Self::try_create_audio_backend();
        let has_audio = audio_ctx.is_some();
        if !has_audio {
            log::warn!("tutoring: proceeding without audio (CoreAudio init failed)");
        }

        // Start publishing local media
        let broadcast =
            Self::create_broadcast(audio_ctx.as_ref(), true, has_audio).await?;
        room.publish(BROADCAST_NAME, broadcast.producer())
            .await
            .map_err(|e| format!("failed to publish broadcast: {e}"))?;

        let (events, handle) = room.split();

        let inner_clone = self.inner.clone();
        let audio_ctx_clone = audio_ctx.clone();
        let event_task = tokio::spawn(async move {
            Self::event_loop(events, inner_clone, audio_ctx_clone).await;
        });

        *inner = Some(ActiveSession {
            session_id,
            handle,
            broadcast,
            audio_ctx,
            peers: HashMap::new(),
            video_enabled: true,
            audio_enabled: has_audio,
            screen_sharing: false,
            _event_task: event_task,
        });

        Ok(ticket_str)
    }

    /// Leave the current room.
    pub async fn leave_room(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().await;
        let session = inner.take().ok_or("not in a tutoring session")?;

        // Dropping the handle + broadcast + audio_ctx will close the room
        session._event_task.abort();
        drop(session);

        log::info!("left tutoring session");
        Ok(())
    }

    /// Get the current session status.
    pub async fn status(&self) -> Option<SessionStatus> {
        let inner = self.inner.lock().await;
        let session = inner.as_ref()?;

        Some(SessionStatus {
            session_id: session.session_id.clone(),
            ticket: session.handle.ticket().to_string(),
            peers: session.peers.values().cloned().collect(),
            video_enabled: session.video_enabled,
            audio_enabled: session.audio_enabled,
            screen_sharing: session.screen_sharing,
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

    // ---- Internal helpers ----

    async fn create_broadcast(
        audio_ctx: Option<&AudioBackend>,
        video: bool,
        audio: bool,
    ) -> Result<PublishBroadcast, String> {
        let mut broadcast = PublishBroadcast::new();

        if audio {
            if let Some(ctx) = audio_ctx {
                match ctx.default_input().await {
                    Ok(mic) => {
                        let audio_renditions =
                            AudioRenditions::new::<OpusEncoder>(mic, [AudioPreset::Hq]);
                        broadcast
                            .set_audio(Some(audio_renditions))
                            .map_err(|e| format!("failed to set audio: {e}"))?;
                    }
                    Err(e) => {
                        log::warn!("tutoring: microphone unavailable, continuing without audio: {e}");
                    }
                }
            }
        }

        if video {
            match CameraCapturer::new() {
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

        Ok(broadcast)
    }

    async fn event_loop(
        mut events: mpsc::Receiver<RoomEvent>,
        inner: Arc<Mutex<Option<ActiveSession>>>,
        audio_ctx: Option<AudioBackend>,
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
                                broadcasts,
                                connected: false,
                            });
                    }
                }
                RoomEvent::RemoteConnected { session: moq_session } => {
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

                    // Start watching + listening to the remote broadcast.
                    // Audio goes to default output if available; video
                    // frames will be polled by the frontend via Tauri events.
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
                        match broadcast
                            .watch_and_listen::<FfmpegDecoders>(
                                audio_out,
                                Default::default(),
                            )
                        {
                            Ok(_track) => {
                                log::info!(
                                    "tutoring: watching + listening to {node_id}:{name}"
                                );
                            }
                            Err(e) => {
                                log::error!(
                                    "tutoring: failed to watch {node_id}:{name}: {e}"
                                );
                            }
                        }
                    } else {
                        log::info!(
                            "tutoring: skipping audio playback for {node_id}:{name} (no audio backend)"
                        );
                    }
                }
            }
        }
        log::info!("tutoring: event loop ended");
    }
}
