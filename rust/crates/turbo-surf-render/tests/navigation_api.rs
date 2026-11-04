//! window.navigation tracks same-document session history (entries, navigate(),
//! intercept(), traversal, events) against the Chrome 156 control (#108, #143).
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const PROBE: &str = include_str!("../../../../harness/detection/navigation-api-probe.js");
const CONTROL: &str =
    include_str!("../../../../harness/detection/fixtures/navigation-api-chrome156.json");

/// Measured Chrome behavior the engine does not reproduce, as `path = engine value`.
///
/// The recorder's tab starts on about:blank, so Chrome's `history.length` counts
/// one more (cross-document) entry than this document's own session history.
/// Blink reports an invalid URL in its parsed-and-reserialized form
/// (`http://[/`); the engine echoes the input string.
const KNOWN_GAPS: &[&str] = &[
    "/afterFragment/1/3 = 2",
    "/badURL/1/0/2 = \"Invalid URL 'http://['.\"",
    "/badURL/1/1/2 = \"Invalid URL 'http://['.\"",
];

fn control() -> Value {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    assert!(control["browser"]
        .as_str()
        .unwrap()
        .ends_with("Chrome/156.0.8078.47"));
    assert_eq!(control["v8"], "15.6.75.19");
    assert_eq!(
        control["fixtures"]["navigation-api-probe.js"],
        format!("{:x}", Sha256::digest(PROBE)),
        "stale control"
    );
    control["result"]["navigation-api-probe.js"].clone()
}

/// JSON-pointer paths where `actual` departs from Chrome, with the engine's value.
fn divergences(path: String, actual: &Value, expected: &Value, out: &mut Vec<String>) {
    match (actual, expected) {
        (Value::Object(left), Value::Object(right)) => {
            let mut keys: Vec<_> = left.keys().chain(right.keys()).collect();
            keys.sort();
            keys.dedup();
            let missing = Value::String("<absent>".into());
            for key in keys {
                divergences(
                    format!("{path}/{key}"),
                    left.get(key).unwrap_or(&missing),
                    right.get(key).unwrap_or(&missing),
                    out,
                );
            }
        }
        (Value::Array(left), Value::Array(right)) if left.len() == right.len() => {
            for (index, (left, right)) in left.iter().zip(right).enumerate() {
                divergences(format!("{path}/{index}"), left, right, out);
            }
        }
        _ if actual != expected => out.push(format!("{path} = {actual}")),
        _ => {}
    }
}

#[tokio::test(flavor = "current_thread")]
async fn navigation_api_matches_chrome156() {
    let expected = control();
    let mut page = PageSession::open(
        "<!doctype html><html><head></head><body></body></html>",
        "http://127.0.0.1/",
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    let raw = page
        .eval_json_expression(PROBE.trim().trim_end_matches(';'))
        .await
        .unwrap();
    page.close();
    let value: Value = serde_json::from_str(&raw).unwrap();
    let actual: Value = serde_json::from_str(value.as_str().unwrap()).unwrap();
    let mut found = Vec::new();
    divergences(String::new(), &actual, &expected, &mut found);
    assert_eq!(found, KNOWN_GAPS, "divergences from the Chrome control");
}
