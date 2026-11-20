//! Explicit presentation policy, independent of media preferences and page clocks.
use deno_core::{v8, JsRuntime};
use serde::{Deserialize, Serialize};

/// Whether canonical CSS/Web Animations contribute presented values. Disabled
/// presentation keeps underlying styles; effect timing, events, promises, ordinary
/// timers and animation-frame callbacks keep their normal behavior.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AnimationPolicy {
    #[default]
    Enabled,
    Disabled,
}

/// Frozen when a runtime starts, so later preference updates cannot change its
/// policy or accidentally give a newly created child a different presentation.
pub(crate) struct Selected(pub(crate) AnimationPolicy);

std::thread_local! {
    // Mirrors the installed presentation for the owning realm on this thread, so
    // the surf-local layout admission can honor Disabled without a browser-env
    // getter. Set by `install`; read by `validate_or_request`. Only this thread's
    // runtime writes this cell, and layout admission for that runtime reads it
    // from the same thread.
    pub(crate) static DISABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(crate) fn presentation_disabled() -> bool {
    DISABLED.with(|cell| cell.get())
}

pub(crate) fn install(scope: &mut v8::PinScope) -> Result<(), String> {
    use turbo_dom_browser_env::host_internals::{
        set_animation_presentation, AnimationPresentation,
    };
    let policy = JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<Selected>()
        .0;
    let presentation = match policy {
        AnimationPolicy::Enabled => AnimationPresentation::Enabled,
        AnimationPolicy::Disabled => AnimationPresentation::Disabled,
    };
    DISABLED.with(|cell| cell.set(policy == AnimationPolicy::Disabled));
    let installed = set_animation_presentation(scope, presentation);
    if scope.is_execution_terminating() {
        return Err("Native animation presentation policy execution terminated".into());
    }
    if installed {
        Ok(())
    } else {
        Err("Native animation presentation policy could not be installed".into())
    }
}
