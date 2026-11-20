//! A driver opening may finish at load without discarding its later native work.
use super::*;
use std::time::Instant;
use turbo_dom_browser_env::host_internals as binding;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InitialHydrationGoal {
    #[default]
    Quiescent,
    Load,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitialHydrationOutcome {
    Reached,
    HostReply,
    Superseded,
}
impl InitialHydrationOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reached => "reached",
            Self::HostReply => "host-reply",
            Self::Superseded => "superseded",
        }
    }
}

struct Pending {
    context: v8::Weak<v8::Context>,
    document: v8::Weak<v8::Object>,
    document_id: u64,
    deadline: Option<Instant>,
    timeout_ms: Option<u64>,
}
struct State(Option<Pending>);

pub(super) fn begin(
    rt: &mut OwnedRuntime,
    goal: InitialHydrationGoal,
    deadline: Option<Instant>,
    timeout_ms: Option<u64>,
) -> Result<(), String> {
    if goal != InitialHydrationGoal::Load {
        return Ok(());
    }
    let key = rt.activation_key();
    let _activation = key.enter();
    let context = private_infrastructure::context(rt)?;
    deno_core::scope!(scope, rt);
    let context = v8::Local::new(scope, context);
    let document = context
        .get_slot::<ActiveDocument>()
        .and_then(|active| active.0.to_local(scope))
        .ok_or("Initial navigation document is unavailable")?;
    let pending = Pending {
        context: v8::Weak::new(scope, context),
        document: v8::Weak::new(scope, document),
        document_id: binding::document_id_of(scope, document),
        deadline,
        timeout_ms,
    };
    JsRuntime::op_state_from(scope)
        .borrow_mut()
        .put(State(Some(pending)));
    Ok(())
}
pub(super) fn load_reached(goal: InitialHydrationGoal, phase: HydrationPhase) -> bool {
    goal == InitialHydrationGoal::Load && phase == HydrationPhase::PostLoadHydration
}

fn reached(rt: &JsRuntime) -> bool {
    matches!(
        rt.op_state().borrow().borrow::<HydrationProgress>().phase,
        HydrationPhase::PostLoadHydration
            | HydrationPhase::PostLoadQuiescence
            | HydrationPhase::Complete
    )
}
fn validate(rt: &mut JsRuntime, pending: &Pending) -> Result<bool, String> {
    let current = private_infrastructure::context(rt)?;
    deno_core::scope!(scope, rt);
    let current = v8::Local::new(scope, current);
    let Some(context) = pending.context.to_local(scope) else {
        return Ok(false);
    };
    let Some(document) = pending.document.to_local(scope) else {
        return Ok(false);
    };
    let active = context
        .get_slot::<ActiveDocument>()
        .and_then(|active| active.0.to_local(scope));
    if context != current
        || context.get_slot::<FrameRetired>().is_some()
        || active != Some(document)
        || document.get_creation_context(scope) != Some(context)
        || binding::document_id_of(scope, document) != pending.document_id
    {
        return Ok(false);
    }
    Ok(true)
}
impl PageSession {
    /// Continue only the authenticated initial document, never a later replacement.
    pub async fn continue_initial_hydration(&mut self) -> Result<InitialHydrationOutcome, String> {
        if self.closed {
            return Err("Page session is closed".into());
        }
        let pending = self
            .rt
            .op_state()
            .borrow_mut()
            .try_borrow_mut::<State>()
            .and_then(|state| state.0.take())
            .ok_or("Initial navigation continuation is unavailable")?;
        let key = self.rt.activation_key();
        key.during(self.continue_initial_hydration_active(pending))
            .await
    }
    async fn continue_initial_hydration_active(
        &mut self,
        pending: Pending,
    ) -> Result<InitialHydrationOutcome, String> {
        if !validate(&mut self.rt, &pending)? {
            return Ok(InitialHydrationOutcome::Superseded);
        }
        if reached(&self.rt) {
            return Ok(InitialHydrationOutcome::Reached);
        }
        let remaining = pending
            .deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()));
        if remaining.is_some_and(|time| time.is_zero()) {
            return Err(format!(
                "navigation timeout after {}ms",
                pending.timeout_ms.unwrap_or(self.budget_ms)
            ));
        }
        if crate::shim_bindings::pending(&self.rt) {
            self.rt.op_state().borrow_mut().borrow_mut::<State>().0 = Some(pending);
            return Ok(InitialHydrationOutcome::HostReply);
        }
        let budget = remaining.map_or(self.budget_ms, |time| {
            self.budget_ms.min(time.as_millis().max(1) as u64)
        });
        let execution = AsyncEntryBudget::new(self.rt.v8_isolate(), budget, pending.deadline);
        let base = self.rt.op_state().borrow().borrow::<Base>().0.clone();
        let result = execution
            .run(
                true,
                resume_host_hydration_until(&mut self.rt, &base, InitialHydrationGoal::Load),
            )
            .await;
        let outcome = execution.finish();
        self.rt.v8_isolate().cancel_terminate_execution();
        auxiliary_navigation::retire_interrupted_frames(&mut self.rt);
        if outcome.interrupted {
            return Err(outcome.error(String::new()));
        }
        if !validate(&mut self.rt, &pending)? {
            return Ok(InitialHydrationOutcome::Superseded);
        }
        if outcome.exceeded {
            return Err(format!(
                "navigation timeout after {}ms",
                pending.timeout_ms.unwrap_or(self.budget_ms)
            ));
        }
        result?;
        if reached(&self.rt) {
            return Ok(InitialHydrationOutcome::Reached);
        }
        if !crate::shim_bindings::pending(&self.rt) {
            return Err("Initial navigation stopped before its load goal".into());
        }
        self.rt.op_state().borrow_mut().borrow_mut::<State>().0 = Some(pending);
        Ok(InitialHydrationOutcome::HostReply)
    }
}
