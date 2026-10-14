//! Frames for the webview, as base64 JPEG.
//!
//! Until frames reach the page over a binary channel, each tile's picture is
//! a JPEG string sent as an event. The work is scaling, a colour conversion
//! and the encode, and it runs on a blocking thread so the async workers the
//! call's networking shares never wait on it.
use std::io::Cursor;

use base64::Engine;
use image::ImageEncoder;

/// A frame for the webview: scaled to fit `max` (never up), as base64 JPEG.
pub(super) fn encode(
    img: image::RgbaImage,
    max: (u32, u32),
    quality: u8,
) -> Result<(String, u32, u32), String> {
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
    let rgb: Vec<u8> = img
        .as_raw()
        .chunks_exact(4)
        .flat_map(|px| [px[0], px[1], px[2]])
        .collect();
    let mut jpeg = Vec::with_capacity((width * height) as usize / 4);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut Cursor::new(&mut jpeg), quality)
        .write_image(&rgb, width, height, image::ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    Ok((
        base64::engine::general_purpose::STANDARD.encode(&jpeg),
        width,
        height,
    ))
}
