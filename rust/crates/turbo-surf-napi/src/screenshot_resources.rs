use crate::image_metadata::parse_metadata;
use napi::{Error, Result};
use napi_derive::napi;
use turbo_dom_raster::capture_resources;

/// Plan pixel body fetches independently from already-known intrinsic headers.
#[napi]
pub fn screenshot_capture_resources(
    html: String,
    css: String,
    width: u32,
    height: u32,
    metadata_json: String,
    options_json: String,
) -> Result<String> {
    let metadata = parse_metadata(&metadata_json)?;
    let options = crate::screenshot_options::CaptureOptions::from_json(&options_json)?;
    let plan = capture_resources(
        &html,
        &css,
        crate::viewport(Some(width), Some(height)),
        &metadata,
        &options.base_options(Some(&metadata)),
    );
    serde_json::to_string(&serde_json::json!({
        "pixelUrls": plan.pixel_urls, "fragmentsExamined": plan.fragments_examined,
    }))
    .map_err(|error| Error::from_reason(error.to_string()))
}
