//! YuNet face detector — backend inference via tract.
//!
//! Mirrors the `paste_classifier` pattern: the bundled `yunet.onnx`
//! (OpenCV Zoo, MIT-licensed, `face_detection_yunet_2023mar`) is
//! embedded at compile time via `include_bytes!`, parsed once into a
//! process-wide `OnceLock`, and run on every camera tick. Pure-Rust
//! tract — no WASM, no native ONNX runtime — so it compiles for every
//! Tauri target including iOS/Android.
//!
//! The model is baked at a **fixed 640×640 input** (its declared input
//! fact is `1,3,640,640`; other resolutions fail tract shape inference
//! because the export has hard-coded spatial constants). We letterbox
//! the incoming frame into that square and invert the transform on the
//! decoded boxes, so callers pass frames of any size.
//!
//! Output contract (verified at load + in tests): 12 tensors —
//! `cls×3, obj×3, bbox×3, kps×3`, each triplet ordered by stride
//! `[8, 16, 32]`. A model swap that changes this shape fails loudly in
//! `model_contract_holds`.

use std::io::Cursor;
use std::sync::{Arc, OnceLock, RwLock};

use anyhow::{anyhow, Context, Result};
use tract_onnx::prelude::*;

use super::types::{FaceDetection, FaceFrame};

/// Bundled YuNet weights. Always available; only fails to load on a
/// catastrophic tract-parser regression (release-blocking, asserted in
/// `ensure_initialized`).
const BUNDLED_YUNET: &[u8] = include_bytes!("../../resources/sentinel/yunet.onnx");

/// Fixed square the model is exported at. Do not change without
/// re-exporting the ONNX — the graph has baked spatial constants.
pub const INPUT_SIZE: usize = 640;

/// Anchor-free strides. Output grids are `INPUT_SIZE / stride`:
/// 80×80, 40×40, 20×20.
const STRIDES: [usize; 3] = [8, 16, 32];

/// Minimum `cls * obj` probability for a detection to survive decode.
const SCORE_THRESHOLD: f32 = 0.70;

/// IoU above which a lower-scoring box is suppressed in NMS.
const NMS_IOU_THRESHOLD: f32 = 0.30;

/// Hard cap on candidate boxes fed into NMS — defends against a
/// degenerate frame that clears threshold everywhere.
const MAX_CANDIDATES: usize = 1024;

type Runnable = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

static DETECTOR: OnceLock<RwLock<Arc<Runnable>>> = OnceLock::new();

fn build_runnable(bytes: &[u8]) -> Result<Arc<Runnable>> {
    let model = tract_onnx::onnx()
        .model_for_read(&mut Cursor::new(bytes))
        .context("parse YuNet ONNX bytes")?
        .with_input_fact(
            0,
            f32::fact([1, 3, INPUT_SIZE as i32, INPUT_SIZE as i32]).into(),
        )
        .context("bind YuNet input fact [1,3,640,640]")?
        .into_optimized()
        .context("optimize YuNet graph")?
        .into_runnable()
        .context("compile YuNet runnable")?;
    Ok(Arc::new(model))
}

fn ensure_initialized() -> &'static RwLock<Arc<Runnable>> {
    DETECTOR.get_or_init(|| {
        let model = build_runnable(BUNDLED_YUNET)
            .expect("bundled yunet.onnx failed to parse — release-blocking");
        RwLock::new(model)
    })
}

/// Letterbox transform: how the original frame maps into the 640² square.
struct Letterbox {
    scale: f32,
    pad_x: f32,
    pad_y: f32,
}

impl Letterbox {
    fn compute(w: u32, h: u32) -> Self {
        let scale = (INPUT_SIZE as f32 / w as f32).min(INPUT_SIZE as f32 / h as f32);
        let new_w = w as f32 * scale;
        let new_h = h as f32 * scale;
        Letterbox {
            scale,
            pad_x: (INPUT_SIZE as f32 - new_w) / 2.0,
            pad_y: (INPUT_SIZE as f32 - new_h) / 2.0,
        }
    }

    /// Map a coordinate from 640²-letterboxed space back to original
    /// frame pixels.
    fn invert(&self, x: f32, y: f32) -> (f32, f32) {
        ((x - self.pad_x) / self.scale, (y - self.pad_y) / self.scale)
    }
}

/// Build the NCHW BGR f32 input tensor by letterboxing `frame` into a
/// 640² canvas. YuNet was trained on raw BGR 0-255 (OpenCV convention)
/// with no mean subtraction.
fn preprocess(frame: &FaceFrame, lb: &Letterbox) -> Result<Tensor> {
    if frame.rgba.len() < (frame.width as usize * frame.height as usize * 4) {
        return Err(anyhow!(
            "frame buffer too small: {} bytes for {}x{} RGBA",
            frame.rgba.len(),
            frame.width,
            frame.height
        ));
    }
    let n = INPUT_SIZE * INPUT_SIZE;
    // Plane order B, G, R (channels-first). Init to 0 (black letterbox bars).
    let mut data = vec![0.0_f32; 3 * n];
    let (fw, fh) = (frame.width as f32, frame.height as f32);
    for dy in 0..INPUT_SIZE {
        for dx in 0..INPUT_SIZE {
            // Nearest-neighbour sample back into the source frame.
            let sx = (dx as f32 - lb.pad_x) / lb.scale;
            let sy = (dy as f32 - lb.pad_y) / lb.scale;
            if sx < 0.0 || sy < 0.0 || sx >= fw || sy >= fh {
                continue; // padding bar
            }
            let src = ((sy as u32 * frame.width + sx as u32) * 4) as usize;
            let r = frame.rgba[src] as f32;
            let g = frame.rgba[src + 1] as f32;
            let b = frame.rgba[src + 2] as f32;
            let dst = dy * INPUT_SIZE + dx;
            data[dst] = b; // B plane
            data[n + dst] = g; // G plane
            data[2 * n + dst] = r; // R plane
        }
    }
    Tensor::from_shape(&[1, 3, INPUT_SIZE, INPUT_SIZE], &data)
}

/// Detect faces in `frame`. Returns bounding boxes + 5 landmarks +
/// score in **original frame pixel coordinates**, NMS-deduplicated and
/// sorted by descending score. Never panics; a tract failure surfaces
/// as `Err`.
pub fn detect(frame: &FaceFrame) -> Result<Vec<FaceDetection>> {
    if frame.width == 0 || frame.height == 0 {
        return Ok(vec![]);
    }
    let lb = Letterbox::compute(frame.width, frame.height);
    let input = preprocess(frame, &lb)?;

    let runnable = ensure_initialized()
        .read()
        .map_err(|_| anyhow!("detector rwlock poisoned"))?
        .clone();
    let outputs = runnable
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
        let grid = INPUT_SIZE / stride;
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

        for p in 0..(grid * grid) {
            let score = (cls[p] * obj[p]).clamp(0.0, 1.0);
            if score < SCORE_THRESHOLD {
                continue;
            }
            let col = (p % grid) as f32;
            let row = (p / grid) as f32;
            let s = stride as f32;
            // Anchor-free box decode (centre + log-size), in 640 space.
            let cx = (col + bbox[p * 4]) * s;
            let cy = (row + bbox[p * 4 + 1]) * s;
            let bw = bbox[p * 4 + 2].exp() * s;
            let bh = bbox[p * 4 + 3].exp() * s;
            let (x0, y0) = lb.invert(cx - bw / 2.0, cy - bh / 2.0);
            let (x1, y1) = lb.invert(cx + bw / 2.0, cy + bh / 2.0);

            let mut landmarks5 = [[0.0_f32; 2]; 5];
            for (k, lm) in landmarks5.iter_mut().enumerate() {
                let lx = (col + kps[p * 10 + 2 * k]) * s;
                let ly = (row + kps[p * 10 + 2 * k + 1]) * s;
                let (ox, oy) = lb.invert(lx, ly);
                *lm = [ox, oy];
            }

            candidates.push(FaceDetection {
                bbox: [x0, y0, x1 - x0, y1 - y0],
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

fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
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
        // faces, but the parse → letterbox → decode → NMS path must
        // complete without error.
        let dets = detect(&blank_frame(1280, 720)).expect("detect should not error");
        assert!(dets.is_empty(), "blank frame should detect no faces");
    }

    #[test]
    fn model_contract_holds() {
        // Guard the 12-output / fixed-input contract so a model swap
        // that breaks decode fails here, not silently in production.
        let runnable = build_runnable(BUNDLED_YUNET).expect("build");
        let out = runnable
            .run(tvec!(Tensor::zero::<f32>(&[1, 3, INPUT_SIZE, INPUT_SIZE])
                .unwrap()
                .into()))
            .expect("run");
        assert_eq!(out.len(), 12, "YuNet must emit 12 output tensors");
        // Stride-8 grid = 80×80 = 6400 anchors.
        assert_eq!(out[0].shape(), &[1, 6400, 1], "cls stride-8 shape");
        assert_eq!(out[6].shape(), &[1, 6400, 4], "bbox stride-8 shape");
        assert_eq!(out[9].shape(), &[1, 6400, 10], "kps stride-8 shape");
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
