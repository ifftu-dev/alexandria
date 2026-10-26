//! Generated media: a moving test picture and silence.
//!
//! This is the adapter tests run against, and the one any build without a
//! video feature uses. It never opens a device. With a video feature on it
//! publishes frames through a raw passthrough codec so the whole pipeline
//! (shared source, encoder thread, MoQ track, decode loop, frame bridge)
//! runs end to end in a test; without one the build has no video types and
//! the camera simply reports as absent.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use live::media::av::{AudioFormat, AudioSink, AudioSinkHandle, AudioSource, Quality};
use tokio::sync::broadcast;

use super::{
    AudioDeviceInfo, AudioDeviceInventory, AudioSession, CallAudioInput, CallAudioOutput,
    CameraDeviceInfo, DeviceSelection, FrameTuning, MediaDevices, MediaEvent, MediaProfile,
};

/// Generated frames and silent audio; no device is ever opened.
pub struct SyntheticMedia {
    events: broadcast::Sender<MediaEvent>,
    route: Mutex<DeviceSelection>,
    /// Whether [`MediaDevices::open_camera`] succeeds. Tests flip it to stand
    /// in for a device that is missing or refused.
    camera_available: AtomicBool,
    sessions_started: AtomicBool,
}

impl Default for SyntheticMedia {
    fn default() -> Self {
        Self::new()
    }
}

impl SyntheticMedia {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(16);
        Self {
            events,
            route: Mutex::new(DeviceSelection::default()),
            camera_available: AtomicBool::new(true),
            sessions_started: AtomicBool::new(false),
        }
    }

    /// Raise a device event, as the platform would.
    pub fn raise(&self, event: MediaEvent) {
        let _ = self.events.send(event);
    }

    /// Make the camera open fail from now on (or succeed again).
    pub fn set_camera_available(&self, available: bool) {
        self.camera_available.store(available, Ordering::SeqCst);
    }

    /// The route the last session asked for.
    pub fn route(&self) -> DeviceSelection {
        self.route.lock().expect("route poisoned").clone()
    }

    /// Whether a session told the adapter it was running and not yet ended.
    pub fn session_running(&self) -> bool {
        self.sessions_started.load(Ordering::SeqCst)
    }
}

impl MediaDevices for SyntheticMedia {
    type Audio = SilentAudio;
    #[cfg(tutoring_video)]
    type VideoSource = video::TestPattern;
    #[cfg(tutoring_video)]
    type VideoEncoder = video::RawEncoder;
    #[cfg(tutoring_video)]
    type VideoDecoder = video::RawDecoder;

    fn profile(&self) -> MediaProfile {
        let tuning = FrameTuning {
            viewport: (320, 180),
            jpeg_max: (320, 180),
            jpeg_quality: 70,
            interval: Duration::from_millis(100),
        };
        MediaProfile {
            name: "synthetic",
            video: cfg!(tutoring_video),
            screen_share: false,
            camera_switch: false,
            audio_route_switch: true,
            remote_quality: Quality::Highest,
            self_view: tuning,
            remote_view: tuning,
            audio_level_interval: Duration::from_millis(200),
        }
    }

    async fn cameras(&self) -> Vec<CameraDeviceInfo> {
        if cfg!(tutoring_video) && self.camera_available.load(Ordering::SeqCst) {
            vec![CameraDeviceInfo {
                index: "synthetic".into(),
                name: "Test pattern".into(),
            }]
        } else {
            Vec::new()
        }
    }

    fn audio_devices(&self) -> AudioDeviceInventory {
        let route = self.route();
        let device = |id: &str, name: &str, selected: &Option<String>| AudioDeviceInfo {
            id: id.into(),
            name: Some(name.into()),
            is_default: selected.as_deref().is_none_or(|s| s == id),
        };
        AudioDeviceInventory {
            inputs: vec![device("silence", "Silence", &route.mic_device_id)],
            outputs: vec![device("void", "Void", &route.speaker_device_id)],
            selected_input: Some(route.mic_device_id.unwrap_or_else(|| "silence".into())),
            selected_output: Some(route.speaker_device_id.unwrap_or_else(|| "void".into())),
        }
    }

    fn open_audio(&self, selection: &DeviceSelection) -> Result<SilentAudio, String> {
        *self.route.lock().expect("route poisoned") = selection.clone();
        Ok(SilentAudio)
    }

    #[cfg(tutoring_video)]
    fn open_camera(
        &self,
        _camera: Option<&str>,
    ) -> Result<super::OpenedVideo<video::TestPattern>, String> {
        if !self.camera_available.load(Ordering::SeqCst) {
            return Err("camera unavailable: no synthetic camera".into());
        }
        Ok(super::OpenedVideo {
            source: video::TestPattern::new(),
            camera_name: Some("Test pattern".into()),
            presets: vec![live::media::av::VideoPreset::P180],
        })
    }

    #[cfg(tutoring_video)]
    fn open_screen(&self) -> Result<super::OpenedVideo<video::TestPattern>, String> {
        Err("Screen sharing is not available on this device".into())
    }

    fn set_audio_route(&self, selection: &DeviceSelection) -> Result<(), String> {
        *self.route.lock().expect("route poisoned") = selection.clone();
        Ok(())
    }

    fn events(&self) -> broadcast::Receiver<MediaEvent> {
        self.events.subscribe()
    }

    fn session_started(&self) {
        self.sessions_started.store(true, Ordering::SeqCst);
    }

    fn session_ended(&self) {
        self.sessions_started.store(false, Ordering::SeqCst);
    }
}

// ── Silent audio ─────────────────────────────────────────────────────

/// An audio engine whose microphone hears nothing and whose speaker plays
/// into the void.
#[derive(Debug, Clone)]
pub struct SilentAudio;

impl AudioSession for SilentAudio {
    type Input = SilentInput;
    type Output = SilentOutput;

    async fn input(&self) -> Result<SilentInput, String> {
        Ok(SilentInput)
    }

    async fn output(&self) -> Result<SilentOutput, String> {
        Ok(SilentOutput::default())
    }
}

/// A microphone that only ever produces silence.
#[derive(Debug, Clone)]
pub struct SilentInput;

impl AudioSource for SilentInput {
    fn cloned_boxed(&self) -> Box<dyn AudioSource> {
        Box::new(self.clone())
    }

    fn format(&self) -> AudioFormat {
        AudioFormat::mono_48k()
    }

    fn pop_samples(&mut self, buf: &mut [f32]) -> anyhow::Result<Option<usize>> {
        buf.fill(0.0);
        Ok(Some(buf.len()))
    }
}

impl CallAudioInput for SilentInput {
    fn level(&self) -> f32 {
        0.0
    }
}

/// A speaker that discards what it is given.
#[derive(Debug, Clone, Default)]
pub struct SilentOutput {
    paused: Arc<AtomicBool>,
}

impl AudioSinkHandle for SilentOutput {
    fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    fn toggle_pause(&self) {
        self.paused.fetch_xor(true, Ordering::SeqCst);
    }

    fn smoothed_peak_normalized(&self) -> Option<f32> {
        Some(0.0)
    }
}

impl AudioSink for SilentOutput {
    fn format(&self) -> anyhow::Result<AudioFormat> {
        Ok(AudioFormat::stereo_48k())
    }

    fn push_samples(&mut self, _buf: &[f32]) -> anyhow::Result<()> {
        Ok(())
    }

    fn handle(&self) -> Box<dyn AudioSinkHandle> {
        Box::new(self.clone())
    }
}

impl CallAudioOutput for SilentOutput {
    fn is_active(&self) -> bool {
        true
    }
}

// ── Generated video ──────────────────────────────────────────────────

#[cfg(tutoring_video)]
pub mod video {
    use std::time::{Duration, Instant};

    use anyhow::Context;
    use bytes::Bytes;
    use live::media::av::{
        DecodeConfig, DecodedFrame, PixelFormat, VideoDecoder, VideoEncoder, VideoEncoderInner,
        VideoFormat, VideoFrame, VideoPreset, VideoSource,
    };
    use moq_mux::container::{Frame, Timestamp};

    const WIDTH: u32 = 320;
    const HEIGHT: u32 = 180;
    const FRAME: Duration = Duration::from_millis(1000 / 15);

    /// A camera showing a bar that sweeps across a gradient, so each frame
    /// differs from the last.
    pub struct TestPattern {
        started: Option<Instant>,
        last: Option<Instant>,
        count: u32,
    }

    impl Default for TestPattern {
        fn default() -> Self {
            Self::new()
        }
    }

    impl TestPattern {
        pub fn new() -> Self {
            Self {
                started: None,
                last: None,
                count: 0,
            }
        }

        fn paint(count: u32) -> Vec<u8> {
            let bar = (count * 8) % WIDTH;
            let mut rgba = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    let on_bar = x.abs_diff(bar) < 6;
                    let (r, g, b) = if on_bar {
                        (255, 255, 255)
                    } else {
                        ((x * 255 / WIDTH) as u8, (y * 255 / HEIGHT) as u8, 96)
                    };
                    rgba.extend_from_slice(&[r, g, b, 255]);
                }
            }
            rgba
        }
    }

    impl VideoSource for TestPattern {
        fn name(&self) -> &str {
            "test-pattern"
        }

        fn format(&self) -> VideoFormat {
            VideoFormat {
                pixel_format: PixelFormat::Rgba,
                dimensions: [WIDTH, HEIGHT],
            }
        }

        fn pop_frame(&mut self) -> anyhow::Result<Option<VideoFrame>> {
            if self.started.is_none() {
                return Ok(None);
            }
            let now = Instant::now();
            if self
                .last
                .is_some_and(|last| now.duration_since(last) < FRAME)
            {
                return Ok(None);
            }
            self.last = Some(now);
            self.count += 1;
            Ok(Some(VideoFrame {
                format: self.format(),
                raw: Bytes::from(Self::paint(self.count)),
            }))
        }

        fn start(&mut self) -> anyhow::Result<()> {
            self.started.get_or_insert_with(Instant::now);
            Ok(())
        }

        fn stop(&mut self) -> anyhow::Result<()> {
            self.started = None;
            Ok(())
        }
    }

    /// The codec string a raw packet is labelled with in the catalog.
    const RAW_CODEC: &str = "alexandria-raw-rgba";

    /// Packets are the frame itself: width, height, then RGBA bytes.
    pub struct RawEncoder {
        config: hang::catalog::VideoConfig,
        pending: Option<VideoFrame>,
        started: Instant,
    }

    impl VideoEncoder for RawEncoder {
        fn with_preset(preset: VideoPreset) -> anyhow::Result<Self> {
            let mut config = hang::catalog::VideoConfig::new(hang::catalog::VideoCodec::Unknown(
                RAW_CODEC.into(),
            ));
            config.coded_width = Some(preset.width());
            config.coded_height = Some(preset.height());
            config.framerate = Some(f64::from(preset.fps()));
            config.optimize_for_latency = Some(true);
            Ok(Self {
                config,
                pending: None,
                started: Instant::now(),
            })
        }
    }

    impl VideoEncoderInner for RawEncoder {
        fn name(&self) -> &str {
            RAW_CODEC
        }

        fn config(&self) -> hang::catalog::VideoConfig {
            self.config.clone()
        }

        fn push_frame(&mut self, frame: VideoFrame) -> anyhow::Result<()> {
            self.pending = Some(frame);
            Ok(())
        }

        fn pop_packet(&mut self) -> anyhow::Result<Option<Frame>> {
            let Some(frame) = self.pending.take() else {
                return Ok(None);
            };
            let [w, h] = frame.format.dimensions;
            let mut payload = Vec::with_capacity(8 + frame.raw.len());
            payload.extend_from_slice(&w.to_le_bytes());
            payload.extend_from_slice(&h.to_le_bytes());
            payload.extend_from_slice(&frame.raw);
            let micros = self.started.elapsed().as_micros() as u64;
            Ok(Some(Frame {
                timestamp: Timestamp::from_micros(micros).unwrap_or(Timestamp::ZERO),
                duration: None,
                payload: Bytes::from(payload),
                keyframe: true,
            }))
        }
    }

    /// Unpacks [`RawEncoder`] packets, fitting each inside the viewport.
    pub struct RawDecoder {
        viewport: (u32, u32),
        queue: std::collections::VecDeque<DecodedFrame>,
    }

    impl VideoDecoder for RawDecoder {
        fn new(
            config: &hang::catalog::VideoConfig,
            _playback_config: &DecodeConfig,
        ) -> anyhow::Result<Self> {
            anyhow::ensure!(
                config.codec.to_string() == RAW_CODEC,
                "the synthetic decoder only reads synthetic frames"
            );
            Ok(Self {
                viewport: (WIDTH, HEIGHT),
                queue: Default::default(),
            })
        }

        fn name(&self) -> &str {
            RAW_CODEC
        }

        fn pop_frame(&mut self) -> anyhow::Result<Option<DecodedFrame>> {
            Ok(self.queue.pop_front())
        }

        fn push_packet(&mut self, packet: Frame) -> anyhow::Result<()> {
            let bytes = &packet.payload;
            anyhow::ensure!(bytes.len() >= 8, "raw packet too short");
            let w = u32::from_le_bytes(bytes[0..4].try_into()?);
            let h = u32::from_le_bytes(bytes[4..8].try_into()?);
            let img = image::RgbaImage::from_raw(w, h, bytes[8..].to_vec())
                .context("raw packet does not hold a whole frame")?;
            let (vw, vh) = self.viewport;
            let img = if w > vw || h > vh {
                let scale = f64::min(f64::from(vw) / f64::from(w), f64::from(vh) / f64::from(h));
                image::imageops::thumbnail(
                    &img,
                    ((f64::from(w) * scale).round() as u32).max(1),
                    ((f64::from(h) * scale).round() as u32).max(1),
                )
            } else {
                img
            };
            self.queue.push_back(DecodedFrame {
                frame: image::Frame::new(img),
                timestamp: Duration::from_micros(packet.timestamp.as_micros() as u64),
            });
            Ok(())
        }

        fn set_viewport(&mut self, w: u32, h: u32) {
            self.viewport = (w.max(1), h.max(1));
        }
    }
}
