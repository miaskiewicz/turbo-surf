use super::*;

async fn page() -> PageSession {
    PageSession::open(
        "<title>root</title>",
        "https://history.test/start",
        "",
        "",
        5000,
    )
    .await
    .unwrap()
}

fn value(page: &mut PageSession, source: &str) -> v8::Global<v8::Value> {
    let target = page.auxiliary_root_id().unwrap();
    let _activation = page.rt.activation_key().enter();
    auxiliary_pages::execute_target(&mut page.rt, AuxiliaryContextId(target), source, false)
        .unwrap()
}

fn revision(page: &PageSession, target: u64) -> u64 {
    let state = page.rt.op_state();
    let state = state.borrow();
    state
        .borrow::<Histories>()
        .targets
        .get(&target)
        .unwrap()
        .revision()
}

fn rejected_precommit(page: &mut PageSession, source: &str) -> JsErrorBox {
    let document = value(page, "document");
    let entry = value(page, source);
    let _activation = page.rt.activation_key().enter();
    deno_core::scope!(scope, page.rt);
    let document = v8::Local::new(scope, document);
    let document = v8::Local::<v8::Object>::try_from(document).unwrap();
    let context = document.get_creation_context(scope).unwrap();
    let scope = &mut v8::ContextScope::new(scope, context);
    let entry = v8::Local::new(scope, entry);
    let entry = v8::Local::<v8::Object>::try_from(entry).unwrap();
    match precommit(scope, document, "push", entry) {
        Err(error) => error,
        Ok(_) => panic!("outer native history mutation unexpectedly committed"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn serialization_reentry_preserves_inner_commit_and_rejects_stale_outer_mutation() {
    let mut page = page().await;
    let root = page.auxiliary_root_id().unwrap();
    page.eval_target_expression(root, "history.replaceState({baseline:true},'', '/start');globalThis.peer=window.open('', 'history-peer');peer.document.title='healthy';globalThis.historyPayloadReads=0", false, false).await.unwrap();
    let peer = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|page| page.source_page.as_deref() == Some(&root.to_string()) && !page.closed)
        .unwrap()
        .target
        .parse::<u64>()
        .unwrap();
    let before = revision(&page, root);
    let error = rejected_precommit(
        &mut page,
        r#"({__proto__:null,url:'https://history.test/outer',key:'outer-key',id:'outer-id',scroll:'auto',payload:{__proto__:null,state:{get outer(){historyPayloadReads++;history.pushState({inner:true},'', '/inner');return true}},navState:undefined}})"#,
    );
    assert_eq!(
        error.to_string(),
        "Native history changed during state serialization"
    );
    assert_eq!(revision(&page, root), before + 1);
    assert_eq!(page.eval_target_expression(root, "[historyPayloadReads,history.length,history.state.inner,history.state.outer===undefined,location.pathname]", false, true).await.unwrap(), "[1,2,true,true,\"/inner\"]");
    assert_eq!(
        page.eval_target_expression(peer, "document.title", false, false)
            .await
            .unwrap(),
        "healthy"
    );
    assert_eq!(page.eval_target_expression(root, "history.pushState({recovered:true},'', '/recovered');[history.length,history.state.recovered,location.pathname]", false, true).await.unwrap(), "[3,true,\"/recovered\"]");
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn native_history_url_guard_denies_cross_origin_atomically_and_allows_recovery() {
    let mut page = page().await;
    let root = page.auxiliary_root_id().unwrap();
    page.eval_target_expression(
        root,
        "history.replaceState({baseline:true},'', '/start')",
        false,
        false,
    )
    .await
    .unwrap();
    let before = revision(&page, root);
    let error = rejected_precommit(
        &mut page,
        r#"({__proto__:null,url:'https://other.test/forbidden',key:'denied-key',id:'denied-id',scroll:'auto',payload:{__proto__:null,state:{forbidden:true},navState:undefined}})"#,
    );
    assert_eq!(error.get_class(), "DOMExceptionSecurityError");
    assert_eq!(
        error.to_string(),
        "History URL cannot be rewritten by this document"
    );
    assert_eq!(revision(&page, root), before);
    assert_eq!(page.eval_target_expression(root, "[history.length,history.state.baseline,history.state.forbidden===undefined,location.href]", false, true).await.unwrap(), "[1,true,true,\"https://history.test/start\"]");
    assert_eq!(page.eval_target_expression(root, "history.pushState({recovered:true},'', '/recovered');[history.length,history.state.recovered,location.pathname]", false, true).await.unwrap(), "[2,true,\"/recovered\"]");
    page.close();
}
