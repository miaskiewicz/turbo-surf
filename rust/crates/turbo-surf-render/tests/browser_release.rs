use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn current_release_high_entropy_identity_agrees_with_chrome_and_wire_profile() {
    let source = include_str!("../../../../harness/detection/browser-release-probe.js");
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/browser-release-chrome155.json"
    ))
    .unwrap();
    assert_eq!(control["browser"], "Chrome/155.0.8059.80");
    assert_eq!(control["v8"], "15.5.35.23");
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(
        control["probeSha256"],
        format!("{:x}", Sha256::digest(source))
    );
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://release.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let actual: Value = serde_json::from_str(
        &page
            .eval_json_expression(source.trim().trim_end_matches(';'))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(actual, control["result"]);
    let profile = turbo_surf_core::fingerprint::default_profile();
    assert_eq!(actual["userAgent"], profile.user_agent);
    assert_eq!(actual["high"]["uaFullVersion"], profile.full_version);
    let brands = actual["high"]["fullVersionList"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            format!(
                "\"{}\";v=\"{}\"",
                entry["brand"].as_str().unwrap(),
                entry["version"].as_str().unwrap()
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    assert_eq!(
        brands,
        turbo_surf_core::fingerprint::brand_header(
            profile.chrome_major,
            profile.full_version,
            true
        )
    );
}
