//! Shared authored localhost control: actual V8 object/realm collection, not
//! merely retired host registry counts. No guest GC API is installed.
use serde_json::Value;
#[tokio::test(flavor = "current_thread")]
async fn shared_chrome_frame_ownership_oracle_preserves_retained_documents_and_collects_released_realms(
) {
    let mut page =
        turbo_surf_render::PageSession::open("<body>", "https://ownership.test/", "", "", 10000)
            .await
            .unwrap();
    let source = include_str!("../../../../harness/detection/frame-message-lifetime-probe.js");
    page.eval(&(source.to_owned() + ";globalThis.__RESULT='prepared';"))
        .await
        .unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    let oracle: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/frame-message-lifetime-chrome155.json"
    ))
    .unwrap();
    let first: Value = serde_json::from_str(
        &page
            .eval_expression("JSON.stringify(__readPortLifetime())")
            .await
            .unwrap(),
    )
    .unwrap();
    if first != oracle["first"] {
        if let Some(path) = std::env::var_os("TURBO_SURF_GC_HEAP_SNAPSHOT") {
            page.eval_expression("(()=>{const doc=__portLifetimeState.rows[0].refs[1].deref();if(doc)doc.__nativeLifecycleDiagnostic='native155-removed-document';return 'tagged';})()").await.unwrap();
            std::fs::write(path, page.native_heap_snapshot_for_testing()).unwrap();
        }
    }
    assert_eq!(first, oracle["first"]);
    page.eval_expression("__portLifetimeState.retained=null; 'released'")
        .await
        .unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    let second: Value = serde_json::from_str(
        &page
            .eval_expression("JSON.stringify(__readPortLifetime())")
            .await
            .unwrap(),
    )
    .unwrap();
    if second != oracle["afterRelease"] {
        if let Some(path) = std::env::var_os("TURBO_SURF_GC_HEAP_SNAPSHOT") {
            page.eval_expression("(()=>{const rows=__portLifetimeState.rows; const doc=rows[rows.length-1].refs[1].deref();if(doc)doc.__nativeLifecycleDiagnostic='released-iframe-document';return 'tagged';})()")
                .await.unwrap();
            std::fs::write(path, page.native_heap_snapshot_for_testing()).unwrap();
        }
    }
    assert_eq!(second, oracle["afterRelease"]);
}

#[tokio::test(flavor = "current_thread")]
async fn shared_chrome_frame_owner_getter_has_real_receiver_and_retirement_state() {
    let mut page =
        turbo_surf_render::PageSession::open("<body>", "https://ownership.test/", "", "", 10000)
            .await
            .unwrap();
    let result = page
        .eval_json_expression(include_str!(
            "../../../../harness/detection/frame-owner-element-probe.js"
        ))
        .await
        .unwrap();
    let oracle: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/frame-owner-element-chrome155.json"
    ))
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&result).unwrap(),
        oracle["result"]
    );
}
