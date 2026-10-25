//! CSS Typed OM members that need the full page runtime (the CSS.* factories on the CSS
//! namespace, DOMMatrix-backed toMatrix()/CSSMatrixComponent, matrix() reification and
//! computedStyleMap() against a served document) compared with the authored Chrome 156 control.
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const PROBE: &str = include_str!("../../../../harness/detection/typed-om-probe.js");

#[tokio::test(flavor = "current_thread")]
async fn typed_om_runtime_members_match_chrome156() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/typed-om-chrome156.json"
    ))
    .unwrap();
    assert_eq!(fixture["browser"], "Chrome/156.0.8078.47");
    assert_eq!(
        fixture["fixtures"]["typed-om-probe.js"],
        format!("{:x}", Sha256::digest(PROBE))
    );
    let expected = &fixture["result"]["typed-om-probe.js"];
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
    let mut actual: Value = serde_json::from_str(&raw).unwrap();
    // KNOWN_GAP: Blink's internal DOMMatrix::perspectiveSelf writes m34 without clearing the
    // is2D flag, so Chrome reports CSSPerspective.toMatrix() as 2D; the public DOMMatrix API
    // the binding builds on marks a non-zero m34 as 3D. The entries themselves match.
    for row in [13, 14] {
        let (ours, chrome) = (&actual["toMatrix"][row], &expected["toMatrix"][row]);
        assert_eq!(ours[1][0], chrome[1][0]);
        assert_eq!(
            (&ours[1][1], &chrome[1][1]),
            (&Value::Bool(false), &Value::Bool(true))
        );
        actual["toMatrix"][row] = chrome.clone();
    }
    let differences: Vec<String> = expected
        .as_object()
        .unwrap()
        .iter()
        .filter(|(key, value)| actual.get(key.as_str()) != Some(*value))
        .map(|(key, value)| format!("{key}:\n  native={}\n  Chrome={value}", actual[key]))
        .collect();
    assert!(differences.is_empty(), "{}", differences.join("\n"));
    assert_eq!(&actual, expected);
}
