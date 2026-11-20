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
    // Reentrant-safe stack of the currently-executing realm's presentation flag.
    // Each `install` call pushes onto this stack and the matching `PolicyGuard`
    // pops it on drop, so a layout call reads the policy of the realm whose
    // `install` runs it, never a sibling realm's policy that happened to land
    // on the same tokio worker thread. The old write-once-clobber approach
    // cross-contaminated realms sharing a thread.
    static DISABLED_STACK: std::cell::RefCell<Vec<bool>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
pub(crate) fn push_disabled_for_tests(disabled: bool) -> PolicyGuard {
    DISABLED_STACK.with(|stack| stack.borrow_mut().push(disabled));
    PolicyGuard(())
}

pub(crate) fn presentation_disabled() -> bool {
    DISABLED_STACK.with(|stack| stack.borrow().last().copied().unwrap_or(false))
}

/// Pops the installed presentation flag when the owning realm's install scope
/// unwinds. Held by the runtime through the `Selected` opstate carrier below.
pub(crate) struct PolicyGuard(());
impl Drop for PolicyGuard {
    fn drop(&mut self) {
        // The thread's TLS destructor may have already run by the time a
        // runtime tears down at test or process exit, in which case `with`
        // panics on reentry. `try_with` turns that into a benign Err — the
        // stack is going away anyway, so dropping without popping is sound.
        let _ = DISABLED_STACK.try_with(|stack| {
            let _ = stack.borrow_mut().pop();
        });
    }
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
    let guard = PolicyGuard(());
    DISABLED_STACK.with(|stack| stack.borrow_mut().push(policy == AnimationPolicy::Disabled));
    // Hand the guard to the runtime so the policy stays active for the realm's
    // lifetime and pops when the opstate is torn down. Dropping the guard on
    // the install-scope exit alone would unwind the policy before any layout
    // call ran under it.
    JsRuntime::op_state_from(scope)
        .borrow_mut()
        .put::<PolicyGuard>(guard);
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
