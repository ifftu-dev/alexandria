//! Camera tick scheduling: slow-cadence detection, landmark tracking
//! between detections, and liveness on trigger.
//!
//! The old path ran YuNet, gaze and MiniFASNet on every camera tick. Here
//! a tick runs the detector only when the cadence is due or the track is
//! lost; in between, the five YuNet landmarks are followed by patch
//! matching on the grey frame (a few hundred microseconds), and the gaze
//! proxies are computed from the tracked landmarks. Liveness runs as a
//! short burst when a trigger fires — a new face track, a detection
//! confidence drop, a tracking-quality drop, or a slow refresh — so a
//! snapshot window still gets the several reads the advisory spoof rule
//! needs, without paying for the model every second.
//!
//! Between detections the face count is the last detection's count, so a
//! second face entering the frame is seen at the next detection (at most
//! one `detect_interval` later), and a tracked tick never reports more
//! certainty than the detection it follows: its score is the detection
//! score scaled by tracking quality.
//!
//! `submit` hands a frame to the process-wide inference thread
//! (`inference_worker`), one pipeline per lane, so the IPC handler never
//! runs a model itself and a frame that waits behind a slow one is
//! dropped rather than queued.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::Serialize;
use tokio::sync::oneshot;

use super::camera_budget::CAMERA_PROFILE_BUDGET;
use super::face_detect;
use super::gaze::{self, GazeCalibWeights, GazeCalibrator};
use super::inference_worker::InferenceWorker;
pub use super::inference_worker::{Lane, Outcome};
use super::liveness::{self, LivenessEstimate};
use super::types::{FaceDetection, FaceFrame, GazeEstimate, LM_LEFT_EYE, LM_RIGHT_EYE};

/// The two models a tick may run. A seam so the scheduling can be tested
/// with scripted detections; production uses [`BundledModels`].
pub trait FaceModels {
    fn detect(&mut self, frame: &FaceFrame) -> Result<Vec<FaceDetection>>;
    fn liveness(&mut self, frame: &FaceFrame, bbox: [f32; 4]) -> Result<LivenessEstimate>;
}

/// The bundled YuNet + MiniFASNet models.
pub struct BundledModels;

impl FaceModels for BundledModels {
    fn detect(&mut self, frame: &FaceFrame) -> Result<Vec<FaceDetection>> {
        face_detect::detect(frame)
    }
    fn liveness(&mut self, frame: &FaceFrame, bbox: [f32; 4]) -> Result<LivenessEstimate> {
        liveness::score(frame, bbox)
    }
}

/// Cadence and trigger thresholds for one pipeline.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Longest gap between detector runs while a face is tracked.
    pub detect_interval: Duration,
    /// A track older than this since its last frame is discarded.
    pub stale_after: Duration,
    /// Liveness reads per trigger.
    pub liveness_burst: u8,
    /// A steady track still re-runs liveness this often.
    pub liveness_refresh: Duration,
    /// Below this tracking quality the tick falls back to detection.
    pub min_track_quality: f32,
    /// A detection this much below the track's opening score is a
    /// confidence drop.
    pub score_drop: f32,
    /// A detection overlapping the track less than this is a new face.
    pub new_track_iou: f32,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        PipelineConfig {
            detect_interval: CAMERA_PROFILE_BUDGET.detect_interval,
            stale_after: Duration::from_secs(5),
            liveness_burst: 3,
            liveness_refresh: Duration::from_secs(30),
            min_track_quality: 0.5,
            score_drop: 0.15,
            new_track_iou: 0.3,
        }
    }
}

/// How a tick produced its face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickSource {
    Detected,
    Tracked,
}

/// One tick's face read, before gaze.
#[derive(Debug, Clone)]
pub struct PipelineTick {
    pub source: TickSource,
    pub face_count: usize,
    pub detection: Option<FaceDetection>,
    pub liveness: Option<LivenessEstimate>,
}

/// Single-channel luminance frame the tracker matches patches on.
struct GrayFrame {
    width: u32,
    height: u32,
    px: Vec<u8>,
}

impl GrayFrame {
    fn from_rgba(frame: &FaceFrame) -> Self {
        let px = frame
            .rgba
            .chunks_exact(4)
            .map(|p| ((p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8) as u8)
            .collect();
        GrayFrame {
            width: frame.width,
            height: frame.height,
            px,
        }
    }

    fn at(&self, x: i32, y: i32) -> i32 {
        let x = x.clamp(0, self.width as i32 - 1) as u32;
        let y = y.clamp(0, self.height as i32 - 1) as u32;
        self.px[(y * self.width + x) as usize] as i32
    }
}

/// Mean absolute grey difference at which tracking quality reaches zero.
const MAD_SCALE: f32 = 24.0;

/// Follow one landmark from `prev` to `cur` by exhaustive patch matching
/// (sum of absolute differences). Returns the moved point and the mean
/// absolute difference of the best match.
fn track_point(
    prev: &GrayFrame,
    cur: &GrayFrame,
    p: [f32; 2],
    patch: i32,
    search: i32,
) -> ([f32; 2], f32) {
    let (cx, cy) = (p[0].round() as i32, p[1].round() as i32);
    let mut best = (0, 0, i32::MAX);
    for oy in -search..=search {
        for ox in -search..=search {
            let mut sad = 0;
            'patch: for v in -patch..=patch {
                for u in -patch..=patch {
                    sad += (prev.at(cx + u, cy + v) - cur.at(cx + u + ox, cy + v + oy)).abs();
                    if sad >= best.2 {
                        break 'patch;
                    }
                }
            }
            // Prefer the smallest move on ties so a flat patch stays put.
            if sad < best.2
                || (sad == best.2 && ox * ox + oy * oy < best.0 * best.0 + best.1 * best.1)
            {
                best = (ox, oy, sad);
            }
        }
    }
    let n = ((2 * patch + 1) * (2 * patch + 1)) as f32;
    (
        [p[0] + best.0 as f32, p[1] + best.1 as f32],
        best.2 as f32 / n,
    )
}

fn interocular(det: &FaceDetection) -> f32 {
    let r = det.landmarks5[LM_RIGHT_EYE];
    let l = det.landmarks5[LM_LEFT_EYE];
    ((l[0] - r[0]).powi(2) + (l[1] - r[1]).powi(2)).sqrt()
}

/// Track `det` from `prev` into `cur`. `None` when tracking quality or the
/// face geometry says the track is no longer trustworthy.
fn track_face(
    prev: &GrayFrame,
    cur: &GrayFrame,
    det: &FaceDetection,
    opening_score: f32,
    min_quality: f32,
) -> Option<FaceDetection> {
    let io = interocular(det);
    let patch = (io * 0.2).clamp(3.0, 7.0) as i32;
    let search = (io * 0.4).clamp(4.0, 12.0) as i32;
    let mut moved = det.clone();
    let (mut mad_sum, mut dx_sum, mut dy_sum) = (0.0, 0.0, 0.0);
    for (k, lm) in det.landmarks5.iter().enumerate() {
        let (to, mad) = track_point(prev, cur, *lm, patch, search);
        mad_sum += mad;
        dx_sum += to[0] - lm[0];
        dy_sum += to[1] - lm[1];
        moved.landmarks5[k] = to;
    }
    let quality = 1.0 - (mad_sum / 5.0 / MAD_SCALE).min(1.0);
    let scale = interocular(&moved) / io.max(1e-3);
    if quality < min_quality || !(0.75..=1.33).contains(&scale) {
        return None;
    }
    moved.bbox[0] += dx_sum / 5.0;
    moved.bbox[1] += dy_sum / 5.0;
    moved.score = opening_score * quality;
    Some(moved)
}

/// Highest-score detection in the set.
pub fn best_detection(dets: &[FaceDetection]) -> Option<&FaceDetection> {
    dets.iter().max_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

struct Track {
    det: FaceDetection,
    /// Detector score when this track opened (or was last confirmed).
    opening_score: f32,
    face_count: usize,
    gray: GrayFrame,
    last_detect: Instant,
    last_frame: Instant,
    last_liveness: Instant,
    liveness_left: u8,
}

/// Per-lane scheduling state. One instance per frame stream.
pub struct CameraPipeline {
    config: PipelineConfig,
    track: Option<Track>,
}

impl CameraPipeline {
    pub fn new(config: PipelineConfig) -> Self {
        CameraPipeline {
            config,
            track: None,
        }
    }

    /// Process one frame captured at `now`.
    pub fn tick(
        &mut self,
        frame: &FaceFrame,
        now: Instant,
        models: &mut impl FaceModels,
    ) -> Result<PipelineTick> {
        let gray = GrayFrame::from_rgba(frame);
        let stale = self.track.as_ref().is_some_and(|t| {
            t.gray.width != gray.width
                || t.gray.height != gray.height
                || now.saturating_duration_since(t.last_frame) > self.config.stale_after
        });
        if stale {
            self.track = None;
        }

        let mut tracking_lost = false;
        if let Some(track) = self.track.as_mut() {
            if now.saturating_duration_since(track.last_detect) < self.config.detect_interval {
                match track_face(
                    &track.gray,
                    &gray,
                    &track.det,
                    track.opening_score,
                    self.config.min_track_quality,
                ) {
                    Some(moved) => {
                        track.det = moved;
                        track.gray = gray;
                        track.last_frame = now;
                        let liveness = if track.liveness_left > 0 {
                            track.liveness_left -= 1;
                            Some(models.liveness(frame, track.det.bbox)?)
                        } else {
                            None
                        };
                        return Ok(PipelineTick {
                            source: TickSource::Tracked,
                            face_count: track.face_count,
                            detection: Some(track.det.clone()),
                            liveness,
                        });
                    }
                    None => tracking_lost = true,
                }
            }
        }

        let dets = models.detect(frame)?;
        let Some(best) = best_detection(&dets).cloned() else {
            self.track = None;
            return Ok(PipelineTick {
                source: TickSource::Detected,
                face_count: 0,
                detection: None,
                liveness: None,
            });
        };

        let previous = self.track.take();
        let (mut liveness_left, mut last_liveness) = previous
            .as_ref()
            .map_or((0, now), |t| (t.liveness_left, t.last_liveness));
        let triggered = match &previous {
            None => true,
            Some(t) => {
                tracking_lost
                    || face_detect::iou(&t.det.bbox, &best.bbox) < self.config.new_track_iou
                    || best.score < t.opening_score - self.config.score_drop
                    || now.saturating_duration_since(t.last_liveness)
                        >= self.config.liveness_refresh
            }
        };
        if triggered {
            liveness_left = self.config.liveness_burst;
            last_liveness = now;
        }
        let liveness = if liveness_left > 0 {
            liveness_left -= 1;
            Some(models.liveness(frame, best.bbox)?)
        } else {
            None
        };
        self.track = Some(Track {
            det: best.clone(),
            opening_score: best.score,
            face_count: dets.len(),
            gray,
            last_detect: now,
            last_frame: now,
            last_liveness,
            liveness_left,
        });
        Ok(PipelineTick {
            source: TickSource::Detected,
            face_count: dets.len(),
            detection: Some(best),
            liveness,
        })
    }
}

/// A frame queued for the inference thread.
pub struct CameraJob {
    pub frame: FaceFrame,
    /// The learner's gaze calibration, when trained.
    pub calib: Option<GazeCalibWeights>,
    /// Capture time; cadence and staleness are measured against it.
    pub at: Instant,
}

/// What one camera tick reports back over IPC.
#[derive(Debug, Clone, Serialize)]
pub struct GazeTick {
    pub estimate: GazeEstimate,
    #[serde(rename = "faceCount")]
    pub face_count: usize,
    /// Best face (detected or tracked) for overlay.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detection: Option<FaceDetection>,
    /// Presentation-attack read, present only on ticks a trigger ran it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liveness: Option<LivenessEstimate>,
    /// `true` when the face came from tracking rather than the detector.
    pub tracked: bool,
}

fn process(pipeline: &mut CameraPipeline, job: CameraJob) -> Result<GazeTick, String> {
    let tick = pipeline
        .tick(&job.frame, job.at, &mut BundledModels)
        .map_err(|e| e.to_string())?;
    let calib = job
        .calib
        .as_ref()
        .map(GazeCalibrator::from_weights)
        .transpose()
        .map_err(|e| e.to_string())?;
    let estimate = gaze::estimate(&job.frame, tick.detection.as_ref(), calib.as_ref())
        .map_err(|e| e.to_string())?;
    Ok(GazeTick {
        estimate,
        face_count: tick.face_count,
        detection: tick.detection,
        liveness: tick.liveness,
        tracked: tick.source == TickSource::Tracked,
    })
}

type CameraWorker = InferenceWorker<CameraJob, Result<GazeTick, String>>;

static WORKER: OnceLock<CameraWorker> = OnceLock::new();

/// Queue a frame on the camera inference thread. Resolves `Dropped` when a
/// newer frame in the same lane replaces it before inference reaches it.
pub fn submit(lane: Lane, job: CameraJob) -> oneshot::Receiver<Outcome<Result<GazeTick, String>>> {
    WORKER
        .get_or_init(|| {
            let mut lanes: HashMap<Lane, CameraPipeline> = HashMap::new();
            InferenceWorker::spawn("sentinel-camera", move |lane, job| {
                let pipeline = lanes.entry(lane).or_insert_with(|| {
                    CameraPipeline::new(CAMERA_PROFILE_BUDGET.pipeline_config())
                });
                process(pipeline, job)
            })
        })
        .submit(lane, job)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sentinel::liveness::LivenessEstimate;
    use crate::sentinel::types::{FaceDetection, FaceFrame};
    use std::time::{Duration, Instant};

    const W: u32 = 224;
    const H: u32 = 168;

    /// Deterministic value noise so patches are distinctive, shifted by
    /// `(dx, dy)` to simulate a head moving between ticks.
    fn noise_frame(seed: u32, dx: i32, dy: i32) -> FaceFrame {
        let mut rgba = Vec::with_capacity((W * H * 4) as usize);
        for y in 0..H as i32 {
            for x in 0..W as i32 {
                let (sx, sy) = ((x - dx) as u32, (y - dy) as u32);
                let mut v = sx.wrapping_mul(374_761_393) ^ sy.wrapping_mul(668_265_263) ^ seed;
                v = (v ^ (v >> 13)).wrapping_mul(1_274_126_177);
                let g = (v >> 24) as u8;
                rgba.extend_from_slice(&[g, g, g, 255]);
            }
        }
        FaceFrame {
            width: W,
            height: H,
            rgba,
        }
    }

    fn face_at(x: f32, y: f32, score: f32) -> FaceDetection {
        FaceDetection {
            bbox: [x, y, 60.0, 70.0],
            landmarks5: [
                [x + 18.0, y + 25.0],
                [x + 42.0, y + 25.0],
                [x + 30.0, y + 40.0],
                [x + 20.0, y + 55.0],
                [x + 40.0, y + 55.0],
            ],
            score,
        }
    }

    /// Scripted detector + counting liveness model.
    struct Fake {
        faces: Vec<FaceDetection>,
        detect_calls: usize,
        liveness_calls: usize,
    }

    impl Fake {
        fn with(faces: Vec<FaceDetection>) -> Self {
            Fake {
                faces,
                detect_calls: 0,
                liveness_calls: 0,
            }
        }
    }

    impl FaceModels for Fake {
        fn detect(&mut self, _frame: &FaceFrame) -> anyhow::Result<Vec<FaceDetection>> {
            self.detect_calls += 1;
            Ok(self.faces.clone())
        }
        fn liveness(
            &mut self,
            _frame: &FaceFrame,
            _bbox: [f32; 4],
        ) -> anyhow::Result<LivenessEstimate> {
            self.liveness_calls += 1;
            Ok(LivenessEstimate {
                real_prob: 0.9,
                spoof_suspected: false,
            })
        }
    }

    fn secs(s: f32) -> Duration {
        Duration::from_secs_f32(s)
    }

    #[test]
    fn the_first_tick_detects_and_the_next_one_tracks_the_moved_face() {
        let mut models = Fake::with(vec![face_at(80.0, 40.0, 0.9)]);
        let mut pipeline = CameraPipeline::new(PipelineConfig::default());
        let t0 = Instant::now();

        let first = pipeline
            .tick(&noise_frame(1, 0, 0), t0, &mut models)
            .unwrap();
        assert_eq!(first.source, TickSource::Detected);
        assert_eq!(first.face_count, 1);

        let second = pipeline
            .tick(&noise_frame(1, 3, 2), t0 + secs(1.0), &mut models)
            .unwrap();
        assert_eq!(second.source, TickSource::Tracked);
        assert_eq!(
            models.detect_calls, 1,
            "tracked tick must not run the detector"
        );
        assert_eq!(
            second.face_count, 1,
            "face count carries across tracked ticks"
        );
        let det = second.detection.expect("tracked face");
        let want = face_at(83.0, 42.0, 0.9);
        for (got, want) in det.landmarks5.iter().zip(want.landmarks5) {
            assert!(
                (got[0] - want[0]).abs() <= 0.5 && (got[1] - want[1]).abs() <= 0.5,
                "landmark {got:?} should follow the face to {want:?}"
            );
        }
        assert!((det.bbox[0] - 83.0).abs() <= 0.5 && (det.bbox[1] - 42.0).abs() <= 0.5);
    }

    #[test]
    fn the_detector_runs_again_once_the_cadence_is_due() {
        let mut models = Fake::with(vec![face_at(80.0, 40.0, 0.9)]);
        let config = PipelineConfig::default();
        let interval = config.detect_interval;
        let mut pipeline = CameraPipeline::new(config);
        let t0 = Instant::now();
        let frame = noise_frame(1, 0, 0);
        pipeline.tick(&frame, t0, &mut models).unwrap();
        let mut t = t0;
        while t + secs(1.0) < t0 + interval {
            t += secs(1.0);
            let tick = pipeline.tick(&frame, t, &mut models).unwrap();
            assert_eq!(tick.source, TickSource::Tracked);
        }
        let due = pipeline.tick(&frame, t0 + interval, &mut models).unwrap();
        assert_eq!(due.source, TickSource::Detected);
        assert_eq!(models.detect_calls, 2);
    }

    #[test]
    fn a_frame_the_track_cannot_follow_is_detected_on_the_same_tick() {
        let mut models = Fake::with(vec![face_at(80.0, 40.0, 0.9)]);
        let mut pipeline = CameraPipeline::new(PipelineConfig::default());
        let t0 = Instant::now();
        pipeline
            .tick(&noise_frame(1, 0, 0), t0, &mut models)
            .unwrap();
        // A different scene (a swapped photo, a hand over the lens).
        let tick = pipeline
            .tick(&noise_frame(99, 0, 0), t0 + secs(1.0), &mut models)
            .unwrap();
        assert_eq!(tick.source, TickSource::Detected);
        assert_eq!(models.detect_calls, 2);
    }

    #[test]
    fn with_no_face_every_tick_detects_and_reports_none() {
        let mut models = Fake::with(vec![]);
        let mut pipeline = CameraPipeline::new(PipelineConfig::default());
        let t0 = Instant::now();
        for i in 0..3 {
            let tick = pipeline
                .tick(&noise_frame(1, 0, 0), t0 + secs(i as f32), &mut models)
                .unwrap();
            assert_eq!(tick.source, TickSource::Detected);
            assert_eq!(tick.face_count, 0);
            assert!(tick.detection.is_none() && tick.liveness.is_none());
        }
        assert_eq!(models.detect_calls, 3);
        assert_eq!(models.liveness_calls, 0);
    }

    #[test]
    fn a_new_track_runs_one_liveness_burst_and_a_steady_track_runs_none() {
        let mut models = Fake::with(vec![face_at(80.0, 40.0, 0.9)]);
        let config = PipelineConfig::default();
        let burst = config.liveness_burst as usize;
        let mut pipeline = CameraPipeline::new(config);
        let t0 = Instant::now();
        let frame = noise_frame(1, 0, 0);
        let reads: Vec<bool> = (0..8)
            .map(|i| {
                pipeline
                    .tick(&frame, t0 + secs(i as f32), &mut models)
                    .unwrap()
                    .liveness
                    .is_some()
            })
            .collect();
        assert!(
            models.detect_calls >= 2,
            "cadence re-detected the same face"
        );
        assert_eq!(
            reads,
            (0..8).map(|i| i < burst).collect::<Vec<_>>(),
            "burst on the new track, nothing while the same face stays"
        );
    }

    #[test]
    fn a_different_face_or_a_confidence_drop_triggers_liveness() {
        let t0 = Instant::now();
        let frame = noise_frame(1, 0, 0);
        let interval = PipelineConfig::default().detect_interval;
        for replacement in [face_at(150.0, 80.0, 0.9), face_at(80.0, 40.0, 0.7)] {
            let mut models = Fake::with(vec![face_at(80.0, 40.0, 0.9)]);
            let mut pipeline = CameraPipeline::new(PipelineConfig {
                liveness_burst: 1,
                ..PipelineConfig::default()
            });
            pipeline.tick(&frame, t0, &mut models).unwrap();
            models.faces = vec![replacement];
            let tick = pipeline.tick(&frame, t0 + interval, &mut models).unwrap();
            assert_eq!(tick.source, TickSource::Detected);
            assert!(tick.liveness.is_some(), "trigger must run liveness");
            assert_eq!(models.liveness_calls, 2);
        }
    }

    #[test]
    fn a_track_with_no_frames_for_a_while_starts_over() {
        let mut models = Fake::with(vec![face_at(80.0, 40.0, 0.9)]);
        let config = PipelineConfig {
            liveness_burst: 1,
            ..PipelineConfig::default()
        };
        let gap = config.stale_after + secs(0.5);
        let mut pipeline = CameraPipeline::new(config);
        let t0 = Instant::now();
        pipeline
            .tick(&noise_frame(1, 0, 0), t0, &mut models)
            .unwrap();
        let resumed = pipeline
            .tick(&noise_frame(1, 0, 0), t0 + gap, &mut models)
            .unwrap();
        assert_eq!(resumed.source, TickSource::Detected);
        assert!(
            resumed.liveness.is_some(),
            "a resumed stream is a new track"
        );
    }

    #[test]
    fn the_bundled_models_run_through_the_pipeline() {
        let frame = FaceFrame {
            width: W,
            height: H,
            rgba: vec![0; (W * H * 4) as usize],
        };
        let mut pipeline = CameraPipeline::new(PipelineConfig::default());
        let tick = pipeline
            .tick(&frame, Instant::now(), &mut BundledModels)
            .unwrap();
        assert_eq!((tick.source, tick.face_count), (TickSource::Detected, 0));
    }
}
