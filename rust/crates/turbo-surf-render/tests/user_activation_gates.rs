//! Activation-gated behavior driven through the trusted host input path, against the
//! source-pinned Chrome 156 control: showPicker, fullscreen, Esc close watchers,
//! closedby=any / popover light dismiss and MessageEvent.userActivation.
//!
//! The control drives real trusted input through CDP Input.dispatchMouseEvent /
//! Input.dispatchKeyEvent; this test replays the same steps through `PageSession::input`.
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

const CASES: &str = include_str!("../../../../harness/detection/user-activation-gates-cases.json");
const COLLECTOR: &[u8] =
    include_bytes!("../../../../harness/detection/user-activation-gates-chrome.mjs");
const CONTROL: &str =
    include_str!("../../../../harness/detection/fixtures/user_activation_gates_chrome156.json");

/// Measured Chrome behavior the engine does not reproduce, as `case/path = engine value`.
const KNOWN_GAPS: &[&str] = &[
    // showPicker() succeeds and consumes activation, but no picker UI exists: Chrome's
    // control then matches `:open` (date/color/select) until the picker closes.
    "picker-activation-date/1/open = false",
    "picker-activation-date/1/openLater = false",
    "picker-activation-color/1/open = false",
    "picker-activation-color/1/openLater = false",
    "picker-activation-select/1/open = false",
    // The binding's focus events are not composed (Chrome's FocusEvent is).
    "picker-activation-date/1/log/0/6 = false",
    "picker-activation-color/1/log/0/6 = false",
    "picker-activation-file/1/log/0/6 = false",
    "picker-activation-text/1/log/0/6 = false",
    "picker-activation-checkbox/1/log/0/6 = false",
    "picker-activation-select/1/log/0/6 = false",
    // Headless Chrome resizes its window to the screen in fullscreen; the engine's
    // viewport is fixed, so innerWidth/innerHeight never change.
    "fullscreen-activation/1/sizeChanged = false",
    "fullscreen-activation/1/sizeLater = false",
    // ...and 300 ms later Chrome's window matches the screen (turbo-surf#52 recapture).
    "fullscreen-activation/1/matchesScreenLater = false",
    // Chrome dispatches the dialog `close` event before already-queued toggle tasks
    // during Esc processing (batch-1 leaves close-vs-task order unpinned).
    // Closing a modal dialog whose focused control had no prior focus to restore leaves
    // ACTIVE on that control; Chrome resets focus to <body> because the closed dialog is
    // no longer focusable. The following keyup then targets the control, not <body>.
    r#"escape-modal-no-activation/1/log/3/1 = "inner""#,
    r#"escape-dialog-and-popover/1/log/5/0 = "toggle""#,
    r#"escape-dialog-and-popover/1/log/5/1 = "p""#,
    r#"escape-dialog-and-popover/1/log/6/1 = "d""#,
    r#"escape-dialog-and-popover/1/log/7/0 = "close""#,
];

fn control() -> Value {
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    for (name, bytes) in [
        ("casesSha256", CASES.as_bytes()),
        ("collectorSha256", COLLECTOR),
    ] {
        assert_eq!(
            control[name],
            format!("{:x}", Sha256::digest(bytes)),
            "stale control: {name}"
        );
    }
    assert_eq!(control["browser"], "Chrome/156.0.8078.47");
    assert_eq!(control["v8"], "15.6.75.19");
    assert_eq!(control["headless"], true);
    assert_eq!(control["errors"], serde_json::json!([]));
    control
}

fn differences(path: &str, actual: &Value, expected: &Value, out: &mut Vec<String>) {
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => {
            for (key, value) in expected {
                let actual = actual.get(key).unwrap_or(&Value::Null);
                differences(&format!("{path}/{key}"), actual, value, out);
            }
            for key in actual.keys().filter(|key| !expected.contains_key(*key)) {
                out.push(format!("{path}/{key} (extra)"));
            }
        }
        (Value::Array(actual), Value::Array(expected)) if actual.len() == expected.len() => {
            for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
                differences(&format!("{path}/{index}"), actual, expected, out);
            }
        }
        _ if actual == expected => {}
        _ => out.push(format!("{path} = {actual}")),
    }
}

async fn run_case(prelude: &str, index: usize, case: &Value) -> Vec<Value> {
    let html = format!(
        "<!doctype html><script>{prelude}</script>{}",
        case["html"].as_str().unwrap()
    );
    let url = format!("http://127.0.0.1/{index}");
    let mut page = PageSession::open(&html, &url, "", "", 20000).await.unwrap();
    let mut values = Vec::new();
    // Chrome runs queued tasks between CDP commands; let queued tasks run between steps.
    let settle = serde_json::json!([{ "wait": 0 }]);
    for step in case["steps"].as_array().unwrap() {
        if let Some(expression) = step["eval"].as_str() {
            values.push(match page.eval_json_expression(expression).await {
                Ok(value) => serde_json::from_str(&value).unwrap(),
                Err(error) => Value::String(format!("error: {error}")),
            });
        } else {
            if let Err(error) = page.input(&serde_json::json!([step])).await {
                values.push(Value::String(format!("input error: {error}")));
            }
        }
        page.input(&settle).await.unwrap();
    }
    page.close();
    values
}

#[tokio::test(flavor = "current_thread")]
async fn activation_gates_follow_trusted_host_input_like_chrome() {
    let control = control();
    let cases: Value = serde_json::from_str(CASES).unwrap();
    let prelude = cases["prelude"].as_str().unwrap();
    let mut found = Vec::new();
    for (index, case) in cases["cases"].as_array().unwrap().iter().enumerate() {
        let id = case["id"].as_str().unwrap();
        let expected = control["result"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        let actual = Value::Array(run_case(prelude, index, case).await);
        differences(id, &actual, &expected["values"], &mut found);
    }
    let unexpected = found
        .iter()
        .filter(|line| !KNOWN_GAPS.contains(&line.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let resolved = KNOWN_GAPS
        .iter()
        .filter(|gap| !found.iter().any(|line| line == *gap))
        .collect::<Vec<_>>();
    assert!(
        unexpected.is_empty() && resolved.is_empty(),
        "unexpected:\n{}\nresolved gaps: {resolved:?}",
        unexpected.join("\n")
    );
}
