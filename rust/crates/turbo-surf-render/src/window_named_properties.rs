//! Window's named properties object (HTML "named access on the Window object",
//! turbo-surf#77): `WindowProperties` sits between `Window.prototype` and
//! `EventTarget.prototype` and answers string lookups that nothing else on the
//! chain answers (a non-masking interceptor, as Blink installs it). The realm's
//! trusted resolver (window_named_properties.js) maps a name to its child
//! browsing context, element or live HTMLCollection; everything else here is
//! the interceptor shell: page values never reach an `expect`, and a resolver
//! failure only leaves V8's pending exception in place.
use deno_core::{op2, v8};
use deno_error::JsErrorBox;
use turbo_dom_browser_env::host_internals::{
    enter_document_of, handle_of, with_tree, wrap, Handle, Tree,
};

/// The resolver's answer for `name`, or `None` when the name is not supported
/// (or the resolver threw: V8 then reports that pending exception).
fn resolve<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    args: &v8::PropertyCallbackArguments<'s>,
) -> Option<v8::Local<'s, v8::Value>> {
    if !name.is_string() {
        return None;
    }
    let resolver = v8::Local::<v8::Function>::try_from(args.data()).ok()?;
    let receiver = v8::undefined(scope);
    resolver
        .call(scope, receiver.into(), &[name.into()])
        .filter(|value| !value.is_undefined())
}

fn getter<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue,
) -> v8::Intercepted {
    match resolve(scope, name, &args) {
        Some(value) => {
            rv.set(value);
            v8::Intercepted::kYes
        }
        None => v8::Intercepted::kNo,
    }
}

fn query<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue<v8::Integer>,
) -> v8::Intercepted {
    if resolve(scope, name, &args).is_none() {
        return v8::Intercepted::kNo;
    }
    // Named properties are [LegacyUnenumerableNamedProperties]: writable,
    // configurable, not enumerable.
    rv.set_int32(v8::PropertyAttribute::DONT_ENUM.as_u32() as i32);
    v8::Intercepted::kYes
}

fn descriptor<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue,
) -> v8::Intercepted {
    let Some(value) = resolve(scope, name, &args) else {
        return v8::Intercepted::kNo;
    };
    let null = v8::null(scope);
    let names: Vec<v8::Local<v8::Name>> = ["value", "writable", "enumerable", "configurable"]
        .iter()
        .filter_map(|key| v8::String::new(scope, key).map(Into::into))
        .collect();
    if names.len() != 4 {
        return v8::Intercepted::kNo;
    }
    let values = [
        value,
        v8::Boolean::new(scope, true).into(),
        v8::Boolean::new(scope, false).into(),
        v8::Boolean::new(scope, true).into(),
    ];
    let result = v8::Object::with_prototype_and_properties(scope, null.into(), &names, &values);
    rv.set(result.into());
    v8::Intercepted::kYes
}

/// Blink's named property setter is "not supported": strict assignments throw,
/// sloppy ones and `Reflect.set` are ignored. V8 consults it only when the
/// object itself is the receiver; `window.f = 1` still lands on the window.
fn setter<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: v8::Local<'s, v8::Name>,
    _value: v8::Local<'s, v8::Value>,
    args: v8::PropertyCallbackArguments<'s>,
    _rv: v8::ReturnValue<()>,
) -> v8::Intercepted {
    if args.should_throw_on_error() {
        let text = format!(
            "Failed to set a named property '{}' on 'WindowProperties': Named property setter is not supported.",
            name.to_rust_string_lossy(scope)
        );
        if let Some(message) = v8::String::new(scope, &text) {
            let error = v8::Exception::type_error(scope, message);
            scope.throw_exception(error);
        }
    }
    v8::Intercepted::kYes
}

/// Definitions are accepted and dropped, as Blink's interceptor reports them.
fn definer<'s>(
    _scope: &mut v8::PinScope<'s, '_>,
    _name: v8::Local<'s, v8::Name>,
    _descriptor: &v8::PropertyDescriptor,
    _args: v8::PropertyCallbackArguments<'s>,
    _rv: v8::ReturnValue<()>,
) -> v8::Intercepted {
    v8::Intercepted::kYes
}

/// Every string key reports a failed delete, supported or not.
fn deleter<'s>(
    _scope: &mut v8::PinScope<'s, '_>,
    _name: v8::Local<'s, v8::Name>,
    _args: v8::PropertyCallbackArguments<'s>,
    mut rv: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
    rv.set_bool(false);
    v8::Intercepted::kYes
}

fn construct(
    _scope: &mut v8::PinScope,
    _args: v8::FunctionCallbackArguments,
    _rv: v8::ReturnValue,
) {
}

/// Builds the realm's WindowProperties object with `resolver` and links it as
/// `window_prototype`'s prototype, inheriting from `event_target_prototype`.
/// Its own prototype is immutable, like Blink's. Everything is created in
/// `window_prototype`'s realm.
#[op2(fast)]
pub(crate) fn op_install_window_named_properties(
    scope: &mut v8::PinScope,
    window_prototype: v8::Local<v8::Object>,
    event_target_prototype: v8::Local<v8::Object>,
    resolver: v8::Local<v8::Function>,
) -> Result<(), JsErrorBox> {
    let failed = || JsErrorBox::generic("WindowProperties installation interrupted");
    // Ops run in the realm that created them; build the object in the window's.
    let context = window_prototype
        .get_creation_context(scope)
        .ok_or_else(failed)?;
    let scope = &mut v8::ContextScope::new(scope, context);
    let template = v8::FunctionTemplate::new(scope, construct);
    let instance = template.instance_template(scope);
    instance.set_named_property_handler(
        v8::NamedPropertyHandlerConfiguration::new()
            .getter(getter)
            .query(query)
            .descriptor(descriptor)
            .setter(setter)
            .definer(definer)
            .deleter(deleter)
            .data(resolver.into())
            .flags(
                v8::PropertyHandlerFlags::NON_MASKING
                    | v8::PropertyHandlerFlags::ONLY_INTERCEPT_STRINGS,
            ),
    );
    instance.set_immutable_proto();
    let function = template.get_function(scope).ok_or_else(failed)?;
    // Instances take their (then immutable) prototype from the constructor's
    // `prototype` when the initial map is created, i.e. on first instantiation.
    let key = v8::String::new(scope, "prototype").ok_or_else(failed)?;
    if function.set(scope, key.into(), event_target_prototype.into()) != Some(true) {
        return Err(failed());
    }
    let properties = function.new_instance(scope, &[]).ok_or_else(failed)?;
    if !properties
        .get_prototype(scope)
        .is_some_and(|prototype| prototype.strict_equals(event_target_prototype.into()))
    {
        return Err(failed());
    }
    let tag = v8::Symbol::get_to_string_tag(scope);
    let name = v8::String::new(scope, "WindowProperties").ok_or_else(failed)?;
    if properties.define_own_property(
        scope,
        tag.into(),
        name.into(),
        v8::PropertyAttribute::DONT_ENUM | v8::PropertyAttribute::READ_ONLY,
    ) != Some(true)
    {
        return Err(failed());
    }
    if window_prototype.set_prototype(scope, properties.into()) != Some(true) {
        return Err(failed());
    }
    Ok(())
}

/// The connected elements of `document` named `name`, in tree order: every
/// element whose id is `name`, and every HTML embed/form/img/object whose name
/// attribute is `name`. Template contents and shadow trees are not descended
/// into. Read from the native tree on each call, so nothing retains a removed
/// element (a cached index would keep a detached frame's element alive).
#[op2]
pub(crate) fn op_window_named_elements<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    #[string] name: String,
) -> v8::Local<'s, v8::Array> {
    let _document = enter_document_of(scope, document);
    let handles = handle_of(scope, document)
        .and_then(|root| with_tree(|tree| named_elements(tree, root, &name)))
        .unwrap_or_default();
    let values: Vec<v8::Local<v8::Value>> = handles
        .into_iter()
        .map(|handle| wrap(scope, handle).into())
        .collect();
    v8::Array::new_with_elements(scope, &values)
}

const ELEMENT: u8 = 1;
const HTML: u8 = 0;

fn named_elements(tree: &Tree, root: Handle, name: &str) -> Vec<Handle> {
    let mut found = Vec::new();
    if name.is_empty() {
        return found;
    }
    // Depth-first, preorder, without recursion (documents can be deep).
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if node != root {
            if tree.node_type_id(node) != ELEMENT {
                continue;
            }
            let by_name = tree.namespace_id(node) == HTML
                && matches!(
                    tree.local_name(node),
                    Some("embed" | "form" | "img" | "object")
                )
                && tree.get_attribute(node, "name") == Some(name);
            if tree.get_attribute(node, "id") == Some(name) || by_name {
                found.push(node);
            }
        }
        let children = tree.children(node);
        pending.extend(children.into_iter().rev());
    }
    found
}
