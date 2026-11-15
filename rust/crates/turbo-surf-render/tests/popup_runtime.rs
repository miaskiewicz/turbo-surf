//! Native Page ownership cannot be forged by copying a Window prototype.
use deno_core::{v8, JsRuntime, RuntimeOptions};
use turbo_dom_browser_env::host_internals as binding;

#[tokio::test(flavor = "current_thread")]
async fn producer_root_template_admits_only_native_contexts() {
    let mut runtime = JsRuntime::new(RuntimeOptions::default());
    deno_core::scope!(scope, runtime);
    let infrastructure = scope.get_current_context();
    // Two internal fields describe template compatibility, not native Page ownership.
    assert!(binding::auxiliary_context_identity(infrastructure).is_none());
    let guest = binding::create_auxiliary_window_context(scope, None);
    assert!(binding::is_auxiliary_window_context(scope, guest));
    {
        let scope = &mut v8::ContextScope::new(scope, guest);
        binding::initialize_auxiliary_document(scope, "<!doctype html><body>owned</body>");
        assert!(binding::install_auxiliary_context(
            scope,
            guest,
            "https://native.test/",
            None
        ));
    }
    let identity = binding::auxiliary_context_identity(guest).unwrap();
    assert!(!identity.closed);
    assert!(binding::auxiliary_context_identity(infrastructure).is_none());
    let ordinary = v8::Context::new(scope, Default::default());
    assert!(!binding::is_auxiliary_window_context(scope, ordinary));
    let prototype = {
        let scope = &mut v8::ContextScope::new(scope, guest);
        guest.global(scope).get_prototype(scope).unwrap()
    };
    {
        let scope = &mut v8::ContextScope::new(scope, ordinary);
        v8::tc_scope!(let scope, scope);
        let reflect_key = v8::String::new(scope, "Reflect").unwrap();
        let reflect = ordinary
            .global(scope)
            .get(scope, reflect_key.into())
            .unwrap()
            .to_object(scope)
            .unwrap();
        let key = v8::String::new(scope, "setPrototypeOf").unwrap();
        let set = reflect.get(scope, key.into()).unwrap();
        let set = v8::Local::<v8::Function>::try_from(set).unwrap();
        let global = ordinary.global(scope);
        let result = set.call(scope, reflect.into(), &[global.into(), prototype]);
        assert!(!scope.is_execution_terminating());
        if let Some(result) = result {
            assert!(result.is_boolean());
        } else {
            assert!(
                scope.has_caught(),
                "prototype assignment did not produce a result or exception"
            );
            scope.reset();
        }
    }
    assert!(!binding::is_auxiliary_window_context(scope, ordinary));
    assert!(binding::auxiliary_context_identity(ordinary).is_none());
    let retained = binding::auxiliary_context_identity(guest).unwrap();
    assert_eq!(retained.id, identity.id);
    assert_eq!(retained.creation_source, identity.creation_source);
    assert!(!retained.closed);
}

#[tokio::test]
async fn persistent_root_publishes_actual_capability_and_shared_actor_child() {
    use turbo_surf_render::{AuxiliaryCapability, PageSession};
    let mut session = PageSession::open(
        "<!doctype html><title>Root</title>",
        "https://popup.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let log = session.auxiliary_event_log();
    assert!(matches!(log.capability(), AuxiliaryCapability::Supported));
    let root = session.auxiliary_root_id().unwrap();
    session.eval_expression("globalThis.child = window.open('', 'child'); child.document.title = 'Owned child'; String(child.opener === window)").await.unwrap();
    let pages = log.pages();
    let child = pages
        .iter()
        .find(|page| page.source_page.as_deref() == Some(&root.to_string()))
        .unwrap();
    let child_id = child.target.parse().unwrap();
    assert_ne!(root, child_id);
    assert_eq!(
        session
            .eval_target_expression(child_id, "document.title", false, false)
            .await
            .unwrap(),
        "Owned child"
    );
    assert!(session.close_target(root).unwrap());
    assert_eq!(
        session
            .eval_target_expression(child_id, "document.title", false, false)
            .await
            .unwrap(),
        "Owned child"
    );
    assert!(!session.close_target(child_id).unwrap());
}

#[tokio::test]
async fn native_window_frame_accessors_survive_bootstrap_and_track_independent_children() {
    use turbo_surf_render::PageSession;
    let mut session = PageSession::open(
        "<!doctype html><title>Frames</title>",
        "https://popup.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let result = session.eval_expression(r#"(() => {
        const empty = window.length === 0 && window.frames === window;
        const frame = document.createElement('iframe');
        document.body.appendChild(frame);
        const root = window.length === 1 && window[0] === frame.contentWindow;
        const child = window.open('', 'independent');
        const independent = child.length === 0 && child.frames === child;
        const nested = child.document.createElement('iframe');
        child.document.body.appendChild(nested);
        const isolated = child.length === 1 && child[0] === nested.contentWindow && window.length === 1;
        frame.remove();
        return JSON.stringify([empty, root, independent, isolated, window.length === 0 && child.length === 1]);
    })()"#).await.unwrap();
    assert_eq!(result, "[true,true,true,true,true]");
}

#[tokio::test]
async fn blank_popup_inherits_frozen_base_without_changing_visible_location() {
    use turbo_surf_render::PageSession;
    let mut session = PageSession::open(
        "<!doctype html><base href='https://popup.test/assets/'>",
        "https://popup.test/opener/index.html",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let result = session.eval_expression(r#"(() => {
        const child = window.open('', 'inherited-base');
        const initial = child.location.href === 'about:blank' && child.document.URL === 'about:blank';
        const request = new child.Request('./child');
        const base = child.document.baseURI;
        document.querySelector('base').href = 'https://popup.test/changed/';
        return JSON.stringify([initial, request.url, base, child.document.baseURI]);
    })()"#).await.unwrap();
    assert_eq!(result, "[true,\"https://popup.test/assets/child\",\"https://popup.test/assets/\",\"https://popup.test/assets/\"]");
}
