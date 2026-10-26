//! Desktop media: a nokhwa camera, the xcap screen, software H.264 through
//! ffmpeg, and cpal audio on the devices the learner picked.

use std::time::Duration;

use live::media::audio::AudioBackend;
use live::media::av::Quality;
use tokio::sync::broadcast;

use super::ffmpeg;
use super::{
    AudioDeviceInventory, CameraDeviceInfo, DeviceSelection, FrameTuning, MediaDevices, MediaEvent,
    MediaProfile, OpenedVideo,
};

pub struct DesktopMedia {
    /// Desktops raise no route or lifecycle events; the sender lives here so
    /// receivers stay open for the length of a session.
    events: broadcast::Sender<MediaEvent>,
}

impl Default for DesktopMedia {
    fn default() -> Self {
        Self::new()
    }
}

impl DesktopMedia {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(4);
        Self { events }
    }
}

impl MediaDevices for DesktopMedia {
    type Audio = AudioBackend;
    type VideoSource = ffmpeg::Source;
    type VideoEncoder = ffmpeg::Encoder;
    type VideoDecoder = ffmpeg::Decoder;

    fn profile(&self) -> MediaProfile {
        MediaProfile {
            name: "desktop",
            video: true,
            screen_share: true,
            camera_switch: true,
            audio_route_switch: false,
            remote_quality: Quality::Highest,
            // The page shows the camera itself when it can; these frames are
            // the fallback (and the shared screen). 960x540 fills a tile
            // without the IPC cost of full 720p frames thirty times a second.
            // The decoder already fitted the frame, so the JPEG step never
            // scales again.
            self_view: FrameTuning {
                viewport: (960, 540),
                jpeg_max: (u32::MAX, u32::MAX),
                jpeg_quality: 80,
                interval: Duration::from_millis(1000 / 30),
            },
            // Fit inside 960x960: a landscape camera arrives at 960x540 and
            // a phone held upright at 540x960, instead of both squeezed into
            // 640x360.
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
        ffmpeg::open_camera(camera, |camera| Some(camera.device_name().to_string()))
    }

    fn open_screen(&self) -> Result<OpenedVideo<ffmpeg::Source>, String> {
        ffmpeg::open_screen()
    }

    fn set_audio_route(&self, _selection: &DeviceSelection) -> Result<(), String> {
        Err("Changing audio devices during a live tutoring session is currently supported on iPhone and iPad only".into())
    }

    fn events(&self) -> broadcast::Receiver<MediaEvent> {
        self.events.subscribe()
    }
}
