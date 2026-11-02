use turbo_surf_render::run_with_dom;

fn inline_advance_measurement(family: &str) -> serde_json::Value {
    let family = serde_json::to_string(family).unwrap();
    let probe = r#"(family=>{
      document.body.innerHTML='<div style="width:1000px;font:16px/24px monospace"><span id="a">abc</span><span id="b">z</span></div>';
      document.body.firstElementChild.style.fontFamily=family;
      const a=document.getElementById('a'),b=document.getElementById('b');
      const plain=a.getBoundingClientRect(),before=b.getBoundingClientRect();
      a.style='padding:3px 5px;border:2px solid;margin:0 7px';
      const decorated=a.getBoundingClientRect(),after=b.getBoundingClientRect();
      const range=document.createRange();range.selectNodeContents(a);const text=range.getBoundingClientRect();
      const checks={decoratedWidth:decorated.width===plain.width+14,followingAdvance:after.x===before.x+28,textWidth:text.width===plain.width,textOrigin:text.x===plain.x+14,clientWidth:a.clientWidth===0,clientHeight:a.clientHeight===0,offsetWidth:a.offsetWidth===Math.round(decorated.width)};
      return JSON.stringify({family,checks,actual:{plain:{x:plain.x,width:plain.width},before:{x:before.x,width:before.width},decorated:{x:decorated.x,width:decorated.width},after:{x:after.x,width:after.width},text:{x:text.x,width:text.width},clientWidth:a.clientWidth,clientHeight:a.clientHeight,offsetWidth:a.offsetWidth},expected:{decoratedWidth:plain.width+14,followingX:before.x+28,textWidth:plain.width,textX:plain.x+14,offsetWidth:Math.round(decorated.width)}});
    })"#;
    let actual = run_with_dom(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("{probe}({family})"),
    )
    .unwrap();
    serde_json::from_str(&actual).unwrap()
}
fn expected_inline_checks() -> serde_json::Value {
    serde_json::json!({"decoratedWidth":true,"followingAdvance":true,"textWidth":true,"textOrigin":true,"clientWidth":true,"clientHeight":true,"offsetWidth":true})
}
#[test]
fn inline_decoration_uses_real_text_advances_and_distinguishes_client_and_offset_metrics() {
    let actual = inline_advance_measurement("monospace");
    assert_eq!(actual["checks"], expected_inline_checks(), "{actual}");
}

#[test]
fn bundled_inline_advances_preserve_exact_decoration_and_offset_contracts() {
    let actual = ["IBM Plex Mono", "Fira Code"].map(inline_advance_measurement);
    for row in actual {
        assert_eq!(row["checks"], expected_inline_checks(), "{row}");
    }
}

fn bundled_geometry_matches_chrome(source: &str, expected: &str, families: &[&str]) {
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    let expected: Value = serde_json::from_str(expected).unwrap();
    assert_eq!(expected["mode"], "headed-direct-cdp");
    assert_eq!(expected["errors"], serde_json::json!([]));
    assert_eq!(
        expected["probeSha256"],
        format!("{:x}", Sha256::digest(source))
    );
    assert_eq!(
        expected["collectorSha256"],
        format!(
            "{:x}",
            Sha256::digest(include_str!(
                "../../../../harness/detection/font-run-geometry-chrome-control.mjs"
            ))
        )
    );
    let rows = expected["result"].as_array().unwrap();
    for &family in families {
        let matching: Vec<_> = rows
            .iter()
            .filter(|row| row["viewportMode"] == "natural" && row["geometry"]["family"] == family)
            .collect();
        assert_eq!(matching.len(), 1, "{family}: {expected}");
        let parameters = serde_json::json!({"family": family});
        let actual = run_with_dom(
            "<!doctype html><html><head></head><body></body></html>",
            &format!(
                "JSON.stringify(({})({parameters}))",
                source.trim().trim_end_matches(';')
            ),
        )
        .unwrap();
        let mut actual: Value = serde_json::from_str(&actual).unwrap();
        let mut geometry = matching[0]["geometry"].clone();
        // The oracle varies DPR independently; every geometric value stays exact.
        assert!(actual
            .as_object_mut()
            .unwrap()
            .remove("devicePixelRatio")
            .is_some());
        assert!(geometry
            .as_object_mut()
            .unwrap()
            .remove("devicePixelRatio")
            .is_some());
        assert_eq!(actual, geometry, "{family}: {actual}");
    }
}

#[test]
fn bundled_font_runs_partial_ranges_splits_and_rotation_match_normal_chrome() {
    bundled_geometry_matches_chrome(
        include_str!("../../../../harness/detection/font-run-geometry-probe.js"),
        include_str!("../../../../harness/detection/fixtures/font-run-geometry-chrome156.json"),
        &["IBM Plex Mono", "Fira Code"],
    );
}

#[test]
fn leading_and_internal_spaces_preserve_boundaries_ranges_and_wrapping_like_chrome() {
    bundled_geometry_matches_chrome(
        include_str!("../../../../harness/detection/leading-space-geometry-probe.js"),
        include_str!(
            "../../../../harness/detection/fixtures/leading-space-geometry-chrome156.json"
        ),
        &["IBM Plex Mono"],
    );
}

#[cfg(target_os = "macos")]
#[test]
fn genuine_inline_borders_empty_nodes_nested_fonts_wrapping_and_range_boxes_match_chrome() {
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    let source = include_str!("../../../../harness/detection/inline-border-layout-probe.js");
    let expected: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/inline-border-layout-chrome155.json"
    ))
    .unwrap();
    assert_eq!(
        expected["probeSha256"],
        format!("{:x}", Sha256::digest(source))
    );
    let actual = run_with_dom(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("JSON.stringify({})", source.trim().trim_end_matches(';')),
    )
    .unwrap();
    let actual: Value = serde_json::from_str(&actual).unwrap();
    if actual != expected["result"] {
        eprintln!("INLINE_ACTUAL {}", serde_json::to_string(&actual).unwrap());
    }
    assert_eq!(actual, expected["result"]);
}
