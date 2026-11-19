//! Explicit driver interactions await only native navigation admitted for their owner.
use super::*;

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
        tokio::time::timeout(std::time::Duration::from_millis(budget), async {
            let result = if source.is_empty() {
                String::new()
            } else {
                self.eval_target_engine(target, source, false).await?
            };
            match self.finish_target_action_since(target, &baseline).await? {
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
        .await
        .map_err(|_| format!("Target interaction timeout after {budget}ms"))?
    }
}
