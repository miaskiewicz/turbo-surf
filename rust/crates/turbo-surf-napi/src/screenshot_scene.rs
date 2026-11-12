use crate::screenshot_options::CaptureOptions;
use napi::bindgen_prelude::*;
use napi_derive::napi;
use std::{cell::Cell, collections::HashMap};
use turbo_dom_raster::PreparedScreenshot;

/// A JS-owned immutable raster snapshot; closing or collecting it releases layout and pixels.
#[napi]
pub struct ScreenshotScene {
    prepared: Option<PreparedScreenshot>,
    paints: Cell<u32>,
}

impl ScreenshotScene {
    fn paint<T>(
        &self,
        paint: impl FnOnce(&PreparedScreenshot) -> std::result::Result<T, String>,
    ) -> Result<T> {
        let prepared = self
            .prepared
            .as_ref()
            .ok_or_else(|| Error::from_reason("screenshot scene is closed"))?;
        let output = paint(prepared).map_err(Error::from_reason)?;
        self.paints.set(self.paints.get().saturating_add(1));
        Ok(output)
    }
}

#[napi]
impl ScreenshotScene {
    #[napi(constructor)]
    pub fn new(
        html: String,
        css: String,
        images: HashMap<String, Buffer>,
        width: u32,
        height: u32,
        options_json: Option<String>,
        metadata_json: Option<String>,
    ) -> Result<Self> {
        let options = CaptureOptions::from_json(options_json.as_deref().unwrap_or("{}"))?;
        let metadata =
            crate::image_metadata::parse_metadata(metadata_json.as_deref().unwrap_or("{}"))?;
        let assets = crate::to_assets(images);
        let viewport = crate::viewport(Some(width), Some(height));
        let prepared =
            options.with_options(&html, &css, viewport, &assets, Some(&metadata), |options| {
                Ok(PreparedScreenshot::new(
                    &html, &css, viewport, &assets, options,
                ))
            })?;
        Ok(Self {
            prepared: Some(prepared),
            paints: Cell::new(0),
        })
    }
    #[napi]
    pub fn png(&self) -> Result<Buffer> {
        self.paint(PreparedScreenshot::png).map(Buffer::from)
    }
    #[napi]
    pub fn svg(&self) -> Result<String> {
        self.paint(PreparedScreenshot::svg)
    }
    #[napi]
    pub fn close(&mut self) {
        self.prepared = None;
    }
    #[napi(getter)]
    pub fn closed(&self) -> bool {
        self.prepared.is_none()
    }
    #[napi(getter)]
    pub fn paint_count(&self) -> u32 {
        self.paints.get()
    }
}
