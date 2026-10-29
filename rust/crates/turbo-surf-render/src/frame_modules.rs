//! Child-frame ES modules on the native SDK: each frame realm registers its own
//! network module loader, and only module scripts its document authorized run.
//! Compiled only under `cfg(turbo_surf_native_proxy)`; measured in that configuration.
use super::*;

pub(super) fn register_frame_modules(
    scope: &mut v8::PinScope,
    base: &str,
) -> Result<(), JsErrorBox> {
    let context = scope.get_current_context();
    if context
        .get_slot::<FrameSandbox>()
        .is_some_and(|policy| policy.scripts_blocked)
    {
        return Ok(());
    }
    let state = JsRuntime::op_state_from(scope);
    let state = state.borrow();
    let opaque_origin = window_is_opaque(scope, context);
    let credentials = ModuleCredentials::default();
    context.set_slot(Rc::new(credentials.clone()));
    let cookie_site_is_opaque = frame_cookie_site_is_opaque(scope, context);
    let loader = NetModuleLoader {
        base: base.to_string(),
        source: context
            .get_slot::<FrameDocumentUrl>()
            .map_or_else(|| state.borrow::<Base>().0.clone(), |url| url.0.clone()),
        top_level_source: state.borrow::<Base>().0.clone(),
        origins: Rc::new(ModuleOrigins::new(opaque_origin, cookie_site_is_opaque)),
        jar: context
            .get_slot::<CspHandle>()
            .and_then(|handle| handle.partition.clone())
            .map_or_else(
                || state.borrow::<Jar>().clone(),
                |partition| partition.jar.clone(),
            ),
        ua: state.borrow::<Ua>().0.clone(),
        client_hints: state.borrow::<ClientHints>().clone(),
        http_cache: state.borrow::<BrowserStorage>().http_cache(),
        storage_effects: state.borrow::<BrowserStorage>().partition_response_effects(
            context
                .get_slot::<CspHandle>()
                .and_then(|handle| handle.partition.clone())
                .map_or(0, |partition| partition.id),
        ),
        inline_sources: state.borrow::<InlineModules>().clone(),
        credentials,
        csp: (*context
            .get_slot::<CspHandle>()
            .ok_or_else(|| JsErrorBox::generic("Frame CSP context unavailable"))?)
        .clone(),
        module_stages: state.borrow::<ModuleStages>().clone(),
    };
    drop(state);
    if JsRuntime::register_module_context(scope, Rc::new(loader)) {
        Ok(())
    } else {
        Err(JsErrorBox::generic(
            "Native frame module context unavailable",
        ))
    }
}

#[op2(reentrant)]
pub(super) fn op_frame_module<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window: v8::Local<v8::Object>,
    index: u32,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let context = window
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::generic("iframe context unavailable"))?;
    if context.get_slot::<FrameRetired>().is_some()
        || context
            .get_slot::<FrameSandbox>()
            .is_some_and(|flags| flags.scripts_blocked)
    {
        return Err(JsErrorBox::generic("frame module execution is blocked"));
    }
    let policy = context
        .get_slot::<FrameScriptPolicy>()
        .ok_or_else(|| JsErrorBox::generic("frame script policy unavailable"))?;
    if !policy
        .scripts
        .iter()
        .any(|script| script.module && script.index == index as usize)
    {
        return Err(JsErrorBox::generic(
            "frame module is not authorized by its document",
        ));
    }
    let scope = &mut v8::ContextScope::new(scope, context);
    let index = v8::Integer::new_from_unsigned(scope, index);
    call_frame_helper(scope, "executeFrameModule", &[index.into()])
        .ok_or_else(|| JsErrorBox::generic("frame module dispatch failed"))
}
