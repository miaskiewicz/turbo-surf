//! Interface objects and prototypes of the whole Surf realm (binding, Deno web
//! extensions and the render tier's own interfaces) list their keys in Chrome
//! 156's order, with Chrome's non-configurable constants, in the top-level
//! document and in child frames. The oracle is the binding's
//! key_order_chrome156.json captured by harness/detection/api-shape.mjs.
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_dom_browser_env::test_support;
use turbo_surf_render::PageSession;

const PROBE: &str = test_support::KEY_ORDER_PROBE_JS;

fn oracle() -> Value {
    let fixture: Value = serde_json::from_str(test_support::KEY_ORDER_CHROME156_JSON).unwrap();
    assert_eq!(fixture["browser"], "Chrome/156.0.8078.47");
    assert_eq!(fixture["v8"], "15.6.75.19");
    assert_eq!(
        fixture["probeSha256"],
        format!("{:x}", Sha256::digest(PROBE))
    );
    fixture["result"].clone()
}

async fn observe(html: &str, expression: &str) -> Value {
    let mut page = PageSession::open(html, "http://127.0.0.1/", "", "", 20000)
        .await
        .unwrap();
    let raw = page.eval_json_expression(expression).await.unwrap();
    page.close();
    let text: String = serde_json::from_str(&raw).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn listed(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

fn mismatches(actual: &Value, chrome: &Value) -> (usize, Vec<String>) {
    let mut compared = 0;
    let mut failures = Vec::new();
    for (name, ours) in actual.as_object().unwrap() {
        let Some(theirs) = chrome.get(name) else {
            continue;
        };
        compared += 1;
        for part in ["interface", "prototype"] {
            let mine = listed(&ours[part]["keys"]);
            let expected = listed(&theirs[part]["keys"]);
            let shared: Vec<&str> = mine
                .iter()
                .copied()
                .filter(|key| expected.contains(key))
                .collect();
            let ordered: Vec<&str> = expected
                .iter()
                .copied()
                .filter(|key| mine.contains(key))
                .collect();
            if shared != ordered {
                failures.push(format!(
                    "{name}.{part} order\n  native: {shared:?}\n  chrome: {ordered:?}"
                ));
            }
            let fixed = |entry: &Value| -> Vec<String> {
                listed(&entry[part]["fixed"])
                    .into_iter()
                    .filter(|key| mine.contains(key) && expected.contains(key))
                    .map(str::to_owned)
                    .collect()
            };
            if fixed(ours) != fixed(theirs) {
                failures.push(format!(
                    "{name}.{part} non-configurable\n  native: {:?}\n  chrome: {:?}",
                    fixed(ours),
                    fixed(theirs)
                ));
            }
        }
    }
    (compared, failures)
}

#[tokio::test(flavor = "current_thread")]
async fn top_level_interfaces_follow_chrome156_key_order() {
    let actual = observe(
        "<!doctype html><html><head></head><body></body></html>",
        PROBE.trim().trim_end_matches(';'),
    )
    .await;
    let (compared, failures) = mismatches(&actual, &oracle());
    assert!(compared >= 200, "only {compared} shared interfaces");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// Both child-frame bootstraps: the initial about:blank document and a parsed one.
#[tokio::test(flavor = "current_thread")]
async fn child_frame_interfaces_follow_chrome156_key_order() {
    let probe = serde_json::to_string(PROBE.trim()).unwrap();
    for frame in [
        "<iframe></iframe>",
        "<iframe srcdoc='<p>child</p>'></iframe>",
    ] {
        let actual = observe(
            &format!("<!doctype html><html><head></head><body>{frame}</body></html>"),
            &format!("document.querySelector('iframe').contentWindow.eval({probe})"),
        )
        .await;
        let (compared, failures) = mismatches(&actual, &oracle());
        assert!(
            compared >= 200,
            "{frame}: only {compared} shared interfaces"
        );
        assert!(failures.is_empty(), "{frame}: {}", failures.join("\n"));
    }
}
