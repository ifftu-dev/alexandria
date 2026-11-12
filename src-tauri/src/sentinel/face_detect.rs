//! YuNet face detector — backend inference via tract.
//!
//! Mirrors the `paste_classifier` pattern: the bundled `yunet.onnx`
//! (OpenCV Zoo, MIT-licensed, `face_detection_yunet_2023mar`) is
//! embedded at compile time via `include_bytes!` and parsed once into a
//! process-wide `OnceLock`. Pure-Rust tract — no WASM, no native ONNX
//! runtime — so it compiles for every Tauri target including iOS/Android.
//!
//! The bundled graph is the zoo export with its baked 640×640 input,
//! output and intermediate shapes removed (weights untouched; recipe in
//! `scripts/sentinel/export-yunet-dynamic.py`). Every op is already
//! shape-agnostic, so the detector runs at the **frame's own resolution**:
//! the frame is copied into the top-left of a canvas padded up to a
//! multiple of 32 and decoded boxes are already in frame pixels. A 224×168
//! webview frame runs as 224×192, about a tenth of the 640² letterbox the
//! fixed export forced. One optimized plan is compiled per padded shape
//! and cached, so only the first frame of a new size pays for it.
//!
//! Output contract (verified in tests): 12 tensors — `cls×3, obj×3,
//! bbox×3, kps×3`, each triplet ordered by stride `[8, 16, 32]`, with
//! `(H/stride)·(W/stride)` anchors per stride. A model swap that changes
//! this shape fails loudly in `model_contract_holds_at_the_frame_size`.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{anyhow, Context, Result};
use tract_onnx::prelude::*;

use super::types::{FaceDetection, FaceFrame};

/// Bundled YuNet weights. Always available; only fails to load on a
/// catastrophic tract-parser regression (release-blocking, asserted in
/// `parsed_model`).
const BUNDLED_YUNET: &[u8] = include_bytes!("../../resources/sentinel/yunet.onnx");

/// Input sides are padded up to a multiple of the coarsest stride.
pub const ALIGN: usize = 32;

/// Anchor-free strides. Output grids are `side / stride`.
const STRIDES: [usize; 3] = [8, 16, 32];

/// Minimum `cls * obj` probability for a detection to survive decode.
const SCORE_THRESHOLD: f32 = 0.70;

/// IoU above which a lower-scoring box is suppressed in NMS.
const NMS_IOU_THRESHOLD: f32 = 0.30;

/// Hard cap on candidate boxes fed into NMS — defends against a
/// degenerate frame that clears threshold everywhere.
const MAX_CANDIDATES: usize = 1024;

/// Compiled plans kept at once. A session uses one frame size; the cap
/// only bounds a caller that keeps changing it.
const MAX_PLANS: usize = 4;

type Runnable = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

static PARSED: OnceLock<InferenceModel> = OnceLock::new();
type PlanCache = Mutex<HashMap<(usize, usize), Arc<Runnable>>>;

static PLANS: OnceLock<PlanCache> = OnceLock::new();

fn parsed_model() -> &'static InferenceModel {
    PARSED.get_or_init(|| {
        tract_onnx::onnx()
            .model_for_read(&mut Cursor::new(BUNDLED_YUNET))
            .expect("bundled yunet.onnx failed to parse — release-blocking")
    })
}

/// Padded `(height, width)` the detector runs at for a `w`×`h` frame.
pub fn padded_shape(w: u32, h: u32) -> (usize, usize) {
    let up = |v: u32| (v as usize).div_ceil(ALIGN).max(1) * ALIGN;
    (up(h), up(w))
}

fn build_runnable(height: usize, width: usize) -> Result<Arc<Runnable>> {
    let model = parsed_model()
        .clone()
        .with_input_fact(0, f32::fact([1, 3, height, width]).into())
        .with_context(|| format!("bind YuNet input fact [1,3,{height},{width}]"))?
        .into_optimized()
        .context("optimize YuNet graph")?
        .into_runnable()
        .context("compile YuNet runnable")?;
    Ok(Arc::new(model))
}

fn runnable_for(height: usize, width: usize) -> Result<Arc<Runnable>> {
    let plans = PLANS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(plan) = plans
        .lock()
        .map_err(|_| anyhow!("detector plan cache poisoned"))?
        .get(&(height, width))
    {
        return Ok(plan.clone());
    }
    // Compile outside the lock; a racing duplicate compile is harmless.
    let plan = build_runnable(height, width)?;
    let mut cache = plans
        .lock()
        .map_err(|_| anyhow!("detector plan cache poisoned"))?;
    if cache.len() >= MAX_PLANS {
        cache.clear();
    }
    cache.insert((height, width), plan.clone());
    Ok(plan)
}

/// Compile the plan for a `w`×`h` frame ahead of the first tick (the
/// device benchmark and session start call this so the first camera tick
/// does not pay for graph optimization).
pub fn warm(w: u32, h: u32) -> Result<()> {
    let (height, width) = padded_shape(w, h);
    runnable_for(height, width).map(|_| ())
}

/// Build the NCHW BGR f32 input tensor: the frame at its own resolution
/// in the top-left of a `height`×`width` canvas, zero (black) padding.
/// YuNet was trained on raw BGR 0-255 (OpenCV convention) with no mean
/// subtraction.
fn preprocess(frame: &FaceFrame, height: usize, width: usize) -> Result<Tensor> {
    let fw = frame.width as usize;
    let fh = frame.height as usize;
    if frame.rgba.len() < fw * fh * 4 {
        return Err(anyhow!(
            "frame buffer too small: {} bytes for {}x{} RGBA",
            frame.rgba.len(),
            frame.width,
            frame.height
        ));
    }
    let n = height * width;
    let mut data = vec![0.0_f32; 3 * n];
    for (y, row) in frame.rgba.chunks_exact(fw * 4).take(fh).enumerate() {
        for (x, px) in row.chunks_exact(4).enumerate() {
            let dst = y * width + x;
            data[dst] = px[2] as f32; // B plane
            data[n + dst] = px[1] as f32; // G plane
            data[2 * n + dst] = px[0] as f32; // R plane
        }
    }
    Tensor::from_shape(&[1, 3, height, width], &data)
}

/// Detect faces in `frame`. Returns bounding boxes + 5 landmarks +
/// score in **original frame pixel coordinates**, NMS-deduplicated and
/// sorted by descending score. Never panics; a tract failure surfaces
/// as `Err`.
pub fn detect(frame: &FaceFrame) -> Result<Vec<FaceDetection>> {
    if frame.width == 0 || frame.height == 0 {
        return Ok(vec![]);
    }
    let (height, width) = padded_shape(frame.width, frame.height);
    let input = preprocess(frame, height, width)?;
    let outputs = runnable_for(height, width)?
        .run(tvec!(input.into()))
        .context("run YuNet inference")?;
    if outputs.len() != 12 {
        return Err(anyhow!(
            "YuNet output contract violated: expected 12 tensors, got {}",
            outputs.len()
        ));
    }

    let mut candidates: Vec<FaceDetection> = Vec::new();
    for (si, &stride) in STRIDES.iter().enumerate() {
        let (grid_w, grid_h) = (width / stride, height / stride);
        let cls = outputs[si].to_array_view::<f32>()?;
        let obj = outputs[3 + si].to_array_view::<f32>()?;
        let bbox = outputs[6 + si].to_array_view::<f32>()?;
        let kps = outputs[9 + si].to_array_view::<f32>()?;
        let cls = cls
            .as_slice()
            .ok_or_else(|| anyhow!("cls not contiguous"))?;
        let obj = obj
            .as_slice()
            .ok_or_else(|| anyhow!("obj not contiguous"))?;
        let bbox = bbox
            .as_slice()
            .ok_or_else(|| anyhow!("bbox not contiguous"))?;
        let kps = kps
            .as_slice()
            .ok_or_else(|| anyhow!("kps not contiguous"))?;
        if cls.len() != grid_w * grid_h {
            return Err(anyhow!(
                "YuNet stride-{stride} grid has {} anchors, expected {}",
                cls.len(),
                grid_w * grid_h
            ));
        }

        for p in 0..(grid_w * grid_h) {
            let score = (cls[p] * obj[p]).clamp(0.0, 1.0);
            if score < SCORE_THRESHOLD {
                continue;
            }
            let col = (p % grid_w) as f32;
            let row = (p / grid_w) as f32;
            let s = stride as f32;
            // Anchor-free box decode (centre + log-size). The frame sits at
            // the canvas origin, so canvas pixels are frame pixels.
            let cx = (col + bbox[p * 4]) * s;
            let cy = (row + bbox[p * 4 + 1]) * s;
            let bw = bbox[p * 4 + 2].exp() * s;
            let bh = bbox[p * 4 + 3].exp() * s;

            let mut landmarks5 = [[0.0_f32; 2]; 5];
            for (k, lm) in landmarks5.iter_mut().enumerate() {
                *lm = [
                    (col + kps[p * 10 + 2 * k]) * s,
                    (row + kps[p * 10 + 2 * k + 1]) * s,
                ];
            }

            candidates.push(FaceDetection {
                bbox: [cx - bw / 2.0, cy - bh / 2.0, bw, bh],
                landmarks5,
                score,
            });
            if candidates.len() >= MAX_CANDIDATES {
                break;
            }
        }
    }

    Ok(non_max_suppression(candidates, NMS_IOU_THRESHOLD))
}

pub(crate) fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let (ax0, ay0, aw, ah) = (a[0], a[1], a[2], a[3]);
    let (bx0, by0, bw, bh) = (b[0], b[1], b[2], b[3]);
    let (ax1, ay1) = (ax0 + aw, ay0 + ah);
    let (bx1, by1) = (bx0 + bw, by0 + bh);
    let ix0 = ax0.max(bx0);
    let iy0 = ay0.max(by0);
    let ix1 = ax1.min(bx1);
    let iy1 = ay1.min(by1);
    let iw = (ix1 - ix0).max(0.0);
    let ih = (iy1 - iy0).max(0.0);
    let inter = iw * ih;
    let union = aw * ah + bw * bh - inter;
    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}

fn non_max_suppression(mut boxes: Vec<FaceDetection>, iou_thr: f32) -> Vec<FaceDetection> {
    boxes.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut keep: Vec<FaceDetection> = Vec::new();
    for cand in boxes {
        if keep.iter().all(|k| iou(&k.bbox, &cand.bbox) < iou_thr) {
            keep.push(cand);
        }
    }
    keep
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank_frame(w: u32, h: u32) -> FaceFrame {
        FaceFrame {
            width: w,
            height: h,
            rgba: vec![0_u8; (w * h * 4) as usize],
        }
    }

    #[test]
    fn bundled_model_loads_and_runs() {
        // Cold-load + run on a blank frame. A black frame yields no
        // faces, but the parse → pad → decode → NMS path must complete
        // without error.
        let dets = detect(&blank_frame(1280, 720)).expect("detect should not error");
        assert!(dets.is_empty(), "blank frame should detect no faces");
    }

    #[test]
    fn frames_run_at_their_own_size_padded_to_the_stride() {
        assert_eq!(padded_shape(224, 168), (192, 224));
        assert_eq!(padded_shape(168, 224), (224, 192));
        assert_eq!(padded_shape(640, 480), (480, 640));
        assert_eq!(padded_shape(1, 1), (32, 32));
        // An odd size decodes without tripping the anchor-count check.
        assert!(detect(&blank_frame(225, 170)).unwrap().is_empty());
    }

    #[test]
    fn model_contract_holds_at_the_frame_size() {
        // Guard the 12-output / dynamic-input contract so a model swap
        // that breaks decode fails here, not silently in production.
        let runnable = build_runnable(192, 224).expect("build");
        let out = runnable
            .run(tvec!(Tensor::zero::<f32>(&[1, 3, 192, 224])
                .unwrap()
                .into()))
            .expect("run");
        assert_eq!(out.len(), 12, "YuNet must emit 12 output tensors");
        // Stride-8 grid = 28×24 = 672 anchors.
        assert_eq!(out[0].shape(), &[1, 672, 1], "cls stride-8 shape");
        assert_eq!(out[6].shape(), &[1, 672, 4], "bbox stride-8 shape");
        assert_eq!(out[9].shape(), &[1, 672, 10], "kps stride-8 shape");
        assert_eq!(out[2].shape(), &[1, 42, 1], "cls stride-32 shape");
    }

    /// onnxruntime run on the re-exported graph with the same gradient
    /// input; the sums come from `scripts/sentinel/export-yunet-dynamic.py`.
    #[test]
    fn dynamic_graph_matches_the_onnxruntime_reference() {
        let (h, w) = (192, 224);
        let mut data = vec![0.0_f32; 3 * h * w];
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                data[i] = x as f32 % 255.0;
                data[h * w + i] = y as f32 % 255.0;
                data[2 * h * w + i] = ((x + y) as f32 * 0.5) % 255.0;
            }
        }
        let out = build_runnable(h, w)
            .unwrap()
            .run(tvec!(Tensor::from_shape(&[1, 3, h, w], &data)
                .unwrap()
                .into()))
            .unwrap();
        let sum = |i: usize| out[i].to_array_view::<f32>().unwrap().iter().sum::<f32>();
        for (i, want) in [
            (0, 515.362_7_f32),
            (2, 23.840_708),
            (6, 1_777.860_4),
            (9, 3_513.295_4),
        ] {
            let got = sum(i);
            assert!(
                (got - want).abs() <= want.abs() * 1e-3,
                "output {i}: got {got}, want {want}"
            );
        }
    }

    #[test]
    fn empty_frame_returns_empty() {
        assert!(detect(&blank_frame(0, 0)).unwrap().is_empty());
    }

    #[test]
    fn iou_basic() {
        let a = [0.0, 0.0, 10.0, 10.0];
        let b = [0.0, 0.0, 10.0, 10.0];
        assert!((iou(&a, &b) - 1.0).abs() < 1e-6);
        let c = [100.0, 100.0, 10.0, 10.0];
        assert_eq!(iou(&a, &c), 0.0);
    }

    #[test]
    fn nms_suppresses_overlap() {
        let high = FaceDetection {
            bbox: [0.0, 0.0, 10.0, 10.0],
            landmarks5: [[0.0; 2]; 5],
            score: 0.9,
        };
        let dup = FaceDetection {
            bbox: [1.0, 1.0, 10.0, 10.0],
            landmarks5: [[0.0; 2]; 5],
            score: 0.8,
        };
        let far = FaceDetection {
            bbox: [100.0, 100.0, 10.0, 10.0],
            landmarks5: [[0.0; 2]; 5],
            score: 0.7,
        };
        let kept = non_max_suppression(vec![high, dup, far], NMS_IOU_THRESHOLD);
        assert_eq!(kept.len(), 2, "overlapping dup suppressed, far box kept");
        assert!((kept[0].score - 0.9).abs() < 1e-6);
    }
}
