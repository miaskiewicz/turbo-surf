//! Run a turbo-dom-browser-env Chrome control (probe + cases + recorded result) through
//! surf's real `PageSession`, so the binding's controls also hold over surf's loaders,
//! resource consumer and event loop. Shared by the integration tests via `#[path]`.
#![allow(dead_code)] // each test binary uses its own subset
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

pub fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Run `probe` on every case page and require the Chrome control's result exactly.
pub async fn matches_control(probe: &str, cases: &[u8], control: &str, base: fn(&Value) -> &str) {
    matches_control_with_gaps(probe, cases, control, base, &[]).await;
}

/// [`matches_control`], except each `(member, value)` of `gaps`: a known gap, pinned to
/// surf's current value and required to still differ from Chrome's (so a fix fails
/// the pin and the gap is retired with it).
pub async fn matches_control_with_gaps(
    probe: &str,
    cases: &[u8],
    control: &str,
    base: fn(&Value) -> &str,
    gaps: &[(&str, Value)],
) {
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
        // A generous cap, not a timing: the probes are in-isolate but lay out live.
        let mut page =
            PageSession::open(case["html"].as_str().unwrap(), base(case), "", "", 60_000)
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
        for (name, pinned) in gaps {
            assert_eq!(
                actual[*name], *pinned,
                "{}: known gap {name} changed",
                case["id"]
            );
            assert_ne!(
                expected[*name], *pinned,
                "{}: known gap {name} is fixed",
                case["id"]
            );
        }
        let mismatches: Vec<_> = expected
            .iter()
            .filter(|(name, _)| !gaps.iter().any(|(gap, _)| gap == name))
            .filter(|(name, value)| actual.get(name.as_str()) != Some(value))
            .map(|(name, value)| format!("{name}: expected {value}, got {}", actual[name.as_str()]))
            .collect();
        assert!(mismatches.is_empty(), "{}: {mismatches:#?}", case["id"]);
        assert_eq!(actual.as_object().unwrap().len(), expected.len());
    }
}
