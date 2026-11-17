//! Native storage delivery is selected by the receiving live Page, never its writer.
use super::*;

type State = Rc<RefCell<OpState>>;
type Candidate = (usize, crate::storage_inbox::Notice, Vec<Target>, bool);

fn receives<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    targets: &[u64],
) -> Result<bool, String> {
    if context.get_slot::<FrameRetired>().is_some() {
        return Ok(false);
    }
    selected_document_tasks::context_target(scope, context).map(|target| targets.contains(&target))
}

fn task_selected(scope: &mut v8::PinScope, task: &Task, targets: &[u64]) -> Result<bool, String> {
    match task.context.to_local(scope) {
        Some(context) if context.get_slot::<FrameRetired>().is_some() => Ok(true),
        Some(context) => receives(scope, context, targets),
        None => Ok(true),
    }
}

fn notice_selected(
    scope: &mut v8::PinScope,
    recipients: &[Target],
    targets: &[u64],
) -> Result<bool, String> {
    for recipient in recipients {
        if let Some(context) = recipient.context.to_local(scope) {
            if receives(scope, context, targets)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn candidate(
    scope: &mut v8::PinScope,
    state: &State,
    targets: &[u64],
) -> Result<Option<Candidate>, String> {
    let inbox = state.borrow().borrow::<Queue>().inbox.clone();
    let notices = inbox
        .lock()
        .map_err(|_| "Storage inbox unavailable")?
        .areas
        .clone();
    let first = notices.front().cloned();
    let mut obsolete = None;
    for (index, notice) in notices.into_iter().enumerate() {
        let recipients = matching_recipients(
            scope,
            &state.borrow(),
            None,
            notice.area,
            Some(notice.registration),
        );
        if notice_selected(scope, &recipients, targets)? {
            // Earlier notices must be admitted first even when only a peer
            // receives them, otherwise a later mixed notice reverses its FIFO.
            let first = first.expect("selected storage notice has a native prefix");
            let recipients = matching_recipients(
                scope,
                &state.borrow(),
                None,
                first.area,
                Some(first.registration),
            );
            return Ok(Some((0, first, recipients, true)));
        }
        if recipients.is_empty() && obsolete.is_none() {
            obsolete = Some((index, notice, recipients, false));
        }
    }
    Ok(obsolete)
}

fn remove_notice(inbox: &mut Inbox, index: usize) {
    if let Some(notice) = inbox.areas.remove(index) {
        if let Some(mutation) = notice.mutation {
            inbox.bytes -= payload_bytes(&mutation);
        }
    }
}

fn has_task(scope: &mut v8::PinScope, state: &State, targets: &[u64]) -> Result<bool, String> {
    for task in &state.borrow().borrow::<Queue>().tasks {
        if task_selected(scope, task, targets)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn admit(scope: &mut v8::PinScope, state: &State, targets: &[u64]) -> Result<bool, String> {
    let Some((index, notice, recipients, _)) = candidate(scope, state, targets)? else {
        return Ok(false);
    };
    let inbox = state.borrow().borrow::<Queue>().inbox.clone();
    if !recipients.is_empty() {
        let mut inbox = inbox.lock().map_err(|_| "Storage inbox unavailable")?;
        if std::mem::replace(&mut inbox.overflow, false) {
            return Err("RangeError: Pending storage event budget exceeded".into());
        }
    }
    let bytes = notice
        .mutation
        .as_ref()
        .map_or(0, |mutation| payload_bytes(mutation));
    if let Err(error) = ensure_capacity(&state.borrow(), &recipients, 0, bytes) {
        return if has_task(scope, state, targets)? {
            Ok(false)
        } else {
            Err(error.to_string())
        };
    }
    {
        let mut inbox = inbox.lock().map_err(|_| "Storage inbox unavailable")?;
        remove_notice(&mut inbox, index);
    }
    let mutation = notice.mutation.unwrap_or_else(|| {
        Arc::new(Mutation {
            key: None,
            old: None,
            new: None,
            url: String::new(),
        })
    });
    // Admission is native bookkeeping only. Other live recipients remain queued;
    // no handler, clock or IO belonging to them runs in this selected turn.
    publish_shared(&mut state.borrow_mut(), recipients, mutation);
    Ok(true)
}

fn take_task(
    scope: &mut v8::PinScope,
    state: &State,
    targets: &[u64],
) -> Result<Option<Task>, String> {
    let mut selected = None;
    for (index, task) in state.borrow().borrow::<Queue>().tasks.iter().enumerate() {
        if task_selected(scope, task, targets)? {
            selected = Some(index);
            break;
        }
    }
    let Some(index) = selected else {
        return Ok(None);
    };
    let mut state = state.borrow_mut();
    let queue = state.borrow_mut::<Queue>();
    let task = queue
        .tasks
        .remove(index)
        .expect("selected native storage task");
    let remaining = task.batch.queued.get() - 1;
    task.batch.queued.set(remaining);
    if remaining == 0 {
        queue.bytes -= payload_bytes(&task.batch.mutation);
    }
    Ok(Some(task))
}

pub(in crate::runtime) fn run(rt: &mut JsRuntime, targets: &[u64]) -> Result<bool, String> {
    let state = rt.op_state();
    if !state.borrow().has::<Queue>() {
        return Ok(false);
    }
    let delivered = {
        deno_core::scope!(scope, rt);
        let admitted = admit(scope, &state, targets)?;
        let task = take_task(scope, &state, targets)?;
        if task.is_none() {
            return Ok(admitted);
        }
        dispatch_task(scope, task).map_err(|error| error.to_string())? != 0
    };
    if delivered {
        rt.v8_isolate().perform_microtask_checkpoint();
        if rt.v8_isolate().is_execution_terminating() {
            return Err("Microtask execution was interrupted".into());
        }
    }
    Ok(delivered)
}

pub(in crate::runtime) fn pending(rt: &mut JsRuntime, targets: &[u64]) -> Result<bool, String> {
    let state = rt.op_state();
    if !state.borrow().has::<Queue>() {
        return Ok(false);
    }
    deno_core::scope!(scope, rt);
    if has_task(scope, &state, targets)? {
        return Ok(true);
    }
    match candidate(scope, &state, targets)? {
        Some((_, _, _, selected)) => Ok(selected),
        None => Ok(false),
    }
}
