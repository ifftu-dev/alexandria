//! Backend IPC surface for Sentinel gaze / second-device detection.
//!
//! The frontend forwards a downscaled camera frame as a raw RGBA8 body
//! (`Uint8Array`), with its size and the caller's identity in request
//! headers; no frame crosses IPC as a JSON number array. Session and
//! preview frames go to the camera inference worker
//! (`sentinel::camera_pipeline`), which detects on a slow cadence, tracks
//! landmarks in between, runs liveness on trigger and drops a frame that
//! arrives while the previous one in its lane is still waiting. Frames are
//! processed in memory and never persisted — only the derived tick crosses
//! back over IPC. The per-user calibration model reuses the
//! `sentinel_user_models` table under `model_kind = 'gaze_calib'`.

use crate::profile::scope::ProfileState as State;
use serde::{Deserialize, Serialize};
use tauri::http::HeaderMap;
use tauri::ipc::{InvokeBody, Request};

use crate::commands::sentinel_ml::{load_user_model, save_user_model};
use crate::sentinel::camera_pipeline::{self, CameraJob, GazeTick, Lane, Outcome};
use crate::sentinel::face_detect;
use crate::sentinel::gaze::{self, GazeCalibWeights, GazeCalibrator, GazeFeatures};
use crate::sentinel::types::{FaceDetection, FaceFrame, GazeCalibSample};
use crate::AppState;

const GAZE_CALIB_KIND: &str = "gaze_calib";

/// Frame width in pixels; the body is `width * height * 4` RGBA bytes.
pub const FRAME_WIDTH_HEADER: &str = "x-sentinel-frame-width";
/// Frame height in pixels.
pub const FRAME_HEIGHT_HEADER: &str = "x-sentinel-frame-height";
/// Stake address whose calibration model scores the frame.
pub const USER_HEADER: &str = "x-sentinel-user";
/// 16-hex device fingerprint prefix the calibration model is keyed by.
pub const DEVICE_HEADER: &str = "x-sentinel-device";
/// `1` for a standalone preview that must not supply assessment evidence.
pub const PREVIEW_HEADER: &str = "x-sentinel-preview";

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, String> {
    headers
        .get(name)
        .ok_or_else(|| format!("missing {name} header"))?
        .to_str()
        .map_err(|_| format!("{name} header is not ASCII"))
}

fn dimension(headers: &HeaderMap, name: &str) -> Result<u32, String> {
    header(headers, name)?
        .parse()
        .map_err(|_| format!("{name} header is not a pixel count"))
}

/// Read a frame from a raw IPC body plus its size headers.
pub fn frame_from_parts(headers: &HeaderMap, body: &InvokeBody) -> Result<FaceFrame, String> {
    let width = dimension(headers, FRAME_WIDTH_HEADER)?;
    let height = dimension(headers, FRAME_HEIGHT_HEADER)?;
    match body {
        InvokeBody::Raw(bytes) => FaceFrame::from_raw(width, height, bytes.clone()),
        InvokeBody::Json(_) => Err("camera frames must be sent as a raw byte body".into()),
    }
}

fn frame_from_request(request: &Request<'_>) -> Result<FaceFrame, String> {
    frame_from_parts(request.headers(), request.body())
}

/// Identity + mode for a scored frame, carried in headers beside the body.
#[derive(Debug)]
pub struct ScoreGazeMeta {
    pub user_address: String,
    pub device_fp_prefix: String,
    pub preview_only: bool,
}

impl ScoreGazeMeta {
    pub fn from_headers(headers: &HeaderMap) -> Result<Self, String> {
        Ok(ScoreGazeMeta {
            user_address: header(headers, USER_HEADER)?.to_string(),
            device_fp_prefix: header(headers, DEVICE_HEADER)?.to_string(),
            preview_only: headers
                .get(PREVIEW_HEADER)
                .is_some_and(|v| v.as_bytes() == b"1"),
        })
    }
}

/// Detect faces in a frame (bbox + 5 landmarks + score). Stateless: always
/// a full detection pass, never a tracked one.
#[tauri::command]
pub async fn sentinel_detect_face(
    _profile: crate::profile::scope::ProfileLease,
    request: Request<'_>,
) -> Result<Vec<FaceDetection>, String> {
    let frame = frame_from_request(&request)?;
    tokio::task::spawn_blocking(move || face_detect::detect(&frame).map_err(|e| e.to_string()))
        .await
        .map_err(|e| format!("face detection worker failed: {e}"))?
}

/// Extract head-pose + iris features for the highest-confidence face in
/// a frame. Returns `None` if no usable face. Used by the wizard's
/// 9-point calibration capture, so it always runs a full detection: a
/// calibration sample must not come from a tracked estimate.
#[tauri::command]
pub async fn sentinel_extract_gaze_features(
    _profile: crate::profile::scope::ProfileLease,
    request: Request<'_>,
) -> Result<Option<GazeFeatures>, String> {
    let frame = frame_from_request(&request)?;
    tokio::task::spawn_blocking(move || {
        let dets = face_detect::detect(&frame).map_err(|e| e.to_string())?;
        Ok::<_, String>(
            camera_pipeline::best_detection(&dets)
                .and_then(|best| gaze::extract_features(&frame, best)),
        )
    })
    .await
    .map_err(|e| format!("gaze feature worker failed: {e}"))?
}

/// Reply to a scored frame: the tick, or `{ "dropped": true }` when a newer
/// frame in the same lane replaced this one before inference reached it.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ScoreGazeReply {
    Scored(GazeTick),
    Dropped { dropped: bool },
}

/// Submit a frame to the camera worker → gaze estimate, face count, best
/// (detected or tracked) face, and liveness when a trigger ran it. The
/// `face_count` lets the frontend keep emitting `multiple_faces` /
/// `no_face` without a second round-trip; between detections it carries
/// the last detection's count.
#[tauri::command]
pub async fn sentinel_score_gaze(
    state: State<'_, AppState>,
    request: Request<'_>,
) -> Result<ScoreGazeReply, String> {
    let t0 = std::time::Instant::now();
    let meta = ScoreGazeMeta::from_headers(request.headers())?;
    let frame = frame_from_request(&request)?;

    // Park this frame in memory in case the snapshot about to be written turns
    // out to be flagged. Nothing is persisted here, and an unflagged snapshot
    // lets the next frame overwrite it — see `sentinel::evidence`.
    if !meta.preview_only {
        state.evidence_staging.remember_frame(
            frame.width,
            frame.height,
            &frame.rgba,
            &crate::clock::now_rfc3339(),
        );
    }

    let calib = load_user_model::<GazeCalibWeights>(
        &state,
        &meta.user_address,
        &meta.device_fp_prefix,
        GAZE_CALIB_KIND,
    )
    .await?;

    let lane = if meta.preview_only {
        Lane::Preview
    } else {
        Lane::Session
    };
    let outcome = camera_pipeline::submit(
        lane,
        CameraJob {
            frame,
            calib,
            at: std::time::Instant::now(),
        },
    )
    .await
    .map_err(|_| "camera inference worker stopped".to_string())?;
    let reply = match outcome {
        Outcome::Done(tick) => ScoreGazeReply::Scored(tick?),
        Outcome::Dropped => ScoreGazeReply::Dropped { dropped: true },
    };
    if let ScoreGazeReply::Scored(tick) = &reply {
        log::trace!(
            target: "sentinel",
            "score_gaze: faces={} tracked={} liveness={} total={}ms",
            tick.face_count,
            tick.tracked,
            tick.liveness.is_some(),
            t0.elapsed().as_millis(),
        );
    }
    Ok(reply)
}

#[derive(Debug, Deserialize)]
pub struct TrainGazeCalibRequest {
    pub user_address: String,
    pub device_fp_prefix: String,
    pub samples: Vec<GazeCalibSample>,
    #[serde(default)]
    pub epochs: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct TrainGazeCalibResponse {
    pub train_loss: f32,
    pub training_samples: usize,
    pub trained_epochs: usize,
}

/// Fit (or refit) the per-user gaze calibration MLP from the wizard's
/// 9-point capture and persist it.
#[tauri::command]
pub async fn sentinel_train_gaze_calib(
    state: State<'_, AppState>,
    req: TrainGazeCalibRequest,
) -> Result<TrainGazeCalibResponse, String> {
    let existing = load_user_model::<GazeCalibWeights>(
        &state,
        &req.user_address,
        &req.device_fp_prefix,
        GAZE_CALIB_KIND,
    )
    .await?;
    let epochs = req.epochs.unwrap_or_else(gaze::default_epochs);
    let samples = req.samples;
    let (loss, weights) = tokio::task::spawn_blocking(move || {
        let mut calib = match existing {
            Some(weights) => GazeCalibrator::from_weights(&weights).map_err(|e| e.to_string())?,
            None => GazeCalibrator::new().map_err(|e| e.to_string())?,
        };
        let loss = calib.train(&samples, epochs).map_err(|e| e.to_string())?;
        let weights = calib.export_weights().map_err(|e| e.to_string())?;
        Ok::<_, String>((loss, weights))
    })
    .await
    .map_err(|e| format!("gaze training worker failed: {e}"))??;
    save_user_model(
        &state,
        &req.user_address,
        &req.device_fp_prefix,
        GAZE_CALIB_KIND,
        &weights,
        weights.train_loss,
        weights.trained_epochs,
        weights.training_samples,
    )
    .await?;
    Ok(TrainGazeCalibResponse {
        train_loss: loss,
        training_samples: weights.training_samples,
        trained_epochs: weights.trained_epochs,
    })
}

/// Report the OS frontmost application — used when the assessment window
/// loses focus to identify what the learner switched to. `None` if it
/// can't be resolved (unsupported platform, Wayland, permission).
#[tauri::command]
pub async fn sentinel_frontmost_app(
    _profile: crate::profile::scope::ProfileLease,
) -> Option<crate::sentinel::active_app::ActiveApp> {
    crate::sentinel::active_app::frontmost_app()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.insert(*k, v.parse().unwrap());
        }
        map
    }

    #[test]
    fn a_frame_crosses_ipc_as_raw_bytes_with_its_size_in_headers() {
        let h = headers(&[(FRAME_WIDTH_HEADER, "4"), (FRAME_HEIGHT_HEADER, "2")]);
        let frame = frame_from_parts(&h, &InvokeBody::Raw(vec![9; 32])).unwrap();
        assert_eq!((frame.width, frame.height), (4, 2));
        assert_eq!(frame.rgba, vec![9; 32]);
    }

    #[test]
    fn a_json_body_or_missing_size_is_refused() {
        let h = headers(&[(FRAME_WIDTH_HEADER, "4"), (FRAME_HEIGHT_HEADER, "2")]);
        let json = InvokeBody::Json(serde_json::json!({ "rgba": [0, 0, 0, 0] }));
        assert!(frame_from_parts(&h, &json).is_err());
        let no_height = headers(&[(FRAME_WIDTH_HEADER, "4")]);
        assert!(frame_from_parts(&no_height, &InvokeBody::Raw(vec![0; 32])).is_err());
        let bad = headers(&[(FRAME_WIDTH_HEADER, "four"), (FRAME_HEIGHT_HEADER, "2")]);
        assert!(frame_from_parts(&bad, &InvokeBody::Raw(vec![0; 32])).is_err());
    }

    #[test]
    fn score_metadata_reads_identity_and_preview_from_headers() {
        let h = headers(&[
            (USER_HEADER, "stake_test1abc"),
            (DEVICE_HEADER, "0123456789abcdef"),
            (PREVIEW_HEADER, "1"),
        ]);
        let meta = ScoreGazeMeta::from_headers(&h).unwrap();
        assert_eq!(meta.user_address, "stake_test1abc");
        assert_eq!(meta.device_fp_prefix, "0123456789abcdef");
        assert!(meta.preview_only);
        let session = headers(&[(USER_HEADER, "u"), (DEVICE_HEADER, "d")]);
        assert!(!ScoreGazeMeta::from_headers(&session).unwrap().preview_only);
        assert!(ScoreGazeMeta::from_headers(&headers(&[(USER_HEADER, "u")])).is_err());
    }

    #[test]
    fn a_dropped_frame_replies_with_only_the_dropped_marker() {
        let reply = serde_json::to_value(ScoreGazeReply::Dropped { dropped: true }).unwrap();
        assert_eq!(reply, serde_json::json!({ "dropped": true }));
    }
}
