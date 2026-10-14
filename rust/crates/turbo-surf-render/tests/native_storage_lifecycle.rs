//! Actual V8 collection schedules bounded native reclamation at host boundaries.
use serde_json::json;
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn discarded_native_storage_is_reclaimed_while_range_and_subtree_owners_survive() {
    let mut page = PageSession::open("<body>", "https://storage.test/", "", "", 5000)
        .await
        .unwrap();
    page.eval_expression(
        r#"(()=>{
        globalThis.retained=document.createElement('section');
        const child=retained.appendChild(document.createElement('span'));
        child.textContent='retained';child.marker=41;globalThis.held=child;
        globalThis.range=document.createRange();
        const text=document.createTextNode('range owner');range.selectNodeContents(text);
        for(let i=0;i<600;i++){
            const node=document.createElement('p');node.textContent='x'.repeat(512);
            document.body.appendChild(node);node.remove();
        }
        return 'created';
    })()"#,
    )
    .await
    .unwrap();
    let before = page.native_storage_diagnostics_for_testing();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    // The deterministic V8 GC marks pending work, but does not itself walk Trees.
    let pending = page.native_storage_diagnostics_for_testing();
    assert_eq!(pending["collections"], before["collections"]);
    assert_eq!(pending["pending"], true);
    assert_eq!(
        page.eval_json_expression(
            "[held===retained.firstChild,held.marker,held.textContent,range.toString()]"
        )
        .await
        .unwrap(),
        json!([true, 41, "retained", "range owner"]).to_string()
    );
    let after = page.native_storage_diagnostics_for_testing();
    assert!(
        after["collections"].as_u64().unwrap() > before["collections"].as_u64().unwrap(),
        "{after}"
    );
    assert!(after["reclaimedNodes"].as_u64().unwrap() >= 1200, "{after}");
    assert_eq!(after["rejected"], 0, "{after}");
    let documents = after["last"]["documents"].as_array().unwrap();
    assert!(
        documents
            .iter()
            .any(|row| row["afterPayloadBytes"].as_u64().unwrap()
                < row["beforePayloadBytes"].as_u64().unwrap()),
        "{after}"
    );
    page.eval_expression(
        "globalThis.held=null;globalThis.retained=null;globalThis.range=null;'released'",
    )
    .await
    .unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    page.eval_expression("document.body.tagName").await.unwrap();
    let released = page.native_storage_diagnostics_for_testing();
    assert!(
        released["reclaimedNodes"].as_u64().unwrap() > after["reclaimedNodes"].as_u64().unwrap(),
        "{released}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn native_collection_and_teardown_belong_only_to_their_actual_session() {
    let mut first = PageSession::open("<body>first", "https://first.test/", "", "", 5000)
        .await
        .unwrap();
    let mut second = PageSession::open("<body><p>second</p>", "https://second.test/", "", "", 5000)
        .await
        .unwrap();
    second
        .eval_expression("globalThis.held=document.querySelector('p');held.marker=53;'ready'")
        .await
        .unwrap();
    let peer = second.native_storage_diagnostics_for_testing();
    first.eval_expression("for(let i=0;i<100;i++){const el=document.createElement('i');el.textContent='garbage';document.body.append(el);el.remove();}'created'").await.unwrap();
    first.collect_message_port_garbage_for_testing();
    first
        .eval_expression("document.body.textContent")
        .await
        .unwrap();
    assert!(
        first.native_storage_diagnostics_for_testing()["reclaimedNodes"]
            .as_u64()
            .unwrap()
            >= 200
    );
    assert_eq!(
        second.native_storage_diagnostics_for_testing()["collections"],
        peer["collections"]
    );
    first.close();
    assert_eq!(
        second
            .eval_json_expression("[document.URL,held===document.querySelector('p'),held.marker]")
            .await
            .unwrap(),
        json!(["https://second.test/", true, 53]).to_string()
    );
}
