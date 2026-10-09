//! Face liveness (presentation-attack) detector — MiniFASNetV2 via tract.
//!
//! Closes the gap the virtual-camera label heuristic leaves: a learner who
//! feeds a photo, a replayed video or a rendered face through any camera
//! device. The model is the Apache-2.0 `2.7_80x80_MiniFASNetV2` from
//! minivision-ai's Silent-Face-Anti-Spoofing, exported to ONNX with softmax
//! folded in (`scripts/sentinel/export-minifasnet.py` is the recipe)
//! and embedded at compile time like YuNet and the paste classifier.
//!
//! Input contract, mirroring the upstream `CropImage` + `ToTensor`:
//! the YuNet face box scaled 2.7× about its centre (clamped to the frame),
//! resized to 80×80 with bilinear sampling, BGR channel order, raw 0..255
//! values (upstream's `ToTensor` does not divide by 255). Output is the
//! 3-class softmax; class 1 is "real", the other two are attack classes.
//!
//! Advisory: a single low score is a lighting or webcam artefact as often as
//! an attack, so the frontend promotes a flag only from a ratio over a
//! snapshot window. Frames are processed in memory and dropped.

use std::io::Cursor;
use std::sync::{Arc, OnceLock, RwLock};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use tract_onnx::prelude::*;

use super::types::FaceFrame;

const BUNDLED_MINIFASNET: &[u8] = include_bytes!("../../resources/sentinel/minifasnet-v2-80.onnx");

/// Square the model is exported at.
pub const INPUT_SIZE: usize = 80;

/// Upstream patch scale: the face box is enlarged this much about its centre
/// before the crop, so the patch carries face context (edges of a phone or
/// print show up here).
pub const CROP_SCALE: f32 = 2.7;

/// Softmax index of the genuine-face class.
pub const REAL_CLASS: usize = 1;

/// Below this `real_prob` a tick counts as a possible spoof.
pub const SPOOF_THRESHOLD: f32 = 0.5;

/// Softmax over `[attack_a, real, attack_b]` for one face patch.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct LivenessEstimate {
    /// Probability the face is a live person in front of the camera.
    pub real_prob: f32,
    /// `real_prob < SPOOF_THRESHOLD`.
    pub spoof_suspected: bool,
}

type Runnable = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

static MODEL: OnceLock<RwLock<Arc<Runnable>>> = OnceLock::new();

fn build_runnable(bytes: &[u8]) -> Result<Arc<Runnable>> {
    let model = tract_onnx::onnx()
        .model_for_read(&mut Cursor::new(bytes))
        .context("parse MiniFASNet ONNX bytes")?
        .with_input_fact(
            0,
            f32::fact([1, 3, INPUT_SIZE as i32, INPUT_SIZE as i32]).into(),
        )
        .context("bind MiniFASNet input fact [1,3,80,80]")?
        .into_optimized()
        .context("optimize MiniFASNet graph")?
        .into_runnable()
        .context("compile MiniFASNet runnable")?;
    Ok(Arc::new(model))
}

fn ensure_initialized() -> &'static RwLock<Arc<Runnable>> {
    MODEL.get_or_init(|| {
        let model = build_runnable(BUNDLED_MINIFASNET)
            .expect("bundled minifasnet-v2-80.onnx failed to parse — release-blocking");
        RwLock::new(model)
    })
}

/// Integer crop box `(x0, y0, x1, y1)`, inclusive, after scaling `bbox`
/// about its centre and sliding it back inside the frame. Port of the
/// upstream `CropImage._get_new_box`.
pub fn crop_box(src_w: u32, src_h: u32, bbox: [f32; 4], scale: f32) -> (u32, u32, u32, u32) {
    let (src_w, src_h) = (src_w as f32, src_h as f32);
    let [x, y, box_w, box_h] = bbox;
    let box_w = box_w.max(1.0);
    let box_h = box_h.max(1.0);
    let scale = scale.min((src_h - 1.0) / box_h).min((src_w - 1.0) / box_w);
    let new_w = box_w * scale;
    let new_h = box_h * scale;
    let cx = x + box_w / 2.0;
    let cy = y + box_h / 2.0;
    let mut l = cx - new_w / 2.0;
    let mut t = cy - new_h / 2.0;
    let mut r = cx + new_w / 2.0;
    let mut b = cy + new_h / 2.0;
    if l < 0.0 {
        r -= l;
        l = 0.0;
    }
    if t < 0.0 {
        b -= t;
        t = 0.0;
    }
    if r > src_w - 1.0 {
        l -= r - src_w + 1.0;
        r = src_w - 1.0;
    }
    if b > src_h - 1.0 {
        t -= b - src_h + 1.0;
        b = src_h - 1.0;
    }
    let clamp = |v: f32, max: f32| v.max(0.0).min(max) as u32;
    (
        clamp(l, src_w - 1.0),
        clamp(t, src_h - 1.0),
        clamp(r, src_w - 1.0),
        clamp(b, src_h - 1.0),
    )
}

/// Bilinear sample of one channel at fractional source coordinates.
fn sample(frame: &FaceFrame, x: f32, y: f32, channel: usize) -> f32 {
    let max_x = (frame.width - 1) as f32;
    let max_y = (frame.height - 1) as f32;
    let x = x.max(0.0).min(max_x);
    let y = y.max(0.0).min(max_y);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(frame.width - 1);
    let y1 = (y0 + 1).min(frame.height - 1);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let px = |xx: u32, yy: u32| frame.rgba[((yy * frame.width + xx) * 4) as usize + channel] as f32;
    let top = px(x0, y0) * (1.0 - fx) + px(x1, y0) * fx;
    let bottom = px(x0, y1) * (1.0 - fx) + px(x1, y1) * fx;
    top * (1.0 - fy) + bottom * fy
}

/// Build the `[1,3,80,80]` BGR 0..255 tensor from the scaled face crop.
fn preprocess(frame: &FaceFrame, bbox: [f32; 4]) -> Result<Tensor> {
    if frame.width == 0 || frame.height == 0 {
        return Err(anyhow!("empty frame"));
    }
    if frame.rgba.len() < (frame.width as usize * frame.height as usize * 4) {
        return Err(anyhow!(
            "frame buffer too small: {} bytes for {}x{} RGBA",
            frame.rgba.len(),
            frame.width,
            frame.height
        ));
    }
    let (x0, y0, x1, y1) = crop_box(frame.width, frame.height, bbox, CROP_SCALE);
    let crop_w = (x1 - x0 + 1) as f32;
    let crop_h = (y1 - y0 + 1) as f32;
    let n = INPUT_SIZE * INPUT_SIZE;
    let mut data = vec![0.0_f32; 3 * n];
    // Half-pixel centres, like cv2.resize INTER_LINEAR.
    let sx = crop_w / INPUT_SIZE as f32;
    let sy = crop_h / INPUT_SIZE as f32;
    for dy in 0..INPUT_SIZE {
        let src_y = y0 as f32 + (dy as f32 + 0.5) * sy - 0.5;
        for dx in 0..INPUT_SIZE {
            let src_x = x0 as f32 + (dx as f32 + 0.5) * sx - 0.5;
            let dst = dy * INPUT_SIZE + dx;
            data[dst] = sample(frame, src_x, src_y, 2); // B
            data[n + dst] = sample(frame, src_x, src_y, 1); // G
            data[2 * n + dst] = sample(frame, src_x, src_y, 0); // R
        }
    }
    Tensor::from_shape(&[1, 3, INPUT_SIZE, INPUT_SIZE], &data)
}

fn run(input: Tensor) -> Result<[f32; 3]> {
    let model = ensure_initialized()
        .read()
        .map_err(|_| anyhow!("liveness model lock poisoned"))?
        .clone();
    let out = model.run(tvec!(input.into()))?;
    let probs = out[0].to_array_view::<f32>()?;
    let v: Vec<f32> = probs.iter().copied().collect();
    if v.len() != 3 {
        return Err(anyhow!("unexpected liveness output length {}", v.len()));
    }
    Ok([v[0], v[1], v[2]])
}

fn estimate_from(probs: [f32; 3]) -> LivenessEstimate {
    let real_prob = probs[REAL_CLASS].clamp(0.0, 1.0);
    LivenessEstimate {
        real_prob,
        spoof_suspected: real_prob < SPOOF_THRESHOLD,
    }
}

/// Score the face at `bbox` (YuNet `[x, y, w, h]` in frame pixels).
pub fn score(frame: &FaceFrame, bbox: [f32; 4]) -> Result<LivenessEstimate> {
    let input = preprocess(frame, bbox)?;
    Ok(estimate_from(run(input)?))
}

/// Parse-check the bundled weights at startup so a broken artifact fails
/// loudly rather than on the first camera tick.
pub fn warm() -> Result<()> {
    let _ = ensure_initialized();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_frame(w: u32, h: u32, rgb: [u8; 3]) -> FaceFrame {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        FaceFrame {
            width: w,
            height: h,
            rgba,
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 2e-3
    }

    #[test]
    fn crop_box_scales_about_the_centre_and_stays_inside() {
        // A 20×20 box centred at (50,50) in a 200×160 frame scales to 54×54.
        let (l, t, r, b) = crop_box(200, 160, [40.0, 40.0, 20.0, 20.0], 2.7);
        assert_eq!((l, t, r, b), (23, 23, 77, 77));
        // Near the corner, the box slides inside instead of clipping.
        let (l, t, r, b) = crop_box(200, 160, [0.0, 0.0, 20.0, 20.0], 2.7);
        assert_eq!((l, t), (0, 0));
        assert_eq!((r, b), (54, 54));
        // A box larger than the frame caps the scale to the frame.
        let (l, t, r, b) = crop_box(100, 100, [10.0, 10.0, 80.0, 80.0], 2.7);
        assert_eq!((l, t, r, b), (0, 0, 99, 99));
    }

    #[test]
    fn bundled_model_loads_and_matches_the_torch_reference() {
        // Reference softmax values come from the export script run against
        // the same weights (see module docs). Zero and mid-grey inputs.
        let zeros = run(Tensor::zero::<f32>(&[1, 3, INPUT_SIZE, INPUT_SIZE]).unwrap()).unwrap();
        let mid = run(Tensor::from_shape(
            &[1, 3, INPUT_SIZE, INPUT_SIZE],
            &vec![128.0_f32; 3 * INPUT_SIZE * INPUT_SIZE],
        )
        .unwrap())
        .unwrap();
        let sum: f32 = zeros.iter().sum();
        assert!(close(sum, 1.0), "softmax should sum to 1, got {sum}");
        for (got, want) in zeros.iter().zip(REF_ZEROS) {
            assert!(close(*got, want), "zeros: got {got}, want {want}");
        }
        for (got, want) in mid.iter().zip(REF_MID) {
            assert!(close(*got, want), "mid: got {got}, want {want}");
        }
    }

    #[test]
    fn a_flat_frame_scores_without_error_and_reports_a_probability() {
        let frame = flat_frame(160, 120, [120, 100, 90]);
        let est = score(&frame, [50.0, 30.0, 60.0, 60.0]).unwrap();
        assert!((0.0..=1.0).contains(&est.real_prob));
        assert_eq!(est.spoof_suspected, est.real_prob < SPOOF_THRESHOLD);
    }

    /// The upstream pipeline (cv2 crop + INTER_LINEAR resize + raw BGR) run on
    /// this same synthetic frame through onnxruntime gives the reference box
    /// and softmax below. Pins the preprocess port, not just the graph.
    #[test]
    fn preprocess_port_matches_the_upstream_cv2_pipeline() {
        let (w, h) = (160u32, 120u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let (r, g, b) = if (40..90).contains(&y) && (60..110).contains(&x) {
                    (200, 170, 150)
                } else {
                    (
                        (x * 255 / (w - 1)) as u8,
                        (y * 255 / (h - 1)) as u8,
                        ((x + y) * 255 / (w + h - 2)) as u8,
                    )
                };
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
        }
        let frame = FaceFrame {
            width: w,
            height: h,
            rgba,
        };
        let bbox = [55.0, 35.0, 60.0, 60.0];
        assert_eq!(crop_box(w, h, bbox, CROP_SCALE), (25, 0, 144, 119));
        let probs = run(preprocess(&frame, bbox).unwrap()).unwrap();
        let want = [0.000_089_3, 0.225_82, 0.774_09];
        for (got, want) in probs.iter().zip(want) {
            assert!(
                (got - want).abs() < 0.02,
                "pipeline drift: got {got}, want {want}"
            );
        }
    }

    #[test]
    fn a_short_buffer_is_refused() {
        let frame = FaceFrame {
            width: 10,
            height: 10,
            rgba: vec![0; 10],
        };
        assert!(score(&frame, [0.0, 0.0, 5.0, 5.0]).is_err());
    }

    #[test]
    fn preprocess_is_bgr_with_raw_byte_range() {
        let frame = flat_frame(100, 100, [200, 100, 50]);
        let t = preprocess(&frame, [20.0, 20.0, 30.0, 30.0]).unwrap();
        let v = t.to_array_view::<f32>().unwrap();
        let n = INPUT_SIZE * INPUT_SIZE;
        let flat: Vec<f32> = v.iter().copied().collect();
        assert!(close(flat[0], 50.0), "B plane first");
        assert!(close(flat[n], 100.0), "G plane second");
        assert!(close(flat[2 * n], 200.0), "R plane third");
    }

    // Softmax the PyTorch model returns for the same inputs, captured by the
    // export script. A tract regression or a swapped artifact fails here.
    const REF_ZEROS: [f32; 3] = [0.000_5, 0.007_77, 0.991_74];
    const REF_MID: [f32; 3] = [0.024_81, 0.247_54, 0.727_65];
}
