//! Authored sandbox navigation controls run against the actual pinned engine.
#![cfg(turbo_surf_native_proxy)]
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn sandbox_top_navigation_intersects_attributes_and_csp_like_chrome() {
    assert_eq!(
        turbo_surf_render::v8_version(),
        env!("TURBO_SURF_EXPECTED_NATIVE_V8")
    );
    let cases = include_str!("../../../../harness/detection/sandbox-navigation-cases.json");
    let probe = include_str!("../../../../harness/detection/sandbox-navigation-probe.js");
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/sandbox-navigation-chrome155.json"
    ))
    .unwrap();
    for (name, bytes) in [
        ("casesSha256", cases.as_bytes()),
        ("probeSha256", probe.as_bytes()),
        (
            "collectorSha256",
            include_bytes!("../../../../harness/detection/sandbox-navigation-chrome.mjs")
                .as_slice(),
        ),
    ] {
        assert_eq!(control[name], format!("{:x}", Sha256::digest(bytes)));
    }
    assert_eq!(control["browser"], "Chrome/155.0.8059.80");
    assert_eq!(control["v8"], "15.5.35.23");
    assert_eq!(control["errors"], serde_json::json!([]));
    let cases: Value = serde_json::from_str(cases).unwrap();
    assert_eq!(
        cases.as_array().unwrap().len(),
        control["result"].as_array().unwrap().len()
    );
    for (case, expected) in cases
        .as_array()
        .unwrap()
        .iter()
        .zip(control["result"].as_array().unwrap())
    {
        assert_eq!(case["id"], expected["id"]);
        let headers: Vec<(String, String)> =
            serde_json::from_value(case["headers"].clone()).unwrap();
        let mut page = PageSession::open_with_response_headers(
            case["html"].as_str().unwrap(),
            "http://localhost/control",
            "",
            "",
            10000,
            "",
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let result = page
            .eval_json_expression(probe.trim().trim_end_matches(';'))
            .await
            .unwrap();
        let actual: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(actual, expected["result"], "{}", case["id"]);
        page.close();
    }
}
