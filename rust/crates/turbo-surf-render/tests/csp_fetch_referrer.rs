use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

#[path = "csp_navigation_metadata.rs"]
mod navigation;

const CASES: &str =
    include_str!("../../../../harness/detection/fixtures/csp-fetch-referrer-cases.json");
const CONTROL: &str =
    include_str!("../../../../harness/detection/fixtures/csp-fetch-referrer-chrome155.json");
const PROBE: &str = include_str!("../../../../harness/detection/csp-fetch-referrer-probe.js");

#[tokio::test(flavor = "current_thread")]
async fn initiating_fetch_referrer_policy_and_parser_phases_match_chrome() {
    let cases: Value = serde_json::from_str(CASES).unwrap();
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    for (name, bytes) in [
        ("casesSha256", CASES.as_bytes()),
        ("probeSha256", PROBE.as_bytes()),
        (
            "collectorSha256",
            include_bytes!("../../../../harness/detection/csp-fetch-referrer-chrome.mjs")
                .as_slice(),
        ),
    ] {
        assert_eq!(
            control[name],
            format!("{:x}", Sha256::digest(bytes)),
            "stale control: {name}"
        );
    }
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(control["browser"], "Chrome/155.0.8059.79");
    assert_eq!(control["v8"], "15.5.35.23");
    assert_eq!(cases.as_array().unwrap().len(), 12);
    assert_eq!(control["result"].as_array().unwrap().len(), 12);
    let server = navigation::FrameServer::start_cases(CASES);
    for (index, case) in cases.as_array().unwrap().iter().enumerate() {
        let headers = case["headers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row[0].as_str().unwrap().to_string(),
                    row[1].as_str().unwrap().to_string(),
                )
            })
            .collect::<Vec<_>>();
        let mut page = PageSession::open_with_response_headers(
            case["html"].as_str().unwrap(),
            &format!("{}?case={index}", server.url()),
            "",
            "",
            15000,
            "",
            Default::default(),
            &headers,
        )
        .await
        .unwrap();
        let actual: Value = serde_json::from_str(
            &page
                .eval_json_expression(PROBE.trim().trim_end_matches(';'))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(actual, control["result"][index]["result"], "{}", case["id"]);
        page.close();
    }
}
