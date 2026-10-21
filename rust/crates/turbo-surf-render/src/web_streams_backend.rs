//! Official stream/abort algorithms behind realm-local, private DOM interfaces.
use deno_core::{op2, v8};
use deno_error::JsErrorBox;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

pub(crate) const SOURCE: &str = include_str!("web_streams_backend.js");
#[derive(Default)]
struct Bridges {
    next: u32,
    values: HashMap<u32, Bridge>,
    dirty: Rc<Cell<bool>>,
}

struct Bridge {
    object: v8::Weak<v8::Object>,
    callback: v8::Weak<v8::Function>,
    raw: v8::Weak<v8::Object>,
    listener: Option<v8::Weak<v8::Function>>,
    cleanup: v8::Weak<v8::Function>,
}

#[op2(nofast, reentrant)]
pub(crate) fn op_web_abort_bind(
    scope: &mut v8::PinScope,
    object: v8::Local<v8::Object>,
    callback: v8::Local<v8::Function>,
    raw: v8::Local<v8::Object>,
    cleanup: v8::Local<v8::Function>,
) -> Result<u32, JsErrorBox> {
    if !crate::browser_env::is_native_abort_object(scope, object) {
        return Err(JsErrorBox::type_error("Not a native AbortSignal"));
    }
    let bridges = scope
        .get_slot::<Rc<RefCell<Bridges>>>()
        .cloned()
        .unwrap_or_else(|| {
            let value = Rc::new(RefCell::new(Bridges::default()));
            scope.set_slot(value.clone());
            value
        });
    if bridges.borrow().values.len() >= 65_536 {
        bridges.borrow().dirty.set(true);
    }
    sweep(scope);
    let mut bridges = bridges.borrow_mut();
    if bridges.values.len() >= 65_536 {
        return Err(JsErrorBox::range_error(
            "Native abort signal budget exceeded",
        ));
    }
    bridges.next = bridges
        .next
        .checked_add(1)
        .ok_or_else(|| JsErrorBox::range_error("Abort signal identity exhausted"))?;
    let id = bridges.next;
    let dirty = bridges.dirty.clone();
    let object = v8::Weak::with_finalizer(scope, object, Box::new(move |_| dirty.set(true)));
    bridges.values.insert(
        id,
        Bridge {
            object,
            callback: v8::Weak::new(scope, callback),
            raw: v8::Weak::new(scope, raw),
            listener: None,
            cleanup: v8::Weak::new(scope, cleanup),
        },
    );
    Ok(id)
}

#[op2(nofast, reentrant)]
pub(crate) fn op_web_abort_dispatch(
    scope: &mut v8::PinScope,
    id: u32,
    reason: v8::Local<v8::Value>,
) {
    let Some(bridges) = scope.get_slot::<Rc<RefCell<Bridges>>>().cloned() else {
        return;
    };
    let pair = bridges.borrow_mut().values.remove(&id);
    let Some(Bridge {
        object, callback, ..
    }) = pair
    else {
        return;
    };
    let Some(object) = object.to_local(scope) else {
        return;
    };
    let Some(callback) = callback.to_local(scope) else {
        return;
    };
    let Some(context) = object.get_creation_context(scope) else {
        return;
    };
    let scope = &mut v8::ContextScope::new(scope, context);
    callback.call(scope, object.into(), &[reason]);
}

#[op2(fast)]
pub(crate) fn op_web_abort_listener(
    scope: &mut v8::PinScope,
    id: u32,
    listener: v8::Local<v8::Function>,
) {
    if let Some(bridges) = scope.get_slot::<Rc<RefCell<Bridges>>>().cloned() {
        if let Some(bridge) = bridges.borrow_mut().values.get_mut(&id) {
            bridge.listener = Some(v8::Weak::new(scope, listener));
        }
    }
}
/// V8 finalizers only mark work. Official listener/timer cleanup runs after GC,
/// with no native strong handle to a guest object, function or context.
pub(crate) fn sweep(scope: &mut v8::PinScope) {
    let Some(bridges) = scope.get_slot::<Rc<RefCell<Bridges>>>().cloned() else {
        return;
    };
    if !bridges.borrow().dirty.replace(false) {
        return;
    }
    let dead: Vec<_> = {
        let mut bridges = bridges.borrow_mut();
        let ids: Vec<_> = bridges
            .values
            .iter()
            .filter_map(|(id, bridge)| bridge.object.to_local(scope).is_none().then_some(*id))
            .collect();
        ids.into_iter()
            .filter_map(|id| bridges.values.remove(&id))
            .collect()
    };
    for bridge in dead {
        let Some(raw) = bridge.raw.to_local(scope) else {
            continue;
        };
        let Some(listener) = bridge.listener.and_then(|weak| weak.to_local(scope)) else {
            continue;
        };
        let Some(cleanup) = bridge.cleanup.to_local(scope) else {
            continue;
        };
        let Some(context) = cleanup.get_creation_context(scope) else {
            continue;
        };
        let scope = &mut v8::ContextScope::new(scope, context);
        let receiver = v8::undefined(scope);
        cleanup.call(scope, receiver.into(), &[raw.into(), listener.into()]);
    }
}
