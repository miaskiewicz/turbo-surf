//! Host adapter contracts use real Documents and the actual registered op bindings.
use super::*;

fn own<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
) -> v8::Local<'s, v8::Value> {
    private_boundary::frame_probe::own_value(scope, object, name)
        .expect("own native record property")
}

fn call<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    root: v8::Local<'s, v8::Context>,
    name: &str,
    args: &[v8::Local<'s, v8::Value>],
) -> v8::Local<'s, v8::Object> {
    assert!(matches!(
        name,
        "op_history_select"
            | "op_history_notify"
            | "op_history_status"
            | "op_history_scroll_restoration"
            | "op_history_ack"
    ));
    let core = root.get_slot::<RuntimeCore>().unwrap();
    let core = crate::context_ownership::local(scope, &core.0);
    let ops = own(scope, core, "ops").to_object(scope).unwrap();
    let function = v8::Local::<v8::Function>::try_from(own(scope, ops, name)).unwrap();
    let receiver = v8::undefined(scope);
    function
        .call(scope, receiver.into(), args)
        .expect("registered adapter returns a record")
        .to_object(scope)
        .unwrap()
}

fn success<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    record: v8::Local<'s, v8::Object>,
) -> v8::Local<'s, v8::Value> {
    assert!(record.get_prototype(scope).unwrap().is_null());
    assert!(own(scope, record, "ok").is_true());
    own(scope, record, "value")
}

fn failure_record<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    record: v8::Local<'s, v8::Object>,
    message: &str,
) {
    assert!(record.get_prototype(scope).unwrap().is_null());
    assert!(own(scope, record, "ok").is_false());
    assert_eq!(
        own(scope, record, "name").to_rust_string_lossy(scope),
        "Error"
    );
    assert_eq!(
        own(scope, record, "message").to_rust_string_lossy(scope),
        message
    );
}

fn value(page: &mut PageSession, source: &str) -> v8::Global<v8::Value> {
    let root = page.auxiliary_root_id().unwrap();
    let _activation = page.rt.activation_key().enter();
    auxiliary_pages::execute_target(&mut page.rt, AuxiliaryContextId(root), source, false).unwrap()
}

fn ledger(page: &mut PageSession) -> (u64, usize, usize, u64) {
    let root = page.auxiliary_root_id().unwrap();
    let state = page.rt.op_state();
    let state = state.borrow();
    let history = state.borrow::<Histories>().targets.get(&root).unwrap();
    (
        history.revision(),
        history.entries().len(),
        history.index(),
        history.current().unwrap().row,
    )
}

async fn page() -> PageSession {
    let mut page = PageSession::open(
        "<title>root</title><iframe id=first srcdoc='<title>first</title>'></iframe><iframe id=peer srcdoc='<title>peer</title>'></iframe>",
        "https://history-op.test/start", "", "", 5000,
    ).await.unwrap();
    page.eval_json_expression("history.replaceState({stable:true},'', '/stable');true")
        .await
        .unwrap();
    page
}

#[tokio::test(flavor = "current_thread")]
async fn registered_native_history_selection_requires_ack_and_accepts_its_actual_token_once() {
    let mut page = page().await;
    let root_document = value(&mut page, "document");
    let before = ledger(&mut page);
    let events = page.auxiliary_event_log().queued_len();
    {
        let _activation = page.rt.activation_key().enter();
        let root = private_infrastructure::context(&mut page.rt).unwrap();
        deno_core::scope!(scope, page.rt);
        let root = v8::Local::new(scope, root);
        let scope = &mut v8::ContextScope::new(scope, root);
        let document = v8::Local::new(scope, root_document);
        let status = call(scope, root, "op_history_status", &[document]);
        let status = success(scope, status).to_object(scope).unwrap();
        let local = own(scope, status, "local");
        assert!(local.uint32_value(scope).is_some_and(|local| local > 0));
        let selection = call(scope, root, "op_history_select", &[document, local]);
        let selection = success(scope, selection).to_object(scope).unwrap();
        assert!(own(scope, selection, "local").strict_equals(local));
        let token = own(scope, selection, "transaction");
        assert!(token.is_string());
        let ack = call(scope, root, "op_history_ack", &[document, token]);
        assert!(success(scope, ack).is_true());
        let stale = call(scope, root, "op_history_ack", &[document, token]);
        failure_record(scope, stale, "History transaction is stale");
        let notified = call(scope, root, "op_history_notify", &[document, local]);
        assert!(success(scope, notified).strict_equals(local));
        assert!(!scope.is_execution_terminating());
    }
    let after = ledger(&mut page);
    assert_eq!(after, (before.0 + 1, before.1, before.2, before.3));
    assert_eq!(page.auxiliary_event_log().queued_len(), events + 1);
    assert_eq!(page.eval_json_expression("[history.state.stable,location.pathname,document.querySelector('#peer').contentDocument.title]").await.unwrap(),
        "[true,\"/stable\",\"peer\"]");
    assert_eq!(page.eval_json_expression("history.pushState({recovered:true},'', '/next');[history.state.recovered,history.length,location.pathname]").await.unwrap(),
        "[true,2,\"/next\"]");
}

#[tokio::test(flavor = "current_thread")]
async fn registered_history_bindings_reject_a_retired_real_document_without_affecting_its_peer() {
    let mut page = page().await;
    let retired_document = value(
        &mut page,
        "document.querySelector('#first').contentDocument",
    );
    page.eval_json_expression("document.querySelector('#first').remove();true")
        .await
        .unwrap();
    let before = ledger(&mut page);
    let events = page.auxiliary_event_log().queued_len();
    {
        let _activation = page.rt.activation_key().enter();
        let root = private_infrastructure::context(&mut page.rt).unwrap();
        deno_core::scope!(scope, page.rt);
        let root = v8::Local::new(scope, root);
        let scope = &mut v8::ContextScope::new(scope, root);
        let document = v8::Local::new(scope, retired_document);
        let owner = document
            .to_object(scope)
            .unwrap()
            .get_creation_context(scope)
            .unwrap();
        assert!(owner.get_slot::<FrameRetired>().is_some());
        let local = v8::Integer::new_from_unsigned(scope, 1);
        let mode = v8::String::new(scope, "manual").unwrap();
        let token = v8::String::new(scope, "retired-token").unwrap();
        for (name, args) in [
            ("op_history_status", vec![document]),
            ("op_history_select", vec![document, local.into()]),
            ("op_history_notify", vec![document, local.into()]),
            (
                "op_history_scroll_restoration",
                vec![document, local.into(), mode.into()],
            ),
            ("op_history_ack", vec![document, token.into()]),
        ] {
            let result = call(scope, root, name, &args);
            failure_record(scope, result, "Native history document is retired");
        }
        assert!(!scope.is_execution_terminating());
    }
    assert_eq!(ledger(&mut page), before);
    assert_eq!(page.auxiliary_event_log().queued_len(), events);
    assert_eq!(page.eval_json_expression("[history.state.stable,location.pathname,document.querySelector('#peer').contentDocument.title,frames.length,typeof Deno]").await.unwrap(),
        "[true,\"/stable\",\"peer\",1,\"undefined\"]");
    assert_eq!(page.eval_json_expression("(async()=>{const replacement=document.createElement('iframe');replacement.srcdoc='<title>replacement</title>';const loaded=new Promise(resolve=>replacement.addEventListener('load',resolve,{once:true}));document.body.append(replacement);await loaded;return [replacement.contentDocument.title,document.querySelector('#peer').contentDocument.title]})()").await.unwrap(),
        "[\"replacement\",\"peer\"]");
    assert_eq!(page.eval_json_expression("history.pushState({recovered:true},'', '/recovered');[history.state.recovered,location.pathname]").await.unwrap(),
        "[true,\"/recovered\"]");
}
