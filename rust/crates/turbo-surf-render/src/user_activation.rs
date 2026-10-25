//! Native per-window user activation (https://html.spec.whatwg.org/#tracking-user-activation):
//! the last activation and last history-action activation timestamps live in each frame's
//! native context, never in guest flags. Trusted host input notifies the activated window,
//! its ancestors and its same-origin descendants; consumption spans the whole frame tree.
use super::*;
use std::cell::Cell;

/// "Positive infinity": the window was never activated.
const NEVER: u64 = 0;
/// "Negative infinity": transient activation was consumed.
const CONSUMED: u64 = u64::MAX;
const TRANSIENT_ACTIVATION_DURATION: std::time::Duration = std::time::Duration::from_secs(5);

thread_local! {
    static NEXT_STAMP: Cell<u64> = const { Cell::new(1) };
}

/// Timestamps are opaque activation stamps; `at` is the real time of the last activation.
pub(super) struct HostUserActivation {
    stamp: Cell<u64>,
    at: Cell<Option<std::time::Instant>>,
    history: Cell<u64>,
}

#[derive(Clone)]
pub struct NavigationActivationSnapshot {
    source: String,
    sticky: bool,
}

struct InheritedSticky(bool);

fn record(context: v8::Local<v8::Context>) -> Rc<HostUserActivation> {
    if let Some(record) = context.get_slot::<HostUserActivation>() {
        return record;
    }
    let record = Rc::new(HostUserActivation {
        stamp: Cell::new(NEVER),
        at: Cell::new(None),
        history: Cell::new(NEVER),
    });
    context.set_slot(record.clone());
    record
}

pub(super) fn has_been_active(context: v8::Local<v8::Context>) -> bool {
    context
        .get_slot::<HostUserActivation>()
        .is_some_and(|record| record.stamp.get() != NEVER)
        || context
            .get_slot::<InheritedSticky>()
            .is_some_and(|state| state.0)
}

/// Transient activation: activated less than the transient activation duration ago and
/// not consumed since.
pub(super) fn host_user_activation(context: v8::Local<v8::Context>) -> bool {
    context
        .get_slot::<HostUserActivation>()
        .is_some_and(|record| {
            !matches!(record.stamp.get(), NEVER | CONSUMED)
                && record
                    .at
                    .get()
                    .is_some_and(|at| at.elapsed() < TRANSIENT_ACTIVATION_DURATION)
        })
}

fn history_action(context: v8::Local<v8::Context>) -> bool {
    context
        .get_slot::<HostUserActivation>()
        .is_some_and(|record| record.stamp.get() != record.history.get())
}

fn parent<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) -> Option<v8::Local<'s, v8::Context>> {
    let parent = context.get_slot::<FrameParent>()?;
    let parent = parent.0.to_local(scope)?;
    (parent != context).then_some(parent)
}

fn top<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    mut context: v8::Local<'s, v8::Context>,
) -> v8::Local<'s, v8::Context> {
    while let Some(next) = parent(scope, context) {
        context = next;
    }
    context
}

/// The inclusive descendant frames of `root`, optionally only those same-origin with it.
fn descendants<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    root: v8::Local<'s, v8::Context>,
    same_origin: bool,
) -> Vec<v8::Local<'s, v8::Context>> {
    let origin = root.get_security_token(scope);
    let mut out = vec![root];
    let mut index = 0;
    while index < out.len() {
        let children = frame_children(scope, out[index]);
        for child in children {
            let token = child.get_security_token(scope);
            if !same_origin || token.strict_equals(origin) {
                out.push(child);
            }
        }
        index += 1;
    }
    out
}

fn active_document<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) -> Option<v8::Local<'s, v8::Object>> {
    context.get_slot::<ActiveDocument>()?.0.to_local(scope)
}

/// https://html.spec.whatwg.org/#activation-notification for trusted host input.
#[op2(nofast, reentrant)]
pub(super) fn op_host_user_activation(scope: &mut v8::PinScope) {
    let context = scope.get_current_context();
    if context.get_slot::<HostInputDriver>().is_none()
        || context.get_slot::<FrameRetired>().is_some()
    {
        return;
    }
    let stamp = NEXT_STAMP.with(|next| {
        let stamp = next.get();
        next.set(stamp + 1);
        stamp
    });
    let now = std::time::Instant::now();
    let mut windows = descendants(scope, context, true);
    let mut ancestor = parent(scope, context);
    while let Some(next) = ancestor {
        windows.push(next);
        ancestor = parent(scope, next);
    }
    for window in windows {
        let record = record(window);
        record.stamp.set(stamp);
        record.at.set(Some(now));
        if let Some(document) = active_document(scope, window) {
            crate::browser_env::notify_user_activation(scope, document);
        }
    }
}

/// https://html.spec.whatwg.org/#consume-user-activation and its history-action form: every
/// window of the top-level traversable's frame tree.
fn consume<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    history: bool,
) {
    let root = top(scope, context);
    for window in descendants(scope, root, false) {
        let Some(record) = window.get_slot::<HostUserActivation>() else {
            continue;
        };
        if history {
            record.history.set(record.stamp.get());
        } else if record.stamp.get() != NEVER {
            record.stamp.set(CONSUMED);
        }
    }
}

/// The vendored binding's activation authority for showPicker, fullscreen and close watchers.
fn activation_host(
    scope: &mut v8::PinScope,
    document: v8::Local<v8::Object>,
    query: crate::browser_env::ActivationQuery,
) -> bool {
    use crate::browser_env::ActivationQuery as Query;
    let Some(context) = document.get_creation_context(scope) else {
        return false;
    };
    match query {
        Query::Transient => host_user_activation(context),
        Query::HistoryAction => history_action(context),
        Query::ConsumeTransient => {
            consume(scope, context, false);
            true
        }
        Query::ConsumeHistoryAction => {
            consume(scope, context, true);
            true
        }
        Query::CrossOriginFrame => {
            // A sandboxed frame keeps its creator's token but has an opaque origin.
            let root = top(scope, context);
            let own = context.get_security_token(scope);
            let top_origin = root.get_security_token(scope);
            window_is_opaque(scope, context) != window_is_opaque(scope, root)
                || !own.strict_equals(top_origin)
        }
        Query::FullscreenAllowed => document_feature_enabled(scope, document, "fullscreen"),
    }
}

impl NavigationActivationSnapshot {
    pub(super) fn capture(source: String, context: v8::Local<v8::Context>) -> Self {
        Self {
            source,
            sticky: has_been_active(context),
        }
    }

    fn carries_to(&self, destination: &str) -> bool {
        let (Ok(source), Ok(destination)) =
            (url::Url::parse(&self.source), url::Url::parse(destination))
        else {
            return false;
        };
        self.sticky
            && matches!(source.origin(), url::Origin::Tuple(..))
            && source.origin() == destination.origin()
    }
}

pub(super) fn seed(
    rt: &mut JsRuntime,
    destination: &str,
    snapshot: Option<NavigationActivationSnapshot>,
) {
    let sticky = snapshot.is_some_and(|snapshot| snapshot.carries_to(destination));
    deno_core::scope!(scope, rt);
    scope
        .get_current_context()
        .set_slot(Rc::new(InheritedSticky(sticky)));
}

fn key<'s>(scope: &mut v8::PinScope<'s, '_>, name: &str) -> Option<v8::Local<'s, v8::Private>> {
    let name = v8::String::new(scope, name)?;
    Some(v8::Private::for_api(scope, Some(name)))
}

#[op2(fast)]
pub(super) fn op_user_activation_install(
    scope: &mut v8::PinScope,
    navigator: v8::Local<v8::Object>,
    activation: v8::Local<v8::Object>,
) -> bool {
    let Some(brand) = key(scope, "turbo-surf.user-activation.brand") else {
        return false;
    };
    let yes = v8::Boolean::new(scope, true);
    if activation.set_private(scope, brand, yes.into()) != Some(true) {
        return false;
    }
    let Some(owner) = key(scope, "turbo-surf.user-activation.navigator") else {
        return false;
    };
    let context = navigator
        .get_creation_context(scope)
        .unwrap_or_else(|| scope.get_current_context());
    crate::browser_env::set_activation_host(context, activation_host);
    navigator.set_private(scope, owner, activation.into()) == Some(true)
}

/// Brand a MessageEvent's UserActivation snapshot: the source window's state at post time.
#[op2(fast)]
pub(super) fn op_user_activation_snapshot(
    scope: &mut v8::PinScope,
    snapshot: v8::Local<v8::Object>,
    active: bool,
    sticky: bool,
) -> bool {
    let (Some(brand), Some(state)) = (
        key(scope, "turbo-surf.user-activation.brand"),
        key(scope, "turbo-surf.user-activation.snapshot"),
    ) else {
        return false;
    };
    let yes = v8::Boolean::new(scope, true);
    let value = v8::Integer::new(scope, i32::from(active) | (i32::from(sticky) << 1));
    snapshot.set_private(scope, brand, yes.into()) == Some(true)
        && snapshot.set_private(scope, state, value.into()) == Some(true)
}

#[op2]
pub(super) fn op_user_activation_navigator<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Value>,
) -> Result<v8::Local<'s, v8::Object>, JsErrorBox> {
    let receiver = v8::Local::<v8::Object>::try_from(receiver)
        .map_err(|_| JsErrorBox::type_error("Illegal invocation"))?;
    let owner = key(scope, "turbo-surf.user-activation.navigator")
        .ok_or_else(|| JsErrorBox::type_error("Activation state unavailable"))?;
    receiver
        .get_private(scope, owner)
        .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
        .ok_or_else(|| JsErrorBox::type_error("Illegal invocation"))
}

#[op2(fast)]
pub(super) fn op_user_activation_read(
    scope: &mut v8::PinScope,
    receiver: v8::Local<v8::Value>,
    sticky: bool,
) -> Result<bool, JsErrorBox> {
    let receiver = v8::Local::<v8::Object>::try_from(receiver)
        .map_err(|_| JsErrorBox::type_error("Illegal invocation"))?;
    let brand = key(scope, "turbo-surf.user-activation.brand")
        .ok_or_else(|| JsErrorBox::type_error("Activation state unavailable"))?;
    if !receiver
        .get_private(scope, brand)
        .is_some_and(|value| value.is_true())
    {
        return Err(JsErrorBox::type_error("Illegal invocation"));
    }
    let snapshot = key(scope, "turbo-surf.user-activation.snapshot")
        .and_then(|state| receiver.get_private(scope, state))
        .filter(|value| value.is_int32())
        .and_then(|value| value.int32_value(scope));
    if let Some(bits) = snapshot {
        return Ok(bits & if sticky { 2 } else { 1 } != 0);
    }
    let context = receiver
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::type_error("Illegal invocation"))?;
    Ok(if sticky {
        has_been_active(context)
    } else {
        host_user_activation(context)
    })
}

/// The source window's activation for `postMessage(..., {includeUserActivation: true})`.
pub(super) fn snapshot_bits(context: v8::Local<v8::Context>) -> i32 {
    i32::from(host_user_activation(context)) | (i32::from(has_been_active(context)) << 1)
}

/// A trusted Esc keydown the page did not cancel: process the window's close watchers.
#[op2(nofast, reentrant)]
pub(super) fn op_host_close_request(scope: &mut v8::PinScope) -> bool {
    let context = scope.get_current_context();
    if context.get_slot::<HostInputDriver>().is_none() {
        return false;
    }
    let Some(document) = active_document(scope, context) else {
        return false;
    };
    crate::browser_env::process_close_watchers(scope, document)
}

/// Light dismiss for the host driver's trusted pointerdown/pointerup.
#[op2(nofast, reentrant)]
pub(super) fn op_host_light_dismiss(
    scope: &mut v8::PinScope,
    event: v8::Local<v8::Object>,
    backdrop: bool,
) {
    if scope
        .get_current_context()
        .get_slot::<HostInputDriver>()
        .is_some()
    {
        crate::browser_env::light_dismiss(scope, event, backdrop);
    }
}

/// The modal dialog whose `::backdrop` covers `node` (pointer input hits the dialog).
#[op2(reentrant)]
pub(super) fn op_host_modal_backdrop<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    node: v8::Local<'s, v8::Object>,
) -> v8::Local<'s, v8::Value> {
    if scope
        .get_current_context()
        .get_slot::<HostInputDriver>()
        .is_none()
    {
        return v8::null(scope).into();
    }
    match crate::browser_env::modal_backdrop(scope, node) {
        Some(dialog) => dialog.into(),
        None => v8::null(scope).into(),
    }
}
