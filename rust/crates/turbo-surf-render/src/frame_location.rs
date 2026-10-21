//! Native per-document Location objects and cross-origin navigation checks.
use super::*;
#[path = "frame_ancestors.rs"]
mod ancestors;

struct State {
    object: v8::Weak<v8::Object>,
    window: v8::Weak<v8::Object>,
    url: RefCell<String>,
    ancestors: v8::Weak<v8::Object>,
    ancestor_origins: Vec<String>,
}

fn type_error(scope: &mut v8::PinScope, text: &str) {
    let text = v8::String::new(scope, text).unwrap();
    let error = v8::Exception::type_error(scope, text);
    scope.throw_exception(error);
}

fn object_state(scope: &mut v8::PinScope, object: v8::Local<v8::Object>) -> Option<Rc<State>> {
    let state = object.get_creation_context(scope)?.get_slot::<State>()?;
    state
        .object
        .to_local(scope)?
        .strict_equals(object.into())
        .then_some(state)
}

pub(super) fn navigate<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window: v8::Local<'s, v8::Object>,
    value: v8::Local<'s, v8::Value>,
) {
    let Some(text) = value.to_string(scope) else {
        return;
    };
    let source = scope.get_entered_or_microtask_context();
    let Some(target) = window.get_creation_context(scope) else {
        return;
    };
    if target.get_slot::<FrameRetired>().is_some() {
        return;
    }
    // Sandboxing constrains even same-origin descendants; origin permission
    // cannot grant a sandboxed frame authority over the top-level document.
    if source != target
        && target.get_slot::<FrameParent>().is_none()
        && source
            .get_slot::<FrameSandbox>()
            .is_some_and(|flags| flags.top_navigation_without_activation_blocked)
    {
        deny(scope);
        return;
    }
    let mut ancestor = target;
    let mut allowed = source
        .get_security_token(scope)
        .strict_equals(target.get_security_token(scope));
    if !allowed
        && target.get_slot::<FrameParent>().is_none()
        && source.get_slot::<FrameSandbox>().is_some_and(|flags| {
            flags.top_navigation_without_activation_permitted
                && !flags.top_navigation_without_activation_blocked
        })
    {
        let mut ancestor = source;
        while let Some(parent) = ancestor.get_slot::<FrameParent>() {
            let Some(parent) = parent.0.to_local(scope) else {
                break;
            };
            ancestor = parent;
            if ancestor == target {
                allowed = true;
                break;
            }
        }
    }
    while !allowed {
        let Some(parent) = ancestor.get_slot::<FrameParent>() else {
            break;
        };
        let Some(parent) = parent.0.to_local(scope) else {
            break;
        };
        ancestor = parent;
        allowed = ancestor == source;
    }
    if !allowed {
        deny(scope);
        return;
    }
    let source_url = source
        .get_slot::<FrameDocumentUrl>()
        .map(|url| url.0.clone())
        .unwrap_or_else(|| {
            JsRuntime::op_state_from(scope)
                .borrow()
                .borrow::<Base>()
                .0
                .clone()
        });
    let base = source
        .get_slot::<FrameScriptPolicy>()
        .map(|policy| policy.base.clone())
        .unwrap_or_else(|| source_url.clone());
    let url =
        ModuleSpecifier::parse(&base).and_then(|base| base.join(&text.to_rust_string_lossy(scope)));
    let Ok(url) = url else {
        named_exception(scope, "Invalid navigation URL", "SyntaxError");
        return;
    };
    if !matches!(url.scheme(), "http" | "https" | "about")
        || (url.scheme() == "about" && url.as_str() != "about:blank")
    {
        named_exception(
            scope,
            "This navigation scheme is not supported",
            "NotSupportedError",
        );
        return;
    }
    let Some(host) = target.get_slot::<FrameHost>() else {
        if let Some(state) = target.get_slot::<State>() {
            script_navigation::record(source, target, url.to_string());
            *state.url.borrow_mut() = url.to_string();
        }
        return;
    };
    let Some(parent) = target.get_slot::<FrameParent>() else {
        return;
    };
    let Some(parent) = parent.0.to_local(scope) else {
        return;
    };
    let Some(navigator) = parent.get_slot::<FrameNavigator>() else {
        return;
    };
    let (Some(navigator), Some(host)) = (navigator.0.to_local(scope), host.0.to_local(scope))
    else {
        return;
    };
    let url = v8::String::new(scope, url.as_str()).unwrap();
    let source_url = v8::String::new(scope, &source_url).unwrap();
    let undefined = v8::undefined(scope);
    let scope = &mut v8::ContextScope::new(scope, parent);
    navigator.call(
        scope,
        undefined.into(),
        &[host.into(), url.into(), source_url.into()],
    );
}

pub(super) fn window_set<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    _result: v8::ReturnValue,
) {
    let window = v8::Local::<v8::Object>::try_from(args.data()).unwrap();
    if !args.this().strict_equals(window.into()) {
        type_error(scope, "Illegal invocation");
        return;
    }
    navigate(scope, window, args.get(0));
}

pub(super) fn window_get<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut result: v8::ReturnValue,
) {
    let window = v8::Local::<v8::Object>::try_from(args.data()).unwrap();
    if !args.this().strict_equals(window.into()) {
        type_error(scope, "Illegal invocation");
        return;
    }
    if let Some(location) = window
        .get_creation_context(scope)
        .and_then(|context| context.get_slot::<State>())
    {
        if let Some(location) = location.object.to_local(scope) {
            result.set(location.into());
        }
    }
}

fn href_set<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    _result: v8::ReturnValue,
) {
    let Some(state) = object_state(scope, args.this()) else {
        type_error(scope, "Illegal invocation");
        return;
    };
    let Some(window) = state.window.to_local(scope) else {
        return;
    };
    navigate(scope, window, args.get(0));
}

fn replace<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    result: v8::ReturnValue,
) {
    if args.length() == 0 {
        type_error(scope, "A URL is required");
        return;
    }
    href_set(scope, args, result);
}

fn reload<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    _result: v8::ReturnValue,
) {
    let location = v8::Local::<v8::Object>::try_from(args.data()).unwrap();
    if !args.this().strict_equals(location.into()) {
        type_error(scope, "Illegal invocation");
        return;
    }
    if let Some(state) = object_state(scope, location) {
        let Some(window) = state.window.to_local(scope) else {
            return;
        };
        let url = v8::String::new(scope, &state.url.borrow()).unwrap();
        navigate(scope, window, url.into());
    }
}

fn readable(scope: &mut v8::PinScope, state: &State) -> bool {
    let source = scope.get_entered_or_microtask_context();
    let Some(object) = state.object.to_local(scope) else {
        return false;
    };
    let allowed = object.get_creation_context(scope).is_some_and(|owner| {
        source
            .get_security_token(scope)
            .strict_equals(owner.get_security_token(scope))
    });
    if !allowed {
        deny(scope);
    }
    allowed
}

fn href_get(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let Some(state) = object_state(scope, args.this()) else {
        type_error(scope, "Illegal invocation");
        return;
    };
    if readable(scope, &state) {
        result.set(v8::String::new(scope, &state.url.borrow()).unwrap().into());
    }
}

fn component_get(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let Some(state) = object_state(scope, args.this()) else {
        type_error(scope, "Illegal invocation");
        return;
    };
    if !state
        .object
        .to_local(scope)
        .is_some_and(|object| object.strict_equals(args.this().into()))
    {
        type_error(scope, "Illegal invocation");
        return;
    }
    if !readable(scope, &state) {
        return;
    }
    let property = args.data().to_rust_string_lossy(scope);
    let Ok(url) = ModuleSpecifier::parse(&state.url.borrow()) else {
        return;
    };
    let value = match property.as_str() {
        "origin" => url.origin().ascii_serialization(),
        "protocol" => format!("{}:", url.scheme()),
        "host" => url
            .host_str()
            .map(|host| {
                url.port()
                    .map_or_else(|| host.to_string(), |port| format!("{host}:{port}"))
            })
            .unwrap_or_default(),
        "hostname" => url.host_str().unwrap_or("").to_string(),
        "port" => url.port().map(|port| port.to_string()).unwrap_or_default(),
        "pathname" => url.path().to_string(),
        "search" => url
            .query()
            .filter(|query| !query.is_empty())
            .map(|query| format!("?{query}"))
            .unwrap_or_default(),
        "hash" => url
            .fragment()
            .filter(|hash| !hash.is_empty())
            .map(|hash| format!("#{hash}"))
            .unwrap_or_default(),
        _ => String::new(),
    };
    result.set(v8::String::new(scope, &value).unwrap().into());
}

fn component_set<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    _result: v8::ReturnValue,
) {
    let Some(state) = object_state(scope, args.this()) else {
        type_error(scope, "Illegal invocation");
        return;
    };
    let Some(value) = args.get(0).to_string(scope) else {
        return;
    };
    let value = value.to_rust_string_lossy(scope);
    let property = args.data().to_rust_string_lossy(scope);
    let Ok(mut url) = ModuleSpecifier::parse(&state.url.borrow()) else {
        return;
    };
    let valid = match property.as_str() {
        "protocol" => url.set_scheme(value.trim_end_matches(':')).is_ok(),
        "hostname" => url.set_host(Some(&value)).is_ok(),
        "host" => {
            if let Ok(parsed) = ModuleSpecifier::parse(&format!("{}://{value}/", url.scheme())) {
                url.set_host(parsed.host_str()).is_ok() && url.set_port(parsed.port()).is_ok()
            } else {
                false
            }
        }
        "port" => {
            if value.is_empty() {
                url.set_port(None).is_ok()
            } else {
                value
                    .parse::<u16>()
                    .ok()
                    .is_some_and(|port| url.set_port(Some(port)).is_ok())
            }
        }
        "pathname" => {
            url.set_path(&value);
            true
        }
        "search" => {
            url.set_query(if value.is_empty() {
                None
            } else {
                Some(value.trim_start_matches('?'))
            });
            true
        }
        "hash" => {
            url.set_fragment(if value.is_empty() {
                None
            } else {
                Some(value.trim_start_matches('#'))
            });
            true
        }
        _ => false,
    };
    if valid {
        let value = v8::String::new(scope, url.as_str()).unwrap();
        let Some(window) = state.window.to_local(scope) else {
            return;
        };
        navigate(scope, window, value.into());
    }
}

fn allowed_key(scope: &mut v8::PinScope, name: v8::Local<v8::Name>) -> bool {
    if name.is_symbol() {
        return [
            v8::Symbol::get_to_string_tag(scope),
            v8::Symbol::get_has_instance(scope),
            v8::Symbol::get_is_concat_spreadable(scope),
        ]
        .iter()
        .any(|symbol| symbol.strict_equals(name.into()));
    }
    matches!(
        name.to_rust_string_lossy(scope).as_str(),
        "href" | "replace" | "then"
    )
}

fn cross_get<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    mut result: v8::ReturnValue,
) -> v8::Intercepted {
    if allowed_key(scope, name) {
        if !name.is_symbol() && name.to_rust_string_lossy(scope) == "replace" {
            let location = args.holder();
            result.set(
                {
                    let Some(function) = cached_function(
                        scope,
                        location,
                        "replace",
                        v8::MapFnTo::map_fn_to(replace),
                        1,
                    ) else {
                        return v8::Intercepted::kYes;
                    };
                    function
                }
                .into(),
            );
        } else if !name.is_symbol() && name.to_rust_string_lossy(scope) == "href" {
            deny(scope);
        } else {
            result.set(v8::undefined(scope).into());
        }
    } else {
        deny(scope);
    }
    v8::Intercepted::kYes
}

fn cross_set<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    value: v8::Local<'s, v8::Value>,
    args: v8::PropertyCallbackArguments<'s>,
    mut result: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
    if !name.is_symbol() && name.to_rust_string_lossy(scope) == "href" {
        if let Some(state) = object_state(scope, args.holder()) {
            let Some(window) = state.window.to_local(scope) else {
                return v8::Intercepted::kYes;
            };
            navigate(scope, window, value);
            result.set_bool(true);
        }
    } else {
        deny(scope);
    }
    v8::Intercepted::kYes
}

fn keys<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    _args: v8::PropertyCallbackArguments<'s>,
    mut result: v8::ReturnValue<v8::Array>,
) {
    let mut values = ["href", "replace", "then"]
        .iter()
        .map(|name| v8::String::new(scope, name).unwrap().into())
        .collect::<Vec<_>>();
    for symbol in [
        v8::Symbol::get_to_string_tag(scope),
        v8::Symbol::get_has_instance(scope),
        v8::Symbol::get_is_concat_spreadable(scope),
    ] {
        values.push(symbol.into());
    }
    result.set(v8::Array::new_with_elements(scope, &values));
}

fn cross_query(
    scope: &mut v8::PinScope,
    name: v8::Local<v8::Name>,
    _args: v8::PropertyCallbackArguments,
    mut result: v8::ReturnValue<v8::Integer>,
) -> v8::Intercepted {
    if allowed_key(scope, name) {
        result.set_int32(v8::PropertyAttribute::DONT_ENUM.as_u32() as i32);
    } else {
        deny(scope);
    }
    v8::Intercepted::kYes
}

fn descriptor<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    mut result: v8::ReturnValue,
) -> v8::Intercepted {
    if !allowed_key(scope, name) {
        deny(scope);
        return v8::Intercepted::kYes;
    }
    let descriptor = v8::Object::new(scope);
    for (key, value) in [("enumerable", false), ("configurable", true)] {
        let key = v8::String::new(scope, key).unwrap();
        let value = v8::Boolean::new(scope, value);
        descriptor.set(scope, key.into(), value.into());
    }
    let property = if name.is_symbol() {
        "".to_string()
    } else {
        name.to_rust_string_lossy(scope)
    };
    let (key, value): (&str, v8::Local<v8::Value>) = match property.as_str() {
        "href" => (
            "set",
            {
                let Some(function) = cached_function(
                    scope,
                    args.holder(),
                    "set href",
                    v8::MapFnTo::map_fn_to(href_set),
                    1,
                ) else {
                    return v8::Intercepted::kYes;
                };
                function
            }
            .into(),
        ),
        "replace" => (
            "value",
            {
                let Some(function) = cached_function(
                    scope,
                    args.holder(),
                    "replace",
                    v8::MapFnTo::map_fn_to(replace),
                    1,
                ) else {
                    return v8::Intercepted::kYes;
                };
                function
            }
            .into(),
        ),
        _ => ("value", v8::undefined(scope).into()),
    };
    if key == "value" {
        let writable = v8::String::new(scope, "writable").unwrap();
        let value = v8::Boolean::new(scope, false);
        descriptor.set(scope, writable.into(), value.into());
    }
    let key = v8::String::new(scope, key).unwrap();
    descriptor.set(scope, key.into(), value);
    result.set(descriptor.into());
    v8::Intercepted::kYes
}

fn illegal(
    scope: &mut v8::PinScope,
    _args: v8::FunctionCallbackArguments,
    _result: v8::ReturnValue,
) {
    type_error(scope, "Illegal constructor");
}

pub(super) fn install(scope: &mut v8::PinScope, url: &str) {
    let context = scope.get_current_context();
    let window = context.global(scope);
    let constructor = v8::FunctionTemplate::new(scope, illegal);
    let name = v8::String::new(scope, "Location").unwrap();
    constructor.set_class_name(name);
    constructor.read_only_prototype();
    let template = constructor.instance_template(scope);
    template.set_access_check_callback_and_handler(
        access_allowed,
        v8::NamedPropertyHandlerConfiguration::new()
            .getter(cross_get)
            .setter(cross_set)
            .query(cross_query)
            .enumerator(keys)
            .descriptor(descriptor),
        v8::IndexedPropertyHandlerConfiguration::new(),
        None,
    );
    template.set_immutable_proto();
    let location = template.new_instance(scope).unwrap();
    let origins = ancestors::origins(scope, context);
    let ancestors = ancestors::install(scope);
    for (name, value) in [
        ("location", location.into()),
        ("location-ancestors", ancestors.into()),
    ] {
        if retain_owned(scope, name, value).is_none() {
            return;
        }
    }
    let name = v8::String::new(scope, "turbo-surf.location-window").unwrap();
    let key = v8::Private::for_api(scope, Some(name));
    if location.set_private(scope, key, window.into()) != Some(true) {
        return;
    }
    context.set_slot(Rc::new(State {
        object: v8::Weak::new(scope, location),
        window: v8::Weak::new(scope, window),
        url: RefCell::new(url.to_string()),
        ancestors: v8::Weak::new(scope, ancestors),
        ancestor_origins: origins,
    }));
    let object_key = v8::String::new(scope, "Object").unwrap();
    let object = window
        .get(scope, object_key.into())
        .unwrap()
        .to_object(scope)
        .unwrap();
    let prototype_key = v8::String::new(scope, "prototype").unwrap();
    let prototype = object
        .get(scope, prototype_key.into())
        .unwrap()
        .to_object(scope)
        .unwrap();
    let name = v8::String::new(scope, "valueOf").unwrap();
    let value = prototype.get(scope, name.into()).unwrap();
    location.define_own_property(
        scope,
        name.into(),
        value,
        v8::PropertyAttribute::READ_ONLY
            | v8::PropertyAttribute::DONT_ENUM
            | v8::PropertyAttribute::DONT_DELETE,
    );
    let primitive = v8::Symbol::get_to_primitive(scope);
    let undefined = v8::undefined(scope);
    location.define_own_property(
        scope,
        primitive.into(),
        undefined.into(),
        v8::PropertyAttribute::READ_ONLY
            | v8::PropertyAttribute::DONT_ENUM
            | v8::PropertyAttribute::DONT_DELETE,
    );
    let getter = v8::Function::builder(ancestors::get)
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope)
        .unwrap();
    let name = v8::String::new(scope, "get ancestorOrigins").unwrap();
    getter.set_name(name);
    let undefined = v8::undefined(scope);
    let mut descriptor = v8::PropertyDescriptor::new_from_get_set(getter.into(), undefined.into());
    descriptor.set_enumerable(true);
    descriptor.set_configurable(false);
    let name = v8::String::new(scope, "ancestorOrigins").unwrap();
    location.define_property(scope, name.into(), &descriptor);
    let Some(getter) = cached_function(
        scope,
        location,
        "get href",
        v8::MapFnTo::map_fn_to(href_get),
        0,
    ) else {
        return;
    };
    let Some(setter) = cached_function(
        scope,
        location,
        "set href",
        v8::MapFnTo::map_fn_to(href_set),
        1,
    ) else {
        return;
    };
    let name = v8::String::new(scope, "href").unwrap();
    let mut descriptor = v8::PropertyDescriptor::new_from_get_set(getter.into(), setter.into());
    descriptor.set_enumerable(true);
    descriptor.set_configurable(false);
    location.define_property(scope, name.into(), &descriptor);
    for property in [
        "origin", "protocol", "host", "hostname", "port", "pathname", "search", "hash",
    ] {
        let data = v8::String::new(scope, property).unwrap();
        let getter = v8::Function::builder(component_get)
            .data(data.into())
            .constructor_behavior(v8::ConstructorBehavior::Throw)
            .build(scope)
            .unwrap();
        let name = v8::String::new(scope, &format!("get {property}")).unwrap();
        getter.set_name(name);
        let setter: v8::Local<v8::Value> = if property == "origin" {
            v8::undefined(scope).into()
        } else {
            let setter = v8::Function::builder(component_set)
                .data(data.into())
                .length(1)
                .constructor_behavior(v8::ConstructorBehavior::Throw)
                .build(scope)
                .unwrap();
            let name = v8::String::new(scope, &format!("set {property}")).unwrap();
            setter.set_name(name);
            setter.into()
        };
        let name = v8::String::new(scope, property).unwrap();
        let mut descriptor = v8::PropertyDescriptor::new_from_get_set(getter.into(), setter);
        descriptor.set_enumerable(true);
        descriptor.set_configurable(false);
        location.define_property(scope, name.into(), &descriptor);
    }
    let name = v8::String::new(scope, "Location").unwrap();
    let function = constructor.get_function(scope).unwrap();
    let prototype_key = v8::String::new(scope, "prototype").unwrap();
    let prototype = function
        .get(scope, prototype_key.into())
        .unwrap()
        .to_object(scope)
        .unwrap();
    let tag = v8::Symbol::get_to_string_tag(scope);
    let value = v8::String::new(scope, "Location").unwrap();
    prototype.define_own_property(
        scope,
        tag.into(),
        value.into(),
        v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_ENUM,
    );
    window.set(scope, name.into(), function.into());
    for (name, callback, length) in [
        ("assign", v8::MapFnTo::map_fn_to(replace), 1),
        ("reload", v8::MapFnTo::map_fn_to(reload), 0),
        ("replace", v8::MapFnTo::map_fn_to(replace), 1),
        ("toString", v8::MapFnTo::map_fn_to(href_get), 0),
    ] {
        let Some(function) = cached_function(scope, location, name, callback, length) else {
            return;
        };
        let name = v8::String::new(scope, name).unwrap();
        location.define_own_property(
            scope,
            name.into(),
            function.into(),
            v8::PropertyAttribute::READ_ONLY | v8::PropertyAttribute::DONT_DELETE,
        );
    }
    let Some(getter) = cached_function(
        scope,
        window,
        "get location",
        v8::MapFnTo::map_fn_to(window_get),
        0,
    ) else {
        return;
    };
    let Some(setter) = cached_function(
        scope,
        window,
        "set location",
        v8::MapFnTo::map_fn_to(window_set),
        1,
    ) else {
        return;
    };
    let name = v8::String::new(scope, "location").unwrap();
    let mut descriptor = v8::PropertyDescriptor::new_from_get_set(getter.into(), setter.into());
    descriptor.set_enumerable(true);
    descriptor.set_configurable(false);
    window.define_property(scope, name.into(), &descriptor);
}

pub(super) fn get<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) -> Option<v8::Local<'s, v8::Object>> {
    context
        .get_slot::<State>()
        .and_then(|state| state.object.to_local(scope))
}

pub(super) fn installed(scope: &mut v8::PinScope) -> bool {
    scope.get_current_context().get_slot::<State>().is_some()
}
pub(super) fn seed(scope: &mut v8::PinScope, url: &str) {
    if let Some(state) = scope.get_current_context().get_slot::<State>() {
        *state.url.borrow_mut() = url.to_string();
    }
}

pub(super) fn url(context: v8::Local<v8::Context>) -> Option<String> {
    context
        .get_slot::<State>()
        .map(|state| state.url.borrow().clone())
}
