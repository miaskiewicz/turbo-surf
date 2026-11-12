use napi::{Error, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use turbo_dom_layout::image::Intrinsic;
use turbo_dom_raster::ImageMetadata;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MetadataRecord {
    width: u32,
    height: u32,
    natural_ratio: Option<f32>,
}

pub(crate) fn parse_metadata(json: &str) -> Result<ImageMetadata> {
    let records: BTreeMap<String, MetadataRecord> = serde_json::from_str(json)
        .map_err(|error| Error::from_reason(format!("Invalid image metadata: {error}")))?;
    Ok(records
        .into_iter()
        .filter_map(|(url, size)| {
            if size.width == 0 || size.height == 0 {
                return None;
            }
            let intrinsic = match size
                .natural_ratio
                .filter(|ratio| ratio.is_finite() && *ratio > 0.0)
            {
                Some(ratio) => Intrinsic::ratio_only(size.width, size.height, ratio, true),
                None => Intrinsic::new(size.width, size.height, true),
            };
            Some((url, intrinsic))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_validation_preserves_ratio_and_skips_unusable_sizes() {
        let metadata = parse_metadata(r#"{"ratio":{"width":3,"height":2,"naturalRatio":1.5},"unknown":{"width":0,"height":0}}"#).unwrap();
        assert_eq!(metadata["ratio"].natural_ratio, Some(1.5));
        assert!(!metadata.contains_key("unknown"));
        assert!(parse_metadata("[]").is_err());
    }
}
