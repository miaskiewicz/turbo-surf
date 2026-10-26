//! Same-document and element-scoped View Transitions on the render tier's real frame loop,
//! against the binding's Chrome 156 control (turbo-test scripts/view-transitions-chrome-control.mjs).
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_dom_browser_env::test_support;
use turbo_surf_render::PageSession;

const PROBE: &str = test_support::VIEW_TRANSITIONS_PROBE_JS;
const HIDDEN: &str = test_support::VIEW_TRANSITIONS_HIDDEN_PROBE_JS;

fn control() -> Value {
    let control: Value =
        serde_json::from_str(test_support::VIEW_TRANSITIONS_CHROME156_JSON).unwrap();
    assert_eq!(control["browser"], "Chrome/156.0.8078.47");
    // The saved control is only valid for the exact probe bytes Chrome executed.
    assert_eq!(
        control["probeSha256"],
        format!("{:x}", Sha256::digest(PROBE))
    );
    assert_eq!(
        control["hiddenProbeSha256"],
        format!("{:x}", Sha256::digest(HIDDEN))
    );
    control
}

async fn observe(probe: &str, sections: &[&[&str]]) -> Value {
    let mut page = PageSession::open(
        "<!doctype html><html><head></head><body></body></html>",
        "http://127.0.0.1/",
        "",
        "",
        60000,
    )
    .await
    .unwrap();
    // Each evaluation gets a fresh virtual-timer budget; the probe runs the named scenarios.
    let mut merged = serde_json::Map::new();
    for group in sections {
        let selection = serde_json::to_string(group).unwrap();
        let expression = format!(
            "globalThis.viewTransitionProbeSections = {selection}, {}",
            probe.trim().trim_end_matches(';')
        );
        let raw = page.eval_json_expression(&expression).await.unwrap();
        let Value::Object(part) = serde_json::from_str(&raw).unwrap() else {
            panic!("probe returned a non-object");
        };
        merged.extend(part);
    }
    page.close();
    Value::Object(merged)
}

fn mismatches(actual: &Value, expected: &Value, skip: &[(&str, &str)]) -> Vec<String> {
    let mut out = Vec::new();
    for (section, cases) in expected.as_object().unwrap() {
        match cases.as_object() {
            Some(cases) => {
                for (case, value) in cases {
                    if !skip.contains(&(section.as_str(), case.as_str()))
                        && &actual[section][case] != value
                    {
                        out.push(format!(
                            "{section}.{case}\n  native: {}\n  chrome: {value}",
                            actual[section][case]
                        ));
                    }
                }
                if actual[section].as_object().map(|object| object.len()) != Some(cases.len()) {
                    out.push(format!("{section}: case count differs"));
                }
            }
            None if !skip.contains(&(section.as_str(), "")) && &actual[section] != cases => {
                out.push(format!(
                    "{section}\n  native: {}\n  chrome: {cases}",
                    actual[section]
                ));
            }
            None => {}
        }
    }
    out
}

#[tokio::test(flavor = "current_thread")]
async fn view_transitions_match_chrome156_on_the_frame_loop() {
    let control = control();
    let actual = observe(
        PROBE,
        &[
            &[
                "surface",
                "brands",
                "conversion",
                "frameFromCallback",
                "values",
            ],
            &["lifecycle"],
            &["skips", "abort", "skipOrder", "cascade"],
            &["results", "documents"],
            &["scopes"],
            &["scoped", "names"],
            &["selectors", "waitUntil", "unhandled"],
        ],
    )
    .await;
    // KNOWN_GAP: the render tier never dispatches unhandledrejection (no host promise
    // rejection tracker), for these promises or any other.
    assert_eq!(actual["unhandled"], serde_json::json!([]));
    // KNOWN_GAP: the binding's selector-method SyntaxError (matches, closest,
    // querySelector*) omits Chrome's "Failed to execute 'matches' on 'Element': " prefix;
    // the rejected selectors, exception names and DOMException identity agree.
    let prefix = "Failed to execute 'matches' on 'Element': ";
    for (index, expected) in control["result"]["selectors"]["syntax"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let message = expected[1].as_str().unwrap().strip_prefix(prefix).unwrap();
        assert_eq!(
            actual["selectors"]["syntax"][index],
            serde_json::json!([expected[0], message, expected[2]])
        );
    }
    let differences = mismatches(
        &actual,
        &control["result"],
        &[("unhandled", ""), ("selectors", "syntax")],
    );
    assert!(
        differences.is_empty(),
        "{} differences:\n{}",
        differences.len(),
        differences.join("\n")
    );
}

/// KNOWN_GAP: a render-tier page has no tab or occlusion model, so its document is never
/// hidden and the "Document hidden" skip is unreachable here; the binding's own regression
/// drives that path through the rendering seam's page-visibility answer. On a visible page
/// the same probe runs its transitions to completion.
#[tokio::test(flavor = "current_thread")]
async fn view_transitions_never_see_a_hidden_page() {
    let control = control();
    assert_eq!(control["hidden"]["sync"][0], "hidden");
    let actual = observe(HIDDEN, &[&[]]).await;
    assert_eq!(actual["sync"][0], "visible");
    assert_eq!(
        actual["documentOutcome"],
        serde_json::json!({"ucd": "ok", "ready": "ok", "finished": "ok"})
    );
    assert_eq!(
        actual["elementOutcome"],
        serde_json::json!({"ucd": "ok", "ready": "ok", "finished": "ok"})
    );
}
