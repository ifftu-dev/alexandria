//! Android media: the NDK Camera2 camera over the same ffmpeg pipeline as
//! the desktop, cpal audio, no screen share, and a phone's frame and
//! rendition choices. Camera2 does not name its device, so no camera name is
//! reported. Without a video feature it makes audio-only calls.

use live::media::audio::AudioBackend;
use tokio::sync::broadcast;

#[cfg(tutoring_video)]
use super::{ffmpeg, OpenedVideo};
use super::{
    AudioDeviceInventory, CameraDeviceInfo, DeviceCheckResult, DeviceSelection, MediaDevices,
    MediaEvent, MediaProfile,
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
    #[cfg(tutoring_video)]
    type VideoSource = ffmpeg::Source;
    #[cfg(tutoring_video)]
    type VideoEncoder = ffmpeg::Encoder;
    #[cfg(tutoring_video)]
    type VideoDecoder = ffmpeg::Decoder;

    fn profile(&self) -> MediaProfile {
        MediaProfile {
            name: "android",
            video: cfg!(tutoring_video),
            screen_share: false,
            camera_switch: cfg!(tutoring_video),
            audio_route_switch: false,
            ..super::phone_profile()
        }
    }

    async fn cameras(&self) -> Vec<CameraDeviceInfo> {
        #[cfg(tutoring_video)]
        return ffmpeg::list_cameras().await;
        #[cfg(not(tutoring_video))]
        Vec::new()
    }

    fn audio_devices(&self) -> AudioDeviceInventory {
        super::cpal_inventory()
    }

    async fn check_devices(&self) -> DeviceCheckResult {
        let cameras = self.cameras().await;
        DeviceCheckResult {
            has_camera: !cameras.is_empty(),
            camera_name: cameras.first().map(|c| c.name.clone()),
            has_audio: super::has_cpal_input(),
            error: (!cfg!(tutoring_video)).then(|| super::VIDEO_DISABLED_ERROR.into()),
        }
    }

    fn open_audio(&self, selection: &DeviceSelection) -> Result<AudioBackend, String> {
        Ok(AudioBackend::new_with_devices(
            super::parse_device_id(selection.mic_device_id.as_deref()),
            super::parse_device_id(selection.speaker_device_id.as_deref()),
        ))
    }

    #[cfg(tutoring_video)]
    fn open_camera(&self, camera: Option<&str>) -> Result<OpenedVideo<ffmpeg::Source>, String> {
        ffmpeg::open_camera(camera, |_| None)
    }

    #[cfg(tutoring_video)]
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
