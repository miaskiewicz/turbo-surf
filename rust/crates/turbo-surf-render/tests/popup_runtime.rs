//! The Deno root must use the producer constructor, never a copied JS prototype.
use deno_core::{v8, JsRuntime, RuntimeOptions};
use turbo_dom_browser_env::host_internals as binding;

#[test]
fn producer_root_template_admits_only_native_contexts() {
    let mut extension = deno_core::Extension::default();
    extension.global_template_middleware =
        Some(|scope, _| binding::auxiliary_window_template(scope));
    let mut runtime = JsRuntime::new(RuntimeOptions {
        extensions: vec![extension],
        ..Default::default()
    });
    deno_core::scope!(scope, runtime);
    let root = scope.get_current_context();
    assert!(binding::is_auxiliary_window_context(scope, root));
    let ordinary = v8::Context::new(scope, Default::default());
    assert!(!binding::is_auxiliary_window_context(scope, ordinary));
    let native_global = root.global(scope);
    let prototype = native_global.get_prototype(scope).unwrap();
    let ordinary_global = ordinary.global(scope);
    ordinary_global.set_prototype(scope, prototype);
    assert!(!binding::is_auxiliary_window_context(scope, ordinary));
    assert!(binding::auxiliary_context_identity(ordinary).is_none());
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
