//! Native async work belongs to authenticated document contexts, not the shared
//! infrastructure op driver. Leases and Promise edges never retain a guest.
use super::*;
use std::collections::HashSet;

#[path = "async_owner/bridge.rs"]
mod bridge;
#[path = "async_owner/drain.rs"]
mod drain;
#[path = "async_owner/owners.rs"]
mod owners;
#[path = "async_owner/work.rs"]
mod work;
pub(super) use drain::{drain, quiescence_turn, QuiescenceTurn};

use bridge::OwnerAsyncContext;
use owners::{LeaseState, WeakOwners};
use work::{OwnerId, PromiseTurn, PromiseWork};

struct State {
    bridge: Rc<OwnerAsyncContext>,
    owners: WeakOwners,
    work: PromiseWork,
    failure: Option<String>,
    retained_streams: HashSet<OwnerId>,
}

#[derive(Clone)]
struct Tracker(Rc<RefCell<State>>);

pub(super) struct Selection {
    pub contexts: Vec<v8::Global<v8::Context>>,
    pub targets: Vec<u64>,
    pub pending_ops: usize,
    pub runnable: bool,
    pub modules: bool,
    pub blocked_reactions: bool,
}

fn tracker(scope: &v8::PinScope) -> Option<Rc<RefCell<State>>> {
    scope.get_slot::<Tracker>().map(|tracker| tracker.0.clone())
}

fn document_error(
    scope: &mut v8::PinScope,
    context: v8::Local<v8::Context>,
) -> Option<&'static str> {
    if context.get_slot::<FrameRetired>().is_some() {
        return Some("retired document");
    }
    if context
        .get_slot::<private_boundary::Infrastructure>()
        .is_some()
    {
        return Some("infrastructure context");
    }
    if context.get_slot::<CspHandle>().is_none() {
        return Some("missing native document policy");
    }
    let Some(document) = context
        .get_slot::<ActiveDocument>()
        .and_then(|document| document.0.to_local(scope))
    else {
        return Some("missing native active document");
    };
    if document.get_creation_context(scope) != Some(context) {
        return Some("foreign native document");
    }
    None
}

fn valid_document(scope: &mut v8::PinScope, context: v8::Local<v8::Context>) -> bool {
    document_error(scope, context).is_none()
}

fn known(lease: LeaseState) -> Option<OwnerId> {
    match lease {
        LeaseState::Live(owner) | LeaseState::Retired(owner) => Some(owner),
        LeaseState::Unknown => None,
    }
}

fn current_owner(scope: &mut v8::PinScope, shared: &Rc<RefCell<State>>) -> Option<OwnerId> {
    let bridge = shared.borrow().bridge.clone();
    // The getter is trusted E code. No state/registry/graph borrow crosses it.
    let token = bridge.current(scope).ok()?;
    known(shared.borrow().owners.resolve(scope, token, valid_document))
}

fn document_owner<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    state: &mut State,
    context: v8::Local<'s, v8::Context>,
) -> Option<OwnerId> {
    match state.owners.identify(scope, context, valid_document) {
        LeaseState::Live(owner) | LeaseState::Retired(owner) => Some(owner),
        LeaseState::Unknown => state
            .owners
            .register(scope, context, valid_document)
            .map(|(owner, _)| owner),
    }
}

fn promise_owner<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    shared: &Rc<RefCell<State>>,
    promise: v8::Local<'s, v8::Promise>,
) -> Option<OwnerId> {
    // A borrowed foreign Promise constructor does not replace the native
    // registering continuation's owner.
    if let Some(owner) = current_owner(scope, shared) {
        return Some(owner);
    }
    let context = promise.get_creation_context(scope)?;
    document_owner(scope, &mut shared.borrow_mut(), context)
}

extern "C" fn promise_hook(
    turn: v8::PromiseHookType,
    promise: v8::Local<v8::Promise>,
    parent: v8::Local<v8::Value>,
) {
    // SAFETY: V8 supplies the live callback Promise, the public binding's
    // documented CallbackScope entry for its native Promise hook.
    v8::callback_scope!(unsafe scope, promise);
    if scope.is_execution_terminating() {
        return;
    }
    let Some(shared) = tracker(scope) else {
        return;
    };
    let owner = promise_owner(scope, &shared, promise);
    let parent = v8::Local::<v8::Promise>::try_from(parent).ok();
    let turn = match turn {
        v8::PromiseHookType::Init => PromiseTurn::Init,
        v8::PromiseHookType::Before => PromiseTurn::Before,
        v8::PromiseHookType::After => PromiseTurn::After,
        v8::PromiseHookType::Resolve => PromiseTurn::Resolve,
    };
    shared
        .borrow_mut()
        .work
        .promise_turn(scope, turn, promise, parent, owner);
}

extern "C" fn inactive_hook(
    _: v8::PromiseHookType,
    _: v8::Local<v8::Promise>,
    _: v8::Local<v8::Value>,
) {
}

pub(super) fn install(rt: &mut JsRuntime) -> Result<(), String> {
    let shared = {
        deno_core::scope!(scope, rt);
        let infrastructure = scope.get_current_context();
        if infrastructure
            .get_slot::<private_boundary::Infrastructure>()
            .is_none()
        {
            return Err("Native async infrastructure context is unavailable".into());
        }
        let core = infrastructure
            .get_slot::<RuntimeCore>()
            .and_then(|core| core.0.to_local(scope))
            .ok_or("Native async infrastructure core is unavailable")?;
        if core.get_creation_context(scope) != Some(infrastructure) {
            return Err("Native async core belongs to another context".into());
        }
        let bridge = OwnerAsyncContext::initialize(scope, infrastructure, core)?;
        Rc::new(RefCell::new(State {
            bridge: Rc::new(bridge),
            owners: WeakOwners::new(scope),
            work: PromiseWork::new(scope),
            failure: None,
            retained_streams: HashSet::new(),
        }))
    };
    let observed = Rc::downgrade(&shared);
    rt.op_state()
        .borrow_mut()
        .put(deno_core::AsyncOpObserver(Rc::new(move |scope, event| {
            let Some(shared) = observed.upgrade() else {
                return;
            };
            let owner = if event.event == deno_core::AsyncOpEvent::Registered {
                current_owner(scope, &shared)
            } else {
                None
            };
            shared.borrow_mut().work.op_turn(scope, event, owner);
        })));
    rt.v8_isolate().set_slot(Tracker(shared));
    rt.v8_isolate().set_promise_hook(promise_hook);
    Ok(())
}

pub(super) fn uninstall(rt: &mut JsRuntime) {
    if rt.v8_isolate().get_slot::<Tracker>().is_none() {
        return;
    }
    rt.v8_isolate().set_promise_hook(inactive_hook);
    rt.v8_isolate().remove_slot::<Tracker>();
    let state = rt.op_state();
    let mut state = state.borrow_mut();
    if state.has::<deno_core::AsyncOpObserver>() {
        state.take::<deno_core::AsyncOpObserver>();
    }
}

pub(super) fn with_document<'s, T>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    run: impl FnOnce(&mut v8::PinScope<'s, '_>) -> T,
) -> Result<T, String> {
    let Some(shared) = tracker(scope) else {
        if JsRuntime::op_state_from(scope)
            .borrow()
            .has::<private_boundary::Infrastructure>()
        {
            return Err("Native async ownership is unavailable".into());
        }
        return Ok(run(scope));
    };
    if let Some(reason) = document_error(scope, context) {
        return Err(document_diagnostic(scope, context, reason));
    }
    let (token, bridge) = {
        let mut state = shared.borrow_mut();
        let (owner, token) = state
            .owners
            .register(scope, context, valid_document)
            .ok_or("Native async owner registration is unavailable")?;
        let _ = owner;
        (v8::Global::new(scope, token), state.bridge.clone())
    };
    let token = v8::Local::new(scope, token);
    bridge
        .with_owner(scope, token.into(), run)
        .map_err(str::to_owned)
}

// A retained stream is an existing JS algorithm, not renewed document admission.
fn valid_retained_stream_document(
    scope: &mut v8::PinScope,
    context: v8::Local<v8::Context>,
) -> bool {
    if context
        .get_slot::<private_boundary::Infrastructure>()
        .is_some()
        || context.get_slot::<CspHandle>().is_none()
    {
        return false;
    }
    context
        .get_slot::<ActiveDocument>()
        .and_then(|document| document.0.to_local(scope))
        .is_some_and(|document| document.get_creation_context(scope) == Some(context))
}

pub(super) fn with_retained_stream<'s, T>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    run: impl FnOnce(&mut v8::PinScope<'s, '_>) -> T,
) -> Result<T, String> {
    if !valid_retained_stream_document(scope, context) {
        return Err("Retained stream document is unavailable".into());
    }
    let Some(shared) = tracker(scope) else {
        return with_document(scope, context, run);
    };
    let (token, bridge) = {
        let mut state = shared.borrow_mut();
        let registered = if context.get_slot::<FrameRetired>().is_some() {
            state.owners.retained_stream(scope, context)
        } else {
            state.owners.register(scope, context, valid_document)
        };
        let (owner, token) = registered.ok_or("Retained stream owner is unavailable")?;
        state.retained_streams.insert(owner);
        (v8::Global::new(scope, token), state.bridge.clone())
    };
    let token = v8::Local::new(scope, token);
    bridge
        .with_owner(scope, token.into(), run)
        .map_err(str::to_owned)
}

#[derive(Clone, Copy)]
enum ReactionSelection {
    Strict,
    AwaitCompletion,
}

struct SelectedContexts {
    contexts: Vec<v8::Global<v8::Context>>,
    pending_ops: usize,
    runnable: bool,
    blocked_reactions: bool,
}

fn selected_contexts<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    state: &mut State,
    roots: &[v8::Global<v8::Context>],
    reactions: ReactionSelection,
) -> Result<SelectedContexts, String> {
    if let Some(error) = &state.failure {
        return Err(error.clone());
    }
    state.owners.prune(scope);
    state
        .retained_streams
        .retain(|owner| state.owners.contains(*owner));
    let mut owners = HashSet::new();
    for context in roots {
        let context = v8::Local::new(scope, context);
        owners.insert(
            document_owner(scope, state, context).ok_or("Native async root is unavailable")?,
        );
    }
    let snapshot = state.work.snapshot(scope, &owners);
    if let Some(error) = snapshot.tracking_error {
        return Err(error.into());
    }
    if snapshot.inconsistent_owners
        || snapshot.unknown_ops != 0
        || (snapshot.unknown_reaction_parents != 0
            && (matches!(reactions, ReactionSelection::Strict)
                || snapshot.unknown_reaction_parents != snapshot.collected_root_reaction_parents))
    {
        return Err("Native async work has unclassified ownership".into());
    }
    let mut contexts = Vec::with_capacity(snapshot.owners.len());
    for owner in snapshot.owners {
        let context = state
            .owners
            .context(scope, owner, valid_retained_stream_document)
            .ok_or("Native async dependency document is closed")?;
        if context.get_slot::<FrameRetired>().is_some() {
            if !state.retained_streams.contains(&owner) {
                return Err("Native async dependency document is closed".into());
            }
            // Its native Promise edges remain classified; no retired timer,
            // resource, message, history or IDB task enters the selection.
            continue;
        }
        contexts.push(v8::Global::new(scope, context));
    }
    Ok(SelectedContexts {
        contexts,
        pending_ops: snapshot.selected_ops,
        runnable: snapshot.runnable_reactions != 0,
        blocked_reactions: snapshot.unknown_reaction_parents != 0,
    })
}

pub(super) fn selection(rt: &mut JsRuntime, target: u64) -> Result<Selection, String> {
    select_work(rt, target, ReactionSelection::Strict)
}

pub(super) fn completion_selection(rt: &mut JsRuntime, target: u64) -> Result<Selection, String> {
    select_work(rt, target, ReactionSelection::AwaitCompletion)
}

fn select_work(
    rt: &mut JsRuntime,
    target: u64,
    reactions: ReactionSelection,
) -> Result<Selection, String> {
    let roots = auxiliary_task_contexts(rt, Some(target))?;
    let (contexts, pending_ops, runnable, modules, blocked_reactions) = {
        deno_core::scope!(scope, rt);
        let shared = tracker(scope).ok_or("Native async ownership is unavailable")?;
        let SelectedContexts {
            contexts,
            pending_ops,
            runnable,
            blocked_reactions,
        } = selected_contexts(scope, &mut shared.borrow_mut(), &roots, reactions)?;
        let mut modules = false;
        for context in &contexts {
            let context = v8::Local::new(scope, context);
            if context.get_slot::<NativePageModuleContext>().is_some() {
                modules |= JsRuntime::module_context_pending_work(scope, context)
                    .ok_or("Native async module owner is unavailable")?;
            }
        }
        (contexts, pending_ops, runnable, modules, blocked_reactions)
    };
    let targets = selected_document_tasks::selected_task_targets(rt, &contexts)?;
    Ok(Selection {
        contexts,
        targets,
        pending_ops,
        runnable,
        modules,
        blocked_reactions,
    })
}

pub(super) fn enabled(rt: &mut JsRuntime) -> bool {
    rt.v8_isolate().get_slot::<Tracker>().is_some()
}

/// Retain a failed owner admission instead of silently discarding the task.
pub(super) fn task_failure(scope: &mut v8::PinScope, error: String) {
    if let Some(state) = tracker(scope) {
        state.borrow_mut().failure.get_or_insert(error);
    }
}

fn document_diagnostic(
    scope: &mut v8::PinScope,
    context: v8::Local<v8::Context>,
    reason: &str,
) -> String {
    let document = context
        .get_slot::<ActiveDocument>()
        .and_then(|document| document.0.to_local(scope));
    let matches =
        document.and_then(|document| document.get_creation_context(scope)) == Some(context);
    let id = turbo_dom_browser_env::host_internals::auxiliary_context_identity(context)
        .map(|identity| identity.id.0);
    format!("Native async document is unavailable: {reason}; native_target={id:?} infrastructure={} retired={} policy={} document={} document_realm={} current={} entered={}",
        context.get_slot::<private_boundary::Infrastructure>().is_some(),
        context.get_slot::<FrameRetired>().is_some(),
        context.get_slot::<CspHandle>().is_some(), document.is_some(), matches,
        scope.get_current_context() == context,
        scope.get_entered_or_microtask_context() == context)
}
