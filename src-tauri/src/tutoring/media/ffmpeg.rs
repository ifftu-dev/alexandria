//! The ffmpeg video pipeline the desktop and Android adapters share: a
//! `CameraCapturer` (nokhwa on the desktop, NDK Camera2 on Android) or an
//! `xcap` screen, software H.264 in, ffmpeg decode out.

use live::media::av::{VideoFormat, VideoFrame, VideoPreset, VideoSource};
use live::media::capture::{CameraCapturer, CameraIndex, ScreenCapturer};

use super::{CameraDeviceInfo, OpenedVideo};

pub(super) use live::media::ffmpeg::{FfmpegVideoDecoder as Decoder, H264Encoder as Encoder};

/// A camera or the screen, behind one source type for the seam.
pub enum Source {
    Camera(CameraCapturer),
    Screen(ScreenCapturer),
}

impl VideoSource for Source {
    fn name(&self) -> &str {
        match self {
            Source::Camera(camera) => camera.name(),
            Source::Screen(screen) => screen.name(),
        }
    }

    fn format(&self) -> VideoFormat {
        match self {
            Source::Camera(camera) => camera.format(),
            Source::Screen(screen) => screen.format(),
        }
    }

    fn pop_frame(&mut self) -> anyhow::Result<Option<VideoFrame>> {
        match self {
            Source::Camera(camera) => camera.pop_frame(),
            Source::Screen(screen) => screen.pop_frame(),
        }
    }

    fn start(&mut self) -> anyhow::Result<()> {
        match self {
            Source::Camera(camera) => camera.start(),
            Source::Screen(screen) => screen.start(),
        }
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        match self {
            Source::Camera(camera) => camera.stop(),
            Source::Screen(screen) => screen.stop(),
        }
    }
}

/// Parse a camera index string (Debug-formatted `CameraIndex`) back to the
/// enum. Accepts "Index(0)", "String(\"FaceTime HD Camera\")", or a plain
/// number.
pub(super) fn parse_camera_index(s: &str) -> Option<CameraIndex> {
    if let Some(inner) = s.strip_prefix("Index(").and_then(|s| s.strip_suffix(')')) {
        inner.parse::<u32>().ok().map(CameraIndex::Index)
    } else if let Some(inner) = s
        .strip_prefix("String(\"")
        .and_then(|s| s.strip_suffix("\")"))
    {
        Some(CameraIndex::String(inner.to_string()))
    } else if let Ok(n) = s.parse::<u32>() {
        Some(CameraIndex::Index(n))
    } else {
        None
    }
}

/// Enumerate cameras on a blocking thread with a timeout: on macOS the
/// first enumeration can block on the permission dialog.
pub(super) async fn list_cameras() -> Vec<CameraDeviceInfo> {
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::task::spawn_blocking(CameraCapturer::list_cameras),
    )
    .await
    {
        Ok(Ok(Ok(cams))) => cams
            .into_iter()
            .map(|(idx, name)| CameraDeviceInfo {
                index: format!("{idx:?}"),
                name,
            })
            .collect(),
        Ok(Ok(Err(e))) => {
            log::warn!("tutoring: camera enumeration failed: {e}");
            Vec::new()
        }
        Ok(Err(e)) => {
            log::warn!("tutoring: camera enumeration task panicked: {e}");
            Vec::new()
        }
        Err(_) => {
            log::warn!("tutoring: camera enumeration timed out (5s) — may need camera permission");
            Vec::new()
        }
    }
}

/// Open a camera by index (or the preferred one). `label` names it as the
/// webview labels the same device, where the platform knows.
///
/// The renditions stop at the capture height: offering 1080p from a 720p
/// capture made the encoder upscale and every phone decode 2.25x the pixels
/// for nothing (15 Oct).
pub(super) fn open_camera(
    camera: Option<&str>,
    label: impl FnOnce(&CameraCapturer) -> Option<String>,
) -> Result<OpenedVideo<Source>, String> {
    let index = camera.and_then(parse_camera_index);
    let camera =
        CameraCapturer::with_index(index).map_err(|e| format!("camera unavailable: {e}"))?;
    let camera_name = label(&camera);
    let presets = VideoPreset::up_to(camera.format().dimensions[1]);
    Ok(OpenedVideo {
        source: Source::Camera(camera),
        camera_name,
        presets,
    })
}

/// Open the screen for sharing, at every rendition.
pub(super) fn open_screen() -> Result<OpenedVideo<Source>, String> {
    let screen = ScreenCapturer::new().map_err(|e| format!("screen capture unavailable: {e}"))?;
    Ok(OpenedVideo {
        source: Source::Screen(screen),
        camera_name: None,
        presets: VideoPreset::all().to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_indexes_round_trip_from_their_debug_form() {
        assert!(matches!(
            parse_camera_index("Index(2)"),
            Some(CameraIndex::Index(2))
        ));
        assert!(matches!(
            parse_camera_index("String(\"FaceTime HD Camera\")"),
            Some(CameraIndex::String(name)) if name == "FaceTime HD Camera"
        ));
        assert!(matches!(
            parse_camera_index("1"),
            Some(CameraIndex::Index(1))
        ));
        assert!(parse_camera_index("front").is_none());
    }
}
