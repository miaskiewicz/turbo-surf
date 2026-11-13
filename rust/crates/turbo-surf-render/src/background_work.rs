//! Interruptible native actor turns; waiting never advances a virtual clock.
use super::*;
use std::task::{Context, Poll};

#[derive(Default)]
struct BackgroundTurn {
    pending_io: bool,
    timer: Option<(tokio::time::Instant, std::pin::Pin<Box<tokio::time::Sleep>>)>,
}

pub(super) fn ready_tasks(rt: &mut JsRuntime) -> Result<bool, String> {
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
    fn poll(&mut self, rt: &mut JsRuntime, cx: &mut Context<'_>) -> Poll<Result<(), String>> {
        rt.v8_isolate().perform_microtask_checkpoint();
        if ready_tasks(rt)? {
            return Poll::Ready(Ok(()));
        }
        let event = rt.poll_event_loop(cx, Default::default());
        if let Poll::Ready(Err(error)) = &event {
            return Poll::Ready(Err(error.to_string()));
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
                    if park_on_window_timer_deadline(cx, &mut self.timer, deadline).is_some() {
                        return Poll::Ready(Ok(()));
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
            let watchdog = Watchdog::start(self.rt.v8_isolate().thread_safe_handle(), budget);
            let result = turn.poll(&mut self.rt, cx);
            let exceeded = watchdog.exceeded.load(std::sync::atomic::Ordering::Relaxed);
            drop(watchdog);
            self.rt.v8_isolate().cancel_terminate_execution();
            if exceeded {
                Poll::Ready(Err(
                    "Native background task exceeded execution budget".into()
                ))
            } else {
                result
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
