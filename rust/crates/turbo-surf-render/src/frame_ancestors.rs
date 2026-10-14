//! Location.ancestorOrigins is a genuine native DOMStringList.
use super::*;

pub(super) fn origins<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    mut context: v8::Local<'s, v8::Context>,
) -> Vec<String> {
    let mut origins = Vec::new();
    while let Some(parent) = context.get_slot::<FrameParent>() {
        let Some(parent) = parent.0.to_local(scope) else {
            break;
        };
        context = parent;
        origins.push(window_origin(scope, context));
    }
    origins
}

fn state(scope: &mut v8::PinScope, receiver: v8::Local<v8::Object>) -> Option<Rc<State>> {
    let Some(state) = receiver
        .get_creation_context(scope)
        .and_then(|context| context.get_slot::<State>())
    else {
        type_error(scope, "Illegal invocation");
        return None;
    };
    if !state
        .ancestors
        .to_local(scope)
        .is_some_and(|ancestors| ancestors.strict_equals(receiver.into()))
    {
        type_error(scope, "Illegal invocation");
        return None;
    }
    Some(state)
}

pub(super) fn get(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let Some(state) = object_state(scope, args.this()) else {
        type_error(scope, "Illegal invocation");
        return;
    };
    if readable(scope, &state) {
        if let Some(ancestors) = state.ancestors.to_local(scope) {
            result.set(ancestors.into());
        }
    }
}

fn length(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    if let Some(state) = state(scope, args.this()) {
        result.set_uint32(state.ancestor_origins.len() as u32);
    }
}

fn item(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let Some(state) = state(scope, args.this()) else {
        return;
    };
    if args.length() == 0 {
        type_error(scope, "An index is required");
        return;
    }
    let Some(index) = args.get(0).uint32_value(scope) else {
        return;
    };
    if let Some(origin) = state.ancestor_origins.get(index as usize) {
        result.set(v8::String::new(scope, origin).unwrap().into());
    } else {
        result.set(v8::null(scope).into());
    }
}

fn contains(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let Some(state) = state(scope, args.this()) else {
        return;
    };
    if args.length() == 0 {
        type_error(scope, "A string is required");
        return;
    }
    let Some(origin) = args.get(0).to_string(scope) else {
        return;
    };
    let origin = origin.to_rust_string_lossy(scope);
    result.set_bool(state.ancestor_origins.contains(&origin));
}

fn index<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    index: u32,
    args: v8::PropertyCallbackArguments<'s>,
    mut result: v8::ReturnValue<v8::Value>,
) -> v8::Intercepted {
    let Some(state) = state(scope, args.holder()) else {
        return v8::Intercepted::kNo;
    };
    let Some(origin) = state.ancestor_origins.get(index as usize) else {
        return v8::Intercepted::kNo;
    };
    result.set(v8::String::new(scope, origin).unwrap().into());
    v8::Intercepted::kYes
}
fn query<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    index: u32,
    args: v8::PropertyCallbackArguments<'s>,
    mut result: v8::ReturnValue<v8::Integer>,
) -> v8::Intercepted {
    let Some(state) = state(scope, args.holder()) else {
        return v8::Intercepted::kNo;
    };
    if index as usize >= state.ancestor_origins.len() {
        return v8::Intercepted::kNo;
    }
    result.set_int32(v8::PropertyAttribute::READ_ONLY.as_u32() as i32);
    v8::Intercepted::kYes
}
fn keys<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::PropertyCallbackArguments<'s>,
    mut result: v8::ReturnValue<v8::Array>,
) {
    let Some(state) = state(scope, args.holder()) else {
        return;
    };
    let keys = state
        .ancestor_origins
        .iter()
        .enumerate()
        .map(|(index, _)| v8::Integer::new_from_unsigned(scope, index as u32).into())
        .collect::<Vec<_>>();
    result.set(v8::Array::new_with_elements(scope, &keys));
}
fn illegal(
    scope: &mut v8::PinScope,
    _args: v8::FunctionCallbackArguments,
    _result: v8::ReturnValue,
) {
    type_error(scope, "Illegal constructor");
}

pub(super) fn install<'s>(scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Object> {
    let constructor = v8::FunctionTemplate::new(scope, illegal);
    let name = v8::String::new(scope, "DOMStringList").unwrap();
    constructor.set_class_name(name);
    constructor.read_only_prototype();
    let template = constructor.instance_template(scope);
    template.set_indexed_property_handler(
        v8::IndexedPropertyHandlerConfiguration::new()
            .getter(index)
            .query(query)
            .enumerator(keys),
    );
    let object = template.new_instance(scope).unwrap();
    let function = constructor.get_function(scope).unwrap();
    let prototype_key = v8::String::new(scope, "prototype").unwrap();
    let prototype = function
        .get(scope, prototype_key.into())
        .unwrap()
        .to_object(scope)
        .unwrap();
    for (name, callback, arity) in [
        ("item", v8::MapFnTo::map_fn_to(item), 1),
        ("contains", v8::MapFnTo::map_fn_to(contains), 1),
    ] {
        let function = v8::Function::builder_raw(callback)
            .length(arity)
            .constructor_behavior(v8::ConstructorBehavior::Throw)
            .build(scope)
            .unwrap();
        let name = v8::String::new(scope, name).unwrap();
        function.set_name(name);
        prototype.define_own_property(
            scope,
            name.into(),
            function.into(),
            v8::PropertyAttribute::NONE,
        );
    }
    let getter = v8::Function::builder(length)
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope)
        .unwrap();
    let name = v8::String::new(scope, "get length").unwrap();
    getter.set_name(name);
    let undefined = v8::undefined(scope);
    let mut descriptor = v8::PropertyDescriptor::new_from_get_set(getter.into(), undefined.into());
    descriptor.set_enumerable(true);
    descriptor.set_configurable(true);
    let name = v8::String::new(scope, "length").unwrap();
    prototype.define_property(scope, name.into(), &descriptor);
    let tag = v8::Symbol::get_to_string_tag(scope);
    let value = v8::String::new(scope, "DOMStringList").unwrap();
    prototype.define_own_property(
        scope,
        tag.into(),
        value.into(),
        v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_ENUM,
    );
    let context = scope.get_current_context();
    let window = context.global(scope);
    let name = v8::String::new(scope, "DOMStringList").unwrap();
    window.set(scope, name.into(), function.into());
    object
}
