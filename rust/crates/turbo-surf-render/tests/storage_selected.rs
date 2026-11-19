use turbo_surf_core::request_context::{ClientHints, Destination, RequestContext, RequestMode};
use turbo_surf_render::{BrowserStorage, PageSession};

async fn pages(storage: BrowserStorage) -> (PageSession, u64) {
    let mut page = PageSession::open_with_browsing_state(
        "<!doctype html><body>",
        "https://storage-selection.test/",
        "",
        "",
        5000,
        "",
        ClientHints::default(),
        &[],
        storage,
    )
    .await
    .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    page.eval_expression("window.open('about:blank','storage-peer');void 0")
        .await
        .unwrap();
    let peer = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|entry| entry.target != root.to_string())
        .unwrap()
        .target
        .parse()
        .unwrap();
    page.eval_target_expression(
        peer,
        "sessionStorage.setItem('peer-seed','value');globalThis.seen=[];addEventListener('storage',e=>seen.push([e.key,e.isTrusted]));void 0",
        false,
        false,
    )
    .await
    .unwrap();
    (page, peer)
}

#[tokio::test(flavor = "current_thread")]
async fn selecting_root_preserves_peer_storage_tasks_until_peer_is_selected() {
    let (mut page, peer) = pages(BrowserStorage::default()).await;
    page.eval_expression("localStorage.setItem('peer-task','value');void 0")
        .await
        .unwrap();
    page.eval("globalThis.__RESULT='root-done'").await.unwrap();
    assert_eq!(
        page.read_target_engine(peer, "return seen.length").unwrap(),
        "0"
    );
    assert!(page.pending_storage_events_for_testing() > 0);
    assert_eq!(
        page.eval_target_expression(
            peer,
            "(async()=>{await new Promise(r=>setTimeout(r,0));return JSON.stringify(seen)})()",
            false,
            false
        )
        .await
        .unwrap(),
        "[[\"peer-task\",true]]"
    );
    assert_eq!(page.pending_storage_events_for_testing(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn selected_response_storage_admission_preserves_other_live_recipients() {
    let storage = BrowserStorage::default();
    let (mut page, peer) = pages(storage.clone()).await;
    page.eval_expression("globalThis.clears=0;addEventListener('storage',e=>{if(e.key===null)clears++});localStorage.setItem('seed','value');sessionStorage.setItem('seed','value');void 0").await.unwrap();
    // Drain the earlier peer seed without contaminating the response-only witness.
    page.eval_target_expression(
        peer,
        "(async()=>{await new Promise(r=>setTimeout(r,0));seen=[];return 'ready'})()",
        false,
        false,
    )
    .await
    .unwrap();
    let base = "https://storage-selection.test/";
    storage.response_storage_effects()(
        &RequestContext::resource(base, Destination::Empty, RequestMode::Cors),
        base,
        &[("Clear-Site-Data".into(), "\"storage\"".into())],
    );
    page.eval("globalThis.__RESULT='root-response'")
        .await
        .unwrap();
    assert_eq!(page.eval_expression("clears").await.unwrap(), "2");
    assert_eq!(
        page.read_target_engine(peer, "return seen.length").unwrap(),
        "0"
    );
    assert!(page.pending_storage_events_for_testing() > 0);
    assert_eq!(
        page.eval_target_expression(
            peer,
            "(async()=>{await new Promise(r=>setTimeout(r,0));return JSON.stringify(seen)})()",
            false,
            false
        )
        .await
        .unwrap(),
        "[[null,true],[null,true]]"
    );
    assert_eq!(page.pending_storage_events_for_testing(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn earlier_peer_only_notice_precedes_later_mixed_notice_for_that_peer() {
    let storage = BrowserStorage::default();
    let (mut page, peer) = pages(storage.clone()).await;
    page.eval_target_expression(peer,
        "globalThis.clearOrder=[];addEventListener('storage',e=>{if(e.key===null)clearOrder.push(e.storageArea===localStorage?'local':'session')});void 0",
        false,false).await.unwrap();
    let base = "https://storage-selection.test/";
    let context = RequestContext::resource(base, Destination::Empty, RequestMode::Cors);
    let clear = storage.response_storage_effects();
    // Empty local storage does not notify; the first peer notice is session-only.
    clear(
        &context,
        base,
        &[("Clear-Site-Data".into(), "\"storage\"".into())],
    );
    page.eval_expression("localStorage.setItem('between','value');void 0")
        .await
        .unwrap();
    clear(
        &context,
        base,
        &[("Clear-Site-Data".into(), "\"storage\"".into())],
    );
    page.eval("globalThis.__RESULT='root-drained'")
        .await
        .unwrap();
    assert_eq!(
        page.read_target_engine(peer, "return clearOrder.length")
            .unwrap(),
        "0"
    );
    let result = page.eval_target_expression(peer,
        "(async()=>{await new Promise(r=>setTimeout(r,0));return JSON.stringify(clearOrder)})()",
        false,false).await.unwrap();
    let order: Vec<String> = serde_json::from_str(&result).unwrap();
    assert_eq!(order.len(), 3);
    assert_eq!(order[0], "session");
    assert_eq!(
        order.iter().filter(|area| area.as_str() == "local").count(),
        1
    );
    assert_eq!(page.pending_storage_events_for_testing(), 0);
}
