//! Module execution for an authenticated guest Page, never the infrastructure realm.
use super::*;

pub(super) async fn load_and_evaluate(
    rt: &mut JsRuntime,
    context: &v8::Global<v8::Context>,
    specifier: &ModuleSpecifier,
    code: String,
) -> Result<(), String> {
    require_active(rt, context)?;
    if code.is_empty() {
        rt.load_side_es_module_in_context(context, specifier).await
    } else {
        rt.load_side_es_module_from_code_in_context(context, specifier, code)
            .await
    }
    .map_err(|error| error.to_string())?;
    require_active(rt, context)?;
    let result = match begin_evaluation(rt, context, specifier) {
        Ok(promise) => drive_evaluation(rt, context, promise).await,
        Err(error) => Err(error),
    };
    let cleanup = if require_active(rt, context).is_ok() {
        set_current_script(rt, context, false)
    } else {
        Ok(())
    };
    result.and(cleanup)
}

fn require_active(rt: &mut JsRuntime, context: &v8::Global<v8::Context>) -> Result<(), String> {
    deno_core::scope!(scope, rt);
    let context = v8::Local::new(scope, context);
    if context.get_slot::<NativePageModuleContext>().is_none()
        || context.get_slot::<FrameRetired>().is_some()
        || context.get_slot::<FrameParent>().is_some()
    {
        return Err("Active native Page module context unavailable".into());
    }
    Ok(())
}

fn set_current_script(
    rt: &mut JsRuntime,
    context: &v8::Global<v8::Context>,
    active: bool,
) -> Result<(), String> {
    deno_core::scope!(scope, rt);
    let context = v8::Local::new(scope, context);
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope, scope);
    let value = if active {
        let namespace = frame_namespace(scope).ok_or("Module namespace unavailable")?;
        let key = v8::String::new(scope, "currentModuleEl").unwrap();
        namespace
            .get(scope, key.into())
            .ok_or("Module element unavailable")?
    } else {
        v8::null(scope).into()
    };
    call_frame_helper(scope, "setCurrentScript", &[value])
        .map(|_| ())
        .ok_or_else(|| "Module currentScript installation failed".into())
}

fn begin_evaluation(
    rt: &mut JsRuntime,
    context: &v8::Global<v8::Context>,
    specifier: &ModuleSpecifier,
) -> Result<v8::Global<v8::Value>, String> {
    set_current_script(rt, context, true)?;
    let specifier =
        deno_core::serde_json::to_string(specifier.as_str()).map_err(|error| error.to_string())?;
    deno_core::scope!(scope, rt);
    let context = v8::Local::new(scope, context);
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope, scope);
    crate::browser_env::run_engine_script(scope, &format!("return import({specifier});"))
        .map(|value| v8::Global::new(scope, value))
        .ok_or_else(|| {
            scope
                .exception()
                .map(|error| error.to_rust_string_lossy(scope))
                .unwrap_or_else(|| "Native Page module evaluation failed".into())
        })
}

async fn drive_evaluation(
    rt: &mut JsRuntime,
    context: &v8::Global<v8::Context>,
    promise: v8::Global<v8::Value>,
) -> Result<(), String> {
    let completion = {
        deno_core::scope!(scope, rt);
        let context = v8::Local::new(scope, context);
        let scope = &mut v8::ContextScope::new(scope, context);
        JsRuntime::scoped_resolve(scope, promise.clone())
    };
    drain_event_loop_until_completion(rt, None, Some(&promise)).await?;
    if crate::shim_bindings::pending(rt) {
        return Ok(());
    }
    rt.with_event_loop_promise(completion, Default::default())
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}
