//! Storage tasks retain native payloads and weak receiving realms only.
use super::*;
use crate::storage_inbox::{broadcast_commit, payload_bytes, InboxError, Membership};
pub(super) use crate::storage_inbox::{Inbox, Mutation};
use std::sync::Arc;

impl From<InboxError> for JsErrorBox {
    fn from(error: InboxError) -> Self {
        match error {
            InboxError::Budget => Self::range_error("Pending storage event budget exceeded"),
            InboxError::Registration => Self::range_error("Storage realm registration exhausted"),
            InboxError::Unavailable => Self::generic("Storage inbox unavailable"),
        }
    }
}

struct Factory {
    callback: v8::Weak<v8::Function>,
    creator: v8::Weak<v8::Function>,
    local: u32,
    session: u32,
    registration: u64,
    membership: Arc<Membership>,
}
pub(super) struct Target {
    context: v8::Weak<v8::Context>,
    session: bool,
}
struct Task {
    context: v8::Weak<v8::Context>,
    session: bool,
    batch: Rc<Batch>,
}
struct Batch {
    mutation: Arc<Mutation>,
    queued: std::cell::Cell<usize>,
}
#[derive(Default)]
struct Queue {
    inbox: Arc<std::sync::Mutex<Inbox>>,
    contexts: Vec<v8::Weak<v8::Context>>,
    tasks: std::collections::VecDeque<Task>,
    bytes: usize,
}

#[op2(fast)]
pub(super) fn op_storage_events_factory(
    scope: &mut v8::PinScope,
    state: &mut OpState,
    callback: v8::Local<v8::Function>,
    creator: v8::Local<v8::Function>,
) -> Result<(), JsErrorBox> {
    let Some(context) = callback.get_creation_context(scope) else {
        return Ok(());
    };
    let window = context.global(scope);
    let local = storage_area(scope, state, window, false, false).unwrap_or(0);
    let session = storage_area(scope, state, window, true, false).unwrap_or(0);
    if creator.get_creation_context(scope) != Some(context) {
        return Ok(());
    }
    let Some(creator) = crate::context_ownership::function(scope, creator) else {
        return Ok(());
    };
    let Some(callback) = crate::context_ownership::function(scope, callback) else {
        return Ok(());
    };
    if !state.has::<Queue>() {
        let queue = Queue::default();
        let storage = state.borrow::<BrowserStorage>().clone();
        if let Ok(mut areas) = storage.0.lock() {
            areas
                .subscribers
                .retain(|subscriber| subscriber.strong_count() != 0);
            areas.subscribers.push(Arc::downgrade(&queue.inbox));
        }
        state.put(queue);
    }
    let active = context.get_slot::<FrameRetired>().is_none();
    let (registration, membership) = {
        let mut inbox = state
            .borrow::<Queue>()
            .inbox
            .lock()
            .map_err(|_| JsErrorBox::generic("Storage inbox unavailable"))?;
        let registration = inbox.register()?;
        let membership = Arc::new(Membership {
            local,
            session,
            active: std::sync::atomic::AtomicBool::new(active),
        });
        inbox.memberships.retain(|entry| entry.strong_count() != 0);
        inbox.memberships.push(Arc::downgrade(&membership));
        (registration, membership)
    };
    context.set_slot(Rc::new(Factory {
        callback,
        creator,
        local,
        session,
        registration,
        membership,
    }));
    let queue = state.borrow_mut::<Queue>();
    queue.contexts.retain(|entry| {
        entry
            .to_local(scope)
            .is_some_and(|registered| registered != context)
    });
    queue.contexts.push(v8::Weak::new(scope, context));
    Ok(())
}

pub(super) fn recipients(
    scope: &mut v8::PinScope,
    state: &OpState,
    owner: Option<v8::Local<v8::Context>>,
    area: u32,
) -> Vec<Target> {
    matching_recipients(scope, state, owner, area, None)
}

fn matching_recipients(
    scope: &mut v8::PinScope,
    state: &OpState,
    owner: Option<v8::Local<v8::Context>>,
    area: u32,
    registration: Option<u64>,
) -> Vec<Target> {
    if !state.has::<Queue>() {
        return Vec::new();
    }
    state
        .borrow::<Queue>()
        .contexts
        .iter()
        .filter_map(|entry| {
            let context = entry.to_local(scope)?;
            if owner == Some(context) || context.get_slot::<FrameRetired>().is_some() {
                return None;
            }
            let factory = context.get_slot::<Factory>()?;
            if registration.is_some_and(|cutoff| factory.registration > cutoff) {
                return None;
            }
            let session = if factory.local == area {
                false
            } else if factory.session == area {
                true
            } else {
                return None;
            };
            Some(Target {
                context: v8::Weak::new(scope, context),
                session,
            })
        })
        .collect()
}

pub(super) fn ensure_capacity(
    state: &OpState,
    recipients: &[Target],
    units: usize,
    url_bytes: usize,
) -> Result<(), JsErrorBox> {
    if recipients.is_empty() {
        return Ok(());
    }
    let queue = state.borrow::<Queue>();
    // Guest writes cannot grow an unaccounted host queue beyond the isolate's
    // task budget. Check before committing the backing-store mutation.
    const MAX_PENDING_BYTES: usize = 64 * 1024 * 1024;
    const MAX_PENDING_TASKS: usize = 8192;
    let payload = units.saturating_mul(2).saturating_add(url_bytes);
    if queue.bytes.saturating_add(payload) > MAX_PENDING_BYTES
        || queue.tasks.len().saturating_add(recipients.len()) > MAX_PENDING_TASKS
    {
        return Err(JsErrorBox::range_error(
            "Pending storage event budget exceeded",
        ));
    }
    Ok(())
}

pub(super) fn retire(context: v8::Local<v8::Context>) {
    if let Some(factory) = context.get_slot::<Factory>() {
        factory
            .membership
            .active
            .store(false, std::sync::atomic::Ordering::Release);
    }
}

fn publish_shared(state: &mut OpState, recipients: Vec<Target>, mutation: Arc<Mutation>) {
    if recipients.is_empty() {
        return;
    }
    let queue = state.borrow_mut::<Queue>();
    queue.bytes += payload_bytes(&mutation);
    // Per-queue accounting must not depend on other inboxes sharing the payload.
    let batch = Rc::new(Batch {
        mutation,
        queued: std::cell::Cell::new(recipients.len()),
    });
    for target in recipients {
        queue.tasks.push_back(Task {
            context: target.context,
            session: target.session,
            batch: batch.clone(),
        });
    }
}

/// Native locks cover preflight and backing commit; no V8 or callback runs here.
pub(super) fn commit(
    state: &mut OpState,
    recipients: Vec<Target>,
    subscribers: &[std::sync::Weak<std::sync::Mutex<Inbox>>],
    area: u32,
    mutation: Mutation,
    apply: impl FnOnce(),
) -> Result<Vec<std::task::Waker>, JsErrorBox> {
    let bytes = payload_bytes(&mutation);
    ensure_capacity(state, &recipients, 0, bytes)?;
    let own = state
        .has::<Queue>()
        .then(|| state.borrow::<Queue>().inbox.clone());
    let mutation = Arc::new(mutation);
    let wake = broadcast_commit(subscribers, own.as_ref(), area, &mutation, apply)?;
    publish_shared(state, recipients, mutation);
    Ok(wake)
}

fn optional_string<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: &Option<Vec<u16>>,
) -> v8::Local<'s, v8::Value> {
    match value {
        Some(units) => storage_string(scope, units),
        None => v8::null(scope).into(),
    }
}

#[op2(fast, reentrant)]
pub(super) fn op_storage_events(scope: &mut v8::PinScope) -> Result<u32, JsErrorBox> {
    deliver_next(scope)
}

pub(super) fn deliver_next(scope: &mut v8::PinScope) -> Result<u32, JsErrorBox> {
    let state = JsRuntime::op_state_from(scope);
    {
        let mut state = state.borrow_mut();
        if state.has::<Queue>() {
            let inbox = state.borrow::<Queue>().inbox.clone();
            loop {
                let notice = inbox
                    .lock()
                    .map_err(|_| JsErrorBox::generic("Storage inbox unavailable"))?
                    .next()?;
                let Some(notice) = notice else {
                    break;
                };
                // No V8 work holds the native inbox lock. A capacity-blocked
                // notice remains queued while existing tasks free space.
                // A response cannot notify a realm created after its headers.
                let targets = matching_recipients(
                    scope,
                    &state,
                    None,
                    notice.area,
                    Some(notice.registration),
                );
                if let Err(error) = ensure_capacity(
                    &state,
                    &targets,
                    0,
                    notice
                        .mutation
                        .as_ref()
                        .map_or(0, |mutation| payload_bytes(mutation)),
                ) {
                    if state.borrow::<Queue>().tasks.is_empty() {
                        return Err(error);
                    }
                    break;
                }
                inbox
                    .lock()
                    .map_err(|_| JsErrorBox::generic("Storage inbox unavailable"))?
                    .pop();
                let has_targets = !targets.is_empty();
                let mutation = notice.mutation.unwrap_or_else(|| {
                    Arc::new(Mutation {
                        key: None,
                        old: None,
                        new: None,
                        url: String::new(),
                    })
                });
                publish_shared(&mut state, targets, mutation);
                if has_targets {
                    break;
                }
            }
        }
    }
    // Deliver one task per checkpoint. A handler's microtasks precede the next
    // storage task, and reentrant mutations cannot run in this dispatch batch.
    let task = {
        let mut state = state.borrow_mut();
        if !state.has::<Queue>() {
            return Ok(0);
        }
        let queue = state.borrow_mut::<Queue>();
        let task = queue.tasks.pop_front();
        if let Some(task) = &task {
            let remaining = task.batch.queued.get() - 1;
            task.batch.queued.set(remaining);
            if remaining == 0 {
                queue.bytes -= payload_bytes(&task.batch.mutation);
            }
        }
        task
    };
    let Some(task) = task else {
        return Ok(0);
    };
    let Some(context) = task.context.to_local(scope) else {
        return Ok(1);
    };
    if context.get_slot::<FrameRetired>().is_some() {
        return Ok(1);
    }
    let Some(factory) = context.get_slot::<Factory>() else {
        return Ok(1);
    };
    let Some(callback) = factory.callback.to_local(scope) else {
        return Ok(1);
    };
    let scope = &mut v8::ContextScope::new(scope, context);
    let key = optional_string(scope, &task.batch.mutation.key);
    let old = optional_string(scope, &task.batch.mutation.old);
    let new = optional_string(scope, &task.batch.mutation.new);
    let url = v8::String::new(scope, &task.batch.mutation.url)
        .unwrap()
        .into();
    let session = v8::Boolean::new(scope, task.session).into();
    // Native strings have been copied into V8. Release the payload before a
    // guest handler can reenter dispatch and admit another bounded batch.
    drop(task);
    let receiver = v8::undefined(scope).into();
    callback.call(scope, receiver, &[key, old, new, url, session]);
    Ok(1)
}

pub(super) fn waiter(state: &OpState) -> Option<crate::storage_inbox::Waiter> {
    state
        .has::<Queue>()
        .then(|| crate::storage_inbox::Waiter::new(state.borrow::<Queue>().inbox.clone()))
}

pub(super) fn signal(state: &OpState) -> crate::StorageEventSignal {
    let ready = state.has::<Queue>() && !state.borrow::<Queue>().tasks.is_empty();
    crate::StorageEventSignal::new(waiter(state), ready)
}

pub(super) fn pending(state: &OpState) -> usize {
    if state.has::<Queue>() {
        let queue = state.borrow::<Queue>();
        queue
            .tasks
            .len()
            .saturating_add(queue.inbox.lock().map_or(0, |inbox| inbox.areas.len()))
    } else {
        0
    }
}

#[op2(reentrant)]
pub(super) fn op_storage_event_create<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    document_runtime_state(scope, document)?;
    let context = document
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::type_error("Illegal invocation"))?;
    let factory = context
        .get_slot::<Factory>()
        .ok_or_else(|| JsErrorBox::generic("Event creation context unavailable"))?;
    let callback = crate::context_ownership::local(scope, &factory.creator);
    let scope = &mut v8::ContextScope::new(scope, context);
    let receiver = v8::undefined(scope).into();
    callback
        .call(scope, receiver, &[])
        .ok_or_else(|| JsErrorBox::generic("Event creation interrupted"))
}
