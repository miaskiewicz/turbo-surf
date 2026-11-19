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
    budget_ms: u64,
}
#[derive(Clone, Copy)]
pub(super) struct EntryOutcome {
    pub interrupted: bool,
    pub exceeded: bool,
}
impl AsyncEntryBudget {
    pub(super) fn new(isolate: &mut v8::Isolate, budget_ms: u64, caller: Option<Instant>) -> Self {
        Self {
            execution: PollExecutionBudget::new(isolate, budget_ms),
            caller: caller.map(|deadline| Watchdog::until(isolate, deadline)),
            caller_deadline: caller,
            aggregate_exceeded: AtomicBool::new(false),
            budget_ms,
        }
    }
    pub(super) async fn run<T>(
        &self,
        enforce_deadline: bool,
        future: impl std::future::Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
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
        with_async_execution_budget(
            enforce_deadline || self.caller.is_some(),
            budget,
            &self.aggregate_exceeded,
            self.execution.run(future),
        )
        .await
    }
    pub(super) fn finish(self) -> EntryOutcome {
        let interrupted = self.execution.interrupted();
        let exceeded = self.execution.expired() || self.aggregate_exceeded.load(Ordering::Relaxed);
        let caller: Option<Arc<AtomicBool>> =
            self.caller.as_ref().map(|guard| guard.exceeded.clone());
        let caller_expired = self
            .caller_deadline
            .is_some_and(|deadline| Instant::now() >= deadline);
        // Disarm/join both workers before observing the final caller request or clearing V8.
        drop(self);
        let interrupted = interrupted
            || caller_expired
            || caller.is_some_and(|flag| flag.load(Ordering::Relaxed));
        EntryOutcome {
            interrupted,
            exceeded: exceeded || interrupted,
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
