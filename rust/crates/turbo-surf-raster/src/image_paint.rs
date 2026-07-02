//! Decode fetched image bytes (PNG/JPEG/GIF/WebP + SVG) into a single canonical
//! form — straight-alpha RGBA8 — that both painters and the layout engine consume:
//!
//! - **Layout sizing**: turbo-html2pdf's `probe` only reads PNG/JPEG/SVG, so we
//!   normalize *every* decoded image to a PNG ([`DecodedImage::to_png`]) and hand
//!   that to the resolver. The layout engine never sees WebP/GIF — it just sizes a
//!   PNG of the right intrinsic dimensions.
//! - **PNG paint**: scale the RGBA into the fragment box ([`DecodedImage::scaled_pixmap`]).
//! - **SVG paint**: embed a base64 PNG `data:` URI ([`data_uri`]) — universally
//!   renderable regardless of the source format.
//!
//! Format support therefore lives entirely here (the `image` crate's codec list +
//! resvg for SVG), decoupled from the layout engine.

use std::collections::HashMap;
use std::io::Cursor;

use tiny_skia::{Pixmap, PremultipliedColorU8};

/// A decoded image in straight-alpha RGBA8 (row-major, 4 bytes/px), plus its
/// intrinsic pixel size. The canonical form every source format normalizes to.
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Decoded images keyed by their reference (the same key [`crate::image_urls`]
/// returns), decoded once and shared by the layout-sizing and paint passes.
pub type DecodedAssets = HashMap<String, DecodedImage>;

/// Decode every fetched image, dropping any that don't decode. Done once so the
/// bytes are parsed a single time for both sizing and painting.
pub fn decode_all(raw: &HashMap<String, Vec<u8>>) -> DecodedAssets {
    raw.iter()
        .filter_map(|(k, v)| Some((k.clone(), decode(v)?)))
        .collect()
}

/// The `ref -> PNG bytes` map handed to turbo-html2pdf's image resolver: every
/// decoded image re-encoded as PNG so the layout engine can size it regardless of
/// the original format. Images that fail to re-encode are omitted (they lay out
/// without a box, as an unresolved image would).
pub fn png_map(decoded: &DecodedAssets) -> HashMap<String, Vec<u8>> {
    decoded
        .iter()
        .filter_map(|(k, img)| Some((k.clone(), img.to_png()?)))
        .collect()
}

/// Decode PNG/JPEG/GIF/WebP (via the `image` crate) or SVG (via resvg) into
/// straight-alpha RGBA. `None` on an unrecognized or corrupt image.
pub fn decode(bytes: &[u8]) -> Option<DecodedImage> {
    if is_svg(bytes) {
        return decode_svg(bytes);
    }
    let rgba = image::load_from_memory(bytes).ok()?.to_rgba8();
    Some(DecodedImage {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

/// A light structural SVG sniff (SVG has no byte magic): the first non-space chunk
/// looks like an `<svg` or XML/doctype preamble leading to one.
fn is_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(1024)];
    let lower = head.to_ascii_lowercase();
    let contains = |needle: &[u8]| lower.windows(needle.len()).any(|w| w == needle);
    contains(b"<svg")
        && (lower.starts_with(b"<svg") || contains(b"<?xml") || contains(b"<!doctype"))
}

/// Rasterize SVG bytes at their intrinsic CSS-pixel size into straight RGBA. Uses
/// resvg's re-exported tiny-skia internally, converting to a plain `Vec<u8>` so
/// the raster's own tiny-skia version is never coupled to resvg's.
fn decode_svg(bytes: &[u8]) -> Option<DecodedImage> {
    use resvg::{tiny_skia as rsk, usvg};
    // Default options ship an empty fontdb (deterministic; shapes/paths/gradients
    // render, text is skipped) — matching turbo-html2pdf's svg tier.
    let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let (w, h) = (size.width().ceil() as u32, size.height().ceil() as u32);
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        return None;
    }
    let mut pm = rsk::Pixmap::new(w, h)?;
    resvg::render(&tree, rsk::Transform::identity(), &mut pm.as_mut());
    // Convert premultiplied → straight RGBA (`demultiply`), the canonical form.
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for px in pm.pixels() {
        let c = px.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Some(DecodedImage {
        width: w,
        height: h,
        rgba,
    })
}

impl DecodedImage {
    /// Re-encode as a PNG (for the layout resolver + SVG data-URIs). `None` on an
    /// encode failure or a degenerate size.
    pub fn to_png(&self) -> Option<Vec<u8>> {
        let img = image::RgbaImage::from_raw(self.width, self.height, self.rgba.clone())?;
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).ok()?;
        Some(out.into_inner())
    }

    /// Resize into a `w × h` premultiplied pixmap ready to `draw_pixmap` into the
    /// fragment box. `None` on a degenerate target size.
    pub fn scaled_pixmap(&self, w: u32, h: u32) -> Option<Pixmap> {
        if w == 0 || h == 0 || self.width == 0 || self.height == 0 {
            return None;
        }
        let src = image::RgbaImage::from_raw(self.width, self.height, self.rgba.clone())?;
        let scaled = image::imageops::resize(&src, w, h, image::imageops::FilterType::Triangle);
        let mut pm = Pixmap::new(w, h)?;
        for (src, dst) in scaled.pixels().zip(pm.pixels_mut()) {
            let [r, g, b, a] = src.0;
            // premul(c,a) ≤ a always, so the premultiplied invariant holds.
            *dst = PremultipliedColorU8::from_rgba(premul(r, a), premul(g, a), premul(b, a), a)?;
        }
        Some(pm)
    }
}

/// `round(c * a / 255)` — a straight channel scaled into premultiplied space.
fn premul(c: u8, a: u8) -> u8 {
    ((c as u16 * a as u16 + 127) / 255) as u8
}

/// A base64 PNG `data:` URI for embedding a decoded image in an SVG `<image href>`.
/// Always PNG (re-encoded from RGBA), so any SVG viewer renders it regardless of
/// the source format. `None` if the image won't re-encode.
pub fn data_uri(img: &DecodedImage) -> Option<String> {
    let png = img.to_png()?;
    Some(format!("data:image/png;base64,{}", base64(&png)))
}

/// Standard base64 (RFC 4648, `+/`, `=` padding). Hand-rolled to keep the crate
/// dependency-light; only the SVG data-URI path uses it.
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18 & 63) as usize] as char);
        out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{base64, decode};

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn decodes_a_simple_svg_to_rgba() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="6"><rect width="10" height="6" fill="#f00"/></svg>"##;
        let img = decode(svg).expect("svg decodes");
        assert_eq!((img.width, img.height), (10, 6));
        assert_eq!(img.rgba.len(), 10 * 6 * 4);
        // Top-left pixel is red, fully opaque.
        assert_eq!(&img.rgba[0..4], &[255, 0, 0, 255]);
    }
}
