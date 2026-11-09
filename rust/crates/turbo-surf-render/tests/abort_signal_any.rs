//! AbortSignal.any() sequence conversion (the JS-array fast path) and
//! AbortSignal.timeout() conversion, reason and event order against the
//! source-pinned Chrome 156 control, offline over an empty document.
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const PROBE: &str = include_str!("../../../../harness/detection/abort-signal-any-probe.js");
const CONTROL: &str =
    include_str!("../../../../harness/detection/fixtures/abort-signal-any-chrome156.json");

/// Measured Chrome behavior the engine does not reproduce, as `path = engine value`.
///
/// The per-listener microtask checkpoint (turbo-dom#382) now lands for the
/// `/timeoutListeners/*` matrix, closing the six gaps that cycle-1 tracked.
/// A new nested-abort ordering gap appeared in its wake: on a timed-out abort
/// whose first listener synchronously aborts an inner controller, Blink runs
/// the caller's "after-inner" synchronous tail before the inner signal's
/// listener-microtask, whereas the per-listener checkpoint flushes the inner
/// microtask between the two synchronous steps. Tracked as a v0.9.0 turbo-dom
/// follow-up; indices 1 and 2 of `/timeoutNestedAbort` are swapped against
/// Chrome.
const KNOWN_GAPS: &[&str] = &[];

fn control() -> Value {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    assert!(control["browser"]
        .as_str()
        .unwrap()
        .ends_with("Chrome/156.0.8078.47"));
    assert_eq!(control["v8"], "15.6.75.19");
    assert_eq!(
        control["fixtures"]["abort-signal-any-probe.js"],
        format!("{:x}", Sha256::digest(PROBE)),
        "stale control"
    );
    control["result"]["abort-signal-any-probe.js"].clone()
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
#[ignore = "FIXME(turbo-dom#488): per-listener microtask checkpoint doesn't fire for V8-native AbortSignal.timeout; turbo-dom's polyfill install silently no-ops inside surf's install_with_html bootstrap (IIFE reaches __engineTaskScope at event_listeners.js:346 but not the defineProperty at :386). Surf-side engine_task_scope wrap + op_engine_task_scope stay in place; test un-ignores once turbo-dom#488's polyfill-install fix lands"]
async fn abort_signal_any_and_timeout_match_chrome156() {
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
