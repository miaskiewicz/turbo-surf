//! The interceptor fix is ACTIVE in surf's real runtime: strict [[Set]]/[[Delete]] and
//! Object.defineProperty on document.all, <select> and select.options throw Blink's
//! TypeError, while sloppy writes and the Reflect forms stay quiet, exactly as the
//! committed Chrome 156 control recorded (tests/fixture-gen/interceptor-mode-chrome.mjs).
//! Without turbo-dom-v8's ShouldThrowOnError fix every strict case is silently quiet.
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const PROBE: &str = include_str!("fixtures/interceptor_mode_probe.js");
const CONTROL: &str = include_str!("fixtures/interceptor_mode_chrome156.json");
/// Measured divergences that are not about the interceptor mode, each asserted to
/// still diverge so a fix shows up here. None remain: the binding's
/// HTMLOptionsCollection now treats a quiet `Reflect.defineProperty` accessor
/// definition like Chrome (true, no effect; turbo-dom#20).
const KNOWN_GAPS: &[&str] = &[];

#[tokio::test(flavor = "current_thread")]
async fn strict_and_sloppy_interceptor_writes_match_chrome156() {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    assert!(control["browser"]
        .as_str()
        .unwrap()
        .starts_with("Chrome/156."));
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(
        control["probeSha256"],
        format!("{:x}", Sha256::digest(PROBE.as_bytes())),
        "stale interceptor control: re-run the collector"
    );
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://interceptor-mode.test/",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let actual: Value = serde_json::from_str(
        &page
            .eval_json_expression(PROBE.trim_end().trim_end_matches(';'))
            .await
            .unwrap(),
    )
    .unwrap();
    let expected = control["result"].as_object().unwrap();
    // The control must exercise both outcomes of every operation kind.
    let threw = |name: &str| expected[name]["threw"] == "TypeError";
    assert!(threw("allSetStrict") && !threw("allSetSloppy"));
    assert!(threw("allDeleteStrict") && !threw("allDeleteSloppy"));
    assert!(threw("allDefine") && !threw("allReflectDefine"));
    assert!(threw("selectDefineAccessor") && !threw("selectReflectDefineAccessor"));
    for gap in KNOWN_GAPS {
        assert_ne!(actual[gap], expected[*gap], "{gap} now matches Chrome");
    }
    let mismatches: Vec<_> = expected
        .iter()
        .filter(|(name, _)| !KNOWN_GAPS.contains(&name.as_str()))
        .filter(|(name, value)| actual.get(name.as_str()) != Some(value))
        .map(|(name, value)| format!("{name}: expected {value}, got {}", actual[name.as_str()]))
        .collect();
    assert!(mismatches.is_empty(), "{mismatches:#?}");
    assert_eq!(actual.as_object().unwrap().len(), expected.len());
}
