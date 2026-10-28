//! Real V8 microtasks retain their native realm and report errors to its Window.
use super::*;

fn type_error(scope: &mut v8::PinScope, text: &str) {
    if let Some(text) = v8::String::new(scope, text) {
        let error = v8::Exception::type_error(scope, text);
        scope.throw_exception(error);
    }
}
fn deliver(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    if scope.is_execution_terminating()
        || scope
            .get_current_context()
            .get_slot::<FrameRetired>()
            .is_some()
    {
        return;
    }
    let Ok(callback) = v8::Local::<v8::Function>::try_from(args.data()) else {
        return;
    };
    let Some(context) = callback.get_creation_context(scope) else {
        return;
    };
    let scope = &mut v8::ContextScope::new(scope, context);
    let failure = {
        v8::tc_scope!(let catch, scope);
        let receiver = v8::undefined(catch);
        if callback.call(catch, receiver.into(), &[]).is_some() || catch.is_execution_terminating()
        {
            return;
        }
        let Some(error) = catch.exception() else {
            return;
        };
        let message = catch.message();
        let details = serde_json::json!({
            "message": message.map(|message| message.get(catch).to_rust_string_lossy(catch)).unwrap_or_else(|| "Uncaught exception".into()),
            "filename": message.and_then(|message| message.get_script_resource_name(catch)).map(|name| name.to_rust_string_lossy(catch)).unwrap_or_default(),
            "lineno": message.and_then(|message| message.get_line_number(catch)).unwrap_or(0),
            "colno": message.map(|message| message.get_start_column()+1).unwrap_or(0)
        });
        (v8::Global::new(catch, error), details)
    };
    let error = v8::Local::new(scope, failure.0);
    let Some(json) = v8::String::new(scope, &failure.1.to_string()) else {
        return;
    };
    let Some(details) = v8::json::parse(scope, json) else {
        return;
    };
    v8::tc_scope!(let catch, scope);
    catch.set_verbose(true);
    call_frame_helper(catch, "reportClassicScriptError", &[error, details]);
}
fn enqueue(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    let window = args.this();
    let Some(context) = window.get_creation_context(scope) else {
        type_error(scope, "Illegal invocation");
        return;
    };
    if !window.strict_equals(context.global(scope).into()) {
        type_error(scope, "Illegal invocation");
        return;
    }
    let Ok(callback) = v8::Local::<v8::Function>::try_from(args.get(0)) else {
        type_error(
            scope,
            "Failed to execute 'queueMicrotask' on 'Window': parameter 1 is not of type 'Function'.",
        );
        return;
    };
    if scope.is_execution_terminating() {
        return;
    }
    // A detached window still queues (Chrome 156); the job belongs to the
    // callback's realm, so `deliver` drops it once that realm is retired.
    let owner = callback.get_creation_context(scope).unwrap_or(context);
    let scope = &mut v8::ContextScope::new(scope, owner);
    if let Some(job) = v8::Function::builder(deliver)
        .data(callback.into())
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope)
    {
        scope.enqueue_microtask(job);
    }
}
/// Installs the brand-checked `queueMicrotask` on `window`'s own realm. Ops run in
/// the context that created them (the main realm), so a frame's bootstrap passes
/// its own global and the function is created there (turbo-surf#73): a frame's
/// `queueMicrotask` then rejects a foreign `this` like the main window's.
#[op2(fast)]
pub(super) fn op_install_microtask_queue(
    scope: &mut v8::PinScope,
    window: v8::Local<v8::Object>,
) -> Result<(), JsErrorBox> {
    let context = window
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::generic("microtask realm unavailable"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    let global = context.global(scope);
    if !window.strict_equals(global.into()) {
        return Err(JsErrorBox::type_error("Illegal invocation"));
    }
    let name = v8::String::new(scope, "queueMicrotask")
        .ok_or_else(|| JsErrorBox::generic("microtask name allocation failed"))?;
    let function = v8::Function::builder(enqueue)
        .length(1)
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope)
        .ok_or_else(|| JsErrorBox::generic("microtask function allocation failed"))?;
    function.set_name(name);
    if global.create_data_property(scope, name.into(), function.into()) != Some(true) {
        return Err(JsErrorBox::generic("microtask installation failed"));
    }
    Ok(())
}
