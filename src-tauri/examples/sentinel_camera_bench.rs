//! Per-tick cost of the Sentinel camera path.
//!
//! Usage:
//!   cargo run --release --manifest-path src-tauri/Cargo.toml \
//!       --example sentinel_camera_bench -- [face.jpg]
//!
//! With a JPEG, the frame is that image downscaled to the 224 px longest
//! side the webview sends; without one, a synthetic textured frame (no
//! face, so detection decodes nothing). Prints the median of each stage,
//! then a simulated minute of 1 Hz pipeline ticks (detection on cadence,
//! tracking between, liveness on trigger) and the device benchmark.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use app_lib::sentinel::camera_budget::{self, CAMERA_PROFILE_BUDGET};
use app_lib::sentinel::camera_pipeline::{BundledModels, CameraPipeline, TickSource};
use app_lib::sentinel::types::FaceFrame;
use app_lib::sentinel::{face_detect, gaze, liveness};

const ITERS: usize = 30;
const LONG_SIDE: u32 = 224;

fn load_frame() -> Result<FaceFrame> {
    if let Some(path) = std::env::args().nth(1) {
        let img = image::open(&path).with_context(|| format!("open {path}"))?;
        let scale = LONG_SIDE as f32 / img.width().max(img.height()) as f32;
        let (w, h) = (
            (img.width() as f32 * scale).round() as u32,
            (img.height() as f32 * scale).round() as u32,
        );
        let rgba = img
            .resize_exact(w, h, image::imageops::FilterType::Triangle)
            .to_rgba8();
        return Ok(FaceFrame {
            width: w,
            height: h,
            rgba: rgba.into_raw(),
        });
    }
    let (w, h) = (LONG_SIDE, 168);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let v = ((x * 7 + y * 13) % 255) as u8;
            rgba.extend_from_slice(&[v, v / 2, 255 - v, 255]);
        }
    }
    Ok(FaceFrame {
        width: w,
        height: h,
        rgba,
    })
}

fn median_ms(mut f: impl FnMut()) -> f64 {
    f(); // warm: model parse + plan compile stay out of the steady state
    let mut samples: Vec<f64> = (0..ITERS)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    samples[ITERS / 2]
}

fn main() -> Result<()> {
    let frame = load_frame()?;
    println!("frame {}x{}", frame.width, frame.height);

    // IPC: the old path parsed a JSON number array into Vec<u8>.
    let json = serde_json::to_string(&frame.rgba)?;
    let ipc_json = median_ms(|| {
        let v: Vec<u8> = serde_json::from_str(&json).expect("parse");
        std::hint::black_box(v);
    });
    let ipc_raw = median_ms(|| {
        std::hint::black_box(frame.rgba.clone());
    });
    println!(
        "ipc payload: json {} KiB parse {ipc_json:.2} ms | raw {} KiB copy {ipc_raw:.3} ms",
        json.len() / 1024,
        frame.rgba.len() / 1024
    );

    let dets = face_detect::detect(&frame)?;
    let best = dets.first().cloned();
    println!(
        "faces {} best {:?}",
        dets.len(),
        best.as_ref().map(|d| d.score)
    );
    let detect = median_ms(|| {
        std::hint::black_box(face_detect::detect(&frame).expect("detect"));
    });
    let gaze_ms = median_ms(|| {
        std::hint::black_box(gaze::estimate(&frame, best.as_ref(), None).expect("gaze"));
    });
    let bbox = best
        .as_ref()
        .map(|d| d.bbox)
        .unwrap_or([60.0, 30.0, 80.0, 80.0]);
    let live = median_ms(|| {
        std::hint::black_box(liveness::score(&frame, bbox).expect("liveness"));
    });
    println!(
        "full tick (detect + gaze + liveness): {:.2} ms = detect {detect:.2} + gaze {gaze_ms:.3} + liveness {live:.2}",
        detect + gaze_ms + live
    );

    // Pipeline: one detection per cadence, tracking between, liveness on
    // trigger. Simulate a minute of 1 Hz ticks with a slowly moving frame.
    let mut pipeline = CameraPipeline::new(CAMERA_PROFILE_BUDGET.pipeline_config());
    let t0 = Instant::now();
    let (mut detected, mut tracked, mut live_reads) = (vec![], vec![], 0);
    for i in 0..60u64 {
        let shifted = shift(&frame, (i % 3) as i32);
        let t = Instant::now();
        let tick = pipeline.tick(&shifted, t0 + Duration::from_secs(i), &mut BundledModels)?;
        std::hint::black_box(gaze::estimate(&shifted, tick.detection.as_ref(), None)?);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        live_reads += tick.liveness.is_some() as usize;
        match tick.source {
            TickSource::Detected => detected.push(ms),
            TickSource::Tracked => tracked.push(ms),
        }
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
    let all: Vec<f64> = detected.iter().chain(&tracked).copied().collect();
    println!(
        "pipeline 60 ticks: {} detected (mean {:.2} ms), {} tracked (mean {:.3} ms), {live_reads} liveness reads; mean tick {:.2} ms",
        detected.len(),
        mean(&detected),
        tracked.len(),
        mean(&tracked),
        mean(&all)
    );
    let bench = camera_budget::benchmark()?;
    println!(
        "device benchmark: {bench:?} camera={} full={}",
        CAMERA_PROFILE_BUDGET.admits(&bench),
        camera_budget::FULL_PROFILE_BUDGET.admits(&bench)
    );
    Ok(())
}

/// Shift a frame right by `dx` pixels (edge-replicated), a stand-in for a
/// head drifting between ticks.
fn shift(frame: &FaceFrame, dx: i32) -> FaceFrame {
    let w = frame.width as i32;
    let mut rgba = frame.rgba.clone();
    for y in 0..frame.height as i32 {
        for x in 0..w {
            let sx = (x - dx).clamp(0, w - 1);
            let (dst, src) = (((y * w + x) * 4) as usize, ((y * w + sx) * 4) as usize);
            rgba[dst..dst + 4].copy_from_slice(&frame.rgba[src..src + 4]);
        }
    }
    FaceFrame {
        width: frame.width,
        height: frame.height,
        rgba,
    }
}
