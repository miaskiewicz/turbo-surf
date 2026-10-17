#![cfg(target_os = "macos")]
use sha2::{Digest, Sha256};
#[cfg(target_os = "macos")]
#[tokio::test(flavor = "current_thread")]
async fn native_host_media_matches_chrome_and_drives_live_css_without_guest_getters() {
    let probe = include_str!("../../../../harness/detection/host-media-probe.js");
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/host-media-chrome155.json"
    ))
    .unwrap();
    assert_eq!(fixture["browser"], "Chrome/155.0.8059.80");
    assert_eq!(
        fixture["probeSha256"],
        format!("{:x}", Sha256::digest(probe.as_bytes()))
    );
    let mut page = turbo_surf_render::PageSession::open(
        "<!doctype html><head></head><body>",
        "https://example.test/",
        "",
        "",
        // This is a host/CSS conformance fixture, not an execution-budget test.
        60_000,
    )
    .await
    .unwrap();
    let actual: serde_json::Value = serde_json::from_str(
        &page
            .eval_json_expression(probe.trim().trim_end_matches(';'))
            .await
            .unwrap(),
    )
    .unwrap();
    if std::env::var_os("TURBO_SURF_VALIDATE_SAVED_HOST_MEDIA").is_some() {
        assert_eq!(actual, fixture["result"]);
    }
    let rows = actual["matches"].as_array().unwrap();
    let matches = |query: &str| {
        rows.iter().find(|row| row[0] == query).unwrap()[1]
            .as_bool()
            .unwrap()
    };
    for query in [
        "(forced-colors: none)",
        "not (forced-colors: active)",
        "(prefers-color-scheme)",
        "(hover: hover)",
        "(pointer: fine)",
        "(any-hover: hover)",
        "(any-pointer: fine)",
    ] {
        assert!(matches(query), "{query}");
    }
    assert!(!matches("(forced-colors: active)"));
    assert_eq!(actual["box"], serde_json::json!([20, 50]));
    assert_eq!(
        matches("(prefers-contrast: no-preference)"),
        !matches("(prefers-contrast: more)")
    );
    assert_eq!(
        matches("(prefers-color-scheme: light)"),
        !matches("(prefers-color-scheme: dark)")
    );
    for kind in ["prefers-reduced-motion", "prefers-reduced-transparency"] {
        assert_eq!(
            matches(&format!("({kind})")),
            matches(&format!("({kind}: reduce)"))
        );
        assert_eq!(
            matches(&format!("({kind}: no-preference)")),
            !matches(&format!("({kind}: reduce)"))
        );
    }
    let value=page.eval_json_expression("(()=>{Object.defineProperty(globalThis,'innerWidth',{value:1,configurable:true});return matchMedia('(forced-colors:none) and (min-width:100px)').matches})()").await.unwrap();
    assert_eq!(value, "true");
    page.close();
}
