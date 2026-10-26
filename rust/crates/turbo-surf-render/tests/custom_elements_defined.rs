//! turbo-dom#24 item 3: `:defined` follows each element's custom element state. The
//! binding's Chrome 156 control (parsed, created, upgraded, failed and template elements
//! seen through matches(), querySelectorAll() and getComputedStyle()) also holds through
//! surf's `PageSession`, whose resource consumer and lifecycle run beside the binding's
//! custom element reactions — including after a V8 GC has run turbo-dom's storage
//! collection over the parsed tree (turbo-surf#41, fixed in turbo-dom bac0074).
#[path = "support/binding_control.rs"]
mod binding_control;
use turbo_dom_browser_env::test_support;

#[tokio::test(flavor = "current_thread")]
async fn defined_state_through_a_surf_session_matches_chrome156() {
    binding_control::matches_control(
        test_support::CUSTOM_ELEMENTS_DEFINED_PROBE_JS,
        test_support::CUSTOM_ELEMENTS_DEFINED_CASES_JSON,
        test_support::CUSTOM_ELEMENTS_DEFINED_CHROME156_JSON,
        |case| case["documentBase"].as_str().unwrap(),
    )
    .await;
}
