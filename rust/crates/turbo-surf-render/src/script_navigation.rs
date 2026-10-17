//! Native scalar admission snapshots; no guest objects or realms are retained.
use super::*;

struct Admission {
    target: String,
    user_activation: bool,
}

pub(super) fn record(source: v8::Local<v8::Context>, target: v8::Local<v8::Context>, url: String) {
    if source.get_slot::<FrameRetired>().is_some() || target.get_slot::<FrameRetired>().is_some() {
        return;
    }
    target.set_slot(Rc::new(Admission {
        target: url,
        user_activation: host_user_activation(source),
    }));
}

pub(super) fn snapshot(context: v8::Local<v8::Context>) -> Option<(String, bool)> {
    context
        .get_slot::<Admission>()
        .map(|state| (state.target.clone(), state.user_activation))
}
