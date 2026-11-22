//! Explicit driver interactions await only native navigation admitted for their owner.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct ActionNavigation {
    target: u64,
    network: u64,
    history: Option<u64>,
}
impl ActionNavigation {
    fn capture(rt: &JsRuntime, target: u64, network: u64) -> Self {
        Self {
            target,
            network,
            history: native_history::current_revision(&rt.op_state().borrow(), target),
        }
    }
    pub(super) fn admitted(self, rt: &JsRuntime) -> Result<bool, String> {
        let state = rt.op_state();
        let state = state.borrow();
        let network = state
            .borrow::<NetworkLog>()
            .current_navigation_revision(self.target)
            .ok_or("Native navigation tracking is invalid")?;
        let history = self
            .history
            .zip(native_history::current_revision(&state, self.target))
            .is_some_and(|(baseline, current)| current > baseline);
        Ok(network > self.network || history)
    }
}

struct CanceledAction<'a> {
    session: &'a mut PageSession,
    completed: bool,
}
impl Drop for CanceledAction<'_> {
    fn drop(&mut self) {
        if !self.completed {
            auxiliary_navigation::finish_canceled_execution(&mut self.session.rt);
        }
    }
}

impl PageSession {
    /// Finish only main-frame signals admitted after the driver's whole action began.
    /// This method executes no guest source and never waits for resource loading.
    pub(super) async fn finish_target_action_since(
        &mut self,
        target: u64,
        baseline: &str,
    ) -> Result<auxiliary_navigation::ActionCompletion, String> {
        let baseline: u64 = baseline
            .parse()
            .map_err(|_| "Invalid native action baseline")?;
        let latest = self
            .rt
            .op_state()
            .borrow()
            .borrow::<NetworkLog>()
            .current_navigation_revision(target)
            .ok_or("Native navigation tracking is invalid")?;
        if latest <= baseline {
            return Ok(auxiliary_navigation::ActionCompletion::Released);
        }
        let form = form_submission::take_for_destination_since(
            &mut self.rt.op_state().borrow_mut(),
            target,
            latest - 1,
        );
        if let Some(form) = form {
            if let auxiliary_navigation::ActionCompletion::NavigationFailed(error) =
                self.finish_action_form(target, form).await?
            {
                self.publish_auxiliary_state();
                return Ok(auxiliary_navigation::ActionCompletion::NavigationFailed(
                    error,
                ));
            }
        }
        let outcome = self
            .finish_target_navigation_since(target, baseline)
            .await?;
        self.publish_auxiliary_state();
        Ok(outcome)
    }

    /// Dispatch in the actual guest, retaining its result across document retirement.
    /// Passive reads, collectors and ordinary evaluate do not use this completion path.
    pub async fn dispatch_target_engine(
        &mut self,
        target: u64,
        source: &str,
    ) -> Result<String, String> {
        self.dispatch_target_engine_since(target, source, None)
            .await
    }

    pub async fn dispatch_target_engine_since(
        &mut self,
        target: u64,
        source: &str,
        baseline: Option<&str>,
    ) -> Result<String, String> {
        let signal_only = baseline.is_some();
        if signal_only && !source.is_empty() {
            return Err("Native signal completion requires an empty driver source".into());
        }
        let budget = self.budget_ms;
        let snapshot = self
            .rt
            .op_state()
            .borrow()
            .borrow::<NetworkLog>()
            .target_network_state(&target.to_string());
        if let Some(error) = snapshot["error"].as_str() {
            return Err(error.into());
        }
        let baseline = baseline
            .or_else(|| snapshot["navigationRevision"].as_str())
            .ok_or("Native navigation baseline is unavailable")?
            .to_owned();
        let baseline_revision = baseline
            .parse()
            .map_err(|_| "Invalid native action baseline")?;
        let navigation = ActionNavigation::capture(&self.rt, target, baseline_revision);
        let mut action = CanceledAction {
            session: self,
            completed: false,
        };
        let outcome = tokio::time::timeout(std::time::Duration::from_millis(budget), async {
            let result = if source.is_empty() {
                String::new()
            } else {
                action
                    .session
                    .eval_target_action_engine(target, source, navigation)
                    .await?
            };
            match action
                .session
                .finish_target_action_since(target, &baseline)
                .await?
            {
                auxiliary_navigation::ActionCompletion::Released if signal_only => {
                    Ok(serde_json::json!({"status":"released"}).to_string())
                }
                auxiliary_navigation::ActionCompletion::Released => Ok(result),
                auxiliary_navigation::ActionCompletion::NavigationFailed(error) if signal_only => {
                    Ok(serde_json::json!({"status":"navigation-failed","error":error}).to_string())
                }
                auxiliary_navigation::ActionCompletion::NavigationFailed(error) => Err(error),
            }
        })
        .await;
        action.completed = outcome.is_ok();
        outcome.map_err(|_| format!("Target interaction timeout after {budget}ms"))?
    }
}
