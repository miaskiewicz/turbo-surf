#[path = "support/popup_contract.rs"]
pub mod fixture;
use serde_json::{json, Value};
use turbo_surf_render::{
    AuxiliaryHostServices, AuxiliaryResourcePolicy, NativeHistoryLog, PageSession,
};

async fn observe(page: &mut PageSession, target: u64, expression: &str) -> Value {
    serde_json::from_str(
        &page
            .eval_target_expression(target, expression, false, true)
            .await
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn driver_same_document_traversal_preserves_identity_and_reports_selected_rows() {
    let server = fixture::Server::start().await;
    let history = NativeHistoryLog::default();
    let mut page = fixture::open(
        &server.origin,
        AuxiliaryHostServices {
            history_log: history.clone(),
            ..Default::default()
        },
    )
    .await;
    let root = page.auxiliary_root_id().unwrap();
    let peer = fixture::child(&mut page).await;
    page.eval_target_expression(root, "globalThis.originalDocument=document;globalThis.originalMarker={};globalThis.mark=originalMarker;globalThis.pops=[];addEventListener('popstate',event=>pops.push(event.state.n));history.replaceState({n:0},'', '/zero');history.pushState({n:1},'', '/one');history.pushState({n:2},'', '/two')", false, false).await.unwrap();
    let before = history.snapshot(&root.to_string()).unwrap();
    for (mode, n, path) in [("back", 1, "/one"), ("forward", 2, "/two")] {
        let receipt: Value =
            serde_json::from_str(&page.navigate_target(root, "", mode).await.unwrap()).unwrap();
        let selected = history.snapshot(&root.to_string()).unwrap();
        assert_eq!(receipt["sameDocument"], true);
        assert_eq!(receipt["finalUrl"], server.url(path));
        let index = selected["index"].as_u64().unwrap() as usize;
        assert_eq!(receipt["historyRow"], selected["entries"][index]["row"]);
        assert_eq!(selected["document"], before["document"]);
        assert_eq!(observe(&mut page, root, "[history.length,history.state.n,location.pathname,document===originalDocument,mark===originalMarker]").await, json!([3,n,path,true,true]));
    }
    assert_eq!(observe(&mut page, root, "pops").await, json!([1, 2]));
    assert_eq!(observe(&mut page, peer, "document.title").await, "original");
    assert!(server.requests().is_empty());
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn public_history_queue_limit_rejects_only_the_excess_task_and_recovers() {
    let mut page = fixture::open(
        "https://history.test",
        AuxiliaryHostServices {
            policy: AuxiliaryResourcePolicy {
                max_pending_navigations: 1,
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .await;
    let root = page.auxiliary_root_id().unwrap();
    let peer = fixture::child(&mut page).await;
    page.eval_target_expression(root, "history.replaceState({n:0},'', '/zero');history.pushState({n:1},'', '/one');history.pushState({n:2},'', '/two');globalThis.pops=[];addEventListener('popstate',event=>pops.push(event.state.n));globalThis.queueFailure='';history.back();try{history.back()}catch(error){queueFailure=error.message}", false, false).await.unwrap();
    fixture::settle(&mut page).await;
    assert_eq!(
        observe(
            &mut page,
            root,
            "[queueFailure,history.length,history.state.n,location.pathname,pops]"
        )
        .await,
        json!(["Native history task budget exceeded", 3, 1, "/one", [1]])
    );
    page.eval_target_expression(root, "history.forward()", false, false)
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    assert_eq!(
        observe(
            &mut page,
            root,
            "[history.length,history.state.n,location.pathname,pops]"
        )
        .await,
        json!([3, 2, "/two", [1, 2]])
    );
    assert_eq!(observe(&mut page, peer, "document.title").await, "original");
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn public_zero_delta_reload_refetches_and_materializes_the_selected_state_graph() {
    let server = fixture::Server::start().await;
    let history = NativeHistoryLog::default();
    let mut page = fixture::open(
        &server.origin,
        AuxiliaryHostServices {
            history_log: history.clone(),
            ..Default::default()
        },
    )
    .await;
    let root = page.auxiliary_root_id().unwrap();
    let peer = fixture::child(&mut page).await;
    page.eval_target_expression(root, "const shared={n:7};history.replaceState({shared,again:shared,map:new Map([['entry',shared]]),date:new Date('2020-01-02T00:00:00Z'),bytes:new Uint8Array([0,128,255])},'', '/reload');globalThis.retiredMarker=true", false, false).await.unwrap();
    let before = history.snapshot(&root.to_string()).unwrap();
    page.eval_target_expression(root, "history.go(0)", false, false)
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    let after = history.snapshot(&root.to_string()).unwrap();
    assert_ne!(after["document"], before["document"]);
    assert_eq!(after["group"], before["group"]);
    let before_index = before["index"].as_u64().unwrap() as usize;
    let after_index = after["index"].as_u64().unwrap() as usize;
    assert_eq!(
        after["entries"][after_index]["row"],
        before["entries"][before_index]["row"]
    );
    assert_eq!(observe(&mut page, root, "[document.title,location.pathname,history.length,typeof retiredMarker,history.state.shared.n,history.state.shared===history.state.again,history.state.map.get('entry')===history.state.shared,history.state.date.toISOString(),Array.from(history.state.bytes)]").await, json!(["reload","/reload",1,"undefined",7,true,true,"2020-01-02T00:00:00.000Z",[0,128,255]]));
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|request| request.target == "/reload" && request.method == "GET")
            .count(),
        1
    );
    assert_eq!(observe(&mut page, peer, "document.title").await, "original");
    page.close();
}
