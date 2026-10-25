use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const PROBE: &str = include_str!("../../../../harness/detection/browser-release-probe.js");

async fn release_probe() -> Value {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://release.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let actual = serde_json::from_str(
        &page
            .eval_json_expression(PROBE.trim().trim_end_matches(';'))
            .await
            .unwrap(),
    )
    .unwrap();
    page.close();
    actual
}

// The browser-release probe's view of a measured identity-battery fixture.
fn expected_from_identity(major: u16) -> Value {
    let path = format!(
        "{}/../../../harness/detection/fixtures/chrome-identity-{major}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let data = &fixture["js"]["userAgentData"];
    json!({
        "userAgent": fixture["js"]["navigator"]["userAgent"],
        "brands": data["brands"],
        "high": {
            "brands": data["high"]["brands"],
            "fullVersionList": data["high"]["fullVersionList"],
            "mobile": data["high"]["mobile"],
            "platform": data["high"]["platform"],
            "uaFullVersion": data["high"]["uaFullVersion"],
        },
    })
}

// One test: set_fingerprint is process-global, so the default and the explicit
// older-release override must not race each other.
#[tokio::test(flavor = "current_thread")]
async fn release_high_entropy_identity_agrees_with_chrome_and_wire_profile() {
    turbo_surf_render::set_fingerprint("{}");
    let actual = release_probe().await;
    assert_eq!(
        actual,
        expected_from_identity(turbo_surf_core::browser::DEFAULT_MAJOR)
    );
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

    // Older releases stay selectable: a pinned Chrome 155 identity reproduces the
    // independent Chrome 155 control exactly.
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/browser-release-chrome155.json"
    ))
    .unwrap();
    assert_eq!(control["browser"], "Chrome/155.0.8059.80");
    assert_eq!(control["v8"], "15.5.35.23");
    assert_eq!(control["errors"], json!([]));
    assert_eq!(
        control["probeSha256"],
        format!("{:x}", Sha256::digest(PROBE))
    );
    let old = turbo_surf_core::browser::release(155);
    turbo_surf_render::set_fingerprint(
        &json!({
            "userAgent": control["result"]["userAgent"],
            "chromeMajor": old.major,
            "fullVersion": old.full_version,
        })
        .to_string(),
    );
    let pinned = release_probe().await;
    turbo_surf_render::set_fingerprint("{}");
    assert_eq!(pinned, control["result"]);
}
