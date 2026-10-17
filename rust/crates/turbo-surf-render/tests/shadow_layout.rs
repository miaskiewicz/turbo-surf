use sha2::{Digest, Sha256};
use turbo_surf_render::run_with_dom;

fn observe_control(source: &str, fixture: &str) -> (serde_json::Value, serde_json::Value) {
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
    (
        serde_json::from_str(&actual).unwrap(),
        expected["result"].clone(),
    )
}

fn compare_control(source: &str, fixture: &str) {
    let (actual, expected) = observe_control(source, fixture);
    assert_eq!(actual, expected);
}

// Installed monospace faces differ between the macOS oracle and Linux. Keep
// all structural/line-box observations exact without substituting native text
// metrics for independent Chrome measurements.
fn shadow_range_font_independent(value: &serde_json::Value) -> serde_json::Value {
    let mut result = value.clone();
    for rows in ["before", "reassigned", "unassigned"] {
        let ranges = if rows == "unassigned" {
            &mut result[rows]["ranges"]
        } else {
            &mut result[rows]
        };
        let baseline = ranges[2]["rect"][1].as_f64().unwrap();
        for (index, row) in ranges.as_array_mut().unwrap().iter_mut().enumerate() {
            let line_top = if index == 2 { 10.0 } else { 30.0 };
            let project = |rect: &serde_json::Value| {
                let values = rect.as_array().unwrap();
                assert_eq!(values.len(), 4);
                let x = values[0].as_f64().unwrap();
                let y = values[1].as_f64().unwrap();
                let width = values[2].as_f64().unwrap();
                let height = values[3].as_f64().unwrap();
                if width <= 0.0 || height <= 0.0 {
                    return rect.clone();
                }
                serde_json::json!({"x":x,"lineOffset":y-baseline,
                    "positiveWidth":width>0.0,"positiveHeight":height>0.0,
                    "withinHostWidth":width<=240.0,
                    "withinLineBox":y>=line_top && y+height<=line_top+20.0})
            };
            row["rect"] = project(&row["rect"]);
            for rect in row["rects"].as_array_mut().unwrap() {
                *rect = project(rect);
            }
        }
    }
    result
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
    let (actual, expected) = observe_control(
        include_str!("../../../../harness/detection/shadow-range-layout-probe.js"),
        include_str!("../../../../harness/detection/fixtures/shadow-range-layout-chrome155.json"),
    );
    assert_eq!(
        shadow_range_font_independent(&actual),
        shadow_range_font_independent(&expected)
    );
    #[cfg(target_os = "macos")]
    assert_eq!(actual, expected);
}

#[test]
fn shadow_range_font_scope_preserves_positions_boundaries_and_retirement() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/shadow-range-layout-chrome155.json"
    ))
    .unwrap();
    let expected = &fixture["result"];
    let scoped = shadow_range_font_independent(expected);
    let mut other_font = expected.clone();
    for phase in ["before", "reassigned", "unassigned"] {
        let rows = if phase == "unassigned" {
            &mut other_font[phase]["ranges"]
        } else {
            &mut other_font[phase]
        };
        for row in rows.as_array_mut().unwrap() {
            let adjust = |rect: &mut serde_json::Value| {
                if rect[2].as_f64().unwrap() > 0.0 {
                    rect[1] = serde_json::json!(rect[1].as_f64().unwrap() - 0.5);
                    rect[2] = serde_json::json!(rect[2].as_f64().unwrap() + 1.0);
                    rect[3] = serde_json::json!(20.0);
                }
            };
            adjust(&mut row["rect"]);
            for rect in row["rects"].as_array_mut().unwrap() {
                adjust(rect);
            }
        }
    }
    assert_ne!(other_font, *expected);
    assert_eq!(shadow_range_font_independent(&other_font), scoped);
    for (pointer, value) in [
        ("/before/0/text", serde_json::json!("wrong source")),
        ("/before/0/rect/0", serde_json::json!(11)),
        ("/before/0/rect/1", serde_json::json!(31.5)),
        ("/before/0/rect/2", serde_json::json!(0)),
        ("/unassigned/light/0", serde_json::json!(1)),
        ("/retired/0/rect/0", serde_json::json!(1)),
    ] {
        let mut changed = expected.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert_ne!(shadow_range_font_independent(&changed), scoped, "{pointer}");
    }
    let mut changed = expected.clone();
    changed["before"][0]["rects"].as_array_mut().unwrap().pop();
    assert_ne!(shadow_range_font_independent(&changed), scoped);
}

#[tokio::test(flavor = "current_thread")]
async fn same_thread_peer_documents_and_replacements_do_not_share_layout_snapshots() {
    use turbo_surf_render::PageSession;
    // The first geometry read discovers and parses native system fonts. That
    // one-time host setup is outside this cache-isolation conformance contract.
    const CONFORMANCE_BUDGET_MS: u64 = 60_000;
    let html =
        |width| format!("<!doctype html><body><div style='width:{width}px;height:20px'></div>");
    let mut first = PageSession::open(
        &html(31),
        "https://layout.test/first",
        "",
        "",
        CONFORMANCE_BUDGET_MS,
    )
    .await
    .unwrap();
    let mut second = PageSession::open(
        &html(73),
        "https://layout.test/second",
        "",
        "",
        CONFORMANCE_BUDGET_MS,
    )
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
    let mut replacement = PageSession::open(
        &html(97),
        "https://layout.test/replaced",
        "",
        "",
        CONFORMANCE_BUDGET_MS,
    )
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

#[cfg(turbo_surf_native_proxy)]
#[tokio::test(flavor = "current_thread")]
async fn nested_shadow_scope_mutations_slot_reassignment_and_range_frame_migrations_match_chrome() {
    let source = include_str!("../../../../harness/detection/composed-layout-lifecycle-probe.js");
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/composed-layout-lifecycle-chrome155.json"
    ))
    .unwrap();
    assert_eq!(
        expected["probeSha256"],
        format!("{:x}", Sha256::digest(source))
    );
    let mut page = turbo_surf_render::PageSession::open(
        "<!doctype html><html><head></head><body></body></html>",
        "https://layout-lifecycle.test/",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let actual: serde_json::Value = serde_json::from_str(
        &page
            .eval_json_expression(source.trim().trim_end_matches(';'))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(actual, expected["result"]);
    page.close();
}
