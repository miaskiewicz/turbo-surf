//! XMLHttpRequest's Privacy Sandbox operations (turbo-surf#165) against the Chrome 156
//! control: `setAttributionReporting` and `setPrivateToken` with their arity, receiver
//! checks, `PrivateToken` dictionary conversion, state checks and issuer validation.
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const PROBE: &str = include_str!("fixtures/xhr_privacy_sandbox_probe.js");
const CONTROL: &str = include_str!("fixtures/xhr_privacy_sandbox_chrome156.json");

#[tokio::test(flavor = "current_thread")]
async fn xhr_privacy_sandbox_operations_match_chrome156() {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    assert_eq!(
        control["probe"],
        format!("{:x}", Sha256::digest(PROBE)),
        "stale control"
    );
    let mut page = PageSession::open("<!doctype html><body>", "http://127.0.0.1:9/", "", "", 5000)
        .await
        .unwrap();
    let raw = page
        .eval_json_expression(PROBE.trim().trim_end_matches(';'))
        .await
        .unwrap();
    page.close();
    let text: String = serde_json::from_str(&raw).unwrap();
    let actual: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(actual, control["result"]);
}
