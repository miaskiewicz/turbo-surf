use napi::{Error, Result};
use serde::Deserialize;
use turbo_surf_render::DocumentResponseMetadata;

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct DocumentResponse {
    #[serde(flatten)]
    pub metadata: DocumentResponseMetadata,
    pub headers: Vec<(String, String)>,
}
pub(crate) fn parse(value: Option<String>) -> Result<DocumentResponse> {
    value.map_or_else(
        || Ok(DocumentResponse::default()),
        |value| {
            serde_json::from_str(&value).map_err(|error| {
                Error::from_reason(format!("Invalid document response metadata: {error}"))
            })
        },
    )
}
