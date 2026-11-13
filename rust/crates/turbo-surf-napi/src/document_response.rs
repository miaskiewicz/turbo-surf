use napi::{Error, Result};
use serde::Deserialize;
use turbo_surf_render::{AuxiliaryResourcePolicy, DocumentResponseMetadata};

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct DocumentResponse {
    #[serde(flatten)]
    pub metadata: DocumentResponseMetadata,
    pub headers: Vec<(String, String)>,
    pub viewport: Option<[u32; 2]>,
    #[serde(rename = "hostNetworkLog")]
    pub host_network_log: Option<u32>,
    #[serde(rename = "auxiliaryInitScript")]
    pub auxiliary_init_script: String,
    #[serde(rename = "auxiliaryEngineInitScript")]
    pub auxiliary_engine_init_script: String,
    #[serde(rename = "pageInitScript")]
    pub page_init_script: String,
    #[serde(rename = "pageEngineInitScript")]
    pub page_engine_init_script: String,
    #[serde(rename = "auxiliaryPolicy")]
    pub auxiliary_policy: Option<AuxiliaryResourcePolicy>,
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
