//! Document task selection follows the same native Promise dependency closure
//! as the async-op wait predicate. A peer's refed op is not a Root deadline.
use super::super::*;

fn selected(context: &v8::Global<v8::Context>, contexts: &[v8::Global<v8::Context>]) -> bool {
    contexts.iter().any(|candidate| candidate == context)
}

fn run_queued_task(rt: &mut JsRuntime, selection: &async_owner::Selection) -> Result<bool, String> {
    if selected_document_tasks::run_selected_tasks(rt, &selection.targets)? {
        return Ok(true);
    }
    let message = {
        let state = rt.op_state();
        let mut state = state.borrow_mut();
        let queue = &mut state.borrow_mut::<FrameContexts>().messages;
        queue
            .iter()
            .position(|job| selected(&job.target, &selection.contexts))
            .and_then(|index| queue.remove(index))
    };
    if let Some(message) = message {
        deliver_window_message(rt, message)?;
        return Ok(true);
    }
    let job = {
        let state = rt.op_state();
        let mut state = state.borrow_mut();
        let queue = &mut state.borrow_mut::<ClassicScripts>().jobs;
        queue
            .iter()
            .position(|job| selected(&job.context, &selection.contexts))
            .and_then(|index| queue.remove(index))
    };
    if let Some(job) = job {
        run_pending_classic_script(rt, job)?;
        return Ok(true);
    }
    Ok(false)
}

fn queued(rt: &JsRuntime, selection: &async_owner::Selection) -> bool {
    if selected_document_tasks::has_selected_tasks(rt, &selection.targets) {
        return true;
    }
    let state = rt.op_state();
    let state = state.borrow();
    state
        .borrow::<FrameContexts>()
        .messages
        .iter()
        .any(|job| selected(&job.target, &selection.contexts))
        || state
            .borrow::<ClassicScripts>()
            .jobs
            .iter()
            .any(|job| selected(&job.context, &selection.contexts))
}

fn document_pending(
    rt: &mut JsRuntime,
    contexts: &[v8::Global<v8::Context>],
) -> Result<bool, String> {
    deno_core::scope!(scope, rt);
    for context in contexts {
        let context = v8::Local::new(scope, context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let pending = async_owner::with_document(scope, context, |scope| {
            call_frame_helper(scope, "pendingWork", &[])
                .map(|value| value.to_rust_string_lossy(scope) == "1")
        })?
        .ok_or("Selected async document work predicate is unavailable")?;
        if pending {
            return Ok(true);
        }
    }
    Ok(false)
}

enum Turn {
    Again,
    Timer(u32),
    Complete,
}

#[derive(Clone, Copy)]
enum Goal {
    Complete,
    Quiescence { timers: bool },
}

struct DrainOptions<'a> {
    condition: Option<&'a str>,
    completion: Option<&'a v8::Global<v8::Value>>,
    goal: Goal,
    real: bool,
}

#[derive(Default)]
pub(in crate::runtime) struct QuiescenceTurn {
    pub fired: u32,
    pub busy: bool,
}

pub(in crate::runtime) async fn quiescence_turn(
    rt: &mut JsRuntime,
    root: u64,
    timers: bool,
) -> Result<QuiescenceTurn, String> {
    drain_selected(rt, root, None, None, Goal::Quiescence { timers }).await
}

/// Poll the shared VM once, then decide whether to wait using native selected
/// work. An agent-wide checkpoint may execute microtasks in other documents;
/// that does not admit their native document tasks or wait for their IO.
pub(in crate::runtime) async fn drain(
    rt: &mut JsRuntime,
    root: u64,
    condition: Option<&str>,
    completion: Option<&v8::Global<v8::Value>>,
) -> Result<(), String> {
    drain_selected(rt, root, condition, completion, Goal::Complete)
        .await
        .map(|_| ())
}

async fn drain_selected(
    rt: &mut JsRuntime,
    root: u64,
    condition: Option<&str>,
    completion: Option<&v8::Global<v8::Value>>,
    goal: Goal,
) -> Result<QuiescenceTurn, String> {
    let real = rt.op_state().borrow().borrow::<TimerClock>().mode == WindowTimerMode::RealTime;
    let options = DrainOptions {
        condition,
        completion,
        goal,
        real,
    };
    let mut wake = None;
    checkpoint_window_timer(rt)?;
    loop {
        if completion.is_some_and(|promise| promise_settled(rt, promise)) {
            return Ok(QuiescenceTurn::default());
        }
        let selection = async_owner::selection(rt, root)?;
        let ran = run_queued_task(rt, &selection)?;
        drop(selection);
        if ran {
            tokio::task::yield_now().await;
            continue;
        }
        let turn = std::future::poll_fn(|cx| {
            let polled = rt.poll_event_loop(cx, deno_core::PollEventLoopOptions::default());
            if let std::task::Poll::Ready(Err(error)) = polled {
                return std::task::Poll::Ready(Err(error.to_string()));
            }
            if completion.is_some_and(|promise| promise_settled(rt, promise)) {
                return std::task::Poll::Ready(Ok(Turn::Complete));
            }
            match poll_selected_turn(rt, root, &options, cx, &mut wake) {
                Ok(turn) => turn.map(Ok),
                Err(error) => std::task::Poll::Ready(Err(error)),
            }
        })
        .await?;
        match turn {
            Turn::Complete if matches!(goal, Goal::Complete) => {
                return Ok(QuiescenceTurn::default());
            }
            Turn::Complete => return quiescence_result(rt, root, 0),
            Turn::Timer(fired) if matches!(goal, Goal::Quiescence { .. }) => {
                return quiescence_result(rt, root, fired);
            }
            _ => {}
        }
        // A ready native queue must not starve the existing outer deadline.
        tokio::task::yield_now().await;
    }
}

fn poll_selected_turn(
    rt: &mut JsRuntime,
    root: u64,
    options: &DrainOptions<'_>,
    cx: &mut std::task::Context<'_>,
    wake: &mut Option<(tokio::time::Instant, std::pin::Pin<Box<tokio::time::Sleep>>)>,
) -> Result<std::task::Poll<Turn>, String> {
    let selection = async_owner::selection(rt, root)?;
    if queued(rt, &selection) {
        return Ok(std::task::Poll::Ready(Turn::Again));
    }
    if condition_complete(rt, root, options.condition)? {
        return Ok(std::task::Poll::Ready(Turn::Complete));
    }
    let busy = selected_busy(&selection);
    let allow_advance = !options.real && !busy;
    let timer = if matches!(options.goal, Goal::Quiescence { timers: false }) {
        WindowTimerTurn::default()
    } else {
        run_next_window_timer_in(rt, allow_advance, selection.contexts.clone())
    };
    if timer.fired != 0 {
        checkpoint_window_timer(rt)?;
        return Ok(std::task::Poll::Ready(Turn::Timer(timer.fired)));
    }
    if run_background_resource_task_in(rt, selection.contexts.clone()) != 0 {
        return Ok(std::task::Poll::Ready(Turn::Again));
    }
    drop(selection);
    finish_selected_turn(rt, root, options, cx, wake, timer.deadline)
}

fn finish_selected_turn(
    rt: &mut JsRuntime,
    root: u64,
    options: &DrainOptions<'_>,
    cx: &mut std::task::Context<'_>,
    wake: &mut Option<(tokio::time::Instant, std::pin::Pin<Box<tokio::time::Sleep>>)>,
    timer_deadline: Option<tokio::time::Instant>,
) -> Result<std::task::Poll<Turn>, String> {
    // Trusted task turns may create new native work or record a failed owner
    // admission. Never decide quiescence from the pre-dispatch graph snapshot.
    let selection = async_owner::selection(rt, root)?;
    if queued(rt, &selection) {
        return Ok(std::task::Poll::Ready(Turn::Again));
    }
    let pending = document_pending(rt, &selection.contexts)?;
    let busy = selected_busy(&selection);
    drop(selection);
    if matches!(options.goal, Goal::Quiescence { timers: false })
        || (matches!(options.goal, Goal::Quiescence { .. }) && !busy)
    {
        return Ok(std::task::Poll::Ready(Turn::Complete));
    }
    if keep_pending(
        busy,
        pending,
        options.completion.is_some(),
        options.condition,
    ) {
        if park_on_window_timer_deadline(cx, wake, timer_deadline).is_some() {
            return Ok(std::task::Poll::Ready(Turn::Again));
        }
        return Ok(std::task::Poll::Pending);
    }
    Ok(std::task::Poll::Ready(Turn::Complete))
}

fn quiescence_result(rt: &mut JsRuntime, root: u64, fired: u32) -> Result<QuiescenceTurn, String> {
    let selection = async_owner::selection(rt, root)?;
    Ok(QuiescenceTurn {
        fired,
        busy: selected_busy(&selection) || queued(rt, &selection),
    })
}

fn condition_complete(
    rt: &mut JsRuntime,
    root: u64,
    condition: Option<&str>,
) -> Result<bool, String> {
    let Some(name) = condition else {
        return Ok(false);
    };
    deno_core::scope!(scope, rt);
    let context = auxiliary_pages::context(
        scope,
        turbo_dom_browser_env::host_internals::AuxiliaryContextId(root),
    )?;
    let scope = &mut v8::ContextScope::new(scope, context);
    async_owner::with_document(scope, context, |scope| {
        call_frame_helper(scope, name, &[]).map(|value| value.to_rust_string_lossy(scope) == "true")
    })?
    .ok_or_else(|| "Selected document completion predicate is unavailable".into())
}

fn selected_busy(selection: &async_owner::Selection) -> bool {
    selection.pending_ops != 0 || selection.runnable || selection.modules
}

fn keep_pending(busy: bool, document: bool, completion: bool, condition: Option<&str>) -> bool {
    busy || document || completion || condition.is_some()
}
