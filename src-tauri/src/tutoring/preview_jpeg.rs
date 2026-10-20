//! Frames for the webview, as base64 JPEG.
//!
//! Until frames reach the page over a binary channel, each tile's picture is
//! a JPEG string sent as an event. The work is scaling, dropping the alpha
//! channel and the encode, and it runs on a blocking thread so the async workers the
//! call's networking shares never wait on it. The encode itself lives in the
//! media crate, which development builds optimise (see `live::media::preview`).
use base64::Engine;

/// A frame for the webview: scaled to fit `max` (never up), as base64 JPEG.
pub(super) fn encode(
    img: image::RgbaImage,
    max: (u32, u32),
    quality: u8,
) -> Result<(String, u32, u32), String> {
    let (jpeg, width, height) =
        live::media::preview::jpeg(img, max, quality).map_err(|e| e.to_string())?;
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
    fn frames_arrive_as_base64_jpeg() {
        let frame = image::RgbaImage::from_pixel(64, 32, image::Rgba([220, 20, 20, 255]));
        let (b64, w, h) = super::encode(frame, (32, 32), 90).expect("encode");
        assert_eq!((w, h), (32, 16));
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("base64");
        let decoded =
            image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg).expect("a JPEG");
        assert_eq!((decoded.width(), decoded.height()), (32, 16));
    }
}
