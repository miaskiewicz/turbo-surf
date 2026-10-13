//! Shared private Web IDL records use V8's own ephemeron map, so collecting an
//! object releases its state across realms without exposing a guest registry.
use super::*;
use std::rc::Rc;

struct Records {
    map: v8::Global<v8::Object>,
    get: v8::Global<v8::Function>,
    set: v8::Global<v8::Function>,
}
fn initialize(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue) {
    if scope.get_slot::<Rc<Records>>().is_some() {
        return;
    }
    let (Ok(map), Ok(get), Ok(set)) = (
        v8::Local::<v8::Object>::try_from(args.get(0)),
        v8::Local::<v8::Function>::try_from(args.get(1)),
        v8::Local::<v8::Function>::try_from(args.get(2)),
    ) else {
        return;
    };
    let records = Records {
        map: v8::Global::new(scope, map),
        get: v8::Global::new(scope, get),
        set: v8::Global::new(scope, set),
    };
    scope.set_slot(Rc::new(records));
}
pub fn get<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
) -> Option<v8::Local<'s, v8::Value>> {
    let records = scope.get_slot::<Rc<Records>>().cloned()?;
    let map = v8::Local::new(scope, &records.map);
    let function = v8::Local::new(scope, &records.get);
    function.call(scope, map.into(), &[object.into()])
}
pub fn set<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    record: v8::Local<'s, v8::Value>,
) -> bool {
    let Some(records) = scope.get_slot::<Rc<Records>>().cloned() else {
        return false;
    };
    let map = v8::Local::new(scope, &records.map);
    let function = v8::Local::new(scope, &records.set);
    function
        .call(scope, map.into(), &[object.into(), record])
        .is_some()
}
fn get_callback<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue,
) {
    if let Ok(object) = v8::Local::<v8::Object>::try_from(args.get(0)) {
        if let Some(value) = get(scope, object) {
            rv.set(value);
        }
    }
}
fn set_callback<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    _rv: v8::ReturnValue,
) {
    if let Ok(object) = v8::Local::<v8::Object>::try_from(args.get(0)) {
        set(scope, object, args.get(1));
    }
}
pub fn clear(isolate: &mut v8::Isolate) {
    isolate.remove_slot::<Rc<Records>>();
}
pub fn register(scope: &mut v8::PinScope) {
    let global = scope.get_current_context().global(scope);
    bind_method(scope, global, "__initializeCollectionRecords", initialize);
    bind_method(scope, global, "__getCollectionRecord", get_callback);
    bind_method(scope, global, "__setCollectionRecord", set_callback);
    run_js(scope,"(()=>{__initializeCollectionRecords(new WeakMap(),WeakMap.prototype.get,WeakMap.prototype.set);delete globalThis.__initializeCollectionRecords;})()");
}
