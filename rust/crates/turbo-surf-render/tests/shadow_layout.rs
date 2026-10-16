use sha2::{Digest, Sha256};
use turbo_surf_render::run_with_dom;

fn compare_control(source: &str, fixture: &str) {
    let expected: serde_json::Value = serde_json::from_str(fixture).unwrap();
    assert_eq!(expected["browser"], "Chrome/155.0.8059.80");
    assert_eq!(expected["v8"], "15.5.35.23");
    assert_eq!(
        expected["probeSha256"],
        format!("{:x}", Sha256::digest(source.as_bytes()))
    );
    let probe = source.trim().trim_end_matches(';');
    let actual = run_with_dom(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("JSON.stringify({probe})"),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        expected["result"]
    );
}

#[test]
fn unboxed_slots_have_zero_metrics_and_do_not_clip_or_position_assigned_children() {
    compare_control(
        include_str!("../../../../harness/detection/unboxed-shadow-layout-probe.js"),
        include_str!("../../../../harness/detection/fixtures/unboxed-shadow-layout-chrome155.json"),
    );
}

#[test]
fn nonempty_host_light_and_shadow_ranges_follow_actual_boundaries_after_reassignment_and_retirement(
) {
    compare_control(
        include_str!("../../../../harness/detection/shadow-range-layout-probe.js"),
        include_str!("../../../../harness/detection/fixtures/shadow-range-layout-chrome155.json"),
    );
}

#[tokio::test(flavor = "current_thread")]
async fn same_thread_peer_documents_and_replacements_do_not_share_layout_snapshots() {
    use turbo_surf_render::PageSession;
    let html =
        |width| format!("<!doctype html><body><div style='width:{width}px;height:20px'></div>");
    let mut first = PageSession::open(&html(31), "https://layout.test/first", "", "", 5000)
        .await
        .unwrap();
    let mut second = PageSession::open(&html(73), "https://layout.test/second", "", "", 5000)
        .await
        .unwrap();
    let read = "document.querySelector('div').getBoundingClientRect().width";
    // Both isolates begin native document numbering at one, with equal parse
    // mutation counts. Interleave reads after both viewport installations.
    for _ in 0..3 {
        assert_eq!(first.eval_expression(read).await.unwrap(), "31");
        assert_eq!(second.eval_expression(read).await.unwrap(), "73");
    }
    first.close();
    let mut replacement =
        PageSession::open(&html(97), "https://layout.test/replaced", "", "", 5000)
            .await
            .unwrap();
    assert_eq!(second.eval_expression(read).await.unwrap(), "73");
    assert_eq!(replacement.eval_expression(read).await.unwrap(), "97");
    second.close();
    assert_eq!(replacement.eval_expression(read).await.unwrap(), "97");
    replacement.close();
}

#[test]
fn composed_shadow_layout_matches_independent_chrome_control() {
    let source = include_str!("../../../../harness/detection/shadow-layout-probe.js");
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/shadow-layout-chrome155.json"
    ))
    .unwrap();
    assert_eq!(expected["browser"], "Chrome/155.0.8059.80");
    assert_eq!(expected["v8"], "15.5.35.23");
    assert_eq!(
        expected["probeSha256"],
        format!("{:x}", Sha256::digest(source.as_bytes()))
    );
    let probe = source.trim().trim_end_matches(';');
    let actual = run_with_dom(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("JSON.stringify({probe})"),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        expected["result"]
    );
}

#[test]
fn inline_csp_applied_values_drive_light_and_shadow_layout_without_rewriting_attributes() {
    let source = include_str!("../../../../harness/detection/shadow-csp-layout-probe.js");
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/shadow-csp-layout-chrome155.json"
    ))
    .unwrap();
    assert_eq!(expected["browser"], "Chrome/155.0.8059.80");
    assert_eq!(expected["v8"], "15.5.35.23");
    assert_eq!(
        expected["probeSha256"],
        format!("{:x}", Sha256::digest(source.as_bytes()))
    );
    let probe = source.trim().trim_end_matches(';');
    let actual = run_with_dom(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("JSON.stringify({probe})"),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        expected["result"]
    );
}

#[test]
fn real_supports_declarations_boolean_groups_and_live_source_order_match_chrome() {
    compare_control(
        include_str!("../../../../harness/detection/supports-layout-probe.js"),
        include_str!("../../../../harness/detection/fixtures/supports-layout-chrome155.json"),
    );
}

#[test]
fn genuine_css_supports_overloads_native_grammar_selectors_and_private_coercion_match_chrome() {
    compare_control(
        include_str!("../../../../harness/detection/css-supports-probe.js"),
        include_str!("../../../../harness/detection/fixtures/css-supports-chrome155.json"),
    );
}

#[test]
fn genuine_selector_matching_and_specificity_match_light_and_closed_shadow_chrome_controls() {
    compare_control(
        include_str!("../../../../harness/detection/selector-layout-probe.js"),
        include_str!("../../../../harness/detection/fixtures/selector-layout-chrome155.json"),
    );
}
