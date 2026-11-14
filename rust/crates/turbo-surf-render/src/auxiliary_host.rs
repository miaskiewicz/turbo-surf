//! Synchronous admission uses only real contexts in the current actor. Loading
//! and driver callbacks are queued until the canonical identity is published.
use super::*;
use turbo_dom_browser_env::host_internals as binding;

pub(super) fn install_root(scope: &mut v8::PinScope, url: &str) -> Result<(), String> {
    if !JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<auxiliary_pages::Pages>()
        .enabled
    {
        return Ok(());
    }
    let context = scope.get_current_context();
    if binding::auxiliary_context_identity(context).is_some() {
        return Ok(());
    }
    install_hyperlink_handler(context);
    binding::set_auxiliary_ready_host(context, Some(ready));
    binding::install_auxiliary_window_hooks(context, hooks());
    if !binding::install_auxiliary_context(scope, context, url, Some(host())) {
        return Err("Native root browsing-context installation failed".into());
    }
    auxiliary_pages::register_root(scope, context, url);
    let frames = JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<FrameContexts>()
        .contexts
        .clone();
    for frame in frames {
        let frame = v8::Local::new(scope, frame);
        if binding::auxiliary_context_identity(frame).is_some() {
            continue;
        }
        let Some(parent) = frame
            .get_slot::<FrameParent>()
            .and_then(|parent| parent.0.to_local(scope))
        else {
            continue;
        };
        let url = frame
            .get_slot::<FrameDocumentLocation>()
            .map(|url| url.0.clone())
            .unwrap_or_else(|| "about:blank".into());
        let name = frame
            .get_slot::<FrameHost>()
            .and_then(|host| host.0.to_local(scope))
            .and_then(|host| crate::browser_env::node_attribute(scope, host, "name"))
            .unwrap_or_default();
        install_nested(scope, frame, parent, &url, &name)?;
    }
    Ok(())
}

fn host() -> binding::AuxiliaryHost {
    binding::AuxiliaryHost { open, close }
}

pub(super) fn hooks() -> binding::AuxiliaryWindowHooks {
    binding::AuxiliaryWindowHooks {
        location: Some(location),
        navigate: Some(navigate),
        post_message: Some(post_message),
        children: Some(frame_children),
        ..Default::default()
    }
}

fn blocked(context: v8::Local<v8::Context>) -> bool {
    context
        .get_slot::<FrameSandbox>()
        .is_some_and(|sandbox| sandbox.popups_blocked)
        || context
            .get_slot::<CspHandle>()
            .is_some_and(|csp| csp.policy.borrow().current().blocks_auxiliary_open())
}

fn open<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    source: v8::Local<'s, v8::Context>,
    selected: Option<v8::Local<'s, v8::Context>>,
    request: &binding::OpenRequest,
) -> Option<v8::Local<'s, v8::Context>> {
    if source.get_slot::<FrameRetired>().is_some() || blocked(source) {
        return None;
    }
    let _reservation = auxiliary_policy::reserve(scope, request)?;
    let snapshot =
        auxiliary_navigation::SourceSnapshot::capture(scope, source, request.target_id).ok()?;
    let target = if let Some(selected) = selected {
        selected
    } else {
        match allocate(scope, source, request) {
            Ok(context) => context,
            Err(error) => {
                let message = v8::String::new(scope, &error.to_string())?;
                let exception = v8::Exception::error(scope, message);
                scope.throw_exception(exception);
                return None;
            }
        }
    };
    let source_page = auxiliary_pages::top_level_identity(scope, source);
    auxiliary_pages::admitted(scope, target, request, source_page, snapshot);
    Some(target)
}

fn close<'s>(scope: &mut v8::PinScope<'s, '_>, target: v8::Local<'s, v8::Context>) -> bool {
    auxiliary_pages::close(scope, target)
}

fn allocate<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    source: v8::Local<'s, v8::Context>,
    request: &binding::OpenRequest,
) -> Result<v8::Local<'s, v8::Context>, JsErrorBox> {
    let core = source
        .get_slot::<RuntimeCore>()
        .and_then(|core| core.0.to_local(scope))
        .ok_or_else(|| JsErrorBox::generic("Native auxiliary runtime core unavailable"))?;
    let context = binding::create_auxiliary_window_context(scope, None);
    context.set_security_token(source.get_security_token(scope));
    // The owning actor owns deno_core's allocation until its last Page closes.
    for slot in [
        deno_core::CONTEXT_STATE_SLOT_INDEX,
        deno_core::MODULE_MAP_SLOT_INDEX,
    ] {
        let pointer = source.get_aligned_pointer_from_embedder_data(slot);
        // SAFETY: auxiliary contexts never decrement these borrowed actor pointers.
        unsafe {
            context.set_aligned_pointer_in_embedder_data(slot, pointer);
        }
    }
    initialize(scope, context, source, core.into(), request)?;
    Ok(context)
}

fn initialize<'s>(
    outer: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    source: v8::Local<'s, v8::Context>,
    core: v8::Local<'s, v8::Value>,
    request: &binding::OpenRequest,
) -> Result<(), JsErrorBox> {
    let fallback_base = {
        let scope = &mut v8::ContextScope::new(outer, source);
        call_frame_helper(scope, "documentBaseURL", &[])
            .map(|base| base.to_rust_string_lossy(scope))
            .ok_or_else(|| JsErrorBox::generic("Auxiliary creator base URL unavailable"))?
    };
    context.set_slot(Rc::new(AuxiliaryFallbackBase(fallback_base)));
    let scope = &mut v8::ContextScope::new(outer, context);
    inherit_parent_csp(scope, context, source);
    auxiliary_navigation::inherit_opener_policy(scope, context, source);
    if let Some(sandbox) = source.get_slot::<FrameSandbox>() {
        context.set_slot(Rc::new(*sandbox));
    }
    crate::browser_env::defer_key_order(scope);
    binding::initialize_auxiliary_document(
        scope,
        "<!doctype html><html><head></head><body></body></html>",
    );
    install_css_authorization(scope)?;
    let facts = DocumentFacts::inherited(scope, context, source);
    install_document_facts(scope, facts);
    context.set_slot(Rc::new(FrameDocumentLocation("about:blank".into())));
    let source_url = source
        .get_slot::<FrameDocumentUrl>()
        .map(|url| url.0.clone())
        .unwrap_or_else(|| {
            JsRuntime::op_state_from(scope)
                .borrow()
                .borrow::<Base>()
                .0
                .clone()
        });
    if let Some(csp) = context.get_slot::<CspHandle>() {
        csp.response_metadata
            .replace(crate::DocumentResponseMetadata {
                status: 0,
                referrer: if request.noreferrer {
                    String::new()
                } else {
                    source_url.clone()
                },
            });
    }
    context.set_slot(Rc::new(FrameDocumentUrl(source_url)));
    auxiliary_storage::install_new(scope, context, source, !request.noopener)?;
    let namespace = crate::browser_env::engine_namespace(scope)
        .ok_or_else(|| JsErrorBox::generic("Native auxiliary environment namespace unavailable"))?;
    let key = v8::String::new(scope, "isAuxiliary").unwrap();
    let flag = v8::Boolean::new(scope, true);
    namespace.set(scope, key.into(), flag.into());
    context.set_slot(Rc::new(FrameEnvironment(
        crate::context_ownership::object(scope, namespace)
            .ok_or_else(|| JsErrorBox::generic("Auxiliary namespace ownership interrupted"))?,
    )));
    context.set_slot(Rc::new(BootstrapDepth(0)));
    install_hyperlink_handler(context);
    window_proxy::install_location(scope, "about:blank");
    install_environment(scope, core).map_err(JsErrorBox::generic)?;
    let core = core
        .to_object(scope)
        .ok_or_else(|| JsErrorBox::generic("Auxiliary core unavailable"))?;
    let secure = secure_context(scope);
    crate::crypto_backend::install(scope, core, secure).map_err(JsErrorBox::generic)?;
    crate::audio_backend::install(scope, core).map_err(JsErrorBox::generic)?;
    crate::geometry_backend::install(scope, core).map_err(JsErrorBox::generic)?;
    binding::install_auxiliary_window_hooks(context, hooks());
    binding::capture_auxiliary_window_exception(scope, context);
    crate::browser_env::finish_key_order(scope);
    // The producer installs opener/name/identity after this callback returns.
    // Guest init scripts and document loads cannot run inside admission.
    Ok(())
}

fn location<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    target: v8::Local<'s, v8::Context>,
) -> Option<v8::Local<'s, v8::Value>> {
    window_proxy::protected_location(scope, target).map(Into::into)
}

fn navigate<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    source: v8::Local<'s, v8::Context>,
    target: v8::Local<'s, v8::Context>,
    value: v8::Local<'s, v8::Value>,
) -> bool {
    if sandbox_blocks_top_navigation(source, target) {
        return false;
    }
    let Some(identity) = binding::auxiliary_context_identity(target) else {
        return false;
    };
    let Some(source_id) = binding::auxiliary_context_identity(source).map(|source| source.id)
    else {
        return false;
    };
    let Some(text) = value.to_string(scope) else {
        return false;
    };
    let base = source
        .get_slot::<FrameScriptPolicy>()
        .map(|policy| policy.base.clone())
        .or_else(|| {
            source
                .get_slot::<FrameDocumentUrl>()
                .map(|url| url.0.clone())
        });
    let Some(url) = base
        .and_then(|base| ModuleSpecifier::parse(&base).ok())
        .and_then(|base| base.join(&text.to_rust_string_lossy(scope)).ok())
    else {
        return false;
    };
    auxiliary_pages::queue_navigation(
        scope,
        identity.id,
        source_id,
        source,
        url.to_string(),
        false,
    )
}

fn post_message<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    source: v8::Local<'s, v8::Context>,
    target: v8::Local<'s, v8::Context>,
    message: v8::Local<'s, v8::Value>,
    target_origin: v8::Local<'s, v8::Value>,
    transfer: v8::Local<'s, v8::Value>,
) -> bool {
    let Some(callback) = host_capability(scope, source, "postWindowMessage") else {
        return false;
    };
    let window = target.global(scope);
    let scope = &mut v8::ContextScope::new(scope, source);
    let receiver = v8::undefined(scope);
    // A clone rejection is still handled; preserve its pending guest exception.
    let _ = callback.call(
        scope,
        receiver.into(),
        &[window.into(), message, target_origin, transfer],
    );
    true
}

pub(super) fn install_nested(
    scope: &mut v8::PinScope,
    context: v8::Local<v8::Context>,
    parent: v8::Local<v8::Context>,
    url: &str,
    name: &str,
) -> Result<(), String> {
    install_hyperlink_handler(context);
    binding::set_auxiliary_ready_host(context, Some(ready));
    binding::install_auxiliary_window_hooks(context, hooks());
    if !binding::install_nested_auxiliary_source(
        scope,
        context,
        parent,
        url,
        &name.encode_utf16().collect::<Vec<_>>(),
        Some(host()),
    ) {
        return Err("Native nested opening-source registration failed".into());
    }
    Ok(())
}

pub(super) fn ready<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    _source: v8::Local<'s, v8::Context>,
    target: v8::Local<'s, v8::Context>,
    _request: &binding::OpenRequest,
) -> binding::AuxiliaryReadyAction {
    v8::tc_scope!(let scope, scope);
    auxiliary_pages::publish(scope);
    if let Err(error) = run_context_bootstrap(scope, target) {
        if scope.has_caught() {
            scope.rethrow();
            return binding::AuxiliaryReadyAction::Abort;
        }
        if let Some(message) = v8::String::new(scope, &error) {
            let error = v8::Exception::error(scope, message);
            scope.throw_exception(error);
        }
        return binding::AuxiliaryReadyAction::Abort;
    }
    auxiliary_pages::publish(scope);
    binding::AuxiliaryReadyAction::InitializedByHost
}

pub(super) fn run_context_bootstrap(
    outer: &mut v8::PinScope,
    target: v8::Local<v8::Context>,
) -> Result<(), String> {
    // Drop native state borrows before any guest script can reenter open/close.
    let bootstrap = JsRuntime::op_state_from(outer)
        .borrow()
        .borrow::<auxiliary_pages::Pages>()
        .bootstrap
        .clone();
    let scope = &mut v8::ContextScope::new(outer, target);
    binding::run_auxiliary_init_scripts(scope, target);
    run_bootstrap_sources(scope, &bootstrap)
}

pub(super) fn run_bootstrap_sources(
    scope: &mut v8::PinScope,
    bootstrap: &AuxiliaryBootstrap,
) -> Result<(), String> {
    if !bootstrap.engine_init_script.is_empty() {
        crate::browser_env::run_engine_script(scope, &bootstrap.engine_init_script)
            .ok_or("Auxiliary driver bootstrap failed")?;
    }
    if !bootstrap.init_script.is_empty() {
        let source = v8::String::new(scope, &bootstrap.init_script)
            .ok_or("Auxiliary init source allocation failed")?;
        v8::Script::compile(scope, source, None)
            .and_then(|script| script.run(scope))
            .ok_or("Auxiliary context init script failed")?;
    }
    Ok(())
}

pub(super) fn install_hyperlink_handler(context: v8::Local<v8::Context>) {
    binding::set_hyperlink_activation_handler(context, Some(dispatch_anchor_default));
}

/// The binding has already selected an uncancelled native anchor activation.
/// A null Window result also covers denial/noopener, so this bool records only
/// ownership of the default action, never Page creation.
fn dispatch_anchor_default<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    node: v8::Local<'s, v8::Object>,
) -> bool {
    use turbo_dom_browser_env::host_internals::{
        enter_document_of, handle_of, is_connected, with_tree, wrap,
    };
    let context = scope.get_current_context();
    if binding::auxiliary_context_identity(context).is_none()
        || context.get_slot::<FrameRetired>().is_some()
    {
        return false;
    }
    let _document = enter_document_of(scope, node);
    let Some(start) = handle_of(scope, node) else {
        return false;
    };
    let anchor = with_tree(|tree| {
        let mut current = Some(start);
        while let Some(node) = current {
            if matches!(tree.local_name(node), Some("a" | "area")) {
                return (is_connected(tree, node)
                    && tree.get_attribute(node, "href").is_some()
                    && tree.get_attribute(node, "download").is_none())
                .then_some(node);
            }
            current = tree.parent(node);
        }
        None
    })
    .flatten();
    let Some(anchor) = anchor else {
        return false;
    };
    let anchor = wrap(scope, anchor);
    binding::open_auxiliary_hyperlink(scope, anchor);
    true
}

/// The protected driver reads the native dispatch receipt without activating twice.
#[op2(fast, reentrant)]
pub(super) fn op_auxiliary_anchor_default(
    scope: &mut v8::PinScope,
    event: v8::Local<v8::Object>,
) -> bool {
    binding::hyperlink_activation_handled(scope, event)
}

pub(super) fn finish_nested_navigation<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    previous: Option<v8::Local<'s, v8::Context>>,
    parent: v8::Local<'s, v8::Context>,
    host: v8::Local<'s, v8::Object>,
    metadata_location: &str,
) -> Result<(), JsErrorBox> {
    if turbo_dom_browser_env::host_internals::auxiliary_context_identity(parent).is_some() {
        if let Some(previous) = previous.filter(|previous| {
            turbo_dom_browser_env::host_internals::auxiliary_context_identity(*previous).is_some()
        }) {
            if !turbo_dom_browser_env::host_internals::commit_auxiliary_navigation(
                scope,
                previous,
                context,
                metadata_location,
            ) {
                return Err(JsErrorBox::generic(
                    "Native nested navigation identity commit failed",
                ));
            }
        } else {
            let name = crate::browser_env::node_attribute(scope, host, "name").unwrap_or_default();
            install_nested(scope, context, parent, metadata_location, &name)
                .map_err(JsErrorBox::generic)?;
        }
    }
    Ok(())
}

pub(super) fn install_root_document_html(
    scope: &mut v8::PinScope,
    html: &str,
    source_positions: bool,
) {
    if JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<auxiliary_pages::Pages>()
        .enabled
    {
        if source_positions {
            turbo_dom_browser_env::host_internals::initialize_root_auxiliary_utf16_source_positions(
                scope,
                &html.encode_utf16().collect::<Vec<_>>(),
            );
        } else {
            turbo_dom_browser_env::host_internals::initialize_root_auxiliary_document(scope, html);
        }
    } else if source_positions {
        crate::browser_env::install_with_html_source_positions(scope, html);
    } else {
        crate::browser_env::install_html(scope, html);
    }
}
