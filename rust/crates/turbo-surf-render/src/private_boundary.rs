//! Only native context provenance admits infrastructure values to the guest bridge.
use super::*;

pub(crate) struct Infrastructure;

#[op2(fast)]
pub(super) fn op_infrastructure_active(state: &OpState) -> bool {
    state.has::<Infrastructure>()
}

#[derive(Default)]
struct BoundaryOwners {
    next: u32,
    contexts: std::collections::HashMap<u32, v8::Weak<v8::Context>>,
}

fn token_key<'s>(scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Private> {
    let name = v8::String::new(scope, "turbo-surf.boundary-owner-token").unwrap();
    v8::Private::for_api(scope, Some(name))
}

fn capture_owner(scope: &mut v8::PinScope) -> Result<u32, String> {
    let context = scope.get_current_context();
    let weak = v8::Weak::new(scope, context);
    let state = JsRuntime::op_state_from(scope);
    let mut state = state.borrow_mut();
    if !state.has::<BoundaryOwners>() {
        state.put(BoundaryOwners::default());
    }
    let owners = state.borrow_mut::<BoundaryOwners>();
    owners
        .contexts
        .retain(|_, owner| owner.to_local(scope).is_some());
    owners.next = owners
        .next
        .checked_add(1)
        .ok_or("Private boundary owner limit exceeded")?;
    owners.contexts.insert(owners.next, weak);
    Ok(owners.next)
}

fn owned_key<'s>(scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Private> {
    let name = v8::String::new(scope, "turbo-surf.guest-owned-native-bytes").unwrap();
    v8::Private::for_api(scope, Some(name))
}

fn infrastructure(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> bool {
    let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
        return false;
    };
    let owned_by_infrastructure = object
        .get_creation_context(scope)
        .is_some_and(|context| context.get_slot::<Infrastructure>().is_some());
    if !owned_by_infrastructure {
        return false;
    }
    let key = owned_key(scope);
    !object
        .get_private(scope, key)
        .is_some_and(|owned| owned.is_true())
}

#[op2(fast, reentrant)]
pub(super) fn op_infrastructure_origin(
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
) -> bool {
    infrastructure(scope, value)
}

pub(crate) fn capabilities<'s>(
    scope: &mut v8::PinScope<'s, '_>,
) -> Result<v8::Local<'s, v8::Object>, String> {
    let id = capture_owner(scope)?;
    let token = v8::Object::new(scope);
    let key = token_key(scope);
    let id = v8::Integer::new_from_unsigned(scope, id);
    if token.set_private(scope, key, id.into()) != Some(true) {
        return Err("Private boundary token installation failed".into());
    }
    let result = v8::Object::new(scope);
    let null = v8::null(scope);
    result.set_prototype(scope, null.into());
    let rehome = v8::Function::builder(rehome_callback)
        .data(token.into())
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope)
        .ok_or("Private boundary capability allocation failed")?;
    let observe = v8::Function::builder(observe_callback)
        .data(token.into())
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope)
        .ok_or("Private boundary capability allocation failed")?;
    let invoke = v8::Function::builder(invoke_callback)
        .data(token.into())
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope)
        .ok_or("Private boundary capability allocation failed")?;
    let frame_invoke = v8::Function::builder(frame_invoke_callback)
        .data(token.into())
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope)
        .ok_or("Private frame capability allocation failed")?;
    let stream_invoke = v8::Function::builder(stream_invoke_callback)
        .data(token.into())
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope)
        .ok_or("Private stream capability allocation failed")?;
    let stream_observe = v8::Function::builder(stream_observe_callback)
        .data(token.into())
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope)
        .ok_or("Private stream capability allocation failed")?;
    let stream_rehome = v8::Function::builder(stream_rehome_callback)
        .data(token.into())
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope)
        .ok_or("Private stream capability allocation failed")?;
    for (name, function) in [
        ("rehome", rehome),
        ("observe", observe),
        ("invoke", invoke),
        ("invokeFrame", frame_invoke),
        ("streamInvoke", stream_invoke),
        ("streamObserve", stream_observe),
        ("streamRehome", stream_rehome),
    ] {
        let name = v8::String::new(scope, name).unwrap();
        if result.create_data_property(scope, name.into(), function.into()) != Some(true) {
            return Err("Private boundary capability installation failed".into());
        }
    }
    Ok(result)
}

fn owner_context<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    data: v8::Local<'s, v8::Value>,
) -> Result<v8::Local<'s, v8::Context>, JsErrorBox> {
    // Only trusted bootstrap installs the native token in Function data. Its
    // actor-owned entry is weak and cannot retain a retired document.
    let token = v8::Local::<v8::Object>::try_from(data).map_err(|_| unsupported())?;
    let key = token_key(scope);
    let id = token
        .get_private(scope, key)
        .filter(|value| value.is_uint32())
        .and_then(|value| value.uint32_value(scope))
        .ok_or_else(unsupported)?;
    let weak = JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<BoundaryOwners>()
        .contexts
        .get(&id)
        .cloned()
        .ok_or_else(unsupported)?;
    let context = weak.to_local(scope).ok_or_else(unsupported)?;
    Ok(context)
}

fn owner<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    data: v8::Local<'s, v8::Value>,
) -> Result<v8::Local<'s, v8::Context>, JsErrorBox> {
    let context = owner_context(scope, data)?;
    if context.get_slot::<Infrastructure>().is_some()
        || context.get_slot::<FrameRetired>().is_some()
        || context.get_slot::<CspHandle>().is_none()
    {
        return Err(JsErrorBox::type_error(
            "Private boundary document is closed or not a guest",
        ));
    }
    Ok(context)
}

fn stream_owner<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    data: v8::Local<'s, v8::Value>,
) -> Result<v8::Local<'s, v8::Context>, JsErrorBox> {
    let context = owner_context(scope, data)?;
    if context.get_slot::<Infrastructure>().is_some() || context.get_slot::<CspHandle>().is_none() {
        return Err(unsupported());
    }
    let document = context
        .get_slot::<ActiveDocument>()
        .and_then(|document| document.0.to_local(scope))
        .ok_or_else(unsupported)?;
    if document.get_creation_context(scope) != Some(context) {
        return Err(unsupported());
    }
    Ok(context)
}

fn stream_frontend(scope: &mut v8::PinScope, function: v8::Local<v8::Function>) -> bool {
    let Some(context) = function.get_creation_context(scope) else {
        return false;
    };
    if context.get_slot::<Infrastructure>().is_some() || context.get_slot::<CspHandle>().is_none() {
        return false;
    }
    let Some(document) = context
        .get_slot::<ActiveDocument>()
        .and_then(|document| document.0.to_local(scope))
    else {
        return false;
    };
    if document.get_creation_context(scope) != Some(context) {
        return false;
    }
    JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<BoundaryOwners>()
        .contexts
        .values()
        .any(|known| known.to_local(scope) == Some(context))
}

fn stream_rehome_callback(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let value = v8::Local::new(scope, args.get(0));
    let data = v8::Local::new(scope, args.data());
    let context = stream_owner(scope, data);
    match context.and_then(|context| {
        let scope = &mut v8::ContextScope::new(scope, context);
        rehome(scope, value)
    }) {
        Ok(value) => result.set(value),
        Err(error) => fail(scope, error),
    }
}

fn stream_invoke_callback(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let data = v8::Local::new(scope, args.data());
    let thunk_value = v8::Local::new(scope, args.get(0));
    let context = match stream_owner(scope, data) {
        Ok(context) => context,
        Err(error) => return fail(scope, error),
    };
    let Ok(thunk) = v8::Local::<v8::Function>::try_from(thunk_value) else {
        return fail(scope, unsupported());
    };
    // A borrowed frontend method can create its private service thunk in a
    // different guest realm. The receiver's bound token still owns the lease.
    if !stream_frontend(scope, thunk) {
        return fail(scope, unsupported());
    }
    let invoked = async_owner::with_retained_stream(scope, context, |scope| {
        let scope = &mut v8::ContextScope::new(scope, context);
        let receiver = v8::undefined(scope);
        thunk.call(scope, receiver.into(), &[])
    });
    match invoked {
        Ok(Some(value)) => result.set(value),
        Ok(None) => {}
        Err(error) => fail(scope, JsErrorBox::type_error(error)),
    }
}

fn stream_observe_callback(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    _: v8::ReturnValue,
) {
    let data = v8::Local::new(scope, args.data());
    let promise_value = v8::Local::new(scope, args.get(0));
    let fulfilled_value = v8::Local::new(scope, args.get(1));
    let rejected_value = v8::Local::new(scope, args.get(2));
    let observed = stream_owner(scope, data).and_then(|context| {
        let promise =
            v8::Local::<v8::Promise>::try_from(promise_value).map_err(|_| unsupported())?;
        if !infrastructure(scope, promise.into()) {
            return Err(unsupported());
        }
        let fulfilled =
            v8::Local::<v8::Function>::try_from(fulfilled_value).map_err(|_| unsupported())?;
        let rejected =
            v8::Local::<v8::Function>::try_from(rejected_value).map_err(|_| unsupported())?;
        if !stream_frontend(scope, fulfilled) || !stream_frontend(scope, rejected) {
            return Err(unsupported());
        }
        async_owner::with_retained_stream(scope, context, |scope| {
            let scope = &mut v8::ContextScope::new(scope, context);
            promise.then2(scope, fulfilled, rejected)
        })
        .map_err(JsErrorBox::type_error)?
        .ok_or_else(unsupported)?;
        Ok(())
    });
    if let Err(error) = observed {
        fail(scope, error);
    }
}

fn fail(scope: &mut v8::PinScope, error: JsErrorBox) {
    let message = v8::String::new(scope, &error.to_string()).unwrap();
    let error = v8::Exception::type_error(scope, message);
    scope.throw_exception(error);
}

fn rehome_callback(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let value = v8::Local::new(scope, args.get(0));
    let data = v8::Local::new(scope, args.data());
    let owned = owner(scope, data);
    match owned.and_then(|context| {
        let scope = &mut v8::ContextScope::new(scope, context);
        rehome(scope, value)
    }) {
        Ok(value) => result.set(value),
        Err(error) => fail(scope, error),
    }
}

fn rehome<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    if !infrastructure(scope, value) {
        return Ok(value);
    }
    if value.is_proxy() {
        return Err(unsupported());
    }
    if value.is_native_error() {
        return native_error(scope, value);
    }
    rehome_bytes(scope, value)?;
    Ok(value)
}

/// Reporting preserves guest values and copies only safe native error fields.
pub(super) fn report_value<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
) -> v8::Local<'s, v8::Value> {
    if !infrastructure(scope, value) {
        return value;
    }
    if !value.is_proxy() && value.is_native_error() {
        if let Ok(error) = native_error(scope, value) {
            return error;
        }
    }
    let message = v8::String::new(scope, "Unsupported infrastructure exception").unwrap();
    v8::Exception::error(scope, message)
}

// The trusted frontend closure retains original receiver/arguments/newTarget.
// Its capability's native owner is authenticated before any E algorithm runs.
fn invoke_callback(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let data = v8::Local::new(scope, args.data());
    let context = match owner(scope, data) {
        Ok(context) => context,
        Err(error) => return fail(scope, error),
    };
    let Ok(thunk) = v8::Local::<v8::Function>::try_from(args.get(0)) else {
        return fail(scope, unsupported());
    };
    let invoked = async_owner::with_document(scope, context, |scope| {
        let scope = &mut v8::ContextScope::new(scope, context);
        let receiver = v8::undefined(scope);
        thunk.call(scope, receiver.into(), &[])
    });
    match invoked {
        Ok(Some(value)) => result.set(value),
        // Preserve the actual pending exception, including an authored value.
        Ok(None) => {}
        Err(error) => fail(scope, JsErrorBox::type_error(error)),
    }
}

fn frame_owner<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    parent: v8::Local<'s, v8::Context>,
    window: v8::Local<'s, v8::Object>,
) -> Result<v8::Local<'s, v8::Context>, JsErrorBox> {
    let child = window.get_creation_context(scope).ok_or_else(unsupported)?;
    if !window.strict_equals(child.global(scope).into())
        || child.get_slot::<FrameContextMarker>().is_none()
        || child.get_slot::<FrameRetired>().is_some()
        || child.get_slot::<CspHandle>().is_none()
    {
        return Err(unsupported());
    }
    let actual_parent = child
        .get_slot::<FrameParent>()
        .and_then(|link| link.0.to_local(scope))
        .ok_or_else(unsupported)?;
    if actual_parent != parent {
        return Err(unsupported());
    }
    let parent_core = parent
        .get_slot::<RuntimeCore>()
        .and_then(|core| core.0.to_local(scope));
    let child_core = child
        .get_slot::<RuntimeCore>()
        .and_then(|core| core.0.to_local(scope));
    if !parent_core
        .zip(child_core)
        .is_some_and(|(a, b)| a.strict_equals(b.into()))
    {
        return Err(unsupported());
    }
    Ok(child)
}

fn frame_invoke_callback(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let data = v8::Local::new(scope, args.data());
    let window_value = v8::Local::new(scope, args.get(0));
    let thunk_value = v8::Local::new(scope, args.get(1));
    let context = owner(scope, data).and_then(|parent| {
        let window = v8::Local::<v8::Object>::try_from(window_value).map_err(|_| unsupported())?;
        frame_owner(scope, parent, window)
    });
    let context = match context {
        Ok(context) => context,
        Err(error) => return fail(scope, error),
    };
    let Ok(thunk) = v8::Local::<v8::Function>::try_from(thunk_value) else {
        return fail(scope, unsupported());
    };
    let invoked = async_owner::with_document(scope, context, |scope| {
        let scope = &mut v8::ContextScope::new(scope, context);
        let receiver = v8::undefined(scope);
        thunk.call(scope, receiver.into(), &[])
    });
    match invoked {
        Ok(Some(value)) => result.set(value),
        Ok(None) => {}
        Err(error) => fail(scope, JsErrorBox::type_error(error)),
    }
}

fn observe_callback(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    _: v8::ReturnValue,
) {
    let data = v8::Local::new(scope, args.data());
    let promise_value = v8::Local::new(scope, args.get(0));
    let fulfilled_value = v8::Local::new(scope, args.get(1));
    let rejected_value = v8::Local::new(scope, args.get(2));
    let owned = owner(scope, data);
    let observed = owned.and_then(|context| {
        let promise =
            v8::Local::<v8::Promise>::try_from(promise_value).map_err(|_| unsupported())?;
        if !infrastructure(scope, promise.into()) {
            return Err(unsupported());
        }
        let fulfilled =
            v8::Local::<v8::Function>::try_from(fulfilled_value).map_err(|_| unsupported())?;
        let rejected =
            v8::Local::<v8::Function>::try_from(rejected_value).map_err(|_| unsupported())?;
        async_owner::with_document(scope, context, |scope| {
            let scope = &mut v8::ContextScope::new(scope, context);
            promise.then2(scope, fulfilled, rejected)
        })
        .map_err(JsErrorBox::type_error)?
        .ok_or_else(unsupported)?;
        Ok(())
    });
    if let Err(error) = observed {
        fail(scope, error);
    }
}

#[op2(fast, reentrant)]
pub(super) fn op_infrastructure_adopt_crypto_key(
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Object>,
) -> Result<(), JsErrorBox> {
    if !deno_crypto::is_crypto_key(scope, value.into())
        && !crate::crypto_hybrid::is_hybrid_key(scope, value.into())
    {
        return Err(unsupported());
    }
    mark_owned(scope, value)
}

fn mark_owned(scope: &mut v8::PinScope, object: v8::Local<v8::Object>) -> Result<(), JsErrorBox> {
    let key = owned_key(scope);
    let yes = v8::Boolean::new(scope, true);
    if object.set_private(scope, key, yes.into()) != Some(true) {
        return Err(unsupported());
    }
    Ok(())
}

fn unsupported() -> JsErrorBox {
    JsErrorBox::type_error("Unsupported private infrastructure value")
}

fn plain_bytes(
    scope: &mut v8::PinScope,
    object: v8::Local<v8::Object>,
    typed_indices: bool,
) -> Result<(), JsErrorBox> {
    let args = v8::GetPropertyNamesArgs {
        mode: v8::KeyCollectionMode::OwnOnly,
        property_filter: v8::PropertyFilter::ALL_PROPERTIES,
        index_filter: if typed_indices {
            v8::IndexFilter::SkipIndices
        } else {
            v8::IndexFilter::IncludeIndices
        },
        key_conversion: v8::KeyConversionMode::KeepNumbers,
    };
    let keys = object
        .get_property_names(scope, args)
        .ok_or_else(unsupported)?;
    if keys.length() != 0 {
        return Err(unsupported());
    }
    Ok(())
}

fn install_prototype(
    scope: &mut v8::PinScope,
    object: v8::Local<v8::Object>,
    prototype: v8::Local<v8::Value>,
) -> Result<(), JsErrorBox> {
    if object.set_prototype(scope, prototype) != Some(true) {
        return Err(unsupported());
    }
    mark_owned(scope, object)
}

fn rehome_bytes(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> Result<(), JsErrorBox> {
    let object = v8::Local::<v8::Object>::try_from(value).map_err(|_| unsupported())?;
    plain_bytes(scope, object, value.is_typed_array())?;
    if value.is_shared_array_buffer() {
        return Err(unsupported());
    }
    if value.is_array_buffer() {
        let sample = v8::ArrayBuffer::new(scope, 0);
        let prototype = sample.get_prototype(scope).ok_or_else(unsupported)?;
        return install_prototype(scope, object, prototype);
    }
    let view = v8::Local::<v8::ArrayBufferView>::try_from(value).map_err(|_| unsupported())?;
    let buffer = view.buffer(scope).ok_or_else(unsupported)?;
    let buffer_value: v8::Local<v8::Value> = buffer.into();
    if buffer_value.is_shared_array_buffer() {
        return Err(unsupported());
    }
    if infrastructure(scope, buffer.into()) {
        rehome_bytes(scope, buffer.into())?;
    }
    view_prototype(scope, value, object)
}

fn view_prototype(
    scope: &mut v8::PinScope,
    value: v8::Local<v8::Value>,
    object: v8::Local<v8::Object>,
) -> Result<(), JsErrorBox> {
    let buffer = v8::ArrayBuffer::new(scope, 0);
    macro_rules! typed {
        ($kind:ident) => {
            if v8::Local::<v8::$kind>::try_from(value).is_ok() {
                let sample = v8::$kind::new(scope, buffer, 0, 0).ok_or_else(unsupported)?;
                let prototype = sample.get_prototype(scope).ok_or_else(unsupported)?;
                return install_prototype(scope, object, prototype);
            }
        };
    }
    typed!(Uint8Array);
    typed!(Uint8ClampedArray);
    typed!(Int8Array);
    typed!(Uint16Array);
    typed!(Int16Array);
    typed!(Uint32Array);
    typed!(Int32Array);
    typed!(Float16Array);
    typed!(Float32Array);
    typed!(Float64Array);
    typed!(BigUint64Array);
    typed!(BigInt64Array);
    if value.is_data_view() {
        let sample = v8::DataView::new(scope, buffer, 0, 0);
        let prototype = sample.get_prototype(scope).ok_or_else(unsupported)?;
        return install_prototype(scope, object, prototype);
    }
    Err(unsupported())
}

fn own_string<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
) -> Option<v8::Local<'s, v8::String>> {
    if object.is_proxy() {
        return None;
    }
    let name = v8::String::new(scope, name)?;
    let descriptor = object.get_own_property_descriptor(scope, name.into())?;
    let descriptor = v8::Local::<v8::Object>::try_from(descriptor).ok()?;
    let key = v8::String::new(scope, "value")?;
    if descriptor.has_own_property(scope, key.into()) != Some(true) {
        return None;
    }
    v8::Local::<v8::String>::try_from(descriptor.get(scope, key.into())?).ok()
}

fn native_error<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let object = v8::Local::<v8::Object>::try_from(value).map_err(|_| unsupported())?;
    let message = own_string(scope, object, "message").unwrap_or_else(|| v8::String::empty(scope));
    let name = own_string(scope, object, "name")
        .or_else(|| {
            let prototype = v8::Local::<v8::Object>::try_from(object.get_prototype(scope)?).ok()?;
            own_string(scope, prototype, "name")
        })
        .ok_or_else(unsupported)?;
    let error = match name.to_rust_string_lossy(scope).as_str() {
        "Error" => v8::Exception::error(scope, message),
        "RangeError" => v8::Exception::range_error(scope, message),
        "ReferenceError" => v8::Exception::reference_error(scope, message),
        "SyntaxError" => v8::Exception::syntax_error(scope, message),
        "TypeError" => v8::Exception::type_error(scope, message),
        _ => return Err(unsupported()),
    };
    Ok(error)
}

#[path = "private_boundary/frame_probe.rs"]
pub(crate) mod frame_probe;

#[cfg(test)]
#[path = "private_boundary/native_guard_tests.rs"]
mod native_guard_tests;
