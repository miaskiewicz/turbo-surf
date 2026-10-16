//! Schedule bounded native storage reclamation after an actual V8 collection.
use deno_core::v8;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::Rc;
use turbo_dom_parser::rtdom::tree::CollectionBudget;

const BUDGET: CollectionBudget = CollectionBudget {
    max_nodes: 250_000,
    max_edges: 2_000_000,
    max_payload_bytes: 64 * 1024 * 1024,
};
const MAX_DOCUMENTS: usize = 256;

#[derive(Default)]
struct State {
    context: Option<v8::Weak<v8::Context>>,
    pending: Cell<bool>,
    collections: Cell<usize>,
    reclaimed: Cell<usize>,
    rejected: Cell<usize>,
    last: RefCell<serde_json::Value>,
}

unsafe extern "C" fn collected(
    _: v8::UnsafeRawIsolatePtr,
    _: v8::GCType,
    _: v8::GCCallbackFlags,
    data: *mut c_void,
) {
    // The isolate-owned Rc allocation stays fixed until unregister. Never
    // localize handles or traverse native Trees from within V8's GC callback.
    unsafe { &*data.cast::<State>() }.pending.set(true);
}

pub(crate) fn install(scope: &mut v8::PinScope) {
    let context = scope.get_current_context();
    let state = Rc::new(State {
        context: Some(v8::Weak::new(scope, context)),
        ..State::default()
    });
    let data = Rc::as_ptr(&state).cast_mut().cast();
    scope.set_slot(state);
    scope.add_gc_epilogue_callback(collected, data, v8::GCType::kGCTypeAll);
}

pub(crate) fn uninstall(isolate: &mut v8::Isolate) {
    if let Some(state) = isolate.remove_slot::<Rc<State>>() {
        isolate.remove_gc_epilogue_callback(collected, Rc::as_ptr(&state).cast_mut().cast());
    }
}

/// Called with an entered isolate after all guest scopes for a host operation
/// have ended. Weak lookups retain any V8-kept or pending-job-owned wrappers.
pub(crate) fn boundary(isolate: &mut v8::Isolate) {
    let Some(state) = isolate.get_slot::<Rc<State>>().cloned() else {
        return;
    };
    if isolate.is_execution_terminating() || !state.pending.replace(false) {
        return;
    }
    let scope = std::pin::pin!(v8::HandleScope::new(isolate));
    let scope = &mut scope.init();
    let Some(context) = state.context.as_ref().and_then(|weak| weak.to_local(scope)) else {
        return;
    };
    let scope = &mut v8::ContextScope::new(scope, context);
    crate::web_streams_backend::sweep(scope);
    crate::runtime::fetch_streams::sweep(scope);
    match crate::browser_env::collect_native_storage(scope, BUDGET, MAX_DOCUMENTS) {
        Ok(report) => {
            let reclaimed = report
                .documents
                .iter()
                .map(|(_, row)| row.reclaimed_nodes)
                .sum::<usize>();
            state
                .collections
                .set(state.collections.get().saturating_add(1));
            state
                .reclaimed
                .set(state.reclaimed.get().saturating_add(reclaimed));
            *state.last.borrow_mut() = serde_json::json!({
                "documents": report.documents.iter().map(|(owner, row)| serde_json::json!({
                    "owner":owner,"reclaimed":row.reclaimed_nodes,
                    "beforeNodes":row.before.live_nodes,"afterNodes":row.after.live_nodes,
                    "beforePayloadBytes":row.before.payload_bytes,"afterPayloadBytes":row.after.payload_bytes
                })).collect::<Vec<_>>()
            });
        }
        Err(error) => {
            // Budget rejection preserves that Tree's storage and page behavior.
            // Retry after another actual GC rather than rescanning every poll.
            state.rejected.set(state.rejected.get().saturating_add(1));
            *state.last.borrow_mut() = serde_json::json!({"error":format!("{error:?}")});
        }
    }
}

pub(crate) fn diagnostics(isolate: &v8::Isolate) -> serde_json::Value {
    let Some(state) = isolate.get_slot::<Rc<State>>() else {
        return serde_json::Value::Null;
    };
    serde_json::json!({"pending":state.pending.get(),"collections":state.collections.get(),
        "reclaimedNodes":state.reclaimed.get(),"rejected":state.rejected.get(),"last":*state.last.borrow()})
}
