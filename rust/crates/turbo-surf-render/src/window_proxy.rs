//! Genuine V8 WindowProxy support, compiled only with the verified native SDK.
use super::*;
#[path = "frame_location.rs"]
mod location;

struct Proxy(v8::Global<v8::Object>);
struct Document(v8::Global<v8::Value>);
struct Origin(v8::Global<v8::Value>);
struct ExceptionConstructor(v8::Global<v8::Function>);
type CachedFunctions = std::collections::HashMap<String, v8::Global<v8::Function>>;
#[derive(Default)]
struct Functions(RefCell<Vec<(v8::Global<v8::Object>, CachedFunctions)>>);

fn access_allowed<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    accessing: v8::Local<'s, v8::Context>,
    accessed: v8::Local<'s, v8::Object>,
    _data: v8::Local<'s, v8::Value>,
) -> bool {
    accessed.get_creation_context(scope).is_some_and(|owner| {
        owner
            .get_security_token(scope)
            .strict_equals(accessing.get_security_token(scope))
    })
}

pub(super) fn capture_exception(scope: &mut v8::PinScope) {
    let context = scope.get_current_context();
    let global = context.global(scope);
    let key = v8::String::new(scope, "DOMException").unwrap();
    if let Some(function) = global
        .get(scope, key.into())
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    {
        context.set_slot(Rc::new(ExceptionConstructor(v8::Global::new(
            scope, function,
        ))));
    }
}

fn named_exception(scope: &mut v8::PinScope, message: &str, name: &str) {
    let context = scope.get_current_context();
    let message = v8::String::new(scope, message).unwrap();
    let name = v8::String::new(scope, name).unwrap();
    // A retired realm's global proxy may now belong to another origin. Reading
    // its DOMException property here would recursively fail the same access check.
    if let Some(constructor) = context.get_slot::<ExceptionConstructor>() {
        let constructor = v8::Local::new(scope, &constructor.0);
        if let Some(error) = constructor.new_instance(scope, &[message.into(), name.into()]) {
            scope.throw_exception(error.into());
            return;
        }
        return;
    }
    let error = v8::Exception::error(scope, message);
    let object = v8::Local::<v8::Object>::try_from(error).unwrap();
    let key = v8::String::new(scope, "name").unwrap();
    object.set(scope, key.into(), name.into());
    scope.throw_exception(error);
}

fn deny(scope: &mut v8::PinScope) {
    named_exception(scope, "Blocked cross-origin window access", "SecurityError");
}

pub(super) fn failed_access(
    scope: &mut v8::PinScope,
    _target: v8::Local<v8::Object>,
    _kind: v8::AccessType,
    _data: v8::Local<v8::Value>,
) {
    deny(scope);
}

fn document_get(
    scope: &mut v8::PinScope,
    _args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let context = scope.get_current_context();
    let Some(proxy) = context.get_slot::<Proxy>() else {
        return;
    };
    let proxy = v8::Local::new(scope, &proxy.0);
    let Some(active) = proxy.get_creation_context(scope) else {
        return;
    };
    if !context
        .get_security_token(scope)
        .strict_equals(active.get_security_token(scope))
    {
        deny(scope);
        return;
    }
    if let Some(document) = active.get_slot::<Document>() {
        result.set(v8::Local::new(scope, &document.0));
    }
}

pub(super) fn install_document(scope: &mut v8::PinScope) {
    let context = scope.get_current_context();
    let global = context.global(scope);
    let key = v8::String::new(scope, "document").unwrap();
    let document = global.get(scope, key.into()).unwrap();
    context.set_slot(Rc::new(Document(v8::Global::new(scope, document))));
    context.set_slot(Rc::new(Origin(v8::Global::new(
        scope,
        context.get_security_token(scope),
    ))));
    context.set_slot(Rc::new(Proxy(v8::Global::new(scope, global))));
    let getter = v8::Function::builder(document_get)
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope)
        .unwrap();
    let name = v8::String::new(scope, "get document").unwrap();
    getter.set_name(name);
    let undefined = v8::undefined(scope);
    let mut descriptor = v8::PropertyDescriptor::new_from_get_set(getter.into(), undefined.into());
    descriptor.set_enumerable(true);
    descriptor.set_configurable(false);
    assert_eq!(
        global.define_property(scope, key.into(), &descriptor),
        Some(true)
    );
}

pub(super) fn synchronize_origins(scope: &mut v8::PinScope, active: v8::Local<v8::Context>) {
    let proxy = active.global(scope);
    let token = active.get_security_token(scope);
    let contexts = JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<FrameContexts>()
        .contexts
        .clone();
    for context in contexts {
        let context = v8::Local::new(scope, context);
        if context == active {
            continue;
        }
        let Some(previous) = context.get_slot::<Proxy>() else {
            continue;
        };
        if !v8::Local::new(scope, &previous.0).strict_equals(proxy.into()) {
            continue;
        }
        let Some(origin) = context.get_slot::<Origin>() else {
            continue;
        };
        let origin = v8::Local::new(scope, &origin.0);
        if origin.strict_equals(token) {
            context.set_security_token(origin);
        } else {
            context.use_default_security_token();
        }
    }
}

fn cross_message<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    _result: v8::ReturnValue,
) {
    if args.length() == 0 {
        let message = v8::String::new(scope, "A message is required").unwrap();
        let error = v8::Exception::type_error(scope, message);
        scope.throw_exception(error);
        return;
    }
    let Ok(target) = v8::Local::<v8::Object>::try_from(args.data()) else {
        return;
    };
    let source = v8::Global::new(scope, scope.get_entered_or_microtask_context());
    let source = v8::Local::new(scope, source);
    let scope = &mut v8::ContextScope::new(scope, source);
    // Reuse the source realm's private Web IDL/serialization capability; target
    // origin filtering still happens after cloning and ownership detachment.
    let _ = call_frame_helper(
        scope,
        "postWindowMessage",
        &[target.into(), args.get(0), args.get(1), args.get(2)],
    );
}

fn harmless_window_action(
    _scope: &mut v8::PinScope,
    _args: v8::FunctionCallbackArguments,
    _result: v8::ReturnValue,
) {
}

fn cached_function<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    target: v8::Local<'s, v8::Object>,
    name: &str,
    callback: v8::FunctionCallback,
    length: i32,
) -> v8::Local<'s, v8::Function> {
    cached_function_with_data(scope, target, name, callback, length, target.into())
}

fn cached_function_with_data<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    target: v8::Local<'s, v8::Object>,
    name: &str,
    callback: v8::FunctionCallback,
    length: i32,
    data: v8::Local<'s, v8::Value>,
) -> v8::Local<'s, v8::Function> {
    let caller = scope.get_current_context();
    if caller.get_slot::<Functions>().is_none() {
        caller.set_slot(Rc::new(Functions::default()));
    }
    let functions = caller.get_slot::<Functions>().unwrap();
    let mut functions = functions.0.borrow_mut();
    let index = functions
        .iter()
        .position(|(object, _)| v8::Local::new(scope, object).strict_equals(target.into()))
        .unwrap_or_else(|| {
            functions.push((v8::Global::new(scope, target), Default::default()));
            functions.len() - 1
        });
    if let Some(function) = functions[index].1.get(name) {
        return v8::Local::new(scope, function);
    }
    let function = v8::Function::builder_raw(callback)
        .data(data)
        .length(length)
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope)
        .unwrap();
    let key = v8::String::new(scope, name).unwrap();
    function.set_name(key);
    functions[index]
        .1
        .insert(name.to_string(), v8::Global::new(scope, function));
    function
}

fn cross_set<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    value: v8::Local<'s, v8::Value>,
    args: v8::PropertyCallbackArguments<'s>,
    mut result: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
    if !name.is_symbol() && name.to_rust_string_lossy(scope) == "location" {
        location::navigate(scope, args.holder(), value);
        result.set_bool(true);
    } else {
        deny(scope);
    }
    v8::Intercepted::kYes
}

fn cross_descriptor<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    mut result: v8::ReturnValue,
) -> v8::Intercepted {
    let property = if name.is_symbol() {
        None
    } else {
        Some(name.to_rust_string_lossy(scope))
    };
    let allowed_symbol = name.is_symbol()
        && [
            v8::Symbol::get_to_string_tag(scope),
            v8::Symbol::get_has_instance(scope),
            v8::Symbol::get_is_concat_spreadable(scope),
        ]
        .iter()
        .any(|symbol| symbol.strict_equals(name.into()));
    if !allowed_symbol
        && !property
            .as_deref()
            .is_some_and(|property| CROSS_KEYS.contains(&property))
    {
        deny(scope);
        return v8::Intercepted::kYes;
    }
    let object = v8::Object::new(scope);
    for (key, value) in [("enumerable", false), ("configurable", true)] {
        let key = v8::String::new(scope, key).unwrap();
        let value = v8::Boolean::new(scope, value);
        object.set(scope, key.into(), value.into());
    }
    if property.as_deref() == Some("location") {
        for (key, method, callback, length) in [
            (
                "get",
                "get location",
                v8::MapFnTo::map_fn_to(location::window_get),
                0,
            ),
            (
                "set",
                "set location",
                v8::MapFnTo::map_fn_to(location::window_set),
                1,
            ),
        ] {
            let function = cached_function(scope, args.holder(), method, callback, length);
            let key = v8::String::new(scope, key).unwrap();
            object.set(scope, key.into(), function.into());
        }
    } else if property.as_deref().is_some_and(|property| {
        matches!(
            property,
            "window" | "self" | "closed" | "frames" | "length" | "top" | "opener" | "parent"
        )
    }) {
        let property = property.as_deref().unwrap();
        let name = v8::String::new(scope, property).unwrap();
        let data = v8::Array::new_with_elements(scope, &[args.holder().into(), name.into()]);
        let getter = cached_function_with_data(
            scope,
            args.holder(),
            &format!("get {property}"),
            v8::MapFnTo::map_fn_to(cross_property_get),
            0,
            data.into(),
        );
        let key = v8::String::new(scope, "get").unwrap();
        object.set(scope, key.into(), getter.into());
    } else {
        let value = property
            .as_deref()
            .and_then(|name| cross_value(scope, args.holder(), name))
            .unwrap_or_else(|| v8::undefined(scope).into());
        let key = v8::String::new(scope, "value").unwrap();
        object.set(scope, key.into(), value);
        let key = v8::String::new(scope, "writable").unwrap();
        let value = v8::Boolean::new(scope, false);
        object.set(scope, key.into(), value.into());
    }
    result.set(object.into());
    v8::Intercepted::kYes
}

fn cross_query<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    _args: v8::PropertyCallbackArguments<'s>,
    mut result: v8::ReturnValue<v8::Integer>,
) -> v8::Intercepted {
    let allowed = if name.is_symbol() {
        [
            v8::Symbol::get_to_string_tag(scope),
            v8::Symbol::get_has_instance(scope),
            v8::Symbol::get_is_concat_spreadable(scope),
        ]
        .iter()
        .any(|key| key.strict_equals(name.into()))
    } else {
        CROSS_KEYS.contains(&name.to_rust_string_lossy(scope).as_str())
    };
    if allowed {
        result.set_int32(v8::PropertyAttribute::DONT_ENUM.as_u32() as i32);
    } else {
        deny(scope);
    }
    v8::Intercepted::kYes
}

fn cross_property_get(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let data = v8::Local::<v8::Array>::try_from(args.data()).unwrap();
    let target = data.get_index(scope, 0).unwrap().to_object(scope).unwrap();
    if !args.this().strict_equals(target.into()) {
        let message = v8::String::new(scope, "Illegal invocation").unwrap();
        let error = v8::Exception::type_error(scope, message);
        scope.throw_exception(error);
        return;
    }
    let property = data
        .get_index(scope, 1)
        .unwrap()
        .to_rust_string_lossy(scope);
    if let Some(value) = cross_value(scope, target, &property) {
        result.set(value);
    }
}

pub(super) fn install_location(scope: &mut v8::PinScope, url: &str) {
    location::install(scope, url);
}

fn cross_get<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    mut result: v8::ReturnValue<v8::Value>,
) -> v8::Intercepted {
    if name.is_symbol() {
        let allowed = [
            v8::Symbol::get_to_string_tag(scope),
            v8::Symbol::get_has_instance(scope),
            v8::Symbol::get_is_concat_spreadable(scope),
        ];
        if allowed.iter().any(|key| key.strict_equals(name.into())) {
            result.set(v8::undefined(scope).into());
        } else {
            deny(scope);
        }
        return v8::Intercepted::kYes;
    }
    let property = name.to_rust_string_lossy(scope);
    if let Some(value) = cross_value(scope, args.holder(), &property) {
        result.set(value);
    }
    v8::Intercepted::kYes
}

fn cross_value<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    proxy: v8::Local<'s, v8::Object>,
    property: &str,
) -> Option<v8::Local<'s, v8::Value>> {
    let context = proxy.get_creation_context(scope)?;
    Some(match property {
        "self" | "window" | "frames" => proxy.into(),
        "closed" => v8::Boolean::new(scope, context.get_slot::<FrameRetired>().is_some()).into(),
        "opener" => v8::null(scope).into(),
        "location" => location::get(scope, context)?.into(),
        "then" => v8::undefined(scope).into(),
        "length" => {
            let length = children(scope, context).len() as u32;
            v8::Integer::new_from_unsigned(scope, length).into()
        }
        "parent" | "top" => {
            let mut owner = context;
            while let Some(parent) = owner.get_slot::<FrameParent>() {
                owner = v8::Local::new(scope, &parent.0);
                if property == "parent" {
                    break;
                }
            }
            owner.global(scope).into()
        }
        "postMessage" | "blur" | "focus" | "close" => {
            let callback = if property == "postMessage" {
                v8::MapFnTo::map_fn_to(cross_message)
            } else {
                v8::MapFnTo::map_fn_to(harmless_window_action)
            };
            cached_function(
                scope,
                proxy,
                property,
                callback,
                if property == "postMessage" { 1 } else { 0 },
            )
            .into()
        }
        _ => {
            deny(scope);
            return None;
        }
    })
}

fn children<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    owner: v8::Local<'s, v8::Context>,
) -> Vec<v8::Local<'s, v8::Context>> {
    frame_children(scope, owner)
}

fn cross_index<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    index: u32,
    args: v8::PropertyCallbackArguments<'s>,
    mut result: v8::ReturnValue<v8::Value>,
) -> v8::Intercepted {
    if let Some(context) = args.holder().get_creation_context(scope) {
        if let Some(child) = children(scope, context).get(index as usize) {
            result.set(child.global(scope).into());
            return v8::Intercepted::kYes;
        }
    }
    deny(scope);
    v8::Intercepted::kYes
}

fn illegal_window_constructor(
    scope: &mut v8::PinScope,
    _args: v8::FunctionCallbackArguments,
    _result: v8::ReturnValue,
) {
    let message = v8::String::new(scope, "Illegal constructor").unwrap();
    let error = v8::Exception::type_error(scope, message);
    scope.throw_exception(error);
}

const CROSS_KEYS: &[&str] = &[
    "window",
    "self",
    "location",
    "closed",
    "frames",
    "length",
    "top",
    "opener",
    "parent",
    "blur",
    "close",
    "focus",
    "postMessage",
    "then",
];

fn cross_keys<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::PropertyCallbackArguments<'s>,
    mut result: v8::ReturnValue<v8::Array>,
) {
    let mut keys = Vec::new();
    if let Some(context) = args.holder().get_creation_context(scope) {
        for index in 0..children(scope, context).len() {
            keys.push(v8::String::new(scope, &index.to_string()).unwrap().into());
        }
    }
    for key in CROSS_KEYS {
        keys.push(v8::String::new(scope, key).unwrap().into());
    }
    for key in [
        v8::Symbol::get_to_string_tag(scope),
        v8::Symbol::get_has_instance(scope),
        v8::Symbol::get_is_concat_spreadable(scope),
    ] {
        keys.push(key.into());
    }
    result.set(v8::Array::new_with_elements(scope, &keys));
}

pub(super) fn template<'s>(
    scope: &mut v8::PinScope<'s, '_, ()>,
    _template: v8::Local<'s, v8::ObjectTemplate>,
) -> v8::Local<'s, v8::ObjectTemplate> {
    let constructor = v8::FunctionTemplate::new(scope, illegal_window_constructor);
    let name = v8::String::new(scope, "Window").unwrap();
    constructor.set_class_name(name);
    constructor.read_only_prototype();
    let template = constructor.instance_template(scope);
    template.set(name.into(), constructor.into());
    template.set_access_check_callback_and_handler(
        access_allowed,
        v8::NamedPropertyHandlerConfiguration::new()
            .getter(cross_get)
            .setter(cross_set)
            .query(cross_query)
            .descriptor(cross_descriptor)
            .enumerator(cross_keys),
        v8::IndexedPropertyHandlerConfiguration::new().getter(cross_index),
        None,
    );
    template.set_immutable_proto();
    template
}

pub(super) fn context<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    previous: Option<v8::Local<'s, v8::Context>>,
) -> v8::Local<'s, v8::Context> {
    let global = previous.map(|context| {
        let global = context.global(scope);
        context.detach_global();
        global
    });
    let template = v8::ObjectTemplate::new(scope);
    let template = self::template(scope, template);
    let context = v8::Context::new(
        scope,
        v8::ContextOptions {
            global_template: Some(template),
            global_object: global.map(Into::into),
            ..Default::default()
        },
    );
    context
}

pub(super) fn location_installed(scope: &mut v8::PinScope) -> bool {
    location::installed(scope)
}
pub(super) fn seed_location(scope: &mut v8::PinScope, url: &str) {
    let context = scope.get_current_context();
    if let Some(functions) = context.get_slot::<Functions>() {
        let window = context.global(scope);
        // The nonconfigurable root Location accessors keep their identity across
        // pooled installs; cross-frame methods must not retain previous pages.
        functions
            .0
            .borrow_mut()
            .retain(|(target, _)| v8::Local::new(scope, target).strict_equals(window.into()));
    }
    location::seed(scope, url);
}
