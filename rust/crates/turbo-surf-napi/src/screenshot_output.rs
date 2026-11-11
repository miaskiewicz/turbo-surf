use image::codecs::jpeg::JpegEncoder;
use image::ImageFormat;
use napi::bindgen_prelude::*;
use napi_derive::napi;

/// Encode raster PNG output as JPEG without changing its CSS layout.
#[napi]
pub fn screenshot_jpeg(png: Buffer, quality: Option<u8>) -> Result<Buffer> {
    encode_jpeg(&png, quality.unwrap_or(80))
        .map(Buffer::from)
        .map_err(Error::from_reason)
}

fn encode_jpeg(png: &[u8], quality: u8) -> std::result::Result<Vec<u8>, String> {
    if quality > 100 {
        return Err("screenshot quality must be between 0 and 100".into());
    }
    let pixels = image::load_from_memory_with_format(png, ImageFormat::Png)
        .map_err(|e| e.to_string())?
        .into_rgb8();
    let mut output = Vec::new();
    JpegEncoder::new_with_quality(&mut output, quality)
        .encode_image(&pixels)
        .map_err(|e| e.to_string())?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn jpeg_preserves_dimensions_and_uses_requested_quality() {
        let pixels = image::RgbImage::from_fn(40, 30, |x, y| {
            image::Rgb([(x * 5) as u8, (y * 7) as u8, ((x + y) * 3) as u8])
        });
        let mut png = Cursor::new(Vec::new());
        pixels.write_to(&mut png, ImageFormat::Png).unwrap();
        let low = encode_jpeg(png.get_ref(), 10).unwrap();
        let high = encode_jpeg(png.get_ref(), 90).unwrap();
        assert_eq!(&low[..2], &[0xff, 0xd8]);
        assert_ne!(low, high);
        let decoded = image::load_from_memory(&high).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (40, 30));
        assert!(encode_jpeg(png.get_ref(), 101).is_err());
        assert!(encode_jpeg(b"invalid", 80).is_err());
    }
}
