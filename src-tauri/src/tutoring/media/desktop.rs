//! Desktop media: a nokhwa camera, the xcap screen, software H.264 through
//! ffmpeg, and cpal audio on the devices the learner picked. Without a video
//! feature it makes audio-only calls on the same audio.

use std::time::Duration;

use live::media::audio::AudioBackend;
use live::media::av::Quality;
use tokio::sync::broadcast;

#[cfg(tutoring_video)]
use super::{ffmpeg, OpenedVideo};
use super::{
    AudioDeviceInventory, CameraDeviceInfo, DeviceCheckResult, DeviceSelection, FrameTuning,
    MediaDevices, MediaEvent, MediaProfile,
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
    #[cfg(tutoring_video)]
    type VideoSource = ffmpeg::Source;
    #[cfg(tutoring_video)]
    type VideoEncoder = ffmpeg::Encoder;
    #[cfg(tutoring_video)]
    type VideoDecoder = ffmpeg::Decoder;

    fn profile(&self) -> MediaProfile {
        MediaProfile {
            name: "desktop",
            video: cfg!(tutoring_video),
            screen_share: cfg!(tutoring_video),
            camera_switch: cfg!(tutoring_video),
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
            room_open_timeout: None,
            start_device_timeout: None,
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
        ffmpeg::open_camera(camera, |camera| Some(camera.device_name().to_string()))
    }

    #[cfg(tutoring_video)]
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Without a video feature the desktop still calls on its real audio:
    /// the adapter is the desktop one, and it says video is off rather than
    /// silently carrying generated media.
    #[cfg(not(tutoring_video))]
    #[tokio::test]
    async fn a_desktop_build_without_video_makes_audio_only_calls() {
        assert!(std::any::type_name::<crate::tutoring::PlatformMedia>().ends_with("DesktopMedia"));
        let media = DesktopMedia::new();
        assert!(!media.profile().video);
        assert!(media.cameras().await.is_empty());
        // Enumeration only; nothing is opened.
        let check = media.check_devices().await;
        assert!(!check.has_camera);
        assert_eq!(
            check.error.as_deref(),
            Some(super::super::VIDEO_DISABLED_ERROR)
        );
    }

    #[cfg(tutoring_video)]
    #[test]
    fn a_desktop_build_with_video_publishes_and_shares_its_screen() {
        assert!(std::any::type_name::<crate::tutoring::PlatformMedia>().ends_with("DesktopMedia"));
        let profile = DesktopMedia::new().profile();
        assert!(profile.video && profile.screen_share && profile.camera_switch);
        assert!(profile.start_device_timeout.is_none() && profile.room_open_timeout.is_none());
    }
}
