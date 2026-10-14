//! Trusted capabilities live in private V8 edges; native lookups never root realms.
use deno_core::v8;
use std::{cell::Cell, rc::Rc};
struct Counter(Cell<u32>);
fn retain<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    owner: v8::Local<'s, v8::Context>,
    value: v8::Local<'s, v8::Value>,
) -> Option<()> {
    if scope.is_execution_terminating() {
        return None;
    }
    register(scope, owner);
    let counter = owner.get_slot::<Counter>().unwrap_or_else(|| {
        let counter = Rc::new(Counter(Cell::new(0)));
        owner.set_slot(counter.clone());
        counter
    });
    let id = counter.0.get().checked_add(1)?;
    counter.0.set(id);
    let scope = &mut v8::ContextScope::new(scope, owner);
    let global = owner.global(scope);
    let name = v8::String::new(scope, "turbo-surf.context-owned-capabilities")?;
    let key = v8::Private::for_api(scope, Some(name));
    let roots = if let Some(roots) = global
        .get_private(scope, key)
        .and_then(|value| v8::Local::<v8::Map>::try_from(value).ok())
    {
        roots
    } else {
        let roots = v8::Map::new(scope);
        if global.set_private(scope, key, roots.into()) != Some(true) {
            return None;
        }
        roots
    };
    let id = v8::Integer::new_from_unsigned(scope, id);
    roots.set(scope, id.into(), value)?;
    Some(())
}
pub(crate) fn function<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    function: v8::Local<'_, v8::Function>,
) -> Option<v8::Weak<v8::Function>> {
    let function = v8::Local::new(scope, function);
    let owner = function.get_creation_context(scope)?;
    retain(scope, owner, function.into())?;
    Some(v8::Weak::new(scope, function))
}
pub(crate) fn object<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'_, v8::Object>,
) -> Option<v8::Weak<v8::Object>> {
    let object = v8::Local::new(scope, object);
    let owner = object.get_creation_context(scope)?;
    retain(scope, owner, object.into())?;
    Some(v8::Weak::new(scope, object))
}
pub(crate) fn parent<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    owner: v8::Local<'_, v8::Context>,
    parent: v8::Local<'_, v8::Context>,
) -> Option<v8::Weak<v8::Context>> {
    let owner = v8::Local::new(scope, owner);
    register(scope, owner);
    let parent = v8::Local::new(scope, parent);
    let value = parent.global(scope);
    let scope = &mut v8::ContextScope::new(scope, owner);
    let name = v8::String::new(scope, "turbo-surf.frame-parent")?;
    let key = v8::Private::for_api(scope, Some(name));
    let global = owner.global(scope);
    if global.set_private(scope, key, value.into()) != Some(true) {
        return None;
    }
    Some(v8::Weak::new(scope, parent))
}
pub(crate) fn release_parent<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    owner: v8::Local<'s, v8::Context>,
) {
    let scope = &mut v8::ContextScope::new(scope, owner);
    if let Some(name) = v8::String::new(scope, "turbo-surf.frame-parent") {
        let key = v8::Private::for_api(scope, Some(name));
        owner.global(scope).delete_private(scope, key);
    }
}
pub(crate) trait NativeHandle<T> {
    fn local<'s>(self, scope: &mut v8::PinScope<'s, '_>) -> Option<v8::Local<'s, T>>;
}
impl<T> NativeHandle<T> for &v8::Global<T> {
    fn local<'s>(self, scope: &mut v8::PinScope<'s, '_>) -> Option<v8::Local<'s, T>> {
        Some(v8::Local::new(scope, self))
    }
}
impl<T> NativeHandle<T> for v8::Global<T> {
    fn local<'s>(self, scope: &mut v8::PinScope<'s, '_>) -> Option<v8::Local<'s, T>> {
        Some(v8::Local::new(scope, self))
    }
}
impl<T> NativeHandle<T> for &v8::Weak<T> {
    fn local<'s>(self, scope: &mut v8::PinScope<'s, '_>) -> Option<v8::Local<'s, T>> {
        self.to_local(scope)
    }
}
impl<T> NativeHandle<T> for v8::Weak<T> {
    fn local<'s>(self, scope: &mut v8::PinScope<'s, '_>) -> Option<v8::Local<'s, T>> {
        self.to_local(scope)
    }
}
pub(crate) fn local<'s, T>(
    scope: &mut v8::PinScope<'s, '_>,
    handle: impl NativeHandle<T>,
) -> v8::Local<'s, T> {
    handle
        .local(scope)
        .expect("the native caller retains its capability realm")
}

#[derive(Default)]
struct Contexts(Vec<v8::Weak<v8::Context>>);
struct Registered;
pub(crate) fn register<'s>(scope: &mut v8::PinScope<'s, '_>, context: v8::Local<'s, v8::Context>) {
    if context.get_slot::<Registered>().is_some() {
        return;
    }
    context.set_slot(Rc::new(Registered));
    let weak = v8::Weak::new(scope, context);
    if scope.get_slot::<Contexts>().is_none() {
        scope.set_slot(Contexts::default());
    }
    scope.get_slot_mut::<Contexts>().unwrap().0.push(weak);
}
pub(crate) fn take_contexts(scope: &mut v8::PinScope) -> Vec<v8::Weak<v8::Context>> {
    scope
        .remove_slot::<Contexts>()
        .map(|contexts| contexts.0)
        .unwrap_or_default()
}
