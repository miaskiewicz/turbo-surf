//! Real native op identities exercise the selection guard, including one
//! deliberately contradictory registration as a hostile invariant control.
use super::*;
use crate::runtime::{auxiliary_pages, private_boundary, PageSession, RuntimeCore};
use turbo_dom_browser_env::host_internals::AuxiliaryContextId;

fn defer<'s>(scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Promise> {
    let context = scope.get_current_context();
    let core = context.get_slot::<RuntimeCore>().unwrap();
    let core = crate::context_ownership::local(scope, &core.0);
    let ops = private_boundary::frame_probe::own_value(scope, core, "ops").unwrap();
    let ops = v8::Local::<v8::Object>::try_from(ops).unwrap();
    let operation = private_boundary::frame_probe::own_value(scope, ops, "op_defer").unwrap();
    let operation = v8::Local::<v8::Function>::try_from(operation).unwrap();
    let receiver = v8::undefined(scope);
    let result = operation.call(scope, receiver.into(), &[]).unwrap();
    v8::Local::<v8::Promise>::try_from(result).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn an_actual_unowned_refed_native_op_remains_rejected() {
    let mut page = PageSession::open("<body>", "https://selection-guard.test/", "", "", 5000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    let infrastructure = page.rt.main_context();
    let _activation = page.rt.activation_key().enter();
    deno_core::scope!(scope, &mut page.rt);
    let root = auxiliary_pages::context(scope, AuxiliaryContextId(root)).unwrap();
    let roots = vec![v8::Global::new(scope, root)];
    let context = v8::Local::new(scope, infrastructure);
    assert!(context
        .get_slot::<private_boundary::Infrastructure>()
        .is_some());
    let scope = &mut v8::ContextScope::new(scope, context);
    let shared = super::super::tracker(scope).unwrap();
    assert!(super::super::current_owner(scope, &shared).is_none());
    let promise = defer(scope);
    let mut state = shared.borrow_mut();
    assert!(state.work.recorded_owner(scope, promise).is_none());
    assert_eq!(state.work.ops.len(), 1, "actual lazy op remains refed");
    assert!(state
        .work
        .ops
        .values()
        .all(|op| op.refed && op.owner.is_none()));
    let error = super::super::selected_contexts(scope, &mut state, &roots)
        .err()
        .expect("unowned native work must never acquire Root authority");
    assert_eq!(error, "Native async work has unclassified ownership");
}

#[tokio::test(flavor = "current_thread")]
async fn a_contradictory_real_promise_registration_remains_rejected() {
    let mut page = PageSession::open("<body>", "https://selection-guard.test/", "", "", 5000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    page.eval_expression("window.open('about:blank','registered-peer');void 0")
        .await
        .unwrap();
    let peer = page
        .auxiliary_event_log()
        .pages()
        .into_iter()
        .find(|info| info.source_page.as_deref() == Some(&root.to_string()))
        .unwrap()
        .target
        .parse::<u64>()
        .unwrap();
    let _activation = page.rt.activation_key().enter();
    deno_core::scope!(scope, &mut page.rt);
    let root_context = auxiliary_pages::context(scope, AuxiliaryContextId(root)).unwrap();
    let roots = vec![v8::Global::new(scope, root_context)];
    let shared = super::super::tracker(scope).unwrap();
    let promise = {
        let scope = &mut v8::ContextScope::new(scope, root_context);
        super::super::with_document(scope, root_context, |scope| {
            let promise = defer(scope);
            v8::Global::new(scope, promise)
        })
        .unwrap()
    };
    let promise = v8::Local::new(scope, promise);
    let peer_context = auxiliary_pages::context(scope, AuxiliaryContextId(peer)).unwrap();
    let mut state = shared.borrow_mut();
    let peer_owner = super::super::document_owner(scope, &mut state, peer_context).unwrap();
    let root_owner = state.work.recorded_owner(scope, promise).unwrap();
    assert_ne!(
        peer_owner, root_owner,
        "both owners are genuine native document leases"
    );
    let native_id = state.work.recorded_identity(scope, promise).unwrap();
    let key = *state
        .work
        .ops
        .iter()
        .find(|(_, op)| op.node == native_id)
        .unwrap()
        .0;
    assert!(!state.work.inconsistent_owners);
    // Inject the hostile contradictory observation with real identities. Normal
    // bridge entry must never generate this mismatch; the invariant must reject it.
    state.work.op_turn(
        scope,
        AsyncOpObservation {
            promise_id: key.0,
            generation: key.1,
            event: AsyncOpEvent::Registered,
            promise: Some(promise),
        },
        Some(peer_owner),
    );
    assert!(state.work.inconsistent_owners);
    let error = super::super::selected_contexts(scope, &mut state, &roots)
        .err()
        .expect("contradictory genuine owners must remain rejected");
    assert_eq!(error, "Native async work has unclassified ownership");
}

#[tokio::test(flavor = "current_thread")]
async fn an_actual_window_timer_turn_ignores_only_a_collected_root_parent() {
    let mut page = PageSession::open("<body>", "https://selection-guard.test/", "", "", 5000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    page.read_target_engine(root, "const parent=new Promise(()=>{});globalThis.parentWeak=new WeakRef(parent);globalThis.child=parent.then(()=>{globalThis.unexpected=true;});return 'armed';")
        .unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
        if page
            .read_target_engine(root, "return parentWeak.deref()===undefined;")
            .unwrap()
            == "true"
        {
            break;
        }
    }
    assert_eq!(
        page.read_target_engine(root, "return parentWeak.deref()===undefined;")
            .unwrap(),
        "true"
    );
    page.read_target_engine(
        root,
        "globalThis.timerRuns=0;setTimeout(()=>{timerRuns++;},1);return 'armed';",
    )
    .unwrap();
    let turn = {
        let _activation = page.rt.activation_key().enter();
        crate::runtime::run_window_timer_tasks(&mut page.rt, 1, false).unwrap()
    };
    assert_eq!(turn.fired, 1, "the genuine selected timer ran exactly once");
    assert_eq!(
        page.read_target_engine(
            root,
            "return [timerRuns,globalThis.unexpected===undefined];"
        )
        .unwrap(),
        "[1,true]"
    );
    let error = page.eval_json_expression("child").await.unwrap_err();
    assert_eq!(error, "Native async work has unclassified ownership");
}

#[tokio::test(flavor = "current_thread")]
async fn a_collected_infrastructure_parent_never_acquires_root_authority() {
    let mut page = PageSession::open("<body>", "https://selection-guard.test/", "", "", 5000)
        .await
        .unwrap();
    let root = page.auxiliary_root_id().unwrap();
    {
        let infrastructure = page.rt.main_context();
        let _activation = page.rt.activation_key().enter();
        deno_core::scope!(scope, &mut page.rt);
        let context = v8::Local::new(scope, infrastructure);
        let scope = &mut v8::ContextScope::new(scope, context);
        assert!(context
            .get_slot::<private_boundary::Infrastructure>()
            .is_some());
        let shared = super::super::tracker(scope).unwrap();
        assert!(super::super::current_owner(scope, &shared).is_none());
        let resolver = v8::PromiseResolver::new(scope).unwrap();
        let promise = resolver.get_promise(scope);
        assert!(shared
            .borrow()
            .work
            .recorded_owner(scope, promise)
            .is_none());
        // A trusted test supplies a genuine unowned Promise across the boundary;
        // no owner ID or native identity metadata is fabricated or overwritten.
        let root_context = auxiliary_pages::context(scope, AuxiliaryContextId(root)).unwrap();
        let scope = &mut v8::ContextScope::new(scope, root_context);
        let key = v8::String::new(scope, "unownedParent").unwrap();
        assert_eq!(
            root_context
                .global(scope)
                .create_data_property(scope, key.into(), promise.into()),
            Some(true)
        );
    }
    page.read_target_engine(root, "const parent=unownedParent;globalThis.parentWeak=new WeakRef(parent);globalThis.child=parent.then(()=>{globalThis.unexpected=true;});unownedParent=null;return 'attached';").unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
        if page
            .read_target_engine(root, "return parentWeak.deref()===undefined;")
            .unwrap()
            == "true"
        {
            break;
        }
    }
    assert_eq!(
        page.read_target_engine(root, "return parentWeak.deref()===undefined;")
            .unwrap(),
        "true"
    );
    let error = page
        .dispatch_target_engine(root, "globalThis.actionRuns=1;globalThis.__RESULT='ran';")
        .await
        .unwrap_err();
    assert_eq!(error, "Native async work has unclassified ownership");
    assert_eq!(
        page.read_target_engine(
            root,
            "return [actionRuns,globalThis.unexpected===undefined];"
        )
        .unwrap(),
        "[1,true]"
    );
}
