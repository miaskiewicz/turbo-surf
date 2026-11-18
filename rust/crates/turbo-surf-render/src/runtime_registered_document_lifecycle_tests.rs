//! Registered native document services preserve ownership across frame retirement.
use super::*;

fn own<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
) -> v8::Local<'s, v8::Value> {
    private_boundary::frame_probe::own_value(scope, object, name)
        .expect("fixture own data property")
}

fn registered<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    root: v8::Local<'s, v8::Context>,
    name: &str,
) -> v8::Local<'s, v8::Function> {
    assert!(matches!(
        name,
        "op_frame_host_helper"
            | "op_shim_binding_namespace"
            | "op_retire_frame"
            | "op_queue_frame_script"
            | "op_frame_move_candidates"
            | "op_frame_load_attempt"
            | "op_frame_set_document"
    ));
    let core = root.get_slot::<RuntimeCore>().expect("real runtime core");
    let core = crate::context_ownership::local(scope, &core.0);
    let ops = own(scope, core, "ops");
    let ops = v8::Local::<v8::Object>::try_from(ops).unwrap();
    v8::Local::<v8::Function>::try_from(own(scope, ops, name)).unwrap()
}

fn invoke<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    root: v8::Local<'s, v8::Context>,
    name: &str,
    arguments: &[v8::Local<'s, v8::Value>],
) -> Result<v8::Global<v8::Value>, String> {
    assert_eq!(scope.get_current_context(), root);
    assert_eq!(scope.get_entered_or_microtask_context(), root);
    let function = registered(scope, root, name);
    assert!(function.get_creation_context(scope).is_some());
    v8::tc_scope!(let scope, scope);
    let receiver = v8::undefined(scope);
    match function.call(scope, receiver.into(), arguments) {
        Some(value) => Ok(v8::Global::new(scope, value)),
        None => {
            assert!(!scope.has_terminated());
            let exception = scope.exception().expect("actual registered op exception");
            Err(exception.to_rust_string_lossy(scope))
        }
    }
}

struct References {
    root: v8::Global<v8::Context>,
    document: v8::Global<v8::Object>,
    first_host: v8::Global<v8::Object>,
    second_host: v8::Global<v8::Object>,
    first: v8::Global<v8::Context>,
    second: v8::Global<v8::Context>,
    first_document: v8::Global<v8::Object>,
    second_document: v8::Global<v8::Object>,
}

fn child_context<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    contexts: &[v8::Global<v8::Context>],
    host: v8::Local<'s, v8::Object>,
) -> v8::Local<'s, v8::Context> {
    for candidate in contexts {
        let context = v8::Local::new(scope, candidate);
        let same_host = context.get_slot::<FrameHost>().is_some_and(|saved| {
            saved
                .0
                .to_local(scope)
                .is_some_and(|saved| saved.strict_equals(host.into()))
        });
        if context.get_slot::<FrameRetired>().is_none() && same_host {
            return context;
        }
    }
    panic!("actual child identified by its native host element")
}

async fn fixture(opaque_second: bool) -> (PageSession, References) {
    let sandbox = if opaque_second {
        "sandbox=allow-scripts"
    } else {
        ""
    };
    let html = format!(
        "<body><p id=stable>root</p><section id=container><iframe id=first srcdoc='<body><p>first</p><iframe id=nested srcdoc=\"nested\"></iframe>'></iframe></section><iframe id=second {sandbox} srcdoc='<body>second'></iframe></body>"
    );
    let mut page = PageSession::open(&html, "https://document-boundary.test/", "", "", 5000)
        .await
        .unwrap();
    page.eval("globalThis.firstHost=document.querySelector('#first');globalThis.secondHost=document.querySelector('#second');globalThis.container=document.querySelector('#container')")
        .await
        .unwrap();
    let references = {
        let _activation = page.rt.activation_key().enter();
        let root = private_infrastructure::context(&mut page.rt).unwrap();
        let contexts = page
            .rt
            .op_state()
            .borrow()
            .borrow::<FrameContexts>()
            .contexts
            .clone();
        deno_core::scope!(scope, page.rt);
        let root = v8::Local::new(scope, root);
        let scope = &mut v8::ContextScope::new(scope, root);
        let document = root
            .get_slot::<ActiveDocument>()
            .unwrap()
            .0
            .to_local(scope)
            .unwrap();
        let global = root.global(scope);
        let first_host =
            v8::Local::<v8::Object>::try_from(own(scope, global, "firstHost")).unwrap();
        let second_host =
            v8::Local::<v8::Object>::try_from(own(scope, global, "secondHost")).unwrap();
        let first = child_context(scope, &contexts, first_host);
        let second = child_context(scope, &contexts, second_host);
        let first_document = first
            .get_slot::<ActiveDocument>()
            .unwrap()
            .0
            .to_local(scope)
            .unwrap();
        let second_document = second
            .get_slot::<ActiveDocument>()
            .unwrap()
            .0
            .to_local(scope)
            .unwrap();
        References {
            root: v8::Global::new(scope, root),
            document: v8::Global::new(scope, document),
            first_host: v8::Global::new(scope, first_host),
            second_host: v8::Global::new(scope, second_host),
            first: v8::Global::new(scope, first),
            second: v8::Global::new(scope, second),
            first_document: v8::Global::new(scope, first_document),
            second_document: v8::Global::new(scope, second_document),
        }
    };
    (page, references)
}

fn assert_current_documents(scope: &mut v8::PinScope, refs: &References) {
    for (context, document) in [
        (&refs.first, &refs.first_document),
        (&refs.second, &refs.second_document),
    ] {
        let context = v8::Local::new(scope, context);
        assert!(context.get_slot::<FrameRetired>().is_none());
        let actual = context
            .get_slot::<ActiveDocument>()
            .unwrap()
            .0
            .to_local(scope)
            .unwrap();
        let expected = v8::Local::new(scope, document);
        assert!(actual.strict_equals(expected.into()));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn registered_frame_helper_rejects_foreign_documents_without_mutation() {
    let (mut page, refs) = fixture(true).await;
    let foreign_error;
    {
        let _activation = page.rt.activation_key().enter();
        deno_core::scope!(scope, page.rt);
        let root = v8::Local::new(scope, &refs.root);
        let scope = &mut v8::ContextScope::new(scope, root);
        let document = v8::Local::new(scope, &refs.document);
        let host = v8::Local::new(scope, &refs.first_host);
        let name = v8::String::new(scope, "frameRecordFor").unwrap();
        let args = v8::Array::new_with_elements(scope, &[host.into()]);
        let record = invoke(
            scope,
            root,
            "op_frame_host_helper",
            &[document.into(), name.into(), args.into()],
        )
        .unwrap();
        let record = v8::Local::new(scope, record).to_object(scope).unwrap();
        let owner = own(scope, record, "owner");
        assert!(owner.strict_equals(root.global(scope).into()));
        let unknown = v8::String::new(scope, "notAFrameOperation").unwrap();
        assert!(invoke(
            scope,
            root,
            "op_frame_host_helper",
            &[document.into(), unknown.into(), args.into()]
        )
        .unwrap_err()
        .contains("Unknown private frame operation"));
        let foreign_context = v8::Local::new(scope, &refs.second);
        assert!(!root
            .get_security_token(scope)
            .strict_equals(foreign_context.get_security_token(scope)));
        let foreign_document = v8::Local::new(scope, &refs.second_document);
        foreign_error = invoke(
            scope,
            root,
            "op_frame_host_helper",
            &[foreign_document.into(), name.into(), args.into()],
        )
        .unwrap_err();
        assert_current_documents(scope, &refs);
    }
    assert_eq!(page.read_engine("return [document.querySelector('#stable').textContent,firstHost.contentDocument.body.textContent,typeof Deno]").unwrap(), "[\"root\",\"first\",\"undefined\"]");
    assert!(
        foreign_error.contains("Frame owner is cross-origin"),
        "actual registered rejection after document identity/Root health checks: {foreign_error:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn registered_retirement_requires_the_owner_and_revokes_child_services() {
    let (mut page, refs) = fixture(false).await;
    {
        let _activation = page.rt.activation_key().enter();
        deno_core::scope!(scope, page.rt);
        let root = v8::Local::new(scope, &refs.root);
        let scope = &mut v8::ContextScope::new(scope, root);
        let child = v8::Local::new(scope, &refs.first);
        let window = child.global(scope);
        let code = v8::String::new(scope, "globalThis.registeredLiveSourceRan=true").unwrap();
        let url = v8::String::new(scope, "https://document-boundary.test/live.js").unwrap();
        let response = v8::Integer::new_from_unsigned(scope, 0);
        let index = v8::Integer::new_from_unsigned(scope, u32::MAX);
        let promise = invoke(
            scope,
            root,
            "op_queue_frame_script",
            &[
                window.into(),
                code.into(),
                url.into(),
                response.into(),
                index.into(),
            ],
        )
        .unwrap();
        assert!(v8::Local::new(scope, promise).is_promise());
    }
    page.eval("").await.unwrap();
    assert_eq!(
        page.read_engine("return firstHost.contentWindow.registeredLiveSourceRan")
            .unwrap(),
        "true"
    );
    {
        let _activation = page.rt.activation_key().enter();
        deno_core::scope!(scope, page.rt);
        let root = v8::Local::new(scope, &refs.root);
        let scope = &mut v8::ContextScope::new(scope, root);
        let child = v8::Local::new(scope, &refs.first);
        let sibling = v8::Local::new(scope, &refs.second);
        let child_window = child.global(scope);
        let sibling_window = sibling.global(scope);
        let root_window = root.global(scope);
        let namespace = invoke(
            scope,
            root,
            "op_shim_binding_namespace",
            &[child_window.into()],
        )
        .unwrap();
        assert!(v8::Local::new(scope, namespace).is_object());
        assert!(invoke(
            scope,
            root,
            "op_retire_frame",
            &[child_window.into(), sibling_window.into()]
        )
        .unwrap_err()
        .contains("frame can only be retired by its owning context"));
        assert_current_documents(scope, &refs);
        let result = invoke(
            scope,
            root,
            "op_retire_frame",
            &[child_window.into(), root_window.into()],
        )
        .unwrap();
        // Unmarked Rust unit returns use deno_core's registered null ABI.
        assert!(v8::Local::new(scope, result).is_null());
        assert!(child.get_slot::<FrameRetired>().is_some());
        assert!(sibling.get_slot::<FrameRetired>().is_none());
        let absent = invoke(
            scope,
            root,
            "op_shim_binding_namespace",
            &[child_window.into()],
        )
        .unwrap();
        let absent = v8::Local::new(scope, absent);
        assert!(absent.is_null_or_undefined());
        let document = v8::Local::new(scope, &refs.first_document);
        let name = v8::String::new(scope, "syncFrameConnections").unwrap();
        let args = v8::Array::new(scope, 0);
        let result = invoke(
            scope,
            root,
            "op_frame_host_helper",
            &[document.into(), name.into(), args.into()],
        )
        .unwrap();
        assert!(v8::Local::new(scope, result).is_undefined());
        let code = v8::String::new(scope, "document.body.textContent='must not execute'").unwrap();
        let url = v8::String::new(scope, "https://document-boundary.test/retired.js").unwrap();
        let response = v8::Integer::new_from_unsigned(scope, 0);
        let index = v8::Integer::new_from_unsigned(scope, u32::MAX);
        assert!(invoke(
            scope,
            root,
            "op_queue_frame_script",
            &[
                child_window.into(),
                code.into(),
                url.into(),
                response.into(),
                index.into()
            ]
        )
        .unwrap_err()
        .contains("iframe document has been replaced"));
        let expected = v8::Local::new(scope, &refs.second_document);
        let actual = sibling
            .get_slot::<ActiveDocument>()
            .unwrap()
            .0
            .to_local(scope)
            .unwrap();
        assert!(actual.strict_equals(expected.into()));
    }
    assert_eq!(page.read_engine("return [document.querySelector('#stable').textContent,secondHost.contentDocument.body.textContent,typeof Deno]").unwrap(), "[\"root\",\"second\",\"undefined\"]");
}

fn candidate_hosts<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    root: v8::Local<'s, v8::Context>,
    document: v8::Local<'s, v8::Object>,
    nodes: &[v8::Local<'s, v8::Value>],
) -> Vec<v8::Global<v8::Object>> {
    let nodes = v8::Array::new_with_elements(scope, nodes);
    let result = invoke(
        scope,
        root,
        "op_frame_move_candidates",
        &[document.into(), nodes.into()],
    )
    .unwrap();
    let result = v8::Local::new(scope, result);
    let result = v8::Local::<v8::Array>::try_from(result).unwrap();
    (0..result.length())
        .map(|index| {
            let host = result
                .get_index(scope, index)
                .unwrap()
                .to_object(scope)
                .unwrap();
            v8::Global::new(scope, host)
        })
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn registered_move_candidates_follow_real_owner_subtrees_and_retirement() {
    let (mut page, refs) = fixture(false).await;
    {
        let _activation = page.rt.activation_key().enter();
        let contexts = page
            .rt
            .op_state()
            .borrow()
            .borrow::<FrameContexts>()
            .contexts
            .clone();
        deno_core::scope!(scope, page.rt);
        let root = v8::Local::new(scope, &refs.root);
        let scope = &mut v8::ContextScope::new(scope, root);
        let document = v8::Local::new(scope, &refs.document);
        let first = v8::Local::new(scope, &refs.first);
        let first_host = v8::Local::new(scope, &refs.first_host);
        let second_host = v8::Local::new(scope, &refs.second_host);
        let global = root.global(scope);
        let container = v8::Local::<v8::Object>::try_from(own(scope, global, "container")).unwrap();
        let primitive = v8::Integer::new(scope, 7);
        let hosts = candidate_hosts(scope, root, document, &[primitive.into(), container.into()]);
        assert_eq!(hosts.len(), 1);
        assert!(v8::Local::new(scope, &hosts[0]).strict_equals(first_host.into()));
        let all = candidate_hosts(scope, root, document, &[document.into()]);
        assert_eq!(all.len(), 2);
        assert!(all
            .iter()
            .any(|host| v8::Local::new(scope, host).strict_equals(first_host.into())));
        assert!(all
            .iter()
            .any(|host| v8::Local::new(scope, host).strict_equals(second_host.into())));
        let mut nested = None;
        for candidate in &contexts {
            let context = v8::Local::new(scope, candidate);
            let same_parent = context
                .get_slot::<FrameParent>()
                .is_some_and(|parent| crate::context_ownership::local(scope, &parent.0) == first);
            if context.get_slot::<FrameRetired>().is_none() && same_parent {
                nested = Some(context);
                break;
            }
        }
        let nested = nested.expect("actual nested child");
        let nested_host = nested
            .get_slot::<FrameHost>()
            .unwrap()
            .0
            .to_local(scope)
            .unwrap();
        let first_document = v8::Local::new(scope, &refs.first_document);
        let nested_hosts = candidate_hosts(scope, root, first_document, &[first_document.into()]);
        assert_eq!(nested_hosts.len(), 1);
        assert!(v8::Local::new(scope, &nested_hosts[0]).strict_equals(nested_host.into()));
        let window = first.global(scope);
        let owner = root.global(scope);
        invoke(
            scope,
            root,
            "op_retire_frame",
            &[window.into(), owner.into()],
        )
        .unwrap();
        assert!(first.get_slot::<FrameRetired>().is_some());
        assert!(nested.get_slot::<FrameRetired>().is_some());
        assert!(candidate_hosts(scope, root, document, &[container.into()]).is_empty());
        let remaining = candidate_hosts(scope, root, document, &[document.into()]);
        assert_eq!(remaining.len(), 1);
        assert!(v8::Local::new(scope, &remaining[0]).strict_equals(second_host.into()));
    }
    assert_eq!(page.read_engine("return [document.querySelector('#stable').textContent,secondHost.contentDocument.body.textContent]").unwrap(), "[\"root\",\"second\"]");
}

fn begin_attempt<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    root: v8::Local<'s, v8::Context>,
    host: v8::Local<'s, v8::Object>,
) -> v8::Global<v8::Value> {
    let owner = root.global(scope);
    let attempt = invoke(
        scope,
        root,
        "op_frame_load_attempt",
        &[owner.into(), host.into()],
    )
    .unwrap();
    assert!(v8::Local::new(scope, &attempt).is_object());
    attempt
}

#[tokio::test(flavor = "current_thread")]
async fn registered_document_replacement_rejects_a_different_real_host_without_retiring_current() {
    let (mut page, refs) = fixture(false).await;
    {
        let _activation = page.rt.activation_key().enter();
        deno_core::scope!(scope, page.rt);
        let root = v8::Local::new(scope, &refs.root);
        let scope = &mut v8::ContextScope::new(scope, root);
        let first = v8::Local::new(scope, &refs.first);
        let first_host = v8::Local::new(scope, &refs.first_host);
        let second_host = v8::Local::new(scope, &refs.second_host);
        let owner = root.global(scope);
        let attempt = begin_attempt(scope, root, first_host);
        let attempt = v8::Local::new(scope, attempt);
        let window = first.global(scope);
        let html = v8::String::new(scope, "<body><p id=replaced>replacement</p>").unwrap();
        let metadata = v8::String::new(scope, "{\"url\":\"https://document-boundary.test/\",\"origin\":\"https://document-boundary.test\",\"location\":\"about:srcdoc\",\"inherited\":true,\"depth\":1,\"referrer\":\"https://document-boundary.test/\"}").unwrap();
        let response = v8::Integer::new_from_unsigned(scope, 0);
        let replacement = invoke(
            scope,
            root,
            "op_frame_set_document",
            &[
                window.into(),
                owner.into(),
                first_host.into(),
                html.into(),
                metadata.into(),
                response.into(),
                attempt,
            ],
        )
        .unwrap();
        let replacement = v8::Local::new(scope, replacement).to_object(scope).unwrap();
        let current = replacement.get_creation_context(scope).unwrap();
        assert_ne!(current, first);
        assert!(first.get_slot::<FrameRetired>().is_some());
        assert!(current.get_slot::<FrameRetired>().is_none());
        let document = current
            .get_slot::<ActiveDocument>()
            .unwrap()
            .0
            .to_local(scope)
            .unwrap();
        assert!(!document.strict_equals(v8::Local::new(scope, &refs.first_document).into()));
        {
            let scope = &mut v8::ContextScope::new(scope, current);
            let key = v8::String::new(scope, "body").unwrap();
            let body = document
                .get(scope, key.into())
                .unwrap()
                .to_object(scope)
                .unwrap();
            let key = v8::String::new(scope, "textContent").unwrap();
            assert_eq!(
                body.get(scope, key.into())
                    .unwrap()
                    .to_rust_string_lossy(scope),
                "replacement"
            );
        }
        let attempt = begin_attempt(scope, root, second_host);
        let attempt = v8::Local::new(scope, attempt);
        let rejected = invoke(
            scope,
            root,
            "op_frame_set_document",
            &[
                replacement.into(),
                owner.into(),
                second_host.into(),
                html.into(),
                metadata.into(),
                response.into(),
                attempt,
            ],
        );
        assert!(rejected
            .unwrap_err()
            .contains("iframe host cannot be replaced"));
        assert!(current.get_slot::<FrameRetired>().is_none());
        let after = current
            .get_slot::<ActiveDocument>()
            .unwrap()
            .0
            .to_local(scope)
            .unwrap();
        assert!(after.strict_equals(document.into()));
        let actual_host = current
            .get_slot::<FrameHost>()
            .unwrap()
            .0
            .to_local(scope)
            .unwrap();
        assert!(actual_host.strict_equals(first_host.into()));
        let sibling = v8::Local::new(scope, &refs.second);
        assert!(sibling.get_slot::<FrameRetired>().is_none());
        let sibling_document = sibling
            .get_slot::<ActiveDocument>()
            .unwrap()
            .0
            .to_local(scope)
            .unwrap();
        let expected = v8::Local::new(scope, &refs.second_document);
        assert!(sibling_document.strict_equals(expected.into()));
        let namespace = invoke(
            scope,
            root,
            "op_shim_binding_namespace",
            &[replacement.into()],
        )
        .unwrap();
        assert!(v8::Local::new(scope, namespace).is_object());
    }
    assert_eq!(page.read_engine("return [document.querySelector('#stable').textContent,secondHost.contentDocument.body.textContent,typeof Deno]").unwrap(), "[\"root\",\"second\",\"undefined\"]");
}
