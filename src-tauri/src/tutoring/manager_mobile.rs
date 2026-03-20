//! Mobile TutoringManager with video support (Phase 3).
//!
//! Uses `VtEncoder` (VideoToolbox H.264) for encoding camera frames,
//! `IosCameraSource` (AVCaptureSession) for camera capture, and
//! `IosDecoders` (`PureOpusDecoder` + `VtDecoder`) for decoding
//! remote audio+video streams.
//!
//! Video frames from remote peers (and local preview) are encoded as
//! JPEG, base64'd, and emitted to the webview via Tauri events —
//! same bridge pattern as the desktop manager.
//!
//! ## Main-thread requirement
//!
//! AVFoundation (AVCaptureSession, AVAudioSession) and CoreAudio APIs
//! must be called from the main thread (or a thread with an active run
//! loop). Since Tauri commands execute on a tokio worker thread, we use
//! GCD's `dispatch_sync_f(dispatch_get_main_queue(), ...)` to forward
//! these calls to the main thread, blocking until they complete.
//!
//! Phase 3: Full audio+video P2P tutoring on iOS.

use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use bytes::Bytes;
use image::ImageEncoder;
use iroh::{Endpoint, EndpointId};
use iroh_gossip::net::Gossip;
use iroh_live::media::audio::{AudioBackend, DeviceId, InputStream, OutputStream};
use iroh_live::media::av::{AudioPreset, AudioSinkHandle, VideoPreset};
use iroh_live::media::opus::PureOpusDecoder;
use iroh_live::media::opus::PureOpusEncoder;
use iroh_live::media::publish::{AudioRenditions, PublishBroadcast, VideoRenditions};
use iroh_live::media::subscribe::{SubscribeBroadcast, WatchTrack};
use iroh_live::media::videotoolbox::{IosCameraSource, VtDecoder, VtEncoder};
use iroh_live::moq::MoqSession;
use iroh_live::rooms::{Room, RoomEvent, RoomHandle, RoomTicket};
use iroh_live::Live;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;

// ── GCD main-thread dispatch ───────────────────────────────────────

unsafe extern "C" {
    /// The real symbol behind `dispatch_get_main_queue()` macro.
    /// `dispatch_get_main_queue()` is `#define dispatch_get_main_queue() (&_dispatch_main_q)`
    static _dispatch_main_q: std::ffi::c_void;

    fn dispatch_sync_f(
        queue: *const std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: unsafe extern "C" fn(*mut std::ffi::c_void),
    );
}

unsafe extern "C" {
    /// Returns 1 if the calling thread is the main thread, 0 otherwise.
    fn pthread_main_np() -> std::ffi::c_int;
}

/// Execute a closure synchronously on the main thread via GCD.
///
/// This blocks the calling thread until `f` completes on the main queue.
/// Required for AVFoundation, AVAudioSession, and CoreAudio APIs on iOS.
///
/// # Safety
/// The closure must not panic. If it does, the process will abort.
fn run_on_main_thread<F, R>(f: F) -> R
where
    F: FnOnce() -> R + Send,
    R: Send,
{
    // If we're already on the main thread, just run directly.
    // (Calling dispatch_sync on the main queue from the main thread deadlocks.)
    if unsafe { pthread_main_np() } != 0 {
        return f();
    }

    // Pack the closure and result slot into a context struct on the stack.
    struct Context<F, R> {
        f: Option<F>,
        result: Option<R>,
    }

    let mut ctx = Context {
        f: Some(f),
        result: None,
    };

    unsafe extern "C" fn trampoline<F, R>(raw: *mut std::ffi::c_void)
    where
        F: FnOnce() -> R,
    {
        let ctx = &mut *(raw as *mut Context<F, R>);
        if let Some(f) = ctx.f.take() {
            ctx.result = Some(f());
        }
    }

    unsafe {
        let main_queue = &_dispatch_main_q as *const std::ffi::c_void;
        dispatch_sync_f(
            main_queue,
            &mut ctx as *mut Context<F, R> as *mut std::ffi::c_void,
            trampoline::<F, R>,
        );
    }

    ctx.result.expect("main-thread closure did not produce a result")
}

/// Query `UIApplication.shared.applicationState` on the main thread.
///
/// Returns: 0 = UIApplicationStateActive, 1 = Inactive, 2 = Background.
fn get_application_state() -> i64 {
    run_on_main_thread(|| {
        unsafe {
            type Id = *mut std::ffi::c_void;
            type Sel = *mut std::ffi::c_void;
            unsafe extern "C" {
                fn objc_getClass(name: *const u8) -> Id;
                fn sel_registerName(name: *const u8) -> Sel;
                fn objc_msgSend(receiver: Id, sel: Sel, ...) -> Id;
            }

            type MsgSendNoArgs = unsafe extern "C" fn(Id, Sel) -> Id;
            type MsgSendState = unsafe extern "C" fn(Id, Sel) -> i64;

            let send: MsgSendNoArgs =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
            let send_state: MsgSendState =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);

            let cls = objc_getClass(b"UIApplication\0".as_ptr());
            if cls.is_null() {
                return -1;
            }

            let shared_sel = sel_registerName(b"sharedApplication\0".as_ptr());
            let app: Id = send(cls, shared_sel);
            if app.is_null() {
                return -1;
            }

            let state_sel = sel_registerName(b"applicationState\0".as_ptr());
            send_state(app, state_sel)
        }
    })
}

/// Set `UIApplication.shared.isIdleTimerDisabled` on the main thread.
///
/// When `disabled` is true, the iPhone won't auto-lock during a tutoring session.
fn set_idle_timer_disabled(disabled: bool) {
    run_on_main_thread(move || {
        unsafe {
            type Id = *mut std::ffi::c_void;
            type Sel = *mut std::ffi::c_void;
            unsafe extern "C" {
                fn objc_getClass(name: *const u8) -> Id;
                fn sel_registerName(name: *const u8) -> Sel;
                fn objc_msgSend(receiver: Id, sel: Sel, ...) -> Id;
            }

            type MsgSendNoArgs = unsafe extern "C" fn(Id, Sel) -> Id;
            type MsgSendBool = unsafe extern "C" fn(Id, Sel, i8);

            let send: MsgSendNoArgs =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
            let send_bool: MsgSendBool =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);

            let cls = objc_getClass(b"UIApplication\0".as_ptr());
            if cls.is_null() {
                log::error!("tutoring: UIApplication class not found");
                return;
            }

            let shared_sel = sel_registerName(b"sharedApplication\0".as_ptr());
            let app: Id = send(cls, shared_sel);
            if app.is_null() {
                log::error!("tutoring: UIApplication.sharedApplication returned nil");
                return;
            }

            let set_idle_sel = sel_registerName(b"setIdleTimerDisabled:\0".as_ptr());
            send_bool(app, set_idle_sel, if disabled { 1 } else { 0 });

            log::info!("tutoring: idleTimerDisabled = {disabled}");
        }
    });
}

// ── Constants ──────────────────────────────────────────────────────

/// Maximum chat message text length (bytes).
const CHAT_MAX_LENGTH: usize = 2000;

/// Minimum interval between chat messages from the local user (ms).
const CHAT_RATE_LIMIT_MS: u64 = 200;

/// Interval for re-broadcasting our display name (seconds).
const NAME_BROADCAST_INTERVAL_SECS: u64 = 15;

/// Mobile video presets — use lower resolutions to save battery/bandwidth.
const MOBILE_VIDEO_PRESETS: [VideoPreset; 2] = [VideoPreset::P180, VideoPreset::P360];

// ── Chat message protocol ──────────────────────────────────────────

/// Chat message sent over the gossip channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Sender's iroh node ID (hex).
    pub sender: String,
    /// Display name (optional).
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

/// User-selected audio devices for a tutoring session.
///
/// Mobile: camera is always the default device camera (front preferred).
#[derive(Debug, Clone, Default)]
pub struct DeviceSelection {
    /// Audio input device ID string (from `DeviceId::to_string()`).
    pub mic_device_id: Option<String>,
    /// Audio output device ID string (from `DeviceId::to_string()`).
    pub speaker_device_id: Option<String>,
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
    /// Recent log entries from the tutoring subsystem (ring buffer).
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

// ── Internal state ─────────────────────────────────────────────────

/// Tauri event payload for audio level updates.
#[derive(Debug, Clone, Serialize)]
struct AudioLevelEvent {
    /// Mic input level 0.0–1.0.
    mic_level: f32,
    /// Output level 0.0–1.0 (if available from output stream).
    output_level: f32,
}

/// Maximum number of log entries retained in the ring buffer.
const MAX_LOG_ENTRIES: usize = 50;

/// Internal state for an active room (mobile: audio + video).
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
    /// Handle for the self-preview task (aborted on toggle/leave).
    self_preview_task: Option<JoinHandle<()>>,
    /// User's selected audio devices — preserved for toggle_audio re-creation.
    _device_selection: DeviceSelection,
    /// Background tasks to abort on leave.
    _tasks: Vec<JoinHandle<()>>,
    /// Ring buffer of recent log entries for diagnostics.
    recent_logs: Vec<String>,
    /// Home relay URL at time of session creation.
    home_relay: Option<String>,
    /// MoQ sessions for subscribed broadcasts — kept alive to prevent QUIC close.
    _moq_sessions: Vec<MoqSession>,
    /// SubscribeBroadcast handles — kept alive so the BroadcastConsumer doesn't
    /// drop and close the MoQ track subscriptions (audio/video).
    _subscribe_broadcasts: Vec<SubscribeBroadcast>,
    _subscribed_keys: HashSet<String>,
    remote_broadcasts: Vec<(EndpointId, String)>,
}

// ── TutoringManager ────────────────────────────────────────────────

/// Manages live tutoring rooms (mobile: audio + video via VideoToolbox).
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

    /// Configure the iOS AVAudioSession so Bluetooth devices appear in
    /// cpal enumeration. Called from `tutoring_list_devices` before
    /// device enumeration so AirPods etc. are visible. Idempotent.
    pub fn configure_ios_audio_session_for_devices() {
        Self::configure_ios_audio_session();
    }

    /// Configure the iOS AVAudioSession for play-and-record.
    ///
    /// Must be called before any CoreAudio / AVCaptureSession usage,
    /// otherwise iOS will crash when the mic or speaker is accessed.
    ///
    /// Dispatches to the **main thread** via GCD because AVAudioSession
    /// APIs require it.
    fn configure_ios_audio_session() {
        run_on_main_thread(|| {
            unsafe {
                // ObjC: [[AVAudioSession sharedInstance] setCategory:AVAudioSessionCategoryPlayAndRecord
                //         withOptions:AVAudioSessionCategoryOptionDefaultToSpeaker|AllowBluetooth
                //         error:nil]
                // Then: [[AVAudioSession sharedInstance] setActive:YES error:nil]
                //
                // IMPORTANT: On arm64 iOS, objc_msgSend with C variadic `...` passes
                // extra arguments in different registers than ObjC methods expect.
                // We MUST cast objc_msgSend to the exact function pointer type for
                // each call signature. This is the standard pattern for raw ObjC FFI
                // on arm64.
                type Id = *mut std::ffi::c_void;
                type Sel = *mut std::ffi::c_void;
                unsafe extern "C" {
                    fn objc_getClass(name: *const u8) -> Id;
                    fn sel_registerName(name: *const u8) -> Sel;
                    fn objc_msgSend(receiver: Id, sel: Sel, ...) -> Id;
                }

                // Type aliases for specific objc_msgSend signatures.
                // Each matches the exact parameter list of the ObjC method being called.
                type MsgSendNoArgs = unsafe extern "C" fn(Id, Sel) -> Id;
                type MsgSendOnePtr = unsafe extern "C" fn(Id, Sel, *const u8) -> Id;
                type MsgSendCatOpts =
                    unsafe extern "C" fn(Id, Sel, Id, u64, Id) -> i8; // BOOL return
                type MsgSendMode = unsafe extern "C" fn(Id, Sel, Id, Id) -> i8; // BOOL return
                type MsgSendActivate =
                    unsafe extern "C" fn(Id, Sel, i8, Id) -> i8; // BOOL return

                let send: MsgSendNoArgs =
                    std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
                let send_ptr: MsgSendOnePtr =
                    std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
                let send_cat: MsgSendCatOpts =
                    std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
                let send_mode: MsgSendMode =
                    std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);
                let send_act: MsgSendActivate =
                    std::mem::transmute(objc_msgSend as unsafe extern "C" fn(Id, Sel, ...) -> Id);

                let cls = objc_getClass(b"AVAudioSession\0".as_ptr());
                if cls.is_null() {
                    log::error!("tutoring: AVAudioSession class not found");
                    return;
                }

                // [AVAudioSession sharedInstance]
                let shared_sel = sel_registerName(b"sharedInstance\0".as_ptr());
                let session: Id = send(cls, shared_sel);
                if session.is_null() {
                    log::error!("tutoring: AVAudioSession.sharedInstance returned nil");
                    return;
                }

                // Create NSString @"AVAudioSessionCategoryPlayAndRecord"
                // via [NSString stringWithUTF8String:"AVAudioSessionCategoryPlayAndRecord"]
                let nsstring_cls = objc_getClass(b"NSString\0".as_ptr());
                let utf8_sel = sel_registerName(b"stringWithUTF8String:\0".as_ptr());
                let category: Id = send_ptr(
                    nsstring_cls,
                    utf8_sel,
                    b"AVAudioSessionCategoryPlayAndRecord\0".as_ptr(),
                );
                if category.is_null() {
                    log::error!("tutoring: failed to create category NSString");
                    return;
                }

                // [session setCategory:category withOptions:(DefaultToSpeaker|AllowBluetooth|AllowBluetoothA2DP) error:nil]
                // Options: DefaultToSpeaker (0x02) | AllowBluetooth (0x04) | AllowBluetoothA2DP (0x20)
                let options: u64 = 0x02 | 0x04 | 0x20;
                let set_cat_sel = sel_registerName(b"setCategory:withOptions:error:\0".as_ptr());
                let nil: Id = std::ptr::null_mut();
                let set_cat_ok: i8 = send_cat(session, set_cat_sel, category, options, nil);
                if set_cat_ok == 0 {
                    crate::diag::log("configure_ios_audio_session: setCategory failed");
                } else {
                    crate::diag::log("configure_ios_audio_session: setCategory OK");
                }

                let mode: Id = send_ptr(
                    nsstring_cls,
                    utf8_sel,
                    b"AVAudioSessionModeVideoChat\0".as_ptr(),
                );
                if mode.is_null() {
                    crate::diag::log("configure_ios_audio_session: failed to create mode NSString");
                } else {
                    let set_mode_sel = sel_registerName(b"setMode:error:\0".as_ptr());
                    let set_mode_ok: i8 = send_mode(session, set_mode_sel, mode, nil);
                    if set_mode_ok == 0 {
                        crate::diag::log("configure_ios_audio_session: setMode(VideoChat) failed");
                    } else {
                        crate::diag::log("configure_ios_audio_session: setMode(VideoChat) OK");
                    }
                }

                // [session setActive:YES error:nil]
                let set_active_sel = sel_registerName(b"setActive:error:\0".as_ptr());
                let set_active_ok: i8 = send_act(session, set_active_sel, 1i8, nil);
                if set_active_ok == 0 {
                    crate::diag::log("configure_ios_audio_session: setActive failed");
                } else {
                    crate::diag::log("configure_ios_audio_session: setActive OK");
                }

                log::info!("tutoring: AVAudioSession configured for PlayAndRecord");
                crate::diag::log("iOS audio session configured: PlayAndRecord + VideoChat + DefaultToSpeaker");
            }
        });
    }

    /// Try to create the audio backend (Firewheel + cpal).
    ///
    /// Both AVAudioSession configuration and CoreAudio/cpal init are
    /// dispatched to the main thread — iOS requires it.
    /// Uses spawn_blocking + timeout to prevent indefinite hangs.
    async fn try_create_audio_backend(
        input_device_id: Option<DeviceId>,
        output_device_id: Option<DeviceId>,
    ) -> Option<AudioBackend> {
        crate::diag::log("try_create_audio_backend: starting...");

        // Run the entire audio init (which needs the main thread) inside
        // spawn_blocking so we can apply a timeout without blocking tokio.
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            tokio::task::spawn_blocking(move || {
                // Configure iOS audio session first — required before any CoreAudio usage.
                Self::configure_ios_audio_session();

                // CoreAudio (cpal) init also needs the main thread on iOS.
                run_on_main_thread(move || {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                        AudioBackend::new_with_devices(input_device_id, output_device_id)
                    }))
                })
            }),
        )
        .await;

        match result {
            Ok(Ok(Ok(backend))) => {
                log::info!("tutoring: audio backend initialized");
                crate::diag::log("try_create_audio_backend: OK");
                Some(backend)
            }
            Ok(Ok(Err(e))) => {
                log::error!("tutoring: audio backend panicked during init: {e:?}");
                crate::diag::log(&format!("try_create_audio_backend: PANIC: {e:?}"));
                None
            }
            Ok(Err(e)) => {
                log::error!("tutoring: audio backend spawn_blocking failed: {e}");
                crate::diag::log(&format!("try_create_audio_backend: spawn_blocking ERR: {e}"));
                None
            }
            Err(_) => {
                log::error!("tutoring: audio backend init TIMED OUT after 10s");
                crate::diag::log("try_create_audio_backend: TIMEOUT 10s");
                None
            }
        }
    }

    /// Parse an optional device ID string into a `DeviceId`.
    fn parse_device_id(s: &Option<String>) -> Option<DeviceId> {
        s.as_ref().and_then(|id| id.parse::<DeviceId>().ok())
    }

    /// Append a log entry to the session's ring buffer (if session is active).
    async fn push_log(inner: &Mutex<Option<ActiveSession>>, msg: String) {
        let mut guard = inner.lock().await;
        if let Some(session) = guard.as_mut() {
            if session.recent_logs.len() >= MAX_LOG_ENTRIES {
                session.recent_logs.remove(0);
            }
            session.recent_logs.push(msg);
        }
    }

    fn now_millis() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    // ── Room lifecycle ─────────────────────────────────────────────

    /// Create a new tutoring room (host mode, audio + video).
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
        crate::diag::log("create_room: starting");
        set_idle_timer_disabled(true);
        let mut inner = self.inner.lock().await;
        if inner.is_some() {
            return Err("already in a tutoring session".into());
        }

        let our_node_id = endpoint.id().to_string();
        let home_relay = endpoint.addr().relay_urls().next().map(|u| u.to_string());
        crate::diag::log(&format!("create_room: node_id={}, relay={}", &our_node_id[..12.min(our_node_id.len())], home_relay.as_deref().unwrap_or("none")));

        crate::diag::log("create_room: calling Room::new...");
        let ticket = RoomTicket::generate();
        let room = tokio::time::timeout(
            Duration::from_secs(15),
            Room::new(endpoint, gossip.clone(), live, ticket),
        )
        .await
        .map_err(|_| {
            crate::diag::log("create_room: Room::new TIMED OUT after 15s");
            "Room::new timed out after 15s".to_string()
        })?
        .map_err(|e| {
            crate::diag::log(&format!("create_room: Room::new FAILED: {e}"));
            format!("failed to create room: {e}")
        })?;
        crate::diag::log("create_room: Room::new OK");

        let ticket_str = room.ticket().to_string();

        crate::diag::log("create_room: initializing audio backend...");
        let mic_id = Self::parse_device_id(&devices.mic_device_id);
        let speaker_id = Self::parse_device_id(&devices.speaker_device_id);
        let audio_ctx = Self::try_create_audio_backend(mic_id, speaker_id).await;
        let has_audio = audio_ctx.is_some();
        crate::diag::log(&format!("create_room: audio backend done, has_audio={has_audio}"));
        if !has_audio {
            log::warn!("tutoring: proceeding without audio (CoreAudio init failed)");
        }

        crate::diag::log("create_room: creating broadcast (camera + mic)...");
        let (mut broadcast, mic_input, has_video) =
            Self::create_broadcast(audio_ctx.as_ref(), has_audio).await?;
        crate::diag::log(&format!("create_room: broadcast created, video={has_video}"));
        room.publish(BROADCAST_NAME, broadcast.producer())
            .await
            .map_err(|e| format!("failed to publish broadcast: {e}"))?;
        crate::diag::log("create_room: published broadcast OK");

        let (events, handle) = room.split();

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

        // Spawn event loop to track peers and subscribe to audio+video
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

        // Start self-preview from local camera source
        let self_preview_task =
            Self::start_self_preview(&mut broadcast, app_handle.clone());

        let mut init_logs = vec![
            format!("create_room: audio={has_audio}, video={has_video}, self_preview={}", self_preview_task.is_some()),
            format!("home_relay={}", home_relay.as_deref().unwrap_or("none")),
        ];
        if !has_audio {
            init_logs.push("WARN: audio backend init failed".to_string());
        }

        *inner = Some(ActiveSession {
            session_id,
            session_title: title,
            handle,
            broadcast,
            audio_ctx,
            mic_input,
            output_stream: None,
            peers: HashMap::new(),
            video_enabled: has_video,
            audio_enabled: has_audio,
            chat_sender,
            our_node_id,
            our_display_name: display_name,
            started_at: Self::now_millis(),
            last_chat_sent: Instant::now() - Duration::from_secs(10),
            app_handle: app_handle.clone(),
            self_preview_task,
            _device_selection: devices,
            _tasks: tasks,
            recent_logs: init_logs,
            home_relay,
            _moq_sessions: Vec::new(),
            _subscribe_broadcasts: Vec::new(),
            _subscribed_keys: HashSet::new(),
            remote_broadcasts: Vec::new(),
        });

        // Spawn audio level emitter after session is stored
        let audio_level_task = Self::start_audio_level_emitter(
            self.inner.clone(),
            app_handle,
        );
        let lifecycle_task = Self::start_lifecycle_watcher(self.inner.clone());
        if let Some(session) = inner.as_mut() {
            session._tasks.push(audio_level_task);
            session._tasks.push(lifecycle_task);
        }

        log::info!(
            "tutoring: room created — audio={has_audio}, video={has_video}, ticket={}...",
            &ticket_str[..ticket_str.len().min(20)]
        );

        Ok(ticket_str)
    }

    /// Join an existing tutoring room using a ticket string (audio + video).
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
        crate::diag::log("join_room: starting");
        set_idle_timer_disabled(true);
        let mut inner = self.inner.lock().await;
        if inner.is_some() {
            return Err("already in a tutoring session".into());
        }

        let our_node_id = endpoint.id().to_string();
        let home_relay = endpoint.addr().relay_urls().next().map(|u| u.to_string());
        crate::diag::log(&format!("join_room: node_id={}, relay={}", &our_node_id[..12.min(our_node_id.len())], home_relay.as_deref().unwrap_or("none")));

        let ticket: RoomTicket = ticket_str
            .parse()
            .map_err(|e| format!("invalid room ticket: {e}"))?;

        crate::diag::log("join_room: calling Room::new...");
        let room = tokio::time::timeout(
            Duration::from_secs(15),
            Room::new(endpoint, gossip.clone(), live, ticket),
        )
        .await
        .map_err(|_| {
            crate::diag::log("join_room: Room::new TIMED OUT after 15s");
            "Room::new timed out after 15s".to_string()
        })?
        .map_err(|e| {
            crate::diag::log(&format!("join_room: Room::new FAILED: {e}"));
            format!("failed to join room: {e}")
        })?;
        crate::diag::log("join_room: Room::new OK");

        let ticket_str = room.ticket().to_string();

        crate::diag::log("join_room: creating audio backend...");
        let mic_id = Self::parse_device_id(&devices.mic_device_id);
        let speaker_id = Self::parse_device_id(&devices.speaker_device_id);
        let audio_ctx = Self::try_create_audio_backend(mic_id, speaker_id).await;
        let has_audio = audio_ctx.is_some();
        crate::diag::log(&format!("join_room: audio={has_audio}"));
        if !has_audio {
            log::warn!("tutoring: proceeding without audio (CoreAudio init failed)");
        }

        // Start publishing local audio + video
        crate::diag::log("join_room: creating broadcast (camera + mic)...");
        let (mut broadcast, mic_input, has_video) =
            Self::create_broadcast(audio_ctx.as_ref(), has_audio).await?;
        crate::diag::log(&format!("join_room: broadcast created, video={has_video}"));

        crate::diag::log("join_room: publishing broadcast...");
        room.publish(BROADCAST_NAME, broadcast.producer())
            .await
            .map_err(|e| {
                crate::diag::log(&format!("join_room: publish FAILED: {e}"));
                format!("failed to publish broadcast: {e}")
            })?;
        crate::diag::log("join_room: publish OK");

        let (events, handle) = room.split();

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

        let self_preview_task =
            Self::start_self_preview(&mut broadcast, app_handle.clone());

        let init_logs = vec![
            format!("join_room: audio={has_audio}, video={has_video}, self_preview={}", self_preview_task.is_some()),
            format!("home_relay={}", home_relay.as_deref().unwrap_or("none")),
        ];

        *inner = Some(ActiveSession {
            session_id,
            session_title: title,
            handle,
            broadcast,
            audio_ctx,
            mic_input,
            output_stream: None,
            peers: HashMap::new(),
            video_enabled: has_video,
            audio_enabled: has_audio,
            chat_sender,
            our_node_id,
            our_display_name: display_name,
            started_at: Self::now_millis(),
            last_chat_sent: Instant::now() - Duration::from_secs(10),
            app_handle: app_handle.clone(),
            self_preview_task,
            _device_selection: devices,
            _tasks: tasks,
            recent_logs: init_logs,
            home_relay,
            _moq_sessions: Vec::new(),
            _subscribe_broadcasts: Vec::new(),
            _subscribed_keys: HashSet::new(),
            remote_broadcasts: Vec::new(),
        });

        // Spawn audio level emitter after session is stored
        let audio_level_task = Self::start_audio_level_emitter(
            self.inner.clone(),
            app_handle,
        );
        let lifecycle_task = Self::start_lifecycle_watcher(self.inner.clone());
        if let Some(session) = inner.as_mut() {
            session._tasks.push(audio_level_task);
            session._tasks.push(lifecycle_task);
        }

        Ok(ticket_str)
    }

    /// Leave the current room.
    pub async fn leave_room(&self) -> Result<(), String> {
        let mut inner = self.inner.lock().await;
        let session = inner.take().ok_or("not in a tutoring session")?;

        if let Some(t) = &session.self_preview_task {
            t.abort();
        }
        for task in &session._tasks {
            task.abort();
        }
        drop(session);

        set_idle_timer_disabled(false);
        log::info!("left tutoring session");
        Ok(())
    }

    // ── Media controls ─────────────────────────────────────────────

    /// Toggle local microphone on/off.
    pub async fn toggle_audio(&self, enable: bool) -> Result<bool, String> {
        let mut inner = self.inner.lock().await;
        let session = inner.as_mut().ok_or("not in a tutoring session")?;

        if enable == session.audio_enabled {
            return Ok(session.audio_enabled);
        }

        if enable {
            if session.mic_input.is_some() {
                session.broadcast.set_audio_muted(false);
                session.audio_enabled = true;
            } else if let Some(ref ctx) = session.audio_ctx {
                match ctx.default_input().await {
                    Ok(mic) => {
                        session.mic_input = Some(mic.clone());
                        let renditions =
                            AudioRenditions::new::<PureOpusEncoder>(mic, [AudioPreset::Hq]);
                        session
                            .broadcast
                            .set_audio(Some(renditions))
                            .map_err(|e| format!("failed to enable audio: {e}"))?;
                        session.broadcast.set_audio_muted(false);
                        session.audio_enabled = true;
                    }
                    Err(e) => {
                        return Err(format!("microphone unavailable: {e}"));
                    }
                }
            } else {
                return Err("audio backend not available".into());
            }
        } else {
            session.broadcast.set_audio_muted(true);
            session.audio_enabled = false;
        }

        Ok(session.audio_enabled)
    }

    /// Toggle local camera on/off.
    pub async fn toggle_video(&self, enable: bool) -> Result<bool, String> {
        let mut inner = self.inner.lock().await;
        let session = inner.as_mut().ok_or("not in a tutoring session")?;

        if enable == session.video_enabled {
            return Ok(session.video_enabled);
        }

        if enable {
            // Try to create camera and video renditions.
            // Dispatch to main thread — AVCaptureSession requires it on iOS.
            let camera_result = run_on_main_thread(|| IosCameraSource::front());
            match camera_result {
                Ok(camera) => {
                    let renditions =
                        VideoRenditions::new::<VtEncoder>(camera, MOBILE_VIDEO_PRESETS);
                    session
                        .broadcast
                        .set_video(Some(renditions))
                        .map_err(|e| format!("failed to enable video: {e}"))?;
                    session.video_enabled = true;
                    log::info!("tutoring: camera enabled");
                }
                Err(e) => {
                    return Err(format!("camera unavailable: {e}"));
                }
            }
        } else {
            session
                .broadcast
                .set_video(None)
                .map_err(|e| format!("failed to disable video: {e}"))?;
            session.video_enabled = false;
            // Notify frontend that self-video ended
            let _ = session.app_handle.emit(
                "tutoring:peer-video-ended",
                PeerVideoEndedEvent {
                    node_id: "self".into(),
                },
            );
            log::info!("tutoring: camera disabled");
        }

        Ok(session.video_enabled)
    }

    // ── Chat ───────────────────────────────────────────────────────

    /// Send a text chat message to all peers in the room.
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
            screen_sharing: false, // No screen sharing on mobile
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

    /// Get diagnostic info about the current session for debugging A/V pipeline.
    pub async fn diagnostics(&self) -> Option<SessionDiagnostics> {
        let inner = self.inner.lock().await;
        let session = inner.as_ref()?;

        // Merge session ring buffer + diag file log into recent_logs
        let mut all_logs = session.recent_logs.clone();
        all_logs.push("--- diag.log ---".to_string());
        let diag_contents = crate::diag::read();
        for line in diag_contents.lines() {
            all_logs.push(line.to_string());
        }

        Some(SessionDiagnostics {
            session_id: session.session_id.clone(),
            our_node_id: session.our_node_id.clone(),
            video_enabled: session.video_enabled,
            audio_enabled: session.audio_enabled,
            has_audio_ctx: session.audio_ctx.is_some(),
            has_mic_input: session.mic_input.is_some(),
            has_output_stream: session.output_stream.is_some(),
            has_self_preview: session.self_preview_task.is_some(),
            peer_count: session.peers.len(),
            peers: session.peers.values().map(|p| PeerDiagnostics {
                node_id: p.node_id.clone(),
                display_name: p.display_name.clone(),
                broadcasts: p.broadcasts.clone(),
                connected: p.connected,
            }).collect(),
            task_count: session._tasks.len(),
            recent_logs: all_logs,
            home_relay: session.home_relay.clone(),
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

    // ── Internal helpers ───────────────────────────────────────────

    /// Create a PublishBroadcast with mic + camera.
    ///
    /// Uses `PureOpusEncoder` for audio and `VtEncoder` (VideoToolbox) for video.
    /// Returns `(broadcast, mic_input, has_video)`.
    async fn create_broadcast(
        audio_ctx: Option<&AudioBackend>,
        audio: bool,
    ) -> Result<(PublishBroadcast, Option<InputStream>, bool), String> {
        let mut broadcast = PublishBroadcast::new();
        let mut mic_input: Option<InputStream> = None;
        let mut has_video = false;

        if audio {
            if let Some(ctx) = audio_ctx {
                match ctx.default_input().await {
                    Ok(mic) => {
                        mic_input = Some(mic.clone());
                        let audio_renditions =
                            AudioRenditions::new::<PureOpusEncoder>(mic, [AudioPreset::Hq]);
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

        crate::diag::log("create_broadcast: initializing camera via spawn_blocking...");
        let camera_result = tokio::time::timeout(
            Duration::from_secs(10),
            tokio::task::spawn_blocking(|| {
                run_on_main_thread(|| {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| IosCameraSource::front()))
                })
            }),
        )
        .await;
        match camera_result {
            Ok(Ok(Ok(Ok(camera)))) => {
                let video_renditions =
                    VideoRenditions::new::<VtEncoder>(camera, MOBILE_VIDEO_PRESETS);
                broadcast
                    .set_video(Some(video_renditions))
                    .map_err(|e| format!("failed to set video: {e}"))?;
                has_video = true;
                log::info!("tutoring: camera initialized (front)");
                crate::diag::log("create_broadcast: camera OK");
            }
            Ok(Ok(Ok(Err(e)))) => {
                log::warn!("tutoring: camera unavailable, continuing without video: {e}");
                crate::diag::log(&format!("create_broadcast: camera ERR: {e}"));
            }
            Ok(Ok(Err(panic_info))) => {
                log::error!("tutoring: camera init panicked: {panic_info:?}");
                crate::diag::log(&format!("create_broadcast: camera PANIC: {panic_info:?}"));
            }
            Ok(Err(e)) => {
                log::error!("tutoring: camera spawn_blocking failed: {e}");
                crate::diag::log(&format!("create_broadcast: camera spawn_blocking ERR: {e}"));
            }
            Err(_) => {
                log::error!("tutoring: camera init TIMED OUT after 10s");
                crate::diag::log("create_broadcast: camera TIMEOUT 10s");
            }
        }

        Ok((broadcast, mic_input, has_video))
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
                        None => break,
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

    /// Polls `UIApplication.applicationState` and toggles video/audio on
    /// foreground↔background transitions so the camera resumes after unlock.
    fn start_lifecycle_watcher(
        manager_inner: Arc<Mutex<Option<ActiveSession>>>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            // UIApplicationState: 0 = Active, 2 = Background
            const STATE_ACTIVE: i64 = 0;
            const STATE_BACKGROUND: i64 = 2;

            let mut was_active = get_application_state() == STATE_ACTIVE;
            let mut interval = tokio::time::interval(Duration::from_millis(500));

            loop {
                interval.tick().await;

                let state = get_application_state();
                if state < 0 {
                    continue; // UIApplication not available
                }

                let is_active = state == STATE_ACTIVE;

                if was_active && !is_active {
                    // Transitioning to background — stop camera to release hardware
                    log::info!("tutoring: app entering background, stopping camera");
                    let mut guard = manager_inner.lock().await;
                    if let Some(session) = guard.as_mut() {
                        if session.video_enabled {
                            session
                                .broadcast
                                .set_video(None)
                                .ok();
                            session.video_enabled = false;
                            if let Some(task) = session.self_preview_task.take() {
                                task.abort();
                            }
                            let _ = session.app_handle.emit(
                                "tutoring:peer-video-ended",
                                PeerVideoEndedEvent {
                                    node_id: "self".into(),
                                },
                            );
                            log::info!("tutoring: camera stopped (background)");
                        }
                    } else {
                        break;
                    }
                } else if !was_active && is_active {
                    log::info!("tutoring: app returning to foreground, waiting 1s for connections to stabilize");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    log::info!("tutoring: restarting camera after foreground return");
                    let mut remotes_to_resubscribe: Vec<(EndpointId, String)> = Vec::new();
                    let mut guard = manager_inner.lock().await;
                    if let Some(session) = guard.as_mut() {
                        if !session.video_enabled {
                            let camera_result = run_on_main_thread(|| IosCameraSource::front());
                            match camera_result {
                                Ok(camera) => {
                                    let renditions =
                                        VideoRenditions::new::<VtEncoder>(camera, MOBILE_VIDEO_PRESETS);
                                    if session.broadcast.set_video(Some(renditions)).is_ok() {
                                        session.video_enabled = true;
                                        let preview = Self::start_self_preview(
                                            &mut session.broadcast,
                                            session.app_handle.clone(),
                                        );
                                        session.self_preview_task = preview;
                                        log::info!("tutoring: camera restarted (foreground)");
                                    }
                                }
                                Err(e) => {
                                    log::warn!("tutoring: failed to restart camera after unlock: {e}");
                                }
                            }
                        }
                        remotes_to_resubscribe = session.remote_broadcasts.clone();
                        session._subscribed_keys.clear();
                    } else {
                        break;
                    }
                    drop(guard);
                    for (endpoint, name) in remotes_to_resubscribe {
                        let mut guard = manager_inner.lock().await;
                        if let Some(session) = guard.as_mut() {
                            if let Err(e) = session.handle.force_resubscribe(endpoint, name.as_str()).await {
                                log::warn!("tutoring: force_resubscribe failed for {name}: {e}");
                            }
                        } else {
                            break;
                        }
                    }
                }

                was_active = is_active;
            }
            log::info!("tutoring: lifecycle watcher ended");
        })
    }

    /// Set up a chat channel on a gossip topic derived from the room.
    async fn setup_chat(
        gossip: &Gossip,
        topic_seed: &[u8],
        our_node_id: &str,
        app_handle: AppHandle,
    ) -> Option<iroh_gossip::api::GossipSender> {
        use iroh_gossip::proto::TopicId;

        let mut hasher = blake3::Hasher::new();
        hasher.update(topic_seed);
        hasher.update(b"/chat");
        let hash = hasher.finalize();
        let topic_id = TopicId::from_bytes(*hash.as_bytes());

        match gossip.subscribe(topic_id, vec![]).await {
            Ok(topic) => {
                let (sender, mut receiver) = topic.split();
                let our_id = our_node_id.to_string();

                tokio::spawn(async move {
                    use futures::StreamExt;
                    while let Some(Ok(event)) = receiver.next().await {
                        if let iroh_gossip::api::Event::Received(msg) = event {
                            match postcard::from_bytes::<ChatMessage>(&msg.content) {
                                Ok(chat_msg) => {
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

    /// Set up display name exchange on a `/names` gossip topic.
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
                    broadcast_interval.tick().await;

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
                                            if announce.node_id == our_id {
                                                continue;
                                            }
                                            log::info!(
                                                "tutoring: learned peer name: {} = {}",
                                                &announce.node_id[..8.min(announce.node_id.len())],
                                                announce.display_name
                                            );

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

                                            let _ = app_handle.emit(
                                                "tutoring:peer-name",
                                                PeerNameEvent {
                                                    node_id: announce.node_id,
                                                    display_name: announce.display_name,
                                                },
                                            );
                                        }
                                    }
                                    Some(Ok(_)) => {}
                                    Some(Err(e)) => {
                                        log::warn!("tutoring: names gossip error: {e}");
                                    }
                                    None => break,
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
    /// connections, broadcast subscriptions) and subscribes to
    /// audio+video streams from remote peers.
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
                    let short_id = node_id[..node_id.len().min(12)].to_string();
                    log::info!("tutoring: peer announced: {short_id} with {broadcasts:?}");
                    crate::diag::log(&format!("RemoteAnnounced: {short_id} broadcasts={broadcasts:?}"));

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
                                broadcasts: broadcasts.clone(),
                                connected: false,
                            });
                        // Log to ring buffer
                        let msg = format!("peer_announced: {short_id} broadcasts={broadcasts:?}");
                        if session.recent_logs.len() >= MAX_LOG_ENTRIES {
                            session.recent_logs.remove(0);
                        }
                        session.recent_logs.push(msg);
                    }
                }
                RoomEvent::RemoteConnected {
                    session: moq_session,
                } => {
                    let node_id = moq_session.conn().remote_id().to_string();
                    let short_id = node_id[..node_id.len().min(12)].to_string();
                    log::info!("tutoring: peer connected (MoQ): {short_id}");
                    crate::diag::log(&format!("RemoteConnected: {short_id}"));

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
                        let msg = format!("peer_connected_moq: {short_id}");
                        if session.recent_logs.len() >= MAX_LOG_ENTRIES {
                            session.recent_logs.remove(0);
                        }
                        session.recent_logs.push(msg);
                    }
                }
                RoomEvent::BroadcastSubscribed {
                    session: moq_session,
                    broadcast,
                } => {
                    let remote_endpoint = moq_session.remote_id();
                    let node_id = remote_endpoint.to_string();
                    let name = broadcast.broadcast_name().to_string();
                    let short_id = node_id[..node_id.len().min(12)].to_string();

                    log::info!("tutoring: subscribed to {short_id}:{name}");
                    crate::diag::log(&format!("BroadcastSubscribed: {short_id}:{name}"));

                    // Deduplicate using _subscribed_keys (not peers.broadcasts — those are set by RemotePeerAnnounced)
                    let is_duplicate = {
                        let mut guard = inner.lock().await;
                        if let Some(session) = guard.as_mut() {
                            let key = format!("{node_id}:{name}");
                            let dup = !session._subscribed_keys.insert(key);
                            if !dup
                                && !session
                                    .remote_broadcasts
                                    .iter()
                                    .any(|(endpoint, bname)| *endpoint == remote_endpoint && *bname == name)
                            {
                                session
                                    .remote_broadcasts
                                    .push((remote_endpoint, name.clone()));
                            }
                            session
                                .peers
                                .entry(node_id.clone())
                                .and_modify(|p| {
                                    if !p.broadcasts.contains(&name) {
                                        p.broadcasts.push(name.clone());
                                    }
                                    p.connected = true;
                                })
                                .or_insert(TutoringPeer {
                                    node_id: node_id.clone(),
                                    display_name: None,
                                    broadcasts: vec![name.clone()],
                                    connected: true,
                                });
                            session._moq_sessions.push(moq_session);
                            let msg = format!("broadcast_subscribed: {short_id}:{name}");
                            if session.recent_logs.len() >= MAX_LOG_ENTRIES {
                                session.recent_logs.remove(0);
                            }
                            session.recent_logs.push(msg);
                            dup
                        } else {
                            false
                        }
                    };
                    if is_duplicate {
                        log::warn!("tutoring: DUPLICATE BroadcastSubscribed for {short_id}:{name}, skipping");
                        crate::diag::log(&format!("  [{short_id}] DUPLICATE — skipping subscription"));
                        {
                            let mut guard = inner.lock().await;
                            if let Some(session) = guard.as_mut() {
                                session._subscribe_broadcasts.push(broadcast);
                            }
                        }
                        continue;
                    }

                    // ── Audio subscription ──────────────────────────
                    crate::diag::log(&format!("  [{short_id}] step 1: getting audio output..."));
                    let audio_out = match &audio_ctx {
                        Some(ctx) => {
                            // Wrap in catch_unwind to prevent panics from killing event loop
                            let out_result = std::panic::catch_unwind(
                                std::panic::AssertUnwindSafe(|| {
                                    // AudioBackend::default_output() is async but we need
                                    // catch_unwind which is sync. Use block_in_place.
                                    tokio::task::block_in_place(|| {
                                        tokio::runtime::Handle::current()
                                            .block_on(ctx.default_output())
                                    })
                                })
                            );
                            match out_result {
                                Ok(Ok(out)) => {
                                    crate::diag::log(&format!("  [{short_id}] audio output OK"));
                                    Some(out)
                                }
                                Ok(Err(e)) => {
                                    let msg = format!("  [{short_id}] audio output err: {e}");
                                    log::warn!("tutoring: audio output unavailable: {e}");
                                    crate::diag::log(&msg);
                                    Self::push_log(&inner, format!("ERR audio_output: {e}")).await;
                                    None
                                }
                                Err(panic) => {
                                    let msg = format!("  [{short_id}] audio output PANIC: {panic:?}");
                                    log::error!("tutoring: audio output panicked: {panic:?}");
                                    crate::diag::log(&msg);
                                    Self::push_log(&inner, format!("PANIC audio_output: {panic:?}")).await;
                                    None
                                }
                            }
                        }
                        None => {
                            crate::diag::log(&format!("  [{short_id}] no audio_ctx, skipping audio"));
                            Self::push_log(&inner, "WARN: no audio_ctx for playback".into()).await;
                            None
                        }
                    };

                    crate::diag::log(&format!("  [{short_id}] step 2: subscribing to audio..."));
                    let catalog_has_audio = broadcast.catalog().audio.is_some();
                    crate::diag::log(&format!("  [{short_id}] catalog audio={catalog_has_audio}"));
                    if !catalog_has_audio {
                        log::warn!("tutoring: catalog from {short_id}:{name} has NO audio renditions");
                    }
                    if let Some(audio_out) = audio_out {
                        let output_clone = audio_out.clone();

                        match broadcast.listen::<PureOpusDecoder>(audio_out) {
                            Ok(audio_track) => {
                                log::info!(
                                    "tutoring: listening to audio from {short_id}:{name}"
                                );
                                crate::diag::log(&format!("  [{short_id}] audio_listen OK"));
                                Self::push_log(&inner, format!("audio_listen OK: {short_id}:{name}")).await;
                                {
                                    let mut guard = inner.lock().await;
                                    if let Some(session) = guard.as_mut() {
                                        session.output_stream = Some(output_clone);
                                    }
                                }
                                let inner_audio = inner.clone();
                                let nid_audio = node_id.clone();
                                let bname_audio = name.clone();
                                let sub_key_audio = format!("{node_id}:{name}");
                                let audio_keepalive = tokio::spawn(async move {
                                    audio_track.stopped().await;
                                    log::warn!("tutoring: audio track stopped for {nid_audio}:{bname_audio}");
                                    crate::diag::log(&format!("audio_track[{nid_audio}]: STOPPED for {bname_audio}"));
                                    let mut guard = inner_audio.lock().await;
                                    if let Some(session) = guard.as_mut() {
                                        session._subscribed_keys.remove(&sub_key_audio);
                                    }
                                });
                                {
                                    let mut guard = inner.lock().await;
                                    if let Some(session) = guard.as_mut() {
                                        session._tasks.push(audio_keepalive);
                                    }
                                }
                            }
                            Err(e) => {
                                log::warn!(
                                    "tutoring: no audio track for {short_id}:{name}: {e}"
                                );
                                crate::diag::log(&format!("  [{short_id}] audio_listen ERR: {e}"));
                                Self::push_log(&inner, format!("ERR audio_listen: {short_id}:{name}: {e}")).await;
                            }
                        }
                    } else {
                        log::warn!(
                            "tutoring: no audio output available, skipping audio for {short_id}:{name}"
                        );
                    }

                    // ── Video subscription ──────────────────────────
                    // Log catalog contents for debugging
                    crate::diag::log(&format!("  [{short_id}] step 3: checking catalog..."));
                    let catalog = broadcast.catalog();
                    let has_video_in_catalog = catalog.video.is_some();
                    let has_audio_in_catalog = catalog.audio.is_some();
                    crate::diag::log(&format!(
                        "  [{short_id}] catalog: video={has_video_in_catalog}, audio={has_audio_in_catalog}"
                    ));
                    if let Some(ref video_cat) = catalog.video {
                        for (rname, vcfg) in &video_cat.renditions {
                            let has_desc = vcfg.description.is_some();
                            let desc_len = vcfg.description.as_ref().map(|d| d.len()).unwrap_or(0);
                            crate::diag::log(&format!(
                                "  [{short_id}] video rendition '{rname}': {}x{}, desc={has_desc} ({desc_len} bytes)",
                                vcfg.coded_width.unwrap_or(0),
                                vcfg.coded_height.unwrap_or(0),
                            ));
                        }
                    }

                    crate::diag::log(&format!("  [{short_id}] step 4: subscribing to video (VtDecoder, quality=Mid)..."));
                    match catalog.select_video_rendition(iroh_live::media::av::Quality::Mid) {
                        Ok(selected) => {
                            crate::diag::log(&format!(
                                "  [{short_id}] selected video rendition for Mid: {selected}"
                            ));
                        }
                        Err(e) => {
                            crate::diag::log(&format!(
                                "  [{short_id}] failed to select video rendition for Mid: {e:#}"
                            ));
                        }
                    }
                    match broadcast.watch_with::<VtDecoder>(
                        &iroh_live::media::av::DecodeConfig::default(),
                        iroh_live::media::av::Quality::Mid,
                    ) {
                        Ok(video_track) => {
                            log::info!(
                                "tutoring: watching video from {short_id}:{name}"
                            );
                            crate::diag::log(&format!("  [{short_id}] video_watch OK, spawning frame bridge"));
                            Self::push_log(&inner, format!("video_watch OK: {short_id}:{name}")).await;
                            let sub_key = format!("{node_id}:{name}");
                            Self::spawn_frame_bridge_inner(
                                video_track,
                                node_id.clone(),
                                app_handle.clone(),
                                Some(inner.clone()),
                                Some(sub_key),
                                Some(remote_endpoint),
                                Some(name.clone()),
                            );
                        }
                        Err(e) => {
                            log::info!(
                                "tutoring: no video track for {short_id}:{name}: {e:#}"
                            );
                            crate::diag::log(&format!("  [{short_id}] video_watch ERR: {e:#}"));
                            Self::push_log(&inner, format!("ERR video_watch: {short_id}:{name}: {e:#}")).await;
                        }
                    }

                    // Keep SubscribeBroadcast alive — dropping it closes the
                    // BroadcastConsumer which kills all subscribed tracks.
                    {
                        let mut guard = inner.lock().await;
                        if let Some(session) = guard.as_mut() {
                            session._subscribe_broadcasts.push(broadcast);
                        }
                    }

                    crate::diag::log(&format!("  [{short_id}] BroadcastSubscribed handler complete"));
                }
            }
        }
        log::info!("tutoring: event loop ended");
        crate::diag::log("event_loop: ended (events channel closed)");
    }

    fn start_self_preview(
        broadcast: &mut PublishBroadcast,
        app_handle: AppHandle,
    ) -> Option<JoinHandle<()>> {
        let config = iroh_live::media::av::DecodeConfig::default();
        let watch = broadcast.watch_local(config)?;
        log::info!("tutoring: starting self-preview (mobile)");
        crate::diag::log("self_preview: STARTED");
        Some(Self::spawn_frame_bridge(watch, "self".into(), app_handle))
    }

    fn spawn_frame_bridge(
        watch: WatchTrack,
        node_id: String,
        app_handle: AppHandle,
    ) -> JoinHandle<()> {
        Self::spawn_frame_bridge_inner(watch, node_id, app_handle, None, None, None, None)
    }

    fn spawn_frame_bridge_inner(
        watch: WatchTrack,
        node_id: String,
        app_handle: AppHandle,
        session_inner: Option<Arc<Mutex<Option<ActiveSession>>>>,
        subscription_key: Option<String>,
        remote_endpoint: Option<EndpointId>,
        broadcast_name: Option<String>,
    ) -> JoinHandle<()> {
        let (mut frames, handle) = watch.split();
        handle.set_viewport(480, 640);

        tokio::spawn(async move {
            let _handle = handle;
            log::info!("tutoring: frame bridge started for {node_id}");
            crate::diag::log(&format!("frame_bridge[{node_id}]: STARTED, waiting for frames"));

            let frame_interval = Duration::from_millis(55);
            let mut last_emit = std::time::Instant::now();
            let mut frame_count: u64 = 0;
            let initial_timeout = Duration::from_secs(10);
            let stall_timeout = Duration::from_secs(30);

            loop {
                let timeout = if frame_count == 0 { initial_timeout } else { stall_timeout };
                let maybe_frame = match tokio::time::timeout(timeout, frames.next_frame()).await {
                    Ok(f) => f,
                    Err(_) => {
                        if frame_count == 0 {
                            log::warn!(
                                "tutoring: frame bridge {node_id}: no frames after {initial_timeout:?} — track likely dead"
                            );
                            crate::diag::log(&format!("frame_bridge[{node_id}]: TIMEOUT {initial_timeout:?} — no frames, treating as dead"));
                        } else {
                            log::warn!(
                                "tutoring: frame bridge {node_id}: stalled after {frame_count} frames ({stall_timeout:?} with no new frame) — treating as dead"
                            );
                            crate::diag::log(&format!("frame_bridge[{node_id}]: STALL after {frame_count} frames, triggering recovery"));
                        }
                        None
                    }
                };

                match maybe_frame {
                    Some(frame) => {
                        frame_count += 1;
                        if frame_count == 1 || frame_count % 100 == 0 {
                            log::info!(
                                "tutoring: frame bridge {node_id}: received frame #{frame_count}"
                            );
                            crate::diag::log(&format!(
                                "frame_bridge[{node_id}]: frame #{frame_count} ({}x{})",
                                frame.img().width(), frame.img().height()
                            ));
                        }
                        let now = std::time::Instant::now();
                        if now.duration_since(last_emit) < frame_interval {
                            continue;
                        }
                        last_emit = now;

                        let img = frame.img();
                        let (width, height) = img.dimensions();

                        let rgb_data: Vec<u8> = img
                            .as_raw()
                            .chunks_exact(4)
                            .flat_map(|px| [px[0], px[1], px[2]])
                            .collect();

                        let mut jpeg_buf = Vec::with_capacity((width * height) as usize / 4);
                        let mut cursor = Cursor::new(&mut jpeg_buf);
                        let encoder =
                            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 65);
                        match encoder.write_image(
                            &rgb_data,
                            width,
                            height,
                            image::ExtendedColorType::Rgb8,
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
                        crate::diag::log(&format!("frame_bridge[{node_id}]: ENDED (track closed), total frames={frame_count}"));
                        let _ = app_handle.emit(
                            "tutoring:peer-video-ended",
                            PeerVideoEndedEvent {
                                node_id: node_id.clone(),
                            },
                        );
                        if let (Some(inner_ref), Some(key)) = (&session_inner, &subscription_key) {
                            let mut guard = inner_ref.lock().await;
                            if let Some(session) = guard.as_mut() {
                                session._subscribed_keys.remove(key);
                                log::info!("tutoring: cleared subscription key {key} — not force_resubscribing to avoid cascade");
                                crate::diag::log(&format!("frame_bridge[{node_id}]: cleared key {key}, no resubscribe"));
                            }
                        }
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
