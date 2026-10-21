//! Stream transport callbacks are owned by their realm, never a host strong root.
use deno_core::{op2, v8};
use deno_error::JsErrorBox;
struct Adapter(v8::Weak<v8::Function>);
#[op2(fast)]
pub(crate) fn op_stream_transfer_install(
    scope: &mut v8::PinScope,
    callback: v8::Local<v8::Function>,
) {
    if let Some(context) = callback.get_creation_context(scope) {
        if let Some(callback) = crate::context_ownership::function(scope, callback) {
            context.set_slot(std::rc::Rc::new(Adapter(callback)));
        }
    }
}
pub(crate) fn call<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    mode: i32,
    values: &[v8::Local<'s, v8::Value>],
) -> Option<v8::Local<'s, v8::Value>> {
    let adapter = scope.get_current_context().get_slot::<Adapter>()?;
    let function = crate::context_ownership::local(scope, &adapter.0);
    let mode = v8::Integer::new(scope, mode);
    let mut args = vec![mode.into()];
    args.extend_from_slice(values);
    let receiver = v8::undefined(scope);
    let scope = std::pin::pin!(v8::AllowJavascriptExecutionScope::new(scope));
    function.call(&scope.init(), receiver.into(), &args)
}
pub(crate) fn kind<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
) -> Result<i32, JsErrorBox> {
    let value = call(scope, 0, &[object.into()])
        .ok_or_else(|| JsErrorBox::generic("Stream validation interrupted"))?;
    value
        .int32_value(scope)
        .ok_or_else(|| JsErrorBox::generic("Stream validation failed"))
}
