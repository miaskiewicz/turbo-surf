//! Keep a caller's hard deadline distinct from an internal aggregate drain cap.
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;

pub(super) struct AsyncEntryBudget {
    execution: PollExecutionBudget,
    caller: Option<Watchdog>,
    caller_deadline: Option<Instant>,
    aggregate_exceeded: AtomicBool,
    nested_interrupted: AtomicBool,
    budget_ms: u64,
}
#[derive(Clone, Copy)]
pub(super) struct EntryOutcome {
    pub interrupted: bool,
    pub exceeded: bool,
    pub caller_expired: bool,
}
impl AsyncEntryBudget {
    pub(super) fn new(isolate: &mut v8::Isolate, budget_ms: u64, caller: Option<Instant>) -> Self {
        Self {
            execution: PollExecutionBudget::new(isolate, budget_ms),
            caller: caller.map(|deadline| Watchdog::until(isolate, deadline)),
            caller_deadline: caller,
            aggregate_exceeded: AtomicBool::new(false),
            nested_interrupted: AtomicBool::new(false),
            budget_ms,
        }
    }
    // Finish constructing the drain state before polling it. Keeping it inline
    // inflates every forwarding constructor's frame during cold V8 bootstrap.
    pub(super) fn run<'a, T: 'a>(
        &'a self,
        enforce_deadline: bool,
        future: impl std::future::Future<Output = Result<T, String>> + 'a,
    ) -> std::pin::Pin<Box<impl std::future::Future<Output = Result<T, String>> + 'a>> {
        Box::pin(async move {
            if self
                .caller_deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
            {
                return Err("Execution interrupted by the navigation caller deadline".into());
            }
            let budget = self.caller_deadline.map_or(self.budget_ms, |deadline| {
                self.budget_ms.min(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_millis()
                        .max(1) as u64,
                )
            });
            let result = with_async_execution_budget(
                enforce_deadline || self.caller.is_some(),
                budget,
                &self.aggregate_exceeded,
                self.execution.run(future),
            )
            .await;
            // An inner guard can win the stop before this poll's supervisor.
            if hard_interruption(&result) {
                self.nested_interrupted.store(true, Ordering::Relaxed);
            }
            result
        })
    }
    pub(super) fn interrupted(&self) -> bool {
        self.execution.interrupted() || self.nested_interrupted.load(Ordering::Relaxed)
    }
    pub(super) fn finish(self) -> EntryOutcome {
        let interrupted = self.interrupted();
        let exceeded = self.execution.expired() || self.aggregate_exceeded.load(Ordering::Relaxed);
        let caller: Option<Arc<AtomicBool>> =
            self.caller.as_ref().map(|guard| guard.exceeded.clone());
        let caller_expired = self
            .caller_deadline
            .is_some_and(|deadline| Instant::now() >= deadline);
        // Disarm/join both workers before observing the final caller request or clearing V8.
        drop(self);
        let caller_expired =
            caller_expired || caller.is_some_and(|flag| flag.load(Ordering::Relaxed));
        let interrupted = interrupted || caller_expired;
        EntryOutcome {
            interrupted,
            exceeded: exceeded || interrupted,
            caller_expired,
        }
    }
}

impl EntryOutcome {
    pub(super) fn error(self, fallback: String) -> String {
        if self.interrupted {
            "Execution interrupted by the session execution budget".into()
        } else {
            fallback
        }
    }
}

fn hard_interruption<T>(result: &Result<T, String>) -> bool {
    result
        .as_ref()
        .is_err_and(|error| error == "Execution interrupted by the session execution budget")
}
#[cfg(test)]
mod error_priority_tests {
    use super::*;
    #[test]
    fn nested_hard_error_is_distinct_from_an_ordinary_error_or_reached_result() {
        assert!(hard_interruption::<()>(&Err(
            "Execution interrupted by the session execution budget".into()
        )));
        assert!(!hard_interruption::<()>(&Err("ordinary failure".into())));
        assert!(!hard_interruption(&Ok(())));
    }
}
