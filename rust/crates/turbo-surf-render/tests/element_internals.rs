use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn internals_states_and_completed_iterators_retain_only_guest_owned_realms() {
    #[cfg(turbo_surf_native_proxy)]
    assert_eq!(
        turbo_surf_render::v8_version(),
        env!("TURBO_SURF_EXPECTED_NATIVE_V8")
    );
    const CASES: &str =
        include_str!("../../../../harness/detection/element-internals-lifecycle-cases.json");
    const PROBE: &str =
        include_str!("../../../../harness/detection/element-internals-lifecycle-probe.js");
    const CONTROL: &str =
        include_str!("../../../../harness/detection/element-internals-lifecycle-chrome155.json");
    let control: Value = serde_json::from_str(CONTROL).unwrap();
    for (name, bytes) in [
        ("casesSha256", CASES.as_bytes()),
        ("probeSha256", PROBE.as_bytes()),
        (
            "collectorSha256",
            include_bytes!("../../../../harness/detection/element-internals-lifecycle-chrome.mjs")
                .as_slice(),
        ),
    ] {
        assert_eq!(
            control[name],
            format!("{:x}", Sha256::digest(bytes)),
            "stale {name}"
        );
    }
    assert_eq!(control["errors"], serde_json::json!([]));
    let cases: Value = serde_json::from_str(CASES).unwrap();
    let case = &cases[0];
    let headers = case["headers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row[0].as_str().unwrap().to_string(),
                row[1].as_str().unwrap().to_string(),
            )
        })
        .collect::<Vec<_>>();
    let mut page = PageSession::open_with_response_headers(
        case["html"].as_str().unwrap(),
        "http://localhost/control",
        "",
        "",
        15000,
        "",
        Default::default(),
        &headers,
    )
    .await
    .unwrap();
    assert_eq!(
        page.eval_json_expression(PROBE.trim().trim_end_matches(';'))
            .await
            .unwrap(),
        "\"prepared\""
    );
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    let first: Value = serde_json::from_str(
        &page
            .eval_json_expression("__readInternalsLifetime()")
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(first, control["result"][0]["result"]["first"]);
    page.eval_json_expression("globalThis.__internalsLifetime.retained=null; 'released'")
        .await
        .unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    let released: Value = serde_json::from_str(
        &page
            .eval_json_expression("__readInternalsLifetime()")
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(released, control["result"][0]["result"]["afterRelease"]);
    page.close();
}
