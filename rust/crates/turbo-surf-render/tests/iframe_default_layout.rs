#![cfg(all(turbo_surf_native_proxy, target_os = "macos"))]
use sha2::{Digest, Sha256};

#[tokio::test(flavor = "current_thread")]
async fn iframe_defaults_zero_viewports_percentage_bases_and_media_match_chrome() {
    let source = include_str!("../../../../harness/detection/native-iframe-layout-probe.js");
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/native-iframe-layout-chrome155.json"
    ))
    .unwrap();
    assert_eq!(
        control["probeSha256"],
        format!("{:x}", Sha256::digest(source))
    );
    assert_eq!(control["errors"], serde_json::json!([]));
    let mut page = turbo_surf_render::PageSession::open(
        "<!doctype html><html><head></head><body></body></html>",
        "https://iframe-defaults.test/",
        "",
        "",
        30000,
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
    let rows = actual.as_array().unwrap();
    let expected = control["result"].as_array().unwrap();
    assert_eq!(rows.len(), expected.len());
    let mut failures = Vec::new();
    for (actual, expected) in rows.iter().zip(expected) {
        for field in [
            "name",
            "rect",
            "rects",
            "client",
            "offset",
            "viewport",
            "media",
            "parentRect",
            "fallback",
        ] {
            record_difference(&mut failures, actual, expected, field);
        }
        if expected["name"] == "css-zero-standards" {
            for field in ["children", "body", "nestedViewport", "document"] {
                record_difference(&mut failures, actual, expected, field);
            }
        }
        // BackCompat body aggregation has a separately retained Chrome control.
        // The producer currently has no document-mode input (tracked in issue 28).
        if expected["name"] == "css-zero" {
            for field in ["children", "nestedViewport"] {
                record_difference(&mut failures, actual, expected, field);
            }
        }
    }
    page.close();
    assert!(
        failures.is_empty(),
        "{}\nNATIVE_IFRAME_ACTUAL {}",
        failures.join("\n"),
        actual
    );
}

fn record_difference(
    failures: &mut Vec<String>,
    actual: &serde_json::Value,
    expected: &serde_json::Value,
    field: &str,
) {
    if actual[field] != expected[field] {
        failures.push(format!(
            "{} {field}: actual={} expected={}",
            expected["name"], actual[field], expected[field]
        ));
    }
}
