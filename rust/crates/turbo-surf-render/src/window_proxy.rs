//! Genuine V8 WindowProxy support, compiled only with the verified native SDK.
use super::*;
#[path = "frame_location.rs"]
mod location;

struct Proxy(v8::Weak<v8::Object>);
struct Document(v8::Weak<v8::Value>);
struct Origin(v8::Weak<v8::Value>);
struct ExceptionConstructor(v8::Weak<v8::Function>);
struct LexicalOwner(v8::Weak<v8::Object>);
#[derive(Default)]
struct WindowContexts(RefCell<Vec<v8::Weak<v8::Context>>>);
fn register_context(scope: &mut v8::PinScope) {
    let context = scope.get_current_context();
    let registry = scope
        .get_slot::<Rc<WindowContexts>>()
        .cloned()
        .unwrap_or_else(|| {
            let registry = Rc::new(WindowContexts::default());
            scope.set_slot(registry.clone());
            registry
        });
    let mut contexts = registry.0.borrow_mut();
    contexts.retain(|context| !context.is_empty());
    if !contexts
        .iter()
        .any(|owner| owner.to_local(scope) == Some(context))
    {
        contexts.push(v8::Weak::new(scope, context));
    }
}
struct Functions {
    map: RefCell<v8::Weak<v8::Object>>,
    constructor: v8::Weak<v8::Function>,
    get: v8::Weak<v8::Function>,
    set: v8::Weak<v8::Function>,
}

fn retain_owned<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: &str,
    value: v8::Local<'s, v8::Value>,
) -> Option<()> {
    let context = scope.get_current_context();
    let owner = if let Some(owner) = context.get_slot::<LexicalOwner>() {
        owner.0.to_local(scope)?
    } else {
        // Native GetPrototype does not skip the hidden JSGlobalObject. Capturing
        // it before navigation preserves lexical ownership when its proxy moves.
        let owner = context
            .global(scope)
            .get_prototype(scope)?
            .to_object(scope)?;
        context.set_slot(Rc::new(LexicalOwner(v8::Weak::new(scope, owner))));
        owner
    };
    let name = v8::String::new(scope, &format!("turbo-surf.lexical-window.{name}"))?;
    let key = v8::Private::for_api(scope, Some(name));
    (owner.set_private(scope, key, value) == Some(true)).then_some(())
}
fn capture_functions(scope: &mut v8::PinScope) -> Option<()> {
    let context = scope.get_current_context();
    if context.get_slot::<Functions>().is_some() {
        return Some(());
    }
    let global = context.global(scope);
    let key = v8::String::new(scope, "WeakMap")?;
    let constructor = v8::Local::<v8::Function>::try_from(global.get(scope, key.into())?).ok()?;
    let key = v8::String::new(scope, "prototype")?;
    let prototype = constructor.get(scope, key.into())?.to_object(scope)?;
    let key = v8::String::new(scope, "get")?;
    let get = v8::Local::<v8::Function>::try_from(prototype.get(scope, key.into())?).ok()?;
    let key = v8::String::new(scope, "set")?;
    let set = v8::Local::<v8::Function>::try_from(prototype.get(scope, key.into())?).ok()?;
    let map = constructor.new_instance(scope, &[])?;
    for (name, value) in [
        ("functions", map.into()),
        ("weakmap-constructor", constructor.into()),
        ("weakmap-get", get.into()),
        ("weakmap-set", set.into()),
    ] {
        retain_owned(scope, name, value)?;
    }
    context.set_slot(Rc::new(Functions {
        map: RefCell::new(v8::Weak::new(scope, map)),
        constructor: v8::Weak::new(scope, constructor),
        get: v8::Weak::new(scope, get),
        set: v8::Weak::new(scope, set),
    }));
    Some(())
}

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
    if capture_functions(scope).is_none() {
        return;
    }
    let global = context.global(scope);
    let key = v8::String::new(scope, "DOMException").unwrap();
    if let Some(function) = global
        .get(scope, key.into())
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    {
        if retain_owned(scope, "exception", function.into()).is_some() {
            context.set_slot(Rc::new(ExceptionConstructor(v8::Weak::new(
                scope, function,
            ))));
        }
    }
}

fn named_exception(scope: &mut v8::PinScope, message: &str, name: &str) {
    let context = scope.get_current_context();
    let message = v8::String::new(scope, message).unwrap();
    let name = v8::String::new(scope, name).unwrap();
    // A retired realm's global proxy may now belong to another origin. Reading
    // its DOMException property here would recursively fail the same access check.
    if let Some(constructor) = context.get_slot::<ExceptionConstructor>() {
        let Some(constructor) = constructor.0.to_local(scope) else {
            return;
        };
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
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let source = scope.get_current_context();
    let lexical = source
        .get_slot::<LexicalOwner>()
        .and_then(|owner| owner.0.to_local(scope));
    let target = if lexical.is_some_and(|owner| owner.strict_equals(args.this().into())) {
        source
            .get_slot::<Proxy>()
            .and_then(|proxy| proxy.0.to_local(scope))
    } else {
        Some(args.this())
    };
    let Some(target) = target else {
        return;
    };
    let Some(active) = target.get_creation_context(scope) else {
        return;
    };
    if !active.global(scope).strict_equals(target.into()) {
        let message = v8::String::new(scope, "Illegal invocation").unwrap();
        let error = v8::Exception::type_error(scope, message);
        scope.throw_exception(error);
        return;
    }
    if !source
        .get_security_token(scope)
        .strict_equals(active.get_security_token(scope))
    {
        deny(scope);
        return;
    }
    let document = active
        .get_slot::<Document>()
        .and_then(|document| document.0.to_local(scope))
        .or_else(|| {
            let name = v8::String::new(scope, "turbo-dom.ownership.window-document")?;
            let key = v8::Private::for_api(scope, Some(name));
            active.global(scope).get_private(scope, key)
        });
    if let Some(document) = document {
        result.set(document);
    }
}

fn closed_get(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let Some(context) = args.this().get_creation_context(scope) else {
        return;
    };
    if !context.global(scope).strict_equals(args.this().into()) {
        let message = v8::String::new(scope, "Illegal invocation").unwrap();
        let error = v8::Exception::type_error(scope, message);
        scope.throw_exception(error);
        return;
    }
    result.set_bool(context.get_slot::<FrameRetired>().is_some());
}

pub(super) fn install_document(scope: &mut v8::PinScope) {
    register_context(scope);
    let context = scope.get_current_context();
    let global = context.global(scope);
    let native_name = v8::String::new(scope, "turbo-dom.ownership.window-document").unwrap();
    let native_key = v8::Private::for_api(scope, Some(native_name));
    let Some(document) = global.get_private(scope, native_key) else {
        return;
    };
    let key = v8::String::new(scope, "document").unwrap();
    let origin = context.get_security_token(scope);
    for (name, value) in [
        ("document", document),
        ("origin", origin),
        ("proxy", global.into()),
    ] {
        if retain_owned(scope, name, value).is_none() {
            return;
        }
    }
    context.set_slot(Rc::new(Document(v8::Weak::new(scope, document))));
    context.set_slot(Rc::new(Origin(v8::Weak::new(scope, origin))));
    context.set_slot(Rc::new(Proxy(v8::Weak::new(scope, global))));
    let Some(closed) = v8::Function::builder(closed_get)
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope)
    else {
        return;
    };
    let name = v8::String::new(scope, "get closed").unwrap();
    closed.set_name(name);
    let undefined = v8::undefined(scope);
    let mut descriptor = v8::PropertyDescriptor::new_from_get_set(closed.into(), undefined.into());
    descriptor.set_enumerable(true);
    descriptor.set_configurable(true);
    let name = v8::String::new(scope, "closed").unwrap();
    if global.define_property(scope, name.into(), &descriptor) != Some(true) {
        return;
    }
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
    let contexts = scope
        .get_slot::<Rc<WindowContexts>>()
        .map(|registry| {
            let mut contexts = registry.0.borrow_mut();
            contexts.retain(|context| !context.is_empty());
            contexts.clone()
        })
        .unwrap_or_default();
    for context in contexts {
        let Some(context) = context.to_local(scope) else {
            continue;
        };
        if context == active {
            continue;
        }
        let Some(previous) = context.get_slot::<Proxy>() else {
            continue;
        };
        let Some(previous) = previous.0.to_local(scope) else {
            continue;
        };
        if !previous.strict_equals(proxy.into()) {
            continue;
        }
        let Some(origin) = context.get_slot::<Origin>() else {
            continue;
        };
        let Some(origin) = origin.0.to_local(scope) else {
            continue;
        };
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
) -> Option<v8::Local<'s, v8::Function>> {
    cached_function_with_data(scope, target, name, callback, length, target.into())
}

fn cached_function_with_data<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    target: v8::Local<'s, v8::Object>,
    name: &str,
    callback: v8::FunctionCallback,
    length: i32,
    data: v8::Local<'s, v8::Value>,
) -> Option<v8::Local<'s, v8::Function>> {
    capture_functions(scope)?;
    let caller = scope.get_current_context();
    let functions = caller.get_slot::<Functions>()?;
    let map = functions.map.borrow().to_local(scope)?;
    let get = functions.get.to_local(scope)?;
    let row = get.call(scope, map.into(), &[target.into()])?;
    let row = if let Ok(row) = v8::Local::<v8::Map>::try_from(row) {
        row
    } else {
        let row = v8::Map::new(scope);
        let set = functions.set.to_local(scope)?;
        set.call(scope, map.into(), &[target.into(), row.into()])?;
        row
    };
    let key = v8::String::new(scope, name)?;
    if let Some(function) = row
        .get(scope, key.into())
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    {
        return Some(function);
    }
    let function = v8::Function::builder_raw(callback)
        .data(data)
        .length(length)
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope)?;
    function.set_name(key);
    row.set(scope, key.into(), function.into())?;
    Some(function)
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
            let Some(function) = cached_function(scope, args.holder(), method, callback, length)
            else {
                return v8::Intercepted::kYes;
            };
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
        let Some(getter) = cached_function_with_data(
            scope,
            args.holder(),
            &format!("get {property}"),
            v8::MapFnTo::map_fn_to(cross_property_get),
            0,
            data.into(),
        ) else {
            return v8::Intercepted::kYes;
        };
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
                let Some(parent) = parent.0.to_local(scope) else {
                    break;
                };
                owner = parent;
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
            )?
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
        let old_map = functions.map.borrow().to_local(scope);
        if let (Some(map), Some(get), Some(set), Some(constructor)) = (
            old_map,
            functions.get.to_local(scope),
            functions.set.to_local(scope),
            functions.constructor.to_local(scope),
        ) {
            let window = context.global(scope);
            if let (Some(row), Some(new)) = (
                get.call(scope, map.into(), &[window.into()]),
                constructor.new_instance(scope, &[]),
            ) {
                if set.call(scope, new.into(), &[window.into(), row]).is_some()
                    && retain_owned(scope, "functions", new.into()).is_some()
                {
                    *functions.map.borrow_mut() = v8::Weak::new(scope, new);
                }
            }
        }
    }
    location::seed(scope, url);
}
