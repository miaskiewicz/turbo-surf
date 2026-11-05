//! turbo-surf#36 items 1–2: getComputedStyle reports the UA sheet's `display` /
//! `position` / `visibility` for elements no author rule styles, and a document without
//! a `<title>` gets none injected. The binding's Chrome 156 control holds through surf's
//! `PageSession` (surf installs no getComputedStyle defaults and no title of its own).
#[path = "support/binding_control.rs"]
mod binding_control;
use turbo_dom_browser_env::test_support;

/// surf's current transform-resolution boxes: Chrome's, except `xform`.
fn transform_boxes() -> serde_json::Value {
    let mut boxes = serde_json::json!({"page": [784, 1100, 784, 1100, 16], "none": [784, 0, 784, 0, 16],
        "x12": [784, 0, 784, 0, 20], "xform": [784, 0, 784, 0, 16]});
    for id in 1..=11 {
        boxes[format!("x{id}")] = serde_json::json!([200, 100, 200, 100, 16]);
    }
    boxes
}

#[tokio::test(flavor = "current_thread")]
async fn computed_defaults_and_titles_through_a_surf_session_match_chrome156() {
    // `used-size` (turbo-dom#316) reads every element's client rects, and live layout
    // throws for its <math> element (turbo-dom-layout's live capture has no MathML yet);
    // surf's used sizes are held against Chrome 156 in tests/parity_080.rs.
    // The control's headless Chrome window: 800x600 at device scale 1 (the
    // transform-resolution case measures boxes against it).
    turbo_surf_render::set_fingerprint(
        r#"{"innerWidth":800,"innerHeight":600,"outerWidth":800,"outerHeight":600,"devicePixelRatio":1}"#,
    );
    binding_control::matches_control_except(
        test_support::COMPUTED_DEFAULTS_PROBE_JS,
        test_support::COMPUTED_DEFAULTS_CASES_JSON,
        test_support::COMPUTED_DEFAULTS_CHROME156_JSON,
        |case| case["documentBase"].as_str().unwrap_or("http://localhost/"),
        // An empty unknown inline (`<xform>`, last in its line) reads the line's width as
        // offsetWidth (784) where Chrome reads 0: live layout's inline geometry
        // (turbo-dom-layout cssom_view::inline), not the transform rows this case is for.
        &[("transformBoxes", transform_boxes())],
        &["used-size"],
    )
    .await;
    turbo_surf_render::set_fingerprint("{}");
}
