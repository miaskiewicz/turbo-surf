//! IndexedDB's native event hook (indexeddb.js).
use deno_core::{op2, v8};

/// Fires an engine event at an IndexedDB target through the binding's host dispatch and
/// returns whether a listener it invoked threw. The binding has already reported the
/// exception; the request's transaction then aborts with "Uncaught exception in event
/// handler." like Chrome 156 (#148). Listeners of events dispatched from inside the
/// listeners do not count.
#[op2(nofast, reentrant)]
pub(crate) fn op_idb_dispatch(
    scope: &mut v8::PinScope,
    event: v8::Local<v8::Object>,
    target: v8::Local<v8::Object>,
) -> bool {
    crate::browser_env::dispatch_host_event_did_listeners_throw(scope, event, target)
}
