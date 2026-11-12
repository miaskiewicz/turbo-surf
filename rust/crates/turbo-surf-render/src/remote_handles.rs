//! Host handles root values in their owning realm, without rooting retired realms.
use deno_core::{op2, v8, Extension};
use deno_error::JsErrorBox;
use std::collections::HashMap;

#[derive(Default)]
struct Handles {
    next: u32,
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
    let id = scope
        .get_slot::<Handles>()
        .unwrap()
        .next
        .checked_add(1)
        .ok_or_else(error)?;
    let weak = v8::Weak::new(scope, context);
    let handles = scope.get_slot_mut::<Handles>().unwrap();
    handles.next = id;
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
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let context = scope
        .get_slot::<Handles>()
        .and_then(|handles| handles.owners.get(&id).cloned())
        .and_then(|owner| owner.to_local(scope))
        .ok_or_else(error)?;
    if crate::runtime::native_frame_is_retired(context) {
        return Err(error());
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
pub(crate) fn extension() -> Extension {
    Extension {
        name: "turbo_surf_remote_handles",
        ops: std::borrow::Cow::Owned(vec![
            op_remote_handle_retain(),
            op_remote_handle_get(),
            op_remote_handle_dispose(),
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
