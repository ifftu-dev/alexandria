// Copyright 2026 Alexandria Pvt. Ltd.
// Licensed under the Apache License, Version 2.0 — see LICENSE-APACHE.
//
// Original Alexandria work, not derived from n0-computer/iroh-live.
// See crates/VENDORING.md.

//! End to end: a 30 fps camera, the real H.264 encoder, a MoQ broadcast,
//! and a receiver whose decoder is slower than the sender, as an iPhone's
//! debug build was. The picture it shows must stay near live rather than
//! fall further behind every second (15 Oct: ~100 s behind after 2 min).
//!
//! Runs real encoding for several seconds, so it is ignored by default:
//! `cargo test -p moq-media --test live_latency -- --ignored`.
#![cfg(feature = "video")]

use std::time::{Duration, Instant};

use moq_media::{
    av::{
        DecodeConfig, DecodedFrame, PixelFormat, Quality, VideoDecoder, VideoFormat, VideoFrame,
        VideoPreset, VideoSource,
    },
    ffmpeg::{FfmpegVideoDecoder, H264Encoder},
    publish::{PublishBroadcast, VideoRenditions},
    subscribe::SubscribeBroadcast,
};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;

/// A camera that delivers a frame every 1/30 s, blocking like nokhwa does.
struct Camera {
    started: Instant,
    sent: u32,
}

impl VideoSource for Camera {
    fn name(&self) -> &str {
        "cam"
    }
    fn format(&self) -> VideoFormat {
        VideoFormat {
            pixel_format: PixelFormat::Rgba,
            dimensions: [WIDTH, HEIGHT],
        }
    }
    fn pop_frame(&mut self) -> anyhow::Result<Option<VideoFrame>> {
        self.sent += 1;
        let due = self.started + Duration::from_secs_f64(f64::from(self.sent) / 30.0);
        std::thread::sleep(due.saturating_duration_since(Instant::now()));
        // Moving content, so frames aren't trivially identical.
        let shade = (self.sent % 256) as u8;
        let raw: Vec<u8> = (0..WIDTH * HEIGHT)
            .flat_map(|i| [shade, (i % 251) as u8, 255 - shade, 255])
            .collect();
        Ok(Some(VideoFrame {
            format: self.format(),
            raw: raw.into(),
        }))
    }
    fn start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// ffmpeg's decoder, but every frame it hands out costs 70 ms, about what
/// copying a 1080p frame out cost on the iPhone.
struct SlowDecoder(FfmpegVideoDecoder);

impl VideoDecoder for SlowDecoder {
    fn new(config: &hang::catalog::VideoConfig, playback: &DecodeConfig) -> anyhow::Result<Self> {
        Ok(Self(FfmpegVideoDecoder::new(config, playback)?))
    }
    fn name(&self) -> &str {
        "slow"
    }
    fn pop_frame(&mut self) -> anyhow::Result<Option<DecodedFrame>> {
        let frame = self.0.pop_frame()?;
        if frame.is_some() {
            std::thread::sleep(Duration::from_millis(70));
        }
        Ok(frame)
    }
    fn push_packet(&mut self, packet: moq_mux::container::Frame) -> anyhow::Result<()> {
        self.0.push_packet(packet)
    }
    fn set_viewport(&mut self, w: u32, h: u32) {
        self.0.set_viewport(w, h)
    }
    fn discard_frames(&mut self) -> anyhow::Result<()> {
        self.0.discard_frames()
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "encodes real video for ~12 s"]
async fn a_slow_receiver_stays_near_live() {
    let camera = Camera {
        started: Instant::now(),
        sent: 0,
    };
    let mut broadcast = PublishBroadcast::new();
    broadcast
        .set_video(Some(VideoRenditions::new::<H264Encoder>(
            camera,
            [VideoPreset::P360],
        )))
        .expect("set video");

    let subscribed = SubscribeBroadcast::new("cam".into(), broadcast.producer().consume())
        .await
        .expect("subscribe");
    let track = subscribed
        .watch_with::<SlowDecoder>(&DecodeConfig::default(), Quality::Highest)
        .expect("watch");
    let (mut frames, handle) = track.split();
    handle.set_viewport(WIDTH, HEIGHT);

    // Lag = how far the shown picture's timestamp trails wall time, relative
    // to the first frame (which sets the baseline).
    let mut baseline: Option<(Instant, Duration)> = None;
    let mut worst_lag = Duration::ZERO;
    let mut shown = 0u32;
    let deadline = Instant::now() + Duration::from_secs(12);
    while Instant::now() < deadline {
        let Ok(Some(frame)) =
            tokio::time::timeout(Duration::from_secs(3), frames.next_frame()).await
        else {
            panic!("video stopped after {shown} frames");
        };
        shown += 1;
        let (wall0, ts0) = *baseline.get_or_insert((Instant::now(), frame.timestamp));
        let lag = wall0
            .elapsed()
            .saturating_sub(frame.timestamp.saturating_sub(ts0));
        // Ignore the first seconds while the stream settles.
        if wall0.elapsed() > Duration::from_secs(3) {
            worst_lag = worst_lag.max(lag);
        }
    }
    eprintln!("shown {shown} frames in 12 s, worst lag {worst_lag:?}");
    assert!(shown > 60, "only {shown} frames shown");
    assert!(
        worst_lag < Duration::from_millis(1500),
        "the picture fell {worst_lag:?} behind"
    );
}
