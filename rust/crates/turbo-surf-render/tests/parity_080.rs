//! Chrome 156 parity rows from the 0.7.1 parity run (tests/fixtures/parity_080_*;
//! collector tests/fixture-gen/parity-080-chrome.mjs): the default line box at device
//! scale 1 and 2 (turbo-surf#158), getComputedStyle's used width / height and resolved
//! `em` / percentage translates (#156), a viewBox-only SVG `<img>` (#161), CSS-wide
//! keywords and the `<button>` UA background (#159, #160), a hit in the border-spacing gap
//! (#162) and CSS.supports for Chrome 156 features (#163).
//!
//! Its own binary: the viewport and devicePixelRatio come from the process-wide
//! fingerprint.
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const CASES: &[u8] = include_bytes!("fixtures/parity_080_cases.json");
const CONTROL: &str = include_str!("fixtures/parity_080_chrome156.json");

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

async fn run_case(case: &Value) -> Value {
    turbo_surf_render::set_fingerprint(
        &serde_json::json!({"innerWidth": 800, "innerHeight": 600, "outerWidth": 800,
            "outerHeight": 600, "devicePixelRatio": case["dpr"]})
        .to_string(),
    );
    let mut page = PageSession::open(
        case["html"].as_str().unwrap(),
        "http://127.0.0.1/",
        "",
        "",
        60_000,
    )
    .await
    .unwrap();
    let probe = case["probe"].as_str().unwrap();
    let value = serde_json::from_str(&page.eval_json_expression(probe).await.unwrap()).unwrap();
    page.close();
    value
}

#[tokio::test(flavor = "current_thread")]
async fn parity_rows_match_chrome156() {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    assert!(control["browser"]
        .as_str()
        .unwrap()
        .starts_with("Chrome/156."));
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(control["casesSha256"], sha(CASES));
    let cases: Value = serde_json::from_slice(CASES).unwrap();
    let cases = cases.as_array().unwrap();
    let expected = control["result"].as_array().unwrap();
    assert_eq!(cases.len(), expected.len());
    let mut failures = Vec::new();
    for (case, chrome) in cases.iter().zip(expected) {
        let id = case["id"].as_str().unwrap();
        assert_eq!(chrome["id"], id);
        let native = run_case(case).await;
        if native != chrome["result"] {
            failures.push(format!(
                "{id}\n  native: {native}\n  chrome: {}",
                chrome["result"]
            ));
        }
    }
    turbo_surf_render::set_fingerprint("{}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
