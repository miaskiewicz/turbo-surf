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
/// Known gaps owned by turbo-dom, as (case, JSON pointer into its result, surf's current
/// value): each is required to still differ from Chrome, so a fix fails its pin and the
/// gap is retired with it.
const GAPS: &[(&str, &str, &str)] = &[
    // Live layout keeps a percentage width unsnapped (261.3067px); Blink's LayoutUnit
    // floors it to 1/64 px (261.296875).
    ("used-size", "/percent/0", r#""261.307px""#),
    // The binding resolves an `em` translate against the element's declared font-size
    // only (16px when inherited), not the layoutUsedSize hook's font size.
    ("used-size", "/transforms/1", r#""matrix(1, 0, 0, 1, 32, 0)""#),
    // The binding serializes an authored named color as written.
    ("css-wide-and-button", "/1", r#""red""#),
    ("css-wide-and-button", "/2", r#""red""#),
];
/// CSS.supports answers from surf's Lightning CSS grammar, which rejects the 18 Chrome 156
/// features (every form). The engine's grammar (`cssSupports` / `cssSupportsProperty`,
/// turbo-dom#322) accepts them, but coerces its arguments with the page's `String`, which
/// a page may replace (turbo-dom: private coercion), so CSS.supports cannot route there yet.
const UNSUPPORTED_FEATURES: usize = 18;

fn gaps() -> Vec<(&'static str, String, String)> {
    let mut gaps: Vec<_> = GAPS
        .iter()
        .map(|(id, pointer, value)| (*id, pointer.to_string(), value.to_string()))
        .collect();
    for row in 0..UNSUPPORTED_FEATURES {
        for form in 1..=3 {
            gaps.push(("css-supports", format!("/{row}/{form}"), "false".into()));
        }
    }
    gaps
}
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
        let mut native = run_case(case).await;
        for (_, pointer, pinned) in gaps().iter().filter(|gap| gap.0 == id) {
            let pinned: Value = serde_json::from_str(pinned).unwrap();
            let slot = native.pointer_mut(pointer).unwrap();
            assert_eq!(*slot, pinned, "{id}{pointer}: known gap changed");
            let chrome = chrome["result"].pointer(pointer).unwrap();
            assert_ne!(*chrome, pinned, "{id}{pointer}: known gap is fixed");
            *slot = chrome.clone();
        }
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
