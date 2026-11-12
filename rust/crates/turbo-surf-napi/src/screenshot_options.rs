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
    mask_indices: Vec<usize>,
    mask_color: String,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            full_page: false,
            system_fonts: false,
            device_scale: 1.0,
            mask_indices: Vec::new(),
            mask_color: "#ff00ff".into(),
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
        html: &str,
        css: &str,
        viewport: Viewport,
        assets: &ImageAssets,
        metadata: Option<&ImageMetadata>,
        capture: impl FnOnce(&ScreenshotOptions<'_>) -> std::result::Result<T, String>,
    ) -> Result<T> {
        let mut options = self.base_options(metadata);
        let masks = if self.mask_indices.is_empty() {
            Vec::new()
        } else {
            let color = turbo_dom_parser::rtdom::color::parse_color(&self.mask_color)
                .ok_or_else(|| Error::from_reason("maskColor must be a supported CSS color"))?;
            let color = [
                color.r.round() as u8,
                color.g.round() as u8,
                color.b.round() as u8,
                (color.a * 255.0).round() as u8,
            ];
            turbo_dom_raster::screenshot_element_rects(
                html,
                css,
                viewport,
                assets,
                &options,
                &self.mask_indices,
            )
            .map_err(Error::from_reason)?
            .into_iter()
            .flatten()
            .map(|rect| turbo_dom_raster::ScreenshotMask { rect, color })
            .collect::<Vec<_>>()
        };
        options.masks = &masks;
        capture(&options).map_err(Error::from_reason)
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

    #[test]
    fn mask_geometry_uses_snapshot_indices_clipping_and_intrinsic_metadata() {
        let html = "<style>html,body{margin:0}#clip{position:absolute;left:10px;top:10px;width:10px;height:10px;overflow:hidden}#secret{width:30px;height:30px;background:red}img{display:block}</style><svg><rect width='1' height='1'/></svg><div id='clip'><div id='secret'></div></div><img src='missing'>";
        let tree = turbo_dom_parser::rtdom::Tree::parse(html);
        let index = tree
            .query_selector_all("*")
            .iter()
            .position(|node| tree.get_attribute(*node, "id") == Some("secret"))
            .unwrap();
        let config = CaptureOptions::from_json(
            &format!(r##"{{"maskIndices":[{index}],"maskColor":"#00ff00"}}"##),
        )
        .unwrap();
        let metadata =
            crate::image_metadata::parse_metadata(r#"{"missing":{"width":20,"height":140}}"#)
                .unwrap();
        config
            .with_options(
                html,
                "",
                Viewport {
                    width: 100,
                    height: 80,
                },
                &ImageAssets::new(),
                Some(&metadata),
                |options| {
                    assert_eq!(options.masks.len(), 1);
                    let mask = &options.masks[0];
                    assert_eq!(
                        [mask.rect.x, mask.rect.y, mask.rect.width, mask.rect.height],
                        [10.0, 10.0, 10.0, 10.0]
                    );
                    assert_eq!(mask.color, [0, 255, 0, 255]);
                    assert!(options.image_metadata.unwrap().contains_key("missing"));
                    Ok(())
                },
            )
            .unwrap();
    }
}
