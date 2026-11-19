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

/// Crop rendered PNG pixels while preserving the layout viewport.
#[napi]
pub fn screenshot_crop(png: Buffer, x: f64, y: f64, width: f64, height: f64) -> Result<Buffer> {
    crop_png(&png, x, y, width, height)
        .map(Buffer::from)
        .map_err(Error::from_reason)
}

fn crop_png(
    png: &[u8],
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> std::result::Result<Vec<u8>, String> {
    if ![x, y, width, height].iter().all(|v| v.is_finite())
        || x < 0.0
        || y < 0.0
        || width <= 0.0
        || height <= 0.0
    {
        return Err(
            "screenshot clip must have nonnegative coordinates and positive finite dimensions"
                .into(),
        );
    }
    let pixels =
        image::load_from_memory_with_format(png, ImageFormat::Png).map_err(|e| e.to_string())?;
    let left = (x.floor() as u32).min(pixels.width());
    let top = (y.floor() as u32).min(pixels.height());
    let right = ((x + width).ceil() as u32).min(pixels.width());
    let bottom = ((y + height).ceil() as u32).min(pixels.height());
    if right <= left || bottom <= top {
        return Err("screenshot clip is outside the rendered image".into());
    }
    let cropped = pixels.crop_imm(left, top, right - left, bottom - top);
    let mut output = std::io::Cursor::new(Vec::new());
    cropped
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(output.into_inner())
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

    #[test]
    fn crop_preserves_requested_pixels_and_checks_bounds() {
        let pixels =
            image::RgbaImage::from_fn(40, 30, |x, y| image::Rgba([x as u8, y as u8, 90, 255]));
        let mut png = Cursor::new(Vec::new());
        pixels.write_to(&mut png, ImageFormat::Png).unwrap();
        let cropped = crop_png(png.get_ref(), 5.0, 6.0, 20.0, 10.0).unwrap();
        let decoded = image::load_from_memory(&cropped).unwrap().into_rgba8();
        assert_eq!(decoded.dimensions(), (20, 10));
        assert_eq!(*decoded.get_pixel(0, 0), pixels[(5, 6)]);
        assert_eq!(*decoded.get_pixel(19, 9), pixels[(24, 15)]);
        let clipped = crop_png(png.get_ref(), 35.0, 25.0, 20.0, 10.0).unwrap();
        let decoded = image::load_from_memory(&clipped).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (5, 5));
        assert!(crop_png(png.get_ref(), 50.0, 0.0, 2.0, 2.0).is_err());
        assert!(crop_png(png.get_ref(), -1.0, 0.0, 2.0, 2.0).is_err());
        assert!(crop_png(png.get_ref(), 0.0, 0.0, f64::NAN, 2.0).is_err());
    }
}
