//! Actor-owned navigation jobs survive cancellation of an individual driver turn.
//! HTTP futures contain native request snapshots, never guest contexts or callbacks.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use turbo_dom_browser_env::host_internals::{self as binding, AuxiliaryContextId};
use turbo_surf_core::net::FetchResult;

#[derive(Clone)]
pub(super) struct SourceSnapshot {
    source: String,
    policy: crate::csp::Policy,
    referrer_policy: turbo_surf_core::referrer_policy::ReferrerPolicy,
    user_activation: bool,
    request: RequestPlan,
}
impl SourceSnapshot {
    pub(super) fn capture(
        scope: &mut v8::PinScope,
        source: v8::Local<v8::Context>,
        _target: AuxiliaryContextId,
    ) -> Result<Self, String> {
        let url = source
            .get_slot::<FrameDocumentLocation>()
            .map(|url| url.0.clone())
            .or_else(|| {
                source
                    .get_slot::<FrameDocumentUrl>()
                    .map(|url| url.0.clone())
            })
            .unwrap_or_else(|| {
                JsRuntime::op_state_from(scope)
                    .borrow()
                    .borrow::<Base>()
                    .0
                    .clone()
            });
        let policy = source
            .get_slot::<CspHandle>()
            .ok_or("Navigation source policy unavailable")?;
        let policy = policy.policy.borrow();
        Ok(Self {
            source: url,
            policy: policy.active.clone(),
            referrer_policy: policy.referrer_policy,
            user_activation: host_user_activation(source),
            request: RequestPlan::default(),
        })
    }
}

#[derive(Clone, Default)]
struct RequestPlan {
    method: Option<String>,
    body: Option<Vec<u8>>,
    headers: BTreeMap<String, String>,
    referer: Option<String>,
    form_action: bool,
}
#[derive(Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct NavigationOptions {
    method: Option<String>,
    body: Option<String>,
    body_bytes: Option<Vec<u8>>,
    headers: BTreeMap<String, String>,
    referer: Option<String>,
    timeout_ms: Option<u64>,
    wait_until: Option<String>,
    content_type: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CoopValue {
    UnsafeNone,
    SameOrigin,
    SameOriginAllowPopups,
}
#[derive(Clone)]
pub(super) struct OpenerPolicy {
    value: CoopValue,
    origin: String,
}
impl OpenerPolicy {
    fn response(headers: &[(String, String)], url: &str) -> Self {
        let value = crate::fetch_cors::header(headers, "cross-origin-opener-policy")
            .and_then(|value| {
                sfv::Parser::new(value)
                    .with_version(sfv::Version::Rfc8941)
                    .parse::<sfv::Item>()
                    .ok()
            })
            .and_then(|item| {
                item.bare_item
                    .as_token()
                    .map(|token| token.as_str().to_owned())
            });
        let value = match value.as_deref() {
            Some("same-origin") => CoopValue::SameOrigin,
            Some("same-origin-allow-popups") => CoopValue::SameOriginAllowPopups,
            _ => CoopValue::UnsafeNone,
        };
        Self {
            value,
            origin: url::Url::parse(url)
                .map(|url| url.origin().ascii_serialization())
                .unwrap_or_else(|_| "null".into()),
        }
    }
}

pub(super) fn install_opener_policy(
    context: v8::Local<v8::Context>,
    headers: &[(String, String)],
    url: &str,
) {
    context.set_slot(Rc::new(OpenerPolicy::response(headers, url)));
}

pub(super) fn inherit_opener_policy(
    scope: &mut v8::PinScope,
    target: v8::Local<v8::Context>,
    source: v8::Local<v8::Context>,
) {
    let policy = source
        .get_slot::<OpenerPolicy>()
        .map(|policy| (*policy).clone())
        .unwrap_or_else(|| OpenerPolicy {
            value: CoopValue::UnsafeNone,
            origin: window_origin(scope, source),
        });
    target.set_slot(Rc::new(policy));
}

fn needs_group_switch(previous: Option<&OpenerPolicy>, next: &OpenerPolicy) -> bool {
    let Some(previous) = previous else {
        return next.value != CoopValue::UnsafeNone;
    };
    if previous.value == CoopValue::UnsafeNone && next.value == CoopValue::UnsafeNone {
        return false;
    }
    previous.value != next.value || previous.origin != next.origin
}

#[derive(Clone, Copy, Default)]
pub(super) enum HistoryMode {
    #[default]
    Push,
    Replace,
    Traverse(usize),
}

struct Fetched {
    response: FetchResult,
    metadata: crate::DocumentResponseMetadata,
    request_id: Option<u64>,
}
type FetchFuture = Pin<Box<dyn Future<Output = Result<Fetched, String>>>>;
struct Job {
    generation: u64,
    future: FetchFuture,
    history: HistoryMode,
}
struct Loading {
    generation: u64,
    promise: v8::Global<v8::Value>,
    phase: LoadPhase,
}
#[derive(Clone, Copy)]
enum LoadPhase {
    Parsing,
    Resources,
}
#[derive(Default)]
struct History {
    entries: Vec<String>,
    index: usize,
}
#[derive(Default)]
pub(super) struct State {
    sequence: u64,
    jobs: BTreeMap<u64, Job>,
    loading: BTreeMap<u64, Loading>,
    results: BTreeMap<u64, Result<serde_json::Value, String>>,
    waiters: BTreeSet<u64>,
    history: BTreeMap<u64, History>,
}

fn state(state: &mut OpState) -> &mut State {
    if !state.has::<State>() {
        state.put(State::default());
    }
    state.borrow_mut::<State>()
}

pub(super) fn pending(state: &OpState) -> bool {
    state
        .try_borrow::<State>()
        .is_some_and(|state| !state.jobs.is_empty() || !state.loading.is_empty())
}

pub(super) fn cancel(state: &mut OpState, target: u64) {
    let state = self::state(state);
    if let Some(job) = state.jobs.remove(&target) {
        if state.waiters.contains(&job.generation) {
            state.results.insert(
                job.generation,
                Err("Navigation target closed or superseded".into()),
            );
        }
    }
    state.loading.remove(&target);
}

fn fetch_job(
    state: &OpState,
    navigation: auxiliary_pages::Navigation,
    frame: RequestFrameMetadata,
    budget_ms: u64,
) -> FetchFuture {
    let jar = state.borrow::<Jar>().clone();
    let ua = state.borrow::<Ua>().0.clone();
    let hints = state.borrow::<ClientHints>().clone();
    let storage = state.borrow::<BrowserStorage>().clone();
    let log = state.borrow::<NetworkLog>().clone();
    Box::pin(async move {
        let mut local = jar.borrow().fork_for_request();
        let mut request = RequestContext::navigation(
            Some(navigation.snapshot.source.clone()),
            navigation.snapshot.user_activation,
        );
        request.referrer = navigation.snapshot.request.referer.clone();
        request.referrer_policy = if navigation.noreferrer {
            turbo_surf_core::referrer_policy::ReferrerPolicy::NoReferrer
        } else {
            navigation.snapshot.referrer_policy
        };
        let metadata = std::sync::Mutex::new(crate::DocumentResponseMetadata::default());
        let response_policy =
            |request: &RequestContext, target: &str, _headers: &[(String, String)]| {
                *metadata.lock().unwrap() =
                    crate::DocumentResponseMetadata::from_request(0, request, target);
                true
            };
        // General navigations are not connect-src resources. The source snapshot is
        // retained for the submitting form-action gate when a form plan supplies one.
        let request_policy = |target: &str, redirected: bool| {
            !navigation.snapshot.request.form_action
                || navigation
                    .snapshot
                    .policy
                    .allows_request("form-action", target, "", redirected)
        };
        let trace = std::sync::Arc::new(FetchResponseTrace::default());
        let context = FetchEventContext {
            frame,
            navigation: true,
            resource_type: "document".into(),
        };
        let mut completion = RequestCompletion {
            log: log.clone(),
            trace: trace.clone(),
            context: serde_json::to_value(&context).unwrap(),
            url: navigation.url.clone(),
            method: navigation
                .snapshot
                .request
                .method
                .clone()
                .unwrap_or_else(|| "GET".into()),
            finished: false,
        };
        let observer = |hop| trace.observe(&log, &context, hop);
        let cache = storage.http_cache();
        let effects = storage.response_storage_effects();
        let mut headers = navigation.snapshot.request.headers.clone();
        if !ua.is_empty() {
            headers.insert("user-agent".into(), ua);
        }
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(budget_ms),
            fetch_html(
                &navigation.url,
                FetchOptions {
                    headers,
                    method: navigation.snapshot.request.method.clone(),
                    body_bytes: navigation.snapshot.request.body.clone(),
                    request_policy: Some(&request_policy),
                    context: Some(request),
                    jar: Some(&mut local),
                    client_hints: Some(&hints),
                    http_cache: Some(&cache),
                    response_side_effects: Some(effects.as_ref()),
                    response_policy: Some(&response_policy),
                    response_observer: Some(&observer),
                    max_redirects: Some(20),
                    now: cookie_now(),
                    ..Default::default()
                },
            ),
        )
        .await;
        jar.borrow_mut().merge_request_changes(local);
        let response = result
            .map_err(|_| "Native target navigation exceeded session budget".to_owned())?
            .map_err(|error| format!("Native target navigation failed: {error}"))?;
        let request_id = trace.last_id();
        if let Some(id) = request_id {
            log.push(
                serde_json::json!({"event":"responsebody","id":id,"bodyBytes":response.body_bytes,
                "body":response.html,"bodyIncomplete":false,"error":null}),
            );
        }
        completion.finished = true;
        let mut metadata = metadata.into_inner().unwrap();
        metadata.status = response.status;
        Ok(Fetched {
            response,
            metadata,
            request_id,
        })
    })
}

fn start(
    rt: &mut JsRuntime,
    navigation: auxiliary_pages::Navigation,
    mode: HistoryMode,
    budget_ms: u64,
) -> Result<u64, String> {
    let _activation = rt.activation_key().enter();
    let target = navigation.target;
    let (frame, current_url) = {
        deno_core::scope!(scope, rt);
        let context = auxiliary_pages::context(scope, target)?;
        let url = context
            .get_slot::<FrameDocumentLocation>()
            .map(|url| url.0.clone())
            .unwrap_or_else(|| "about:blank".into());
        (request_frame_metadata(scope, context, None), url)
    };
    let shared = rt.op_state();
    let mut shared = shared.borrow_mut();
    let limit = shared
        .borrow::<auxiliary_pages::Pages>()
        .policy
        .max_history_entries;
    let history_len = shared
        .try_borrow::<State>()
        .and_then(|state| state.history.get(&target.0))
        .map_or(1, |history| history.index + 1);
    if limit == 0
        || (matches!(mode, HistoryMode::Push)
            && current_url != "about:blank"
            && history_len >= limit)
    {
        return Err("Native target session history limit exceeded".into());
    }
    cancel(&mut shared, target.0);
    let future = fetch_job(&shared, navigation, frame, budget_ms);
    let state = self::state(&mut shared);
    if !state.history.contains_key(&target.0) {
        state.history.insert(
            target.0,
            History {
                entries: vec![current_url.clone()],
                index: 0,
            },
        );
    }
    let mode = if current_url == "about:blank" && matches!(mode, HistoryMode::Push) {
        HistoryMode::Replace
    } else {
        mode
    };
    state.sequence = state
        .sequence
        .checked_add(1)
        .ok_or("Navigation identity exhausted")?;
    let generation = state.sequence;
    state.jobs.insert(
        target.0,
        Job {
            generation,
            future,
            history: mode,
        },
    );
    Ok(generation)
}

fn poll_fetches(
    state: &mut State,
    cx: &mut Context<'_>,
) -> Vec<(u64, Job, Result<Fetched, String>)> {
    let ready = state
        .jobs
        .iter_mut()
        .filter_map(|(target, job)| match job.future.as_mut().poll(cx) {
            Poll::Pending => None,
            Poll::Ready(result) => Some((*target, result)),
        })
        .collect::<Vec<_>>();
    ready
        .into_iter()
        .map(|(target, result)| (target, state.jobs.remove(&target).unwrap(), result))
        .collect()
}

fn update_history(state: &mut State, target: u64, url: &str, mode: HistoryMode) {
    let history = state.history.entry(target).or_default();
    if history.entries.is_empty() {
        history.entries.push(url.into());
        history.index = 0;
        return;
    }
    match mode {
        HistoryMode::Push => {
            history.entries.truncate(history.index + 1);
            history.entries.push(url.into());
            history.index += 1;
        }
        HistoryMode::Replace => history.entries[history.index] = url.into(),
        HistoryMode::Traverse(index) => {
            history.index = index;
            history.entries[index] = url.into();
        }
    }
}

fn install_response<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    previous: v8::Local<'s, v8::Context>,
    fetched: &Fetched,
) -> Result<v8::Global<v8::Context>, String> {
    let response = &fetched.response;
    let next_policy = OpenerPolicy::response(&response.headers, &response.final_url);
    let isolated = needs_group_switch(previous.get_slot::<OpenerPolicy>().as_deref(), &next_policy);
    let current = binding::create_auxiliary_window_context(scope, (!isolated).then_some(previous));
    current.set_slot(Rc::new(next_policy));
    let result = install_response_backing(scope, previous, current, fetched, isolated);
    if let Err(error) = &result {
        // DetachGlobal cannot restore the old lexical realm. A failed backing
        // installation retires this real Page; its peers retain their actor.
        report_scope_error(scope, previous, error);
        if binding::auxiliary_context_identity(current).is_some() {
            auxiliary_pages::close(scope, current);
        } else {
            auxiliary_pages::close(scope, previous);
        }
        auxiliary_pages::retire_owned_resources(scope, current);
        auxiliary_pages::retire_owned_resources(scope, previous);
    }
    result
}

fn install_response_backing<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    previous: v8::Local<'s, v8::Context>,
    current: v8::Local<'s, v8::Context>,
    fetched: &Fetched,
    isolated: bool,
) -> Result<v8::Global<v8::Context>, String> {
    let response = &fetched.response;
    current.set_slot(Rc::new(FrameContextMarker));
    let core = previous
        .get_slot::<RuntimeCore>()
        .and_then(|core| core.0.to_local(scope))
        .ok_or("Native auxiliary core unavailable")?;
    for slot in [
        deno_core::CONTEXT_STATE_SLOT_INDEX,
        deno_core::MODULE_MAP_SLOT_INDEX,
    ] {
        let pointer = previous.get_aligned_pointer_from_embedder_data(slot);
        // SAFETY: only the existing actor owns/drops these deno_core pointers.
        unsafe {
            current.set_aligned_pointer_in_embedder_data(slot, pointer);
        }
    }
    let bootstrap = auxiliary_pages::bootstrap_for_context(scope, previous);
    let scope = &mut v8::ContextScope::new(scope, current);
    let policy = CspHandle::new(&response.final_url, &response.headers, &response.html);
    policy.response_metadata.replace(fetched.metadata.clone());
    JsRuntime::op_state_from(scope)
        .borrow_mut()
        .borrow_mut::<CspContexts>()
        .0
        .insert(policy.id, policy.clone());
    current.set_slot(Rc::new(policy.clone()));
    set_origin_token(scope, current, &response.final_url);
    install_csp_sandbox_flags(scope, current, &policy);
    crate::browser_env::defer_key_order(scope);
    let units = response.html.encode_utf16().collect::<Vec<_>>();
    binding::initialize_auxiliary_utf16_source_positions(scope, &units);
    install_css_authorization(scope).map_err(|e| e.to_string())?;
    let facts = DocumentFacts::response(scope, current, &response.final_url, &response.headers);
    install_document_facts(scope, facts);
    current.set_slot(Rc::new(FrameDocumentLocation(response.final_url.clone())));
    current.set_slot(Rc::new(FrameDocumentUrl(response.final_url.clone())));
    current.set_slot(Rc::new(FrameScriptPolicy::from_html_units(
        &units,
        &response.final_url,
        &policy.policy.borrow().initial_base,
    )));
    current.set_slot(Rc::new(BootstrapDepth(0)));
    let origin = window_origin(scope, current);
    policy.policy.borrow_mut().permissions_policy =
        crate::permissions_policy::PermissionsPolicy::from_headers(&origin, &response.headers);
    let allowed =
        cross_origin_isolated_allowed(&policy, &response.headers, &response.final_url, &origin);
    current.set_slot(Rc::new(SharedMemoryCapability {
        allowed,
        group: policy.id,
        origin,
    }));
    frame_modules::register_frame_modules(scope, &response.final_url).map_err(|e| e.to_string())?;
    let namespace =
        crate::browser_env::engine_namespace(scope).ok_or("Auxiliary namespace unavailable")?;
    let key = v8::String::new(scope, "isAuxiliary").unwrap();
    let yes = v8::Boolean::new(scope, true);
    namespace.set(scope, key.into(), yes.into());
    current.set_slot(Rc::new(FrameEnvironment(
        crate::context_ownership::object(scope, namespace)
            .ok_or("Auxiliary namespace ownership interrupted")?,
    )));
    auxiliary_host::install_hyperlink_handler(current);
    binding::set_auxiliary_ready_host(current, Some(auxiliary_host::ready));
    binding::install_auxiliary_window_hooks(current, auxiliary_host::hooks());
    if previous
        .remove_slot::<FailAuxiliaryBackingForTesting>()
        .is_some()
    {
        return Err("Injected native auxiliary backing initialization failure".into());
    }
    // Commit identity before ENV can initialize actual nested frame sources.
    auxiliary_pages::committed(scope, previous, current, &response.final_url, isolated)?;
    install_environment(scope, core.into())?;
    let secure = secure_context(scope);
    crate::crypto_backend::install(scope, core, secure)?;
    crate::audio_backend::install(scope, core)?;
    crate::geometry_backend::install(scope, core)?;
    window_proxy::install_location(scope, &response.final_url);
    binding::capture_auxiliary_window_exception(scope, current);
    crate::browser_env::finish_key_order(scope);
    run_navigation_init(scope, current, &bootstrap)?;
    auxiliary_pages::retire_owned_resources(scope, previous);
    if let Some(retired) = previous.get_slot::<CspHandle>() {
        JsRuntime::op_state_from(scope)
            .borrow_mut()
            .borrow_mut::<CspContexts>()
            .0
            .remove(&retired.id);
    }
    Ok(v8::Global::new(scope, current))
}

fn attachment(response: &FetchResult) -> bool {
    response.headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("content-disposition")
            && value
                .split(';')
                .next()
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("attachment"))
    })
}

fn receipt(fetched: &Fetched) -> serde_json::Value {
    let response = &fetched.response;
    serde_json::json!({"finalUrl":response.final_url,"status":response.status,
        "statusText":response.status_text,"headers":response.headers,"referrer":fetched.metadata.referrer,
        "requestId":fetched.request_id,"html":response.html,"bodyBytes":response.body_bytes,
        "attachment":attachment(response)})
}

fn commit_fetched(
    rt: &mut JsRuntime,
    target: u64,
    job: &Job,
    fetched: Fetched,
) -> Result<serde_json::Value, String> {
    let receipt = receipt(&fetched);
    if attachment(&fetched.response) {
        return Ok(receipt);
    }
    {
        deno_core::scope!(scope, rt);
        let previous = auxiliary_pages::context(scope, AuxiliaryContextId(target))?;
        install_response(scope, previous, &fetched)?;
    }
    let id = AuxiliaryContextId(target);
    auxiliary_pages::target_call(rt, id, "documentLoading", &[])?;
    let promise =
        auxiliary_pages::execute_target(rt, id, "return __tsNs.hydrate(300,200000,false)", true)?;
    let shared = rt.op_state();
    let mut shared = shared.borrow_mut();
    let state = self::state(&mut shared);
    update_history(state, target, &fetched.response.final_url, job.history);
    state.loading.insert(
        target,
        Loading {
            generation: job.generation,
            promise,
            phase: LoadPhase::Parsing,
        },
    );
    Ok(receipt)
}

fn promise_result(
    rt: &mut JsRuntime,
    target: u64,
    value: &v8::Global<v8::Value>,
) -> Result<bool, String> {
    deno_core::scope!(scope, rt);
    let context = auxiliary_pages::context(scope, AuxiliaryContextId(target))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    let value = v8::Local::new(scope, value);
    let Ok(promise) = v8::Local::<v8::Promise>::try_from(value) else {
        return Ok(true);
    };
    match promise.state() {
        v8::PromiseState::Pending => Ok(false),
        v8::PromiseState::Fulfilled => Ok(true),
        v8::PromiseState::Rejected => Err(promise.result(scope).to_rust_string_lossy(scope)),
    }
}

fn advance_loads(rt: &mut JsRuntime) -> Result<(), String> {
    let pending = {
        let shared = rt.op_state();
        let mut shared = shared.borrow_mut();
        self::state(&mut shared)
            .loading
            .iter()
            .map(|(id, load)| (*id, load.promise.clone(), load.phase))
            .collect::<Vec<_>>()
    };
    for (target, promise, phase) in pending {
        match promise_result(rt, target, &promise) {
            Ok(false) => continue,
            Ok(true) => (),
            Err(error) => {
                report_target_error(rt, target, &error);
                let shared = rt.op_state();
                self::state(&mut shared.borrow_mut())
                    .loading
                    .remove(&target);
                continue;
            }
        }
        let id = AuxiliaryContextId(target);
        let next = match phase {
            LoadPhase::Parsing => {
                auxiliary_pages::target_call(rt, id, "dispatchDocumentInteractive", &[])?;
                Some(auxiliary_pages::execute_target(
                    rt,
                    id,
                    "return __tsNs.waitForDocumentResources()",
                    true,
                )?)
            }
            LoadPhase::Resources => {
                auxiliary_pages::target_call(rt, id, "dispatchDocumentComplete", &[])?;
                None
            }
        };
        let shared = rt.op_state();
        let mut shared = shared.borrow_mut();
        let state = self::state(&mut shared);
        if let Some(promise) = next {
            if let Some(load) = state.loading.get_mut(&target) {
                load.promise = promise;
                load.phase = LoadPhase::Resources;
            }
        } else {
            state.loading.remove(&target);
        }
    }
    Ok(())
}

impl PageSession {
    pub fn has_pending_auxiliary_work(&self) -> bool {
        let shared = self.rt.op_state();
        let shared = shared.borrow();
        pending(&shared) || shared.borrow::<auxiliary_pages::Pages>().pending_count() > 0
    }

    /// One interruptible actor turn. The jobs and hydration phases remain owned
    /// by OpState if the caller is dropped to service another target's command.
    pub(super) async fn service_auxiliary_navigation(&mut self) -> Result<(), String> {
        let budget = self.budget_ms;
        let watchdog = Watchdog::start(self.rt.v8_isolate().thread_safe_handle(), budget);
        let key = self.rt.activation_key();
        let result = key
            .during(tokio::time::timeout(
                std::time::Duration::from_millis(budget),
                self.service_auxiliary_navigation_active(),
            ))
            .await;
        let exceeded = watchdog.exceeded.load(std::sync::atomic::Ordering::Relaxed);
        drop(watchdog);
        self.rt.v8_isolate().cancel_terminate_execution();
        if exceeded {
            return Err("Native auxiliary work exceeded execution budget".into());
        }
        result.map_err(|_| "Native auxiliary work exceeded execution budget".to_owned())?
    }

    async fn service_auxiliary_navigation_active(&mut self) -> Result<(), String> {
        loop {
            let navigation = auxiliary_pages::next_navigation(&mut self.rt.op_state().borrow_mut());
            let Some(navigation) = navigation else {
                break;
            };
            let mode = if navigation.replace {
                HistoryMode::Replace
            } else {
                HistoryMode::Push
            };
            start(&mut self.rt, navigation, mode, self.budget_ms)?;
        }
        if !self.has_pending_auxiliary_work() {
            return Ok(());
        }
        self.rt.v8_isolate().perform_microtask_checkpoint();
        let completed = std::future::poll_fn(|cx| {
            let ready = {
                let shared = self.rt.op_state();
                let mut shared = shared.borrow_mut();
                poll_fetches(self::state(&mut shared), cx)
            };
            let event = self.rt.poll_event_loop(cx, Default::default());
            if let Poll::Ready(Err(error)) = event {
                return Poll::Ready(Err(error.to_string()));
            }
            let loads = {
                let shared = self.rt.op_state();
                let mut shared = shared.borrow_mut();
                self::state(&mut shared)
                    .loading
                    .iter()
                    .map(|(target, load)| (*target, load.promise.clone()))
                    .collect::<Vec<_>>()
            };
            let settled = loads.iter().any(|(target, promise)| {
                !matches!(promise_result(&mut self.rt, *target, promise), Ok(false))
            });
            if !ready.is_empty() || settled {
                Poll::Ready(Ok(ready))
            } else {
                Poll::Pending
            }
        })
        .await?;
        for (target, job, fetched) in completed {
            let result =
                fetched.and_then(|fetched| commit_fetched(&mut self.rt, target, &job, fetched));
            let shared = self.rt.op_state();
            let mut shared = shared.borrow_mut();
            let state = self::state(&mut shared);
            if state.waiters.contains(&job.generation) {
                state.results.insert(job.generation, result);
            }
        }
        advance_loads(&mut self.rt)?;
        Ok(())
    }

    pub async fn navigate_target(
        &mut self,
        target: u64,
        url: &str,
        history: &str,
    ) -> Result<String, String> {
        self.navigate_target_with_options(target, url, history, "{}")
            .await
    }

    pub async fn navigate_target_with_options(
        &mut self,
        target: u64,
        url: &str,
        history: &str,
        options: &str,
    ) -> Result<String, String> {
        let options: NavigationOptions = serde_json::from_str(options)
            .map_err(|error| format!("Invalid target navigation options: {error}"))?;
        if options.wait_until.as_deref().is_some_and(|value| {
            !matches!(
                value,
                "commit" | "domcontentloaded" | "load" | "networkidle"
            )
        }) {
            return Err("Unsupported target navigation waitUntil".into());
        }
        let budget = options
            .timeout_ms
            .filter(|value| *value > 0)
            .unwrap_or(self.budget_ms);
        let (navigation, mode) = {
            let _activation = self.rt.activation_key().enter();
            deno_core::scope!(scope, self.rt);
            let context = auxiliary_pages::context(scope, AuxiliaryContextId(target))?;
            let source = binding::auxiliary_context_identity(context)
                .ok_or("Target identity is unavailable")?
                .id;
            let mut snapshot = SourceSnapshot::capture(scope, context, source)?;
            let mut headers = options.headers;
            if let Some(content_type) = options.content_type {
                if !headers
                    .keys()
                    .any(|name| name.eq_ignore_ascii_case("content-type"))
                {
                    headers.insert("content-type".into(), content_type);
                }
            }
            snapshot.request = RequestPlan {
                method: options.method.map(|method| method.to_ascii_uppercase()),
                body: options
                    .body_bytes
                    .or_else(|| options.body.map(String::into_bytes)),
                headers,
                referer: options.referer,
                form_action: false,
            };
            let mode = history_mode(&JsRuntime::op_state_from(scope).borrow(), target, history)?;
            let resolved = if let HistoryMode::Traverse(index) = mode {
                let shared = JsRuntime::op_state_from(scope);
                let shared = shared.borrow();
                shared.borrow::<State>().history[&target].entries[index].clone()
            } else {
                resolve(&snapshot.source, url).ok_or("Invalid target navigation URL")?
            };
            (
                auxiliary_pages::Navigation {
                    target: source,
                    url: resolved,
                    noreferrer: false,
                    replace: false,
                    snapshot,
                },
                mode,
            )
        };
        let generation = start(&mut self.rt, navigation, mode, budget)?;
        self.wait_target_navigation(generation, budget).await
    }

    pub(super) async fn navigate_form_target(
        &mut self,
        target: u64,
        pending: form_submission::Pending,
    ) -> Result<String, String> {
        let identity = AuxiliaryContextId(target);
        let snapshot = SourceSnapshot {
            source: pending.source,
            policy: pending.policy,
            referrer_policy: pending.referrer_policy,
            user_activation: pending.user_activation,
            request: RequestPlan {
                method: Some(pending.plan.method),
                body: pending.plan.body,
                headers: pending.plan.headers,
                referer: None,
                form_action: true,
            },
        };
        let navigation = auxiliary_pages::Navigation {
            target: identity,
            url: pending.plan.url,
            noreferrer: false,
            replace: false,
            snapshot,
        };
        let generation = start(&mut self.rt, navigation, HistoryMode::Push, self.budget_ms)?;
        self.wait_target_navigation(generation, self.budget_ms)
            .await
    }

    async fn wait_target_navigation(
        &mut self,
        generation: u64,
        budget: u64,
    ) -> Result<String, String> {
        let shared = self.rt.op_state();
        self::state(&mut shared.borrow_mut())
            .waiters
            .insert(generation);
        let mut waiter = NavigationWaiter {
            state: shared,
            generation,
            completed: false,
        };
        let key = self.rt.activation_key();
        let watchdog = Watchdog::start(self.rt.v8_isolate().thread_safe_handle(), budget);
        let result = key
            .during(tokio::time::timeout(
                std::time::Duration::from_millis(budget),
                async {
                    loop {
                        let result = {
                            let shared = self.rt.op_state();
                            let mut shared = shared.borrow_mut();
                            self::state(&mut shared).results.remove(&generation)
                        };
                        if let Some(result) = result {
                            return result.map(|receipt| receipt.to_string());
                        }
                        self.service_auxiliary_navigation().await?;
                    }
                },
            ))
            .await;
        let exceeded = watchdog.exceeded.load(std::sync::atomic::Ordering::Relaxed);
        drop(watchdog);
        self.rt.v8_isolate().cancel_terminate_execution();
        if exceeded {
            return Err(format!("Target navigation timeout after {budget}ms"));
        }
        let result = result.map_err(|_| format!("Target navigation timeout after {budget}ms"))?;
        waiter.completed = result.is_ok();
        result
    }
}

fn history_mode(state: &OpState, target: u64, mode: &str) -> Result<HistoryMode, String> {
    match mode {
        "push" => Ok(HistoryMode::Push),
        "replace" => Ok(HistoryMode::Replace),
        "back" | "forward" => {
            let history = state
                .try_borrow::<State>()
                .and_then(|state| state.history.get(&target))
                .ok_or("Target has no recorded session history")?;
            let index = if mode == "back" {
                history.index.checked_sub(1)
            } else {
                history
                    .index
                    .checked_add(1)
                    .filter(|index| *index < history.entries.len())
            };
            index
                .map(HistoryMode::Traverse)
                .ok_or_else(|| "Target history traversal is out of range".into())
        }
        _ => Err(format!("Unsupported target history operation: {mode}")),
    }
}

struct NavigationWaiter {
    state: Rc<RefCell<OpState>>,
    generation: u64,
    completed: bool,
}
impl Drop for NavigationWaiter {
    fn drop(&mut self) {
        let mut state = self.state.borrow_mut();
        let state = self::state(&mut state);
        state.waiters.remove(&self.generation);
        state.results.remove(&self.generation);
        if !self.completed {
            state
                .jobs
                .retain(|_, job| job.generation != self.generation);
            state
                .loading
                .retain(|_, load| load.generation != self.generation);
        }
    }
}

struct RequestCompletion {
    log: NetworkLog,
    trace: std::sync::Arc<FetchResponseTrace>,
    context: serde_json::Value,
    url: String,
    method: String,
    finished: bool,
}
impl Drop for RequestCompletion {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let error = "Native navigation cancelled or failed before body completion";
        if let Some(id) = self.trace.last_id() {
            self.log.push(
                serde_json::json!({"event":"responsebody","id":id,"error":error,
                "bodyIncomplete":false,"bodyUnavailable":true}),
            );
        } else {
            let id = self.log.next_id();
            self.log.push(
                serde_json::json!({"event":"response","id":id,"url":self.url,
                "method":self.method,"context":self.context,"status":0,"ok":false,
                "error":error,"headers":[],"body":"","bodyBytes":[],"bodyIncomplete":false}),
            );
        }
    }
}

fn report_target_error(rt: &mut JsRuntime, target: u64, message: &str) {
    deno_core::scope!(scope, rt);
    let Ok(context) = auxiliary_pages::context(scope, AuxiliaryContextId(target)) else {
        return;
    };
    let frame = request_frame_metadata(scope, context, None);
    let shared = JsRuntime::op_state_from(scope);
    shared
        .borrow()
        .borrow::<NetworkLog>()
        .push(serde_json::json!({
            "event":"pageerror","name":"Error","message":message,"context":{"frame":frame}
        }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_history_traversal_and_replacement_preserve_independent_entries() {
        let mut state = State::default();
        update_history(&mut state, 2, "https://local.test/first", HistoryMode::Push);
        update_history(
            &mut state,
            2,
            "https://local.test/second",
            HistoryMode::Push,
        );
        update_history(&mut state, 3, "https://local.test/other", HistoryMode::Push);
        update_history(
            &mut state,
            2,
            "https://local.test/first",
            HistoryMode::Traverse(0),
        );
        update_history(
            &mut state,
            2,
            "https://local.test/replaced",
            HistoryMode::Replace,
        );
        update_history(&mut state, 2, "https://local.test/new", HistoryMode::Push);
        assert_eq!(
            state.history[&2].entries,
            ["https://local.test/replaced", "https://local.test/new"]
        );
        assert_eq!(state.history[&2].index, 1);
        assert_eq!(state.history[&3].entries, ["https://local.test/other"]);
    }

    #[test]
    fn options_keep_binary_body_and_content_type_independent_of_text_fallback() {
        let options: NavigationOptions = serde_json::from_str(r#"{"method":"POST","body":"fallback","bodyBytes":[0,255,10],"contentType":"application/octet-stream","timeoutMs":0}"#).unwrap();
        assert_eq!(options.body_bytes, Some(vec![0, 255, 10]));
        assert_eq!(options.body.as_deref(), Some("fallback"));
        assert_eq!(
            options.content_type.as_deref(),
            Some("application/octet-stream")
        );
        assert_eq!(options.timeout_ms, Some(0));
    }
}

fn report_scope_error<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    message: &str,
) {
    let frame = request_frame_metadata(scope, context, None);
    JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<NetworkLog>()
        .push(serde_json::json!({
            "event":"pageerror","name":"Error","message":message,"context":{"frame":frame}
        }));
}

fn run_navigation_init<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    bootstrap: &AuxiliaryBootstrap,
) -> Result<(), String> {
    v8::tc_scope!(let scope, scope);
    if !binding::run_auxiliary_init_scripts(scope, context) {
        let error = scope
            .exception()
            .map(|error| error.to_rust_string_lossy(scope))
            .unwrap_or_else(|| "Auxiliary document init script failed".into());
        report_scope_error(scope, context, &error);
        scope.reset();
    }
    if !bootstrap.engine_init_script.is_empty() {
        crate::browser_env::run_engine_script(scope, &bootstrap.engine_init_script)
            .ok_or("Auxiliary driver bootstrap failed")?;
    }
    if bootstrap.init_script.is_empty() {
        return Ok(());
    }
    let source = v8::String::new(scope, &bootstrap.init_script)
        .ok_or("Auxiliary init source allocation failed")?;
    if v8::Script::compile(scope, source, None)
        .and_then(|script| script.run(scope))
        .is_none()
    {
        if scope.is_execution_terminating() {
            return Err("Auxiliary init exceeded execution budget".into());
        }
        let error = scope
            .exception()
            .map(|error| error.to_rust_string_lossy(scope))
            .unwrap_or_else(|| "Auxiliary document init script failed".into());
        report_scope_error(scope, context, &error);
        scope.reset();
    }
    Ok(())
}

struct FailAuxiliaryBackingForTesting;
impl PageSession {
    /// Inject a host installation fault after real DOM creation, before backing commit.
    #[doc(hidden)]
    pub fn fail_next_auxiliary_backing_for_testing(&mut self, target: u64) -> Result<(), String> {
        let _activation = self.rt.activation_key().enter();
        deno_core::scope!(scope, self.rt);
        let context = auxiliary_pages::context(scope, AuxiliaryContextId(target))?;
        context.set_slot(Rc::new(FailAuxiliaryBackingForTesting));
        Ok(())
    }
}
