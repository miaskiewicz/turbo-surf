use turbo_surf_core::request_context::{ClientHints, Destination, RequestContext, RequestMode};
use turbo_surf_render::{BrowserStorage, PageSession};

#[tokio::test(flavor = "current_thread")]
async fn response_clear_survives_a_full_multi_recipient_task_queue() {
    let storage = BrowserStorage::default();
    let base = "https://storage-inbox.test/";
    let mut page = PageSession::open_with_browsing_state(
        "<!doctype html><body>",
        base,
        "",
        "",
        30000,
        "",
        ClientHints::default(),
        &[],
        storage.clone(),
    )
    .await
    .unwrap();
    page.eval(r#"(async()=>{
      globalThis.clearCounts=[0,0,0];sessionStorage.setItem('seed','value');
      const frames=[];
      for(let i=0;i<2;i++) {
        const frame=document.createElement('iframe');
        const loaded=new Promise(resolve=>frame.onload=resolve);
        document.body.append(frame);await loaded;frames.push(frame);
        frame.contentWindow.addEventListener('storage',event=>{if(event.key===null)clearCounts[i+1]++});
      }
      addEventListener('storage',event=>{if(event.key===null)clearCounts[0]++});
      globalThis.__RESULT='ready';
    })()"#).await.unwrap();
    page.eval_expression("(()=>{for(let i=0;i<4095;i++)localStorage.setItem('key'+i,'value');return localStorage.length})()")
        .await.unwrap();
    assert_eq!(page.pending_storage_events_for_testing(), 8190);
    let context = RequestContext::resource(base, Destination::Empty, RequestMode::Cors);
    storage.response_storage_effects()(
        &context,
        base,
        &[("Clear-Site-Data".into(), "\"storage\"".into())],
    );
    page.eval_expression("globalThis.lateClears=0;globalThis.lateFrame=document.createElement('iframe');document.body.append(lateFrame);lateFrame.contentWindow.addEventListener('storage',event=>{if(event.key===null)lateClears++})").await.unwrap();
    page.eval("globalThis.__RESULT='drained'").await.unwrap();
    assert_eq!(
        page.eval_expression("JSON.stringify(clearCounts)")
            .await
            .unwrap(),
        "[2,2,2]"
    );
    assert_eq!(page.eval_expression("lateClears").await.unwrap(), "0");
    assert_eq!(page.pending_storage_events_for_testing(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn accepted_response_notices_survive_inbox_overflow() {
    let storage = BrowserStorage::default();
    let base = "https://storage-overflow.test/";
    let mut page = PageSession::open_with_browsing_state(
        "<!doctype html><body>",
        base,
        "",
        "",
        30000,
        "",
        ClientHints::default(),
        &[],
        storage.clone(),
    )
    .await
    .unwrap();
    page.eval_expression("globalThis.clears=0;sessionStorage.setItem('seed','value');addEventListener('storage',event=>{if(event.key===null)clears++})").await.unwrap();
    let context = RequestContext::resource(base, Destination::Empty, RequestMode::Cors);
    let callback = storage.response_storage_effects();
    for _ in 0..8193 {
        callback(
            &context,
            base,
            &[("Clear-Site-Data".into(), "\"storage\"".into())],
        );
    }
    // The first checkpoint reports overflow; the accepted notices stay queued.
    let error = page
        .eval_engine_expression("__tsNs.runTimers()")
        .await
        .unwrap_err();
    assert!(
        error.contains("RangeError: Pending storage event budget exceeded"),
        "{error}"
    );
    assert_eq!(page.pending_storage_events_for_testing(), 8192);
    page.eval("globalThis.__RESULT='recovered'").await.unwrap();
    assert_eq!(page.eval_expression("clears").await.unwrap(), "8192");
    assert_eq!(page.pending_storage_events_for_testing(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn unrelated_origin_notices_do_not_stall_matching_delivery() {
    let storage = BrowserStorage::default();
    let first = "https://first.storage-inbox.test/";
    let second = "https://second.storage-inbox.test/";
    let mut pages = Vec::new();
    for base in [first, second] {
        let mut page = PageSession::open_with_browsing_state(
            "<!doctype html><body>",
            base,
            "",
            "",
            30000,
            "",
            ClientHints::default(),
            &[],
            storage.clone(),
        )
        .await
        .unwrap();
        page.eval_expression("globalThis.clears=0;localStorage.setItem('seed','value');sessionStorage.setItem('seed','value');addEventListener('storage',event=>{if(event.key===null)clears++})").await.unwrap();
        pages.push(page);
    }
    let callback = storage.response_storage_effects();
    for base in [second, first] {
        let context = RequestContext::resource(base, Destination::Empty, RequestMode::Cors);
        callback(
            &context,
            base,
            &[("Clear-Site-Data".into(), "\"storage\"".into())],
        );
    }
    let page = &mut pages[0];
    assert_eq!(page.pending_storage_events_for_testing(), 4);
    page.eval("globalThis.__RESULT='drained'").await.unwrap();
    assert_eq!(page.eval_expression("clears").await.unwrap(), "2");
    assert_eq!(page.pending_storage_events_for_testing(), 0);
}
