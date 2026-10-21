use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

fn control(source: &str, fixture: &str, collector: &[u8]) -> Value {
    let control: Value = serde_json::from_str(fixture).unwrap();
    assert_eq!(control["browser"], "Chrome/155.0.8059.80");
    assert_eq!(control["v8"], "15.5.35.23");
    assert_eq!(control["errors"], serde_json::json!([]));
    assert_eq!(
        control["probeSha256"],
        format!("{:x}", Sha256::digest(source))
    );
    assert_eq!(
        control["collectorSha256"],
        format!("{:x}", Sha256::digest(collector))
    );
    control
}

async fn compare(source: &str, fixture: &str) {
    let expected = control(
        source,
        fixture,
        include_bytes!("../../../../harness/detection/authored-dom-chrome.mjs"),
    );
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://ranges.test/",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let actual: Value = serde_json::from_str(
        &page
            .eval_json_expression(source.trim().trim_end_matches(';'))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(actual, expected["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn abstract_node_and_static_range_interfaces_keep_native_boundary_semantics() {
    compare(
        include_str!("../src/abstract_ranges_probe.js"),
        include_str!("../src/abstract_ranges_chrome155.json"),
    )
    .await;
}

#[cfg(turbo_surf_native_proxy)]
#[tokio::test(flavor = "current_thread")]
async fn static_boundaries_and_borrowed_accessors_survive_real_iframe_retirement() {
    compare(
        include_str!("../src/abstract_ranges_frames_probe.js"),
        include_str!("../src/abstract_ranges_frames_chrome155.json"),
    )
    .await;
}

#[cfg(turbo_surf_native_proxy)]
#[tokio::test(flavor = "current_thread")]
async fn static_ranges_retain_native_node_realms_only_while_guest_reachable() {
    const SOURCE: &str =
        include_str!("../../../../harness/detection/abstract-ranges-lifecycle-probe.js");
    const FIXTURE: &str = include_str!(
        "../../../../harness/detection/fixtures/abstract-ranges-lifecycle-chrome155.json"
    );
    const CASES: &str =
        include_str!("../../../../harness/detection/abstract-ranges-lifecycle-cases.json");
    let expected = control(
        SOURCE,
        FIXTURE,
        include_bytes!("../../../../harness/detection/authored-gc-chrome.mjs"),
    );
    assert_eq!(
        expected["casesSha256"],
        format!("{:x}", Sha256::digest(CASES))
    );
    let mut page = PageSession::open(
        "<!doctype html><html><head></head><body></body></html>",
        "https://ranges.test/",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    assert_eq!(
        page.eval_json_expression(SOURCE.trim().trim_end_matches(';'))
            .await
            .unwrap(),
        "\"prepared\""
    );
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    let first: Value = serde_json::from_str(
        &page
            .eval_json_expression("__authoredLifecycle.read()")
            .await
            .unwrap(),
    )
    .unwrap();
    page.eval_expression("__authoredLifecycle.release();'released'")
        .await
        .unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    let after: Value = serde_json::from_str(
        &page
            .eval_json_expression("__authoredLifecycle.read()")
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        serde_json::json!({"first":first,"afterRelease":after}),
        expected["result"][0]["result"]
    );
}
