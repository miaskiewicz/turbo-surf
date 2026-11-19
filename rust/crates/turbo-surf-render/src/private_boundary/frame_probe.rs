//! Host-only security probe; no native capability is returned to guest code.
use super::*;

fn admitted_callback(
    _scope: &mut v8::PinScope,
    _args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    result.set_bool(true);
}

pub(crate) fn admitted<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    parent: v8::Local<'s, v8::Context>,
    window: v8::Local<'s, v8::Value>,
) -> bool {
    let scope = &mut v8::ContextScope::new(scope, parent);
    v8::tc_scope!(let scope, scope);
    let Ok(capabilities) = capabilities(scope) else {
        return false;
    };
    let key = v8::String::new(scope, "invokeFrame").unwrap();
    let invoke = capabilities.get(scope, key.into()).unwrap();
    let invoke = v8::Local::<v8::Function>::try_from(invoke).unwrap();
    let thunk = v8::Function::builder(admitted_callback)
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope)
        .unwrap();
    let receiver = v8::undefined(scope);
    invoke
        .call(scope, receiver.into(), &[window, thunk.into()])
        .is_some_and(|value| value.is_true())
}

pub(crate) fn own_value<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
) -> Option<v8::Local<'s, v8::Value>> {
    if object.is_proxy() {
        return None;
    }
    let key = v8::String::new(scope, name)?;
    let descriptor = object.get_own_property_descriptor(scope, key.into())?;
    let descriptor = v8::Local::<v8::Object>::try_from(descriptor).ok()?;
    let value = v8::String::new(scope, "value")?;
    if descriptor.has_own_property(scope, value.into()) != Some(true) {
        return None;
    }
    descriptor.get(scope, value.into())
}
