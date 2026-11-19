//! Interruptible native actor turns; waiting never advances a virtual clock.
use super::*;
use std::task::{Context, Poll};

#[derive(Default)]
struct BackgroundTurn {
    pending_io: bool,
    timer: Option<(tokio::time::Instant, std::pin::Pin<Box<tokio::time::Sleep>>)>,
}

pub(super) fn ready_tasks(rt: &mut JsRuntime) -> Result<bool, String> {
    ready_tasks_for(rt, None)
}

pub(super) fn ready_tasks_for(rt: &mut JsRuntime, owner: Option<u64>) -> Result<bool, String> {
    if indexeddb_store::run_task_for(rt, owner)? {
        return Ok(true);
    }
    if native_history::run_task_for(rt, owner)? {
        return Ok(true);
    }
    #[cfg(turbo_surf_native_proxy)]
    if drive_pending_module_stage(rt)? {
        return Ok(true);
    }
    let message = rt
        .op_state()
        .borrow_mut()
        .borrow_mut::<FrameContexts>()
        .messages
        .pop_front();
    if let Some(message) = message {
        deliver_window_message(rt, message)?;
        return Ok(true);
    }
    let job = rt
        .op_state()
        .borrow_mut()
        .borrow_mut::<ClassicScripts>()
        .jobs
        .pop_front();
    if let Some(job) = job {
        run_pending_classic_script(rt, job)?;
        return Ok(true);
    }
    Ok(false)
}

fn task_result(
    outcome: Option<Poll<Option<Result<(), String>>>>,
) -> Option<Poll<Result<(), String>>> {
    match outcome {
        Some(Poll::Ready(Some(Err(error)))) => Some(Poll::Ready(Err(error))),
        Some(Poll::Ready(_)) => Some(Poll::Ready(Ok(()))),
        _ => None,
    }
}

impl BackgroundTurn {
    fn park_timer(
        &mut self,
        cx: &mut Context<'_>,
        deadline: Option<tokio::time::Instant>,
    ) -> Option<Poll<Result<(), String>>> {
        // A ready sleep has only scheduled another poll; no callback has run yet.
        park_on_window_timer_deadline(cx, &mut self.timer, deadline).map(|_| Poll::Pending)
    }

    fn poll(&mut self, rt: &mut JsRuntime, cx: &mut Context<'_>) -> Poll<Result<(), String>> {
        rt.v8_isolate().perform_microtask_checkpoint();
        if ready_tasks(rt)? {
            return Poll::Ready(Ok(()));
        }
        let event = rt.poll_event_loop(cx, Default::default());
        if let Poll::Ready(Err(error)) = &event {
            return Poll::Ready(Err(error.to_string()));
        }
        // Retirement is synchronous actor work; a matching journal entry needs
        // another turn before this poll may park, without retaining its Waker.
        if indexeddb_store::has_tasks_for(rt, None) {
            return Poll::Ready(Ok(()));
        }
        if let Some(result) = task_result(drain_pending_storage_event(rt)) {
            return result;
        }
        if let Some(result) = task_result(drain_next_background_resource_task(rt)) {
            return result;
        }
        if rt.op_state().borrow().borrow::<TimerClock>().mode == WindowTimerMode::RealTime {
            let mut budget = 1;
            match run_due_window_timers(rt, &mut budget, false) {
                WindowTimerStep::Fired(outcome) => {
                    return task_result(Some(outcome)).unwrap_or(Poll::Ready(Ok(())));
                }
                WindowTimerStep::Idle(deadline) => {
                    if let Some(result) = self.park_timer(cx, deadline) {
                        return result;
                    }
                }
            }
        }
        let finished_io = self.pending_io && event.is_ready();
        self.pending_io = event.is_pending();
        if finished_io {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}

impl PageSession {
    /// Wait for one actual native task, without awaiting unrelated page quiescence.
    /// Dropping the future releases its borrow so another target or Close can run.
    pub async fn poll_background_work(&mut self) -> Result<String, String> {
        if self.closed {
            return Err("Page session is closed".into());
        }
        self.admit_actor_form_navigations()?;
        if self.has_pending_auxiliary_work() {
            return self
                .service_auxiliary_navigation()
                .await
                .map(|_| String::new());
        }
        let signal = self.storage_event_signal();
        let key = self.rt.activation_key();
        let budget = self.budget_ms;
        let mut turn = BackgroundTurn::default();
        let work = std::future::poll_fn(|cx| {
            let watchdog = Watchdog::start(self.rt.v8_isolate(), budget);
            let result = turn.poll(&mut self.rt, cx);
            let exceeded = watchdog.exceeded.load(std::sync::atomic::Ordering::Relaxed);
            drop(watchdog);
            self.rt.v8_isolate().cancel_terminate_execution();
            auxiliary_navigation::retire_interrupted_frames(&mut self.rt);
            let result = if exceeded {
                Poll::Ready(Err(
                    "Native background task exceeded execution budget".into()
                ))
            } else {
                result
            };
            match result {
                Poll::Ready(result) => Poll::Ready(self.finish_history_command(result)),
                Poll::Pending => Poll::Pending,
            }
        });
        key.during(async {
            tokio::select! {
                result = work => result,
                () = signal => Ok(()),
            }
        })
        .await?;
        self.publish_auxiliary_state();
        Ok(String::new())
    }
}

#[cfg(test)]
#[path = "background_work/message_contracts.rs"]
mod message_contracts;

#[cfg(test)]
mod timer_wake_contracts {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::task::{Wake, Waker};

    #[derive(Default)]
    struct WakeCount(AtomicUsize);

    impl Wake for WakeCount {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[tokio::test]
    async fn expired_timer_deadline_requests_a_repoll_without_completing_background_work() {
        let count = Arc::new(WakeCount::default());
        let waker = Waker::from(count.clone());
        let mut cx = Context::from_waker(&waker);
        let deadline = tokio::time::Instant::now();
        let mut sleep = Box::pin(tokio::time::sleep_until(deadline));
        sleep.as_mut().await;
        let mut turn = BackgroundTurn {
            timer: Some((deadline, sleep)),
            ..Default::default()
        };
        let outcome = turn.park_timer(&mut cx, Some(deadline));
        assert!(
            count.0.load(Ordering::Relaxed) > 0,
            "the real ready sleep must request another poll"
        );
        assert!(
            matches!(outcome, Some(Poll::Pending)),
            "a deadline wake is not an executed background task: {outcome:?}"
        );
        assert!(turn.park_timer(&mut cx, None).is_none());
        assert!(
            turn.timer.is_none(),
            "removing the deadline releases its sleep"
        );
    }
}
