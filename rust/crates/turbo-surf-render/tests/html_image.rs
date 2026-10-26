//! turbo-surf#32: HTMLImageElement load state through surf's real image loader.
//! The binding owns the prototype accessors (complete/naturalWidth/naturalHeight/
//! currentSrc) and its Chrome 156 control; surf publishes each load through the
//! document's private `imageHost` record and keeps `src`/`decode()` on the prototype,
//! so no image ever carries own properties.
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_dom_browser_env::test_support;
use turbo_surf_render::PageSession;

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Run `probe` on every case page and require the Chrome control's result exactly.
async fn matches_control(probe: &str, cases: &[u8], control: &str, base: fn(&Value) -> &str) {
    let control: Value = serde_json::from_str(control).unwrap();
    assert!(control["browser"]
        .as_str()
        .unwrap()
        .starts_with("Chrome/156."));
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(
        control["probeSha256"],
        sha(probe.as_bytes()),
        "stale probe control"
    );
    assert_eq!(control["casesSha256"], sha(cases), "stale cases control");
    let cases: Value = serde_json::from_slice(cases).unwrap();
    let cases = cases.as_array().unwrap();
    let expected = control["result"].as_array().unwrap();
    assert_eq!(cases.len(), expected.len());
    for (case, expected) in cases.iter().zip(expected) {
        assert_eq!(case["id"], expected["id"]);
        let mut page = PageSession::open(case["html"].as_str().unwrap(), base(case), "", "", 10000)
            .await
            .unwrap();
        let actual: Value = serde_json::from_str(
            &page
                .eval_json_expression(probe.trim_end().trim_end_matches(';'))
                .await
                .unwrap(),
        )
        .unwrap();
        let expected = expected["result"].as_object().unwrap();
        let mismatches: Vec<_> = expected
            .iter()
            .filter(|(name, value)| actual.get(name.as_str()) != Some(value))
            .map(|(name, value)| format!("{name}: expected {value}, got {}", actual[name.as_str()]))
            .collect();
        assert!(mismatches.is_empty(), "{}: {mismatches:#?}", case["id"]);
        assert_eq!(actual.as_object().unwrap().len(), expected.len());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn image_state_through_the_surf_loader_matches_chrome156() {
    matches_control(
        test_support::HTML_IMAGE_PROBE_JS,
        test_support::HTML_IMAGE_CASES_JSON,
        test_support::HTML_IMAGE_CHROME156_JSON,
        |case| case["documentBase"].as_str().unwrap(),
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn src_and_decode_are_prototype_members_that_load_like_chrome156() {
    matches_control(
        include_str!("fixtures/html_image_members_probe.js"),
        include_bytes!("fixtures/html_image_members_cases.json"),
        include_str!("fixtures/html_image_members_chrome156.json"),
        |case| case["url"].as_str().unwrap(),
    )
    .await;
}
