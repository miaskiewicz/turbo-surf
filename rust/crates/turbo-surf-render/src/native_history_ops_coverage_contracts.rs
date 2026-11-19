use super::*;

async fn page() -> PageSession {
    let mut page = PageSession::open(
        "<title>root</title><iframe id=child srcdoc='<title>child</title>'></iframe>",
        "https://history.test/start",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval_json_expression("history.replaceState({root:true},'', '/stable');true")
        .await
        .unwrap();
    page
}

fn value(page: &mut PageSession, source: &str) -> v8::Global<v8::Value> {
    let target = page.auxiliary_root_id().unwrap();
    let _activation = page.rt.activation_key().enter();
    auxiliary_pages::execute_target(&mut page.rt, AuxiliaryContextId(target), source, false)
        .unwrap()
}

fn revision(page: &mut PageSession) -> u64 {
    let target = page.auxiliary_root_id().unwrap();
    let state = page.rt.op_state();
    let state = state.borrow();
    state
        .borrow::<Histories>()
        .targets
        .get(&target)
        .unwrap()
        .revision()
}

fn record_value<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    record: v8::Local<'s, v8::Value>,
) -> v8::Local<'s, v8::Value> {
    let record = record.to_object(scope).unwrap();
    assert!(record.get_prototype(scope).unwrap().is_null());
    let key = v8::String::new(scope, "ok").unwrap();
    assert!(record.get(scope, key.into()).unwrap().is_true());
    let key = v8::String::new(scope, "value").unwrap();
    record.get(scope, key.into()).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn native_history_adapter_routes_actual_child_documents_without_mutating_root() {
    let mut page = page().await;
    let root_document = value(&mut page, "document");
    let child_document = value(
        &mut page,
        "document.getElementById('child').contentDocument",
    );
    let before = revision(&mut page);
    let events = page.auxiliary_event_log().queued_len();
    {
        let _activation = page.rt.activation_key().enter();
        deno_core::scope!(scope, page.rt);
        let root = v8::Local::new(scope, root_document);
        let root = v8::Local::<v8::Object>::try_from(root).unwrap();
        let context = root.get_creation_context(scope).unwrap();
        let scope = &mut v8::ContextScope::new(scope, context);
        let root_status = status(scope, root).unwrap();
        assert!(record_value(scope, root_status).is_object());
        let child = v8::Local::new(scope, child_document);
        let child = v8::Local::<v8::Object>::try_from(child).unwrap();
        let context = child.get_creation_context(scope).unwrap();
        let scope = &mut v8::ContextScope::new(scope, context);
        let selected = select(scope, child, 1).unwrap();
        assert_eq!(record_value(scope, selected).uint32_value(scope), Some(0));
        let notified = notify(scope, child, 1).unwrap();
        assert_eq!(record_value(scope, notified).uint32_value(scope), Some(0));
        let scroll = scroll_restoration(scope, child, 1, "manual").unwrap();
        assert_eq!(record_value(scope, scroll).uint32_value(scope), Some(0));
        let child_status = status(scope, child).unwrap();
        assert!(record_value(scope, child_status).is_null());
    }
    assert_eq!(revision(&mut page), before);
    assert_eq!(page.auxiliary_event_log().queued_len(), events);
    assert_eq!(page.eval_json_expression("[history.state.root,history.length,location.pathname,document.getElementById('child').contentDocument.title]").await.unwrap(),
        "[true,1,\"/stable\",\"child\"]");
    assert_eq!(page.eval_json_expression("document.getElementById('child').contentWindow.history.replaceState({child:true},'');[history.state.root,document.getElementById('child').contentWindow.history.state.child]").await.unwrap(), "[true,true]");
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_native_history_notifications_and_modes_fail_before_mutation() {
    let mut page = page().await;
    let document = value(&mut page, "document");
    let before = revision(&mut page);
    let events = page.auxiliary_event_log().queued_len();
    {
        let _activation = page.rt.activation_key().enter();
        deno_core::scope!(scope, page.rt);
        let document = v8::Local::new(scope, document);
        let document = v8::Local::<v8::Object>::try_from(document).unwrap();
        let context = document.get_creation_context(scope).unwrap();
        let scope = &mut v8::ContextScope::new(scope, context);
        assert_eq!(
            notify(scope, document, 0).err().unwrap().to_string(),
            "Native history notification does not identify the selected entry"
        );
        assert_eq!(
            scroll_restoration(scope, document, 1, "invalid")
                .err()
                .unwrap()
                .to_string(),
            "Native history scroll restoration mode is invalid"
        );
        assert!(!scope.is_execution_terminating());
    }
    assert_eq!(revision(&mut page), before);
    assert_eq!(page.auxiliary_event_log().queued_len(), events);
    assert_eq!(
        page.eval_json_expression(
            "[history.state.root,history.length,history.scrollRestoration,location.pathname]"
        )
        .await
        .unwrap(),
        "[true,1,\"auto\",\"/stable\"]"
    );
    assert_eq!(page.eval_json_expression("history.scrollRestoration='manual';history.pushState({recovered:true},'', '/next');[history.state.recovered,history.scrollRestoration,location.pathname]").await.unwrap(),
        "[true,\"manual\",\"/next\"]");
}
