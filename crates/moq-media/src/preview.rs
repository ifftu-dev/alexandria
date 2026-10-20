// Copyright 2026 Alexandria Pvt. Ltd.
// Licensed under the Apache License, Version 2.0 — see LICENSE-APACHE.
//
// Original Alexandria work, not derived from n0-computer/iroh-live.
// See crates/VENDORING.md.

//! Video frames as JPEG, for showing in a webview.
//!
//! `image`'s JPEG encoder, scaler and colour conversion are generic, so they
//! are compiled into whichever crate calls them, at that crate's
//! optimisation level. Called from the app's development build they ran
//! unoptimised: 63 ms to encode one 960x540 frame, which held the desktop
//! self view to 15 fps. Encoding here gets this crate's optimised build.

use std::io::Cursor;

use anyhow::Result;
use image::ImageEncoder;

/// Encode `img` as JPEG, scaled down to fit `max` (never up). JPEG has no
/// alpha, so alpha is dropped rather than blending the picture away.
/// Returns the JPEG and its dimensions.
pub fn jpeg(img: image::RgbaImage, max: (u32, u32), quality: u8) -> Result<(Vec<u8>, u32, u32)> {
    let (w, h) = img.dimensions();
    let img = if w > max.0 || h > max.1 {
        let scale = f64::min(max.0 as f64 / w as f64, max.1 as f64 / h as f64);
        let (sw, sh) = (
            ((w as f64 * scale).round() as u32).max(1),
            ((h as f64 * scale).round() as u32).max(1),
        );
        image::imageops::thumbnail(&img, sw, sh)
    } else {
        img
    };
    let (width, height) = img.dimensions();
    let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
    let mut jpeg = Vec::with_capacity((width * height) as usize / 4);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut Cursor::new(&mut jpeg), quality)
        .write_image(rgb.as_raw(), width, height, image::ExtendedColorType::Rgb8)?;
    Ok((jpeg, width, height))
}

#[cfg(test)]
mod tests {
    #[test]
    fn rgba_frames_encode_as_jpeg_scaled_down_to_fit() {
        let red = image::RgbaImage::from_pixel(64, 32, image::Rgba([220, 20, 20, 0]));
        let (bytes, w, h) = super::jpeg(red, (32, 32), 90).expect("encode");
        assert_eq!((w, h), (32, 16));
        let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg)
            .expect("a JPEG")
            .to_rgb8();
        assert_eq!(decoded.dimensions(), (32, 16));
        // Alpha 0 must not blank the picture: JPEG has no alpha, the colour stays.
        let px = decoded.get_pixel(16, 8);
        assert!(px[0] > 180 && px[1] < 60 && px[2] < 60, "{px:?}");
    }

    #[test]
    fn small_frames_are_never_scaled_up() {
        let frame = image::RgbaImage::from_pixel(20, 10, image::Rgba([0, 0, 0, 255]));
        let (_, w, h) = super::jpeg(frame, (360, 640), 75).expect("encode");
        assert_eq!((w, h), (20, 10));
    }
}
