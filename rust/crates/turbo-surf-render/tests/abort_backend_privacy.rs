use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn abort_any_does_not_expose_raw_signals_through_page_intrinsics() {
    let probe = include_str!("../../../../harness/detection/abort-any-private-probe.js");
    let expected: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/abort-any-private-chrome155.json"
    ))
    .unwrap();
    assert_eq!(expected["errors"], serde_json::json!([]));
    assert_eq!(expected["browser"], "Chrome/155.0.8059.80");
    assert_eq!(
        expected["probeSha256"],
        format!("{:x}", Sha256::digest(probe.as_bytes()))
    );
    let mut page = PageSession::open("<body>", "http://localhost/", "", "", 10000)
        .await
        .unwrap();
    let actual: Value = serde_json::from_str(
        &page
            .eval_json_expression(probe.trim().trim_end_matches(';'))
            .await
            .unwrap(),
    )
    .unwrap();
    let control = &expected["result"][0]["result"];
    for field in [
        "arrayFrom",
        "numericSetter",
        "descriptorAttack",
        "conversions",
        "sequence",
        "firstReason",
        "propagation",
    ] {
        assert_eq!(actual[field], control[field], "{field}");
    }
    assert_eq!(
        actual["arrayIterator"]["rawExposed"],
        control["arrayIterator"]["rawExposed"]
    );
    // Chrome's array fast path skips an inherited iterator. This wrapper still
    // calls the public input iterator once; that separate behavior stays visible.
    assert_eq!(actual["arrayIterator"]["calls"], 1);
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn stream_controller_wrapping_ignores_hostile_array_includes() {
    let probe = include_str!("../../../../harness/detection/stream-controller-private-probe.js");
    let expected: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/stream-controller-private-chrome155.json"
    ))
    .unwrap();
    assert_eq!(expected["errors"], serde_json::json!([]));
    assert_eq!(expected["browser"], "Chrome/155.0.8059.80");
    assert_eq!(
        expected["probeSha256"],
        format!("{:x}", Sha256::digest(probe.as_bytes()))
    );
    let mut page = PageSession::open("<body>", "http://localhost/", "", "", 10000)
        .await
        .unwrap();
    let actual: Value = serde_json::from_str(
        &page
            .eval_json_expression(probe.trim().trim_end_matches(';'))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(actual, expected["result"][0]["result"]);
    page.close();
}
