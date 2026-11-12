use crate::{to_assets, viewport};
use napi::bindgen_prelude::*;
use napi_derive::napi;
use serde::Deserialize;
use std::collections::HashMap;
use turbo_dom_raster::{ImageAssets, ImageMetadata, ScreenshotOptions, Viewport};

#[derive(Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct CaptureOptions {
    pub full_page: bool,
    pub system_fonts: bool,
    pub device_scale: f64,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            full_page: false,
            system_fonts: false,
            device_scale: 1.0,
        }
    }
}

impl CaptureOptions {
    pub(crate) fn from_json(json: &str) -> Result<Self> {
        let options: Self = serde_json::from_str(json)
            .map_err(|error| Error::from_reason(format!("Invalid screenshot options: {error}")))?;
        if !options.device_scale.is_finite() || options.device_scale <= 0.0 {
            return Err(Error::from_reason(
                "screenshot device scale must be finite and positive",
            ));
        }
        Ok(options)
    }

    pub(crate) fn base_options<'a>(
        &self,
        metadata: Option<&'a ImageMetadata>,
    ) -> ScreenshotOptions<'a> {
        let mut options = ScreenshotOptions::default();
        options.full_page = self.full_page;
        options.system_fonts = self.system_fonts;
        options.device_scale = self.device_scale;
        options.image_metadata = metadata;
        options
    }

    pub(crate) fn with_options<T>(
        &self,
        _html: &str,
        _css: &str,
        _viewport: Viewport,
        _assets: &ImageAssets,
        metadata: Option<&ImageMetadata>,
        capture: impl FnOnce(&ScreenshotOptions<'_>) -> std::result::Result<T, String>,
    ) -> Result<T> {
        capture(&self.base_options(metadata)).map_err(Error::from_reason)
    }
}

#[napi]
#[allow(clippy::too_many_arguments)]
pub fn screenshot_capture(
    html: String,
    external_css: String,
    images: HashMap<String, Buffer>,
    width: Option<u32>,
    height: Option<u32>,
    options_json: Option<String>,
    metadata_json: Option<String>,
) -> Result<Buffer> {
    let config = CaptureOptions::from_json(options_json.as_deref().unwrap_or("{}"))?;
    let metadata = crate::image_metadata::parse_metadata(metadata_json.as_deref().unwrap_or("{}"))?;
    let assets = to_assets(images);
    let viewport = viewport(width, height);
    config
        .with_options(
            &html,
            &external_css,
            viewport,
            &assets,
            Some(&metadata),
            |options| {
                turbo_dom_raster::screenshot_png_with_options(
                    &html,
                    &external_css,
                    viewport,
                    &assets,
                    options,
                )
            },
        )
        .map(Buffer::from)
}

#[napi]
#[allow(clippy::too_many_arguments)]
pub fn screenshot_capture_svg(
    html: String,
    external_css: String,
    images: HashMap<String, Buffer>,
    width: Option<u32>,
    height: Option<u32>,
    options_json: Option<String>,
    metadata_json: Option<String>,
) -> Result<String> {
    let config = CaptureOptions::from_json(options_json.as_deref().unwrap_or("{}"))?;
    let metadata = crate::image_metadata::parse_metadata(metadata_json.as_deref().unwrap_or("{}"))?;
    let assets = to_assets(images);
    let viewport = viewport(width, height);
    config.with_options(
        &html,
        &external_css,
        viewport,
        &assets,
        Some(&metadata),
        |options| {
            turbo_dom_raster::screenshot_svg_with_options(
                &html,
                &external_css,
                viewport,
                &assets,
                options,
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_validation_and_metadata_keep_native_capture_inputs() {
        assert!(CaptureOptions::from_json(r#"{"deviceScale":0}"#).is_err());
        assert!(CaptureOptions::from_json(r#"{"deviceScale":-1}"#).is_err());
        assert!(CaptureOptions::from_json("invalid").is_err());
        let config = CaptureOptions::from_json(r#"{"deviceScale":1.25,"fullPage":true}"#).unwrap();
        let metadata =
            crate::image_metadata::parse_metadata(r#"{"missing":{"width":20,"height":140}}"#)
                .unwrap();
        let assets = ImageAssets::new();
        let viewport = Viewport {
            width: 100,
            height: 80,
        };
        let html = "<style>html,body{margin:0}img{display:block}</style><img src=missing>";
        let png = config
            .with_options(html, "", viewport, &assets, Some(&metadata), |options| {
                turbo_dom_raster::screenshot_png_with_options(html, "", viewport, &assets, options)
            })
            .unwrap();
        let decoded = image::load_from_memory(&png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (125, 175));
    }
}
