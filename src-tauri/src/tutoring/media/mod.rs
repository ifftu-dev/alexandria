//! The seam between a call and the device it runs on.
//!
//! Everything about a call that is the same everywhere — the room, task
//! admission, chat, transcript, presence, names, the frame bridge to the
//! webview, diagnostics, audio levels — lives in the call manager. What is
//! not the same is small and listed here: where a camera and a screen come
//! from, which encoder and decoder carry video, how the microphone and the
//! speaker open and which route they use, whether the platform wants its
//! capture APIs called on the main thread, and which audio and lifecycle
//! events the device raises on its own.
//!
//! One adapter fills the seam per target:
//!
//! - [`desktop`]: nokhwa camera, xcap screen, software H.264 (ffmpeg).
//! - [`ios`]: AVFoundation camera, VideoToolbox H.264, AVAudioSession routes
//!   and interruptions, GCD main-thread dispatch.
//! - [`android`]: the NDK Camera2 camera over the same ffmpeg pipeline as
//!   the desktop; no screen share.
//! - [`synthetic`]: generated frames and silent audio, for tests (and any
//!   target with no adapter of its own).
//!
//! Every platform adapter also works without its video feature, making
//! audio-only calls on the real microphone and speaker.
//!
//! [`PlatformMedia`] names the adapter the current build uses.

use std::future::Future;
use std::time::Duration;

use live::media::audio::{AudioBackend, InputStream, OutputStream};
use live::media::av::{AudioSink, AudioSource, Quality};
#[cfg(tutoring_video)]
use live::media::av::{VideoDecoder, VideoEncoder, VideoPreset, VideoSource};
use serde::Serialize;
use tokio::sync::broadcast;

#[cfg(target_os = "android")]
pub mod android;
#[cfg(desktop)]
pub mod desktop;
#[cfg(all(any(desktop, target_os = "android"), tutoring_video))]
mod ffmpeg;
#[cfg(target_os = "ios")]
pub mod ios;
#[cfg(target_os = "ios")]
pub(crate) mod main_thread;
#[cfg(any(test, not(any(desktop, target_os = "android", target_os = "ios"))))]
pub mod synthetic;

/// The adapter this build fills the seam with. Without a video feature it
/// still makes real audio-only calls.
#[cfg(desktop)]
pub type PlatformMedia = desktop::DesktopMedia;
/// The adapter this build fills the seam with. Without a video feature it
/// still makes real audio-only calls.
#[cfg(target_os = "android")]
pub type PlatformMedia = android::AndroidMedia;
/// The adapter this build fills the seam with. Without a video feature it
/// still makes real audio-only calls.
#[cfg(target_os = "ios")]
pub type PlatformMedia = ios::IosMedia;
/// A target with no platform adapter runs over generated media.
#[cfg(not(any(desktop, target_os = "android", target_os = "ios")))]
pub type PlatformMedia = synthetic::SyntheticMedia;

/// Error for a video operation in a build without video.
pub const VIDEO_DISABLED_ERROR: &str = "video support is disabled in this build";

// ── Devices as the webview sees them ─────────────────────────────────

/// User-selected devices for a session. `None` means the platform default.
#[derive(Debug, Clone, Default)]
pub struct DeviceSelection {
    /// Camera index string as [`MediaDevices::cameras`] reports it, e.g.
    /// "Index(0)" on the desktop. Phones have one camera and ignore it.
    pub camera_index: Option<String>,
    /// Audio input id as [`MediaDevices::audio_devices`] reports it.
    pub mic_device_id: Option<String>,
    /// Audio output id as [`MediaDevices::audio_devices`] reports it.
    pub speaker_device_id: Option<String>,
}

/// An available camera.
#[derive(Debug, Clone, Serialize)]
pub struct CameraDeviceInfo {
    /// Camera index (numeric or string key), passed back to select it.
    pub index: String,
    /// Human-readable name.
    pub name: String,
}

/// An available audio device.
#[derive(Debug, Clone, Serialize)]
pub struct AudioDeviceInfo {
    /// Device id, stable across restarts, passed back to select it.
    pub id: String,
    /// Human-readable name.
    pub name: Option<String>,
    /// Whether this is the current default.
    pub is_default: bool,
}

/// Result of a pre-join device availability check.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceCheckResult {
    /// Whether at least one camera was found.
    pub has_camera: bool,
    /// Name of the first camera (e.g. "FaceTime HD Camera").
    pub camera_name: Option<String>,
    /// Whether an audio input device was found.
    pub has_audio: bool,
    /// Error message if something failed (informational).
    pub error: Option<String>,
}

/// The audio devices a platform offers and which it currently uses.
#[derive(Debug, Clone, Default)]
pub struct AudioDeviceInventory {
    pub inputs: Vec<AudioDeviceInfo>,
    pub outputs: Vec<AudioDeviceInfo>,
    pub selected_input: Option<String>,
    pub selected_output: Option<String>,
}

// ── What a platform can do, and how it likes its frames ──────────────

/// How frames for one webview tile are produced: the decoder fits each frame
/// inside `viewport`, the JPEG step scales again only if a frame still
/// arrives larger than `jpeg_max`, and at most one frame per `interval` is
/// sent.
#[derive(Debug, Clone, Copy)]
pub struct FrameTuning {
    pub viewport: (u32, u32),
    pub jpeg_max: (u32, u32),
    pub jpeg_quality: u8,
    pub interval: Duration,
}

/// What the platform's media can do. Read once per session.
#[derive(Debug, Clone)]
pub struct MediaProfile {
    /// The adapter's name, for logs and diagnostics.
    pub name: &'static str,
    /// A camera can be published.
    pub video: bool,
    /// The screen can be shared in place of the camera.
    pub screen_share: bool,
    /// Another camera can be picked mid-call.
    pub camera_switch: bool,
    /// The audio route can be changed mid-call.
    pub audio_route_switch: bool,
    /// The rendition to watch from a remote peer.
    pub remote_quality: Quality,
    /// Frames for the self view tile.
    pub self_view: FrameTuning,
    /// Frames for a remote peer's tile.
    pub remote_view: FrameTuning,
    /// How often mic and speaker levels are reported to the webview.
    pub audio_level_interval: Duration,
    /// How long joining the room may take before the start fails. `None`
    /// waits as long as it takes.
    pub room_open_timeout: Option<Duration>,
    /// How long the audio engine and the first camera may take to open when
    /// a call starts before the call goes on without them. `None` waits:
    /// on a desktop the first open can sit behind a permission prompt the
    /// learner has not clicked yet, and a timeout there would start a call
    /// with no camera or microphone and drop the device once it opens.
    pub start_device_timeout: Option<Duration>,
}

/// The rendition a phone watches: 720p at most. Its tiles are never larger
/// than the 960px remote viewport, and decoding plus copying out 1080p frames
/// fell behind the sender (15 Oct).
#[cfg(any(target_os = "android", target_os = "ios"))]
pub(crate) const PHONE_VIDEO_QUALITY: Quality = Quality::High;

/// What a phone watches and how its tiles are fed; each phone adapter fills
/// in what it can do.
#[cfg(any(target_os = "android", target_os = "ios"))]
pub(crate) fn phone_profile() -> MediaProfile {
    MediaProfile {
        name: "phone",
        video: false,
        screen_share: false,
        camera_switch: false,
        audio_route_switch: false,
        remote_quality: PHONE_VIDEO_QUALITY,
        // The decoder scales each frame to fit this box before the JPEG
        // step, which is the costly part: a 720x1280 self view encoded
        // several times a second held a core at 100% (iOS killed the app
        // for it). The self view only fills a phone-sized tile, so 360x640
        // is plenty; a remote camera fits 960x960 whichever way up it is
        // held. The encoder scales again only if a frame still arrives
        // larger.
        self_view: FrameTuning {
            viewport: (360, 640),
            jpeg_max: (360, 640),
            jpeg_quality: 75,
            interval: Duration::from_millis(42),
        },
        remote_view: FrameTuning {
            viewport: (960, 960),
            jpeg_max: (960, 960),
            jpeg_quality: 70,
            interval: Duration::from_millis(50),
        },
        audio_level_interval: Duration::from_millis(200),
        room_open_timeout: None,
        start_device_timeout: None,
    }
}

/// Something the device did on its own that a call must react to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaEvent {
    /// The audio session is ours again after another app or a phone call
    /// took it; outputs should resume.
    AudioResumed,
    /// Headphones, a Bluetooth device or a car came or went; outputs should
    /// resume on the new route.
    AudioRouteChanged,
    /// The app left the foreground; the camera must be released.
    AppBackgrounded,
    /// The app is in the foreground again; the camera may reopen.
    AppForegrounded,
}

// ── Audio streams ────────────────────────────────────────────────────

/// A microphone: an audio source with a level meter.
pub trait CallAudioInput: AudioSource + Clone + Sync {
    /// Smoothed peak level, 0.0–1.0.
    fn level(&self) -> f32;
}

/// A speaker: an audio sink that can be resumed after a route change.
pub trait CallAudioOutput: AudioSink + Clone + Sync {
    /// Whether the stream is still running on its device.
    fn is_active(&self) -> bool;
}

/// One session's audio engine, handing out the mic and speaker streams.
pub trait AudioSession: Clone + Send + Sync + 'static {
    type Input: CallAudioInput;
    type Output: CallAudioOutput;

    fn input(&self) -> impl Future<Output = Result<Self::Input, String>> + Send;
    fn output(&self) -> impl Future<Output = Result<Self::Output, String>> + Send;
}

impl CallAudioInput for InputStream {
    fn level(&self) -> f32 {
        self.smoothed_peak_normalized()
    }
}

impl CallAudioOutput for OutputStream {
    fn is_active(&self) -> bool {
        OutputStream::is_active(self)
    }
}

/// The Firewheel/cpal backend every real adapter uses.
impl AudioSession for AudioBackend {
    type Input = InputStream;
    type Output = OutputStream;

    async fn input(&self) -> Result<InputStream, String> {
        self.default_input().await.map_err(|e| e.to_string())
    }

    async fn output(&self) -> Result<OutputStream, String> {
        self.default_output().await.map_err(|e| e.to_string())
    }
}

/// Parse a device id string from [`AudioDeviceInfo::id`].
#[cfg(any(desktop, target_os = "android"))]
pub(crate) fn parse_device_id(id: Option<&str>) -> Option<live::media::audio::DeviceId> {
    id.and_then(|id| id.parse().ok())
}

/// Whether cpal sees a microphone. Enumeration only.
#[cfg(any(desktop, target_os = "android", target_os = "ios"))]
pub(crate) fn has_cpal_input() -> bool {
    !AudioBackend::list_input_devices().is_empty()
}

/// The cpal device list as [`AudioDeviceInfo`], default first.
#[cfg(any(desktop, target_os = "android"))]
pub(crate) fn cpal_inventory() -> AudioDeviceInventory {
    let map = |devices: Vec<live::media::audio::DeviceInfo>| -> Vec<AudioDeviceInfo> {
        devices
            .into_iter()
            .map(|d| AudioDeviceInfo {
                id: d.id.to_string(),
                name: d.name,
                is_default: d.is_default,
            })
            .collect()
    };
    let inputs = map(AudioBackend::list_input_devices());
    let outputs = map(AudioBackend::list_output_devices());
    let pick = |devices: &[AudioDeviceInfo]| {
        devices
            .iter()
            .find(|d| d.is_default)
            .or_else(|| devices.first())
            .map(|d| d.id.clone())
    };
    AudioDeviceInventory {
        selected_input: pick(&inputs),
        selected_output: pick(&outputs),
        inputs,
        outputs,
    }
}

// ── Video sources ────────────────────────────────────────────────────

/// A camera or screen ready to publish: the frames, the system's name for
/// the camera (none for a screen, or where the platform does not name it)
/// and the renditions it can fill.
#[cfg(tutoring_video)]
pub struct OpenedVideo<S> {
    pub source: S,
    pub camera_name: Option<String>,
    pub presets: Vec<VideoPreset>,
}

// ── The seam ─────────────────────────────────────────────────────────

/// What a call needs from the device it runs on.
///
/// Blocking methods (`open_audio`, `open_camera`, `open_screen`,
/// `set_audio_route`) may take seconds or never return when a device is
/// gone; the call manager runs them on blocking threads with timeouts and
/// never under its own lock. An adapter whose platform wants them on the
/// main thread hops there inside the method with [`Self::on_main_thread`].
pub trait MediaDevices: Send + Sync + 'static {
    /// The audio engine for one session.
    type Audio: AudioSession;
    /// Frames from a camera or screen.
    #[cfg(tutoring_video)]
    type VideoSource: VideoSource;
    /// Encoder for published video.
    #[cfg(tutoring_video)]
    type VideoEncoder: VideoEncoder;
    /// Decoder for watched video.
    #[cfg(tutoring_video)]
    type VideoDecoder: VideoDecoder;

    /// What this device can do and how it likes its frames.
    fn profile(&self) -> MediaProfile;

    /// The cameras available right now.
    fn cameras(&self) -> impl Future<Output = Vec<CameraDeviceInfo>> + Send;

    /// The audio devices available right now and which are in use. May
    /// configure the platform's audio session to see them all.
    fn audio_devices(&self) -> AudioDeviceInventory;

    /// Whether a camera and a microphone are there, before a call. Only
    /// enumerates: it must not start a stream or configure an audio session,
    /// which would stop the learner's music on the pre-join screen.
    fn check_devices(&self) -> impl Future<Output = DeviceCheckResult> + Send;

    /// Start the audio engine on the selected devices or route. Blocking.
    fn open_audio(&self, selection: &DeviceSelection) -> Result<Self::Audio, String>;

    /// Open a camera by index, or the preferred one. Blocking.
    #[cfg(tutoring_video)]
    fn open_camera(&self, camera: Option<&str>) -> Result<OpenedVideo<Self::VideoSource>, String>;

    /// Open the screen for sharing. Blocking. Platforms that cannot share a
    /// screen return an error that reads well to the learner.
    #[cfg(tutoring_video)]
    fn open_screen(&self) -> Result<OpenedVideo<Self::VideoSource>, String>;

    /// Move the audio to the selected route mid-call. Blocking.
    fn set_audio_route(&self, selection: &DeviceSelection) -> Result<(), String>;

    /// Route, interruption and app-lifecycle events as the device raises
    /// them. A fresh receiver sees only events after this call.
    fn events(&self) -> broadcast::Receiver<MediaEvent>;

    /// A session is running: keep the device awake, watch its lifecycle.
    fn session_started(&self) {}

    /// The session ended; undo [`Self::session_started`].
    fn session_ended(&self) {}

    /// Run `work` where the platform's capture APIs want to be called. Most
    /// platforms have no such requirement and run it in place.
    fn on_main_thread<R: Send>(&self, work: impl FnOnce() -> R + Send) -> R {
        work()
    }
}
