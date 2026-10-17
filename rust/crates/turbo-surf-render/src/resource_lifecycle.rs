//! Trusted resource consumers use native admission and direct classic compilation.
use super::*;

fn owner_context<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<v8::Object>,
) -> Result<v8::Local<'s, v8::Context>, JsErrorBox> {
    let context = document
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::generic("script document context unavailable"))?;
    if context.get_slot::<FrameRetired>().is_some() {
        return Err(JsErrorBox::generic("iframe document has been replaced"));
    }
    if context
        .get_slot::<FrameSandbox>()
        .is_some_and(|flags| flags.scripts_blocked)
    {
        return Err(JsErrorBox::generic(
            "script execution is blocked by the iframe sandbox",
        ));
    }
    let caller = scope.get_entered_or_microtask_context();
    if !caller
        .get_security_token(scope)
        .strict_equals(context.get_security_token(scope))
    {
        return Err(JsErrorBox::generic("script document is not same-origin"));
    }
    Ok(context)
}

#[op2(nofast, reentrant)]
pub(super) fn op_install_resource_consumer(
    scope: &mut v8::PinScope,
    document: v8::Local<v8::Object>,
    callback: v8::Local<v8::Function>,
    state: v8::Local<v8::Object>,
) -> bool {
    if state.is_proxy() || state.get_creation_context(scope) != document.get_creation_context(scope)
    {
        return false;
    }
    let Some(key) = state_key(scope) else {
        return false;
    };
    if document.set_private(scope, key, state.into()) != Some(true) {
        return false;
    }
    crate::browser_env::install_mutation_consumer(scope, document, callback)
}

fn state_key<'s>(scope: &mut v8::PinScope<'s, '_>) -> Option<v8::Local<'s, v8::Private>> {
    let name = v8::String::new(scope, "turbo.resources.document-state")?;
    Some(v8::Private::for_api(scope, Some(name)))
}

pub(super) fn set_current_script<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<v8::Object>,
    value: v8::Local<'s, v8::Value>,
) -> Option<v8::Local<'s, v8::Value>> {
    let key = state_key(scope)?;
    let state = document.get_private(scope, key)?.to_object(scope)?;
    let name = v8::String::new(scope, "currentScript")?;
    let previous = state.get(scope, name.into())?;
    state.create_data_property(scope, name.into(), value)?;
    Some(previous)
}

pub(super) fn restore_current_script<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<v8::Object>,
    value: v8::Local<'s, v8::Value>,
) {
    let interrupted = scope.is_execution_terminating();
    if interrupted {
        scope.cancel_terminate_execution();
    }
    // This native plain-object field never invokes guest JS. Restore it even
    // when termination prevented a JavaScript finally block from running.
    set_current_script(scope, document, value);
    if interrupted {
        scope.terminate_execution();
    }
}

#[op2(nofast, reentrant)]
pub(super) fn op_retire_resource_document(
    scope: &mut v8::PinScope,
    document: v8::Local<v8::Object>,
) {
    crate::browser_env::set_document_resource_active(scope, document, false);
    crate::browser_env::remove_mutation_consumer(scope, document);
}

#[op2(reentrant)]
pub(super) fn op_prepare_resource_script<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<v8::Object>,
    element: v8::Local<v8::Object>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let Some(context) = document.get_creation_context(scope) else {
        return Ok(v8::null(scope).into());
    };
    if context.get_slot::<FrameRetired>().is_some()
        || context
            .get_slot::<FrameSandbox>()
            .is_some_and(|flags| flags.scripts_blocked)
    {
        return Ok(v8::null(scope).into());
    }
    owner_context(scope, document)?;
    let Some(owner) = crate::browser_env::script_document(scope, element) else {
        return Ok(v8::null(scope).into());
    };
    if owner != document {
        return Ok(v8::null(scope).into());
    }
    let Some(script) = crate::browser_env::prepare_script(scope, element) else {
        return Ok(v8::null(scope).into());
    };
    // Parse only scalar metadata natively; source remains an original V8 string.
    // Neither JSON parsing nor data-property creation invokes guest setters.
    let metadata = serde_json::json!({"src":script.src,"kind":script.kind,
        "asynchronous":script.asynchronous,"parserInserted":script.parser_inserted,
        "attributes":script.attributes,"inShadowTree":script.in_shadow_tree})
    .to_string();
    let json = v8::String::new(scope, &metadata)
        .ok_or_else(|| JsErrorBox::generic("script metadata allocation failed"))?;
    let prepared = v8::json::parse(scope, json)
        .and_then(|value| value.to_object(scope))
        .ok_or_else(|| JsErrorBox::generic("script metadata unavailable"))?;
    let source =
        v8::String::new_from_two_byte(scope, &script.source_units, v8::NewStringType::Normal)
            .ok_or_else(|| JsErrorBox::generic("script source allocation failed"))?;
    let name = v8::String::new(scope, "source")
        .ok_or_else(|| JsErrorBox::generic("script property allocation failed"))?;
    if prepared.create_data_property(scope, name.into(), source.into()) != Some(true) {
        return Err(JsErrorBox::generic("script source installation failed"));
    }
    Ok(prepared.into())
}

#[op2(nofast, reentrant)]
pub(super) fn op_execute_inline_script<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    element: v8::Local<'s, v8::Object>,
    code: v8::Local<'s, v8::String>,
    #[string] source_url: &str,
    in_shadow_tree: bool,
) -> Result<bool, JsErrorBox> {
    let context = owner_context(scope, document)?;
    let scope = &mut v8::ContextScope::new(scope, context);
    let catcher = std::pin::pin!(v8::TryCatch::new(scope));
    let scope = &mut catcher.init();
    let resource = v8::String::new(scope, source_url)
        .ok_or_else(|| JsErrorBox::generic("script URL allocation failed"))?;
    let origin = v8::ScriptOrigin::new(
        scope,
        resource.into(),
        0,
        0,
        false,
        0,
        None,
        false,
        false,
        false,
        None,
    );
    let current = if in_shadow_tree {
        v8::null(scope).into()
    } else {
        element.into()
    };
    let previous = set_current_script(scope, document, current)
        .ok_or_else(|| JsErrorBox::generic("script state unavailable"))?;
    let result = (|| -> Result<bool, JsErrorBox> {
        if v8::Script::compile(scope, code, Some(&origin))
            .and_then(|script| script.run(scope))
            .is_some()
        {
            return Ok(true);
        }
        if scope.is_execution_terminating() {
            return Ok(false);
        }
        let Some(error) = scope.exception() else {
            return Ok(false);
        };
        let message = scope.message();
        let metadata = serde_json::json!({"message":message.map(|message| message.get(scope).to_rust_string_lossy(scope)).unwrap_or_else(|| "Uncaught exception".into()),
        "filename":source_url,"lineno":message.and_then(|message|message.get_line_number(scope)).unwrap_or(0),
        "colno":message.map(|message|message.get_start_column()+1).unwrap_or(0)});
        scope.reset();
        let json = v8::String::new(scope, &metadata.to_string())
            .ok_or_else(|| JsErrorBox::generic("script error metadata allocation failed"))?;
        let details = v8::json::parse(scope, json)
            .ok_or_else(|| JsErrorBox::generic("invalid script error metadata"))?;
        call_frame_helper(scope, "reportClassicScriptError", &[error, details]);
        Ok(false)
    })();
    restore_current_script(scope, document, previous);
    result
}
