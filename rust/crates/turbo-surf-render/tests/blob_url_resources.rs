//! blob: URLs as script, module, stylesheet, image and iframe sources
//! (turbo-surf#146) against the Chrome 156 control, offline over an empty document.
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const PROBE: &str = include_str!("../../../../harness/detection/blob-url-resources-probe.js");
const CONTROL: &str =
    include_str!("../../../../harness/detection/fixtures/blob-url-resources-chrome156.json");

fn control() -> Value {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    assert_eq!(control["browser"], "HeadlessChrome/156.0.8078.47");
    assert_eq!(control["v8"], "15.6.75.19");
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(
        control["probeSha256"],
        format!("{:x}", Sha256::digest(PROBE)),
        "stale control"
    );
    serde_json::from_str(control["result"].as_str().unwrap()).unwrap()
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
async fn blob_url_resources_match_chrome156() {
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
    assert_eq!(
        found,
        Vec::<String>::new(),
        "divergences from the Chrome control"
    );
}
