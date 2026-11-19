//! Native embedding contracts. No registered capability is exposed to page code.
use super::*;
use turbo_dom_browser_env::host_internals as host;

const ROOT_SCRIPT: &str = "globalThis.resourceRuns=(globalThis.resourceRuns||0)+1;globalThis.resourceCurrent=!!document.currentScript";

async fn page() -> PageSession {
    let html = format!(
        r#"<body><p id=healthy>healthy</p><script id=probe>{ROOT_SCRIPT}</script><iframe id=first srcdoc="<body>first<script id=probe>globalThis.childRuns=1</script>"></iframe><iframe id=second srcdoc="<body>second"></iframe><iframe id=blocked sandbox srcdoc="<body>blocked"></iframe><iframe id=opaque sandbox=allow-scripts srcdoc="<body>opaque<script>globalThis.opaqueRuns=1</script>"></iframe>"#
    );
    PageSession::open(&html, "https://registered-contract.test/", "", "", 5000)
        .await
        .unwrap()
}

fn own<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
) -> v8::Local<'s, v8::Value> {
    private_boundary::frame_probe::own_value(scope, object, name)
        .expect("actual own data property, never a guest getter")
}

fn global_own<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    name: &str,
) -> v8::Local<'s, v8::Value> {
    let scope = &mut v8::ContextScope::new(scope, context);
    let global = context.global(scope);
    own(scope, global, name)
}

fn root(page: &mut PageSession) -> v8::Global<v8::Context> {
    let target = page.replaced_root_target().unwrap();
    deno_core::scope!(scope, &mut page.rt);
    let context = auxiliary_pages::context(scope, host::AuxiliaryContextId(target)).unwrap();
    assert_eq!(
        host::auxiliary_context_identity(context).unwrap().id.0,
        target
    );
    v8::Global::new(scope, context)
}

fn document<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) -> v8::Local<'s, v8::Object> {
    let document = context
        .get_slot::<ActiveDocument>()
        .unwrap()
        .0
        .to_local(scope)
        .unwrap();
    assert_eq!(document.get_creation_context(scope), Some(context));
    assert!(host::handle_of(scope, document).is_some());
    document
}

fn frame<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    parent: v8::Local<'s, v8::Context>,
    id: &str,
) -> (v8::Local<'s, v8::Context>, v8::Local<'s, v8::Object>) {
    for context in frame_children(scope, parent) {
        let element = context
            .get_slot::<FrameHost>()
            .unwrap()
            .0
            .to_local(scope)
            .unwrap();
        if crate::browser_env::node_attribute(scope, element, "id").as_deref() == Some(id) {
            return (context, element);
        }
    }
    panic!("actual configured iframe {id} missing");
}

fn page_element<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    id: &str,
) -> v8::Local<'s, v8::Object> {
    let context = document.get_creation_context(scope).unwrap();
    let scope = &mut v8::ContextScope::new(scope, context);
    let _document = host::enter_document_of(scope, document);
    let handle = host::with_tree(|tree| {
        let mut pending = vec![tree.root()];
        while let Some(node) = pending.pop() {
            if tree.get_attribute(node, "id") == Some(id) {
                return Some(node);
            }
            pending.extend(tree.children(node).into_iter().rev());
        }
        None
    })
    .flatten()
    .expect("actual native document element");
    host::wrap(scope, handle)
}

fn registered<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    name: &str,
) -> v8::Local<'s, v8::Function> {
    const NAMES: &[&str] = &[
        "op_frame_load_attempt",
        "op_frame_load_active",
        "op_frame_load_complete",
        "op_frame_load_window_valid",
        "op_timer_code_allowed",
        "op_run_timer_code",
        "op_run_local_timers",
        "op_module_credentials",
        "op_inline_module",
        "op_prepare_resource_script",
        "op_execute_inline_script",
    ];
    assert!(NAMES.contains(&name));
    let core = context
        .get_slot::<RuntimeCore>()
        .expect("actual runtime core slot");
    let core = crate::context_ownership::local(scope, &core.0);
    let ops = own(scope, core, "ops");
    let ops = v8::Local::<v8::Object>::try_from(ops).unwrap();
    v8::Local::<v8::Function>::try_from(own(scope, ops, name)).unwrap()
}

fn invoke<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    owner: v8::Local<'s, v8::Context>,
    name: &str,
    arguments: &[v8::Local<'s, v8::Value>],
) -> Result<v8::Global<v8::Value>, String> {
    let function = registered(scope, owner, name);
    let scope = &mut v8::ContextScope::new(scope, owner);
    assert_eq!(scope.get_entered_or_microtask_context(), owner);
    v8::tc_scope!(let scope, scope);
    let receiver = v8::undefined(scope);
    if let Some(value) = function.call(scope, receiver.into(), arguments) {
        return Ok(v8::Global::new(scope, value));
    }
    assert!(!scope.has_terminated(), "no manufactured termination state");
    let error = scope.exception().expect("native rejection");
    assert!(error.is_native_error());
    let error = v8::Local::<v8::Object>::try_from(error).unwrap();
    let message = own(scope, error, "message");
    assert!(message.is_string());
    Err(message.to_rust_string_lossy(scope))
}

fn boolean(scope: &mut v8::PinScope, value: v8::Global<v8::Value>) -> bool {
    let value = v8::Local::new(scope, value);
    assert!(value.is_boolean());
    value.is_true()
}

fn count(scope: &mut v8::PinScope, value: v8::Global<v8::Value>) -> u32 {
    let value = v8::Local::new(scope, value);
    assert!(value.is_uint32());
    value.uint32_value(scope).unwrap()
}

async fn healthy(page: &mut PageSession) {
    assert_eq!(
        page.eval_json_expression(
            "({body:document.querySelector('#healthy').textContent,privateCore:typeof Deno})"
        )
        .await
        .unwrap(),
        r#"{"body":"healthy","privateCore":"undefined"}"#
    );
}

#[tokio::test(flavor = "current_thread")]
async fn registered_frame_capabilities_bind_actual_host_window_and_reject_stale_completion() {
    let mut page = page().await;
    let _activation = page.rt.activation_key().enter();
    let root = root(&mut page);
    {
        deno_core::scope!(scope, &mut page.rt);
        let owner = v8::Local::new(scope, &root);
        let (first, first_host) = frame(scope, owner, "first");
        let (second, second_host) = frame(scope, owner, "second");
        let (opaque, _) = frame(scope, owner, "opaque");
        let parent_window = owner.global(scope);
        let first_window = first.global(scope);
        let second_window = second.global(scope);
        let opaque_window = opaque.global(scope);
        let token = invoke(
            scope,
            owner,
            "op_frame_load_attempt",
            &[parent_window.into(), first_host.into()],
        )
        .unwrap();
        let token = v8::Local::new(scope, token);
        let active = invoke(
            scope,
            owner,
            "op_frame_load_active",
            &[parent_window.into(), first_host.into(), token],
        )
        .unwrap();
        assert!(boolean(scope, active));
        let valid = invoke(
            scope,
            owner,
            "op_frame_load_window_valid",
            &[
                parent_window.into(),
                first_host.into(),
                first_window.into(),
                token,
            ],
        )
        .unwrap();
        assert!(boolean(scope, valid));
        assert!(invoke(
            scope,
            owner,
            "op_frame_load_window_valid",
            &[
                parent_window.into(),
                first_host.into(),
                second_window.into(),
                token
            ]
        )
        .unwrap_err()
        .contains("ownership changed"));
        assert!(invoke(
            scope,
            owner,
            "op_frame_load_active",
            &[parent_window.into(), second_host.into(), token]
        )
        .unwrap_err()
        .contains("ownership changed"));
        assert!(invoke(
            scope,
            owner,
            "op_frame_load_attempt",
            &[opaque_window.into(), first_host.into()]
        )
        .unwrap_err()
        .contains("same-origin"));
        let invalid = v8::Integer::new(scope, 1);
        assert!(invoke(
            scope,
            owner,
            "op_frame_load_attempt",
            &[invalid.into(), first_host.into()]
        )
        .is_err());
        let active = invoke(
            scope,
            owner,
            "op_frame_load_active",
            &[parent_window.into(), first_host.into(), token],
        )
        .unwrap();
        assert!(
            boolean(scope, active),
            "denied inputs cannot cancel the valid token"
        );
        let next = invoke(
            scope,
            owner,
            "op_frame_load_attempt",
            &[parent_window.into(), first_host.into()],
        )
        .unwrap();
        let next = v8::Local::new(scope, next);
        let old = invoke(
            scope,
            owner,
            "op_frame_load_active",
            &[parent_window.into(), first_host.into(), token],
        )
        .unwrap();
        assert!(!boolean(scope, old));
        assert!(invoke(
            scope,
            owner,
            "op_frame_load_complete",
            &[parent_window.into(), first_host.into(), token]
        )
        .unwrap_err()
        .contains("cancelled"));
        let done = invoke(
            scope,
            owner,
            "op_frame_load_complete",
            &[parent_window.into(), first_host.into(), next],
        )
        .unwrap();
        assert!(v8::Local::new(scope, done).is_null());
        let active = invoke(
            scope,
            owner,
            "op_frame_load_active",
            &[parent_window.into(), first_host.into(), next],
        )
        .unwrap();
        assert!(boolean(scope, active));
    }
    healthy(&mut page).await;
    drop(root);
    drop(_activation);
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn registered_timer_document_guards_preserve_actual_queues_and_reject_retired_owner() {
    let mut page = page().await;
    let _activation = page.rt.activation_key().enter();
    let root = root(&mut page);
    let retained = {
        deno_core::scope!(scope, &mut page.rt);
        let owner = v8::Local::new(scope, &root);
        let (child, _) = frame(scope, owner, "first");
        let document = document(scope, child);
        let code = v8::String::new(scope, "globalThis.nativeTimerRuns=0;setTimeout(()=>nativeTimerRuns++,0);setTimeout(()=>nativeTimerRuns++,0);47").unwrap();
        let result = invoke(
            scope,
            owner,
            "op_run_timer_code",
            &[document.into(), code.into()],
        )
        .unwrap();
        assert_eq!(count(scope, result), 47);
        let fake = v8::Object::new(scope);
        let denied = invoke(
            scope,
            owner,
            "op_run_timer_code",
            &[fake.into(), code.into()],
        )
        .unwrap();
        assert!(v8::Local::new(scope, denied).is_undefined());
        let nan = v8::Number::new(scope, f64::NAN);
        let result = invoke(
            scope,
            owner,
            "op_run_local_timers",
            &[document.into(), nan.into()],
        )
        .unwrap();
        assert_eq!(count(scope, result), 0);
        assert_eq!(
            global_own(scope, child, "nativeTimerRuns").uint32_value(scope),
            Some(0)
        );
        let one = v8::Integer::new(scope, 1);
        let result = invoke(
            scope,
            owner,
            "op_run_local_timers",
            &[document.into(), one.into()],
        )
        .unwrap();
        assert_eq!(count(scope, result), 1);
        let invalid = v8::Integer::new(scope, 7);
        assert!(invoke(
            scope,
            owner,
            "op_run_local_timers",
            &[invalid.into(), one.into()]
        )
        .is_err());
        assert_eq!(
            global_own(scope, child, "nativeTimerRuns").uint32_value(scope),
            Some(1)
        );
        let result = invoke(
            scope,
            owner,
            "op_run_local_timers",
            &[document.into(), one.into()],
        )
        .unwrap();
        assert_eq!(count(scope, result), 1);
        assert_eq!(
            global_own(scope, child, "nativeTimerRuns").uint32_value(scope),
            Some(2)
        );
        (
            v8::Global::new(scope, child),
            v8::Global::new(scope, document),
        )
    };
    page.eval_expression("document.getElementById('first').remove();'removed'")
        .await
        .unwrap();
    {
        deno_core::scope!(scope, &mut page.rt);
        let owner = v8::Local::new(scope, &root);
        let child = v8::Local::new(scope, &retained.0);
        assert!(native_frame_is_retired(child));
        let retired_document = v8::Local::new(scope, &retained.1);
        let code = v8::String::new(scope, "globalThis.nativeTimerRuns=999").unwrap();
        let allowed = invoke(
            scope,
            owner,
            "op_timer_code_allowed",
            &[retired_document.into(), code.into()],
        )
        .unwrap();
        assert!(!boolean(scope, allowed));
        let result = invoke(
            scope,
            owner,
            "op_run_timer_code",
            &[retired_document.into(), code.into()],
        )
        .unwrap();
        assert!(v8::Local::new(scope, result).is_undefined());
        let one = v8::Integer::new(scope, 1);
        let result = invoke(
            scope,
            owner,
            "op_run_local_timers",
            &[retired_document.into(), one.into()],
        )
        .unwrap();
        assert_eq!(count(scope, result), 0);
        assert_eq!(
            global_own(scope, child, "nativeTimerRuns").uint32_value(scope),
            Some(2)
        );
        let scope = &mut v8::ContextScope::new(scope, owner);
        let global = owner.global(scope);
        assert!(
            private_boundary::frame_probe::own_value(scope, global, "nativeTimerRuns").is_none()
        );
    }
    healthy(&mut page).await;
    drop(retained);
    drop(root);
    drop(_activation);
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn registered_module_metadata_rejects_invalid_and_retired_owners_without_resurrection() {
    let mut page = page().await;
    let code = "globalThis.nativeModuleObserved='😀';globalThis.nativeModuleSpecifier=import.meta.url;globalThis.nativeModuleSourceURL=document.URL;globalThis.nativeModuleResolve('loaded');delete globalThis.nativeModuleResolve";
    // Inline modules execute without the load event reserved for external scripts.
    let setup = format!("new Promise((resolve,reject)=>{{const doc=document;const script=doc.createElement('script');script.type='module';script.nonce='native-nonce';script.crossOrigin='use-credentials';script.textContent={};script.onerror=()=>reject(new Error('module fixture failed'));globalThis.nativeModuleResolve=resolve;globalThis.nativeModuleElement=script;doc.body.append(script)}})", serde_json::to_string(code).unwrap());
    assert_eq!(page.eval_expression(&setup).await.unwrap(), "loaded");
    page.eval_expression("nativeModuleElement.crossOrigin='anonymous';nativeModuleElement.nonce='changed';nativeModuleElement.integrity='sha256-changed';'mutated'").await.unwrap();
    let _activation = page.rt.activation_key().enter();
    let root = root(&mut page);
    let retained = {
        deno_core::scope!(scope, &mut page.rt);
        let owner = v8::Local::new(scope, &root);
        let (child, _) = frame(scope, owner, "first");
        let element = global_own(scope, owner, "nativeModuleElement");
        let element = v8::Local::<v8::Object>::try_from(element).unwrap();
        {
            let _document = host::enter_document_of(scope, element);
            assert_eq!(host::current_document_context(scope), Some(owner));
        }
        let specifier =
            global_own(scope, owner, "nativeModuleSpecifier").to_rust_string_lossy(scope);
        let shared = JsRuntime::op_state_from(scope);
        let credentials_count = owner
            .get_slot::<ModuleCredentials>()
            .unwrap()
            .0
            .borrow()
            .len();
        let before = owner
            .get_slot::<ModuleCredentials>()
            .unwrap()
            .0
            .borrow()
            .get(&specifier)
            .cloned()
            .unwrap();
        assert!(matches!(before.credentials, Credentials::Include));
        assert_eq!(before.nonce, "native-nonce");
        assert_eq!(before.integrity, "");
        let specifier_value = v8::String::new(scope, &specifier).unwrap();
        let result = invoke(
            scope,
            owner,
            "op_module_credentials",
            &[element.into(), specifier_value.into()],
        )
        .unwrap();
        assert!(v8::Local::new(scope, result).is_null());
        let after = owner
            .get_slot::<ModuleCredentials>()
            .unwrap()
            .0
            .borrow()
            .get(&specifier)
            .cloned()
            .unwrap();
        assert!(matches!(after.credentials, Credentials::Include));
        assert_eq!(after.nonce, "native-nonce");
        assert_eq!(after.integrity, "");
        assert_eq!(
            owner
                .get_slot::<ModuleCredentials>()
                .unwrap()
                .0
                .borrow()
                .len(),
            credentials_count
        );
        let fake = v8::Object::new(scope);
        let code_value = v8::String::new(scope, code).unwrap();
        let base = shared.borrow().borrow::<Base>().0.clone();
        let base_value = v8::String::new(scope, &base).unwrap();
        let source = global_own(scope, owner, "nativeModuleSourceURL").to_rust_string_lossy(scope);
        let source_value = v8::String::new(scope, &source).unwrap();
        let before_count = shared.borrow().borrow::<InlineModules>().0.borrow().len();
        assert!(invoke(
            scope,
            owner,
            "op_inline_module",
            &[
                fake.into(),
                code_value.into(),
                base_value.into(),
                source_value.into()
            ]
        )
        .is_err());
        assert_eq!(
            shared.borrow().borrow::<InlineModules>().0.borrow().len(),
            before_count
        );
        let result = invoke(
            scope,
            owner,
            "op_inline_module",
            &[
                element.into(),
                code_value.into(),
                base_value.into(),
                source_value.into(),
            ],
        )
        .unwrap();
        let specifier = v8::Local::new(scope, result).to_rust_string_lossy(scope);
        let modules = shared.borrow().borrow::<InlineModules>().clone();
        {
            let rows = modules.0.borrow();
            let stored = rows.get(&specifier).unwrap();
            assert_eq!(stored.owner, owner.get_slot::<CspHandle>().unwrap().id);
            assert_eq!(stored.code, code);
            let expected_units: Vec<u16> = code.encode_utf16().collect();
            assert_eq!(stored.code_units.as_ref(), expected_units.as_slice());
            assert_eq!(stored.base, base);
            assert_eq!(stored.source, source);
        }
        let child_document = document(scope, child);
        let old_child_script = page_element(scope, child_document, "probe");
        (
            v8::Global::new(scope, child),
            v8::Global::new(scope, old_child_script),
        )
    };
    page.eval_expression("document.getElementById('first').remove();'removed'")
        .await
        .unwrap();
    {
        deno_core::scope!(scope, &mut page.rt);
        let owner = v8::Local::new(scope, &root);
        let old = v8::Local::new(scope, &retained.1);
        let old_context = v8::Local::new(scope, &retained.0);
        assert!(native_frame_is_retired(old_context));
        let code = v8::String::new(scope, "globalThis.mustNotBeRegistered=1").unwrap();
        let base = v8::String::new(scope, "https://registered-contract.test/").unwrap();
        let source = v8::String::new(scope, "about:srcdoc").unwrap();
        let shared = JsRuntime::op_state_from(scope);
        let before = shared.borrow().borrow::<InlineModules>().0.borrow().len();
        let error = invoke(
            scope,
            owner,
            "op_inline_module",
            &[old.into(), code.into(), base.into(), source.into()],
        )
        .unwrap_err();
        assert!(error.contains("not active") || error.contains("retired"));
        assert_eq!(
            shared.borrow().borrow::<InlineModules>().0.borrow().len(),
            before
        );
        let root_document = document(scope, owner);
        let probe = page_element(scope, root_document, "probe");
        let healthy_code = v8::String::new(scope, ROOT_SCRIPT).unwrap();
        let registered = invoke(
            scope,
            owner,
            "op_inline_module",
            &[probe.into(), healthy_code.into(), base.into(), base.into()],
        )
        .unwrap();
        assert!(v8::Local::new(scope, registered).is_string());
    }
    healthy(&mut page).await;
    drop(retained);
    drop(root);
    drop(_activation);
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn registered_resource_consumers_reject_wrong_and_retired_documents_then_execute_healthy_owner(
) {
    let mut page = page().await;
    let _activation = page.rt.activation_key().enter();
    let root = root(&mut page);
    let retained = {
        deno_core::scope!(scope, &mut page.rt);
        let owner = v8::Local::new(scope, &root);
        let root_document = document(scope, owner);
        let (child, _) = frame(scope, owner, "first");
        let child_document = document(scope, child);
        let (blocked, _) = frame(scope, owner, "blocked");
        let blocked_document = document(scope, blocked);
        let (opaque, _) = frame(scope, owner, "opaque");
        let opaque_document = document(scope, opaque);
        assert!(!owner
            .get_security_token(scope)
            .strict_equals(opaque.get_security_token(scope)));
        let probe = page_element(scope, root_document, "probe");
        let child_probe = page_element(scope, child_document, "probe");
        let wrong = invoke(
            scope,
            owner,
            "op_prepare_resource_script",
            &[root_document.into(), child_probe.into()],
        )
        .unwrap();
        assert!(v8::Local::new(scope, wrong).is_null());
        let already_claimed = invoke(
            scope,
            owner,
            "op_prepare_resource_script",
            &[root_document.into(), probe.into()],
        )
        .unwrap();
        assert!(v8::Local::new(scope, already_claimed).is_null());
        let code = v8::String::new(scope, ROOT_SCRIPT).unwrap();
        let source = v8::String::new(scope, "https://registered-contract.test/").unwrap();
        let shadow = v8::Boolean::new(scope, false);
        let blocked_error = invoke(
            scope,
            owner,
            "op_execute_inline_script",
            &[
                blocked_document.into(),
                probe.into(),
                code.into(),
                source.into(),
                shadow.into(),
            ],
        )
        .unwrap_err();
        assert!(blocked_error.contains("sandbox"));
        let opaque_error = invoke(
            scope,
            owner,
            "op_execute_inline_script",
            &[
                opaque_document.into(),
                probe.into(),
                code.into(),
                source.into(),
                shadow.into(),
            ],
        )
        .unwrap_err();
        assert!(opaque_error.contains("same-origin"));
        let invalid = v8::Integer::new(scope, 1);
        assert!(invoke(
            scope,
            owner,
            "op_execute_inline_script",
            &[
                invalid.into(),
                probe.into(),
                code.into(),
                source.into(),
                shadow.into()
            ]
        )
        .is_err());
        assert_eq!(
            global_own(scope, owner, "resourceRuns").uint32_value(scope),
            Some(1)
        );
        let executed = invoke(
            scope,
            owner,
            "op_execute_inline_script",
            &[
                root_document.into(),
                probe.into(),
                code.into(),
                source.into(),
                shadow.into(),
            ],
        )
        .unwrap();
        assert!(boolean(scope, executed));
        assert_eq!(
            global_own(scope, owner, "resourceRuns").uint32_value(scope),
            Some(2)
        );
        assert!(global_own(scope, owner, "resourceCurrent").is_true());
        assert_eq!(
            global_own(scope, child, "childRuns").uint32_value(scope),
            Some(1)
        );
        v8::Global::new(scope, child_document)
    };
    page.eval_expression("document.getElementById('first').remove();'removed'")
        .await
        .unwrap();
    {
        deno_core::scope!(scope, &mut page.rt);
        let owner = v8::Local::new(scope, &root);
        let old_document = v8::Local::new(scope, &retained);
        let old_context = old_document.get_creation_context(scope).unwrap();
        assert!(native_frame_is_retired(old_context));
        let root_document = document(scope, owner);
        let probe = page_element(scope, root_document, "probe");
        let rejected = invoke(
            scope,
            owner,
            "op_prepare_resource_script",
            &[old_document.into(), probe.into()],
        )
        .unwrap();
        assert!(v8::Local::new(scope, rejected).is_null());
        let code = v8::String::new(scope, "globalThis.resourceRuns=999").unwrap();
        let source = v8::String::new(scope, "https://registered-contract.test/").unwrap();
        let shadow = v8::Boolean::new(scope, false);
        assert!(invoke(
            scope,
            owner,
            "op_execute_inline_script",
            &[
                old_document.into(),
                probe.into(),
                code.into(),
                source.into(),
                shadow.into()
            ]
        )
        .unwrap_err()
        .contains("replaced"));
        assert_eq!(
            global_own(scope, owner, "resourceRuns").uint32_value(scope),
            Some(2)
        );
    }
    assert_eq!(
        page.eval_expression("String(document.currentScript===null)")
            .await
            .unwrap(),
        "true"
    );
    healthy(&mut page).await;
    drop(retained);
    drop(root);
    drop(_activation);
    page.close();
}
