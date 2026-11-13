use super::*;

#[op2]
#[serde]
pub(super) fn op_context_emulation<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    state: &OpState,
    document: v8::Local<'s, v8::Object>,
    #[string] name: &str,
) -> serde_json::Value {
    let Some(context) = document.get_creation_context(scope) else {
        return serde_json::json!({"active": false, "permission": "denied"});
    };
    let (connected, container_allowed) = frame_state(scope, context);
    let active = connected && active_document(scope, context, document);
    let origin = window_origin(scope, context);
    let (granted, position) = state.borrow::<BrowserStorage>().with_emulation(|options| {
        (
            crate::geolocation_config::granted(&options.permissions, &origin, name),
            options.geolocation.clone(),
        )
    });
    let denied = name == "geolocation"
        && (!container_allowed
            || !secure_context_for(scope, context)
            || window_is_opaque(scope, context)
            || !document_feature_enabled(scope, document, name));
    let permission = if denied {
        "denied"
    } else if granted {
        "granted"
    } else {
        ""
    };
    serde_json::json!({
        "active": active,
        "permission": permission,
        "position": position,
    })
}

fn active_document<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    document: v8::Local<'s, v8::Object>,
) -> bool {
    context.get_slot::<FrameRetired>().is_none()
        && context
            .get_slot::<ActiveDocument>()
            .and_then(|active| active.0.to_local(scope))
            .is_some_and(|active| active.strict_equals(document.into()))
}

fn frame_state<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    mut context: v8::Local<'s, v8::Context>,
) -> (bool, bool) {
    let mut allowed = true;
    while let Some(parent) = context.get_slot::<FrameParent>() {
        let Some(parent) = parent.0.to_local(scope) else {
            return (false, false);
        };
        let Some(host) = context
            .get_slot::<FrameHost>()
            .and_then(|host| host.0.to_local(scope))
        else {
            return (false, false);
        };
        if !connected_host(scope, host) {
            return (false, false);
        }
        let parent_origin = window_origin(scope, parent);
        let child_origin = window_origin(scope, context);
        let parent_policy = parent
            .get_slot::<CspHandle>()
            .map(|policy| policy.policy.borrow().permissions_policy.clone())
            .unwrap_or_default();
        let mut policy = crate::permissions_policy::PermissionsPolicy::default();
        policy.inherit(
            &parent_policy,
            &parent_origin,
            &child_origin,
            &child_origin,
            &frame_container_allow(scope, host),
        );
        allowed &= policy.allows_self("geolocation", &child_origin);
        context = parent;
    }
    (true, allowed)
}

fn connected_host(scope: &mut v8::PinScope, host: v8::Local<v8::Object>) -> bool {
    use turbo_dom_browser_env::host_internals::{
        enter_document_of, handle_of, is_connected, with_tree,
    };
    let _document = enter_document_of(scope, host);
    let Some(handle) = handle_of(scope, host) else {
        return false;
    };
    with_tree(|tree| is_connected(tree, handle)).unwrap_or(false)
}

pub(super) fn observing(rt: &mut JsRuntime) -> bool {
    let contexts = window_timer_contexts(rt);
    deno_core::scope!(scope, rt);
    for context in contexts {
        let context = v8::Local::new(scope, context);
        if context.get_slot::<FrameRetired>().is_some() {
            continue;
        }
        let active = &mut v8::ContextScope::new(scope, context);
        if call_frame_helper(active, "hasEmulationObservers", &[])
            .is_some_and(|value| value.boolean_value(active))
        {
            return true;
        }
    }
    false
}

pub(super) fn register_change_waiter(
    waiter: &mut std::pin::Pin<Box<tokio::sync::futures::OwnedNotified>>,
    changed: &std::sync::Arc<tokio::sync::Notify>,
    cx: &mut std::task::Context<'_>,
) {
    if std::future::Future::poll(waiter.as_mut(), cx).is_ready() {
        *waiter = Box::pin(changed.clone().notified_owned());
        if std::future::Future::poll(waiter.as_mut(), cx).is_ready() {
            // A second real update raced subscription renewal. Do not park
            // on an already-completed notification future.
            cx.waker().wake_by_ref();
        }
    }
}
