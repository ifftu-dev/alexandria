//! Backend IPC surface for Sentinel gaze / second-device detection.
//!
//! The frontend forwards a downscaled camera frame; everything (YuNet
//! detection, pose extraction, calibration training + scoring) runs in
//! the Rust backend. Frames are processed in memory and never persisted
//! — only the derived `GazeEstimate` crosses back over IPC. Mirrors the
//! storage + command conventions in `sentinel_ml.rs`; the per-user
//! calibration model reuses the `sentinel_user_models` table under
//! `model_kind = 'gaze_calib'`.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::sentinel_ml::{load_user_model, save_user_model};
use crate::sentinel::face_detect;
use crate::sentinel::gaze::{self, GazeCalibWeights, GazeCalibrator, GazeFeatures};
use crate::sentinel::types::{FaceDetection, FaceFrame, GazeCalibSample, GazeEstimate};
use crate::AppState;

const GAZE_CALIB_KIND: &str = "gaze_calib";

/// Detect faces in a frame (bbox + 5 landmarks + score). Stateless;
/// replaces the legacy JS skin-color detector.
#[tauri::command]
pub async fn sentinel_detect_face(frame: FaceFrame) -> Result<Vec<FaceDetection>, String> {
    let t0 = std::time::Instant::now();
    let dets = face_detect::detect(&frame).map_err(|e| e.to_string())?;
    log::trace!(
        target: "sentinel",
        "detect_face: {}x{} faces={} total={}ms",
        frame.width,
        frame.height,
        dets.len(),
        t0.elapsed().as_millis(),
    );
    Ok(dets)
}

/// Extract head-pose + iris features for the highest-confidence face in
/// a frame. Returns `None` if no usable face. Used by the wizard's
/// 9-point calibration capture so feature extraction stays in one place
/// (no frontend re-implementation to drift).
#[tauri::command]
pub async fn sentinel_extract_gaze_features(
    frame: FaceFrame,
) -> Result<Option<GazeFeatures>, String> {
    let dets = face_detect::detect(&frame).map_err(|e| e.to_string())?;
    let Some(best) = pick_best(&dets) else {
        return Ok(None);
    };
    Ok(gaze::extract_features(&frame, best))
}

#[derive(Debug, Deserialize)]
pub struct ScoreGazeRequest {
    pub frame: FaceFrame,
    pub user_address: String,
    pub device_fp_prefix: String,
}

#[derive(Debug, Serialize)]
pub struct ScoreGazeResponse {
    pub estimate: GazeEstimate,
    #[serde(rename = "faceCount")]
    pub face_count: usize,
    /// Highest-confidence detection (bbox + landmarks) for overlay, so a
    /// caller needs only this one YuNet pass — no separate detect call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detection: Option<FaceDetection>,
}

/// Detect → load per-user calibration (if any) → estimate gaze. The
/// `face_count` lets the frontend keep emitting the existing
/// `multiple_faces` / `no_face` flags without a second round-trip.
#[tauri::command]
pub async fn sentinel_score_gaze(
    state: State<'_, AppState>,
    req: ScoreGazeRequest,
) -> Result<ScoreGazeResponse, String> {
    let t0 = std::time::Instant::now();
    let dets = face_detect::detect(&req.frame).map_err(|e| e.to_string())?;
    let best = pick_best(&dets);

    let calib = match load_user_model::<GazeCalibWeights>(
        &state,
        &req.user_address,
        &req.device_fp_prefix,
        GAZE_CALIB_KIND,
    )? {
        Some(w) => Some(GazeCalibrator::from_weights(&w).map_err(|e| e.to_string())?),
        None => None,
    };

    let estimate = gaze::estimate(&req.frame, best, calib.as_ref()).map_err(|e| e.to_string())?;
    log::trace!(
        target: "sentinel",
        "score_gaze: faces={} on_screen={} occluded={} total={}ms",
        dets.len(),
        estimate.on_screen,
        estimate.occluded,
        t0.elapsed().as_millis(),
    );
    Ok(ScoreGazeResponse {
        estimate,
        face_count: dets.len(),
        detection: best.cloned(),
    })
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
    let mut calib = match load_user_model::<GazeCalibWeights>(
        &state,
        &req.user_address,
        &req.device_fp_prefix,
        GAZE_CALIB_KIND,
    )? {
        Some(w) => GazeCalibrator::from_weights(&w).map_err(|e| e.to_string())?,
        None => GazeCalibrator::new().map_err(|e| e.to_string())?,
    };
    let epochs = req.epochs.unwrap_or_else(gaze::default_epochs);
    let loss = calib
        .train(&req.samples, epochs)
        .map_err(|e| e.to_string())?;
    let weights = calib.export_weights().map_err(|e| e.to_string())?;
    save_user_model(
        &state,
        &req.user_address,
        &req.device_fp_prefix,
        GAZE_CALIB_KIND,
        &weights,
        weights.train_loss,
        weights.trained_epochs,
        weights.training_samples,
    )?;
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
pub async fn sentinel_frontmost_app() -> Option<crate::sentinel::active_app::ActiveApp> {
    crate::sentinel::active_app::frontmost_app()
}

/// Highest-score detection in the set.
fn pick_best(dets: &[FaceDetection]) -> Option<&FaceDetection> {
    dets.iter().max_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}
