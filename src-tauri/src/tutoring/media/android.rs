//! Android media: the NDK Camera2 camera over the same ffmpeg pipeline as
//! the desktop, cpal audio, no screen share. Camera2 does not name its
//! device, so the webview preview is not matched against it.

use std::time::Duration;

use live::media::audio::AudioBackend;
use live::media::av::Quality;
use tokio::sync::broadcast;

use super::ffmpeg;
use super::{
    AudioDeviceInventory, CameraDeviceInfo, DeviceSelection, FrameTuning, MediaDevices, MediaEvent,
    MediaProfile, OpenedVideo,
};

pub struct AndroidMedia {
    events: broadcast::Sender<MediaEvent>,
}

impl Default for AndroidMedia {
    fn default() -> Self {
        Self::new()
    }
}

impl AndroidMedia {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(4);
        Self { events }
    }
}

impl MediaDevices for AndroidMedia {
    type Audio = AudioBackend;
    type VideoSource = ffmpeg::Source;
    type VideoEncoder = ffmpeg::Encoder;
    type VideoDecoder = ffmpeg::Decoder;

    fn profile(&self) -> MediaProfile {
        MediaProfile {
            name: "android",
            video: true,
            screen_share: false,
            camera_switch: true,
            audio_route_switch: false,
            remote_quality: Quality::Highest,
            self_view: FrameTuning {
                viewport: (960, 540),
                jpeg_max: (u32::MAX, u32::MAX),
                jpeg_quality: 80,
                interval: Duration::from_millis(1000 / 30),
            },
            remote_view: FrameTuning {
                viewport: (960, 960),
                jpeg_max: (u32::MAX, u32::MAX),
                jpeg_quality: 82,
                interval: Duration::from_millis(1000 / 30),
            },
            audio_level_interval: Duration::from_millis(50),
        }
    }

    async fn cameras(&self) -> Vec<CameraDeviceInfo> {
        ffmpeg::list_cameras().await
    }

    fn audio_devices(&self) -> AudioDeviceInventory {
        super::cpal_inventory()
    }

    fn open_audio(&self, selection: &DeviceSelection) -> Result<AudioBackend, String> {
        Ok(AudioBackend::new_with_devices(
            super::parse_device_id(selection.mic_device_id.as_deref()),
            super::parse_device_id(selection.speaker_device_id.as_deref()),
        ))
    }

    fn open_camera(&self, camera: Option<&str>) -> Result<OpenedVideo<ffmpeg::Source>, String> {
        ffmpeg::open_camera(camera, |_| Some(String::new()))
    }

    fn open_screen(&self) -> Result<OpenedVideo<ffmpeg::Source>, String> {
        Err("Screen sharing is not available on mobile".into())
    }

    fn set_audio_route(&self, _selection: &DeviceSelection) -> Result<(), String> {
        Err("Changing audio devices during a live tutoring session is currently supported on iPhone and iPad only".into())
    }

    fn events(&self) -> broadcast::Receiver<MediaEvent> {
        self.events.subscribe()
    }
}
