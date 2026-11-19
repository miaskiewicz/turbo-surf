//! A screenshot's intrinsic sizes come from the native resource loader, not guest getters.
use deno_core::{op2, v8};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImageMetadataRecord {
    width: u32,
    height: u32,
    natural_ratio: Option<f32>,
}

#[op2]
#[serde]
pub(crate) fn op_screenshot_image_metadata(
    scope: &mut v8::PinScope,
    document: v8::Local<v8::Object>,
    reset: bool,
) -> BTreeMap<String, ImageMetadataRecord> {
    if reset {
        crate::live_layout_backend::clear_document_images(scope, document);
    }
    crate::live_layout_backend::screenshot_image_metadata(scope, document)
        .iter()
        .map(|(url, size)| {
            (
                url.clone(),
                ImageMetadataRecord {
                    width: size.width,
                    height: size.height,
                    natural_ratio: size.natural_ratio,
                },
            )
        })
        .collect()
}
