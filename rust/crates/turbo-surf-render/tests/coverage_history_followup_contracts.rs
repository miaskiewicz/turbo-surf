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
async fn actor_history_storage_budget_rejects_peer_growth_and_releases_replaced_state() {
    let server = fixture::Server::start().await;
    let mut page = fixture::open(
        &server.origin,
        AuxiliaryHostServices {
            policy: AuxiliaryResourcePolicy {
                max_history_state_bytes: 4096,
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .await;
    let root = page.auxiliary_root_id().unwrap();
    let peer = fixture::child(&mut page).await;
    page.navigate_target(peer, &server.url("/peer"), "replace")
        .await
        .unwrap();
    fixture::settle(&mut page).await;
    page.eval_target_expression(
        root,
        "history.replaceState('r'.repeat(2800),'', '/root-kept')",
        false,
        false,
    )
    .await
    .unwrap();
    page.eval_target_expression(
        peer,
        "history.replaceState({peer:true},'', '/peer-kept')",
        false,
        false,
    )
    .await
    .unwrap();
    let failure = observe(&mut page, peer, "(()=>{try{history.pushState('p'.repeat(2800),'', '/peer-rejected');return ['accepted']}catch(error){return [error.name,error.message]}})()").await;
    assert_eq!(failure, json!(["DataCloneError", "Failed to execute 'pushState' on 'History': Actor session history state storage budget exceeded"]));
    assert_eq!(
        observe(
            &mut page,
            root,
            "[history.length,history.state.length,location.pathname]"
        )
        .await,
        json!([1, 2800, "/root-kept"])
    );
    assert_eq!(
        observe(
            &mut page,
            peer,
            "[history.length,history.state,location.pathname,document.title]"
        )
        .await,
        json!([1,{"peer":true},"/peer-kept","peer"])
    );
    page.eval_target_expression(
        root,
        "history.replaceState({released:true},'', '/root-released')",
        false,
        false,
    )
    .await
    .unwrap();
    page.eval_target_expression(
        peer,
        "history.pushState('p'.repeat(2800),'', '/peer-recovered')",
        false,
        false,
    )
    .await
    .unwrap();
    assert_eq!(
        observe(
            &mut page,
            peer,
            "[history.length,history.state.length,location.pathname]"
        )
        .await,
        json!([2, 2800, "/peer-recovered"])
    );
    assert_eq!(
        observe(
            &mut page,
            root,
            "[history.length,history.state.released,location.pathname]"
        )
        .await,
        json!([1, true, "/root-released"])
    );
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|request| request.target == "/peer")
            .count(),
        1
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn driver_traversal_timeout_preserves_selection_and_allows_a_healthy_retry() {
    let history = NativeHistoryLog::default();
    let mut page = fixture::open(
        "https://history.test",
        AuxiliaryHostServices {
            history_log: history.clone(),
            ..Default::default()
        },
    )
    .await;
    let root = page.auxiliary_root_id().unwrap();
    let peer = fixture::child(&mut page).await;
    page.eval_target_expression(root, "history.replaceState({n:0},'', '/zero');history.pushState({n:1},'', '/one');history.pushState({n:2},'', '/two');globalThis.originalDocument=document;globalThis.blockTraversal=true;navigation.addEventListener('navigate',event=>{if(blockTraversal&&event.navigationType==='traverse'){blockTraversal=false;while(true){}}})", false, false).await.unwrap();
    let before = history.snapshot(&root.to_string()).unwrap();
    let error = page
        .navigate_target_with_options(root, "", "back", r#"{"timeoutMs":50}"#)
        .await
        .unwrap_err();
    assert!(
        error.contains("Native history traversal timeout after 50ms"),
        "{error}"
    );
    let after = history.snapshot(&root.to_string()).unwrap();
    assert_eq!(after["document"], before["document"]);
    assert_eq!(after["index"], before["index"]);
    assert_eq!(after["entries"], before["entries"]);
    assert_eq!(observe(&mut page, root, "[history.length,history.state.n,location.pathname,document===originalDocument,blockTraversal]").await, json!([3,2,"/two",true,false]));
    assert_eq!(observe(&mut page, peer, "document.title").await, "original");
    let receipt: Value =
        serde_json::from_str(&page.navigate_target(root, "", "back").await.unwrap()).unwrap();
    assert_eq!(receipt["sameDocument"], true);
    assert_eq!(
        observe(
            &mut page,
            root,
            "[history.length,history.state.n,location.pathname,document===originalDocument]"
        )
        .await,
        json!([3, 1, "/one", true])
    );
    assert_eq!(observe(&mut page, peer, "document.title").await, "original");
    page.close();
}
