//! Real interruption between the registered frame allocation and document install.
use super::*;

fn own<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
) -> v8::Local<'s, v8::Value> {
    private_boundary::frame_probe::own_value(scope, object, name).expect("fixture own property")
}

fn invoke<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    root: v8::Local<'s, v8::Context>,
    name: &str,
    arguments: &[v8::Local<'s, v8::Value>],
) -> v8::Local<'s, v8::Value> {
    assert!(matches!(name, "op_frame_load_attempt" | "op_create_frame"));
    let core = root.get_slot::<RuntimeCore>().unwrap();
    let core = crate::context_ownership::local(scope, &core.0);
    let ops = own(scope, core, "ops").to_object(scope).unwrap();
    let function = v8::Local::<v8::Function>::try_from(own(scope, ops, name)).unwrap();
    let receiver = v8::undefined(scope);
    function
        .call(scope, receiver.into(), arguments)
        .expect("actual registered frame operation")
}

#[tokio::test(flavor = "current_thread")]
async fn interrupted_registered_frame_allocation_rolls_back_before_healthy_input() {
    let mut page = PageSession::open("<body><input id=q><iframe id=live srcdoc='<body id=live>healthy child</body>'></iframe></body>", "https://provisional.test/", "", "", 60_000).await.unwrap();
    let root_id = page.auxiliary_root_id().unwrap();
    page.eval("globalThis.savedRootDocument=document;globalThis.healthyChild=document.querySelector('#live').contentWindow;globalThis.savedChildDocument=healthyChild.document;globalThis.provisionalHost=document.createElement('iframe');globalThis.peerWindow=window.open('about:blank','peer');peerWindow.peerMarker=29;").await.unwrap();
    let peer_id = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|entry| entry.target != root_id.to_string())
        .unwrap()
        .target
        .parse::<u64>()
        .unwrap();
    let provisional = {
        let _activation = page.rt.activation_key().enter();
        let root = private_infrastructure::context(&mut page.rt).unwrap();
        deno_core::scope!(scope, page.rt);
        let root = v8::Local::new(scope, root);
        let scope = &mut v8::ContextScope::new(scope, root);
        assert_eq!(scope.get_entered_or_microtask_context(), root);
        let global = root.global(scope);
        let host = own(scope, global, "provisionalHost")
            .to_object(scope)
            .unwrap();
        let attempt = invoke(
            scope,
            root,
            "op_frame_load_attempt",
            &[global.into(), host.into()],
        );
        let window = invoke(
            scope,
            root,
            "op_create_frame",
            &[global.into(), host.into(), attempt],
        )
        .to_object(scope)
        .unwrap();
        let context = window.get_creation_context(scope).unwrap();
        assert_ne!(context, root);
        assert!(context.get_slot::<FrameContextMarker>().is_some());
        assert!(context.get_slot::<FrameParent>().is_some());
        assert!(context.get_slot::<FrameConfigured>().is_none());
        assert!(context.get_slot::<CspHandle>().is_none());
        assert!(context.get_slot::<ActiveDocument>().is_none());
        let name = v8::String::new(scope, "provisionalWindow").unwrap();
        assert_eq!(
            global.create_data_property(scope, name.into(), window.into()),
            Some(true)
        );
        v8::Global::new(scope, context)
    };
    // This is a real operation watchdog, not a fabricated retirement/owner slot.
    page.set_execution_budget_for_testing(50);
    let error = page
        .eval_expression("(()=>{for(;;){}})()")
        .await
        .unwrap_err();
    assert!(error.contains("budget"), "{error}");
    page.set_execution_budget_for_testing(60_000);
    page.input(&serde_json::json!([{"focus":"#q"},{"type":"ok"}]))
        .await
        .unwrap();
    {
        let _activation = page.rt.activation_key().enter();
        deno_core::scope!(scope, page.rt);
        let context = v8::Local::new(scope, &provisional);
        assert!(context.get_slot::<FrameRetired>().is_some());
        assert!(!JsRuntime::op_state_from(scope)
            .borrow()
            .borrow::<FrameContexts>()
            .contexts
            .iter()
            .any(|saved| v8::Local::new(scope, saved) == context));
    }
    assert_eq!(page.auxiliary_root_id().unwrap(), root_id);
    assert_eq!(page.eval_expression("JSON.stringify((()=>{let exposed=false;try{exposed='Deno'in provisionalWindow||'__bootstrap'in provisionalWindow||provisionalWindow[Symbol.for('ts.dc')]!==undefined||'op_set_host_input_driver'in provisionalWindow;}catch(error){if(error.name!=='SecurityError')throw error;}return [document===savedRootDocument,healthyChild.document===savedChildDocument,healthyChild.document.body.id,document.querySelector('#q').value,exposed,'Deno'in globalThis||'__bootstrap'in globalThis];})())").await.unwrap(), "[true,true,\"live\",\"ok\",false,false]");
    assert_eq!(
        page.eval_target_expression(peer_id, "peerMarker", false, false)
            .await
            .unwrap(),
        "29"
    );
    assert_eq!(page.eval_expression("new Promise(resolve=>{const frame=document.createElement('iframe');frame.onload=()=>resolve(frame.contentDocument.body.id);frame.srcdoc='<body id=fresh>fresh</body>';document.body.appendChild(frame);})").await.unwrap(), "fresh");
    drop(provisional);
    page.close();
}
