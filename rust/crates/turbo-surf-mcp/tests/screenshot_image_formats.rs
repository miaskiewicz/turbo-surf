//! turbo-surf#30: screenshots paint BMP, ICO/CUR (the largest entry, BMP- or PNG-encoded)
//! and — under the default `avif` feature, which this crate forwards to turbo-dom-raster —
//! AVIF images with Chrome 156's pixels, sampled at each image's centre and an inset
//! corner (`fixture-gen/screenshot-image-formats-chrome.mjs`; the page comes from
//! turbo-surf-render's `fixture-gen/make-image-format-cases.py`). The image bytes reach
//! the raster as the caller-fetched asset map, as the `screenshot` tool hands them over
//! (turbo-dom-raster paints an `<img src="data:…">` only from that map: turbo-dom#60).
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_dom_raster as raster;

const PAGE: &str = include_str!("fixtures/screenshot_image_formats.html");
const CONTROL: &str = include_str!("fixtures/screenshot_image_formats_chrome156.json");

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn screenshots_paint_bmp_ico_and_avif_like_chrome156() {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    assert!(control["browser"]
        .as_str()
        .unwrap()
        .starts_with("Chrome/156."));
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(control["pageSha256"], sha(PAGE.as_bytes()), "stale control");
    let viewport = raster::Viewport {
        width: control["viewport"][0].as_u64().unwrap() as u32,
        height: control["viewport"][1].as_u64().unwrap() as u32,
    };
    let assets: raster::ImageAssets = PAGE
        .split(" src=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .map(|src| {
            let bytes = turbo_dom_layout::image::decode_data_url(src).unwrap().bytes;
            (src.to_string(), bytes)
        })
        .collect();
    assert_eq!(assets.len(), 6);
    let png = raster::screenshot_png_with_assets(PAGE, "", viewport, &assets, false).unwrap();
    let shot = turbo_dom_layout::image::decode_png_rgba(&png).unwrap();
    assert_eq!((shot.width, shot.height), (viewport.width, viewport.height));
    let mut mismatches = Vec::new();
    for sample in control["samples"].as_array().unwrap() {
        let id = sample["id"].as_str().unwrap();
        let (x, y) = (sample["x"].as_u64().unwrap(), sample["y"].as_u64().unwrap());
        let at = ((y * u64::from(shot.width) + x) * 4) as usize;
        let got = &shot.rgba[at..at + 4];
        let chrome: Vec<u8> = sample["rgba"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u8)
            .collect();
        // AVIF is lossy: Chrome's and rav1d's YUV->RGB conversions may round apart by a step.
        let avif = id.starts_with("avif");
        let tolerance = if avif { 2 } else { 0 };
        let close = got
            .iter()
            .zip(&chrome)
            .all(|(a, b)| a.abs_diff(*b) <= tolerance);
        match (avif && !cfg!(feature = "avif"), close) {
            // Without the decoder an AVIF paints as the placeholder, never Chrome's pixels.
            (true, true) => mismatches.push(format!("{id} ({x},{y}): painted without `avif`")),
            (false, false) => {
                mismatches.push(format!("{id} ({x},{y}): expected {chrome:?}, got {got:?}"))
            }
            _ => {}
        }
    }
    assert!(mismatches.is_empty(), "{mismatches:#?}");
}
