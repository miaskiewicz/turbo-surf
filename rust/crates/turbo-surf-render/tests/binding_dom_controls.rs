//! turbo-dom-browser-env's Chrome 156 DOM controls added with the P5b binding fixes
//! (turbo-dom#66 shadow-tree event paths, the `<track>` element, CE reactions on
//! adoption and name changes, the realm's DOMException interface) also hold through
//! surf's `PageSession`, whose own DOMException, event loop and resource consumer run
//! beside the binding.
#[path = "support/binding_control.rs"]
mod binding_control;
use serde_json::Value;
use turbo_dom_browser_env::test_support;

/// The case's own document base, else a neutral https origin (the binding's runner installs
/// these documents without a URL).
fn base(case: &Value) -> &str {
    case["documentBase"]
        .as_str()
        .unwrap_or("https://binding.test/")
}

#[tokio::test(flavor = "current_thread")]
async fn shadow_tree_event_paths_match_chrome156() {
    binding_control::matches_control(
        test_support::SHADOW_EVENTS_PROBE_JS,
        test_support::SHADOW_EVENTS_CASES_JSON.as_bytes(),
        test_support::SHADOW_EVENTS_CHROME156_JSON,
        base,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn track_elements_match_chrome156() {
    binding_control::matches_control(
        test_support::HTML_TRACK_PROBE_JS,
        test_support::HTML_TRACK_CASES_JSON.as_bytes(),
        test_support::HTML_TRACK_CHROME156_JSON,
        base,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn adoption_reactions_match_chrome156() {
    binding_control::matches_control(
        test_support::DOM_ADOPTION_REACTIONS_PROBE_JS,
        test_support::DOM_ADOPTION_REACTIONS_CASES_JSON.as_bytes(),
        test_support::DOM_ADOPTION_REACTIONS_CHROME156_JSON,
        base,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn reaction_names_match_chrome156() {
    binding_control::matches_control(
        test_support::DOM_REACTIONS_NAMES_PROBE_JS,
        test_support::DOM_REACTIONS_NAMES_CASES_JSON.as_bytes(),
        test_support::DOM_REACTIONS_NAMES_CHROME156_JSON,
        base,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn binding_dom_exceptions_are_the_realm_interface_like_chrome156() {
    binding_control::matches_control(
        test_support::DOM_EXCEPTION_REALM_PROBE_JS,
        test_support::DOM_EXCEPTION_REALM_CASES_JSON,
        test_support::DOM_EXCEPTION_REALM_CHROME156_JSON,
        base,
    )
    .await;
}
