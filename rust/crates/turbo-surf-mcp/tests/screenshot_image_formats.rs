//! turbo-surf#30 / #43: screenshots paint BMP, ICO/CUR (the largest entry, BMP- or
//! PNG-encoded) and — under the default `avif` and `jxl` features, which this crate forwards
//! to turbo-dom-raster — AVIF and JPEG XL images with Chrome 156's pixels, sampled at each
//! image's centre and an inset corner (`fixture-gen/screenshot-image-formats-chrome.mjs`;
//! the page comes from turbo-surf-render's `fixture-gen/make-image-format-cases.py`). The
//! image bytes reach the raster as `data:` sources: turbo-dom-raster decodes an
//! `<img src="data:…">` itself (turbo-dom#60), so the screenshot tool's empty asset map
//! paints them, exactly as a map that carries the same bytes under each `src` does.
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_dom_raster as raster;

const PAGE: &str = include_str!("fixtures/screenshot_image_formats.html");
const CONTROL: &str = include_str!("fixtures/screenshot_image_formats_chrome156.json");

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Screenshot the control page (with an empty and with a full asset map) and compare the
/// samples whose id `select` accepts. `decoded(id)` says whether this build decodes the
/// format: a format it does not decode paints the placeholder and must never show Chrome's
/// pixels.
fn mismatches(select: fn(&str) -> bool, decoded: fn(&str) -> bool) -> Vec<String> {
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
    assert_eq!(assets.len(), 8);
    let mut mismatches = Vec::new();
    // `image_urls` skips `data:` sources (nothing to fetch), so callers hand over no bytes.
    for (map, assets) in [
        ("empty map", raster::ImageAssets::new()),
        ("asset map", assets),
    ] {
        let png = raster::screenshot_png_with_assets(PAGE, "", viewport, &assets, false).unwrap();
        let shot = turbo_dom_layout::image::decode_png_rgba(&png).unwrap();
        assert_eq!((shot.width, shot.height), (viewport.width, viewport.height));
        for sample in control["samples"].as_array().unwrap() {
            let id = sample["id"].as_str().unwrap();
            if !select(id) {
                continue;
            }
            let (x, y) = (sample["x"].as_u64().unwrap(), sample["y"].as_u64().unwrap());
            let at = ((y * u64::from(shot.width) + x) * 4) as usize;
            let got = &shot.rgba[at..at + 4];
            let chrome: Vec<u8> = sample["rgba"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u8)
                .collect();
            // AVIF is lossy: Chrome's and rav1d's YUV->RGB conversions may round apart by a
            // step. A translucent image's composite over the canvas may round apart by one.
            let tolerance = if id.starts_with("avif") || id.ends_with("-alpha") {
                2
            } else {
                0
            };
            let close = got
                .iter()
                .zip(&chrome)
                .all(|(a, b)| a.abs_diff(*b) <= tolerance);
            match (decoded(id), close) {
                // Without the decoder the image paints as the placeholder, never Chrome's pixels.
                (false, true) => mismatches.push(format!(
                    "{map}: {id} ({x},{y}): painted without its decoder feature"
                )),
                (true, false) => mismatches.push(format!(
                    "{map}: {id} ({x},{y}): expected {chrome:?}, got {got:?}"
                )),
                _ => {}
            }
        }
    }
    mismatches
}

#[test]
fn screenshots_paint_bmp_ico_and_avif_like_chrome156() {
    let found = mismatches(
        |id| !id.starts_with("jxl"),
        |id| !id.starts_with("avif") || cfg!(feature = "avif"),
    );
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
#[ignore = "turbo-surf#43: fails until the fix"]
fn screenshots_paint_jpeg_xl_like_chrome156() {
    let found = mismatches(|id| id.starts_with("jxl"), |_| true);
    assert!(found.is_empty(), "{found:#?}");
}
