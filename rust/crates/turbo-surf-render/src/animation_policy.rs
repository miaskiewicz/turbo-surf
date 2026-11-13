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
