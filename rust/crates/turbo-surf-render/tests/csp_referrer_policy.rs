#[path = "csp_navigation_metadata.rs"]
mod navigation;

use sha2::{Digest, Sha256};
use turbo_surf_core::request_context::ClientHints;
use turbo_surf_render::PageSession;

const CASES: &str =
    include_str!("../../../../harness/detection/fixtures/csp-referrer-meta-cases.json");
const CONTROL: &str =
    include_str!("../../../../harness/detection/fixtures/csp-referrer-meta-chrome155.json");

#[tokio::test(flavor = "current_thread")]
async fn actual_meta_referrer_lifecycle_matches_source_pinned_chrome() {
    let cases: serde_json::Value = serde_json::from_str(CASES).unwrap();
    let control: serde_json::Value = serde_json::from_str(CONTROL).unwrap();
    for (key, bytes) in [
        ("casesSha256", CASES.as_bytes()),
        (
            "collectorSha256",
            include_bytes!("../../../../harness/detection/csp-referrer-meta-chrome.mjs").as_slice(),
        ),
        (
            "probeSha256",
            include_bytes!("../../../../harness/detection/csp-navigation-probe.js").as_slice(),
        ),
    ] {
        assert_eq!(
            control[key],
            format!("{:x}", Sha256::digest(bytes)),
            "{key}"
        );
    }
    assert_eq!(control["errors"], serde_json::json!([]));
    let server = navigation::FrameServer::start_cases(CASES);
    let base = server.url();
    let cross = base.replace("127.0.0.1", "localhost");
    for (index, case) in cases.as_array().unwrap().iter().enumerate() {
        let owner = format!("{base}start?case={index}&private=authored#fragment");
        let policy = case["policy"].as_str().unwrap();
        let html = case["html"]
            .as_str()
            .unwrap_or("<!doctype html><head></head><body></body>");
        let mut page = PageSession::open_with_response_headers(
            html,
            &owner,
            "",
            "",
            15000,
            "",
            ClientHints::default(),
            &[
                (
                    "Content-Security-Policy".into(),
                    "style-src \x27none\x27".into(),
                ),
                ("Referrer-Policy".into(), policy.into()),
            ],
        )
        .await
        .unwrap();
        let child = format!(
            "{}page?case={index}",
            if case["child"] == "cross" {
                &cross
            } else {
                &base
            }
        );
        let steps = case["steps"].as_str().unwrap_or("");
        let target = serde_json::to_string(&child).unwrap();
        let script=format!("new Promise(resolve=>{{{steps};addEventListener(\x27message\x27,event=>{{if(typeof event.data===\x27string\x27)resolve(JSON.parse(event.data))}},{{once:true}});const frame=document.createElement(\x27iframe\x27);frame.src={target};document.body.append(frame)}})");
        let result = page
            .eval_json_expression(&script)
            .await
            .unwrap_or_else(|error| panic!("{}: {error}", case["id"]))
            .replace(&base, "http://metadata.test/")
            .replace(&cross, "http://cross.metadata.test/");
        let result: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(result, control["result"][index]["result"], "{}", case["id"]);
        page.close();
    }
}
