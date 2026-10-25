//! HTMLSelectElement's indexed getter/setter (`select[i]`, `select[i] = option|null`).
//!
//! Every node wrapper shares one object template, so the interceptor lives there and first
//! checks natively that the holder is an HTML `<select>`; other nodes fall straight through to
//! ordinary lookup. Select semantics are dispatched to the private JS helper captured from
//! select_element.js. No query callback is installed: like Blink, V8 then reports intercepted
//! indices as non-enumerable to `propertyIsEnumerable` while descriptors stay enumerable.
use super::*;

struct Indexed(v8::Weak<v8::Function>);

fn is_select(scope: &mut v8::PinScope, holder: v8::Local<v8::Object>) -> bool {
    let _dom = DomScope::for_object(scope, holder);
    let Some(handle) = handle_of(scope, holder) else {
        return false;
    };
    with_tree(|tree| {
        tree.node_type_id(handle) == 1
            && tree.namespace_id(handle) == 0
            && tree.local_name(handle) == Some("select")
    })
    .unwrap_or(false)
}

enum Outcome<'s> {
    /// Not a select (or the helper declined): ordinary lookup continues.
    Pass,
    /// The helper threw; the interceptor claims the access so V8 rethrows.
    Threw,
    Value(v8::Local<'s, v8::Value>),
}

fn dispatch<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: &v8::PropertyCallbackArguments<'s>,
    operation: &str,
    index: u32,
    value: Option<v8::Local<'s, v8::Value>>,
) -> Outcome<'s> {
    let holder = args.holder();
    if !is_select(scope, holder) {
        return Outcome::Pass;
    }
    let Some(callback) = holder
        .get_creation_context(scope)
        .and_then(|context| context.get_slot::<Indexed>())
        .and_then(|slot| slot.0.to_local(scope))
    else {
        return Outcome::Pass;
    };
    let operation = v8::String::new(scope, operation).unwrap();
    let index = v8::Integer::new_from_unsigned(scope, index);
    let value = value.unwrap_or_else(|| v8::undefined(scope).into());
    let receiver = v8::undefined(scope);
    match callback.call(
        scope,
        receiver.into(),
        &[operation.into(), holder.into(), index.into(), value],
    ) {
        None => Outcome::Threw,
        Some(result) if result.is_undefined() => Outcome::Pass,
        Some(result) => Outcome::Value(result),
    }
}

fn getter<'s, 'i>(
    scope: &mut v8::PinScope<'s, 'i>,
    index: u32,
    args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue,
) -> v8::Intercepted {
    claim(dispatch(scope, &args, "get", index, None), |value| {
        rv.set(value)
    })
}

fn claim(outcome: Outcome, write: impl FnOnce(v8::Local<v8::Value>)) -> v8::Intercepted {
    match outcome {
        Outcome::Pass => v8::Intercepted::kNo,
        Outcome::Threw => v8::Intercepted::kYes,
        Outcome::Value(value) => {
            write(value);
            v8::Intercepted::kYes
        }
    }
}

fn descriptor<'s, 'i>(
    scope: &mut v8::PinScope<'s, 'i>,
    index: u32,
    args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue,
) -> v8::Intercepted {
    claim(dispatch(scope, &args, "descriptor", index, None), |value| {
        rv.set(value)
    })
}

fn setter<'s, 'i, T: 'static>(
    scope: &mut v8::PinScope<'s, 'i>,
    index: u32,
    value: v8::Local<'s, v8::Value>,
    args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue<T>,
) -> v8::Intercepted
where
    for<'cb> v8::ReturnValue<'cb, T>: NamedMutationResult,
{
    claim(
        dispatch(scope, &args, "set", index, Some(value)),
        |result| rv.set_success(result.is_true()),
    )
}

fn deleter<'s, 'i>(
    scope: &mut v8::PinScope<'s, 'i>,
    index: u32,
    args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
    claim(dispatch(scope, &args, "delete", index, None), |result| {
        rv.set_bool(result.is_true())
    })
}

fn definer<'s, 'i, T: 'static>(
    scope: &mut v8::PinScope<'s, 'i>,
    index: u32,
    descriptor: &v8::PropertyDescriptor,
    args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue<T>,
) -> v8::Intercepted
where
    for<'cb> v8::ReturnValue<'cb, T>: NamedMutationResult,
{
    if !is_select(scope, args.holder()) {
        return v8::Intercepted::kNo;
    }
    // Accessor descriptors are rejected. Chrome 156 throws from Object.defineProperty yet reports
    // true from Reflect.defineProperty (Blink keys that on ShouldThrowOnError); this V8 passes
    // false for both, so both report failure here: Object.defineProperty still throws TypeError
    // and Reflect.defineProperty returns false (the one asserted divergence in the tests).
    if descriptor.has_get() || descriptor.has_set() {
        if args.should_throw_on_error() {
            let message = v8::String::new(scope, "Accessor properties are not allowed.").unwrap();
            let error = v8::Exception::type_error(scope, message);
            scope.throw_exception(error);
        } else {
            rv.set_success(false);
        }
        return v8::Intercepted::kYes;
    }
    // A data descriptor runs the indexed setter with its value.
    let value = if descriptor.has_value() {
        v8::Local::new(scope, descriptor.value())
    } else {
        v8::undefined(scope).into()
    };
    claim(
        dispatch(scope, &args, "set", index, Some(value)),
        |result| rv.set_success(result.is_true()),
    )
}

fn enumerator<'s, 'i>(
    scope: &mut v8::PinScope<'s, 'i>,
    args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue<v8::Array>,
) {
    if let Outcome::Value(value) = dispatch(scope, &args, "indices", 0, None) {
        if let Ok(indices) = v8::Local::<v8::Array>::try_from(value) {
            rv.set(indices);
        }
    }
}

/// Install the select indexed interceptor on the shared node wrapper template.
pub(super) fn indexed_handler(tmpl: v8::Local<v8::ObjectTemplate>) {
    tmpl.set_indexed_property_handler(
        v8::IndexedPropertyHandlerConfiguration::new()
            .getter(getter)
            .setter(setter)
            .descriptor(descriptor)
            .deleter(deleter)
            .enumerator(enumerator)
            .definer(definer),
    );
}

pub(super) fn install(scope: &mut v8::PinScope) {
    run_js(scope, include_str!("select_element.js"));
    let global = scope.get_current_context().global(scope);
    let key = v8::String::new(scope, "__captureSelectIndexed").unwrap();
    if let Some(callback) = global
        .get(scope, key.into())
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    {
        if let Some(callback) = dom_ownership::helper_function(scope, callback) {
            scope
                .get_current_context()
                .set_slot(std::rc::Rc::new(Indexed(callback)));
        }
    }
    global.delete(scope, key.into());
}

#[cfg(test)]
#[path = "select_element_tests.rs"]
mod tests;
