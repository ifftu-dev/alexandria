//! Gaze / second-device detection — head-pose proxies + per-user
//! calibration MLP (candle).
//!
//! Phase 1 leans on **head-pose geometry** derived from YuNet's 5
//! landmarks: looking down at a phone or sideways at a second monitor
//! moves the head, and that shows up as nose-vs-eye-line offsets. A
//! coarse iris offset (darkest-region centroid in an eye box) is added
//! as a refinement feature; precise point-of-regard waits on a real
//! iris model (Phase 2, MediaPipe).
//!
//! Two paths:
//!   * **uncalibrated** — threshold the raw yaw/pitch proxies against a
//!     generous on-screen cone. Coarse but needs no enrollment.
//!   * **calibrated** — a tiny `5 → 16 → 2` MLP (trained via candle SGD
//!     on the wizard's 9-point capture) maps pose+iris features to a
//!     normalized screen point. The calibration data is the user's own,
//!     so no external gaze dataset — and no dataset license — is
//!     involved. Weights persist as JSON in `sentinel_user_models`
//!     (`model_kind = 'gaze_calib'`), exactly like the keystroke AE /
//!     mouse CNN.
//!
//! Mirrors `mouse_cnn.rs` for the candle training/serialization shape.

use anyhow::{anyhow, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::{linear, ops, Linear, Module, Optimizer, VarBuilder, VarMap};
use serde::{Deserialize, Serialize};

use super::types::{
    FaceDetection, FaceFrame, GazeCalibSample, GazeEstimate, LM_LEFT_EYE, LM_LEFT_MOUTH, LM_NOSE,
    LM_RIGHT_EYE, LM_RIGHT_MOUTH,
};

/// Calibration MLP dims.
const FEATURE_DIM: usize = 5; // yaw, pitch, roll, iris_dx, iris_dy
const HIDDEN: usize = 16;
const OUTPUT_DIM: usize = 2; // screen x, y in [0,1]
const LEARNING_RATE: f64 = 0.01;
const DEFAULT_EPOCHS: usize = 200;

/// Nominal nose-between-eyes-and-mouth ratio for a frontal face; we
/// subtract it so a forward-looking `pitch` proxy sits near 0.
const PITCH_NOMINAL: f32 = 0.5;

/// Uncalibrated on-screen cone: |yaw| and |pitch| proxy bounds beyond
/// which the learner is judged to be looking away.
const CONE_YAW: f32 = 0.32;
const CONE_PITCH: f32 = 0.30;

/// Calibrated on-screen margin around the unit screen square.
const SCREEN_MARGIN: f32 = 0.15;

/// Eye box side as a fraction of inter-ocular distance, for the coarse
/// iris-offset feature.
const EYE_BOX_FRAC: f32 = 0.5;

/// Raw per-frame gaze features. Single source of truth shared by the
/// live scoring path and the wizard's calibration capture, so the two
/// never drift (same contract as the paste featurizer).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct GazeFeatures {
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    #[serde(rename = "irisDx")]
    pub iris_dx: f32,
    #[serde(rename = "irisDy")]
    pub iris_dy: f32,
}

impl GazeFeatures {
    fn to_array(self) -> [f32; FEATURE_DIM] {
        [self.yaw, self.pitch, self.roll, self.iris_dx, self.iris_dy]
    }
}

/// Extract head-pose proxies + coarse iris offset from a detected face.
/// Returns `None` if the face geometry is degenerate (eyes coincident)
/// — the caller treats that as occlusion.
pub fn extract_features(frame: &FaceFrame, det: &FaceDetection) -> Option<GazeFeatures> {
    let eye_r = det.landmarks5[LM_RIGHT_EYE];
    let eye_l = det.landmarks5[LM_LEFT_EYE];
    let nose = det.landmarks5[LM_NOSE];
    let mouth_r = det.landmarks5[LM_RIGHT_MOUTH];
    let mouth_l = det.landmarks5[LM_LEFT_MOUTH];

    let dx = eye_l[0] - eye_r[0];
    let dy = eye_l[1] - eye_r[1];
    let interocular = (dx * dx + dy * dy).sqrt();
    if interocular < 1e-3 {
        return None;
    }

    let eye_mid = [(eye_r[0] + eye_l[0]) * 0.5, (eye_r[1] + eye_l[1]) * 0.5];
    let mouth_mid = [
        (mouth_r[0] + mouth_l[0]) * 0.5,
        (mouth_r[1] + mouth_l[1]) * 0.5,
    ];

    // Head roll from the eye-line angle, normalized by π → ~[-1,1].
    let roll = dy.atan2(dx) / std::f32::consts::PI;
    // Yaw proxy: horizontal nose offset from the eye midpoint. Turning
    // the head left/right slides the nose across the eye line.
    let yaw = (nose[0] - eye_mid[0]) / interocular;
    // Pitch proxy: where the nose sits between the eye line and the
    // mouth line. Looking up/down changes this ratio.
    let span = (mouth_mid[1] - eye_mid[1]).abs().max(interocular * 0.1);
    let pitch = ((nose[1] - eye_mid[1]) / span) - PITCH_NOMINAL;

    let box_side = interocular * EYE_BOX_FRAC;
    let (rdx, rdy) = iris_offset(frame, eye_r, box_side);
    let (ldx, ldy) = iris_offset(frame, eye_l, box_side);
    let iris_dx = (rdx + ldx) * 0.5;
    let iris_dy = (rdy + ldy) * 0.5;

    Some(GazeFeatures {
        yaw,
        pitch,
        roll,
        iris_dx,
        iris_dy,
    })
}

/// Coarse pupil offset within an eye box: darkness-weighted centroid of
/// the box, expressed relative to the box centre and normalized to
/// `[-1, 1]`. Pure pixel math — no model. Phase 2 replaces this with a
/// real iris landmarker.
fn iris_offset(frame: &FaceFrame, eye: [f32; 2], box_side: f32) -> (f32, f32) {
    let half = (box_side * 0.5).max(1.0);
    let x0 = ((eye[0] - half).floor() as i64).max(0) as u32;
    let y0 = ((eye[1] - half).floor() as i64).max(0) as u32;
    let x1 = ((eye[0] + half).ceil() as i64)
        .min(frame.width as i64 - 1)
        .max(0) as u32;
    let y1 = ((eye[1] + half).ceil() as i64)
        .min(frame.height as i64 - 1)
        .max(0) as u32;
    if x1 <= x0 || y1 <= y0 {
        return (0.0, 0.0);
    }
    let mut sum_w = 0.0_f32;
    let mut sum_wx = 0.0_f32;
    let mut sum_wy = 0.0_f32;
    for py in y0..=y1 {
        for px in x0..=x1 {
            let idx = ((py * frame.width + px) * 4) as usize;
            if idx + 2 >= frame.rgba.len() {
                continue;
            }
            let r = frame.rgba[idx] as f32;
            let g = frame.rgba[idx + 1] as f32;
            let b = frame.rgba[idx + 2] as f32;
            let gray = 0.299 * r + 0.587 * g + 0.114 * b;
            // Darker → heavier (pupil is the darkest region).
            let w = (255.0 - gray).max(0.0);
            let w = w * w; // sharpen toward the darkest cluster
            sum_w += w;
            sum_wx += w * px as f32;
            sum_wy += w * py as f32;
        }
    }
    if sum_w <= 1e-6 {
        return (0.0, 0.0);
    }
    let cx = sum_wx / sum_w;
    let cy = sum_wy / sum_w;
    (
        ((cx - eye[0]) / half).clamp(-1.0, 1.0),
        ((cy - eye[1]) / half).clamp(-1.0, 1.0),
    )
}

/// Serializable calibration weights — same JSON-blob convention as
/// `MouseCnnWeights`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GazeCalibWeights {
    #[serde(rename = "w1")]
    pub w1: Vec<Vec<f32>>,
    #[serde(rename = "b1")]
    pub b1: Vec<f32>,
    #[serde(rename = "w2")]
    pub w2: Vec<Vec<f32>>,
    #[serde(rename = "b2")]
    pub b2: Vec<f32>,
    #[serde(rename = "trainedEpochs")]
    pub trained_epochs: usize,
    #[serde(rename = "trainingSamples")]
    pub training_samples: usize,
    #[serde(rename = "trainLoss")]
    pub train_loss: f32,
}

pub struct GazeCalibrator {
    device: Device,
    varmap: VarMap,
    fc1: Linear,
    fc2: Linear,
    trained_epochs: usize,
    training_samples: usize,
    train_loss: f32,
}

impl GazeCalibrator {
    pub fn new() -> Result<Self> {
        let device = Device::Cpu;
        let varmap = VarMap::new();
        let vb = VarBuilder::from_varmap(&varmap, DType::F32, &device);
        let fc1 = linear(FEATURE_DIM, HIDDEN, vb.pp("fc1"))?;
        let fc2 = linear(HIDDEN, OUTPUT_DIM, vb.pp("fc2"))?;
        Ok(Self {
            device,
            varmap,
            fc1,
            fc2,
            trained_epochs: 0,
            training_samples: 0,
            train_loss: 1.0,
        })
    }

    pub fn from_weights(w: &GazeCalibWeights) -> Result<Self> {
        let mut m = Self::new()?;
        load_linear(&mut m.fc1, &w.w1, &w.b1)?;
        load_linear(&mut m.fc2, &w.w2, &w.b2)?;
        m.trained_epochs = w.trained_epochs;
        m.training_samples = w.training_samples;
        m.train_loss = w.train_loss;
        Ok(m)
    }

    pub fn is_trained(&self) -> bool {
        self.trained_epochs > 0 && self.training_samples >= FEATURE_DIM
    }

    pub fn train_loss(&self) -> f32 {
        self.train_loss
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let h = self.fc1.forward(x)?.relu()?;
        Ok(ops::sigmoid(&self.fc2.forward(&h)?)?)
    }

    /// Fit the MLP to labeled calibration samples (features → screen
    /// target) with MSE loss. Returns final loss, or `-1.0` if there
    /// are too few samples to fit.
    pub fn train(&mut self, samples: &[GazeCalibSample], epochs: usize) -> Result<f32> {
        if samples.len() < FEATURE_DIM {
            return Ok(-1.0);
        }
        let n = samples.len();
        let mut feats: Vec<f32> = Vec::with_capacity(n * FEATURE_DIM);
        let mut targets: Vec<f32> = Vec::with_capacity(n * OUTPUT_DIM);
        for s in samples {
            feats.extend_from_slice(&[s.yaw, s.pitch, s.roll, s.iris_dx, s.iris_dy]);
            targets.extend_from_slice(&[s.target_x.clamp(0.0, 1.0), s.target_y.clamp(0.0, 1.0)]);
        }
        let x = Tensor::from_vec(feats, (n, FEATURE_DIM), &self.device)?;
        let y = Tensor::from_vec(targets, (n, OUTPUT_DIM), &self.device)?;

        let mut opt = candle_nn::optim::SGD::new(self.varmap.all_vars(), LEARNING_RATE)?;
        let mut last = 1.0_f32;
        for _ in 0..epochs {
            let pred = self.forward(&x)?;
            let diff = (&pred - &y)?;
            let loss = diff.sqr()?.mean_all()?;
            opt.backward_step(&loss)?;
            last = loss.to_scalar::<f32>()?;
        }
        self.trained_epochs += epochs;
        self.training_samples = n;
        self.train_loss = last;
        Ok(last)
    }

    pub fn export_weights(&self) -> Result<GazeCalibWeights> {
        Ok(GazeCalibWeights {
            w1: linear_weight(&self.fc1)?,
            b1: linear_bias(&self.fc1)?,
            w2: linear_weight(&self.fc2)?,
            b2: linear_bias(&self.fc2)?,
            trained_epochs: self.trained_epochs,
            training_samples: self.training_samples,
            train_loss: self.train_loss,
        })
    }

    /// Predict normalized screen point for a feature vector.
    fn predict(&self, f: &GazeFeatures) -> Result<(f32, f32)> {
        let x = Tensor::from_vec(f.to_array().to_vec(), (1, FEATURE_DIM), &self.device)?;
        let out = self.forward(&x)?.to_vec2::<f32>()?;
        Ok((out[0][0], out[0][1]))
    }
}

/// Produce a `GazeEstimate` for a frame. `calib` is the per-user model
/// if one is trained; pass `None` for the uncalibrated cone fallback.
/// Returns an `occluded` estimate when no usable face geometry exists.
pub fn estimate(
    frame: &FaceFrame,
    det: Option<&FaceDetection>,
    calib: Option<&GazeCalibrator>,
) -> Result<GazeEstimate> {
    let Some(det) = det else {
        return Ok(occluded_estimate());
    };
    let Some(features) = extract_features(frame, det) else {
        return Ok(occluded_estimate());
    };

    if let Some(c) = calib.filter(|c| c.is_trained()) {
        let (sx, sy) = c.predict(&features)?;
        let on_screen = (-SCREEN_MARGIN..=1.0 + SCREEN_MARGIN).contains(&sx)
            && (-SCREEN_MARGIN..=1.0 + SCREEN_MARGIN).contains(&sy);
        return Ok(GazeEstimate {
            yaw: features.yaw,
            pitch: features.pitch,
            screen_x: Some(sx),
            screen_y: Some(sy),
            on_screen,
            occluded: false,
            confidence: det.score,
        });
    }

    // Uncalibrated: generous cone on the raw proxies.
    let on_screen = features.yaw.abs() <= CONE_YAW && features.pitch.abs() <= CONE_PITCH;
    Ok(GazeEstimate {
        yaw: features.yaw,
        pitch: features.pitch,
        screen_x: None,
        screen_y: None,
        on_screen,
        occluded: false,
        confidence: det.score * 0.5, // lower confidence without calibration
    })
}

fn occluded_estimate() -> GazeEstimate {
    GazeEstimate {
        yaw: 0.0,
        pitch: 0.0,
        screen_x: None,
        screen_y: None,
        on_screen: false,
        occluded: true,
        confidence: 0.0,
    }
}

pub fn default_epochs() -> usize {
    DEFAULT_EPOCHS
}

fn linear_weight(layer: &Linear) -> Result<Vec<Vec<f32>>> {
    let w = layer.weight();
    if w.dims().len() != 2 {
        return Err(anyhow!("linear weight not 2D"));
    }
    Ok(w.to_vec2::<f32>()?)
}

fn linear_bias(layer: &Linear) -> Result<Vec<f32>> {
    layer
        .bias()
        .map(|b| b.to_vec1::<f32>().map_err(|e| anyhow!(e)))
        .unwrap_or_else(|| Ok(vec![]))
}

fn load_linear(layer: &mut Linear, w: &[Vec<f32>], b: &[f32]) -> Result<()> {
    let device = Device::Cpu;
    let rows = w.len();
    let cols = if rows == 0 { 0 } else { w[0].len() };
    let flat: Vec<f32> = w.iter().flatten().copied().collect();
    let weight = Tensor::from_vec(flat, (rows, cols), &device)?;
    let bias = Tensor::from_vec(b.to_vec(), (b.len(),), &device)?;
    *layer = Linear::new(weight, Some(bias));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det_with_landmarks(lms: [[f32; 2]; 5]) -> FaceDetection {
        FaceDetection {
            bbox: [0.0, 0.0, 100.0, 100.0],
            landmarks5: lms,
            score: 0.95,
        }
    }

    fn gray_frame(w: u32, h: u32, val: u8) -> FaceFrame {
        FaceFrame {
            width: w,
            height: h,
            rgba: vec![val; (w * h * 4) as usize],
        }
    }

    #[test]
    fn frontal_face_yields_near_zero_yaw() {
        // Symmetric frontal landmarks: nose centered between eyes.
        let lms = [
            [40.0, 50.0], // right eye
            [60.0, 50.0], // left eye
            [50.0, 65.0], // nose (centered)
            [42.0, 80.0], // right mouth
            [58.0, 80.0], // left mouth
        ];
        let f = extract_features(&gray_frame(100, 100, 128), &det_with_landmarks(lms)).unwrap();
        assert!(f.yaw.abs() < 0.1, "frontal yaw should be ~0, got {}", f.yaw);
    }

    #[test]
    fn head_turned_right_yields_positive_yaw() {
        // Nose shifted toward the left-eye side → nonzero yaw proxy.
        let lms = [
            [40.0, 50.0],
            [60.0, 50.0],
            [58.0, 65.0], // nose pushed toward left eye
            [42.0, 80.0],
            [58.0, 80.0],
        ];
        let f = extract_features(&gray_frame(100, 100, 128), &det_with_landmarks(lms)).unwrap();
        assert!(
            f.yaw > 0.2,
            "turned-head yaw should be large, got {}",
            f.yaw
        );
    }

    #[test]
    fn degenerate_eyes_returns_none() {
        let lms = [
            [50.0, 50.0],
            [50.0, 50.0], // coincident with right eye
            [50.0, 65.0],
            [42.0, 80.0],
            [58.0, 80.0],
        ];
        assert!(extract_features(&gray_frame(100, 100, 128), &det_with_landmarks(lms)).is_none());
    }

    #[test]
    fn untrained_calibrator_uses_cone_fallback() {
        let lms = [
            [40.0, 50.0],
            [60.0, 50.0],
            [50.0, 65.0],
            [42.0, 80.0],
            [58.0, 80.0],
        ];
        let det = det_with_landmarks(lms);
        let est = estimate(&gray_frame(100, 100, 128), Some(&det), None).unwrap();
        assert!(est.on_screen, "frontal face should read on-screen via cone");
        assert!(est.screen_x.is_none(), "no calibration → no screen point");
    }

    #[test]
    fn no_detection_is_occluded() {
        let est = estimate(&gray_frame(100, 100, 128), None, None).unwrap();
        assert!(est.occluded);
        assert!(!est.on_screen);
    }

    #[test]
    fn calibration_trains_and_roundtrips() {
        // Synthetic linear-ish mapping from yaw/pitch to screen.
        let mut samples = Vec::new();
        for i in 0..20 {
            let t = i as f32 / 19.0;
            samples.push(GazeCalibSample {
                yaw: (t - 0.5) * 0.6,
                pitch: (t - 0.5) * 0.4,
                roll: 0.0,
                iris_dx: 0.0,
                iris_dy: 0.0,
                target_x: t,
                target_y: t,
            });
        }
        let mut c = GazeCalibrator::new().unwrap();
        let loss = c.train(&samples, 200).unwrap();
        assert!(loss.is_finite() && loss >= 0.0);
        assert!(c.is_trained());

        let w = c.export_weights().unwrap();
        let c2 = GazeCalibrator::from_weights(&w).unwrap();
        let f = GazeFeatures {
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            iris_dx: 0.0,
            iris_dy: 0.0,
        };
        let a = c.predict(&f).unwrap();
        let b = c2.predict(&f).unwrap();
        assert!(
            (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4,
            "roundtrip drift"
        );
    }

    #[test]
    fn calibrator_params_are_f32_only() {
        let c = GazeCalibrator::new().unwrap();
        for v in c.varmap.all_vars() {
            assert_eq!(v.dtype(), DType::F32, "sentinel ML must stay F32-only");
        }
    }
}
