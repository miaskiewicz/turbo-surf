//! Shared authored localhost control: actual V8 object/realm collection, not
//! merely retired host registry counts. No guest GC API is installed.
use serde_json::Value;
use sha2::{Digest, Sha256};
use turbo_surf_render::PageSession;
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

#[tokio::test(flavor = "current_thread")]
async fn nested_frame_documents_and_functions_retain_only_their_actual_retired_realms() {
    #[cfg(turbo_surf_native_proxy)]
    assert_eq!(
        turbo_surf_render::v8_version(),
        env!("TURBO_SURF_EXPECTED_NATIVE_V8")
    );
    const SOURCE: &str =
        include_str!("../../../../harness/detection/frame-retirement-lifecycle-probe.js");
    const CASES: &str =
        include_str!("../../../../harness/detection/frame-retirement-lifecycle-cases.json");
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/frame-retirement-lifecycle-chrome155.json"
    ))
    .unwrap();
    assert_eq!(control["browser"], "Chrome/155.0.8059.80");
    assert_eq!(control["v8"], "15.5.35.23");
    assert_eq!(control["errors"], serde_json::json!([]));
    for (field, bytes) in [
        ("probeSha256", SOURCE.as_bytes()),
        ("casesSha256", CASES.as_bytes()),
        (
            "collectorSha256",
            include_bytes!("../../../../harness/detection/authored-gc-chrome.mjs").as_slice(),
        ),
    ] {
        assert_eq!(control[field], format!("{:x}", Sha256::digest(bytes)));
    }
    let mut page = PageSession::open("<body>", "https://ownership.test/", "", "", 10000)
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
    let released: Value = serde_json::from_str(
        &page
            .eval_json_expression("__authoredLifecycle.read()")
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        serde_json::json!({"first":first,"afterRelease":released}),
        control["result"][0]["result"]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn collected_frame_context_metadata_is_reclaimed_without_dropping_live_retired_documents() {
    let mut page = PageSession::open("<body>", "https://ownership.test/", "", "", 10000)
        .await
        .unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    page.eval_expression("'initial-boundary'").await.unwrap();
    let baseline = page.context_registry_diagnostics_for_testing();
    page.eval_expression(
        r#"(async()=>{
        globalThis.registryDocuments=[];
        for(let index=0;index<4;index++){
            const frame=document.createElement('iframe');
            const loaded=new Promise(resolve=>frame.addEventListener('load',resolve,{once:true}));
            frame.srcdoc='<body><p>retained-'+index+'</p>';document.body.append(frame);await loaded;
            registryDocuments.push(frame.contentDocument);frame.remove();
        }
        return 'prepared';
    })()"#,
    )
    .await
    .unwrap();
    // Initial about:blank navigation contexts need not be collected during
    // preparation. Compare equivalent post-GC boundaries, retaining all four
    // final Documents until the explicit release below.
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    page.eval_expression("String(registryDocuments.length)")
        .await
        .unwrap();
    let held = page.context_registry_diagnostics_for_testing();
    assert_eq!(held["cleared"], 0, "{held}");
    assert_eq!(
        held["registered"].as_u64().unwrap(),
        baseline["registered"].as_u64().unwrap() + 4,
        "{held}"
    );
    page.eval_expression("registryDocuments.splice(1);'release-three'")
        .await
        .unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    let collected = page.context_registry_diagnostics_for_testing();
    assert_eq!(collected["cleared"], 3, "{collected}");
    assert_eq!(
        page.eval_json_expression(
            "[registryDocuments[0].body.textContent,registryDocuments[0].createElement('b').ownerDocument===registryDocuments[0]]"
        ).await.unwrap(),
        r#"["retained-0",true]"#
    );
    let after = page.context_registry_diagnostics_for_testing();
    assert_eq!(after["cleared"], 0, "{after}");
    assert_eq!(
        after["registered"].as_u64().unwrap(),
        baseline["registered"].as_u64().unwrap() + 1,
        "{after}"
    );
    page.eval_expression("registryDocuments=null;'release-last'")
        .await
        .unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    page.eval_expression("'final-boundary'").await.unwrap();
    assert_eq!(page.context_registry_diagnostics_for_testing(), baseline);
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["hostContexts"],
        0
    );
}
