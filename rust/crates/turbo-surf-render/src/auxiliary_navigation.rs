//! Actor-owned navigation jobs survive cancellation of an individual driver turn.
//! HTTP futures contain native request snapshots, never guest contexts or callbacks.
use super::*;
#[path = "navigation_deadline.rs"]
mod navigation_deadline;

pub(super) fn diagnostic_driver_active(rt: &JsRuntime) -> bool {
    navigation_deadline::diagnostic_active(rt)
}

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
    driver_navigation: bool,
}

impl RequestPlan {
    fn initiator(&self, source: &str) -> Option<String> {
        if self.driver_navigation {
            self.referer.clone()
        } else {
            Some(source.into())
        }
    }
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
    reload: bool,
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
    Reload,
    Traverse(u64),
}

struct Fetched {
    response: FetchResult,
    metadata: crate::DocumentResponseMetadata,
    request_id: Option<u64>,
}
enum NavigationFetchError {
    Failed(String),
    Deadline,
}
type FetchFuture = Pin<Box<dyn Future<Output = Result<Fetched, NavigationFetchError>>>>;
struct Job {
    activity: crate::network_activity::RequestActivity,
    generation: u64,
    future: FetchFuture,
    history: HistoryMode,
    driver_navigation: bool,
    form_action: bool,
}

pub(super) struct CommitHistory {
    pub(super) previous_url: String,
    pub(super) operation: &'static str,
    pub(super) driver_navigation: bool,
    pub(super) mode: HistoryMode,
}

fn history_operation(mode: HistoryMode) -> &'static str {
    match mode {
        HistoryMode::Push => "push",
        HistoryMode::Replace => "replace",
        HistoryMode::Reload => "reload",
        HistoryMode::Traverse(_) => "traverse",
    }
}

struct Loading {
    _activity: crate::network_activity::RequestActivity,
    generation: u64,
    promise: v8::Global<v8::Value>,
    phase: LoadPhase,
}
#[derive(Clone, Copy)]
enum LoadPhase {
    Parsing,
    Resources,
}
pub(super) enum ActionCompletion {
    Released,
    NavigationFailed(String),
}

enum NavigationWaitError {
    Navigation(String),
    Driver(String),
}

enum ActionTerminalError {
    Navigation(String),
    Driver(String),
}

#[derive(Default)]
pub(super) struct State {
    sequence: u64,
    root_commits: u64,
    root_form_commits: u64,
    action_commits: BTreeMap<u64, (u64, Result<(), ActionTerminalError>)>,
    jobs: BTreeMap<u64, Job>,
    loading: BTreeMap<u64, Loading>,
    results: BTreeMap<u64, Result<serde_json::Value, String>>,
    waiters: BTreeSet<u64>,
}

pub(super) fn root_commits(state: &OpState) -> u64 {
    state
        .try_borrow::<State>()
        .map_or(0, |state| state.root_commits)
}

pub(super) fn root_form_commits(state: &OpState) -> u64 {
    state
        .try_borrow::<State>()
        .map_or(0, |state| state.root_form_commits)
}

fn adopt_root_response(
    rt: &mut JsRuntime,
    target: u64,
    fetched: &Fetched,
    form: bool,
) -> Result<(), String> {
    let shared = rt.op_state();
    let mut shared = shared.borrow_mut();
    let root = shared.borrow::<auxiliary_pages::Pages>().events.root();
    let target = target.to_string();
    if root.as_deref() != Some(target.as_str()) {
        return Ok(());
    }
    let current = state(&mut shared);
    let count = current
        .root_commits
        .checked_add(1)
        .ok_or("Native navigation counter exhausted")?;
    let forms = current
        .root_form_commits
        .checked_add(u64::from(form))
        .ok_or("Native form navigation counter exhausted")?;
    current.root_commits = count;
    current.root_form_commits = forms;
    shared
        .borrow_mut::<Base>()
        .0
        .clone_from(&fetched.response.final_url);
    shared
        .borrow_mut::<ResponseHeaders>()
        .0
        .clone_from(&fetched.response.headers);
    *shared.borrow_mut::<crate::DocumentResponseMetadata>() = fetched.metadata.clone();
    Ok(())
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
    state.action_commits.remove(&target);
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
    let auth_credentials = storage.http_credentials();
    let log = state.borrow::<NetworkLog>().clone();
    Box::pin(async move {
        let mut local = jar.borrow().fork_for_request();
        let mut request = RequestContext::navigation(
            navigation
                .snapshot
                .request
                .initiator(&navigation.snapshot.source),
            navigation.snapshot.user_activation,
        );
        request.referrer = navigation.snapshot.request.referer.clone();
        request.referrer_policy = if navigation.noreferrer {
            turbo_surf_core::referrer_policy::ReferrerPolicy::NoReferrer
        } else {
            navigation.snapshot.referrer_policy
        };
        let metadata = std::sync::Mutex::new(crate::DocumentResponseMetadata::default());
        let response_url = std::sync::Mutex::new(navigation.url.clone());
        let response_policy =
            |request: &RequestContext, target: &str, _headers: &[(String, String)]| {
                *response_url.lock().unwrap() = target.to_owned();
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
                    http_credentials: auth_credentials.as_ref(),
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
                    allow_attachments: true,
                    now: cookie_now(),
                    ..Default::default()
                },
            ),
        )
        .await;
        jar.borrow_mut().merge_request_changes(local);
        let mut response = result
            .map_err(|_| NavigationFetchError::Deadline)?
            .map_err(|error| {
                NavigationFetchError::Failed(format!("Native target navigation failed: {error}"))
            })?;
        // The native HTTP client's response URL omits fragments. The admitted
        // response hop retains the document navigation URL, including its fragment.
        response.final_url = response_url.into_inner().unwrap();
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

fn current_admission(
    state: &OpState,
    target: u64,
    activity: Option<&crate::network_activity::RequestActivity>,
) -> bool {
    // Legacy driver navigation has no queued admission token and is admitted by start().
    activity.is_none_or(|activity| {
        activity.navigation_revision().is_some_and(|revision| {
            state
                .borrow::<NetworkLog>()
                .current_navigation_revision(target)
                == Some(revision)
        })
    })
}

fn start(
    rt: &mut OwnedRuntime,
    navigation: auxiliary_pages::Navigation,
    mode: HistoryMode,
    budget_ms: u64,
) -> Result<u64, String> {
    let _activation = rt.activation_key().enter();
    let target = navigation.target;
    let frame = {
        deno_core::scope!(scope, rt);
        let context = auxiliary_pages::context(scope, target)?;
        request_frame_metadata(scope, context, None)
    };
    let shared = rt.op_state();
    let mut shared = shared.borrow_mut();
    if navigation.url.len()
        > shared
            .borrow::<auxiliary_pages::Pages>()
            .policy
            .max_navigation_url_bytes
    {
        return Err("Native target navigation URL budget exceeded".into());
    }
    let mode = native_history::admit_navigation(&shared, target.0, mode, &navigation.url)?;
    let activity = navigation.activity.clone().unwrap_or_else(|| {
        shared
            .borrow::<NetworkLog>()
            .begin_page_navigation(&target.0.to_string())
    });
    cancel(&mut shared, target.0);
    let driver_navigation = navigation.snapshot.request.driver_navigation;
    let form_action = navigation.snapshot.request.form_action;
    let future = fetch_job(&shared, navigation, frame, budget_ms);
    let state = self::state(&mut shared);
    state.sequence = state
        .sequence
        .checked_add(1)
        .ok_or("Navigation identity exhausted")?;
    let generation = state.sequence;
    state.jobs.insert(
        target.0,
        Job {
            activity,
            generation,
            future,
            history: mode,
            driver_navigation,
            form_action,
        },
    );
    Ok(generation)
}

fn poll_fetches(
    state: &mut State,
    cx: &mut Context<'_>,
    owner: Option<u64>,
) -> Vec<(u64, Job, Result<Fetched, NavigationFetchError>)> {
    let ready = state
        .jobs
        .iter_mut()
        .filter(|(target, _)| owner.is_none_or(|owner| owner == **target))
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

fn install_response<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    previous: v8::Local<'s, v8::Context>,
    fetched: &Fetched,
    job: &Job,
) -> Result<v8::Global<v8::Context>, String> {
    let history = CommitHistory {
        previous_url: window_proxy::location_url(previous)
            .ok_or("Previous native document Location is unavailable")?,
        operation: history_operation(job.history),
        driver_navigation: job.driver_navigation,
        mode: job.history,
    };
    let response = &fetched.response;
    let next_policy = OpenerPolicy::response(&response.headers, &response.final_url);
    let isolated = needs_group_switch(previous.get_slot::<OpenerPolicy>().as_deref(), &next_policy);
    let activation =
        NavigationActivationSnapshot::capture(scope, history.previous_url.clone(), previous);
    let current = binding::create_auxiliary_window_context(scope, (!isolated).then_some(previous));
    current.set_slot(Rc::new(next_policy));
    let result = install_response_backing(
        scope, previous, current, fetched, isolated, history, activation,
    );
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
    history: CommitHistory,
    activation: NavigationActivationSnapshot,
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
    crate::script_inventory::register_guest(scope, current);
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
    binding::try_initialize_auxiliary_utf16_source_positions(scope, &units)
        .map_err(|error| error.to_string())?;
    install_css_authorization(scope).map_err(|e| e.to_string())?;
    let facts = DocumentFacts::response(scope, current, &response.final_url, &response.headers);
    install_document_facts(scope, facts);
    user_activation::carry_navigation(scope, activation, current, &response.final_url);
    current.set_slot(Rc::new(FrameDocumentLocation(response.final_url.clone())));
    current.set_slot(Rc::new(FrameDocumentUrl(response.final_url.clone())));
    auxiliary_storage::carry_navigation(scope, previous, current)
        .map_err(|error| error.to_string())?;
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
    let mode = history.mode;
    let mut event = auxiliary_pages::committed(
        scope,
        previous,
        current,
        &response.final_url,
        isolated,
        history,
    )?;
    let active_url = native_history::commit_navigation(scope, current, &response.final_url, mode)?;
    install_response_environment(scope, current, core, &active_url)?;
    if let AuxiliaryPageEvent::Navigated { url, .. } = &mut event {
        *url = active_url;
    }
    JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<auxiliary_pages::Pages>()
        .events
        .push(event);
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

fn install_response_environment<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    current: v8::Local<'s, v8::Context>,
    core: v8::Local<'s, v8::Object>,
    url: &str,
) -> Result<(), String> {
    window_proxy::install_location(scope, url);
    install_environment(scope, core.into())?;
    let secure = secure_context(scope);
    crate::crypto_backend::install(scope, core, secure)?;
    crate::audio_backend::install(scope, core)?;
    crate::geometry_backend::install(scope, core)?;
    native_history::activate(scope, current)?;
    native_history::materialize(scope, current)?;
    binding::capture_auxiliary_window_exception(scope, current);
    crate::browser_env::finish_key_order(scope);
    Ok(())
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
    if attachment(&fetched.response) || matches!(fetched.response.status, 204 | 205) {
        return Ok(receipt);
    }
    native_history::admit_navigation(
        &rt.op_state().borrow(),
        target,
        job.history,
        &fetched.response.final_url,
    )?;
    // Defer the driver deadline only across trusted native document setup.
    let setup = navigation_deadline::TrustedSetup::enter(rt, target, job.generation)?;
    let installed = {
        deno_core::scope!(scope, rt);
        let previous = auxiliary_pages::context(scope, AuxiliaryContextId(target))?;
        let installed = install_response(scope, previous, &fetched, job)?;
        let current = v8::Local::new(scope, &installed);
        navigation_deadline::capture_source_lease(scope, current, target, job.generation)?;
        installed
    };
    adopt_root_response(rt, target, &fetched, job.form_action)?;
    setup.finish()?;
    navigation_deadline::run_deferred_frames(rt)?;
    {
        deno_core::scope!(scope, rt);
        let current = v8::Local::new(scope, installed);
        let scope = &mut v8::ContextScope::new(scope, current);
        let bootstrap = auxiliary_pages::bootstrap_for_context(scope, current);
        run_navigation_init(scope, current, &bootstrap)?;
    }
    let id = AuxiliaryContextId(target);
    auxiliary_pages::target_call(rt, id, "documentLoading", &[])?;
    let promise =
        auxiliary_pages::execute_target(rt, id, "return __tsNs.hydrate(300,200000,false)", true)?;
    let shared = rt.op_state();
    let mut shared = shared.borrow_mut();
    let state = self::state(&mut shared);
    state.loading.insert(
        target,
        Loading {
            _activity: job.activity.clone(),
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

fn advance_loads(rt: &mut JsRuntime, owner: Option<u64>) -> Result<(), String> {
    let pending = {
        let shared = rt.op_state();
        let mut shared = shared.borrow_mut();
        self::state(&mut shared)
            .loading
            .iter()
            .filter(|(id, _)| owner.is_none_or(|owner| owner == **id))
            .map(|(id, load)| (*id, load.promise.clone(), load.phase, load.generation))
            .collect::<Vec<_>>()
    };
    for (target, promise, phase, generation) in pending {
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
                navigation_deadline::finish_source_goal(
                    rt,
                    target,
                    generation,
                    navigation_deadline::ExecutionGoal::Interactive,
                );
                Some(auxiliary_pages::execute_target(
                    rt,
                    id,
                    "return __tsNs.waitForDocumentResources()",
                    true,
                )?)
            }
            LoadPhase::Resources => {
                auxiliary_pages::target_call(rt, id, "dispatchDocumentComplete", &[])?;
                navigation_deadline::finish_source_goal(
                    rt,
                    target,
                    generation,
                    navigation_deadline::ExecutionGoal::Complete,
                );
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
        pending(&shared)
            || shared.borrow::<auxiliary_pages::Pages>().pending_count() > 0
            || native_history::has_tasks(&shared)
    }

    /// One interruptible actor turn. The jobs and hydration phases remain owned
    /// by OpState if the caller is dropped to service another target's command.
    pub(super) async fn service_auxiliary_navigation(&mut self) -> Result<(), String> {
        self.service_auxiliary_navigation_scoped(None, None).await
    }

    async fn service_auxiliary_navigation_scoped(
        &mut self,
        owner: Option<u64>,
        waiter: Option<u64>,
    ) -> Result<(), String> {
        let budget = self.budget_ms;
        let watchdog = Watchdog::start(self.rt.v8_isolate(), budget);
        let key = self.rt.activation_key();
        let result = key
            .during(tokio::time::timeout(
                std::time::Duration::from_millis(budget),
                self.service_auxiliary_navigation_active(owner, waiter),
            ))
            .await;
        let exceeded = watchdog.exceeded.load(std::sync::atomic::Ordering::Relaxed);
        drop(watchdog);
        self.rt.v8_isolate().cancel_terminate_execution();
        auxiliary_navigation::retire_interrupted_frames(&mut self.rt);
        let result = if exceeded {
            Err("Native auxiliary work exceeded execution budget".into())
        } else {
            result.unwrap_or_else(|_| {
                Err("Native auxiliary work exceeded execution budget".to_owned())
            })
        };
        self.finish_history_command(result)
    }

    fn has_pending_auxiliary_work_for(&self, owner: Option<u64>) -> bool {
        match owner {
            Some(target) => self
                .rt
                .op_state()
                .borrow()
                .try_borrow::<State>()
                .is_some_and(|state| {
                    state.jobs.contains_key(&target) || state.loading.contains_key(&target)
                }),
            None => self.has_pending_auxiliary_work(),
        }
    }

    async fn service_auxiliary_navigation_active(
        &mut self,
        owner: Option<u64>,
        waiter: Option<u64>,
    ) -> Result<(), String> {
        loop {
            let navigation = {
                let shared = self.rt.op_state();
                let mut state = shared.borrow_mut();
                match owner {
                    Some(target) => auxiliary_pages::next_navigation_for(&mut state, target),
                    None => auxiliary_pages::next_navigation(&mut state),
                }
            };
            let Some(navigation) = navigation else {
                break;
            };
            if !current_admission(
                &self.rt.op_state().borrow(),
                navigation.target.0,
                navigation.activity.as_ref(),
            ) {
                continue;
            }
            let mode = navigation.history_mode.unwrap_or(if navigation.replace {
                HistoryMode::Replace
            } else {
                HistoryMode::Push
            });
            start(&mut self.rt, navigation, mode, self.budget_ms)?;
        }
        if !self.has_pending_auxiliary_work_for(owner) {
            return Ok(());
        }
        self.rt.v8_isolate().perform_microtask_checkpoint();
        let mut timer_wake = None;
        let completed = std::future::poll_fn(|cx| {
            let host_task = background_work::ready_tasks_for(&mut self.rt, owner)?
                || poll_auxiliary_document_tasks(&mut self.rt, owner, cx, &mut timer_wake)?;
            let ready = {
                let shared = self.rt.op_state();
                let mut shared = shared.borrow_mut();
                poll_fetches(self::state(&mut shared), cx, owner)
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
                    .filter(|(target, _)| owner.is_none_or(|owner| owner == **target))
                    .map(|(target, load)| (*target, load.promise.clone()))
                    .collect::<Vec<_>>()
            };
            let settled = loads.iter().any(|(target, promise)| {
                !matches!(promise_result(&mut self.rt, *target, promise), Ok(false))
            });
            // Guest callbacks can close a target during either checkpoint. Wake
            // only this driver's result; a peer's result must not settle its wait.
            let terminal = waiter.is_some_and(|generation| {
                self.rt
                    .op_state()
                    .borrow()
                    .borrow::<State>()
                    .results
                    .contains_key(&generation)
            });
            if terminal
                || !self.has_pending_auxiliary_work_for(owner)
                || !ready.is_empty()
                || settled
                || host_task
                || has_pending_frame_work(&self.rt, owner)
            {
                Poll::Ready(Ok(ready))
            } else {
                Poll::Pending
            }
        })
        .await?;
        for (target, job, fetched) in completed {
            let (result, terminal) = match fetched {
                Err(NavigationFetchError::Failed(error)) => (
                    Err(error.clone()),
                    Err(ActionTerminalError::Navigation(error)),
                ),
                Err(NavigationFetchError::Deadline) => {
                    let error = "Native target navigation exceeded session budget".to_owned();
                    (Err(error.clone()), Err(ActionTerminalError::Driver(error)))
                }
                Ok(fetched) => {
                    let result = commit_fetched(&mut self.rt, target, &job, fetched);
                    let terminal = result
                        .as_ref()
                        .map(|_| ())
                        .map_err(|error| ActionTerminalError::Driver(error.clone()));
                    (result, terminal)
                }
            };
            let shared = self.rt.op_state();
            let mut shared = shared.borrow_mut();
            let state = self::state(&mut shared);
            if let Some(revision) = job.activity.navigation_revision() {
                state.action_commits.insert(target, (revision, terminal));
            }
            if state.waiters.contains(&job.generation) {
                state.results.insert(job.generation, result);
            }
        }
        if !navigation_deadline::expired(&self.rt) {
            advance_loads(&mut self.rt, owner)?;
        }
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

    fn parse_target_navigation_options(
        &mut self,
        target: u64,
        options: &str,
    ) -> Result<NavigationOptions, String> {
        navigation_deadline::reconcile_expired_source_modules(&mut self.rt, target)?;
        serde_json::from_str(options)
            .map_err(|error| format!("Invalid target navigation options: {error}"))
    }

    pub async fn navigate_target_with_options(
        &mut self,
        target: u64,
        url: &str,
        history: &str,
        options: &str,
    ) -> Result<String, String> {
        let options = self.parse_target_navigation_options(target, options)?;
        if options.wait_until.as_deref().is_some_and(|value| {
            !matches!(
                value,
                "commit" | "domcontentloaded" | "load" | "networkidle"
            )
        }) {
            return Err("Unsupported target navigation waitUntil".into());
        }
        let goal = navigation_deadline::ExecutionGoal::requested(options.wait_until.as_deref());
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
                driver_navigation: true,
            };
            let mode = native_history::resolve_mode(
                &JsRuntime::op_state_from(scope).borrow(),
                target,
                history,
                options.reload,
            )?;
            let resolved = if let HistoryMode::Traverse(row) = mode {
                native_history::traversal_url(
                    &JsRuntime::op_state_from(scope).borrow(),
                    target,
                    row,
                )?
            } else {
                resolve(&snapshot.source, url).ok_or("Invalid target navigation URL")?
            };
            (
                auxiliary_pages::Navigation {
                    target: source,
                    url: resolved,
                    noreferrer: false,
                    replace: false,
                    history_mode: None,
                    snapshot,
                    activity: None,
                },
                mode,
            )
        };
        if let Some(receipt) = self.same_document_history_receipt(target, mode, budget)? {
            return Ok(receipt);
        }
        let generation = start(&mut self.rt, navigation, mode, budget)?;
        self.wait_target_navigation(target, generation, budget, None, goal)
            .await
    }

    fn same_document_history_receipt(
        &mut self,
        target: u64,
        mode: HistoryMode,
        budget: u64,
    ) -> Result<Option<String>, String> {
        match mode {
            HistoryMode::Traverse(row) => self.traverse_same_history_group(target, row, budget),
            _ => Ok(None),
        }
    }

    pub(super) async fn navigate_form_target(
        &mut self,
        target: u64,
        pending: form_submission::Pending,
    ) -> Result<String, String> {
        self.navigate_form_target_scoped(target, pending, false)
            .await
    }

    pub(super) async fn navigate_form_target_scoped(
        &mut self,
        target: u64,
        pending: form_submission::Pending,
        owned_completion: bool,
    ) -> Result<String, String> {
        let generation = self.start_form_target(target, pending)?;
        let receipt = self
            .wait_target_navigation(
                target,
                generation,
                self.budget_ms,
                owned_completion.then_some(target),
                navigation_deadline::ExecutionGoal::Commit,
            )
            .await?;
        while !owned_completion
            && self
                .rt
                .op_state()
                .borrow()
                .borrow::<State>()
                .loading
                .contains_key(&target)
        {
            self.service_auxiliary_navigation_scoped(owned_completion.then_some(target), None)
                .await?;
        }
        Ok(receipt)
    }

    fn start_form_target(
        &mut self,
        target: u64,
        pending: form_submission::Pending,
    ) -> Result<u64, String> {
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
                driver_navigation: false,
            },
        };
        let navigation = auxiliary_pages::Navigation {
            target: identity,
            url: pending.plan.url,
            noreferrer: false,
            replace: false,
            history_mode: None,
            snapshot,
            activity: pending.activity,
        };
        start(&mut self.rt, navigation, HistoryMode::Push, self.budget_ms)
    }

    /// Admit accepted native forms before yielding an interruptible actor turn.
    /// Jobs retain the request; dropping a background poll cannot cancel or replay it.
    pub(super) fn admit_actor_form_navigations(&mut self) -> Result<(), String> {
        self.admit_actor_form_navigations_scoped(None)
    }

    fn admit_actor_form_navigations_scoped(&mut self, owner: Option<u64>) -> Result<(), String> {
        if !self
            .rt
            .op_state()
            .borrow()
            .borrow::<auxiliary_pages::Pages>()
            .enabled
        {
            return Ok(());
        }
        loop {
            let pending = {
                let shared = self.rt.op_state();
                let mut state = shared.borrow_mut();
                let queue = state.borrow_mut::<form_submission::Queue>();
                match owner {
                    Some(_) => queue.take_native_for(owner),
                    None => queue.take_native(),
                }
            };
            let Some((target, pending)) = pending else {
                return Ok(());
            };
            if !current_admission(
                &self.rt.op_state().borrow(),
                target,
                pending.activity.as_ref(),
            ) {
                continue;
            }
            if let Err(error) = self.start_form_target(target, pending) {
                report_target_error(&mut self.rt, target, &error);
                if owner.is_some() {
                    return Err(error);
                }
            }
        }
    }

    async fn wait_target_navigation(
        &mut self,
        target: u64,
        generation: u64,
        budget: u64,
        owner: Option<u64>,
        goal: navigation_deadline::ExecutionGoal,
    ) -> Result<String, String> {
        self.wait_target_navigation_classified(target, generation, budget, owner, None, goal)
            .await
            .map_err(|error| match error {
                NavigationWaitError::Navigation(error) | NavigationWaitError::Driver(error) => {
                    error
                }
            })
    }

    async fn wait_target_navigation_classified(
        &mut self,
        target: u64,
        generation: u64,
        budget: u64,
        owner: Option<u64>,
        admission: Option<u64>,
        goal: navigation_deadline::ExecutionGoal,
    ) -> Result<String, NavigationWaitError> {
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
        let watchdog = navigation_deadline::DriverDeadline::start(
            &mut self.rt,
            budget,
            generation,
            target,
            goal,
        );
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
                            return result.map(|receipt| receipt.to_string()).map_err(|error| {
                                let navigation_failure = owner.zip(admission).is_some_and(|(target, serial)| {
                                    let shared = self.rt.op_state();
                                    let shared = shared.borrow();
                                    matches!(shared.borrow::<State>().action_commits.get(&target),
                                        Some((revision, Err(ActionTerminalError::Navigation(_)))) if *revision == serial)
                                });
                                if navigation_failure { NavigationWaitError::Navigation(error) }
                                else { NavigationWaitError::Driver(error) }
                            });
                        }
                        self.service_auxiliary_navigation_scoped(owner, Some(generation)).await.map_err(NavigationWaitError::Driver)?;
                    }
                },
            ))
            .await;
        let exceeded = watchdog.exceeded();
        let unfinished = exceeded.then(|| watchdog.unfinished_frames());
        drop(watchdog);
        self.rt.v8_isolate().cancel_terminate_execution();
        auxiliary_navigation::retire_interrupted_frames(&mut self.rt);
        if let Some(frames) = unfinished {
            let _activation = key.enter();
            frames.retire(&mut self.rt);
        }
        let result = if exceeded {
            Err(NavigationWaitError::Driver(format!(
                "Target navigation timeout after {budget}ms"
            )))
        } else {
            result.unwrap_or_else(|_| {
                Err(NavigationWaitError::Driver(format!(
                    "Target navigation timeout after {budget}ms"
                )))
            })
        };
        let result = match self.finish_history_command(Ok(())) {
            Ok(()) => result,
            Err(error) => Err(NavigationWaitError::Driver(error)),
        };
        waiter.completed = result.is_ok();
        result
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

fn check_navigation_init_owner<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) -> Result<(), String> {
    let policy = context
        .get_slot::<CspHandle>()
        .ok_or("Native navigation init CSP owner unavailable")?;
    let known_policy = JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<CspContexts>()
        .0
        .contains_key(&policy.id);
    if scope.get_current_context() != context
        || context
            .get_slot::<private_boundary::Infrastructure>()
            .is_some()
        || context.get_slot::<FrameRetired>().is_some()
        || !known_policy
        || !binding::auxiliary_context_identity(context).is_some_and(|identity| !identity.closed)
    {
        return Err("Native navigation init owner is closed or invalid".into());
    }
    Ok(())
}

fn run_navigation_init<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    bootstrap: &AuxiliaryBootstrap,
) -> Result<(), String> {
    check_navigation_init_owner(scope, context)?;
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

impl PageSession {
    pub(super) async fn finish_target_navigation_since(
        &mut self,
        target: u64,
        baseline: u64,
    ) -> Result<ActionCompletion, String> {
        let latest = self
            .rt
            .op_state()
            .borrow()
            .borrow::<NetworkLog>()
            .current_navigation_revision(target)
            .ok_or("Native navigation tracking is invalid")?;
        if latest <= baseline {
            return Ok(ActionCompletion::Released);
        }
        let pending = auxiliary_pages::next_navigation_for_since(
            &mut self.rt.op_state().borrow_mut(),
            target,
            latest - 1,
        );
        if let Some(navigation) = pending {
            let mode = navigation.history_mode.unwrap_or(if navigation.replace {
                HistoryMode::Replace
            } else {
                HistoryMode::Push
            });
            let generation = start(&mut self.rt, navigation, mode, self.budget_ms)?;
            return self
                .wait_action_navigation(target, generation, latest)
                .await;
        }
        let generation = {
            let shared = self.rt.op_state();
            let mut shared = shared.borrow_mut();
            self::state(&mut shared)
                .jobs
                .get(&target)
                .filter(|job| {
                    job.activity
                        .navigation_revision()
                        .is_some_and(|revision| revision == latest)
                })
                .map(|job| job.generation)
        };
        if let Some(generation) = generation {
            return self
                .wait_action_navigation(target, generation, latest)
                .await;
        }
        self.action_terminal_outcome(target, latest)
    }

    fn action_terminal_outcome(
        &mut self,
        target: u64,
        revision: u64,
    ) -> Result<ActionCompletion, String> {
        let shared = self.rt.op_state();
        let mut shared = shared.borrow_mut();
        match self::state(&mut shared).action_commits.get(&target) {
            Some((serial, Err(ActionTerminalError::Navigation(error)))) if *serial == revision => {
                Ok(ActionCompletion::NavigationFailed(error.clone()))
            }
            Some((serial, Err(ActionTerminalError::Driver(error)))) if *serial == revision => {
                Err(error.clone())
            }
            _ => Ok(ActionCompletion::Released),
        }
    }

    async fn wait_action_navigation(
        &mut self,
        target: u64,
        generation: u64,
        revision: u64,
    ) -> Result<ActionCompletion, String> {
        match self
            .wait_target_navigation_classified(
                target,
                generation,
                self.budget_ms,
                Some(target),
                Some(revision),
                navigation_deadline::ExecutionGoal::Commit,
            )
            .await
        {
            Ok(_) => self.action_terminal_outcome(target, revision),
            Err(NavigationWaitError::Navigation(error)) => {
                Ok(ActionCompletion::NavigationFailed(error))
            }
            Err(NavigationWaitError::Driver(error)) => Err(error),
        }
    }

    pub(super) async fn finish_action_form(
        &mut self,
        target: u64,
        pending: form_submission::Pending,
    ) -> Result<ActionCompletion, String> {
        let revision = pending
            .activity
            .as_ref()
            .and_then(|activity| activity.navigation_revision())
            .ok_or("Native form admission token is unavailable")?;
        let generation = self.start_form_target(target, pending)?;
        self.wait_action_navigation(target, generation, revision)
            .await
    }
}

impl PageSession {
    /// Advance admitted navigation belonging to this session's native root only.
    pub async fn poll_root_navigation_work(&mut self) -> Result<(), String> {
        if self.closed {
            return Err("Page session is closed".to_owned());
        }
        let root = self
            .auxiliary_event_log()
            .root()
            .ok_or("Native root ownership unavailable")?
            .parse::<u64>()
            .map_err(|_| "Invalid native root identity")?;
        {
            let _activation = self.rt.activation_key().enter();
            deno_core::scope!(scope, self.rt);
            let context = auxiliary_pages::context(scope, AuxiliaryContextId(root))?;
            if context.get_slot::<FrameRetired>().is_some()
                || binding::auxiliary_context_identity(context)
                    .is_none_or(|identity| identity.id.0 != root)
            {
                return Err("Native root document is closed or ownership changed".to_owned());
            }
        }
        self.admit_actor_form_navigations_scoped(Some(root))?;
        self.service_auxiliary_navigation_scoped(Some(root), None)
            .await?;
        let shared = self.rt.op_state();
        let state = shared.borrow();
        let revision = state
            .borrow::<NetworkLog>()
            .current_navigation_revision(root)
            .ok_or("Native root navigation ownership unavailable")?;
        let failure = state
            .borrow::<State>()
            .action_commits
            .get(&root)
            .filter(|(serial, _)| *serial == revision)
            .and_then(|(_, result)| result.as_ref().err());
        match failure {
            Some(ActionTerminalError::Navigation(error) | ActionTerminalError::Driver(error)) => {
                Err(error.clone())
            }
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_history_traversal_and_replacement_preserve_independent_entries() {
        use super::super::session_history::{Change, History, Limits};
        let limits = Limits {
            entries: 8,
            url_bytes: 1024,
            state_bytes: 0,
        };
        let mut state: BTreeMap<u64, History<()>> = BTreeMap::new();
        for (target, url) in [
            (2, "https://local.test/first"),
            (2, "https://local.test/second"),
            (3, "https://local.test/other"),
        ] {
            state
                .entry(target)
                .or_insert_with(|| History::new(target))
                .placeholder(url.into(), None, Change::Push, limits)
                .unwrap();
        }
        let first = state[&2].entries()[0].row;
        let history = state.get_mut(&2).unwrap();
        history.select(first).unwrap();
        history
            .placeholder(
                "https://local.test/replaced".into(),
                None,
                Change::Replace,
                limits,
            )
            .unwrap();
        history
            .placeholder("https://local.test/new".into(), None, Change::Push, limits)
            .unwrap();
        assert_eq!(
            state[&2]
                .entries()
                .iter()
                .map(|entry| entry.url.as_str())
                .collect::<Vec<_>>(),
            ["https://local.test/replaced", "https://local.test/new"]
        );
        assert_eq!(state[&2].index(), 1);
        assert_eq!(
            state[&3]
                .entries()
                .iter()
                .map(|entry| entry.url.as_str())
                .collect::<Vec<_>>(),
            ["https://local.test/other"]
        );
    }

    #[test]
    fn driver_navigation_initiator_uses_only_an_explicit_referrer() {
        let mut plan = RequestPlan {
            driver_navigation: true,
            ..Default::default()
        };
        assert_eq!(plan.initiator("https://current.test/"), None);
        plan.referer = Some("https://explicit.test/".into());
        assert_eq!(plan.initiator("https://current.test/"), plan.referer);
        plan.driver_navigation = false;
        assert_eq!(
            plan.initiator("https://current.test/"),
            Some("https://current.test/".into())
        );
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

// ENV may create parsed frames, but their authored initialization waits until the
// parent backing is complete and the original driver watchdog is armed again.
pub(super) fn defer_navigation_frame_init(
    scope: &mut v8::PinScope,
    native_init: bool,
) -> Result<bool, JsErrorBox> {
    navigation_deadline::defer_frame_init(scope, native_init).map_err(JsErrorBox::generic)
}

pub(super) fn finish_navigation_frame_init(scope: &mut v8::PinScope) {
    navigation_deadline::finish_frame_init(scope);
}

pub(super) fn new_frame_load_attempt<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    parent: v8::Local<'s, v8::Context>,
    host: v8::Local<'s, v8::Object>,
) -> Result<v8::Local<'s, v8::Object>, JsErrorBox> {
    navigation_deadline::new_frame_attempt(scope, parent, host).map_err(JsErrorBox::generic)
}
pub(super) fn check_frame_load_attempt<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    parent: v8::Local<'s, v8::Context>,
    host: v8::Local<'s, v8::Object>,
    token: v8::Local<'s, v8::Object>,
) -> Result<(), JsErrorBox> {
    navigation_deadline::check_frame_attempt(scope, parent, host, token)
        .map_err(JsErrorBox::generic)
}

pub(super) fn frame_load_attempt_active<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    parent: v8::Local<'s, v8::Context>,
    host: v8::Local<'s, v8::Object>,
    token: v8::Local<'s, v8::Object>,
) -> Result<bool, JsErrorBox> {
    navigation_deadline::frame_attempt_active(scope, parent, host, token)
        .map_err(JsErrorBox::generic)
}

pub(super) fn complete_frame_load_attempt<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    parent: v8::Local<'s, v8::Context>,
    host: v8::Local<'s, v8::Object>,
    token: v8::Local<'s, v8::Object>,
) -> Result<(), JsErrorBox> {
    navigation_deadline::complete_frame_attempt(scope, parent, host, token)
        .map_err(JsErrorBox::generic)
}
pub(super) fn bind_frame_execution<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    parent: v8::Local<'s, v8::Context>,
    host: v8::Local<'s, v8::Object>,
    token: v8::Local<'s, v8::Object>,
) -> Result<(), JsErrorBox> {
    navigation_deadline::bind_frame_execution(scope, context, parent, host, token)
        .map_err(JsErrorBox::generic)
}

pub(super) struct FrameExecutionDeadline(navigation_deadline::FrameExecutionDeadline);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FrameExecutionOutcome {
    Completed,
    Cancelled,
    Interrupted,
}
#[derive(Debug)]
pub(super) enum FrameStartError {
    Cancelled,
    Authority(String),
}
impl FrameExecutionDeadline {
    pub(super) fn finish(self, scope: &mut v8::PinScope) -> FrameExecutionOutcome {
        match self.0.finish(scope) {
            navigation_deadline::FrameExecutionOutcome::Completed => {
                FrameExecutionOutcome::Completed
            }
            navigation_deadline::FrameExecutionOutcome::Cancelled => {
                FrameExecutionOutcome::Cancelled
            }
            navigation_deadline::FrameExecutionOutcome::Interrupted => {
                FrameExecutionOutcome::Interrupted
            }
        }
    }
}
fn frame_start_error(error: navigation_deadline::FrameStartError) -> FrameStartError {
    match error {
        navigation_deadline::FrameStartError::Cancelled => FrameStartError::Cancelled,
        navigation_deadline::FrameStartError::Authority(error) => FrameStartError::Authority(error),
    }
}
pub(super) fn start_frame_execution_deadline<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    parent: v8::Local<'s, v8::Context>,
    host: v8::Local<'s, v8::Object>,
    token: v8::Local<'s, v8::Object>,
) -> Result<Option<FrameExecutionDeadline>, FrameStartError> {
    navigation_deadline::frame_execution_deadline(scope, parent, host, token)
        .map(|guard| guard.map(FrameExecutionDeadline))
        .map_err(frame_start_error)
}
pub(super) fn start_frame_context_deadline<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) -> Result<Option<FrameExecutionDeadline>, FrameStartError> {
    navigation_deadline::frame_context_deadline(scope, context)
        .map(|guard| guard.map(FrameExecutionDeadline))
        .map_err(frame_start_error)
}
pub(super) fn queue_interrupted_frame<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    previous: Option<v8::Local<'s, v8::Context>>,
) {
    navigation_deadline::queue_interrupted_frame(scope, context, previous)
}
pub(super) fn retire_interrupted_frames(rt: &mut OwnedRuntime) {
    navigation_deadline::retire_interrupted_frames(rt)
}

pub(super) fn reconcile_expired_source_modules(
    rt: &mut OwnedRuntime,
    target: u64,
) -> Result<(), String> {
    navigation_deadline::reconcile_expired_source_modules(rt, target)
}
