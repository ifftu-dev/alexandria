//! Per-profile camera budgets and the device benchmark that checks them.
//!
//! A trust profile that runs the camera (`camera`, `full`) is selectable
//! only on a device whose benchmark fits that profile's budget; `core` and
//! `attested` run no model inference and have no budget. The benchmark is
//! the fixed workload the spec names: a face-detection pass on a synthetic
//! frame at the reference size, a liveness pass, and one tracked tick,
//! each timed after a warm-up pass (so plan compilation, a one-off per
//! frame size, is not counted).
//!
//! Budgets are per pass, in milliseconds, for the reference low-end device
//! class: a phone or laptop whose single performance core is roughly a
//! tenth of a current Apple M-series core (Cortex-A55-class phones, 2018
//! dual-core laptops). They are set so that at the camera tick rate (1 Hz)
//! a worst-case tick (detection + liveness + gaze) stays well under the
//! tick, and the steady state (one detection per `detect_interval`,
//! tracking in between) keeps the inference thread under a quarter of one
//! core. The numbers measured on the development machine and the derivation
//! are in `docs/sentinel-trust-profiles/11-camera-performance.md`; on-device
//! confirmation belongs to the device runtime verification ticket.

use std::time::{Duration, Instant};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::camera_pipeline::{CameraPipeline, FaceModels, PipelineConfig};
use super::face_detect;
use super::liveness::{self, LivenessEstimate};
use super::scoring::BudgetFit;
use super::types::{FaceDetection, FaceFrame};

/// Frame size the webview sends (longest side 224) and the benchmark uses.
pub const REFERENCE_FRAME: (u32, u32) = (224, 168);

/// Timed passes per stage; the median is reported.
const PASSES: usize = 3;

/// One device's benchmark, per pass.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub detect_ms: f32,
    pub liveness_ms: f32,
    pub track_ms: f32,
}

/// The most a camera profile may spend per pass, and its detection cadence.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CameraBudget {
    pub detect_ms: f32,
    pub liveness_ms: f32,
    pub track_ms: f32,
    /// Longest gap between detector runs while a face is tracked.
    #[serde(serialize_with = "millis", deserialize_with = "from_millis")]
    pub detect_interval: Duration,
}

fn millis<S: serde::Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u64(d.as_millis() as u64)
}

fn from_millis<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
    u64::deserialize(d).map(Duration::from_millis)
}

/// Budget for the `camera` profile.
pub const CAMERA_PROFILE_BUDGET: CameraBudget = CameraBudget {
    detect_ms: 250.0,
    liveness_ms: 60.0,
    track_ms: 15.0,
    detect_interval: Duration::from_secs(3),
};

/// Budget for the `full` profile: the same camera work alongside every
/// other signal, so it keeps more headroom.
pub const FULL_PROFILE_BUDGET: CameraBudget = CameraBudget {
    detect_ms: 150.0,
    liveness_ms: 40.0,
    track_ms: 10.0,
    detect_interval: Duration::from_secs(3),
};

impl CameraBudget {
    /// Whether a device that benchmarked `result` can sustain this budget.
    /// Every stage must fit; a fast detector does not buy a slow tracker.
    pub fn admits(&self, result: &BenchmarkResult) -> bool {
        let fits = |got: f32, max: f32| got.is_finite() && got <= max;
        fits(result.detect_ms, self.detect_ms)
            && fits(result.liveness_ms, self.liveness_ms)
            && fits(result.track_ms, self.track_ms)
    }

    /// Pipeline cadence for this budget.
    pub fn pipeline_config(&self) -> PipelineConfig {
        PipelineConfig {
            detect_interval: self.detect_interval,
            ..PipelineConfig::default()
        }
    }
}

impl BenchmarkResult {
    /// Which camera profiles this device can sustain.
    pub fn fit(&self) -> BudgetFit {
        BudgetFit {
            camera: CAMERA_PROFILE_BUDGET.admits(self),
            full: FULL_PROFILE_BUDGET.admits(self),
        }
    }
}

/// Deterministic textured frame shifted right by `dx` pixels: no face, but
/// enough structure that the tracker's patch search does real work.
fn synthetic_frame(dx: u32) -> FaceFrame {
    let (w, h) = REFERENCE_FRAME;
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let sx = x.saturating_sub(dx);
            let v = (sx.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)) >> 7;
            rgba.extend_from_slice(&[v as u8, (v >> 8) as u8, (v >> 16) as u8, 255]);
        }
    }
    FaceFrame {
        width: w,
        height: h,
        rgba,
    }
}

/// A face in the middle of the reference frame, for the bench passes that
/// need one.
fn synthetic_face() -> FaceDetection {
    let (x, y) = (82.0, 45.0);
    FaceDetection {
        bbox: [x, y, 60.0, 75.0],
        landmarks5: [
            [x + 18.0, y + 28.0],
            [x + 42.0, y + 28.0],
            [x + 30.0, y + 44.0],
            [x + 20.0, y + 58.0],
            [x + 40.0, y + 58.0],
        ],
        score: 0.9,
    }
}

/// Opens a track on the synthetic face without running a model, so the
/// tracking pass is timed on its own.
struct ScriptedFace;

impl FaceModels for ScriptedFace {
    fn detect(&mut self, _frame: &FaceFrame) -> Result<Vec<FaceDetection>> {
        Ok(vec![synthetic_face()])
    }
    fn liveness(&mut self, _frame: &FaceFrame, _bbox: [f32; 4]) -> Result<LivenessEstimate> {
        Ok(LivenessEstimate {
            real_prob: 1.0,
            spoof_suspected: false,
        })
    }
}

fn median_ms(mut pass: impl FnMut() -> Result<()>) -> Result<f32> {
    pass()?; // warm-up
    let mut samples = Vec::with_capacity(PASSES);
    for _ in 0..PASSES {
        let t = Instant::now();
        pass()?;
        samples.push(t.elapsed().as_secs_f32() * 1000.0);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(samples[PASSES / 2])
}

/// Run the fixed benchmark workload on this device.
pub fn benchmark() -> Result<BenchmarkResult> {
    let frame = synthetic_frame(0);
    let moved = synthetic_frame(2);
    let face = synthetic_face();
    let detect_ms = median_ms(|| face_detect::detect(&frame).map(|_| ()))?;
    let liveness_ms = median_ms(|| liveness::score(&frame, face.bbox).map(|_| ()))?;
    let mut pipeline = CameraPipeline::new(PipelineConfig {
        liveness_burst: 0,
        ..PipelineConfig::default()
    });
    // Open a track, then time tracked ticks on a frame that moves two
    // pixels back and forth, as a head drifts between ticks.
    let start = Instant::now();
    pipeline.tick(&frame, start, &mut ScriptedFace)?;
    let mut flip = false;
    let track_ms = median_ms(|| {
        flip = !flip;
        let next = if flip { &moved } else { &frame };
        pipeline.tick(next, start, &mut ScriptedFace).map(|_| ())
    })?;
    Ok(BenchmarkResult {
        detect_ms,
        liveness_ms,
        track_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAST: BenchmarkResult = BenchmarkResult {
        detect_ms: 12.0,
        liveness_ms: 3.0,
        track_ms: 0.5,
    };

    #[test]
    fn a_device_within_every_stage_is_admitted() {
        assert!(CAMERA_PROFILE_BUDGET.admits(&FAST));
        assert!(FULL_PROFILE_BUDGET.admits(&FAST));
    }

    #[test]
    fn one_stage_over_budget_excludes_the_profile() {
        let slow_detector = BenchmarkResult {
            detect_ms: 200.0,
            ..FAST
        };
        assert!(CAMERA_PROFILE_BUDGET.admits(&slow_detector));
        assert!(!FULL_PROFILE_BUDGET.admits(&slow_detector));
        let slow_tracker = BenchmarkResult {
            track_ms: 40.0,
            ..FAST
        };
        assert!(!CAMERA_PROFILE_BUDGET.admits(&slow_tracker));
        let broken = BenchmarkResult {
            liveness_ms: f32::NAN,
            ..FAST
        };
        assert!(!CAMERA_PROFILE_BUDGET.admits(&broken));
    }

    #[test]
    fn the_benchmark_times_every_stage_on_the_bundled_models() {
        let result = benchmark().unwrap();
        for ms in [result.detect_ms, result.liveness_ms, result.track_ms] {
            assert!(ms.is_finite() && ms > 0.0, "stage time {ms}");
        }
    }
}
