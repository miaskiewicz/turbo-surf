//! Host-owned Pages inside one isolate. The binding owns every WindowProxy;
//! these strong handles last only as long as the corresponding driver Page.
use super::*;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    Arc, Mutex,
};
use turbo_dom_browser_env::host_internals::{
    auxiliary_context_identity, commit_auxiliary_navigation, commit_isolated_auxiliary_navigation,
    retire_auxiliary_context, AuxiliaryContextId, OpenRequest,
};

#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AuxiliaryCapability {
    #[default]
    Pending,
    Supported,
    Unsupported {
        reason: String,
    },
}

#[derive(Clone, Default)]
pub struct AuxiliaryPageEventLog(Arc<AuxiliaryPageEvents>);

#[derive(Default)]
struct AuxiliaryPageEvents {
    events: Mutex<VecDeque<AuxiliaryPageEvent>>,
    targets: Mutex<BTreeMap<String, AuxiliaryPageInfo>>,
    root: Mutex<Option<String>>,
    capability: Mutex<AuxiliaryCapability>,
    changed: tokio::sync::Notify,
    closed: AtomicBool,
    revision: AtomicU64,
    closing: Mutex<()>,
    limit: AtomicUsize,
    overflowed: AtomicBool,
}

impl AuxiliaryPageEventLog {
    pub fn capability(&self) -> AuxiliaryCapability {
        self.0.capability.lock().unwrap().clone()
    }
    pub(super) fn set_capability(&self, capability: AuxiliaryCapability) {
        *self.0.capability.lock().unwrap() = capability;
        self.record_change();
    }

    fn push(&self, event: AuxiliaryPageEvent) {
        {
            let mut targets = self.0.targets.lock().unwrap();
            match &event {
                AuxiliaryPageEvent::Created {
                    target,
                    source,
                    source_page,
                    opener_page,
                    url,
                } => {
                    targets.insert(
                        target.clone(),
                        AuxiliaryPageInfo {
                            target: target.clone(),
                            source: source.clone(),
                            source_page: source_page.clone(),
                            opener_page: opener_page.clone(),
                            url: url.clone(),
                            closed: false,
                        },
                    );
                }
                AuxiliaryPageEvent::Navigated {
                    target,
                    url,
                    opener_page,
                    ..
                } => {
                    if let Some(page) = targets.get_mut(target) {
                        page.url = url.clone();
                        page.opener_page = opener_page.clone();
                    }
                }
                AuxiliaryPageEvent::DomContentLoaded { .. }
                | AuxiliaryPageEvent::Load { .. }
                | AuxiliaryPageEvent::Overflow { .. } => {}
                AuxiliaryPageEvent::Closed { target } => {
                    if let Some(page) = targets.get_mut(target) {
                        page.closed = true;
                    }
                }
            }
        }
        let mut events = self.0.events.lock().unwrap();
        let limit = self.0.limit.load(Ordering::Acquire);
        let limit = if limit == 0 { 1024 } else { limit };
        if events.len() >= limit {
            self.0.overflowed.store(true, Ordering::Release);
            events.pop_back();
            events.push_back(AuxiliaryPageEvent::Overflow {
                message: "Native auxiliary lifecycle queue limit exceeded".into(),
            });
        } else {
            events.push_back(event);
        }
        drop(events);
        self.record_change();
    }

    pub fn set_limit(&self, limit: usize) {
        self.0.limit.store(limit, Ordering::Release);
    }
    pub fn overflowed(&self) -> bool {
        self.0.overflowed.load(Ordering::Acquire)
    }
    pub fn queued_len(&self) -> usize {
        self.0.events.lock().unwrap().len()
    }
    pub fn root(&self) -> Option<String> {
        self.0.root.lock().unwrap().clone()
    }
    pub fn info(&self, target: &str) -> Option<AuxiliaryPageInfo> {
        self.0.targets.lock().unwrap().get(target).cloned()
    }

    pub fn take(&self) -> Vec<AuxiliaryPageEvent> {
        self.0.events.lock().unwrap().drain(..).collect()
    }

    pub fn pages(&self) -> Vec<AuxiliaryPageInfo> {
        self.0.targets.lock().unwrap().values().cloned().collect()
    }
    pub fn has_events(&self) -> bool {
        !self.0.events.lock().unwrap().is_empty()
    }
    pub fn is_closed(&self) -> bool {
        self.0.closed.load(Ordering::Acquire)
    }
    fn record_change(&self) {
        self.0.revision.fetch_add(1, Ordering::AcqRel);
        self.0.changed.notify_waiters();
    }
    pub fn revision(&self) -> u64 {
        self.0.revision.load(Ordering::Acquire)
    }
    /// Called only after all actor-owned native worlds have been retired/dropped.
    pub fn close(&self) {
        let _closing = self.0.closing.lock().unwrap();
        if self.is_closed() {
            return;
        }
        let targets = self
            .0
            .targets
            .lock()
            .unwrap()
            .values()
            .filter(|target| !target.closed)
            .map(|target| target.target.clone())
            .collect::<Vec<_>>();
        for target in targets {
            self.push(AuxiliaryPageEvent::Closed { target });
        }
        self.0.closed.store(true, Ordering::Release);
        self.record_change();
    }
    pub async fn wait_for_revision(&self, observed: u64) -> bool {
        let notified = self.0.changed.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if self.revision() == observed && !self.is_closed() {
            notified.await;
        }
        self.is_closed()
    }
    pub async fn wait(&self) -> bool {
        let revision = self.revision();
        if self.has_events() || self.is_closed() {
            return self.is_closed();
        }
        self.wait_for_revision(revision).await
    }
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuxiliaryPageInfo {
    pub target: String,
    pub source: Option<String>,
    pub source_page: Option<String>,
    pub opener_page: Option<String>,
    pub url: String,
    pub closed: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AuxiliaryPageEvent {
    Created {
        target: String,
        source: Option<String>,
        source_page: Option<String>,
        opener_page: Option<String>,
        url: String,
    },
    Navigated {
        target: String,
        url: String,
        isolated: bool,
        opener_page: Option<String>,
    },
    DomContentLoaded {
        target: String,
    },
    Load {
        target: String,
    },
    Closed {
        target: String,
    },
    Overflow {
        message: String,
    },
}

struct Target {
    context: v8::Global<v8::Context>,
    source: Option<AuxiliaryContextId>,
    source_page: Option<AuxiliaryContextId>,
    published: bool,
    primary: bool,
    url: String,
    page_bootstrap: AuxiliaryBootstrap,
    snapshot: Option<TargetSnapshot>,
    driver_cookies: Option<String>,
}

#[derive(PartialEq, Eq)]
struct TargetSnapshot {
    revision: u64,
    documents: Vec<(u64, u64)>,
}

pub(super) struct Navigation {
    pub target: AuxiliaryContextId,
    pub url: String,
    pub noreferrer: bool,
    pub replace: bool,
    pub snapshot: auxiliary_navigation::SourceSnapshot,
    pub activity: Option<crate::network_activity::RequestActivity>,
}

#[derive(Clone, Default)]
pub struct AuxiliaryBootstrap {
    pub init_script: String,
    pub engine_init_script: String,
}

#[derive(Clone, Default)]
pub struct AuxiliaryHostServices {
    pub bootstrap: AuxiliaryBootstrap,
    pub root_bootstrap: AuxiliaryBootstrap,
    pub events: AuxiliaryPageEventLog,
    pub policy: AuxiliaryResourcePolicy,
}

#[derive(Default)]
pub(super) struct Pages {
    pub(super) enabled: bool,
    pub(super) bootstrap: AuxiliaryBootstrap,
    pub(super) root_bootstrap: AuxiliaryBootstrap,
    targets: BTreeMap<u64, Target>,
    pending: VecDeque<Navigation>,
    pub(super) events: AuxiliaryPageEventLog,
    pub(super) policy: AuxiliaryResourcePolicy,
    pub(super) reserved: usize,
    pub(super) created: usize,
}

impl Pages {
    pub(super) fn auxiliary_count(&self) -> usize {
        self.targets
            .values()
            .filter(|target| target.source.is_some())
            .count()
    }
    pub(super) fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

pub(super) fn register_root<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    url: &str,
) {
    let identity = auxiliary_context_identity(context).expect("installed native root identity");
    let root_bootstrap = JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<Pages>()
        .root_bootstrap
        .clone();
    register_network_activity_context(scope, context);
    let log = event_log(&JsRuntime::op_state_from(scope).borrow());
    *log.0.root.lock().unwrap() = Some(identity.id.0.to_string());
    log.0.targets.lock().unwrap().insert(
        identity.id.0.to_string(),
        AuxiliaryPageInfo {
            target: identity.id.0.to_string(),
            source: None,
            source_page: None,
            opener_page: None,
            url: url.to_owned(),
            closed: false,
        },
    );
    JsRuntime::op_state_from(scope)
        .borrow_mut()
        .borrow_mut::<Pages>()
        .targets
        .insert(
            identity.id.0,
            Target {
                context: v8::Global::new(scope, context),
                source: None,
                source_page: None,
                published: true,
                primary: true,
                url: url.to_owned(),
                page_bootstrap: root_bootstrap,
                snapshot: None,
                driver_cookies: None,
            },
        );
    log.set_capability(AuxiliaryCapability::Supported);
}

/// Admission has already allocated/initialized this actual native context.
/// Publication waits until the producer has installed its canonical identity.
pub(super) fn admitted(
    scope: &mut v8::PinScope,
    context: v8::Local<v8::Context>,
    request: &OpenRequest,
    source_page: Option<AuxiliaryContextId>,
    snapshot: auxiliary_navigation::SourceSnapshot,
) {
    let state = JsRuntime::op_state_from(scope);
    let mut state = state.borrow_mut();
    let network = state.borrow::<NetworkLog>().clone();
    let pages = state.borrow_mut::<Pages>();
    if request.new_context {
        pages.targets.insert(
            request.target_id.0,
            Target {
                context: v8::Global::new(scope, context),
                source: Some(request.source_id),
                source_page,
                published: false,
                primary: false,
                url: "about:blank".to_owned(),
                page_bootstrap: AuxiliaryBootstrap::default(),
                snapshot: None,
                driver_cookies: None,
            },
        );
    }
    if request.navigate && (!request.new_context || request.url != "about:blank") {
        pages
            .pending
            .retain(|navigation| navigation.target != request.target_id);
        pages.pending.push_back(Navigation {
            target: request.target_id,
            url: request.url.clone(),
            noreferrer: request.noreferrer,
            replace: false,
            snapshot,
            activity: Some(network.begin_page_navigation(&request.target_id.0.to_string())),
        });
        auxiliary_navigation::cancel(&mut state, request.target_id.0);
    }
}

pub(super) fn context<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    target: AuxiliaryContextId,
) -> Result<v8::Local<'s, v8::Context>, String> {
    let stored = JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<Pages>()
        .targets
        .get(&target.0)
        .map(|target| target.context.clone())
        .ok_or_else(|| "Auxiliary page is closed or not owned by this actor".to_owned())?;
    Ok(v8::Local::new(scope, stored))
}

/// A raw source may be an iframe; its Page is the nearest actual top-level root.
pub(super) fn top_level_identity<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    mut source: v8::Local<'s, v8::Context>,
) -> Option<AuxiliaryContextId> {
    while let Some(parent) = source.get_slot::<FrameParent>() {
        source = parent.0.to_local(scope)?;
    }
    auxiliary_context_identity(source).map(|identity| identity.id)
}

pub(super) fn contexts(state: &OpState) -> Vec<v8::Global<v8::Context>> {
    state
        .borrow::<Pages>()
        .targets
        .values()
        .filter(|target| !target.primary)
        .map(|target| target.context.clone())
        .collect()
}

fn opener_page<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) -> Option<String> {
    let opener = turbo_dom_browser_env::host_internals::auxiliary_context_opener(scope, context)?;
    let context = opener.get_creation_context(scope)?;
    let identity = turbo_dom_browser_env::host_internals::auxiliary_context_identity(context)?;
    if identity.closed {
        return None;
    }
    top_level_identity(scope, context).map(|identity| identity.0.to_string())
}

pub(super) fn lifecycle(scope: &mut v8::PinScope, context: v8::Local<v8::Context>, phase: &str) {
    if context.get_slot::<FrameParent>().is_some() || context.get_slot::<FrameRetired>().is_some() {
        return;
    }
    let Some(identity) = auxiliary_context_identity(context) else {
        return;
    };
    publish(scope);
    let target = identity.id.0.to_string();
    let event = match phase {
        "domcontentloaded" => AuxiliaryPageEvent::DomContentLoaded { target },
        "load" => AuxiliaryPageEvent::Load { target },
        _ => return,
    };
    event_log(&JsRuntime::op_state_from(scope).borrow()).push(event);
}

pub(super) fn publish(scope: &mut v8::PinScope) {
    let state = JsRuntime::op_state_from(scope);
    let mut state = state.borrow_mut();
    let network = state.borrow::<NetworkLog>().clone();
    let pages = state.borrow_mut::<Pages>();
    let mut created = Vec::new();
    let mut rejected = Vec::new();
    let event_log = pages.events.clone();
    let mut metadata_changed = false;
    pages.targets.retain(|id, target| {
        let context = v8::Local::new(scope, &target.context);
        if target.published {
            if let Some(info) = event_log.0.targets.lock().unwrap().get_mut(&id.to_string()) {
                let opener = opener_page(scope, context);
                if info.opener_page != opener {
                    info.opener_page = opener;
                    metadata_changed = true;
                }
            }
            return true;
        }
        let Some(identity) = auxiliary_context_identity(context) else {
            // The producer refused publication; retire host allocations without emitting a Page.
            rejected.push(target.context.clone());
            return false;
        };
        if identity.id.0 != *id {
            rejected.push(target.context.clone());
            return false;
        }
        if let Some(csp) = context.get_slot::<CspHandle>() {
            network.register_network_context(csp.id, &id.to_string());
        }
        target.published = true;
        created.push(AuxiliaryPageEvent::Created {
            target: id.to_string(),
            source: target.source.map(|source| source.0.to_string()),
            source_page: target.source_page.map(|source| source.0.to_string()),
            opener_page: opener_page(scope, context),
            url: target.url.clone(),
        });
        true
    });
    for event in created {
        pages.events.push(event);
    }
    if metadata_changed {
        event_log.record_change();
    }
    drop(state);
    for rejected in rejected {
        let context = v8::Local::new(scope, rejected);
        retire_auxiliary_context(scope, context);
        retire_owned_resources(scope, context);
    }
}

pub(super) fn event_log(state: &OpState) -> AuxiliaryPageEventLog {
    state.borrow::<Pages>().events.clone()
}

pub(super) fn queue_navigation(
    scope: &mut v8::PinScope,
    target: AuxiliaryContextId,
    source: AuxiliaryContextId,
    source_context: v8::Local<v8::Context>,
    url: String,
    replace: bool,
) -> bool {
    if auxiliary_context_identity(source_context).map(|identity| identity.id) != Some(source) {
        return false;
    }
    let Ok(snapshot) = auxiliary_navigation::SourceSnapshot::capture(scope, source_context, target)
    else {
        return false;
    };
    let state = JsRuntime::op_state_from(scope);
    let mut state = state.borrow_mut();
    let network = state.borrow::<NetworkLog>().clone();
    let pages = state.borrow_mut::<Pages>();
    if pages.events.is_closed()
        || pages.events.overflowed()
        || url.len() > pages.policy.max_navigation_url_bytes
    {
        return false;
    }
    // Repeated writes to one window replace its not-yet-started navigation.
    pages
        .pending
        .retain(|navigation| navigation.target != target);
    if pages.pending.len() >= pages.policy.max_pending_navigations {
        return false;
    }
    pages.pending.push_back(Navigation {
        target,
        url,
        noreferrer: false,
        replace,
        snapshot,
        activity: Some(network.begin_page_navigation(&target.0.to_string())),
    });
    auxiliary_navigation::cancel(&mut state, target.0);
    true
}

pub(super) fn next_navigation(state: &mut OpState) -> Option<Navigation> {
    state.borrow_mut::<Pages>().pending.pop_front()
}

pub(super) fn committed<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    previous: v8::Local<'s, v8::Context>,
    current: v8::Local<'s, v8::Context>,
    url: &str,
    isolated: bool,
) -> Result<(), String> {
    let identity = auxiliary_context_identity(previous).ok_or("Missing native Page identity")?;
    let commit = if isolated {
        commit_isolated_auxiliary_navigation
    } else {
        commit_auxiliary_navigation
    };
    if !commit(scope, previous, current, url) {
        return Err("Native auxiliary navigation identity commit was refused".into());
    }
    let state = JsRuntime::op_state_from(scope);
    let mut state = state.borrow_mut();
    let pages = state.borrow_mut::<Pages>();
    let target = pages
        .targets
        .get_mut(&identity.id.0)
        .ok_or("Page is no longer owned by this actor")?;
    target.context = v8::Global::new(scope, current);
    target.primary = false;
    target.snapshot = None;
    target.url = url.to_owned();
    pages.events.push(AuxiliaryPageEvent::Navigated {
        target: identity.id.0.to_string(),
        url: url.to_owned(),
        isolated,
        opener_page: opener_page(scope, current),
    });
    Ok(())
}

pub(super) fn close<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) -> bool {
    let Some(identity) = auxiliary_context_identity(context) else {
        return rollback_unpublished(scope, context);
    };
    publish(scope);
    if !retire_auxiliary_context(scope, context) {
        return false;
    }
    retire_owned_resources(scope, context);
    let state = JsRuntime::op_state_from(scope);
    let mut state = state.borrow_mut();
    let pages = state.borrow_mut::<Pages>();
    if pages.targets.remove(&identity.id.0).is_some() {
        pages
            .pending
            .retain(|navigation| navigation.target != identity.id);
        pages.events.push(AuxiliaryPageEvent::Closed {
            target: identity.id.0.to_string(),
        });
    }
    auxiliary_navigation::cancel(&mut state, identity.id.0);
    true
}

fn rollback_unpublished<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) -> bool {
    let state = JsRuntime::op_state_from(scope);
    let mut state = state.borrow_mut();
    let pages = state.borrow_mut::<Pages>();
    let owned = pages.targets.iter().find_map(|(id, target)| {
        (!target.published && v8::Local::new(scope, &target.context) == context).then_some(*id)
    });
    let Some(id) = owned else {
        return false;
    };
    pages.targets.remove(&id);
    pages.pending.retain(|navigation| navigation.target.0 != id);
    drop(state);
    retire_owned_resources(scope, context);
    true
}

pub(super) fn retire_owned_resources<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) {
    // The auxiliary's frame subtree is separate from every opener relationship.
    retire_frame_tree(scope, context);
    crate::shim_bindings::retire(context);
    crate::remote_handles::retire_context(scope, context);
    fetch_streams::retire_context(scope, context);
    network_transports::retire_context(scope, context);
    crate::message_ports::retire_context(scope, context);
    blob_urls::retire_context(scope, context);
    storage_events::retire(context);
    auxiliary_storage::retire(context);
    context.set_slot(Rc::new(FrameRetired));
}

pub(super) fn close_all(scope: &mut v8::PinScope) {
    let targets = JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<Pages>()
        .targets
        .values()
        .map(|target| target.context.clone())
        .collect::<Vec<_>>();
    for target in targets {
        close(scope, v8::Local::new(scope, target));
    }
}

pub(super) fn is_empty(state: &OpState) -> bool {
    state.borrow::<Pages>().targets.is_empty()
}

impl PageSession {
    pub fn auxiliary_event_log(&self) -> AuxiliaryPageEventLog {
        event_log(&self.rt.op_state().borrow())
    }

    pub fn auxiliary_root_id(&mut self) -> Result<u64, String> {
        if let Some(root) = self.auxiliary_event_log().root() {
            return root
                .parse()
                .map_err(|_| "Invalid native actor root identity".to_owned());
        }
        let _activation = self.rt.activation_key().enter();
        deno_core::scope!(scope, self.rt);
        let context = scope.get_current_context();
        auxiliary_context_identity(context)
            .map(|identity| identity.id.0)
            .ok_or_else(|| "Native auxiliary root has not been installed".to_owned())
    }

    pub fn read_target_engine(&mut self, target: u64, source: &str) -> Result<String, String> {
        let _activation = self.rt.activation_key().enter();
        let watchdog = Watchdog::start(self.rt.v8_isolate().thread_safe_handle(), self.budget_ms);
        let result = self.read_target_engine_active(target, source);
        let exceeded = watchdog.exceeded.load(std::sync::atomic::Ordering::Relaxed);
        drop(watchdog);
        self.rt.v8_isolate().cancel_terminate_execution();
        if exceeded {
            Err("Target engine read exceeded session budget".into())
        } else {
            result
        }
    }

    fn read_target_engine_active(&mut self, target: u64, source: &str) -> Result<String, String> {
        deno_core::scope!(scope, self.rt);
        let context = context(scope, AuxiliaryContextId(target))?;
        let scope = &mut v8::ContextScope::new(scope, context);
        v8::tc_scope!(let scope, scope);
        let value = crate::browser_env::run_engine_script(scope, source)
            .ok_or("Target engine script failed")?;
        if value.is_promise() {
            return Err("Target engine read must return a synchronous value".into());
        }
        let result = v8::json::stringify(scope, value).map(|json| json.to_rust_string_lossy(scope));
        publish(scope);
        result.ok_or_else(|| "Target engine result could not be serialized".to_owned())
    }

    pub fn serialize_target(&mut self, target: u64) -> Result<String, String> {
        let _activation = self.rt.activation_key().enter();
        deno_core::scope!(scope, self.rt);
        let context = context(scope, AuxiliaryContextId(target))?;
        let document = context
            .get_slot::<ActiveDocument>()
            .and_then(|document| document.0.to_local(scope))
            .ok_or("Target native document is unavailable")?;
        let _document = turbo_dom_browser_env::host_internals::enter_document_of(scope, document);
        turbo_dom_browser_env::host_internals::with_tree(|tree| {
            turbo_dom_parser::rtdom::serialize::serialize_inner(tree, tree.root())
        })
        .ok_or_else(|| "Target native document has retired".to_owned())
    }

    /// True means another owned Page still needs this isolate's actor.
    pub fn close_target(&mut self, target: u64) -> Result<bool, String> {
        let _activation = self.rt.activation_key().enter();
        deno_core::scope!(scope, self.rt);
        let context = match context(scope, AuxiliaryContextId(target)) {
            Ok(context) => context,
            Err(error) => {
                let state = JsRuntime::op_state_from(scope);
                let state = state.borrow();
                if event_log(&state)
                    .info(&target.to_string())
                    .is_some_and(|info| info.closed)
                {
                    return Ok(!is_empty(&state));
                }
                return Err(error);
            }
        };
        if !close(scope, context) {
            return Err("Target native retirement was refused".into());
        }
        Ok(!is_empty(&JsRuntime::op_state_from(scope).borrow()))
    }
}

pub(super) fn execute_target(
    rt: &mut JsRuntime,
    target: AuxiliaryContextId,
    source: &str,
    engine: bool,
) -> Result<v8::Global<v8::Value>, String> {
    deno_core::scope!(scope, rt);
    let context = context(scope, target)?;
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope, scope);
    let value = if engine {
        crate::browser_env::run_engine_script(scope, source)
    } else {
        v8::String::new(scope, source).and_then(|source| {
            v8::Script::compile(scope, source, None).and_then(|script| script.run(scope))
        })
    };
    let value = value.ok_or_else(|| {
        scope
            .exception()
            .map(|error| error.to_rust_string_lossy(scope))
            .unwrap_or_else(|| "Target script execution failed".to_owned())
    })?;
    let value = v8::Global::new(scope, value);
    publish(scope);
    Ok(value)
}

fn target_result(
    rt: &mut JsRuntime,
    target: AuxiliaryContextId,
    value: v8::Global<v8::Value>,
    json: bool,
) -> Result<String, String> {
    deno_core::scope!(scope, rt);
    let context = context(scope, target)?;
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope, scope);
    let value = v8::Local::new(scope, value);
    let result = if json {
        v8::json::stringify(scope, value)
            .map(|json| json.to_rust_string_lossy(scope))
            .ok_or_else(|| "Target result could not be serialized".to_owned())
    } else {
        Ok(value.to_rust_string_lossy(scope))
    };
    publish(scope);
    result
}

impl PageSession {
    pub async fn eval_target_expression(
        &mut self,
        target: u64,
        expression: &str,
        engine: bool,
        json: bool,
    ) -> Result<String, String> {
        let key = self.rt.activation_key();
        key.during(self.eval_target_expression_active(target, expression, engine, json))
            .await
    }

    async fn eval_target_expression_active(
        &mut self,
        target: u64,
        expression: &str,
        engine: bool,
        json: bool,
    ) -> Result<String, String> {
        let target = AuxiliaryContextId(target);
        let watchdog = Watchdog::start(self.rt.v8_isolate().thread_safe_handle(), self.budget_ms);
        let result =
            tokio::time::timeout(std::time::Duration::from_millis(self.budget_ms), async {
                let source = if engine {
                    format!(
                        "return (\n{}\n)",
                        expression.trim_end().trim_end_matches(';')
                    )
                } else {
                    expression.to_owned()
                };
                let value = execute_target(&mut self.rt, target, &source, engine)?;
                // Promise observers must belong to the live target realm: a backing
                // replacement can leave JsRuntime's original main context detached.
                let completion = {
                    deno_core::scope!(scope, self.rt);
                    let context = context(scope, target)?;
                    let scope = &mut v8::ContextScope::new(scope, context);
                    JsRuntime::scoped_resolve(scope, value.clone())
                };
                execute_target(&mut self.rt, target, "__tsNs.resetTimerBudget();", true)?;
                drain_event_loop_until_completion(&mut self.rt, None, Some(&value)).await?;
                let value = self
                    .rt
                    .with_event_loop_promise(completion, Default::default())
                    .await
                    .map_err(|error| error.to_string())?;
                target_result(&mut self.rt, target, value, json)
            })
            .await;
        let exceeded = watchdog.exceeded.load(std::sync::atomic::Ordering::Relaxed);
        drop(watchdog);
        self.rt.v8_isolate().cancel_terminate_execution();
        if exceeded {
            return Err("Target expression exceeded session execution budget".into());
        }
        result.map_err(|_| "Target expression exceeded session execution budget".to_owned())?
    }

    pub async fn eval_target_engine(
        &mut self,
        target: u64,
        source: &str,
        collect: bool,
    ) -> Result<String, String> {
        let expression = format!("(() => {{ {source}\n; return globalThis.__RESULT == null ? '' : String(globalThis.__RESULT); }})()");
        let result = self
            .eval_target_expression(target, &expression, true, false)
            .await?;
        if !collect {
            let hydration = "__tsNs.hydrate(300, 200000, true)";
            self.eval_target_expression(target, hydration, true, false)
                .await?;
        }
        Ok(result)
    }
}

pub(super) fn in_target_tree<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    mut context: v8::Local<'s, v8::Context>,
    root: v8::Local<'s, v8::Context>,
) -> bool {
    loop {
        if context == root {
            return true;
        }
        let Some(parent) = context
            .get_slot::<FrameParent>()
            .and_then(|parent| parent.0.to_local(scope))
        else {
            return false;
        };
        context = parent;
    }
}

fn document_version(
    scope: &mut v8::PinScope,
    context: v8::Local<v8::Context>,
) -> Option<(u64, u64)> {
    use turbo_dom_browser_env::host_internals::{document_id_of, enter_document_of, with_tree};
    let document = context.get_slot::<ActiveDocument>()?.0.to_local(scope)?;
    let id = document_id_of(scope, document);
    let _document = enter_document_of(scope, document);
    Some((id, with_tree(|tree| tree.version)?))
}

fn target_snapshot<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    root: v8::Local<'s, v8::Context>,
) -> Option<TargetSnapshot> {
    let state = JsRuntime::op_state_from(scope);
    let revision = state.borrow().borrow::<driver_sync::Revision>().0;
    let frames = state.borrow().borrow::<FrameContexts>().contexts.clone();
    let mut documents = vec![document_version(scope, root)?];
    for frame in frames {
        let frame = v8::Local::new(scope, frame);
        if frame.get_slot::<FrameRetired>().is_none() && in_target_tree(scope, frame, root) {
            documents.push(document_version(scope, frame)?);
        }
    }
    Some(TargetSnapshot {
        revision,
        documents,
    })
}

impl PageSession {
    pub fn target_is_primary(&self, target: u64) -> Result<bool, String> {
        self.rt
            .op_state()
            .borrow()
            .borrow::<Pages>()
            .targets
            .get(&target)
            .map(|target| target.primary)
            .ok_or_else(|| "Target page is closed".to_owned())
    }

    pub fn serialize_target_for_driver(&mut self, target: u64) -> Result<String, String> {
        let html = self.serialize_target(target)?;
        let cookies = self.cookies();
        let _activation = self.rt.activation_key().enter();
        deno_core::scope!(scope, self.rt);
        let root = context(scope, AuxiliaryContextId(target))?;
        let snapshot = target_snapshot(scope, root);
        let state = JsRuntime::op_state_from(scope);
        let mut state = state.borrow_mut();
        let target = state
            .borrow_mut::<Pages>()
            .targets
            .get_mut(&target)
            .ok_or("Target page is closed")?;
        target.snapshot = snapshot;
        target.driver_cookies = Some(cookies);
        Ok(html)
    }

    pub fn cookies_target_for_driver(&mut self, target: u64) -> Result<String, String> {
        let cookies = self.cookies();
        let state = self.rt.op_state();
        let mut state = state.borrow_mut();
        let target = state
            .borrow_mut::<Pages>()
            .targets
            .get_mut(&target)
            .ok_or("Target page is closed")?;
        target.driver_cookies = Some(cookies.clone());
        Ok(cookies)
    }

    pub async fn eval_target_expression_tracked(
        &mut self,
        target: u64,
        expression: &str,
    ) -> Result<String, String> {
        let value = self
            .eval_target_expression(target, expression, false, false)
            .await?;
        let cookies = self.cookies();
        let messages = has_pending_frame_work(&self.rt);
        let _activation = self.rt.activation_key().enter();
        deno_core::scope!(scope, self.rt);
        let root = context(scope, AuxiliaryContextId(target))?;
        let snapshot = target_snapshot(scope, root);
        let state = JsRuntime::op_state_from(scope);
        let mut state = state.borrow_mut();
        let network_events = driver_sync::pending_events(&mut state);
        let stored = state
            .borrow::<Pages>()
            .targets
            .get(&target)
            .ok_or("Target page is closed")?;
        let needs_sync = snapshot.is_none()
            || snapshot != stored.snapshot
            || stored.driver_cookies.as_ref() != Some(&cookies)
            || messages
            || !state.borrow::<Pages>().pending.is_empty();
        let events = network_events || state.borrow::<Pages>().events.has_events();
        Ok(serde_json::json!({"value":value,"needsSync":needs_sync,"events":events}).to_string())
    }

    pub async fn eval_target_script(
        &mut self,
        target: u64,
        source: &str,
    ) -> Result<String, String> {
        let key = self.rt.activation_key();
        key.during(async {
            let watchdog =
                Watchdog::start(self.rt.v8_isolate().thread_safe_handle(), self.budget_ms);
            let result = execute_target(&mut self.rt, AuxiliaryContextId(target), source, false);
            let exceeded = watchdog.exceeded.load(Ordering::Relaxed);
            drop(watchdog);
            self.rt.v8_isolate().cancel_terminate_execution();
            result?;
            if exceeded {
                return Err("Target script exceeded session budget".into());
            }
            self.eval_target_engine(target, "", false).await
        })
        .await
    }
}

impl PageSession {
    pub fn has_owned_pages(&self) -> bool {
        let state = self.rt.op_state();
        let state = state.borrow();
        if !state.borrow::<Pages>().enabled {
            return !self.closed;
        }
        !is_empty(&state)
    }

    pub async fn poll_owned_work(&mut self) -> Result<String, String> {
        if !self.rt.op_state().borrow().borrow::<Pages>().enabled {
            return self.eval("").await;
        }
        if self.has_pending_auxiliary_work() {
            return self
                .service_auxiliary_navigation()
                .await
                .map(|_| String::new());
        }
        let live = self
            .auxiliary_event_log()
            .pages()
            .into_iter()
            .find(|page| !page.closed);
        let Some(live) = live else {
            return Ok(String::new());
        };
        let target = live
            .target
            .parse::<u64>()
            .map_err(|_| "Invalid native target identity")?;
        if self.target_is_primary(target)? {
            self.eval("").await
        } else {
            self.eval_target_engine(target, "", false).await
        }
    }
}

impl PageSession {
    pub async fn resize_target(
        &mut self,
        target: u64,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        {
            let _activation = self.rt.activation_key().enter();
            deno_core::scope!(scope, self.rt);
            let context = context(scope, AuxiliaryContextId(target))?;
            let scope = &mut v8::ContextScope::new(scope, context);
            crate::live_layout_backend::set_viewport(scope, width, height)?;
        }
        self.eval_target_engine(target, "", false).await.map(|_| ())
    }

    pub async fn set_target_content(
        &mut self,
        target: u64,
        html: &str,
        wait_until: &str,
        timeout_ms: Option<u64>,
    ) -> Result<String, String> {
        if !matches!(wait_until, "load" | "domcontentloaded" | "networkidle") {
            return Err(format!("Unsupported setContent waitUntil: {wait_until}"));
        }
        let budget = timeout_ms
            .filter(|value| *value > 0)
            .unwrap_or(self.budget_ms);
        let watchdog = Watchdog::start(self.rt.v8_isolate().thread_safe_handle(), budget);
        let key = self.rt.activation_key();
        let operation = key.during(self.set_target_content_active(target, html, wait_until));
        let result =
            tokio::time::timeout(std::time::Duration::from_millis(budget), operation).await;
        let exceeded = watchdog.exceeded.load(std::sync::atomic::Ordering::Relaxed);
        drop(watchdog);
        self.rt.v8_isolate().cancel_terminate_execution();
        if exceeded || result.is_err() {
            return Err(format!("setContent timeout after {budget}ms"));
        }
        result.unwrap()?;
        self.serialize_target_for_driver(target)
    }

    async fn set_target_content_active(
        &mut self,
        target: u64,
        html: &str,
        wait_until: &str,
    ) -> Result<(), String> {
        let content = {
            deno_core::scope!(scope, self.rt);
            let value = v8::String::new(scope, html).ok_or("Document content allocation failed")?;
            v8::Global::<v8::Value>::new(scope, v8::Local::<v8::Value>::from(value))
        };
        let target = AuxiliaryContextId(target);
        target_call(&mut self.rt, target, "resetTimerBudget", &[])?;
        target_call(&mut self.rt, target, "documentLoading", &[])?;
        target_call(
            &mut self.rt,
            target,
            "replaceDocumentContent",
            &[HostArg::Value(content)],
        )?;
        self.eval_target_expression(target.0, "__tsNs.hydrate(300,200000,true)", true, false)
            .await?;
        target_call(&mut self.rt, target, "dispatchDocumentInteractive", &[])?;
        if wait_until == "domcontentloaded" {
            target_call(&mut self.rt, target, "scheduleContentLoad", &[])?;
            return Ok(());
        }
        self.eval_target_expression(target.0, "__tsNs.waitForDocumentResources()", true, false)
            .await?;
        target_call(&mut self.rt, target, "dispatchDocumentComplete", &[])?;
        if wait_until == "networkidle" {
            drain_to_quiescence(&mut self.rt).await?;
        }
        Ok(())
    }
}

impl PageSession {
    pub(super) fn publish_auxiliary_state(&mut self) {
        let _activation = self.rt.activation_key().enter();
        deno_core::scope!(scope, self.rt);
        publish(scope);
    }
}

impl PageSession {
    pub fn configure_auxiliary_bootstrap(
        &mut self,
        bootstrap: AuxiliaryBootstrap,
    ) -> Result<(), String> {
        self.rt
            .op_state()
            .borrow_mut()
            .borrow_mut::<Pages>()
            .bootstrap = bootstrap;
        Ok(())
    }

    pub fn configure_target_bootstrap(
        &mut self,
        target: u64,
        bootstrap: AuxiliaryBootstrap,
    ) -> Result<(), String> {
        let state = self.rt.op_state();
        let mut state = state.borrow_mut();
        let target = state
            .borrow_mut::<Pages>()
            .targets
            .get_mut(&target)
            .ok_or("Target page is closed")?;
        target.page_bootstrap = bootstrap;
        Ok(())
    }
}

pub(super) fn bootstrap_for_context<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) -> AuxiliaryBootstrap {
    let target = top_level_identity(scope, context);
    let state = JsRuntime::op_state_from(scope);
    let state = state.borrow();
    let pages = state.borrow::<Pages>();
    let Some(target) = target else {
        let frames = state.borrow::<FrameContexts>();
        return AuxiliaryBootstrap {
            init_script: frames.init_script.clone(),
            engine_init_script: frames.engine_init_script.clone(),
        };
    };
    let page = pages
        .targets
        .get(&target.0)
        .map(|target| target.page_bootstrap.clone())
        .unwrap_or_default();
    AuxiliaryBootstrap {
        init_script: format!("{}\n{}", pages.bootstrap.init_script, page.init_script),
        engine_init_script: format!(
            "{}\n{}",
            pages.bootstrap.engine_init_script, page.engine_init_script
        ),
    }
}

/// Resolve lifecycle capabilities in the actual target realm; guest globals cannot replace them.
pub(super) fn target_call(
    rt: &mut JsRuntime,
    target: AuxiliaryContextId,
    name: &str,
    args: &[HostArg],
) -> Result<v8::Global<v8::Value>, String> {
    deno_core::scope!(scope, rt);
    let context = context(scope, target)?;
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope, scope);
    let values: Vec<v8::Local<v8::Value>> = args
        .iter()
        .map(|arg| match arg {
            HostArg::Number(value) => v8::Number::new(scope, *value).into(),
            HostArg::Bool(value) => v8::Boolean::new(scope, *value).into(),
            HostArg::Null => v8::null(scope).into(),
            HostArg::Value(value) => v8::Local::new(scope, value),
        })
        .collect();
    if let Some(value) = call_frame_helper(scope, name, &values) {
        return Ok(v8::Global::new(scope, value));
    }
    Err(scope
        .exception()
        .map(|error| error.to_rust_string_lossy(scope))
        .unwrap_or_else(|| format!("Target capability {name} failed")))
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    fn created(target: &str) -> AuxiliaryPageEvent {
        AuxiliaryPageEvent::Created {
            target: target.into(),
            source: Some("1".into()),
            source_page: Some("1".into()),
            opener_page: None,
            url: "about:blank".into(),
        }
    }

    #[tokio::test]
    async fn observed_revision_cannot_miss_metadata_only_commit() {
        let log = AuxiliaryPageEventLog::default();
        let observed = log.revision();
        *log.0.root.lock().unwrap() = Some("7".into());
        log.record_change();
        let closed = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            log.wait_for_revision(observed),
        )
        .await
        .expect("committed root metadata must wake the waiter");
        assert!(!closed);
        assert_eq!(log.root().as_deref(), Some("7"));
        assert!(log.take().is_empty());
    }

    #[test]
    fn retirement_boundary_closes_each_known_target_once() {
        let log = AuxiliaryPageEventLog::default();
        log.push(created("2"));
        log.push(created("3"));
        log.push(AuxiliaryPageEvent::Closed { target: "2".into() });
        log.take();
        log.close();
        log.close();
        assert!(log.is_closed());
        assert!(log.pages().iter().all(|page| page.closed));
        let events = log.take();
        assert!(matches!(&events[..], [AuxiliaryPageEvent::Closed { target }] if target == "3"));
    }

    #[test]
    fn queue_overflow_is_bounded_and_explicit() {
        let log = AuxiliaryPageEventLog::default();
        log.set_limit(2);
        log.push(created("2"));
        log.push(AuxiliaryPageEvent::Load { target: "2".into() });
        log.push(AuxiliaryPageEvent::DomContentLoaded { target: "2".into() });
        assert!(log.overflowed());
        assert_eq!(log.queued_len(), 2);
        assert!(matches!(
            log.take().last(),
            Some(AuxiliaryPageEvent::Overflow { .. })
        ));
    }
}
