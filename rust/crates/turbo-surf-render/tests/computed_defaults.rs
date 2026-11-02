//! turbo-surf#36 items 1–2: getComputedStyle reports the UA sheet's `display` /
//! `position` / `visibility` for elements no author rule styles, and a document without
//! a `<title>` gets none injected. The binding's Chrome 156 control holds through surf's
//! `PageSession` (surf installs no getComputedStyle defaults and no title of its own).
#[path = "support/binding_control.rs"]
mod binding_control;
use turbo_dom_browser_env::test_support;

#[tokio::test(flavor = "current_thread")]
async fn computed_defaults_and_titles_through_a_surf_session_match_chrome156() {
    binding_control::matches_control(
        test_support::COMPUTED_DEFAULTS_PROBE_JS,
        test_support::COMPUTED_DEFAULTS_CASES_JSON,
        test_support::COMPUTED_DEFAULTS_CHROME156_JSON,
        |case| case["documentBase"].as_str().unwrap_or("http://localhost/"),
    )
    .await;
}
