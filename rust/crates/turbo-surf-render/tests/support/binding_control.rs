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

/// Run `probe` in `page` and parse its JSON result. A probe that asks for user activation
/// (`__cdpGesture()`, the Chrome collector's trusted CDP click, then `__cdpGestureDone()`)
/// gets a trusted host click from surf's input driver each time it asks.
pub async fn run_probe(page: &mut PageSession, probe: &str) -> Value {
    let probe = probe.trim_end().trim_end_matches(';');
    if !probe.contains("__cdpGesture") {
        return serde_json::from_str(&page.eval_json_expression(probe).await.unwrap()).unwrap();
    }
    // The probe's synchronous part runs here, under the session budget; page.eval below
    // drains the page's tasks and timers (the probe's own short waits) on each call.
    page.eval_expression(&format!(
        "(()=>{{globalThis.__cdpGesture=()=>{{globalThis.__surfGesture=true}};globalThis.__surfProbe=null;\
         Promise.resolve({probe}).then(value=>{{globalThis.__surfProbe=JSON.stringify(value)}},\
         error=>{{globalThis.__surfProbe=JSON.stringify({{probeError:String(error)}})}});return true}})()"
    ))
    .await
    .unwrap();
    // A generous cap, not a timing.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let mut state = Value::Null;
    while std::time::Instant::now() < deadline {
        state = serde_json::from_str(
            &page
                .eval("globalThis.__RESULT=JSON.stringify([globalThis.__surfProbe, globalThis.__surfGesture===true])")
                .await
                .unwrap(),
        )
        .unwrap();
        if let Some(result) = state[0].as_str() {
            return serde_json::from_str(result).unwrap();
        }
        if state[1] == true {
            page.input(&serde_json::json!([{"click": "body"}]))
                .await
                .unwrap();
            page.eval("globalThis.__surfGesture=false;__cdpGestureDone();globalThis.__RESULT=''")
                .await
                .unwrap();
        } else {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    panic!("probe never settled: {state}");
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
        // A generous cap, not a timing: the probes are in-isolate but lay out live, and the
        // largest (computed_defaults' pseudo-element tables) take ~45 s of a debug build on a
        // host at load average 200.
        let mut page =
            PageSession::open(case["html"].as_str().unwrap(), base(case), "", "", 180_000)
                .await
                .unwrap();
        let actual = run_probe(&mut page, probe).await;
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
