//! Frames for the webview, as base64 JPEG.
//!
//! Until frames reach the page over a binary channel, each tile's picture is
//! a JPEG string sent as an event. The work is scaling, dropping the alpha
//! channel and the encode, and it runs on a blocking thread so the async workers the
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
    // JPEG has no alpha; `image` drops it (in optimised code, unlike a
    // hand-written loop in this crate's debug build).
    let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
    let mut jpeg = Vec::with_capacity((width * height) as usize / 4);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut Cursor::new(&mut jpeg), quality)
        .write_image(rgb.as_raw(), width, height, image::ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    Ok((
        base64::engine::general_purpose::STANDARD.encode(&jpeg),
        width,
        height,
    ))
}

#[cfg(test)]
mod tests {
    use base64::Engine;

    #[test]
    fn rgba_frames_encode_as_jpeg_scaled_down_to_fit() {
        let red = image::RgbaImage::from_pixel(64, 32, image::Rgba([220, 20, 20, 0]));
        let (b64, w, h) = super::encode(red, (32, 32), 90).expect("encode");
        assert_eq!((w, h), (32, 16));
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("base64");
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
        let (_, w, h) = super::encode(frame, (360, 640), 75).expect("encode");
        assert_eq!((w, h), (20, 10));
    }
}
