//! Host handles root values in their owning realm, without rooting retired realms.
use deno_core::{op2, v8, Extension};
use deno_error::JsErrorBox;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
static NEXT_HANDLE: AtomicU32 = AtomicU32::new(1);

#[derive(Default)]
struct Handles {
    owners: HashMap<u32, v8::Weak<v8::Context>>,
}
fn key<'s>(scope: &mut v8::PinScope<'s, '_>) -> Option<v8::Local<'s, v8::Private>> {
    let name = v8::String::new(scope, "turbo-surf.remote-handles")?;
    Some(v8::Private::for_api(scope, Some(name)))
}
fn values<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) -> Option<v8::Local<'s, v8::Map>> {
    let key = key(scope)?;
    let global = context.global(scope);
    if let Some(value) = global
        .get_private(scope, key)
        .and_then(|value| v8::Local::<v8::Map>::try_from(value).ok())
    {
        return Some(value);
    }
    let values = v8::Map::new(scope);
    (global.set_private(scope, key, values.into()) == Some(true)).then_some(values)
}
fn error() -> JsErrorBox {
    JsErrorBox::generic("JSHandle is disposed or its execution context was destroyed")
}
#[op2(fast)]
fn op_remote_handle_retain<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    owner: v8::Local<'s, v8::Object>,
    value: v8::Local<'s, v8::Value>,
) -> Result<u32, JsErrorBox> {
    let context = owner.get_creation_context(scope).ok_or_else(error)?;
    crate::context_ownership::register(scope, context);
    if crate::runtime::native_frame_is_retired(context) {
        return Err(error());
    }
    if scope.get_slot::<Handles>().is_none() {
        scope.set_slot(Handles::default());
    }
    let id = NEXT_HANDLE
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
            next.checked_add(1)
        })
        .map_err(|_| error())?;
    let weak = v8::Weak::new(scope, context);
    let handles = scope.get_slot_mut::<Handles>().unwrap();
    handles.owners.retain(|_, owner| !owner.is_empty());
    handles.owners.insert(id, weak);
    let map = values(scope, context).ok_or_else(error)?;
    let number = v8::Integer::new_from_unsigned(scope, id);
    map.set(scope, number.into(), value).ok_or_else(error)?;
    Ok(id)
}
#[op2]
fn op_remote_handle_get<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    id: u32,
    expected_owner: v8::Local<'s, v8::Value>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let context = scope
        .get_slot::<Handles>()
        .and_then(|handles| handles.owners.get(&id).cloned())
        .and_then(|owner| owner.to_local(scope))
        .ok_or_else(error)?;
    if crate::runtime::native_frame_is_retired(context) {
        return Err(error());
    }
    if !expected_owner.is_undefined() {
        let expected = v8::Local::<v8::Object>::try_from(expected_owner)
            .ok()
            .and_then(|owner| owner.get_creation_context(scope))
            .ok_or_else(error)?;
        if expected != context {
            return Err(JsErrorBox::generic(
                "JSHandle belongs to a different execution context",
            ));
        }
    }
    let map = values(scope, context).ok_or_else(error)?;
    let number = v8::Integer::new_from_unsigned(scope, id);
    if map.has(scope, number.into()) != Some(true) {
        return Err(error());
    }
    map.get(scope, number.into()).ok_or_else(error)
}
#[op2(fast)]
fn op_remote_handle_dispose(scope: &mut v8::PinScope, id: u32) {
    dispose(scope, id);
}
pub(crate) fn dispose(scope: &mut v8::PinScope, id: u32) {
    let owner = scope
        .get_slot_mut::<Handles>()
        .and_then(|handles| handles.owners.remove(&id));
    if let Some(context) = owner.and_then(|owner| owner.to_local(scope)) {
        if let Some(map) = values(scope, context) {
            let number = v8::Integer::new_from_unsigned(scope, id);
            map.delete(scope, number.into());
        }
    }
}
pub(crate) fn retire_context(scope: &mut v8::PinScope, context: v8::Local<v8::Context>) {
    let owners = scope
        .get_slot::<Handles>()
        .map(|handles| {
            handles
                .owners
                .iter()
                .map(|(id, owner)| (*id, owner.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let retired = owners
        .into_iter()
        .filter_map(|(id, owner)| (owner.to_local(scope) == Some(context)).then_some(id))
        .collect::<Vec<_>>();
    if let Some(handles) = scope.get_slot_mut::<Handles>() {
        for id in retired {
            handles.owners.remove(&id);
        }
    }
    if let Some(key) = key(scope) {
        let global = context.global(scope);
        global.delete_private(scope, key);
    }
}

#[op2(fast)]
fn op_remote_handle_count(scope: &mut v8::PinScope) -> u32 {
    scope.get_slot::<Handles>().map_or(0, |handles| {
        handles
            .owners
            .values()
            .filter(|owner| !owner.is_empty())
            .count() as u32
    })
}
#[op2(reentrant)]
fn op_remote_handle_compile<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    owner: v8::Local<'s, v8::Object>,
    #[string] source: &str,
) -> Result<v8::Local<'s, v8::Function>, JsErrorBox> {
    let context = owner.get_creation_context(scope).ok_or_else(error)?;
    if crate::runtime::native_frame_is_retired(context) {
        return Err(error());
    }
    let scope = &mut v8::ContextScope::new(scope, context);
    let code =
        v8::String::new(scope, &format!("(function(){{\n{source}\n}})")).ok_or_else(error)?;
    let script = v8::Script::compile(scope, code, None).ok_or_else(error)?;
    script
        .run(scope)
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
        .ok_or_else(error)
}

pub(crate) fn extension() -> Extension {
    Extension {
        name: "turbo_surf_remote_handles",
        ops: std::borrow::Cow::Owned(vec![
            op_remote_handle_retain(),
            op_remote_handle_get(),
            op_remote_handle_dispose(),
            op_remote_handle_count(),
            op_remote_handle_compile(),
        ]),
        ..Default::default()
    }
}

pub(crate) fn diagnostics(isolate: &v8::Isolate) -> serde_json::Value {
    let Some(handles) = isolate.get_slot::<Handles>() else {
        return serde_json::json!({"retained":0,"collected":0});
    };
    let collected = handles
        .owners
        .values()
        .filter(|owner| owner.is_empty())
        .count();
    serde_json::json!({"retained":handles.owners.len()-collected,"collected":collected})
}
