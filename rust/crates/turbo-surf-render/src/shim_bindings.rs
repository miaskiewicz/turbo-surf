//! Driver binding dependencies are private and weakly owned by their guest realm.
use deno_core::{op2, v8, ExternalOpsTracker, JsRuntime};
use std::cell::Cell;
use std::rc::{Rc, Weak};

struct PendingWork {
    count: Cell<usize>,
    external: ExternalOpsTracker,
}
impl PendingWork {
    fn begin(&self) {
        self.count.set(self.count.get() + 1);
        self.external.ref_op();
    }
    fn finish(&self) {
        if self.count.get() > 0 {
            self.count.set(self.count.get() - 1);
            self.external.unref_op();
        }
    }
    fn clear(&self) {
        while self.count.get() > 0 {
            self.finish();
        }
    }
}
impl Drop for PendingWork {
    fn drop(&mut self) {
        self.clear();
    }
}
struct BindingWork(Rc<PendingWork>);
#[derive(Default)]
struct BindingRealms(Vec<Weak<PendingWork>>);

#[op2(fast)]
pub(crate) fn op_shim_binding_pending(
    scope: &mut v8::PinScope,
    document: v8::Local<v8::Object>,
    active: bool,
) {
    let Some(context) = document.get_creation_context(scope) else {
        return;
    };
    if crate::runtime::native_frame_is_retired(context) {
        return;
    }
    let work = match context.get_slot::<BindingWork>() {
        Some(work) => work.0.clone(),
        None => {
            let state = JsRuntime::op_state_from(scope);
            let mut state = state.borrow_mut();
            let work = Rc::new(PendingWork {
                count: Cell::new(0),
                external: state.external_ops_tracker.clone(),
            });
            context.set_slot(Rc::new(BindingWork(work.clone())));
            if state.try_borrow::<BindingRealms>().is_none() {
                state.put(BindingRealms::default());
            }
            state
                .borrow_mut::<BindingRealms>()
                .0
                .push(Rc::downgrade(&work));
            work
        }
    };
    if active {
        work.begin();
    } else {
        work.finish();
    }
}

pub(crate) fn pending(rt: &JsRuntime) -> bool {
    let state = rt.op_state();
    let mut state = state.borrow_mut();
    let Some(realms) = state.try_borrow_mut::<BindingRealms>() else {
        return false;
    };
    let mut pending = false;
    realms.0.retain(|realm| {
        let Some(work) = realm.upgrade() else {
            return false;
        };
        pending |= work.count.get() > 0;
        true
    });
    pending
}

pub(crate) fn retire(context: v8::Local<v8::Context>) {
    if let Some(work) = context.get_slot::<BindingWork>() {
        work.0.clear();
    }
}
