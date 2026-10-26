//! turbo-dom#24 item 3: `:defined` follows each element's custom element state. The
//! binding's Chrome 156 control (parsed, created, upgraded, failed and template elements
//! seen through matches(), querySelectorAll() and getComputedStyle()) also holds through
//! surf's `PageSession`, whose resource consumer and lifecycle run beside the binding's
//! custom element reactions.
#[path = "support/binding_control.rs"]
mod binding_control;
use turbo_dom_browser_env::test_support;

#[tokio::test(flavor = "current_thread")]
async fn defined_state_through_a_surf_session_matches_chrome156() {
    // Known gap (pre-existing, reported with P5b-surf2): parser-created elements with an
    // `is` value (`<button is=x-btn>`, `<button is=x-none>`) read `:defined` once the
    // session has opened, although they are "undefined" throughout hydration (init
    // script, parser scripts, DOMContentLoaded, load); created and inserted ones are
    // right. Pinned per snapshot, which the gap affects.
    let gaps: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/custom_elements_defined_surf_gaps.json"
    ))
    .unwrap();
    let gaps: Vec<(&str, serde_json::Value)> = ["before", "afterDefine", "afterInsert"]
        .into_iter()
        .map(|name| (name, gaps[name].clone()))
        .collect();
    binding_control::matches_control_with_gaps(
        test_support::CUSTOM_ELEMENTS_DEFINED_PROBE_JS,
        test_support::CUSTOM_ELEMENTS_DEFINED_CASES_JSON,
        test_support::CUSTOM_ELEMENTS_DEFINED_CHROME156_JSON,
        |case| case["documentBase"].as_str().unwrap(),
        &gaps,
    )
    .await;
}
