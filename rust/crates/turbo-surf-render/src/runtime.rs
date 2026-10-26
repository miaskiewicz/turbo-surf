//! Render runtime (tier 3). Page JS runs on a `deno_core` V8 isolate. The DOM is
//! a real rtdom↔V8 binding ([`crate::browser_env`], vendored from turbo-test) so
//! jQuery / React / hand-rolled bundles see a genuine `document`/Element. deno_core
//! supplies the rest: the async event loop, `fetch`/cookies over the tier-1 net
//! stack (`#[op2]` ops below), virtual timers, and the runaway-execution budget.
//!
//! Flow per render: build the runtime → graft the DOM binding onto its context with
//! the fetched page parsed in ([`install_dom`]) → run the page script → drain the
//! event loop + virtual timers → serialize the hydrated tree back to HTML.
//!
//! Each isolate owns its native DOM world and private V8 ownership graph. Scoped
//! activation selects that world only while host operations or Future polls run.
//! Isolate-owned cleanup disposes native handles before V8 invalidation, including
//! cached runtimes and independent sessions; an entry point never resets its peer.

#[cfg(turbo_surf_native_proxy)]
#[path = "window_proxy.rs"]
mod window_proxy;

#[path = "microtask_queue.rs"]
mod microtask_queue;
use microtask_queue::op_install_microtask_queue;

#[path = "observable.rs"]
mod observable;
use observable::{op_report_exception, op_value_is_promise};

#[path = "compression_dictionary.rs"]
mod compression_dictionary;
use compression_dictionary::{
    op_dictionary_active, op_dictionary_headers, op_dictionary_links, op_dictionary_prepare,
};

#[path = "resource_lifecycle.rs"]
mod resource_lifecycle;
use resource_lifecycle::{
    op_execute_inline_script, op_install_resource_consumer, op_prepare_resource_script,
    op_retire_resource_document,
};

use deno_core::{
    op2, resolve_import, v8, JsRuntime, ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse,
    ModuleLoader, ModuleResolveResponse, ModuleSource, ModuleSourceCode, ModuleSpecifier,
    ModuleType, OpState, ResolutionKind, RuntimeOptions,
};
use deno_error::JsErrorBox;
use std::cell::RefCell;
use std::rc::Rc;
use turbo_surf_core::cookies::CookieJar;
#[path = "fetch_streams.rs"]
pub(crate) mod fetch_streams;
use turbo_surf_core::net::{fetch_bytes, fetch_html, FetchOptions};
use turbo_surf_core::request_context::{
    ClientHints, Credentials, Destination, RequestContext, RequestMode,
};
use turbo_surf_core::url::resolve;

// Clear private handles before deno_core begins isolate disposal, including error
// exits and thread-local runtimes. Weak registry callbacks can otherwise outlive
// their Rust records during the isolate's own slot teardown.
#[path = "runtime_activation.rs"]
mod runtime_activation;
use runtime_activation::ActivationKey;

#[path = "form_navigation.rs"]
mod form_navigation;
#[path = "privacy_sandbox.rs"]
mod privacy_sandbox;
use privacy_sandbox::{op_private_token_issuer, op_private_token_query};
#[path = "form_submission.rs"]
mod form_submission;
#[path = "script_navigation.rs"]
mod script_navigation;
use form_submission::{op_form_data_entries, op_form_data_event, op_form_data_finish};
#[path = "user_activation.rs"]
mod user_activation;
#[path = "window_interfaces.rs"]
mod window_interfaces;
pub use user_activation::NavigationActivationSnapshot;
use user_activation::{
    host_user_activation, op_host_close_request, op_host_light_dismiss, op_host_modal_backdrop,
    op_host_user_activation, op_user_activation_install, op_user_activation_navigator,
    op_user_activation_read, op_user_activation_snapshot,
};
use window_interfaces::{op_location_ancestor_origins, op_location_object, op_location_seed};

struct OwnedRuntime(JsRuntime, bool, ActivationKey);

impl OwnedRuntime {
    fn park(&mut self) {
        if !self.1 {
            // Balance rusty_v8's constructor entry before a session can yield.
            unsafe { self.0.v8_isolate().exit() };
            self.1 = true;
        }
    }

    fn activation_key(&self) -> ActivationKey {
        self.2
    }
}

impl std::ops::Deref for OwnedRuntime {
    type Target = JsRuntime;
    fn deref(&self) -> &JsRuntime {
        &self.0
    }
}

impl std::ops::DerefMut for OwnedRuntime {
    fn deref_mut(&mut self) -> &mut JsRuntime {
        &mut self.0
    }
}

impl Drop for OwnedRuntime {
    fn drop(&mut self) {
        if self.1 {
            // OwnedIsolate::drop balances its constructor entry. A parked
            // session restores that entry for cleanup and underlying disposal.
            unsafe { self.0.v8_isolate().enter() };
        }
        let _native = crate::browser_env::activate_isolate(self.0.v8_isolate());
        crate::storage_gc::uninstall(self.0.v8_isolate());
        {
            deno_core::scope!(scope, self.0);
            crate::browser_env::clear_context_capabilities(scope);
            for context in crate::context_ownership::take_contexts(scope) {
                if let Some(context) = context.to_local(scope) {
                    context.remove_slot::<HostCapabilities>();
                    context.remove_slot::<FrameEnvironment>();
                    context.remove_slot::<FrameNavigator>();
                    context.remove_slot::<DocumentRuntimeStateAccessor>();
                    context.remove_slot::<RuntimeCore>();
                    context.remove_slot::<HostInputDriver>();
                    context.remove_slot::<FrameParent>();
                    context.remove_slot::<FrameHost>();
                    crate::crypto_clone::clear_context(context);
                    crate::message_ports::clear_context(context);
                }
            }
        }
        crate::browser_env::reset_isolate(self.0.v8_isolate());
        crate::browser_env::clear_interface_records(self.0.v8_isolate());
        crate::message_ports::clear(&mut self.0);
    }
}

#[cfg(turbo_surf_native_proxy)]
fn register_frame_modules(scope: &mut v8::PinScope, base: &str) -> Result<(), JsErrorBox> {
    let context = scope.get_current_context();
    if context
        .get_slot::<FrameSandbox>()
        .is_some_and(|policy| policy.scripts_blocked)
    {
        return Ok(());
    }
    let state = JsRuntime::op_state_from(scope);
    let state = state.borrow();
    let opaque_origin = window_is_opaque(scope, context);
    let credentials = ModuleCredentials::default();
    context.set_slot(Rc::new(credentials.clone()));
    let cookie_site_is_opaque = frame_cookie_site_is_opaque(scope, context);
    let loader = NetModuleLoader {
        base: base.to_string(),
        source: context
            .get_slot::<FrameDocumentUrl>()
            .map_or_else(|| state.borrow::<Base>().0.clone(), |url| url.0.clone()),
        top_level_source: state.borrow::<Base>().0.clone(),
        origins: Rc::new(ModuleOrigins::new(opaque_origin, cookie_site_is_opaque)),
        jar: context
            .get_slot::<CspHandle>()
            .and_then(|handle| handle.partition.clone())
            .map_or_else(
                || state.borrow::<Jar>().clone(),
                |partition| partition.jar.clone(),
            ),
        ua: state.borrow::<Ua>().0.clone(),
        client_hints: state.borrow::<ClientHints>().clone(),
        http_cache: state.borrow::<BrowserStorage>().http_cache(),
        storage_effects: state.borrow::<BrowserStorage>().partition_response_effects(
            context
                .get_slot::<CspHandle>()
                .and_then(|handle| handle.partition.clone())
                .map_or(0, |partition| partition.id),
        ),
        inline_sources: state.borrow::<InlineModules>().clone(),
        credentials,
        csp: (*context
            .get_slot::<CspHandle>()
            .ok_or_else(|| JsErrorBox::generic("Frame CSP context unavailable"))?)
        .clone(),
        module_stages: state.borrow::<ModuleStages>().clone(),
    };
    drop(state);
    if JsRuntime::register_module_context(scope, Rc::new(loader)) {
        Ok(())
    } else {
        Err(JsErrorBox::generic(
            "Native frame module context unavailable",
        ))
    }
}

/// Loads ES modules (a `<script type="module">`'s `import` graph) over the host net
/// layer — the same path `op_fetch` uses — so a Next dev / turbopack build (served as
/// ES modules) hydrates. Carries the page base + shared cookie jar so module fetches
/// are same-origin + session-authenticated like page fetches.
struct NetModuleLoader {
    base: String,
    source: String,
    top_level_source: String,
    origins: Rc<ModuleOrigins>,
    jar: Jar,
    ua: String,
    client_hints: ClientHints,
    http_cache: turbo_surf_core::http_cache::HttpCache,
    storage_effects: std::sync::Arc<turbo_surf_core::net::ResponseSideEffects<'static>>,
    inline_sources: InlineModules,
    credentials: ModuleCredentials,
    csp: CspHandle,
    #[cfg(turbo_surf_native_proxy)]
    module_stages: ModuleStages,
}

#[derive(Default)]
struct ModuleOrigins {
    opaque: std::cell::Cell<bool>,
    cookie_site_opaque: std::cell::Cell<bool>,
}
impl ModuleOrigins {
    fn new(opaque: bool, cookie_site_opaque: bool) -> Self {
        Self {
            opaque: std::cell::Cell::new(opaque),
            cookie_site_opaque: std::cell::Cell::new(cookie_site_opaque),
        }
    }
}

#[derive(Clone, Default)]
struct InlineModules(Rc<RefCell<std::collections::HashMap<String, InlineModule>>>);
struct InlineModule {
    code: String,
    #[cfg_attr(not(turbo_surf_native_proxy), allow(dead_code))]
    code_units: std::sync::Arc<[u16]>,
    base: String,
    #[cfg_attr(not(turbo_surf_native_proxy), allow(dead_code))]
    source: String,
}

#[derive(Clone)]
struct CspHandle {
    id: u32,
    partition: Option<Rc<CredentiallessPartition>>,
    policy: Rc<RefCell<crate::csp::DocumentPolicy>>,
    response_metadata: Rc<RefCell<crate::DocumentResponseMetadata>>,
    dictionaries: Rc<RefCell<compression_dictionary::Discovery>>,
}
impl CspHandle {
    fn new(base: &str, headers: &[(String, String)], html: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(1);
        Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            partition: None,
            dictionaries: Rc::new(RefCell::new(compression_dictionary::Discovery::new(
                base, headers,
            ))),
            response_metadata: Rc::new(RefCell::new(crate::DocumentResponseMetadata::default())),
            policy: Rc::new(RefCell::new(crate::csp::DocumentPolicy::new(
                base, headers, html,
            ))),
        }
    }
}
#[derive(Clone, Default)]
pub(crate) struct FetchPolicySnapshot {
    active: crate::csp::Policy,
    referrer: turbo_surf_core::referrer_policy::ReferrerPolicy,
    embedder: crate::resource_policy::EmbedderPolicy,
}
pub(crate) fn snapshot_fetch_policy(context: v8::Local<v8::Context>) -> FetchPolicySnapshot {
    context
        .get_slot::<CspHandle>()
        .map_or_else(FetchPolicySnapshot::default, |handle| {
            let policy = handle.policy.borrow();
            FetchPolicySnapshot {
                active: policy.active.clone(),
                referrer: policy.referrer_policy,
                embedder: policy.embedder_policy,
            }
        })
}
struct CredentiallessPartition {
    id: u32,
    jar: Jar,
    storage: BrowserStorage,
}
impl Drop for CredentiallessPartition {
    fn drop(&mut self) {
        let Ok(mut areas) = self.storage.0.lock() else {
            return;
        };
        let ids = areas
            .by_origin
            .iter()
            .filter_map(|(key, id)| (key.partition == self.id).then_some(*id))
            .collect::<Vec<_>>();
        areas.by_origin.retain(|key, _| key.partition != self.id);
        for id in ids {
            areas.values.remove(&id);
        }
    }
}
#[derive(Default)]
struct CredentiallessPartitions(std::collections::BTreeMap<u32, Rc<CredentiallessPartition>>);
fn frame_partition(
    scope: &mut v8::PinScope,
    state: &mut OpState,
    parent: v8::Local<v8::Context>,
    host: v8::Local<v8::Object>,
) -> Option<Rc<CredentiallessPartition>> {
    if let Some(partition) = parent
        .get_slot::<CspHandle>()
        .and_then(|handle| handle.partition.clone())
    {
        return Some(partition);
    }
    crate::browser_env::node_attribute(scope, host, "credentialless")?;
    let id = parent
        .get_slot::<SharedMemoryCapability>()
        .map(|capability| capability.group)
        .or_else(|| parent.get_slot::<CspHandle>().map(|handle| handle.id))?;
    if !state.has::<CredentiallessPartitions>() {
        state.put(CredentiallessPartitions::default());
    }
    if !state.has::<BrowserStorage>() {
        state.put(BrowserStorage::default());
    }
    let storage = state.borrow::<BrowserStorage>().clone();
    Some(
        state
            .borrow_mut::<CredentiallessPartitions>()
            .0
            .entry(id)
            .or_insert_with(|| {
                Rc::new(CredentiallessPartition {
                    id,
                    jar: Rc::new(RefCell::new(CookieJar::new())),
                    storage,
                })
            })
            .clone(),
    )
}
#[op2(fast)]
fn op_frame_request_partition(
    scope: &mut v8::PinScope,
    state: &mut OpState,
    document: v8::Local<v8::Object>,
    host: v8::Local<v8::Object>,
) -> u32 {
    document
        .get_creation_context(scope)
        .and_then(|parent| frame_partition(scope, state, parent, host))
        .map_or(0, |partition| partition.id)
}

#[derive(Default)]
struct CspContexts(std::collections::HashMap<u32, CspHandle>);
#[op2(fast)]
fn op_csp_context(scope: &mut v8::PinScope, document: v8::Local<v8::Object>) -> u32 {
    document
        .get_creation_context(scope)
        .and_then(|context| context.get_slot::<CspHandle>())
        .map_or(0, |handle| handle.id)
}
#[op2]
#[serde]
fn op_permissions_policy(
    scope: &mut v8::PinScope,
    document: v8::Local<v8::Object>,
    #[string] name: &str,
    #[string] target: &str,
    action: u32,
) -> serde_json::Value {
    let Some(context) = document.get_creation_context(scope) else {
        return serde_json::Value::Null;
    };
    let origin = window_origin(scope, context);
    let policy = context
        .get_slot::<CspHandle>()
        .map(|handle| handle.policy.borrow().permissions_policy.clone())
        .unwrap_or_default();
    permissions_policy_result(&policy, &origin, name, target, action)
}

/// The iframe's container policy: its `allow` attribute, then the legacy
/// `allowfullscreen` attribute, which grants fullscreen to every origin unless
/// `allow` already declares the feature (the first declaration wins).
fn frame_container_allow(scope: &mut v8::PinScope, host: v8::Local<v8::Object>) -> String {
    let allow = crate::browser_env::node_attribute(scope, host, "allow").unwrap_or_default();
    if crate::browser_env::node_attribute(scope, host, "allowfullscreen").is_some() {
        format!("{allow};fullscreen *")
    } else {
        allow
    }
}

/// `fullscreenEnabled`/`pictureInPictureEnabled`: the feature is enabled in the
/// document's own permissions policy, and the document is the active document of
/// a live browsing context (inert and detached documents report false).
#[op2(fast)]
fn op_document_feature_enabled(
    scope: &mut v8::PinScope,
    document: v8::Local<v8::Object>,
    #[string] feature: &str,
) -> bool {
    document_feature_enabled(scope, document, feature)
}

pub(crate) fn document_feature_enabled(
    scope: &mut v8::PinScope,
    document: v8::Local<v8::Object>,
    feature: &str,
) -> bool {
    let Some(context) = document.get_creation_context(scope) else {
        return false;
    };
    let active = context
        .get_slot::<ActiveDocument>()
        .and_then(|active| active.0.to_local(scope))
        .is_some_and(|active| active.strict_equals(document.into()));
    if !active || context.get_slot::<FrameRetired>().is_some() {
        return false;
    }
    let origin = window_origin(scope, context);
    context.get_slot::<CspHandle>().is_some_and(|handle| {
        handle
            .policy
            .borrow()
            .permissions_policy
            .allows_self(feature, &origin)
    })
}

#[op2(fast)]
fn op_document_origin_opaque(scope: &mut v8::PinScope, document: v8::Local<v8::Object>) -> bool {
    document
        .get_creation_context(scope)
        .is_some_and(|context| window_is_opaque(scope, context))
}

#[op2(fast)]
fn op_origin_agent_cluster(scope: &mut v8::PinScope, window: v8::Local<v8::Object>) -> bool {
    window
        .get_creation_context(scope)
        .and_then(|context| context.get_slot::<AgentClusterKeyed>())
        .is_none_or(|keyed| keyed.0)
}

fn permissions_policy_result(
    policy: &crate::permissions_policy::PermissionsPolicy,
    origin: &str,
    name: &str,
    target: &str,
    action: u32,
) -> serde_json::Value {
    match action {
        0 => serde_json::json!(crate::permissions_policy::PermissionsPolicy::features()),
        1 => serde_json::json!(policy.allowlist(name, origin)),
        2 => serde_json::json!(if target.is_empty() {
            policy.allows_self(name, origin)
        } else {
            policy.allows(name, origin, target)
        }),
        3 => serde_json::json!(crate::permissions_policy::PermissionsPolicy::features()
            .into_iter()
            .filter(|name| policy.allows_self(name, origin))
            .collect::<Vec<_>>()),
        4 => serde_json::json!(
            crate::permissions_policy::PermissionsPolicy::features().contains(&name)
        ),
        _ => serde_json::Value::Null,
    }
}
#[op2]
#[serde]
fn op_iframe_permissions_policy(
    scope: &mut v8::PinScope,
    document: v8::Local<v8::Object>,
    host: v8::Local<v8::Object>,
    #[string] base: &str,
    #[string] name: &str,
    #[string] target: &str,
    action: u32,
) -> serde_json::Value {
    let Some(parent) = document.get_creation_context(scope) else {
        return serde_json::Value::Null;
    };
    let parent_origin = window_origin(scope, parent);
    let src = crate::browser_env::node_attribute(scope, host, "src").unwrap_or_default();
    let inherited = crate::browser_env::node_attribute(scope, host, "srcdoc").is_some()
        || src.is_empty()
        || src == "about:blank";
    let source_origin = if inherited {
        parent_origin.clone()
    } else {
        resolve(base, &src)
            .and_then(|url| url::Url::parse(&url).ok())
            .map_or("null".into(), |url| url.origin().ascii_serialization())
    };
    let parent_policy = parent
        .get_slot::<CspHandle>()
        .map(|handle| handle.policy.borrow().permissions_policy.clone())
        .unwrap_or_default();
    let allow = frame_container_allow(scope, host);
    let mut policy = crate::permissions_policy::PermissionsPolicy::default();
    policy.inherit(
        &parent_policy,
        &parent_origin,
        &source_origin,
        &source_origin,
        &allow,
    );
    permissions_policy_result(&policy, &source_origin, name, target, action)
}

#[op2]
#[string]
fn op_csp_base_url(
    scope: &mut v8::PinScope,
    document: v8::Local<v8::Object>,
    #[string] href: &str,
    #[string] fallback: &str,
    initial: bool,
    enforce: bool,
) -> String {
    if document
        .get_creation_context(scope)
        .is_some_and(|context| context.get_slot::<FrameRetired>().is_some())
    {
        return fallback.to_string();
    }
    let Some(url) = resolve(fallback, href) else {
        return fallback.to_string();
    };
    if url::Url::parse(&url).is_ok_and(|url| matches!(url.scheme(), "data" | "javascript")) {
        return fallback.to_string();
    }
    let allowed = !enforce
        || document
            .get_creation_context(scope)
            .and_then(|context| context.get_slot::<CspHandle>())
            .is_none_or(|handle| {
                let policy = handle.policy.borrow();
                let policy = if initial {
                    &policy.initial_base
                } else {
                    policy.current()
                };
                policy.allows_request("base-uri", &url, "", false)
            });
    if allowed {
        url
    } else {
        fallback.to_string()
    }
}

#[op2(fast)]
fn op_csp_finish_parser(scope: &mut v8::PinScope, document: v8::Local<v8::Object>) {
    let Some(context) = document.get_creation_context(scope) else {
        return;
    };
    let Some(handle) = context.get_slot::<CspHandle>() else {
        return;
    };
    let mut policy = handle.policy.borrow_mut();
    policy.activate(usize::MAX);
    configure_csp_code_generation(scope, context, policy.current());
}

#[op2(fast)]
fn op_csp_meta(
    scope: &mut v8::PinScope,
    document: v8::Local<v8::Object>,
    meta: v8::Local<v8::Object>,
) {
    let Some(context) = document.get_creation_context(scope) else {
        return;
    };
    let Some(handle) = context.get_slot::<CspHandle>() else {
        return;
    };
    if !crate::browser_env::node_attribute(scope, meta, "http-equiv")
        .is_some_and(|value| value.eq_ignore_ascii_case("content-security-policy"))
    {
        return;
    }
    let Some(content) = crate::browser_env::node_attribute(scope, meta, "content") else {
        return;
    };
    let mut policy = handle.policy.borrow_mut();
    policy.active.add(&content);
    configure_csp_code_generation(scope, context, policy.current());
}

#[op2(fast)]
fn op_referrer_meta(
    scope: &mut v8::PinScope,
    document: v8::Local<v8::Object>,
    meta: v8::Local<v8::Object>,
) {
    if !crate::browser_env::node_attribute(scope, meta, "name")
        .is_some_and(|name| name.eq_ignore_ascii_case("referrer"))
    {
        return;
    }
    let Some(policy) = crate::browser_env::node_attribute(scope, meta, "content")
        .as_deref()
        .and_then(crate::referrer_meta::policy)
    else {
        return;
    };
    if let Some(handle) = document
        .get_creation_context(scope)
        .and_then(|context| context.get_slot::<CspHandle>())
    {
        handle.policy.borrow_mut().referrer_policy = policy;
    }
}

#[derive(Default)]
struct CspBootstrapTasks(std::cell::Cell<usize>);

fn csp_bootstrap_key<'s>(scope: &mut v8::PinScope<'s, '_>) -> Option<v8::Local<'s, v8::Private>> {
    let name = v8::String::new(scope, "turbo-surf.csp-bootstrap-tasks")?;
    Some(v8::Private::for_api(scope, Some(name)))
}

fn enqueue_csp_bootstrap<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    target: v8::Local<'s, v8::Object>,
    metadata: v8::Local<'s, v8::Value>,
    bytes: usize,
    parser_offset: Option<usize>,
) -> bool {
    let Some(key) = csp_bootstrap_key(scope) else {
        return false;
    };
    let global = context.global(scope);
    let tasks = global
        .get_private(scope, key)
        .and_then(|value| v8::Local::<v8::Array>::try_from(value).ok())
        .unwrap_or_else(|| v8::Array::new(scope, 0));
    let quota = context.get_slot::<CspBootstrapTasks>().unwrap_or_else(|| {
        let quota = Rc::new(CspBootstrapTasks::default());
        context.set_slot(quota.clone());
        quota
    });
    if tasks.length() >= 4096 || bytes > (64 * 1024 * 1024_usize).saturating_sub(quota.0.get()) {
        return false;
    }
    let offset = v8::Number::new(
        scope,
        parser_offset.map_or(f64::INFINITY, |value| value as f64),
    );
    let entry = v8::Array::new_with_elements(scope, &[target.into(), metadata, offset.into()]);
    if tasks.set_index(scope, tasks.length(), entry.into()) != Some(true)
        || global.set_private(scope, key, tasks.into()) != Some(true)
    {
        return false;
    }
    quota.0.set(quota.0.get() + bytes);
    true
}

fn flush_csp_bootstrap<'s>(scope: &mut v8::PinScope<'s, '_>, context: v8::Local<'s, v8::Context>) {
    let Some(key) = csp_bootstrap_key(scope) else {
        return;
    };
    let global = context.global(scope);
    let tasks = global
        .get_private(scope, key)
        .and_then(|value| v8::Local::<v8::Array>::try_from(value).ok());
    global.delete_private(scope, key);
    if let Some(quota) = context.get_slot::<CspBootstrapTasks>() {
        quota.0.set(0);
    }
    let Some(tasks) = tasks else {
        return;
    };
    let mut ordered = Vec::with_capacity(tasks.length() as usize);
    for index in 0..tasks.length() {
        let Some(entry) = tasks
            .get_index(scope, index)
            .and_then(|value| v8::Local::<v8::Array>::try_from(value).ok())
        else {
            continue;
        };
        let offset = entry
            .get_index(scope, 2)
            .and_then(|value| value.number_value(scope))
            .unwrap_or(f64::INFINITY);
        ordered.push((offset, entry));
    }
    ordered.sort_by(|left, right| left.0.total_cmp(&right.0));
    for (_, entry) in ordered {
        let Some(target) = entry.get_index(scope, 0) else {
            continue;
        };
        let Some(metadata) = entry.get_index(scope, 1) else {
            continue;
        };
        if call_frame_helper(scope, "queueCspViolation", &[target, metadata])
            .is_some_and(|value| value.is_false())
        {
            scope.terminate_execution();
            return;
        }
    }
}

#[derive(Clone, Copy)]
struct CspParserLocation {
    owner_window: bool,
    position: Option<turbo_dom_parser::rtdom::SourcePosition>,
}
struct CspViolationReport<'a> {
    kind: &'a str,
    blocked: &'a str,
    disposition: &'a str,
    records: Vec<crate::csp::ReportRecord>,
    parser: Option<CspParserLocation>,
    sample: Option<&'a [u16]>,
}
fn csp_sample_units(scope: &v8::PinScope, text: v8::Local<v8::String>) -> Vec<u16> {
    let whitespace = |unit: u16| matches!(unit, 9 | 10 | 11 | 12 | 13 | 32);
    let mut buffer = [0; 128];
    let length = text.length();
    let mut start = 0;
    while start < length {
        if scope.is_execution_terminating() {
            return Vec::new();
        }
        let count = (length - start).min(buffer.len());
        text.write_v2(
            scope,
            start as u32,
            &mut buffer[..count],
            v8::WriteFlags::empty(),
        );
        if let Some(index) = buffer[..count].iter().position(|&unit| !whitespace(unit)) {
            start += index;
            break;
        }
        start += count;
    }
    let mut end = length;
    while end > start {
        if scope.is_execution_terminating() {
            return Vec::new();
        }
        let count = (end - start).min(buffer.len());
        text.write_v2(
            scope,
            (end - count) as u32,
            &mut buffer[..count],
            v8::WriteFlags::empty(),
        );
        if let Some(index) = buffer[..count].iter().rposition(|&unit| !whitespace(unit)) {
            end = end - count + index + 1;
            break;
        }
        end -= count;
    }
    let mut units = vec![0; (end - start).min(40)];
    text.write_v2(scope, start as u32, &mut units, v8::WriteFlags::empty());
    units
}
fn queue_csp_report<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    element: v8::Local<'s, v8::Object>,
    report: CspViolationReport<'_>,
) {
    let CspViolationReport {
        kind,
        blocked,
        disposition,
        records,
        parser,
        sample,
    } = report;

    if records.is_empty() || scope.is_execution_terminating() {
        return;
    }
    let (mut source, mut line, mut column) = (String::new(), 0, 0);
    if parser.is_none() {
        if let Some(trace) = v8::StackTrace::current_stack_trace(scope, 32) {
            for index in 0..trace.get_frame_count() {
                let Some(frame) = trace.get_frame(scope, index) else {
                    continue;
                };
                let Some(name) = frame.get_script_name_or_source_url(scope) else {
                    continue;
                };
                let name = name.to_rust_string_lossy(scope);
                if name.starts_with("<") || name.is_empty() {
                    continue;
                }
                source = name;
                line = frame.get_line_number();
                column = frame.get_column();
                break;
            }
        }
    }
    if let Some(position) = parser.and_then(|metadata| metadata.position) {
        line = position.line;
        column = position.column;
    }
    let url = context.get_slot::<FrameDocumentLocation>().map_or_else(
        || {
            JsRuntime::op_state_from(scope)
                .borrow()
                .borrow::<Base>()
                .0
                .clone()
        },
        |value| value.0.clone(),
    );
    if source.is_empty() {
        source = url::Url::parse(&url)
            .map(|mut url| {
                url.set_query(None);
                url.set_fragment(None);
                url.to_string()
            })
            .unwrap_or_else(|_| url.clone());
    }
    if parser.is_some_and(|metadata| !metadata.owner_window) || kind == "img-src" {
        source.clear();
        line = 0;
        column = 0;
    }
    let response_metadata = context
        .get_slot::<CspHandle>()
        .map_or_else(crate::DocumentResponseMetadata::default, |handle| {
            handle.response_metadata.borrow().clone()
        });
    let scope = &mut v8::ContextScope::new(scope, context);
    for record in records {
        let original = &record.original;
        let report_sample = record.sample;
        let sample_units = if report_sample {
            sample.unwrap_or_default()
        } else {
            &[]
        };
        let metadata = serde_json::json!({"documentURI":crate::document_response::report_url(&url),"referrer":response_metadata.referrer,"blockedURI":blocked,"violatedDirective":kind,"effectiveDirective":kind,"originalPolicy":original,"disposition":disposition,"sourceFile":source,"statusCode":response_metadata.status,"lineNumber":line,"columnNumber":column,"sample":""});
        let report_payload = metadata.clone();
        let bytes = metadata
            .to_string()
            .len()
            .saturating_mul(2)
            .saturating_add(sample_units.len().saturating_mul(2))
            .saturating_add(104);
        let Some(json) = v8::String::new(scope, &metadata.to_string()) else {
            return;
        };
        let Some(metadata) = v8::json::parse(scope, json) else {
            return;
        };
        let Some(sample) =
            v8::String::new_from_two_byte(scope, sample_units, v8::NewStringType::Normal)
        else {
            return;
        };
        let key = v8::String::new(scope, "sample").unwrap();
        let metadata = v8::Local::<v8::Object>::try_from(metadata).unwrap();
        metadata.set(scope, key.into(), sample.into());
        let metadata: v8::Local<v8::Value> = metadata.into();
        let accepted = call_frame_helper(scope, "queueCspViolation", &[element.into(), metadata])
            .map_or_else(
                || {
                    enqueue_csp_bootstrap(
                        scope,
                        context,
                        element,
                        metadata,
                        bytes,
                        parser
                            .and_then(|metadata| metadata.position)
                            .map(|position| position.utf16_offset),
                    )
                },
                |value| !value.is_false(),
            );
        if !accepted {
            scope.terminate_execution();
            return;
        }
        send_csp_report(
            scope,
            context,
            &url,
            &record.destination,
            &report_payload,
            sample_units,
        );
    }
}

fn send_csp_report<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    source: &str,
    destination: &crate::csp::ReportDestination,
    metadata: &serde_json::Value,
    sample: &[u16],
) {
    use turbo_surf_core::request_context::{Credentials, Destination, RequestContext, RequestMode};
    let Some(handle) = context.get_slot::<CspHandle>() else {
        return;
    };
    let endpoints = match destination {
        crate::csp::ReportDestination::None => Vec::new(),
        crate::csp::ReportDestination::Legacy(values) => values
            .iter()
            .take(4096)
            .filter_map(|value| url::Url::parse(source).ok()?.join(value).ok())
            .filter(|url| matches!(url.scheme(), "http" | "https"))
            .map(|url| crate::csp_reporting::Endpoint {
                url: url.into(),
                modern: false,
                credentials: true,
            })
            .collect(),
        crate::csp::ReportDestination::Group(group) => handle
            .policy
            .borrow()
            .reporting_endpoints
            .get(group)
            .cloned()
            .into_iter()
            .collect(),
    };
    if endpoints.is_empty() {
        return;
    }
    let mut request = RequestContext::resource(source, Destination::Report, RequestMode::NoCors);
    request.credentials = Credentials::SameOrigin;
    request.opaque_origin = window_is_opaque(scope, context);
    request.cookie_site_is_opaque = frame_cookie_site_is_opaque(scope, context);
    request.referrer_policy = handle.policy.borrow().referrer_policy;
    let state_rc = JsRuntime::op_state_from(scope);
    let mut state = state_rc.borrow_mut();
    if !state.has::<BrowserStorage>() {
        state.put(BrowserStorage::default());
    }
    if !state.has::<ReportCookieTargets>() {
        state.put(ReportCookieTargets::default());
    }
    apply_report_cookies(&mut state);
    request.top_level_source = Some(state.borrow::<Base>().0.clone());
    let jar = handle.partition.as_ref().map_or_else(
        || state.borrow::<Jar>().clone(),
        |partition| partition.jar.clone(),
    );
    let target = handle
        .partition
        .as_ref()
        .map_or(0, |partition| u64::from(partition.id));
    state
        .borrow_mut::<ReportCookieTargets>()
        .0
        .insert(target, Rc::downgrade(&jar));
    let reporter = state.borrow::<BrowserStorage>().3.clone();
    for endpoint in endpoints {
        let cookies = jar.borrow().fork_for_request();
        let body = crate::csp_reporting::body(metadata, sample, endpoint.modern);
        reporter.send(crate::csp_reporting::ReportJob {
            endpoint,
            context: request.clone(),
            jar: cookies,
            target,
            client_hints: state.borrow::<ClientHints>().clone(),
            body,
            response_effects: Some(
                state
                    .borrow::<BrowserStorage>()
                    .partition_response_effects(target as u32),
            ),
        });
    }
}

fn queue_csp_violations<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    element: v8::Local<'s, v8::Object>,
    kind: &str,
    blocked: &str,
    disposition: &str,
    records: Vec<crate::csp::ReportRecord>,
) {
    queue_csp_report(
        scope,
        context,
        element,
        CspViolationReport {
            kind,
            blocked,
            disposition,
            records,
            parser: None,
            sample: None,
        },
    );
}

#[op2(fast, reentrant)]
fn op_csp_script<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    element: v8::Local<'s, v8::Object>,
    index: u32,
    #[string] url: &str,
    text: v8::Local<'s, v8::String>,
) -> bool {
    let Some(context) = document.get_creation_context(scope) else {
        return false;
    };
    let Some(handle) = context.get_slot::<CspHandle>() else {
        return true;
    };
    let sample = csp_sample_units(scope, text);
    let text = text.to_rust_string_lossy(scope);
    let nonce = crate::browser_env::node_nonce(scope, element).unwrap_or_default();
    let mut document_policy = handle.policy.borrow_mut();
    document_policy.activate(index as usize);
    configure_csp_code_generation(scope, context, document_policy.current());
    if document_policy.current().has_policy() {
        crate::browser_env::hide_node_nonce(scope, element);
    }
    if url.is_empty() {
        let policy = document_policy.current().clone();
        let report_only = document_policy.report_only.clone();
        drop(document_policy);
        let allowed = policy.allows_inline("script-src-elem", &text, &nonce);
        queue_csp_report(
            scope,
            context,
            element,
            CspViolationReport {
                kind: "script-src-elem",
                blocked: "inline",
                disposition: "enforce",
                records: policy.inline_violations("script-src-elem", &text, &nonce),
                parser: None,
                sample: Some(&sample),
            },
        );
        queue_csp_report(
            scope,
            context,
            element,
            CspViolationReport {
                kind: "script-src-elem",
                blocked: "inline",
                disposition: "report",
                records: report_only.inline_violations("script-src-elem", &text, &nonce),
                parser: None,
                sample: Some(&sample),
            },
        );
        allowed
    } else {
        let policy = document_policy.current().clone();
        drop(document_policy);
        let allowed = policy.allows_request("script-src-elem", url, &nonce, false);
        if !allowed {
            let blocked = csp_blocked_url(scope, context, url);
            queue_csp_violations(
                scope,
                context,
                document,
                "script-src-elem",
                &blocked,
                "enforce",
                policy.request_violations("script-src-elem", url, &nonce),
            );
        }
        allowed
    }
}
fn csp_blocked_url(
    scope: &mut v8::PinScope,
    context: v8::Local<v8::Context>,
    target: &str,
) -> String {
    if let Ok(mut url) = url::Url::parse(target) {
        url.set_fragment(None);
        if window_origin(scope, context) == url.origin().ascii_serialization() {
            url.to_string()
        } else {
            url.origin().ascii_serialization()
        }
    } else {
        target.to_string()
    }
}
#[op2(fast, reentrant)]
fn op_csp_request<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    #[string] target: &str,
    #[string] kind: &str,
    #[string] nonce: &str,
) -> bool {
    csp_request(scope, document, target, kind, nonce)
}

fn csp_request<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    target: &str,
    kind: &str,
    nonce: &str,
) -> bool {
    let Some(context) = document.get_creation_context(scope) else {
        return false;
    };
    csp_request_for_context(scope, context, document, target, kind, nonce)
}

fn csp_request_for_context<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    document: v8::Local<'s, v8::Object>,
    target: &str,
    kind: &str,
    nonce: &str,
) -> bool {
    let Some(handle) = context.get_slot::<CspHandle>() else {
        return true;
    };
    let policy = handle.policy.borrow().active.clone();
    let allowed = policy.allows_request(kind, target, nonce, false);
    let blocked = csp_blocked_url(scope, context, target);
    queue_csp_violations(
        scope,
        context,
        document,
        kind,
        &blocked,
        "enforce",
        policy.request_violations(kind, target, nonce),
    );
    let report_only = handle.policy.borrow().report_only.clone();
    queue_csp_violations(
        scope,
        context,
        document,
        kind,
        &blocked,
        "report",
        report_only.request_violations(kind, target, nonce),
    );
    if allowed {
        // Chrome dispatches pre/post-request report-only events, but uploads the
        // identical violation once. Duplicate URI tokens still upload separately.
        let records = report_only
            .request_violations(kind, target, nonce)
            .into_iter()
            .map(|mut record| {
                record.destination = crate::csp::ReportDestination::None;
                record
            })
            .collect();
        queue_csp_violations(scope, context, document, kind, &blocked, "report", records);
    }
    allowed
}

#[op2(fast)]
fn op_csp_inline(
    scope: &mut v8::PinScope,
    document: v8::Local<v8::Object>,
    element: v8::Local<v8::Object>,
    #[string] kind: &str,
    #[string] text: &str,
) -> bool {
    let Some(context) = document.get_creation_context(scope) else {
        return false;
    };
    let Some(handle) = context.get_slot::<CspHandle>() else {
        return true;
    };
    let nonce = crate::browser_env::node_nonce(scope, element).unwrap_or_default();
    let allowed = handle
        .policy
        .borrow()
        .active
        .allows_inline(kind, text, &nonce);
    allowed
}

fn authorize_css(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let Some(document) = args.get(0).to_object(scope) else {
        result.set_bool(false);
        return;
    };
    let Some(context) = document.get_creation_context(scope) else {
        result.set_bool(false);
        return;
    };
    let Some(handle) = context.get_slot::<CspHandle>() else {
        result.set_bool(true);
        return;
    };
    let Some(element) = args.get(1).to_object(scope) else {
        result.set_bool(false);
        return;
    };
    let kind = args.get(2).to_rust_string_lossy(scope);
    if kind == "script-src-attr"
        && context
            .get_slot::<FrameSandbox>()
            .is_some_and(|policy| policy.scripts_blocked)
    {
        result.set_bool(false);
        return;
    }
    let sample = v8::Local::<v8::String>::try_from(args.get(3))
        .ok()
        .map(|text| csp_sample_units(scope, text))
        .unwrap_or_default();
    let text = args.get(3).to_rust_string_lossy(scope);
    let initial = args.get(4);
    let mut policy = if initial.is_array() {
        handle.policy.borrow().header.clone()
    } else {
        handle.policy.borrow().current().clone()
    };
    if let Ok(metas) = v8::Local::<v8::Array>::try_from(initial) {
        for index in 0..metas.length() {
            if let Some(value) = metas.get_index(scope, index) {
                policy.add(&value.to_rust_string_lossy(scope));
            }
        }
    }
    let parser = initial.is_array().then(|| CspParserLocation {
        owner_window: crate::browser_env::node_has_window_owner(scope, element),
        position: crate::browser_env::node_parser_position(
            scope,
            element,
            (kind == "style-src-attr").then_some("style"),
        ),
    });
    let nonce = crate::browser_env::node_nonce(scope, element).unwrap_or_default();
    let allowed = policy.allows_inline(&kind, &text, &nonce);
    if policy.has_policy() {
        crate::browser_env::hide_node_nonce(scope, element);
    }
    let target = if parser.is_some_and(|metadata| !metadata.owner_window) {
        document
    } else {
        element
    };
    queue_csp_report(
        scope,
        context,
        target,
        CspViolationReport {
            kind: &kind,
            blocked: "inline",
            disposition: "enforce",
            records: policy.inline_violations(&kind, &text, &nonce),
            parser,
            sample: Some(&sample),
        },
    );
    let report_only = handle.policy.borrow().report_only.clone();
    queue_csp_report(
        scope,
        context,
        target,
        CspViolationReport {
            kind: &kind,
            blocked: "inline",
            disposition: "report",
            records: report_only.inline_violations(&kind, &text, &nonce),
            parser,
            sample: Some(&sample),
        },
    );
    result.set_bool(allowed);
}

fn configure_csp_code_generation(
    scope: &mut v8::PinScope,
    context: v8::Local<v8::Context>,
    policy: &crate::csp::Policy,
) {
    let allowed = policy.allows_eval();
    #[cfg(turbo_surf_native_proxy)]
    let allowed = {
        let key = v8::String::new(scope, "turbo-surf.code-generation-callback").unwrap();
        let key = v8::Private::for_api(scope, Some(key));
        let global = context.global(scope);
        allowed
            && !global
                .get_private(scope, key)
                .is_some_and(|callback| callback.is_function())
    };
    #[cfg(not(turbo_surf_native_proxy))]
    // Published SDKs lack the private native source-conversion callback.
    // Enforced Trusted Types must fail closed rather than admit raw eval.
    let allowed =
        policy.bypasses_trusted_types_eval() || allowed && !policy.requires_trusted_types();
    #[cfg(not(turbo_surf_native_proxy))]
    let _ = scope;
    context.set_allow_generation_from_strings(allowed);
}

#[cfg(turbo_surf_native_proxy)]
fn csp_eval_sample_units(scope: &v8::PinScope, source: v8::Local<v8::String>) -> Vec<u16> {
    // Chrome's code-generation path supplies a 40-unit prefix to the report
    // builder, which then strips ASCII whitespace. Inline reports trim first.
    let mut units = vec![0; source.length().min(40)];
    source.write_v2(scope, 0, &mut units, v8::WriteFlags::empty());
    let whitespace = |unit: &u16| matches!(unit, 9 | 10 | 11 | 12 | 13 | 32);
    let start = units
        .iter()
        .position(|unit| !whitespace(unit))
        .unwrap_or(units.len());
    let end = units
        .iter()
        .rposition(|unit| !whitespace(unit))
        .map_or(start, |index| index + 1);
    units[start..end].to_vec()
}

#[cfg(turbo_surf_native_proxy)]
fn deny_code_generation<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    mut result: v8::ReturnValue,
    message: &str,
) {
    let message = v8::String::new(scope, message).unwrap();
    let detail = v8::Array::new(scope, 1);
    detail.set_index(scope, 0, message.into());
    result.set(detail.into());
}
#[cfg(turbo_surf_native_proxy)]
fn authorize_code_generation<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut result: v8::ReturnValue,
) {
    let context = scope.get_current_context();
    let typed = crate::browser_env::trusted_script_source(scope, args.get(0));
    let original = match typed.or_else(|| v8::Local::<v8::String>::try_from(args.get(0)).ok()) {
        Some(source) => source,
        None => {
            result.set_bool(true);
            return;
        }
    };
    if context.get_slot::<FrameRetired>().is_some() {
        result.set_bool(false);
        return;
    }
    let Some(handle) = context.get_slot::<CspHandle>() else {
        result.set_bool(context.is_code_generation_from_strings_allowed());
        return;
    };
    let policy = handle.policy.borrow().current().clone();
    let report_only = handle.policy.borrow().report_only.clone();
    if policy.bypasses_trusted_types_eval() {
        result.set_bool(true);
        return;
    }
    let key = v8::String::new(scope, "turbo-dom.ownership.window-document").unwrap();
    let key = v8::Private::for_api(scope, Some(key));
    let Some(document) = context
        .global(scope)
        .get_private(scope, key)
        .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
    else {
        result.set_bool(false);
        return;
    };
    // Chromium labels generated constructor source using these engine prefixes.
    // Source admission itself always stays in the real V8 compilation callback.
    let text = original.to_rust_string_lossy(scope);
    let sink = if [
        "(function anonymous",
        "(async function anonymous",
        "(function* anonymous",
        "(async function* anonymous",
    ]
    .iter()
    .any(|prefix| text.starts_with(prefix))
    {
        "Function"
    } else {
        "eval"
    };
    v8::tc_scope!(let scope,scope);
    let source = crate::browser_env::trusted_type_string(
        scope,
        document,
        args.get(0),
        "TrustedScript",
        sink,
        "",
    );
    let message=policy.eval_error_message().unwrap_or_else(||"Evaluating a string as JavaScript violates this document's Trusted Type assignment requirements.".into());
    let Some(source) = source else {
        scope.reset();
        deny_code_generation(scope, result, &message);
        return;
    };
    if crate::browser_env::string_code_units(scope, source)
        != crate::browser_env::string_code_units(scope, original)
    {
        deny_code_generation(scope, result, &message);
        return;
    }
    let sample = csp_eval_sample_units(scope, source);
    for (disposition, records) in [
        ("enforce", policy.eval_violations()),
        ("report", report_only.eval_violations()),
    ] {
        queue_csp_report(
            scope,
            context,
            document,
            CspViolationReport {
                kind: "script-src",
                blocked: "eval",
                disposition,
                records,
                parser: None,
                sample: Some(&sample),
            },
        );
    }
    if policy.allows_eval() {
        result.set(source.into());
    } else {
        deny_code_generation(scope, result, &message);
    }
}
#[cfg(turbo_surf_native_proxy)]
fn install_code_generation_authorization(scope: &mut v8::PinScope) -> Result<(), String> {
    let context = scope.get_current_context();
    let callback = v8::Function::new(scope, authorize_code_generation)
        .ok_or("Native code-generation authorization unavailable")?;
    if context.set_code_generation_callback(callback) {
        context.set_allow_generation_from_strings(false);
        Ok(())
    } else {
        Err("Native code-generation authorization unavailable".into())
    }
}

fn authorize_trusted_types(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let context = scope.get_current_context();
    let Some(handle) = context.get_slot::<CspHandle>() else {
        result.set_bool(true);
        return;
    };
    let policy = handle.policy.borrow().current().clone();
    let report_only = handle.policy.borrow().report_only.clone();
    let action = args.get(0).to_rust_string_lossy(scope);
    if action == "required" {
        result.set_bool(policy.requires_trusted_types() || report_only.requires_trusted_types());
        return;
    }
    let key = v8::String::new(scope, "turbo-dom.ownership.window-document").unwrap();
    let key = v8::Private::for_api(scope, Some(key));
    let document = context
        .global(scope)
        .get_private(scope, key)
        .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok());
    let Some(document) = document else {
        result.set_bool(false);
        return;
    };
    let (kind, blocked, sample, name) = if action == "policy" {
        let value = args.get(1).to_rust_string_lossy(scope);
        let duplicate = args.get(2).is_true();
        let value_string = v8::Local::<v8::String>::try_from(args.get(1)).unwrap();
        let sample = crate::browser_env::string_code_units(scope, value_string)
            .into_iter()
            .take(40)
            .collect::<Vec<_>>();
        (
            "trusted-types",
            "trusted-types-policy",
            sample,
            Some((value, duplicate)),
        )
    } else {
        let sink = args.get(2).to_rust_string_lossy(scope);
        let text = v8::Local::<v8::String>::try_from(args.get(3)).unwrap();
        let mut content = crate::browser_env::string_code_units(scope, text);
        if sink == "Function" {
            let source = text.to_rust_string_lossy(scope);
            for prefix in [
                "(function anonymous",
                "(async function anonymous",
                "(function* anonymous",
                "(async function* anonymous",
            ] {
                if source.starts_with(prefix) {
                    content.drain(..prefix.len());
                    break;
                }
            }
        }
        let sample = format!("{sink}|")
            .encode_utf16()
            .chain(content.into_iter().take(40))
            .collect::<Vec<_>>();
        (
            "require-trusted-types-for",
            "trusted-types-sink",
            sample,
            None,
        )
    };
    let name = name
        .as_ref()
        .map(|(name, duplicate)| (name.as_str(), *duplicate));
    let enforced = policy.trusted_type_violations(name);
    let allowed = enforced.is_empty();
    for (disposition, records) in [
        ("enforce", enforced),
        ("report", report_only.trusted_type_violations(name)),
    ] {
        queue_csp_report(
            scope,
            context,
            document,
            CspViolationReport {
                kind,
                blocked,
                disposition,
                records,
                parser: None,
                sample: Some(&sample),
            },
        );
    }
    result.set_bool(allowed);
}

#[op2(reentrant)]
fn op_trusted_type_string<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    input: v8::Local<'s, v8::Value>,
    #[string] kind: &str,
    #[string] sink: &str,
    #[string] prefix: &str,
) -> v8::Local<'s, v8::Value> {
    crate::browser_env::trusted_type_string(scope, document, input, kind, sink, prefix)
        .map_or_else(|| v8::undefined(scope).into(), Into::into)
}

#[op2(fast, reentrant)]
fn op_timer_code_allowed<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    source: v8::Local<'s, v8::String>,
) -> bool {
    let context = scope.get_current_context();
    let Some(handle) = context.get_slot::<CspHandle>() else {
        return false;
    };
    if context.get_slot::<FrameRetired>().is_some() {
        return false;
    }
    let policy = handle.policy.borrow().current().clone();
    let report_only = handle.policy.borrow().report_only.clone();
    let units = crate::browser_env::string_code_units(scope, source);
    let sample = &units[..units.len().min(40)];
    for (disposition, records) in [
        ("enforce", policy.eval_violations()),
        ("report", report_only.eval_violations()),
    ] {
        queue_csp_report(
            scope,
            context,
            document,
            CspViolationReport {
                kind: "script-src",
                blocked: "eval",
                disposition,
                records,
                parser: None,
                sample: Some(sample),
            },
        );
    }
    policy.allows_eval()
}
#[op2(reentrant)]
fn op_run_timer_code<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    source: v8::Local<'s, v8::String>,
) -> v8::Local<'s, v8::Value> {
    let context = scope.get_current_context();
    if context.get_slot::<FrameRetired>().is_some() {
        return v8::undefined(scope).into();
    }
    let url = context.get_slot::<FrameDocumentLocation>().map_or_else(
        || {
            JsRuntime::op_state_from(scope)
                .borrow()
                .borrow::<Base>()
                .0
                .clone()
        },
        |value| value.0.clone(),
    );
    let name = v8::String::new(scope, &url).unwrap();
    let origin = v8::ScriptOrigin::new(
        scope,
        name.into(),
        0,
        0,
        false,
        -1,
        None,
        false,
        false,
        false,
        None,
    );
    v8::Script::compile(scope, source, Some(&origin))
        .and_then(|script| script.run(scope))
        .unwrap_or_else(|| v8::undefined(scope).into())
}

fn install_css_authorization(scope: &mut v8::PinScope) -> Result<(), JsErrorBox> {
    let forms_blocked = scope
        .get_current_context()
        .get_slot::<FrameSandbox>()
        .is_some_and(|flags| flags.forms_blocked);
    crate::browser_env::set_form_submission_blocked(scope, forms_blocked);
    form_submission::install(scope)?;
    let callback = v8::Function::new(scope, authorize_css)
        .ok_or_else(|| JsErrorBox::generic("native CSS callback initialization interrupted"))?;
    crate::browser_env::install_css_policy(scope, callback);
    let trusted = v8::Function::new(scope, authorize_trusted_types).ok_or_else(|| {
        JsErrorBox::generic("native Trusted Types callback initialization interrupted")
    })?;
    if !crate::browser_env::install_trusted_types_policy(scope, trusted) {
        return Err(JsErrorBox::generic(
            "native Trusted Types authorization initialization interrupted",
        ));
    }
    let context = scope.get_current_context();
    let base = context.get_slot::<CspHandle>().map_or_else(
        || "about:blank".to_string(),
        |handle| {
            let policy = handle.policy.borrow();
            configure_csp_code_generation(scope, context, policy.current());
            policy.header.origin().to_string()
        },
    );
    crate::browser_env::install_inline_event_handlers(scope, callback, &base);
    Ok(())
}

#[op2(fast, reentrant)]
fn op_frame_script_allowed(
    scope: &mut v8::PinScope,
    window: v8::Local<v8::Object>,
    index: u32,
) -> bool {
    let Some(context) = window.get_creation_context(scope) else {
        return false;
    };
    let scope = &mut v8::ContextScope::new(scope, context);
    let index = v8::Integer::new_from_unsigned(scope, index);
    call_frame_helper(scope, "cspFrameScript", &[index.into()])
        .is_some_and(|value| value.boolean_value(scope))
}

#[op2(fast, reentrant)]
fn op_frame_script_event(
    scope: &mut v8::PinScope,
    window: v8::Local<v8::Object>,
    index: u32,
    success: bool,
) {
    let Some(context) = window.get_creation_context(scope) else {
        return;
    };
    if context.get_slot::<FrameRetired>().is_some()
        || context.get_slot::<FrameContextMarker>().is_none()
    {
        return;
    }
    let scope = &mut v8::ContextScope::new(scope, context);
    let index = v8::Integer::new_from_unsigned(scope, index);
    let kind = v8::String::new(scope, if success { "load" } else { "error" }).unwrap();
    call_frame_helper(scope, "fireFrameScript", &[index.into(), kind.into()]);
}
#[op2]
#[string]
fn op_csp_nonce(scope: &mut v8::PinScope, element: v8::Local<v8::Object>) -> String {
    crate::browser_env::node_nonce(scope, element).unwrap_or_default()
}

#[derive(Clone, Default)]
struct ModuleCredentials(Rc<RefCell<std::collections::HashMap<String, ModuleRequest>>>);
#[derive(Clone)]
struct ModuleRequest {
    credentials: Credentials,
    nonce: String,
    integrity: String,
    policy: crate::csp::Policy,
}

#[op2(fast)]
fn op_module_credentials(
    scope: &mut v8::PinScope,
    element: v8::Local<v8::Object>,
    #[string] specifier: String,
) -> Result<(), JsErrorBox> {
    let context = scope.get_current_context();
    let options = context
        .get_slot::<ModuleCredentials>()
        .ok_or_else(|| JsErrorBox::generic("Module context unavailable"))?;
    let value =
        crate::browser_env::node_attribute(scope, element, "crossorigin").unwrap_or_default();
    let credentials = if value.eq_ignore_ascii_case("use-credentials") {
        Credentials::Include
    } else {
        Credentials::SameOrigin
    };
    let nonce = crate::browser_env::node_nonce(scope, element).unwrap_or_default();
    let integrity =
        crate::browser_env::node_attribute(scope, element, "integrity").unwrap_or_default();
    let policy = context
        .get_slot::<CspHandle>()
        .map(|handle| handle.policy.borrow().active.clone())
        .unwrap_or_default();
    options
        .0
        .borrow_mut()
        .entry(specifier)
        .or_insert(ModuleRequest {
            credentials,
            nonce,
            integrity,
            policy,
        });
    Ok(())
}

#[cfg(turbo_surf_native_proxy)]
#[derive(Clone, Default)]
struct ModuleStages(Rc<RefCell<std::collections::VecDeque<ModuleStage>>>);
#[cfg(turbo_surf_native_proxy)]
struct ModuleStage {
    context: v8::Global<v8::Context>,
    specifier: String,
    ready: bool,
    exception: Option<(v8::Global<v8::Value>, String, u32, u32)>,
}
#[op2(fast)]
fn op_module_staging_enabled() -> bool {
    cfg!(turbo_surf_native_proxy)
}

#[op2]
#[string]
fn op_inline_module(
    scope: &mut v8::PinScope,
    state: &mut OpState,
    code: v8::Local<v8::String>,
    #[string] base: &str,
    #[string] source: String,
) -> String {
    let code_units = crate::browser_env::string_code_units(scope, code);
    let code = String::from_utf16_lossy(&code_units);
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let specifier = format!(
        "{}#ts-inline-module-{}",
        base.split('#').next().unwrap_or(base),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    state.borrow::<InlineModules>().0.borrow_mut().insert(
        specifier.clone(),
        InlineModule {
            code,
            code_units: code_units.into(),
            base: base.to_string(),
            source,
        },
    );
    specifier
}

impl ModuleLoader for NetModuleLoader {
    #[cfg(turbo_surf_native_proxy)]
    fn module_source_url(&self, specifier: &str) -> String {
        self.inline_sources
            .0
            .borrow()
            .get(specifier)
            .map(|source| source.source.clone())
            .unwrap_or_else(|| specifier.to_string())
    }

    #[cfg(turbo_surf_native_proxy)]
    fn module_source_code_units(&self, specifier: &str) -> Option<std::sync::Arc<[u16]>> {
        self.inline_sources
            .0
            .borrow()
            .get(specifier)
            .map(|source| source.code_units.clone())
    }

    #[cfg(turbo_surf_native_proxy)]
    fn module_graph_ready(
        &self,
        scope: &mut v8::PinScope,
        specifier: &ModuleSpecifier,
        ready: bool,
        exception: Option<v8::Local<v8::Value>>,
    ) {
        let exception = exception.map(|error| {
            let message = v8::Exception::create_message(scope, error);
            let source = message
                .get_script_resource_name(scope)
                .map(|value| value.to_rust_string_lossy(scope))
                .unwrap_or_default();
            let line = message.get_line_number(scope).unwrap_or(0) as u32;
            let column = message.get_start_column() as u32 + 1;
            (v8::Global::new(scope, error), source, line, column)
        });
        let context = scope.get_current_context();
        let context = v8::Global::new(scope, context);
        self.module_stages.0.borrow_mut().push_back(ModuleStage {
            context,
            specifier: specifier.to_string(),
            ready,
            exception,
        });
    }

    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: ResolutionKind,
    ) -> ModuleResolveResponse {
        // Resolve against the referrer; fall back to the page base for the entry module.
        let referrer = if referrer.is_empty() || referrer == "." {
            &self.base
        } else {
            referrer
        };
        let sources = self.inline_sources.0.borrow();
        let referrer = sources
            .get(referrer)
            .map(|source| source.base.as_str())
            .unwrap_or(referrer);
        resolve_import(specifier, referrer).map_err(|e| JsErrorBox::generic(e.to_string()))
    }

    fn load(
        &self,
        module_specifier: &ModuleSpecifier,
        maybe_referrer: Option<&ModuleLoadReferrer>,
        _options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        let url = module_specifier.clone();
        if let Some(code) = self
            .inline_sources
            .0
            .borrow()
            .get(url.as_str())
            .map(|source| source.code.clone())
        {
            return ModuleLoadResponse::Sync(Ok(ModuleSource::new(
                ModuleType::JavaScript,
                ModuleSourceCode::String(code.into()),
                &url,
                None,
            )));
        }
        let credentials = {
            let mut options = self.credentials.0.borrow_mut();
            let mut inherited = maybe_referrer
                .and_then(|referrer| options.get(referrer.specifier.as_str()).cloned())
                .unwrap_or_else(|| ModuleRequest {
                    credentials: Credentials::SameOrigin,
                    nonce: String::new(),
                    integrity: String::new(),
                    policy: self.csp.policy.borrow().active.clone(),
                });
            // A script element's integrity covers its entry response, not the
            // bytes of every module imported by that entry.
            inherited.integrity.clear();
            options.entry(url.to_string()).or_insert(inherited).clone()
        };
        let module_credentials = self.credentials.clone();
        let jar = self.jar.clone();
        let ua = self.ua.clone();
        let base = self.source.clone();
        let top_level_source = self.top_level_source.clone();
        let cookie_site_is_opaque = self.origins.cookie_site_opaque.get();
        let opaque_origin = self.origins.opaque.get();
        let client_hints = self.client_hints.clone();
        let http_cache = self.http_cache.clone();
        let storage_effects = self.storage_effects.clone();
        let cache_partition = http_cache_partition(
            &self.top_level_source,
            &self.source,
            self.csp
                .partition
                .as_ref()
                .map_or(0, |partition| partition.id),
            if opaque_origin { self.csp.id } else { 0 },
        );
        ModuleLoadResponse::Async(Box::pin(async move {
            let mut local = jar.borrow().fork_for_request();
            let mut headers = std::collections::BTreeMap::new();
            if !ua.is_empty() {
                headers.insert("user-agent".to_string(), ua);
            }
            let mut context =
                RequestContext::resource(&base, Destination::Script, RequestMode::Cors);
            context.opaque_origin = opaque_origin;
            context.top_level_source = Some(top_level_source);
            context.cookie_site_is_opaque = cookie_site_is_opaque;
            context.credentials = credentials.credentials;
            let allowed = |target: &str, redirected: bool| {
                credentials.policy.allows_request(
                    "script-src-elem",
                    target,
                    &credentials.nonce,
                    redirected,
                )
            };
            let response = crate::script_fetch::fetch(
                url.as_str(),
                context,
                headers,
                &client_hints,
                &mut local,
                crate::script_fetch::ScriptPolicies {
                    http_cache: Some(&http_cache),
                    response_side_effects: Some(storage_effects.as_ref()),
                    cache_partition: &cache_partition,
                    request: Some(&allowed),
                    ..Default::default()
                },
                &credentials.integrity,
            )
            .await;
            jar.borrow_mut().merge_request_changes(local);
            let r = response
                .map_err(|error| JsErrorBox::type_error(format!("module fetch {url}: {error}")))?
                .response;
            module_credentials
                .0
                .borrow_mut()
                .entry(r.final_url.clone())
                .or_insert(credentials);
            if !(200..300).contains(&r.status) {
                return Err(JsErrorBox::type_error(format!(
                    "module fetch {url}: HTTP {}",
                    r.status
                )));
            }
            let mime = r
                .content_type
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if !matches!(
                mime.as_str(),
                "application/javascript"
                    | "text/javascript"
                    | "application/ecmascript"
                    | "text/ecmascript"
                    | "application/x-javascript"
                    | "application/x-ecmascript"
                    | "text/jscript"
                    | "text/livescript"
                    | "text/javascript1.0"
                    | "text/javascript1.1"
                    | "text/javascript1.2"
                    | "text/javascript1.3"
                    | "text/javascript1.4"
                    | "text/javascript1.5"
                    | "text/x-javascript"
                    | "text/x-ecmascript"
            ) {
                return Err(JsErrorBox::type_error(format!(
                    "module fetch {url}: unsupported JavaScript MIME type {mime}"
                )));
            }
            let final_url = ModuleSpecifier::parse(&r.final_url)
                .map_err(|error| JsErrorBox::generic(error.to_string()))?;
            Ok(ModuleSource::new_with_redirect(
                ModuleType::JavaScript,
                ModuleSourceCode::String(r.html.into()),
                &url,
                &final_url,
                None,
            ))
        }))
    }
}

/// Page base URL (the `location.href`): the base for relative `fetch` and the
/// scope for the `document.cookie` bridge. Stored in op state.
struct Base(String);
#[derive(Default)]
struct ResponseHeaders(Vec<(String, String)>);

/// Shared cookie jar backing `document.cookie` (and page `fetch`). Stored in op
/// state behind `Rc<RefCell<…>>` since ops borrow it across the isolate.
type Jar = Rc<RefCell<CookieJar>>;

/// Custom User-Agent for this page: drives `navigator.userAgent` and the page-fetch
/// `User-Agent` header. Empty = the engine default. Stored in op state.
struct Ua(String);

/// `fetch` result marshaled back to JS as a `Response`-like object.
#[derive(Default, serde::Serialize)]
struct FetchOut {
    status_text: String,
    stream_id: u32,
    aborted: bool,
    nosniff: bool,
    integrity_blocked: bool,
    script_errors_muted: bool,
    response_received: bool,
    policy_blocked: bool,
    cross_origin: bool,
    body_bytes: Vec<u8>,
    headers: Vec<(String, String)>,
    redirected: bool,
    status: u16,
    ok: bool,
    body: String,
    content_type: String,
    frame_document: u32,
    frame_script: u32,
    final_url: String,
}

#[derive(Default)]
struct NetworkLog(std::collections::VecDeque<serde_json::Value>);

// Classic scripts are scheduled onto the host pump. Executing inside a JS eval
// or a synchronous op retains the dispatcher's frames in page Error stacks.
struct ClassicScriptJob {
    resolver: v8::Global<v8::PromiseResolver>,
    source: String,
    code: Vec<u16>,
    context: v8::Global<v8::Context>,
    waiter_context: v8::Global<v8::Context>,
    document: v8::Global<v8::Object>,
    muted_errors: bool,
    element: Option<v8::Global<v8::Object>>,
}

#[derive(Default)]
struct ClassicScripts {
    html: String,
    used_inline_offsets: std::collections::BTreeSet<usize>,
    jobs: std::collections::VecDeque<ClassicScriptJob>,
}

#[op2(reentrant)]
fn op_queue_classic_script<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    state: &mut OpState,
    window: v8::Local<'s, v8::Object>,
    code: v8::Local<'s, v8::String>,
    #[string] source: String,
    inline: bool,
    muted_errors: bool,
    element: v8::Local<'s, v8::Object>,
    in_shadow_tree: bool,
) -> Result<v8::Local<'s, v8::Promise>, JsErrorBox> {
    let code_units = crate::browser_env::string_code_units(scope, code);
    let code = String::from_utf16_lossy(&code_units);
    let context = window
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::generic("script context unavailable"))?;
    if context.get_slot::<FrameRetired>().is_some() {
        return Err(JsErrorBox::generic("iframe document has been replaced"));
    }
    if context
        .get_slot::<FrameSandbox>()
        .is_some_and(|flags| flags.scripts_blocked)
    {
        return Err(JsErrorBox::generic(
            "script execution is blocked by the iframe sandbox",
        ));
    }
    let caller = scope.get_entered_or_microtask_context();
    if !caller
        .get_security_token(scope)
        .strict_equals(context.get_security_token(scope))
    {
        return Err(JsErrorBox::generic("script context is not same-origin"));
    }
    let resolver = v8::PromiseResolver::new(scope)
        .ok_or_else(|| JsErrorBox::generic("script completion allocation failed"))?;
    let promise = resolver.get_promise(scope);
    let element = (!in_shadow_tree).then(|| v8::Global::new(scope, element));
    let context = v8::Global::new(scope, context);
    let caller = v8::Global::new(scope, caller);
    let queue = state.borrow_mut::<ClassicScripts>();
    let located = if inline && !code.is_empty() {
        if let Some((start, _)) = queue
            .html
            .match_indices(&code)
            .find(|(offset, _)| !queue.used_inline_offsets.contains(offset))
        {
            queue.used_inline_offsets.insert(start);
            let prefix = &queue.html[..start];
            let lines = prefix.bytes().filter(|b| *b == b'\n').count();
            let column = prefix
                .rsplit('\n')
                .next()
                .unwrap_or("")
                .encode_utf16()
                .count();
            let mut located = vec![10; lines];
            located.extend(std::iter::repeat_n(32, column));
            located.extend(code_units);
            located
        } else {
            code_units
        }
    } else {
        code_units
    };
    queue.jobs.push_back(ClassicScriptJob {
        resolver: v8::Global::new(scope, resolver),
        source,
        code: located,
        context,
        waiter_context: caller,
        document: v8::Global::new(scope, window),
        muted_errors,
        element,
    });
    Ok(promise)
}

pub(crate) fn cookie_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

// `document.cookie` getter: cookies applicable to the page's base URL.
#[op2]
#[string]
fn op_cookie_get(state: &mut OpState) -> String {
    apply_report_cookies(state);
    let base = state.borrow::<Base>().0.clone();
    state
        .borrow::<Jar>()
        .borrow()
        .document_cookie(&base, cookie_now())
}

// The custom User-Agent (empty if none) — `navigator.userAgent` reads this.
#[op2]
#[string]
fn op_user_agent(state: &mut OpState) -> String {
    state.borrow::<Ua>().0.clone()
}

// Process-global fingerprint overrides (a JSON object). Read by ENV_BOOTSTRAP to
// override the default Chrome navigator fields at runtime. Empty `{}` = all
// defaults. Process-global (like the napi shared client) — one render process is
// effectively one session; set it before rendering.
static FINGERPRINT_OVERRIDES: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

/// Override the render-tier navigator fields at runtime with a JSON object, e.g.
/// `{"platform":"Win32","hardwareConcurrency":16,"languages":["en-GB","en"],
/// "screen":{"width":2560,"height":1440},"userAgent":"…"}`. Unset keys keep their
/// current configured Chrome release defaults. Pass `"{}"` (or `""`) to reset to defaults.
pub fn set_fingerprint(overrides_json: &str) {
    if let Ok(mut g) = FINGERPRINT_OVERRIDES.write() {
        *g = overrides_json.to_string();
    }
}

// The fingerprint-override JSON (or "{}" when unset) — ENV_BOOTSTRAP merges it.
#[op2]
#[string]
fn op_fingerprint() -> String {
    let profile = turbo_surf_core::fingerprint::default_profile();
    let mut defaults = serde_json::json!({
        "fullVersion": profile.full_version,
        "defaultMajor": profile.chrome_major,
        "defaultUserAgent": profile.user_agent,
        "architecture": profile.architecture(),
        "platformVersion": profile.platform_version(),
    });
    if let Some(serde_json::Value::Object(overrides)) = FINGERPRINT_OVERRIDES
        .read()
        .ok()
        .and_then(|g| serde_json::from_str::<serde_json::Value>(&g).ok())
    {
        defaults.as_object_mut().unwrap().extend(overrides);
    }
    defaults.to_string()
}

#[op2]
#[string]
fn op_browser_brands(major: u32, #[string] version: &str, full: bool) -> String {
    let major = u16::try_from(major).unwrap_or(turbo_surf_core::browser::DEFAULT_MAJOR);
    serde_json::to_string(&turbo_surf_core::fingerprint::brand_versions(
        major, version, full,
    ))
    .unwrap_or_else(|_| "[]".into())
}

#[op2(fast)]
fn op_browser_device_memory(major: u32) -> u32 {
    let major = u16::try_from(major).unwrap_or(turbo_surf_core::browser::DEFAULT_MAJOR);
    u32::from(turbo_surf_core::browser::release(major).default_device_memory)
}

// Process-global text-measurement hook. The render crate has no font/layout stack, so the host
// (napi/mcp, which own turbo-surf-raster) injects a real advance-width measurer. ENV_BOOTSTRAP's
// offsetWidth/offsetHeight read it so a font-detection probe sees per-family metric differences a
// no-layout DOM otherwise can't produce. Same host-injection pattern as `set_fingerprint`.
type MeasureFn = Box<dyn Fn(&str, &str, f64) -> (f64, f64) + Send + Sync>;
static MEASURE_TEXT: std::sync::RwLock<Option<MeasureFn>> = std::sync::RwLock::new(None);

/// Install the host's text measurer: `(text, css_font_family, font_size_px) -> (width_px, height_px)`.
/// Injected once at startup by the crate that owns the font/layout engine (raster). Until set,
/// `offsetWidth`/`offsetHeight` report 0 (a standalone render isolate has no fonts).
pub fn set_measure_fn(f: MeasureFn) {
    if let Ok(mut g) = MEASURE_TEXT.write() {
        *g = Some(f);
    }
}

// Measure text advance for offsetWidth/offsetHeight. Returns JSON `[width,height]`, or `null` when
// no host measurer is installed. Never throws across the boundary.
#[op2]
#[string]
fn op_measure_text(#[string] text: &str, #[string] family: &str, size: f64) -> String {
    match MEASURE_TEXT
        .read()
        .ok()
        .and_then(|g| g.as_ref().map(|f| f(text, family, size)))
    {
        Some((w, h)) => format!("[{w},{h}]"),
        None => "null".to_string(),
    }
}

// Real monotonic high-resolution clock (sub-millisecond), for `performance.now()`. deno_core
// gives no wall-independent monotonic time to the isolate, so the JS clock fell back to
// `Date.now()` (1ms grid) + a synthetic creep — which flatlines under a tight read loop (a timing
// tell: a real browser's performance.now advances on a ~sub-µs hardware clock). This returns ms
// since a process-fixed `Instant`, so consecutive reads show real sub-ms deltas like Chrome.
#[op2(fast)]
fn op_now_perf() -> f64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

// The performance origin needs the actual fractional wall epoch, independently
// of Date's integer-millisecond representation and the virtual timer clock.
#[op2(fast)]
fn op_now_epoch() -> Result<f64, JsErrorBox> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64() * 1000.0)
        .map_err(|error| JsErrorBox::generic(error.to_string()))
}

// Host canvas rasterizer: replay a 2D draw list into real PNG bytes. Same injection
// pattern as `set_measure_fn` — the crate that owns the raster/paint engine installs it,
// so the render crate keeps no tiny-skia/font dep (matters for the PyO3 wheel build). Until
// set, `toDataURL` keeps the vendored synthetic behavior.
type RasterFn = Box<dyn Fn(u32, u32, &str) -> Option<Vec<u8>> + Send + Sync>;
static RASTER_PNG: std::sync::RwLock<Option<RasterFn>> = std::sync::RwLock::new(None);

/// Install the host's canvas rasterizer: `(width, height, ops_json) -> PNG bytes`.
/// `ops_json` is the recorded 2D display list (the vendored `ctx._ops`). Injected once at
/// startup by the crate that owns the raster engine (raster). Until set, `toDataURL` returns
/// the vendored synthetic blob.
pub fn set_raster_fn(f: RasterFn) {
    if let Ok(mut g) = RASTER_PNG.write() {
        *g = Some(f);
    }
}

// Rasterize a canvas draw list to a base64 PNG (no `data:` prefix), or "" when no host
// rasterizer is installed (the JS override then falls back to the vendored blob). Never
// throws across the boundary.
#[op2]
#[string]
fn op_raster_png(width: u32, height: u32, #[string] ops_json: &str) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    match RASTER_PNG
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(|f| f(width, height, ops_json)))
    {
        Some(bytes) => STANDARD.encode(bytes),
        None => String::new(),
    }
}

// Same rasterizer, RAW straight-alpha RGBA output (not PNG) — for getImageData, which returns
// pixels with no encoder in the loop. So `fillRect(red); getImageData(...)` reads back the actual
// rendered red (the vendored synthetic getImageData returned unrelated bytes — a broken-canvas
// tell). Raw pixels also mean solids/shapes match a real browser exactly (no PNG-encoder variance).
static RASTER_RGBA: std::sync::RwLock<Option<RasterFn>> = std::sync::RwLock::new(None);

/// Install the host's raw-RGBA canvas rasterizer: `(width, height, ops_json) -> RGBA8 bytes`
/// (straight alpha, top-left origin, tightly packed). Until set, getImageData keeps the vendored
/// behavior.
pub fn set_raster_rgba_fn(f: RasterFn) {
    if let Ok(mut g) = RASTER_RGBA.write() {
        *g = Some(f);
    }
}

#[op2]
#[string]
fn op_raster_rgba(width: u32, height: u32, #[string] ops_json: &str) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    match RASTER_RGBA
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(|f| f(width, height, ops_json)))
    {
        Some(bytes) => STANDARD.encode(bytes),
        None => String::new(),
    }
}

// Host WebGL executor: run a recorded WebGL call batch on the real GPU and return the
// framebuffer RGBA. Same injection pattern as the rasterizer; installed by the crate that owns
// the wgpu backend (raster, `gpu-metal`). Until set, WebGL readback keeps the synthetic stub.
type WebglFn = Box<dyn Fn(u32, u32, &str) -> Option<Vec<u8>> + Send + Sync>;
static WEBGL_EXEC: std::sync::RwLock<Option<WebglFn>> = std::sync::RwLock::new(None);

/// Install the host's WebGL→GPU executor: `(width, height, calls_json) -> RGBA8 bytes`.
/// `calls_json` is the batch the render tier records live off a page's `gl.*` calls (with real
/// buffer bytes + shader source). Until set, the isolate's WebGL `readPixels` stays synthetic.
pub fn set_webgl_fn(f: WebglFn) {
    if let Ok(mut g) = WEBGL_EXEC.write() {
        *g = Some(f);
    }
}

type WebglMetadataFn = Box<dyn Fn() -> Option<String> + Send + Sync>;
static WEBGL_METADATA: std::sync::RwLock<Option<WebglMetadataFn>> = std::sync::RwLock::new(None);

/// Install metadata from the same adapter that executes WebGL batches.
/// The callback is lazy, so a process without a render request need not open a GPU.
pub fn set_webgl_metadata_fn(f: WebglMetadataFn) {
    if let Ok(mut metadata) = WEBGL_METADATA.write() {
        *metadata = Some(f);
    }
}

#[op2]
#[string]
fn op_webgl_metadata() -> String {
    WEBGL_METADATA
        .read()
        .ok()
        .and_then(|m| m.as_ref().and_then(|f| f()))
        .unwrap_or_else(|| "{}".to_string())
}

// Execute a recorded WebGL batch → base64 RGBA8 (full framebuffer, top-left origin), or "" when
// no host executor is installed (the JS override then keeps its synthetic readback). The JS side
// slices the caller's requested rect out of the returned framebuffer. Never throws.
#[op2]
#[string]
fn op_webgl_readback(width: u32, height: u32, #[string] calls_json: &str) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    match WEBGL_EXEC
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(|f| f(width, height, calls_json)))
    {
        Some(bytes) => STANDARD.encode(bytes),
        None => String::new(),
    }
}

// Whether a host WebGL executor is installed. The op itself is always registered, so the JS
// recorder can't infer presence from `typeof op`; it checks this so it only overrides the WebGL
// context when a GPU backend exists (else the vendored synthetic context is left untouched).
#[op2(fast)]
fn op_webgl_available() -> bool {
    WEBGL_EXEC.read().map(|g| g.is_some()).unwrap_or(false)
}

// URL parsing and component mutation use the native WHATWG URL implementation.
#[op2]
#[serde]
fn op_surf_url_parse(
    #[string] input: &str,
    #[string] base: &str,
) -> Result<serde_json::Value, JsErrorBox> {
    let url = if base.is_empty() {
        ModuleSpecifier::parse(input)
    } else {
        ModuleSpecifier::parse(base).and_then(|base| base.join(input))
    }
    .map_err(|_| JsErrorBox::type_error("Invalid URL"))?;
    Ok(serde_json::json!({
        "href":url.as_str(), "origin":url.origin().ascii_serialization(),
        "protocol":format!("{}:",url.scheme()), "username":url.username(),
        "password":url.password().unwrap_or(""), "hostname":url.host_str().unwrap_or(""),
        "port":url.port().map(|p|p.to_string()).unwrap_or_default(),
        "pathname":url.path(), "search":url::quirks::search(&url),
        "hash":url::quirks::hash(&url),
        "host":url[url::Position::BeforeHost..url::Position::AfterPort].to_string(),
    }))
}
#[op2]
#[string]
fn op_url_update(
    #[string] input: &str,
    #[string] key: &str,
    #[string] value: &str,
) -> Result<String, JsErrorBox> {
    let mut url =
        ModuleSpecifier::parse(input).map_err(|_| JsErrorBox::type_error("Invalid URL"))?;
    // Web-IDL setters silently ignore invalid component values.
    match key {
        "protocol" => {
            let _ = url::quirks::set_protocol(&mut url, value);
        }
        "username" => {
            let _ = url::quirks::set_username(&mut url, value);
        }
        "password" => {
            let _ = url::quirks::set_password(&mut url, value);
        }
        "hostname" => {
            let _ = url::quirks::set_hostname(&mut url, value);
        }
        "host" => {
            let _ = url::quirks::set_host(&mut url, value);
        }
        "port" => {
            let _ = url::quirks::set_port(&mut url, value);
        }
        "pathname" => url::quirks::set_pathname(&mut url, value),
        "search" => url::quirks::set_search(&mut url, value),
        "hash" => url::quirks::set_hash(&mut url, value),
        _ => {}
    }
    Ok(url.into())
}

#[op2]
#[serde]
fn op_url_params_parse(#[string] input: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(input.as_bytes())
        .into_owned()
        .collect()
}
#[op2]
#[string]
fn op_url_params_serialize(#[string] input: &str) -> Result<String, JsErrorBox> {
    let pairs: Vec<(String, String)> = serde_json::from_str(input)
        .map_err(|_| JsErrorBox::type_error("Invalid search parameters"))?;
    Ok(url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish())
}

#[op2]
#[serde]
fn op_heap_statistics(scope: &mut v8::PinScope) -> serde_json::Value {
    let heap = scope.get_heap_statistics();
    serde_json::json!({"jsHeapSizeLimit": heap.heap_size_limit(), "totalJSHeapSize":heap.total_heap_size(), "usedJSHeapSize":heap.used_heap_size()})
}

struct TextDecoderSlot {
    encoding: &'static encoding_rs::Encoding,
    decoder: Option<encoding_rs::Decoder>,
    fatal: bool,
    ignore_bom: bool,
}
#[derive(Default)]
struct TextDecoders {
    next: u32,
    slots: std::collections::HashMap<u32, TextDecoderSlot>,
}
#[op2]
#[serde]
fn op_text_decoder_new(
    state: &mut OpState,
    #[string] label: &str,
    fatal: bool,
    ignore_bom: bool,
) -> Result<serde_json::Value, JsErrorBox> {
    let encoding = encoding_rs::Encoding::for_label_no_replacement(label.as_bytes())
        .ok_or_else(|| JsErrorBox::range_error("Unsupported encoding"))?;
    if !state.has::<TextDecoders>() {
        state.put(TextDecoders::default());
    }
    let collection = state.borrow_mut::<TextDecoders>();
    collection.next = collection
        .next
        .checked_add(1)
        .ok_or_else(|| JsErrorBox::range_error("Too many decoders"))?;
    let id = collection.next;
    collection.slots.insert(
        id,
        TextDecoderSlot {
            encoding,
            decoder: None,
            fatal,
            ignore_bom,
        },
    );
    Ok(serde_json::json!({"id":id,"encoding":encoding.name().to_ascii_lowercase()}))
}
#[op2]
#[string]
fn op_text_decode(
    state: &mut OpState,
    id: u32,
    #[buffer] input: &[u8],
    stream: bool,
) -> Result<String, JsErrorBox> {
    let slot = state
        .borrow_mut::<TextDecoders>()
        .slots
        .get_mut(&id)
        .ok_or_else(|| JsErrorBox::type_error("Invalid decoder"))?;
    let mut decoder = slot.decoder.take().unwrap_or_else(|| {
        if slot.ignore_bom {
            slot.encoding.new_decoder_without_bom_handling()
        } else {
            slot.encoding.new_decoder_with_bom_removal()
        }
    });
    let capacity = input
        .len()
        .checked_mul(4)
        .and_then(|size| size.checked_add(32))
        .ok_or_else(|| JsErrorBox::range_error("Input too large"))?;
    let mut output = String::with_capacity(capacity);
    if slot.fatal {
        let (result, read) =
            decoder.decode_to_string_without_replacement(input, &mut output, !stream);
        if matches!(result, encoding_rs::DecoderResult::Malformed(_, _)) {
            return Err(JsErrorBox::type_error("Encoded data is not valid"));
        }
        if read != input.len() || matches!(result, encoding_rs::DecoderResult::OutputFull) {
            return Err(JsErrorBox::range_error("Decoder output exceeded capacity"));
        }
    } else {
        let (result, read, _) = decoder.decode_to_string(input, &mut output, !stream);
        if read != input.len() || matches!(result, encoding_rs::CoderResult::OutputFull) {
            return Err(JsErrorBox::range_error("Decoder output exceeded capacity"));
        }
    }
    if stream {
        slot.decoder = Some(decoder);
    }
    Ok(output)
}
#[op2(fast)]
fn op_text_decoder_drop(state: &mut OpState, id: u32) {
    if state.has::<TextDecoders>() {
        state.borrow_mut::<TextDecoders>().slots.remove(&id);
    }
}
#[op2]
#[buffer]
fn op_text_encode(#[string] input: &str) -> Vec<u8> {
    input.as_bytes().to_vec()
}

#[op2(fast)]
fn op_random_fill(#[buffer] output: &mut [u8]) -> Result<(), JsErrorBox> {
    getrandom::fill(output).map_err(|error| JsErrorBox::generic(error.to_string()))
}

// `document.cookie` setter: ingest a `name=value; attrs` line against the base.
#[op2(fast)]
fn op_cookie_set(state: &mut OpState, #[string] line: &str) {
    apply_report_cookies(state);
    let base = state.borrow::<Base>().0.clone();
    state
        .borrow::<Jar>()
        .borrow_mut()
        .set_from_document(&base, line, cookie_now());
}

// `fetch(url)` over the tier-1 net stack. Relative URLs resolve against the
// page base. Transport/parse failures return a native failure record; the
// captured fetch wrapper rejects with TypeError before exposing a Response.
#[op2]
#[serde]
async fn op_fetch(
    state: Rc<RefCell<OpState>>,
    #[string] url: String,
    #[string] init_json: String,
) -> FetchOut {
    apply_report_cookies(&mut state.borrow_mut());
    let (base, jar_rc, ua, client_hints) = {
        let s = state.borrow();
        (
            s.borrow::<Base>().0.clone(),
            s.borrow::<Jar>().clone(),
            s.borrow::<Ua>().0.clone(),
            s.borrow::<ClientHints>().clone(),
        )
    };
    let target = resolve(&base, &url).unwrap_or(url);
    // Honor the `fetch(url, init)` request: method, headers, body. Without this every
    // page fetch was a GET with no body — a login POST (PropelAuth) 404'd.
    let init: deno_core::serde_json::Value =
        deno_core::serde_json::from_str(&init_json).unwrap_or(deno_core::serde_json::Value::Null);
    let cancel_id = init
        .get("cancel_id")
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    let cancel_job = fetch_streams::job(&state.borrow(), cancel_id);
    if cancel_id != 0 && cancel_job.is_none() {
        return FetchOut {
            aborted: true,
            final_url: target,
            ..Default::default()
        };
    }
    let method = init
        .get("method")
        .and_then(|m| m.as_str())
        .map(|m| m.to_ascii_uppercase());
    let body = init
        .get("body")
        .and_then(|b| b.as_str())
        .map(|b| b.to_string());
    let body_bytes = init
        .get("body_bytes")
        .and_then(|value| serde_json::from_value::<Vec<u8>>(value.clone()).ok());
    let mut headers: std::collections::BTreeMap<String, String> = init
        .get("headers")
        .and_then(|h| deno_core::serde_json::from_value(h.clone()).ok())
        .unwrap_or_default();
    // These headers are browser-controlled; guest fetch cannot override their
    // context, inject another origin's cookies, or supply a custom identity.
    headers.retain(|name, _| {
        let name = name.to_ascii_lowercase();
        !name.starts_with("sec-")
            && !matches!(
                name.as_str(),
                "cookie"
                    | "origin"
                    | "referer"
                    | "host"
                    | "user-agent"
                    | "upgrade-insecure-requests"
            )
    });
    let source = init.get("source").and_then(|s| s.as_str()).unwrap_or(&base);
    let destination = match init.get("destination").and_then(|s| s.as_str()) {
        Some("script") => Destination::Script,
        Some("iframe") => Destination::Iframe,
        Some("style") => Destination::Style,
        Some("image") => Destination::Image,
        _ => Destination::Empty,
    };
    let csp = init.get("csp").and_then(|id| id.as_u64()).and_then(|id| {
        state
            .borrow()
            .borrow::<CspContexts>()
            .0
            .get(&(id as u32))
            .cloned()
    });
    let partition = init
        .get("partition")
        .and_then(|value| value.as_u64())
        .and_then(|id| {
            state
                .borrow()
                .try_borrow::<CredentiallessPartitions>()
                .and_then(|partitions| partitions.0.get(&(id as u32)))
                .cloned()
        })
        .or_else(|| csp.as_ref().and_then(|handle| handle.partition.clone()));
    let jar_rc = partition
        .as_ref()
        .map_or(jar_rc, |partition| partition.jar.clone());
    // Upload conversion can suspend before this op. Native job preparation owns
    // the initiating policy snapshot without retaining its guest document or realm.
    let snapshot = cancel_job
        .as_ref()
        .map(|job| job.policy.clone())
        .unwrap_or_else(|| {
            csp.as_ref()
                .map_or_else(FetchPolicySnapshot::default, |handle| {
                    let policy = handle.policy.borrow();
                    FetchPolicySnapshot {
                        active: policy.active.clone(),
                        referrer: policy.referrer_policy,
                        embedder: policy.embedder_policy,
                    }
                })
        });
    let referrer_policy = init
        .get("referrerPolicy")
        .and_then(|value| value.as_str())
        .and_then(turbo_surf_core::referrer_policy::ReferrerPolicy::parse)
        .unwrap_or(snapshot.referrer);
    let embedder_policy = snapshot.embedder;
    let policy = snapshot.active;
    let opaque_cache_nonce = csp.as_ref().map_or(0, |handle| handle.id);
    let directive = match destination {
        Destination::Script => "script-src-elem",
        Destination::Style => "style-src-elem",
        Destination::Image => "img-src",
        Destination::Iframe => "frame-src",
        _ => "connect-src",
    };
    let nonce = init
        .get("nonce")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let integrity = init
        .get("integrity")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if !policy.allows_request(directive, &target, nonce, false)
        || !crate::resource_policy::active_mixed_content_allowed(source, &target)
    {
        return FetchOut {
            policy_blocked: true,
            final_url: target,
            ..Default::default()
        };
    }
    if target.starts_with("data:")
        && init.get("destination").and_then(|value| value.as_str()) == Some("iframe")
    {
        let decoded = decode_data_document(&target);
        let ok = decoded.is_some();
        let (body, content_type) = decoded.unwrap_or_default();
        let mut state = state.borrow_mut();
        let frames = state.borrow_mut::<FrameContexts>();
        frames.next_document += 1;
        let id = frames.next_document;
        frames.documents.insert(
            id,
            FrameDocument {
                response_metadata: crate::DocumentResponseMetadata {
                    status: 200,
                    referrer: crate::document_response::local_navigation_referrer(
                        source,
                        referrer_policy,
                    ),
                },
                partition: partition.clone(),
                url: target.clone(),
                html: body.clone(),
                headers: Vec::new(),
            },
        );
        return FetchOut {
            status: if ok { 200 } else { 0 },
            ok,
            body_bytes: body.as_bytes().to_vec(),
            body,
            content_type,
            frame_document: id,
            final_url: target,
            ..Default::default()
        };
    }
    let mode = match init.get("mode").and_then(|s| s.as_str()) {
        Some("no-cors") => RequestMode::NoCors,
        Some("cors") => RequestMode::Cors,
        Some("same-origin") => RequestMode::SameOrigin,
        Some("navigate") => RequestMode::Navigate,
        _ if destination == Destination::Iframe => RequestMode::Navigate,
        _ if destination == Destination::Script => RequestMode::NoCors,
        _ => RequestMode::Cors,
    };
    let mut context = RequestContext::resource(source, destination, mode);
    context.referrer = init
        .get("referrer_source")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    context.referrer_policy = referrer_policy;
    context.credentialless =
        embedder_policy == crate::resource_policy::EmbedderPolicy::Credentialless;
    if let Some(referrer) = init.get("referrer").and_then(|value| value.as_str()) {
        if referrer != "about:client" {
            context.referrer = Some(referrer.to_string());
        }
    }
    context.top_level_source = init
        .get("top_level_source")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    context.cookie_site_is_opaque = init
        .get("cookie_site_is_opaque")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    context.opaque_origin = init
        .get("opaque_origin")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if let Some(credentials) = init.get("credentials").and_then(|s| s.as_str()) {
        context.credentials = match credentials {
            "omit" => Credentials::Omit,
            "include" => Credentials::Include,
            _ => Credentials::SameOrigin,
        };
    }
    if mode == RequestMode::SameOrigin
        && (context.opaque_origin
            || !turbo_surf_core::request_context::same_origin(source, &target))
    {
        return FetchOut {
            status_text: String::new(),
            stream_id: 0,
            aborted: false,
            nosniff: false,
            integrity_blocked: false,
            script_errors_muted: false,
            response_received: false,
            policy_blocked: false,
            cross_origin: false,
            body_bytes: Vec::new(),
            headers: Vec::new(),
            redirected: false,
            status: 0,
            ok: false,
            body: String::new(),
            content_type: String::new(),
            frame_document: 0,
            frame_script: 0,
            final_url: target.clone(),
        };
    }
    let streaming_upload = init.get("upload").and_then(|value| value.as_bool()) == Some(true);
    // Only the native dictionary snapshot emits this field. The public fetch
    // wrapper serializes an explicit whitelist, so author Priority still preflights.
    let dictionary_prefetch = init
        .get("dictionary_prefetch")
        .and_then(|value| value.as_bool())
        == Some(true);
    let before_request = |context,
                          target,
                          method,
                          mut headers: std::collections::BTreeMap<String, String>|
     -> turbo_surf_core::net::BeforeRequestFuture {
        if dictionary_prefetch {
            headers.retain(|name, _| !name.eq_ignore_ascii_case("priority"));
        }
        Box::pin(crate::fetch_cors::authorize_streaming_request(
            context,
            target,
            method,
            headers,
            client_hints.clone(),
            streaming_upload,
        ))
    };
    // Custom User-Agent (if set) overrides the net default for page fetches.
    if !ua.is_empty() {
        headers.insert("user-agent".to_string(), ua);
    }
    // Carry the page's cookies on same-origin fetches and ingest Set-Cookie back, so
    // session-authenticated hydration works (e.g. an auth SDK fetching the current user
    // with the session cookie). Snapshot the shared jar into a local one for the call —
    // a RefCell borrow can't be held across the await.
    let mut local = jar_rc.borrow().fork_for_request();
    let policy_blocked = std::sync::atomic::AtomicBool::new(false);
    let response_received = std::sync::atomic::AtomicBool::new(false);
    let request_policy = |target: &str, redirected: bool| {
        let allowed = policy.allows_request(directive, target, nonce, redirected)
            && crate::resource_policy::active_mixed_content_allowed(source, target);
        policy_blocked.fetch_or(!allowed, std::sync::atomic::Ordering::Relaxed);
        allowed
    };
    let crossed_origin = std::sync::atomic::AtomicBool::new(
        context.opaque_origin || !turbo_surf_core::request_context::same_origin(source, &target),
    );
    let response_referrer = std::sync::Mutex::new(String::new());
    let response_location = std::sync::Mutex::new(target.clone());
    let response_policy = |context: &RequestContext, target: &str, headers: &[(String, String)]| {
        response_received.store(true, std::sync::atomic::Ordering::Relaxed);
        *response_location.lock().unwrap() = target.to_string();
        *response_referrer.lock().unwrap() = context
            .source
            .as_deref()
            .and_then(|source| {
                context
                    .referrer_policy
                    .referrer(context.referrer.as_deref().unwrap_or(source), target)
            })
            .unwrap_or_default();
        let cross = context.opaque_origin
            || context.origin_tainted
            || !context.source.as_deref().is_some_and(|source| {
                turbo_surf_core::request_context::same_origin(source, target)
            });
        crossed_origin.fetch_or(cross, std::sync::atomic::Ordering::Relaxed);
        crate::resource_policy::embedder_allowed(embedder_policy, context, target, headers)
            && (context.mode != RequestMode::Cors
                || !cross
                || crate::fetch_cors::allowed(context, headers))
    };
    let http_cache = {
        let mut state = state.borrow_mut();
        if !state.has::<BrowserStorage>() {
            state.put(BrowserStorage::default());
        }
        state.borrow::<BrowserStorage>().http_cache()
    };
    let cache_mode = init
        .get("cache")
        .and_then(|value| value.as_str())
        .and_then(turbo_surf_core::http_cache::CacheMode::parse)
        .unwrap_or_default();
    let cache_partition = http_cache_partition(
        &base,
        source,
        partition.as_ref().map_or(0, |partition| partition.id),
        if context.opaque_origin {
            opaque_cache_nonce
        } else {
            0
        },
    );
    let storage_partition = partition.as_ref().map_or(0, |partition| partition.id);
    let storage_effects = state
        .borrow()
        .borrow::<BrowserStorage>()
        .partition_response_effects(storage_partition);
    let mut opts = FetchOptions {
        response_side_effects: Some(storage_effects.as_ref()),
        http_cache: Some(&http_cache),
        cache_mode,
        cache_partition: cache_partition.clone(),
        redirect: Some(
            match init.get("redirect").and_then(|value| value.as_str()) {
                Some("manual") => turbo_surf_core::net::RedirectMode::Manual,
                Some("error") => turbo_surf_core::net::RedirectMode::Error,
                _ => turbo_surf_core::net::RedirectMode::Follow,
            },
        ),
        response_policy: Some(&response_policy),
        request_policy: Some(&request_policy),
        before_request: Some(&before_request),
        method: method.clone(),
        body,
        body_bytes,
        upload_body: cancel_job
            .as_ref()
            .and_then(|job| fetch_streams::take_upload(job)),
        headers,
        context: Some(context.clone()),
        client_hints: Some(&client_hints),
        allow_non_html: true, // fetch pulls JSON/text too
        // Follow redirects MANUALLY so `Set-Cookie` on an intermediate 3xx hop is
        // ingested (the browser-equivalent per-hop cookie round-trip). Auto-follow
        // only ingests the final response's cookies — a login/consent endpoint that
        // mints its session cookie on a 302 (Google's `/save` → `NID`) would be lost.
        max_redirects: Some(20),
        jar: None,
        ..Default::default()
    };
    if destination == Destination::Empty
        && integrity.is_empty()
        && init.get("stream").and_then(|value| value.as_bool()) == Some(true)
    {
        let response = {
            opts.jar = Some(&mut local);
            let request = turbo_surf_core::net::fetch_stream(&target, opts);
            if let Some(job) = &cancel_job {
                fetch_streams::until_cancelled(job, request).await
            } else {
                Some(request.await)
            }
        };
        let out = match response {
            None => FetchOut {
                aborted: true,
                final_url: target.clone(),
                ..Default::default()
            },
            Some(Err(error)) => FetchOut {
                response_received: response_received.load(std::sync::atomic::Ordering::Relaxed),
                policy_blocked: policy_blocked.load(std::sync::atomic::Ordering::Relaxed),
                cross_origin: crossed_origin.load(std::sync::atomic::Ordering::Relaxed),
                final_url: target.clone(),
                body: error.to_string(),
                ..Default::default()
            },
            Some(Ok(response)) => {
                let cross = crossed_origin.load(std::sync::atomic::Ordering::Relaxed)
                    || !turbo_surf_core::request_context::same_origin(source, &response.final_url);
                let out = FetchOut {
                    status_text: response.status_text,
                    stream_id: cancel_id,
                    response_received: true,
                    cross_origin: cross,
                    nosniff: response.headers.iter().any(|(name, value)| {
                        name.eq_ignore_ascii_case("x-content-type-options")
                            && value.trim().eq_ignore_ascii_case("nosniff")
                    }),
                    headers: crate::fetch_cors::exposed_headers(&context, response.headers, cross),
                    redirected: response.redirected,
                    status: response.status,
                    ok: (200..300).contains(&response.status),
                    content_type: response.content_type,
                    final_url: response.final_url,
                    ..Default::default()
                };
                if let Some(job) = &cancel_job {
                    if fetch_streams::install(job, response.body).await {
                        out
                    } else {
                        FetchOut {
                            aborted: true,
                            final_url: target.clone(),
                            ..Default::default()
                        }
                    }
                } else {
                    FetchOut {
                        final_url: target.clone(),
                        ..Default::default()
                    }
                }
            }
        };
        jar_rc.borrow_mut().merge_request_changes(local);
        if out.stream_id == 0 {
            fetch_streams::release(&mut state.borrow_mut(), cancel_id)
        }
        let entry = serde_json::json!({"url":target,"status":out.status,"ok":out.ok,"method":method.as_deref().unwrap_or("GET"),"contentType":out.content_type,"streamed":true,"streamId":cancel_id,"body":"","bodyIncomplete":true});
        let mut state = state.borrow_mut();
        let log = state.borrow_mut::<NetworkLog>();
        if log.0.len() == 1000 {
            log.0.pop_front();
        }
        log.0.push_back(entry);
        return out;
    }
    let mut script_errors_muted = false;
    let response = {
        let request = async {
            if destination == Destination::Script {
                crate::script_fetch::fetch(
                    &target,
                    context.clone(),
                    opts.headers.clone(),
                    &client_hints,
                    &mut local,
                    crate::script_fetch::ScriptPolicies {
                        http_cache: Some(&http_cache),
                        response_side_effects: Some(storage_effects.as_ref()),
                        cache_partition: &cache_partition,
                        request: Some(&|target, redirected| {
                            policy.allows_request(directive, target, nonce, redirected)
                        }),
                        embedder: embedder_policy,
                    },
                    integrity,
                )
                .await
                .map(|result| {
                    script_errors_muted = result.muted_errors;
                    result.response
                })
            } else {
                opts.jar = Some(&mut local);
                fetch_html(&target, opts)
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(|response| {
                        let cross = crossed_origin.load(std::sync::atomic::Ordering::Relaxed)
                            || !turbo_surf_core::request_context::same_origin(
                                source,
                                &response.final_url,
                            );
                        let opaque_redirect = init.get("redirect").and_then(|value| value.as_str())
                            == Some("manual")
                            && matches!(response.status, 301 | 302 | 303 | 307 | 308);
                        if crate::integrity::matches(
                            &response.body_bytes,
                            integrity,
                            !opaque_redirect && (mode != RequestMode::NoCors || !cross),
                        ) {
                            Ok(response)
                        } else {
                            Err("Response failed integrity validation".into())
                        }
                    })
            }
        };
        if let Some(job) = &cancel_job {
            fetch_streams::until_cancelled(job, request).await
        } else {
            Some(request.await)
        }
    };
    let Some(response) = response else {
        jar_rc.borrow_mut().merge_request_changes(local);
        fetch_streams::release(&mut state.borrow_mut(), cancel_id);
        return FetchOut {
            aborted: true,
            final_url: target,
            ..Default::default()
        };
    };
    let out = match response {
        Ok(r) => {
            let frame_document = if destination == Destination::Iframe {
                let mut state = state.borrow_mut();
                let frames = state.borrow_mut::<FrameContexts>();
                frames.next_document += 1;
                let id = frames.next_document;
                frames.documents.insert(
                    id,
                    FrameDocument {
                        response_metadata: crate::DocumentResponseMetadata {
                            status: r.status,
                            referrer: response_referrer.lock().unwrap().clone(),
                        },
                        partition: partition.clone(),
                        url: response_location.lock().unwrap().clone(),
                        html: r.html.clone(),
                        headers: r.headers.clone(),
                    },
                );
                id
            } else {
                0
            };
            let frame_script = if destination == Destination::Script
                && init.get("frame_script").and_then(|v| v.as_bool()) == Some(true)
            {
                let mut state = state.borrow_mut();
                let frames = state.borrow_mut::<FrameContexts>();
                frames.next_script += 1;
                let id = frames.next_script;
                frames.scripts.insert(
                    id,
                    FrameScriptResponse {
                        request_url: target.clone(),
                        url: r.final_url.clone(),
                        code: r.html.clone(),
                        muted_errors: script_errors_muted,
                    },
                );
                id
            } else {
                0
            };
            FetchOut {
                status_text: r.status_text,
                stream_id: 0,
                aborted: false,
                integrity_blocked: false,
                script_errors_muted,
                nosniff: r.headers.iter().any(|(name, value)| {
                    name.eq_ignore_ascii_case("x-content-type-options")
                        && value.trim().eq_ignore_ascii_case("nosniff")
                }),
                response_received: true,
                policy_blocked: false,
                cross_origin: crossed_origin.load(std::sync::atomic::Ordering::Relaxed)
                    || !turbo_surf_core::request_context::same_origin(source, &r.final_url),
                body_bytes: r.body_bytes,
                headers: crate::fetch_cors::exposed_headers(
                    &context,
                    r.headers,
                    crossed_origin.load(std::sync::atomic::Ordering::Relaxed)
                        || !turbo_surf_core::request_context::same_origin(source, &r.final_url),
                ),
                redirected: r.redirected,
                status: r.status,
                ok: (200..300).contains(&r.status),
                body: r.html,
                content_type: r.content_type,
                frame_document,
                frame_script,
                final_url: r.final_url,
            }
        }
        Err(error) => FetchOut {
            status_text: String::new(),
            stream_id: 0,
            aborted: false,
            script_errors_muted: false,
            nosniff: false,
            integrity_blocked: error == "Response failed integrity validation"
                || error == "Script response failed integrity validation",
            response_received: response_received.load(std::sync::atomic::Ordering::Relaxed),
            policy_blocked: policy_blocked.load(std::sync::atomic::Ordering::Relaxed),
            cross_origin: crossed_origin.load(std::sync::atomic::Ordering::Relaxed),
            body_bytes: Vec::new(),
            headers: Vec::new(),
            redirected: false,
            status: 0,
            ok: false,
            body: String::new(),
            content_type: String::new(),
            frame_document: 0,
            frame_script: 0,
            final_url: target.clone(),
        },
    };
    jar_rc.borrow_mut().merge_request_changes(local);
    fetch_streams::release(&mut state.borrow_mut(), cancel_id);
    let entry = serde_json::json!({"url":target,"status":out.status,"ok":out.ok,"method":method.as_deref().unwrap_or("GET"),"contentType":out.content_type,"body":out.body});
    let mut state = state.borrow_mut();
    let log = state.borrow_mut::<NetworkLog>();
    if log.0.len() == 1000 {
        log.0.pop_front();
    }
    log.0.push_back(entry);
    out
}

fn http_cache_partition(top_level: &str, source: &str, anonymous: u32, opaque: u32) -> String {
    format!(
        "{}\n{}\n{}\n{}",
        turbo_surf_core::request_context::schemeful_site(top_level).unwrap_or_default(),
        turbo_surf_core::request_context::schemeful_site(source)
            .or_else(|| turbo_surf_core::request_context::schemeful_site(top_level))
            .unwrap_or_default(),
        anonymous,
        opaque
    )
}

#[derive(Default, serde::Serialize)]
struct ImageOut {
    url: String,
    status: u16,
    width: u32,
    height: u32,
    ok: bool,
}

#[op2]
#[serde]
async fn op_load_image(
    state: Rc<RefCell<OpState>>,
    #[string] url: String,
    #[string] source: String,
    csp: u32,
    #[string] context_json: String,
) -> ImageOut {
    let (base, jar, ua, hints) = {
        let state = state.borrow();
        (
            state.borrow::<Base>().0.clone(),
            state.borrow::<Jar>().clone(),
            state.borrow::<Ua>().0.clone(),
            state.borrow::<ClientHints>().clone(),
        )
    };
    let jar = state
        .borrow()
        .borrow::<CspContexts>()
        .0
        .get(&csp)
        .and_then(|handle| handle.partition.as_ref())
        .map_or(jar.clone(), |partition| partition.jar.clone());
    let source = if source.is_empty() {
        base.clone()
    } else {
        source
    };
    let target = resolve(&source, &url).unwrap_or(url);
    let (policy, embedder_policy) = state
        .borrow()
        .borrow::<CspContexts>()
        .0
        .get(&csp)
        .map(|handle| {
            let policy = handle.policy.borrow();
            (policy.active.clone(), policy.embedder_policy)
        })
        .unwrap_or_default();
    if !policy.allows_request("img-src", &target, "", false) {
        return ImageOut {
            url: target,
            ..Default::default()
        };
    }
    let mut local = jar.borrow().fork_for_request();
    let mut headers = std::collections::BTreeMap::new();
    if !ua.is_empty() {
        headers.insert("user-agent".to_string(), ua);
    }
    let request_policy =
        |target: &str, redirected: bool| policy.allows_request("img-src", target, "", redirected);
    let metadata: serde_json::Value = serde_json::from_str(&context_json).unwrap_or_default();
    let mut context = RequestContext::resource(&source, Destination::Image, RequestMode::NoCors);
    context.credentialless =
        embedder_policy == crate::resource_policy::EmbedderPolicy::Credentialless;
    context.top_level_source = metadata
        .get("top_level_source")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    context.cookie_site_is_opaque = metadata
        .get("cookie_site_is_opaque")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    context.opaque_origin = metadata
        .get("opaque_origin")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let http_cache = state.borrow().borrow::<BrowserStorage>().http_cache();
    let anonymous = state
        .borrow()
        .borrow::<CspContexts>()
        .0
        .get(&csp)
        .and_then(|handle| handle.partition.as_ref())
        .map_or(0, |partition| partition.id);
    let cache_partition = http_cache_partition(
        &base,
        &source,
        anonymous,
        if context.opaque_origin { csp } else { 0 },
    );
    let storage_effects = state
        .borrow()
        .borrow::<BrowserStorage>()
        .partition_response_effects(anonymous);
    let opts = FetchOptions {
        response_side_effects: Some(storage_effects.as_ref()),
        http_cache: Some(&http_cache),
        cache_partition,
        request_policy: Some(&request_policy),
        response_policy: Some(&|context, target, headers| {
            crate::resource_policy::embedder_allowed(embedder_policy, context, target, headers)
        }),
        headers,
        context: Some(context),
        client_hints: Some(&hints),
        jar: Some(&mut local),
        ..Default::default()
    };
    let response = if let Some(data) = target.strip_prefix("data:") {
        use base64::Engine;
        data.split_once(",")
            .filter(|(kind, _)| kind.ends_with(";base64"))
            .and_then(|(_, data)| base64::engine::general_purpose::STANDARD.decode(data).ok())
            .map(|bytes| (target.clone(), bytes, 200))
    } else {
        fetch_bytes(&target, opts).await.ok()
    };
    jar.borrow_mut().merge_request_changes(local);
    let Some((url, bytes, status)) = response else {
        return ImageOut::default();
    };
    let image = (200..300)
        .contains(&status)
        .then(|| image::load_from_memory(&bytes).ok())
        .flatten();
    match image {
        Some(image) => ImageOut {
            url,
            status,
            width: image.width(),
            height: image.height(),
            ok: true,
        },
        None => ImageOut {
            url,
            status,
            ..Default::default()
        },
    }
}

// Each frame owns its native tree and JS browser APIs in its own V8 context.
// The host retains contexts only until page reset/runtime destruction.
#[derive(Default)]
struct FrameContextMarker;
struct FrameDocumentUrl(String);
struct FrameDocumentLocation(String);
struct FrameDocumentSourceUrl(String);
struct FrameDepth(u32);
struct FrameParent(v8::Weak<v8::Context>);
struct FrameHost(v8::Weak<v8::Object>);
#[cfg_attr(not(turbo_surf_native_proxy), allow(dead_code))]
struct FrameNavigator(v8::Weak<v8::Function>);
struct FrameConfigured;
#[derive(Clone, Copy, Default)]
struct FrameSandbox {
    opaque: bool,
    scripts_blocked: bool,
    origin_restricted: bool,
    storage_access_blocked: bool,
    top_navigation_without_activation_blocked: bool,
    forms_blocked: bool,
    /// The sandboxed pointer lock browsing context flag (no `allow-pointer-lock`).
    pointer_lock_blocked: bool,
    top_navigation_without_activation_permitted: bool,
    /// The sandboxed document.domain browsing context flag: every sandbox sets it,
    /// whatever its allow tokens.
    document_domain_blocked: bool,
}
impl FrameSandbox {
    fn apply_csp(&mut self, policy: &crate::csp::Policy) {
        if let Some((scripts, origin)) = policy.sandbox_restrictions() {
            self.scripts_blocked |= scripts;
            self.document_domain_blocked = true;
            self.opaque |= origin;
            self.origin_restricted |= origin;
            self.storage_access_blocked |= policy.blocks_storage_access();
            self.forms_blocked |= policy.blocks_form_submission();
            self.pointer_lock_blocked |= policy.blocks_pointer_lock();
            self.top_navigation_without_activation_blocked |=
                policy.blocks_top_navigation_without_activation();
            self.top_navigation_without_activation_permitted |=
                !policy.blocks_top_navigation_without_activation();
        }
    }
    fn from_host(
        scope: &mut v8::PinScope,
        parent: v8::Local<v8::Context>,
        host: v8::Local<v8::Object>,
    ) -> Self {
        let inherited = parent
            .get_slot::<FrameSandbox>()
            .map_or(Self::default(), |flags| *flags);
        let mut flags = Self {
            opaque: inherited.origin_restricted,
            origin_restricted: inherited.origin_restricted,
            scripts_blocked: inherited.scripts_blocked,
            storage_access_blocked: inherited.storage_access_blocked,
            forms_blocked: inherited.forms_blocked,
            pointer_lock_blocked: inherited.pointer_lock_blocked,
            top_navigation_without_activation_blocked: inherited
                .top_navigation_without_activation_blocked,
            top_navigation_without_activation_permitted: inherited
                .top_navigation_without_activation_permitted,
            document_domain_blocked: inherited.document_domain_blocked,
        };
        if let Some(attribute) = crate::browser_env::node_attribute(scope, host, "sandbox") {
            flags.document_domain_blocked = true;
            let tokens = attribute.to_ascii_lowercase();
            let tokens = tokens.split_ascii_whitespace().collect::<Vec<_>>();
            flags.opaque |= !tokens.contains(&"allow-same-origin");
            flags.origin_restricted |= !tokens.contains(&"allow-same-origin");
            flags.scripts_blocked |= !tokens.contains(&"allow-scripts");
            flags.forms_blocked |= !tokens.contains(&"allow-forms");
            flags.pointer_lock_blocked |= !tokens.contains(&"allow-pointer-lock");
            flags.top_navigation_without_activation_blocked |=
                !tokens.contains(&"allow-top-navigation");
            flags.top_navigation_without_activation_permitted |=
                tokens.contains(&"allow-top-navigation");
            flags.storage_access_blocked |=
                !tokens.contains(&"allow-storage-access-by-user-activation");
        }
        flags
    }
}
struct FrameEnvironment(v8::Weak<v8::Object>);
/// Whether this realm's agent cluster is origin-keyed (`window.originAgentCluster`).
struct AgentClusterKeyed(bool);
/// The document a navigation installed as this realm's browsing-context document;
/// inert documents created in the realm never carry its policy-derived state.
struct ActiveDocument(v8::Weak<v8::Object>);

/// What the binding's Document reads from the response that created it.
struct DocumentFacts {
    last_modified: Option<f64>,
    keyed: bool,
    domain_sandboxed: bool,
}
impl DocumentFacts {
    /// A navigation response: its own headers, and this browsing context group's
    /// historical agent cluster keying for the response origin.
    fn response(
        scope: &mut v8::PinScope,
        context: v8::Local<v8::Context>,
        url: &str,
        headers: &[(String, String)],
    ) -> Self {
        let opaque = window_is_opaque(scope, context);
        let keyed = JsRuntime::op_state_from(scope)
            .borrow_mut()
            .borrow_mut::<crate::document_facts::AgentClusters>()
            .keyed(url, opaque, headers, trustworthy_url(url));
        Self {
            last_modified: crate::document_facts::last_modified(headers),
            keyed,
            domain_sandboxed: sandboxed_document_domain(context),
        }
    }
    /// srcdoc and about:blank documents have no response; they join their
    /// creator's agent cluster (an opaque sandboxed one is always origin-keyed).
    fn inherited(
        scope: &mut v8::PinScope,
        context: v8::Local<v8::Context>,
        parent: v8::Local<v8::Context>,
    ) -> Self {
        let keyed = window_is_opaque(scope, context)
            || parent
                .get_slot::<AgentClusterKeyed>()
                .is_none_or(|keyed| keyed.0);
        Self {
            last_modified: None,
            keyed,
            domain_sandboxed: sandboxed_document_domain(context),
        }
    }
}
fn sandboxed_document_domain(context: v8::Local<v8::Context>) -> bool {
    context
        .get_slot::<FrameSandbox>()
        .is_some_and(|flags| flags.document_domain_blocked)
}
/// Record `facts` on the realm's freshly installed document, before guest code runs.
fn install_document_facts(scope: &mut v8::PinScope, facts: DocumentFacts) {
    let context = scope.get_current_context();
    let global = context.global(scope);
    let key = v8::String::new(scope, "document").unwrap();
    let Some(document) = global
        .get(scope, key.into())
        .and_then(|value| value.to_object(scope))
    else {
        return;
    };
    crate::browser_env::set_document_last_modified(scope, document, facts.last_modified);
    crate::browser_env::set_document_origin_keyed(scope, document, facts.keyed);
    crate::browser_env::set_document_domain_sandboxed(scope, document, facts.domain_sandboxed);
    context.set_slot(Rc::new(AgentClusterKeyed(facts.keyed)));
    context.set_slot(Rc::new(ActiveDocument(v8::Weak::new(scope, document))));
}
#[derive(Default)]
struct HostCapabilities(
    std::cell::RefCell<std::collections::HashMap<String, v8::Weak<v8::Function>>>,
);

#[op2(nofast, reentrant)]
fn op_set_host_capabilities(
    scope: &mut v8::PinScope,
    record: v8::Local<v8::Object>,
) -> Result<(), JsErrorBox> {
    let Some(context) = record.get_creation_context(scope) else {
        return Err(JsErrorBox::generic(
            "native host capability context unavailable",
        ));
    };
    let capabilities = context.get_slot::<HostCapabilities>().unwrap_or_else(|| {
        let capabilities = Rc::new(HostCapabilities::default());
        context.set_slot(capabilities.clone());
        capabilities
    });
    let mut functions = Vec::new();
    if let Some(names) = record.get_own_property_names(scope, Default::default()) {
        for index in 0..names.length() {
            let Some(key) = names.get_index(scope, index) else {
                continue;
            };
            let name = key.to_rust_string_lossy(scope);
            if let Some(function) = record
                .get(scope, key)
                .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
            {
                if name == "snapshotFormValue" {
                    // Imported host ops can run in the parent context. The reader
                    // must belong to its actual document, without rooting a child.
                    let scope = &mut v8::ContextScope::new(scope, context);
                    if !crate::browser_env::install_form_value_reader(scope, function) {
                        return Err(JsErrorBox::generic(
                            "native form value capability initialization interrupted",
                        ));
                    }
                }
                if let Some(function) = crate::context_ownership::function(scope, function) {
                    functions.push((name, function));
                }
            }
        }
    }
    let has_csp_queue = functions
        .iter()
        .any(|(name, _)| name == "queueCspViolation");
    capabilities.0.borrow_mut().extend(functions);
    if has_csp_queue {
        let scope = &mut v8::ContextScope::new(scope, context);
        flush_csp_bootstrap(scope, context);
    }
    Ok(())
}

/// Window timer scheduling for an owning session. Virtual time remains the crawler default.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WindowTimerMode {
    #[default]
    Virtual,
    /// Fire timers only after their monotonic deadline, including during hydration.
    RealTime,
}

/// Host-owned policy installed before any document or frame script executes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PageSessionOptions {
    pub window_timer_mode: WindowTimerMode,
    /// Cap this initial hydration without shortening later operation budgets.
    /// Consumed on open; automatic form destinations inherit only the timer mode.
    pub initial_hydration_budget_ms: Option<u64>,
}

#[derive(Default)]
struct WindowTimerNesting {
    level: std::cell::Cell<u32>,
}

fn window_timer_nesting(context: v8::Local<v8::Context>) -> Rc<WindowTimerNesting> {
    context.get_slot::<WindowTimerNesting>().unwrap_or_else(|| {
        let nesting = Rc::new(WindowTimerNesting::default());
        context.set_slot(nesting.clone());
        nesting
    })
}

#[op2(fast)]
fn op_timer_nesting(scope: &mut v8::PinScope, document: v8::Local<v8::Object>) -> u32 {
    document
        .get_creation_context(scope)
        .map_or(0, |context| window_timer_nesting(context).level.get())
}

#[op2(fast)]
fn op_timer_set_nesting(scope: &mut v8::PinScope, document: v8::Local<v8::Object>, level: u32) {
    if let Some(context) = document.get_creation_context(scope) {
        window_timer_nesting(context).level.set(level);
    }
}

// Keep a Window's timer task active through its complete microtask checkpoint.
// A native guard also clears it on watchdog errors without executing more JS or
// retaining the owning frame context after the checkpoint.
struct WindowTimerTask {
    nesting: Rc<WindowTimerNesting>,
    previous: u32,
}
impl WindowTimerTask {
    fn new(context: v8::Local<v8::Context>) -> Self {
        let nesting = window_timer_nesting(context);
        let previous = nesting.level.get();
        Self { nesting, previous }
    }
}
impl Drop for WindowTimerTask {
    fn drop(&mut self) {
        self.nesting.level.set(self.previous);
    }
}

#[derive(Default)]
struct WindowTimerTurn {
    fired: u32,
    deadline: Option<tokio::time::Instant>,
    _task: Option<WindowTimerTask>,
}

struct TimerClock {
    mode: WindowTimerMode,
    started: std::time::Instant,
    advance: std::cell::Cell<f64>,
    resource_cursor: std::cell::Cell<usize>,
}
impl Default for TimerClock {
    fn default() -> Self {
        Self {
            mode: WindowTimerMode::default(),
            started: std::time::Instant::now(),
            advance: std::cell::Cell::new(0.0),
            resource_cursor: std::cell::Cell::new(0),
        }
    }
}
#[op2(fast)]
fn op_window_timers_realtime(state: &mut OpState) -> bool {
    state.borrow::<TimerClock>().mode == WindowTimerMode::RealTime
}
#[op2(fast)]
fn op_timer_clock(state: &mut OpState) -> f64 {
    let clock = state.borrow::<TimerClock>();
    clock.started.elapsed().as_secs_f64() * 1000.0 + clock.advance.get()
}
#[op2(fast)]
fn op_timer_offset(state: &mut OpState) -> f64 {
    state.borrow::<TimerClock>().advance.get()
}
#[op2(fast)]
fn op_timer_advance(state: &mut OpState, due: f64) {
    let clock = state.borrow::<TimerClock>();
    if clock.mode == WindowTimerMode::Virtual && due.is_finite() {
        clock.advance.set(
            clock
                .advance
                .get()
                .max(due - clock.started.elapsed().as_secs_f64() * 1000.0),
        );
    }
}

fn window_timer_deadline(clock: &TimerClock, due: f64) -> Option<tokio::time::Instant> {
    if !due.is_finite() {
        return None;
    }
    let seconds = ((due - clock.advance.get()) / 1000.0).max(0.0);
    let delay = std::time::Duration::try_from_secs_f64(seconds).ok()?;
    clock
        .started
        .checked_add(delay)
        .map(tokio::time::Instant::from_std)
}

fn run_next_window_timer(rt: &mut JsRuntime, allow_advance: bool) -> WindowTimerTurn {
    let mut contexts = vec![rt.main_context()];
    contexts.extend(
        rt.op_state()
            .borrow()
            .borrow::<FrameContexts>()
            .contexts
            .iter()
            .cloned(),
    );
    deno_core::scope!(scope, rt);
    let mut next: Option<(f64, v8::Local<v8::Context>)> = None;
    for context in contexts {
        let context = v8::Local::new(scope, context);
        if context.get_slot::<FrameRetired>().is_some() {
            continue;
        }
        let active = &mut v8::ContextScope::new(scope, context);
        if let Some(value) = call_frame_helper(active, "timerNextDue", &[]) {
            if value.is_number() {
                if let Some(due) = value.number_value(active) {
                    if due.is_finite() && next.as_ref().is_none_or(|(previous, _)| due < *previous)
                    {
                        next = Some((due, context));
                    }
                }
            }
        }
    }
    let Some((due, context)) = next else {
        return WindowTimerTurn::default();
    };
    if !allow_advance {
        let state = JsRuntime::op_state_from(scope);
        let state = state.borrow();
        let clock = state.borrow::<TimerClock>();
        let now = clock.started.elapsed().as_secs_f64() * 1000.0 + clock.advance.get();
        if due > now {
            return WindowTimerTurn {
                deadline: window_timer_deadline(clock, due),
                ..Default::default()
            };
        }
    }
    let task = WindowTimerTask::new(context);
    let active = &mut v8::ContextScope::new(scope, context);
    let fired = call_frame_helper(active, "runNextTimer", &[])
        .and_then(|value| value.uint32_value(active))
        .unwrap_or(0);
    WindowTimerTurn {
        fired,
        deadline: None,
        _task: Some(task),
    }
}
// Each callback is a Window task: finish its complete microtask queue before
// selecting another timer, including timers owned by a different frame realm.
fn checkpoint_window_timer(rt: &mut JsRuntime) -> Result<(), String> {
    rt.v8_isolate().perform_microtask_checkpoint();
    if rt.v8_isolate().is_execution_terminating() {
        return Err("Timer task microtasks were interrupted".into());
    }
    Ok(())
}

fn run_window_timers(rt: &mut JsRuntime, max: u32) -> Result<u32, String> {
    checkpoint_window_timer(rt)?;
    let allow_advance =
        rt.op_state().borrow().borrow::<TimerClock>().mode == WindowTimerMode::Virtual;
    let mut total = 0;
    while total < max {
        let turn = run_next_window_timer(rt, allow_advance);
        if turn.fired == 0 {
            break;
        }
        total += turn.fired;
        checkpoint_window_timer(rt)?;
    }
    Ok(total)
}

fn run_next_background_resource_task(rt: &mut JsRuntime) -> u32 {
    let mut contexts = vec![rt.main_context()];
    contexts.extend(
        rt.op_state()
            .borrow()
            .borrow::<FrameContexts>()
            .contexts
            .iter()
            .cloned(),
    );
    let start = rt
        .op_state()
        .borrow()
        .borrow::<TimerClock>()
        .resource_cursor
        .get()
        % contexts.len();
    deno_core::scope!(scope, rt);
    for offset in 0..contexts.len() {
        let index = (start + offset) % contexts.len();
        let context = &contexts[index];
        let context = v8::Local::new(scope, context);
        if context.get_slot::<FrameRetired>().is_some() {
            continue;
        }
        let active = &mut v8::ContextScope::new(scope, context);
        let caught = std::pin::pin!(v8::TryCatch::new(active));
        let active = &mut caught.init();
        let fired = call_frame_helper(active, "runNextBackgroundResourceTask", &[])
            .and_then(|value| value.uint32_value(active))
            .unwrap_or(0);
        if fired > 0 || active.has_caught() {
            JsRuntime::op_state_from(active)
                .borrow()
                .borrow::<TimerClock>()
                .resource_cursor
                .set(index + 1);
        }
        if active.has_caught() {
            // The queue removes a task before invoking it; an exception must
            // not retain its callback or discard later networking tasks.
            eprintln!("[render] background resource task failed");
            return 1;
        }
        if fired > 0 {
            return 1;
        }
    }
    0
}

struct FrameRetired;
// Directly discarded frames lose their media-query factory; descendants remain
// retained inactive contexts. Both still stop all native work and have no layout.
struct FrameDirectRetirement;
struct FailFrameInitializationForTesting;
struct FailedFrameInitializationForTesting;

struct WindowMessageLimits {
    max_messages: usize,
    max_bytes: usize,
}
impl Default for WindowMessageLimits {
    fn default() -> Self {
        Self {
            max_messages: 4096,
            max_bytes: 64 * 1024 * 1024,
        }
    }
}
#[derive(Default)]
struct FrameContexts {
    init_script: String,
    contexts: Vec<v8::Global<v8::Context>>,
    core: Option<v8::Global<v8::Value>>,
    messages: std::collections::VecDeque<WindowMessage>,
    message_limits: WindowMessageLimits,
    next_document: u32,
    documents: std::collections::HashMap<u32, FrameDocument>,
    next_script: u32,
    scripts: std::collections::HashMap<u32, FrameScriptResponse>,
}
struct FrameDocument {
    response_metadata: crate::DocumentResponseMetadata,
    partition: Option<Rc<CredentiallessPartition>>,
    url: String,
    html: String,
    headers: Vec<(String, String)>,
}

fn decode_data_document(url: &str) -> Option<(String, String)> {
    use base64::Engine;
    let url = url.strip_prefix("data:")?.split('#').next()?;
    let (metadata, payload) = url.split_once(',')?;
    let mut bytes = Vec::with_capacity(payload.len());
    let input = payload.as_bytes();
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' && index + 2 < input.len() {
            let high = (input[index + 1] as char).to_digit(16);
            let low = (input[index + 2] as char).to_digit(16);
            if let (Some(high), Some(low)) = (high, low) {
                bytes.push((high * 16 + low) as u8);
                index += 3;
                continue;
            }
        }
        bytes.push(input[index]);
        index += 1;
    }
    let fields = metadata.split(';').collect::<Vec<_>>();
    if fields
        .last()
        .is_some_and(|field| field.eq_ignore_ascii_case("base64"))
    {
        bytes.retain(|byte| !byte.is_ascii_whitespace());
        let config = base64::engine::general_purpose::GeneralPurposeConfig::new()
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent);
        bytes = base64::engine::general_purpose::GeneralPurpose::new(
            &base64::alphabet::STANDARD,
            config,
        )
        .decode(bytes)
        .ok()?;
    }
    let mime = fields
        .first()
        .copied()
        .filter(|value| !value.is_empty())
        .unwrap_or("text/plain")
        .to_ascii_lowercase();
    let encoding = fields
        .iter()
        .find_map(|field| field.strip_prefix("charset="))
        .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    let (body, _, _) = encoding.decode(&bytes);
    let body = match mime.as_str() {
        "text/html" => body.into_owned(),
        "text/plain" => format!(
            "<html><head></head><body><pre>{}</pre></body></html>",
            body.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        ),
        _ => return None,
    };
    Some((body, mime))
}

struct FrameScriptResponse {
    muted_errors: bool,
    request_url: String,
    url: String,
    code: String,
}
#[derive(Clone, serde::Serialize)]
struct FrameScriptSpec {
    index: usize,
    src: Option<String>,
    code: String,
    #[serde(skip)]
    code_units: Vec<u16>,
    source: String,
    line: usize,
    column: usize,
    asynchronous: bool,
    deferred: bool,
    module: bool,
    crossorigin: Option<String>,
    integrity: String,
}
struct FrameScriptPolicy {
    inline: Vec<Vec<u16>>,
    external: std::collections::HashSet<String>,
    scripts: Vec<FrameScriptSpec>,
    base: String,
}

// Tree supplies executable nodes/attributes; this source-only scan retains the
// opening-tag offsets that rtdom does not expose. Quoted '>' and raw-text content
// cannot shift the location into an attribute or a preceding script.
fn script_source_bodies(html: &str) -> Vec<(usize, usize)> {
    let lower = html.to_ascii_lowercase();
    let bytes = html.as_bytes();
    let mut cursor = 0;
    let mut bodies = Vec::new();
    while let Some(relative) = lower[cursor..].find('<') {
        let start = cursor + relative;
        if lower[start..].starts_with("<!--") {
            cursor = lower[start + 4..]
                .find("-->")
                .map_or(html.len(), |end| start + 4 + end + 3);
            continue;
        }
        let mut name_end = start + 1;
        while name_end < bytes.len() && bytes[name_end].is_ascii_alphanumeric() {
            name_end += 1;
        }
        let name = &lower[start + 1..name_end];
        let mut end = name_end;
        let mut quote = None;
        while end < bytes.len() {
            match (quote, bytes[end]) {
                (Some(q), c) if q == c => quote = None,
                (None, b'\'' | b'"') => quote = Some(bytes[end]),
                (None, b'>') => break,
                _ => (),
            }
            end += 1;
        }
        cursor = (end + 1).min(html.len());
        if name == "plaintext" {
            break;
        }
        if !matches!(
            name,
            "script" | "style" | "textarea" | "title" | "xmp" | "iframe" | "noembed" | "noframes"
        ) {
            continue;
        }
        let closing = format!("</{name}");
        let mut close = cursor;
        loop {
            let Some(relative) = lower[close..].find(&closing) else {
                close = html.len();
                break;
            };
            close += relative;
            let delimiter = bytes.get(close + closing.len());
            if delimiter.is_none_or(|c| c.is_ascii_whitespace() || matches!(*c, b'>' | b'/')) {
                break;
            }
            close += closing.len();
        }
        if name == "script" {
            bodies.push((cursor, close));
        }
        cursor = close;
    }
    bodies
}

impl FrameScriptPolicy {
    fn from_html_units(units: &[u16], url: &str, base_policy: &crate::csp::Policy) -> Self {
        let html = String::from_utf16_lossy(units);
        let tree = turbo_dom_parser::rtdom::tree::Tree::parse_utf16(units);
        let base = tree
            .query_selector_all("base[href]")
            .iter()
            .copied()
            .find(|node| tree.namespace_id(*node) == 0 && tree.local_name(*node) == Some("base"))
            .and_then(|node| tree.get_attribute(node, "href"))
            .and_then(|href| resolve(url, href))
            .filter(|target| {
                url::Url::parse(target)
                    .is_ok_and(|url| !matches!(url.scheme(), "data" | "javascript"))
                    && base_policy.allows_request("base-uri", target, "", false)
            })
            .unwrap_or_else(|| url.to_string());
        let normalized = html.replace("\r\n", "\n").replace('\r', "\n");
        let bodies = script_source_bodies(&normalized);
        let mut next_body = 0;
        let mut inline = Vec::new();
        let mut external = std::collections::HashSet::new();
        let mut scripts = Vec::new();
        for (index, &node) in tree.query_selector_all("script").iter().enumerate() {
            let code_units = tree.text_content_utf16(node);
            let code = String::from_utf16_lossy(&code_units);
            let location = bodies
                .iter()
                .enumerate()
                .skip(next_body)
                .find(|(_, (start, end))| normalized[*start..*end] == code)
                .map(|(index, (start, _))| {
                    next_body = index + 1;
                    *start
                });
            let kind = tree.get_attribute(node, "type").unwrap_or_default();
            let kind = kind.trim().to_ascii_lowercase();
            let module = kind == "module";
            if (!module
                && !matches!(
                    kind.as_str(),
                    "" | "text/javascript"
                        | "application/javascript"
                        | "text/ecmascript"
                        | "application/ecmascript"
                        | "application/x-javascript"
                        | "text/jscript"
                        | "text/livescript"
                ))
                || tree.has_attribute(node, "nomodule")
            {
                continue;
            }
            let mut ancestor = turbo_dom_parser::rtdom::NodeRef::new(&tree, node).parent();
            let mut inert = false;
            while let Some(parent) = ancestor {
                if tree
                    .tag_name(parent.handle())
                    .is_some_and(|tag| tag.eq_ignore_ascii_case("template"))
                {
                    inert = true;
                    break;
                }
                ancestor = parent.parent();
            }
            if inert {
                continue;
            }
            let src = tree
                .get_attribute(node, "src")
                .and_then(|src| resolve(&base, src));
            if !module {
                if let Some(src) = &src {
                    external.insert(src.clone());
                } else {
                    inline.push(code_units.clone());
                }
            }
            let prefix = location.map_or("", |offset| &normalized[..offset]);
            scripts.push(FrameScriptSpec {
                index,
                src,
                code,
                code_units,
                source: url.to_string(),
                line: prefix.bytes().filter(|c| *c == b'\n').count(),
                column: prefix
                    .rsplit('\n')
                    .next()
                    .unwrap_or("")
                    .encode_utf16()
                    .count(),
                asynchronous: tree.has_attribute(node, "async"),
                deferred: tree.has_attribute(node, "defer"),
                module,
                crossorigin: tree.get_attribute(node, "crossorigin").map(str::to_string),
                integrity: tree
                    .get_attribute(node, "integrity")
                    .unwrap_or_default()
                    .to_string(),
            });
        }
        Self {
            inline,
            external,
            scripts,
            base,
        }
    }
}

#[op2]
fn op_frame_scripts<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window: v8::Local<'s, v8::Object>,
) -> Result<v8::Local<'s, v8::Array>, JsErrorBox> {
    let context = window
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::generic("iframe context unavailable"))?;
    let policy = context
        .get_slot::<FrameScriptPolicy>()
        .ok_or_else(|| JsErrorBox::generic("frame script policy unavailable"))?;
    let metadata = serde_json::to_string(&policy.scripts)
        .map_err(|error| JsErrorBox::generic(error.to_string()))?;
    let source = v8::String::new(scope, &metadata)
        .ok_or_else(|| JsErrorBox::generic("frame script metadata allocation failed"))?;
    let scripts = v8::json::parse(scope, source)
        .and_then(|value| v8::Local::<v8::Array>::try_from(value).ok())
        .ok_or_else(|| JsErrorBox::generic("frame script metadata unavailable"))?;
    let key = v8::String::new(scope, "code")
        .ok_or_else(|| JsErrorBox::generic("frame script source key allocation failed"))?;
    for (index, script) in policy.scripts.iter().enumerate() {
        let object = scripts
            .get_index(scope, index as u32)
            .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
            .ok_or_else(|| JsErrorBox::generic("frame script metadata record unavailable"))?;
        let code =
            v8::String::new_from_two_byte(scope, &script.code_units, v8::NewStringType::Normal)
                .ok_or_else(|| JsErrorBox::generic("frame script source allocation failed"))?;
        if object.create_data_property(scope, key.into(), code.into()) != Some(true) {
            return Err(JsErrorBox::generic("frame script source snapshot failed"));
        }
    }
    Ok(scripts)
}

struct WindowMessage {
    target: v8::Global<v8::Context>,
    source: v8::Global<v8::Context>,
    origin: String,
    target_origin: MessageTargetOrigin,
    data: crate::crypto_clone::CloneSnapshot,
    /// The sender's UserActivation snapshot bits, or -1 when not included.
    activation: i32,
}
enum MessageTargetOrigin {
    Any,
    Tuple(String),
    Opaque(v8::Global<v8::Value>),
    Unmatchable,
}
impl MessageTargetOrigin {
    fn matches(&self, scope: &mut v8::PinScope, target: v8::Local<v8::Context>) -> bool {
        match self {
            Self::Any => true,
            Self::Tuple(origin) => {
                !window_is_opaque(scope, target) && *origin == window_origin(scope, target)
            }
            Self::Opaque(token) => {
                let token = v8::Local::new(scope, token);
                token.strict_equals(target.get_security_token(scope))
            }
            Self::Unmatchable => false,
        }
    }
}

fn run_frame_init_script(scope: &mut v8::PinScope) -> Result<(), JsErrorBox> {
    let source = JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<FrameContexts>()
        .init_script
        .clone();
    if source.is_empty() {
        return Ok(());
    }
    let code = v8::String::new(scope, &source)
        .ok_or_else(|| JsErrorBox::generic("frame init source unavailable"))?;
    let name = v8::String::new(scope, "<frame-init>").unwrap();
    let origin = v8::ScriptOrigin::new(
        scope,
        name.into(),
        0,
        0,
        false,
        0,
        None,
        false,
        false,
        false,
        None,
    );
    v8::Script::compile(scope, code, Some(&origin))
        .and_then(|script| script.run(scope))
        .ok_or_else(|| JsErrorBox::generic("frame init script failed"))?;
    Ok(())
}

fn frame_namespace<'s>(scope: &mut v8::PinScope<'s, '_>) -> Option<v8::Local<'s, v8::Object>> {
    let context = scope.get_current_context();
    if let Some(environment) = context.get_slot::<FrameEnvironment>() {
        return Some(crate::context_ownership::local(scope, &environment.0));
    }
    let global = context.global(scope);
    let name = v8::String::new(scope, "ts").unwrap();
    let key = v8::Symbol::for_key(scope, name);
    global.get(scope, key.into())?.to_object(scope)
}
fn call_frame_helper<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: &str,
    args: &[v8::Local<'s, v8::Value>],
) -> Option<v8::Local<'s, v8::Value>> {
    let context = scope.get_current_context();
    let capability = context
        .get_slot::<HostCapabilities>()
        .and_then(|capabilities| capabilities.0.borrow().get(name).cloned());
    if let Some(function) = capability {
        let function = crate::context_ownership::local(scope, function);
        let receiver = v8::undefined(scope);
        return function.call(scope, receiver.into(), args);
    }
    if matches!(
        name,
        "frameParsingDone"
            | "frameDomReady"
            | "fireFrameLoad"
            | "deliverWindowMessage"
            | "postWindowMessage"
            | "documentLoading"
            | "markDocumentInteractive"
            | "dispatchDocumentInteractive"
            | "dispatchDocumentComplete"
            | "completeModuleGraph"
            | "fireFrameScript"
            | "frameScriptElement"
            | "frameScriptDocument"
            | "reportClassicScriptError"
            | "setCurrentScript"
            | "timerNextDue"
            | "runNextTimer"
            | "runLocalTimers"
            | "runNextBackgroundResourceTask"
            | "cancelDocumentTasks"
            | "retireResourceDocument"
            | "queueCspViolation"
            | "frameRecordFor"
            | "accessFrame"
            | "prepareFrameAdoption"
            | "syncFrameConnections"
            | "frameSourceChanged"
            | "recaptchaBframeHandshake"
    ) {
        return None;
    }
    let namespace = frame_namespace(scope)?;
    let key = v8::String::new(scope, name).unwrap();
    let function = namespace.get(scope, key.into())?;
    let function = v8::Local::<v8::Function>::try_from(function).ok()?;
    function.call(scope, namespace.into(), args)
}
#[op2(reentrant)]
fn op_frame_host_helper<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    #[string] name: &str,
    arguments: v8::Local<'s, v8::Array>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    if !matches!(
        name,
        "frameRecordFor"
            | "accessFrame"
            | "prepareFrameAdoption"
            | "syncFrameConnections"
            | "frameSourceChanged"
    ) || arguments.length() > 2
    {
        return Err(JsErrorBox::type_error("Unknown private frame operation"));
    }
    let caller = v8::Global::new(scope, scope.get_entered_or_microtask_context());
    let context = document
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::type_error("Frame owner document unavailable"))?;
    if context.get_slot::<FrameRetired>().is_some() {
        return Ok(v8::undefined(scope).into());
    }
    let caller = v8::Local::new(scope, caller);
    if !caller
        .get_security_token(scope)
        .strict_equals(context.get_security_token(scope))
    {
        return Err(JsErrorBox::new(
            "DOMExceptionSecurityError",
            "Frame owner is cross-origin",
        ));
    }
    let mut values = Vec::new();
    for index in 0..arguments.length() {
        values.push(
            arguments
                .get_index(scope, index)
                .ok_or_else(|| JsErrorBox::generic("Private frame arguments interrupted"))?,
        );
    }
    let scope = &mut v8::ContextScope::new(scope, context);
    call_frame_helper(scope, name, &values)
        .ok_or_else(|| JsErrorBox::generic("Private frame helper unavailable"))
}

fn set_origin_token(scope: &mut v8::PinScope, context: v8::Local<v8::Context>, url: &str) {
    if let Ok(url) = ModuleSpecifier::parse(url) {
        let origin = url.origin().ascii_serialization();
        if origin != "null" {
            let token = v8::String::new_from_utf8(
                scope,
                origin.as_bytes(),
                v8::NewStringType::Internalized,
            )
            .unwrap();
            context.set_security_token(token.into());
            return;
        }
    }
    context.use_default_security_token();
}
fn window_origin(scope: &mut v8::PinScope, context: v8::Local<v8::Context>) -> String {
    let token = context.get_security_token(scope);
    if token.is_string() {
        token.to_rust_string_lossy(scope)
    } else {
        "null".to_string()
    }
}

fn trustworthy_url(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    match url.scheme() {
        "https" | "wss" | "file" => true,
        "http" | "ws" => match url.host() {
            Some(url::Host::Domain(host)) => host == "localhost" || host.ends_with(".localhost"),
            Some(url::Host::Ipv4(address)) => address.is_loopback(),
            Some(url::Host::Ipv6(address)) => address.is_loopback(),
            None => false,
        },
        _ => false,
    }
}

fn secure_context(scope: &mut v8::PinScope) -> bool {
    let context = scope.get_current_context();
    secure_context_for(scope, context)
}

// Guest metadata cannot grant shared memory. Navigation/agent-cluster policies
// must install this private capability before isolated sharing is supported.
#[derive(Clone)]
struct SharedMemoryCapability {
    allowed: bool,
    group: u32,
    origin: String,
}
pub(crate) fn shared_memory_allowed(scope: &mut v8::PinScope) -> bool {
    scope
        .get_current_context()
        .get_slot::<SharedMemoryCapability>()
        .is_some_and(|capability| capability.allowed)
}
pub(crate) fn shared_memory_identity(scope: &mut v8::PinScope) -> Option<(u32, String)> {
    let capability = scope
        .get_current_context()
        .get_slot::<SharedMemoryCapability>()?;
    capability
        .allowed
        .then(|| (capability.group, capability.origin.clone()))
}
#[op2(fast)]
fn op_shared_memory_allowed(scope: &mut v8::PinScope) -> bool {
    scope
        .get_entered_or_microtask_context()
        .get_slot::<SharedMemoryCapability>()
        .is_some_and(|capability| capability.allowed)
}

struct SecureContext(bool);
fn secure_context_for<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) -> bool {
    if let Some(value) = context.get_slot::<SecureContext>() {
        return value.0;
    }
    let secure = compute_secure_context(scope, context);
    context.set_slot(Rc::new(SecureContext(secure)));
    secure
}
fn compute_secure_context<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    mut context: v8::Local<'s, v8::Context>,
) -> bool {
    loop {
        let parent = context.get_slot::<FrameParent>();
        let url = context
            .get_slot::<FrameDocumentUrl>()
            .map(|value| value.0.clone())
            .unwrap_or_else(|| {
                if parent.is_some() {
                    "about:blank".into()
                } else {
                    JsRuntime::op_state_from(scope)
                        .borrow()
                        .borrow::<Base>()
                        .0
                        .clone()
                }
            });
        let inherited = parent.is_some()
            && (url.starts_with("about:") || url.starts_with("data:") || url.starts_with("blob:"));
        if !inherited && !trustworthy_url(&url) {
            return false;
        }
        let Some(parent) = parent else {
            return true;
        };
        let Some(parent) = parent.0.to_local(scope) else {
            return false;
        };
        context = parent;
    }
}

#[op2(fast)]
fn op_is_secure_context<'s>(scope: &mut v8::PinScope<'s, '_>) -> bool {
    let context = scope.get_entered_or_microtask_context();
    let context = v8::Global::new(scope, context);
    let context = v8::Local::new(scope, context);
    secure_context_for(scope, context)
}

#[op2]
fn op_window_parent<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window: v8::Local<v8::Object>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let context = window
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::generic("Window context unavailable"))?;
    if context.get_slot::<FrameRetired>().is_some() {
        return Ok(v8::null(scope).into());
    }
    let parent = context
        .get_slot::<FrameParent>()
        .map(|parent| crate::context_ownership::local(scope, &parent.0))
        .unwrap_or(context);
    Ok(parent.global(scope).into())
}

#[op2]
fn op_window_top<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window: v8::Local<v8::Object>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let mut context = window
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::generic("Window context unavailable"))?;
    if context.get_slot::<FrameRetired>().is_some() {
        return Ok(v8::null(scope).into());
    }
    for _ in 0..64 {
        let Some(parent) = context.get_slot::<FrameParent>() else {
            break;
        };
        context = crate::context_ownership::local(scope, &parent.0);
    }
    Ok(context.global(scope).into())
}

#[op2(reentrant)]
fn op_frame_owner_element<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window: v8::Local<'s, v8::Object>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let context = window
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::type_error("Illegal invocation"))?;
    if !window.strict_equals(context.global(scope).into()) {
        return Err(JsErrorBox::type_error("Illegal invocation"));
    }
    if context.get_slot::<FrameRetired>().is_some() {
        return Ok(v8::null(scope).into());
    }
    let Some(parent) = context
        .get_slot::<FrameParent>()
        .and_then(|parent| parent.0.to_local(scope))
    else {
        return Ok(v8::null(scope).into());
    };
    if !parent
        .get_security_token(scope)
        .strict_equals(context.get_security_token(scope))
    {
        return Ok(v8::null(scope).into());
    }
    Ok(context
        .get_slot::<FrameHost>()
        .and_then(|host| host.0.to_local(scope))
        .map(|host| host.into())
        .unwrap_or_else(|| v8::null(scope).into()))
}

#[op2(fast)]
fn op_windows_same_origin(scope: &mut v8::PinScope, window: v8::Local<v8::Object>) -> bool {
    let caller = scope.get_entered_or_microtask_context();
    let Some(target) = window.get_creation_context(scope) else {
        return false;
    };
    let a = caller.get_security_token(scope);
    let b = target.get_security_token(scope);
    a.strict_equals(b)
}

#[op2(fast, reentrant)]
fn op_window_post_message<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window: v8::Local<'s, v8::Object>,
    message: v8::Local<'s, v8::Value>,
    #[string] target_origin: &str,
    transfer: Option<v8::Local<'s, v8::Value>>,
    include_activation: bool,
) -> Result<(), JsErrorBox> {
    queue_window_message(
        scope,
        window,
        message,
        target_origin,
        transfer,
        include_activation,
    )
}

/// The entered window's UserActivation snapshot bits for an outgoing message.
pub(crate) fn activation_snapshot_bits(context: v8::Local<v8::Context>) -> i32 {
    user_activation::snapshot_bits(context)
}

fn admit_window_snapshot(scope: &mut v8::PinScope, size: usize) -> Result<(), JsErrorBox> {
    let state = JsRuntime::op_state_from(scope);
    let state = state.borrow();
    let frames = state.borrow::<FrameContexts>();
    let bytes = frames.messages.iter().fold(0usize, |sum, message| {
        sum.saturating_add(message.data.size_bytes())
    });
    if frames.messages.len() >= frames.message_limits.max_messages
        || bytes.saturating_add(size) > frames.message_limits.max_bytes
    {
        return Err(JsErrorBox::range_error(
            "Native Window message queue budget exceeded",
        ));
    }
    Ok(())
}

fn queue_window_message<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window: v8::Local<'s, v8::Object>,
    message: v8::Local<'s, v8::Value>,
    target_origin: &str,
    transfer: Option<v8::Local<'s, v8::Value>>,
    include_activation: bool,
) -> Result<(), JsErrorBox> {
    let source = v8::Global::new(scope, scope.get_entered_or_microtask_context());
    let source = v8::Local::new(scope, source);
    let activation = if include_activation {
        user_activation::snapshot_bits(source)
    } else {
        -1
    };
    let target = window
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::generic("message target is not a Window"))?;
    let origin = window_origin(scope, source);
    let expected = match target_origin {
        "*" => MessageTargetOrigin::Any,
        "/" if window_is_opaque(scope, source) => {
            let token = source.get_security_token(scope);
            MessageTargetOrigin::Opaque(v8::Global::new(scope, token))
        }
        "/" => MessageTargetOrigin::Tuple(origin.clone()),
        value => {
            let parsed = url::Url::parse(value)
                .map_err(|_| JsErrorBox::new("DOMExceptionSyntaxError", "Invalid target origin"))?;
            match parsed.origin() {
                url::Origin::Tuple(..) => {
                    MessageTargetOrigin::Tuple(parsed.origin().ascii_serialization())
                }
                url::Origin::Opaque(..) => MessageTargetOrigin::Unmatchable,
            }
        }
    };
    let data = {
        let scope = &mut v8::ContextScope::new(scope, source);
        v8::tc_scope!(let scope, scope);
        let result = crate::crypto_clone::prepare_transfer(scope, message, transfer, None);
        if scope.has_caught() || scope.has_terminated() {
            scope.rethrow();
            return Ok(());
        }
        let prepared = result?;
        // Guest getters may enqueue recursively or retire/navigate the receiver.
        // Validate current capacity after them, before native ownership commits.
        if source != target
            && target.get_slot::<FrameRetired>().is_none()
            && expected.matches(scope, target)
        {
            admit_window_snapshot(scope, prepared.size_bytes())?;
        }
        prepared.commit(scope)?
    };
    // Structured serialization/transfer precedes the inactive-target check.
    // A payload getter can retire the destination while serialization runs.
    if target.get_slot::<FrameRetired>().is_some() || !expected.matches(scope, target) {
        return Ok(());
    }
    if source == target {
        let scope = &mut v8::ContextScope::new(scope, target);
        if !expected.matches(scope, target) {
            return Ok(());
        }
        let crate::crypto_clone::MessageClone {
            value: cloned,
            ports,
            failed,
        } = crate::crypto_clone::deserialize_message(scope, &data)?;
        let port_values = ports
            .into_iter()
            .map(|port| port.into())
            .collect::<Vec<_>>();
        let port_array = v8::Array::new_with_elements(scope, &port_values);
        let origin = v8::String::new(scope, &origin).unwrap();
        let source_window = source.global(scope);
        let failed = v8::Boolean::new(scope, failed);
        let activation = v8::Integer::new(scope, activation);
        // Queue the JS timer immediately for synchronous run_with_dom callers;
        // listener delivery remains asynchronous and sees an actual clone.
        call_frame_helper(
            scope,
            "deliverWindowMessage",
            &[
                cloned,
                source_window.into(),
                origin.into(),
                port_array.into(),
                failed.into(),
                activation.into(),
            ],
        )
        .ok_or_else(|| JsErrorBox::generic("message dispatch unavailable"))?;
        return Ok(());
    }
    let item = WindowMessage {
        target: v8::Global::new(scope, target),
        source: v8::Global::new(scope, source),
        origin,
        target_origin: expected,
        data,
        activation,
    };
    JsRuntime::op_state_from(scope)
        .borrow_mut()
        .borrow_mut::<FrameContexts>()
        .messages
        .push_back(item);
    Ok(())
}

#[op2(reentrant)]
fn op_frame_set_document<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window: v8::Local<'s, v8::Object>,
    parent_window: v8::Local<'s, v8::Object>,
    host: v8::Local<'s, v8::Object>,
    html: v8::Local<'s, v8::String>,
    #[string] metadata: &str,
    response_id: u32,
) -> Result<v8::Local<'s, v8::Object>, JsErrorBox> {
    let supplied_units = crate::browser_env::string_code_units(scope, html);
    let mut context = window
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::generic("iframe context unavailable"))?;
    if context.get_slot::<FrameRetired>().is_some() {
        return Err(JsErrorBox::generic("iframe document has been replaced"));
    }
    if context.get_slot::<FrameHost>().is_some_and(|registered| {
        registered
            .0
            .to_local(scope)
            .is_none_or(|registered| !registered.strict_equals(host.into()))
    }) {
        return Err(JsErrorBox::generic("iframe host cannot be replaced"));
    }
    let parent = context
        .get_slot::<FrameParent>()
        .ok_or_else(|| JsErrorBox::generic("target is not a child frame"))?;
    let parent = crate::context_ownership::local(scope, &parent.0);
    let caller = v8::Global::new(scope, scope.get_entered_or_microtask_context());
    let caller = v8::Local::new(scope, caller);
    let native_parent = parent.global(scope);
    if !parent_window.strict_equals(native_parent.into())
        || !caller
            .get_security_token(scope)
            .strict_equals(parent.get_security_token(scope))
    {
        return Err(JsErrorBox::generic(
            "frame document can only be replaced by its owning context",
        ));
    }
    // The recorded owner and V8 token authorize borrowed same-origin callbacks;
    // a guest __ownerWin facade cannot substitute for the owner's actual global.
    let parent_window = native_parent;
    let mut metadata: serde_json::Value =
        serde_json::from_str(metadata).map_err(|e| JsErrorBox::generic(e.to_string()))?;
    // Origin assignment is authorized by a native network result, never by the
    // guest's JSON or URL getters. srcdoc/blank documents inherit their owner.
    let document = if response_id != 0 {
        Some(
            JsRuntime::op_state_from(scope)
                .borrow_mut()
                .borrow_mut::<FrameContexts>()
                .documents
                .remove(&response_id)
                .ok_or_else(|| JsErrorBox::generic("frame response unavailable"))?,
        )
    } else {
        None
    };
    let mut replaced = None;
    if cfg!(turbo_surf_native_proxy)
        || context.get_slot::<FrameConfigured>().is_some()
        || document.is_some()
    {
        // References obtained from the initial empty document must retain its
        // realm/tree. A token change on the same context would turn those old
        // Function/document handles into access to the replacement origin.
        let previous = context;
        retire_frame_tree(scope, previous);
        let window = allocate_frame(scope, parent, Some(previous), false)?;
        context = window.get_creation_context(scope).unwrap();
        replaced = Some(previous);
    }
    let result = (|| {
        let mut sandbox = FrameSandbox::from_host(scope, parent, host);
        if let Some(document) = &document {
            let policy = crate::csp::Policy::new(
                &document.url,
                document
                    .headers
                    .iter()
                    .filter(|(name, _)| name.eq_ignore_ascii_case("content-security-policy"))
                    .map(|(_, value)| value.clone()),
            );
            sandbox.apply_csp(&policy);
        }
        let credentialless =
            crate::browser_env::node_attribute(scope, host, "credentialless").is_some();
        let embedding_blocked = document.as_ref().is_some_and(|document| {
            let mut ancestors = Vec::new();
            let mut context = parent;
            loop {
                if ancestors.len() > 12 || context.get_slot::<FrameRetired>().is_some() {
                    return true;
                }
                ancestors.push(window_origin(scope, context));
                let Some(link) = context.get_slot::<FrameParent>() else {
                    break;
                };
                let Some(owner) = link.0.to_local(scope) else {
                    return true;
                };
                context = owner;
            }
            if !crate::resource_policy::ancestors_allowed(
                &document.url,
                &document.headers,
                &ancestors,
            ) {
                return true;
            }
            let Some(parent_policy) = parent.get_slot::<CspHandle>() else {
                return false;
            };
            let policy = parent_policy.policy.borrow().embedder_policy;
            if policy == crate::resource_policy::EmbedderPolicy::UnsafeNone || credentialless {
                return false;
            }
            let mut request = turbo_surf_core::request_context::RequestContext::resource(
                &window_origin(scope, parent),
                turbo_surf_core::request_context::Destination::Iframe,
                turbo_surf_core::request_context::RequestMode::NoCors,
            );
            request.opaque_origin = window_is_opaque(scope, parent);
            crate::resource_policy::EmbedderPolicy::from_headers(&document.headers)
                == crate::resource_policy::EmbedderPolicy::UnsafeNone
                || !crate::resource_policy::embedder_allowed(
                    policy,
                    &request,
                    &document.url,
                    &document.headers,
                )
        });
        if embedding_blocked {
            sandbox.opaque = true;
            sandbox.origin_restricted = true;
            sandbox.scripts_blocked = true;
        }
        let data_document = document
            .as_ref()
            .is_some_and(|document| document.url.starts_with("data:"));
        sandbox.opaque |= data_document || document.is_none() && window_is_opaque(scope, parent);
        context.set_slot(Rc::new(sandbox));
        context.set_slot(Rc::new(FrameConfigured));
        context.set_slot(Rc::new(FrameHost(v8::Weak::new(scope, host))));
        if let Some(document) = &document {
            set_origin_token(scope, context, &document.url);
            metadata["url"] = serde_json::Value::String(document.url.clone());
            metadata["location"] = serde_json::Value::String(document.url.clone());
            metadata["origin"] = serde_json::Value::String(window_origin(scope, context));
        } else {
            context.set_security_token(parent.get_security_token(scope));
            metadata["origin"] = serde_json::Value::String(window_origin(scope, parent));
            let owner_url = parent
                .get_slot::<FrameDocumentUrl>()
                .map(|url| url.0.clone())
                .unwrap_or_else(|| {
                    JsRuntime::op_state_from(scope)
                        .borrow()
                        .borrow::<Base>()
                        .0
                        .clone()
                });
            metadata["url"] = serde_json::Value::String(owner_url);
        }
        if sandbox.origin_restricted || data_document {
            context.use_default_security_token();
        }
        if sandbox.opaque {
            metadata["origin"] = serde_json::Value::String("null".into());
        }
        #[cfg(turbo_surf_native_proxy)]
        if replaced.is_some() {
            window_proxy::synchronize_origins(scope, context);
        }
        #[cfg(not(turbo_surf_native_proxy))]
        let _ = replaced;
        metadata["depth"] =
            serde_json::Value::from(context.get_slot::<FrameDepth>().map_or(1, |depth| depth.0));
        if metadata
            .get("initial")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            metadata["location"] = serde_json::Value::String("about:blank".into());
        }
        let url = metadata
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("about:blank");
        context.set_slot(Rc::new(FrameDocumentUrl(url.to_string())));
        context.set_slot(Rc::new(FrameDocumentLocation(
            metadata
                .get("location")
                .and_then(|value| value.as_str())
                .unwrap_or(url)
                .to_string(),
        )));
        let html_units = if embedding_blocked {
            "<!doctype html><html><head></head><body></body></html>"
                .encode_utf16()
                .collect::<Vec<_>>()
        } else {
            document.as_ref().map_or(supplied_units, |document| {
                document.html.encode_utf16().collect()
            })
        };
        let html = String::from_utf16_lossy(&html_units);
        let mut csp = if let Some(document) = document
            .as_ref()
            .filter(|document| !document.url.starts_with("data:"))
        {
            CspHandle::new(url, &document.headers, &html)
        } else {
            let handle = CspHandle::new(url, &[], &html);
            if let Some(parent_policy) = parent.get_slot::<CspHandle>() {
                let inherited = parent_policy.policy.borrow().current().clone();
                let mut policy = handle.policy.borrow_mut();
                policy.header = inherited.clone();
                policy.initial_base.inherit_headers(&inherited);
                policy.active = inherited;
                policy.report_only = parent_policy.policy.borrow().report_only.clone();
                policy.referrer_policy = parent_policy.policy.borrow().referrer_policy;
                policy.reporting_endpoints =
                    parent_policy.policy.borrow().reporting_endpoints.clone();
                policy.embedder_policy = parent_policy.policy.borrow().embedder_policy;
            }
            handle
        };
        let response_metadata = document.as_ref().map_or_else(
            || {
                let source = parent.get_slot::<FrameDocumentLocation>().map_or_else(
                    || {
                        JsRuntime::op_state_from(scope)
                            .borrow()
                            .borrow::<Base>()
                            .0
                            .clone()
                    },
                    |location| location.0.clone(),
                );
                let referrer = if metadata.get("location").and_then(|value| value.as_str())
                    == Some("about:blank")
                {
                    source
                } else {
                    let policy = frame_referrer_policy_token(scope, host)
                        .as_deref()
                        .and_then(turbo_surf_core::referrer_policy::ReferrerPolicy::parse)
                        .unwrap_or_else(|| {
                            parent
                                .get_slot::<CspHandle>()
                                .map_or(Default::default(), |handle| {
                                    handle.policy.borrow().referrer_policy
                                })
                        });
                    crate::document_response::local_navigation_referrer(&source, policy)
                };
                crate::DocumentResponseMetadata {
                    status: 0,
                    referrer,
                }
            },
            |document| document.response_metadata.clone(),
        );
        csp.response_metadata.replace(response_metadata.clone());
        let state = JsRuntime::op_state_from(scope);
        csp.partition = document
            .as_ref()
            .and_then(|document| document.partition.clone())
            .or_else(|| frame_partition(scope, &mut state.borrow_mut(), parent, host));
        if let Some(parent_policy) = parent.get_slot::<CspHandle>() {
            let parent_origin = window_origin(scope, parent);
            let child_origin = window_origin(scope, context);
            if let Some(document) = &document {
                csp.policy.borrow_mut().permissions_policy =
                    crate::permissions_policy::PermissionsPolicy::from_headers(
                        &child_origin,
                        &document.headers,
                    );
            }
            let allow = frame_container_allow(scope, host);
            csp.policy.borrow_mut().permissions_policy.inherit(
                &parent_policy.policy.borrow().permissions_policy,
                &parent_origin,
                &child_origin,
                &child_origin,
                &allow,
            );
        }
        let child_origin = window_origin(scope, context);
        let allowed = !sandbox.opaque
            && !embedding_blocked
            && secure_context_for(scope, context)
            && parent
                .get_slot::<SharedMemoryCapability>()
                .is_some_and(|capability| capability.allowed)
            && csp.policy.borrow().permissions_policy.allows(
                "cross-origin-isolated",
                &child_origin,
                &child_origin,
            );
        let group = parent
            .get_slot::<SharedMemoryCapability>()
            .map_or(csp.id, |capability| capability.group);
        context.set_slot(Rc::new(SharedMemoryCapability {
            allowed,
            group,
            origin: child_origin,
        }));
        let initial_base = csp.policy.borrow().initial_base.clone();
        context.set_slot(Rc::new(csp.clone()));
        JsRuntime::op_state_from(scope)
            .borrow_mut()
            .borrow_mut::<CspContexts>()
            .0
            .insert(csp.id, csp);
        let mut policy = FrameScriptPolicy::from_html_units(&html_units, url, &initial_base);
        let source = metadata
            .get("location")
            .and_then(|value| value.as_str())
            .unwrap_or(url)
            .to_string();
        context.set_slot(Rc::new(FrameDocumentSourceUrl(source.clone())));
        for script in &mut policy.scripts {
            if script.src.is_none() {
                script.source = source.clone();
            }
        }
        if sandbox.scripts_blocked {
            policy.scripts.clear();
            policy.inline.clear();
            policy.external.clear();
        }
        let document_base = policy.base.clone();
        context.set_slot(Rc::new(policy));
        let core = JsRuntime::op_state_from(scope)
            .borrow()
            .borrow::<FrameContexts>()
            .core
            .clone()
            .ok_or_else(|| JsErrorBox::generic("iframe host core unavailable"))?;
        let scope = &mut v8::ContextScope::new(scope, context);
        #[cfg(turbo_surf_native_proxy)]
        register_frame_modules(scope, &document_base)?;
        let source_positions = context.get_slot::<CspHandle>().is_some_and(|handle| {
            let policy = handle.policy.borrow();
            policy.initial_base.has_policy() || policy.report_only.has_policy()
        });
        crate::browser_env::defer_key_order(scope);
        if metadata["location"] == "about:srcdoc" {
            crate::browser_env::install_independent_srcdoc_units(
                scope,
                &html_units,
                source_positions,
            );
        } else if source_positions {
            crate::browser_env::install_independent_utf16_source_positions(scope, &html_units);
        } else {
            crate::browser_env::install_independent_html_units(scope, &html_units);
        }
        install_css_authorization(scope)?;
        // Before the environment bootstrap, which already creates the parsed iframes.
        let facts = match &document {
            Some(document) => {
                DocumentFacts::response(scope, context, &document.url, &document.headers)
            }
            None => DocumentFacts::inherited(scope, context, parent),
        };
        install_document_facts(scope, facts);
        #[cfg(turbo_surf_native_proxy)]
        window_proxy::install_document(scope);
        let global = context.global(scope);
        let core = v8::Local::new(scope, core);

        let location_key = v8::String::new(scope, "location").unwrap();
        if let Some(location) = global
            .get(scope, location_key.into())
            .and_then(|v| v.to_object(scope))
        {
            for (name, value) in [
                ("href", url),
                (
                    "origin",
                    metadata
                        .get("origin")
                        .and_then(|v| v.as_str())
                        .unwrap_or("null"),
                ),
            ] {
                let key = v8::String::new(scope, name).unwrap();
                let value = v8::String::new(scope, value).unwrap();
                location.set(scope, key.into(), value.into());
            }
        }
        let depth_key = v8::String::new(scope, "__depth").unwrap();
        let depth = v8::Integer::new_from_unsigned(
            scope,
            context.get_slot::<FrameDepth>().map_or(1, |depth| depth.0),
        );
        global.set(scope, depth_key.into(), depth.into());
        if let Some(namespace) = frame_namespace(scope) {
            let name = v8::String::new(scope, "isFrame").unwrap();
            let flag = v8::Boolean::new(scope, true);
            namespace.set(scope, name.into(), flag.into());
            context.set_slot(Rc::new(FrameEnvironment(
                crate::context_ownership::object(scope, namespace).ok_or_else(|| {
                    JsErrorBox::generic("Frame environment ownership interrupted")
                })?,
            )));
        }
        install_environment(scope, core).map_err(JsErrorBox::generic)?;
        let secure = secure_context(scope);
        let crypto_core = core
            .to_object(scope)
            .ok_or_else(|| JsErrorBox::generic("Native crypto core unavailable"))?;
        crate::crypto_backend::install(scope, crypto_core, secure).map_err(JsErrorBox::generic)?;
        crate::audio_backend::install(scope, crypto_core).map_err(JsErrorBox::generic)?;
        crate::geometry_backend::install(scope, crypto_core).map_err(JsErrorBox::generic)?;
        if parent
            .remove_slot::<FailFrameInitializationForTesting>()
            .is_some()
        {
            parent.set_slot(Rc::new(FailedFrameInitializationForTesting));
            return Err(JsErrorBox::generic("injected frame initialization failure"));
        }
        #[cfg(turbo_surf_native_proxy)]
        window_proxy::capture_exception(scope);
        if let Some(namespace) = frame_namespace(scope) {
            context.set_slot(Rc::new(FrameEnvironment(
                crate::context_ownership::object(scope, namespace).ok_or_else(|| {
                    JsErrorBox::generic("Frame environment ownership interrupted")
                })?,
            )));
        }
        metadata["base"] = serde_json::Value::String(document_base);
        let metadata_location = metadata
            .get("location")
            .and_then(|value| value.as_str())
            .unwrap_or("about:blank")
            .to_string();
        let json = v8::String::new(scope, &metadata.to_string()).unwrap();
        let metadata = v8::json::parse(scope, json).unwrap();
        call_frame_helper(
            scope,
            "configureFrame",
            &[metadata, parent_window.into(), host.into()],
        )
        .ok_or_else(|| JsErrorBox::generic("frame configuration failed"))?;
        #[cfg(turbo_surf_native_proxy)]
        window_proxy::install_location(scope, &metadata_location);
        #[cfg(not(turbo_surf_native_proxy))]
        let _ = metadata_location;
        crate::browser_env::finish_key_order(scope);
        run_frame_init_script(scope)?;
        Ok(global)
    })();
    if result.is_err() {
        retire_failed_frame_tree(scope, context);
    }
    result
}

#[op2(nofast, reentrant)]
fn op_retire_frame<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window: v8::Local<'s, v8::Object>,
    owner: v8::Local<'s, v8::Object>,
) -> Result<(), JsErrorBox> {
    let context = window
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::generic("iframe context unavailable"))?;
    let parent = context
        .get_slot::<FrameParent>()
        .ok_or_else(|| JsErrorBox::generic("target is not a child frame"))?;
    let parent = crate::context_ownership::local(scope, &parent.0);
    let caller = scope.get_entered_or_microtask_context();
    if !owner.strict_equals(parent.global(scope).into())
        || !caller
            .get_security_token(scope)
            .strict_equals(parent.get_security_token(scope))
    {
        return Err(JsErrorBox::generic(
            "frame can only be retired by its owning context",
        ));
    }
    retire_frame_tree(scope, context);
    Ok(())
}

#[op2(fast)]
fn op_set_frame_navigator(scope: &mut v8::PinScope, callback: v8::Local<v8::Function>) {
    if let Some(context) = callback.get_creation_context(scope) {
        let Some(callback) = crate::context_ownership::function(scope, callback) else {
            return;
        };
        context.set_slot(Rc::new(FrameNavigator(callback)));
    }
}

fn frame_children<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    owner: v8::Local<'s, v8::Context>,
) -> Vec<v8::Local<'s, v8::Context>> {
    let contexts = JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<FrameContexts>()
        .contexts
        .clone();
    contexts
        .into_iter()
        .filter_map(|context| {
            let context = v8::Local::new(scope, context);
            if context.get_slot::<FrameRetired>().is_some()
                || context.get_slot::<FrameConfigured>().is_none()
            {
                return None;
            }
            let parent = context.get_slot::<FrameParent>()?;
            if crate::context_ownership::local(scope, &parent.0) != owner {
                return None;
            }
            let host = context.get_slot::<FrameHost>()?;
            let host = host.0.to_local(scope)?;
            crate::browser_env::is_light_document_node(scope, host).then_some(context)
        })
        .collect()
}

#[op2]
fn op_frame_move_candidates<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    nodes: v8::Local<'s, v8::Array>,
) -> v8::Local<'s, v8::Array> {
    let contexts = JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<FrameContexts>()
        .contexts
        .clone();
    let owner = document.get_creation_context(scope);
    let mut values = Vec::new();
    for context in contexts {
        let context = v8::Local::new(scope, context);
        if context.get_slot::<FrameRetired>().is_some() {
            continue;
        }
        let Some(parent) = context.get_slot::<FrameParent>() else {
            continue;
        };
        if Some(crate::context_ownership::local(scope, &parent.0)) != owner {
            continue;
        }
        let Some(host) = context.get_slot::<FrameHost>() else {
            continue;
        };
        let Some(host) = host.0.to_local(scope) else {
            continue;
        };
        for index in 0..nodes.length() {
            let Some(node) = nodes
                .get_index(scope, index)
                .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
            else {
                continue;
            };
            if crate::browser_env::composed_contains_node(scope, node, host) {
                values.push(host.into());
                break;
            }
        }
    }
    v8::Array::new_with_elements(scope, &values)
}

#[op2]
fn op_window_child_frames<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
) -> v8::Local<'s, v8::Array> {
    let contexts = document
        .get_creation_context(scope)
        .map(|owner| frame_children(scope, owner))
        .unwrap_or_default();
    let values = contexts
        .into_iter()
        .map(|context| context.global(scope).into())
        .collect::<Vec<_>>();
    v8::Array::new_with_elements(scope, &values)
}

#[op2(fast)]
fn op_window_opaque(scope: &mut v8::PinScope, window: v8::Local<v8::Object>) -> bool {
    window
        .get_creation_context(scope)
        .is_none_or(|context| window_is_opaque(scope, context))
}

fn window_is_opaque(scope: &mut v8::PinScope, context: v8::Local<v8::Context>) -> bool {
    context.get_slot::<FrameSandbox>().map_or_else(
        || !context.get_security_token(scope).is_string(),
        |flags| flags.opaque,
    )
}

#[op2(fast)]
fn op_document_scripts_allowed(scope: &mut v8::PinScope, document: v8::Local<v8::Object>) -> bool {
    document.get_creation_context(scope).is_some_and(|context| {
        context.get_slot::<FrameRetired>().is_none()
            && !context
                .get_slot::<FrameSandbox>()
                .is_some_and(|flags| flags.scripts_blocked)
    })
}

#[op2(fast)]
fn op_native_location_installed(scope: &mut v8::PinScope) -> bool {
    #[cfg(turbo_surf_native_proxy)]
    {
        window_proxy::location_installed(scope)
    }
    #[cfg(not(turbo_surf_native_proxy))]
    {
        let _ = scope;
        false
    }
}

#[op2(fast)]
fn op_native_window_proxy() -> bool {
    cfg!(turbo_surf_native_proxy)
}

#[op2(fast)]
fn op_window_message_pending(state: &mut OpState) -> bool {
    !state.borrow::<FrameContexts>().messages.is_empty()
}

#[op2(fast)]
fn op_window_active(scope: &mut v8::PinScope, window: v8::Local<v8::Object>) -> bool {
    window
        .get_creation_context(scope)
        .is_some_and(|context| context.get_slot::<FrameRetired>().is_none())
}

#[op2(reentrant)]
fn op_frame_finish<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window: v8::Local<v8::Object>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let context = window
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::generic("iframe context unavailable"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    call_frame_helper(scope, "fireFrameLoad", &[])
        .ok_or_else(|| JsErrorBox::generic("frame lifecycle unavailable"))
}

#[op2(fast, reentrant)]
fn op_frame_parser_done(
    scope: &mut v8::PinScope,
    window: v8::Local<v8::Object>,
) -> Result<(), JsErrorBox> {
    let context = window
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::generic("iframe context unavailable"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    call_frame_helper(scope, "frameParsingDone", &[])
        .ok_or_else(|| JsErrorBox::generic("frame lifecycle unavailable"))?;
    Ok(())
}

#[op2(fast, reentrant)]
fn op_frame_dom_ready(
    scope: &mut v8::PinScope,
    window: v8::Local<v8::Object>,
) -> Result<(), JsErrorBox> {
    let context = window
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::generic("iframe context unavailable"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    call_frame_helper(scope, "frameDomReady", &[])
        .ok_or_else(|| JsErrorBox::generic("frame lifecycle unavailable"))?;
    Ok(())
}

#[op2(reentrant)]
fn op_frame_resources<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window: v8::Local<v8::Object>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let context = window
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::generic("iframe context unavailable"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    let global = context.global(scope);
    let key = v8::String::new(scope, "document").unwrap();
    let document = global.get(scope, key.into()).unwrap();
    call_frame_helper(scope, "loadDocumentResources", &[document, global.into()])
        .ok_or_else(|| JsErrorBox::generic("frame resource loading unavailable"))
}

#[op2(reentrant)]
fn op_create_frame<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    parent_window: v8::Local<'s, v8::Object>,
) -> Result<v8::Local<'s, v8::Object>, JsErrorBox> {
    let parent = parent_window
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::generic("frame owner context unavailable"))?;
    let caller = scope.get_entered_or_microtask_context();
    if !parent_window.strict_equals(parent.global(scope).into())
        || !caller
            .get_security_token(scope)
            .strict_equals(parent.get_security_token(scope))
    {
        return Err(JsErrorBox::generic("frame owner is not same-origin"));
    }
    create_frame(scope, parent)
}

fn retire_frame_tree<'s>(scope: &mut v8::PinScope<'s, '_>, previous: v8::Local<'s, v8::Context>) {
    retire_frame_tree_impl(scope, previous, true);
}

// Failed initialization has no published document or runnable guest task. Stop
// native ownership without invoking partially installed JavaScript helpers.
fn retire_failed_frame_tree<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    previous: v8::Local<'s, v8::Context>,
) {
    retire_frame_tree_impl(scope, previous, false);
}

fn retire_frame_tree_impl<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    previous: v8::Local<'s, v8::Context>,
    call_helpers: bool,
) {
    previous.set_slot(Rc::new(FrameDirectRetirement));
    let contexts = JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<FrameContexts>()
        .contexts
        .clone();
    for context in contexts {
        let context = v8::Local::new(scope, context);
        let mut ancestor = context;
        loop {
            if ancestor == previous {
                if call_helpers {
                    let old_scope = &mut v8::ContextScope::new(scope, context);
                    let _ = call_frame_helper(old_scope, "cancelModuleGraphs", &[]);
                    let _ = call_frame_helper(old_scope, "cancelDocumentTasks", &[]);
                    let _ = call_frame_helper(old_scope, "retireResourceDocument", &[]);
                }
                if context.get_slot::<FrameRetired>().is_none() {
                    crate::script_inventory::retire_frame(scope, context);
                }
                fetch_streams::retire_context(scope, context);
                crate::message_ports::retire_context(scope, context);
                storage_events::retire(context);
                crate::browser_env::retire_native_document(scope, context);
                context.set_slot(Rc::new(FrameRetired));
                crate::context_ownership::release_parent(scope, context);
                #[cfg(turbo_surf_native_proxy)]
                JsRuntime::detach_module_context(scope, context);
                break;
            }
            let Some(parent) = ancestor.get_slot::<FrameParent>() else {
                break;
            };
            ancestor = crate::context_ownership::local(scope, &parent.0);
        }
    }
    let state = JsRuntime::op_state_from(scope);
    let discarded = {
        let mut state = state.borrow_mut();
        let frames = state.borrow_mut::<FrameContexts>();
        frames.contexts.retain(|context| {
            v8::Local::new(scope, context)
                .get_slot::<FrameRetired>()
                .is_none()
        });
        let messages = std::mem::take(&mut frames.messages);
        let (discarded, deliverable) = messages
            .into_iter()
            .partition::<std::collections::VecDeque<_>, _>(|message| {
                v8::Local::new(scope, &message.target)
                    .get_slot::<FrameRetired>()
                    .is_some()
            });
        frames.messages = deliverable;
        discarded
    };
    // Native transfer destructors can touch endpoint queues; release them after
    // the op-state borrow. Messages addressed to a live parent remain pending.
    drop(discarded);
}

fn create_frame<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    parent: v8::Local<'s, v8::Context>,
) -> Result<v8::Local<'s, v8::Object>, JsErrorBox> {
    allocate_frame(scope, parent, None, true)
}

fn allocate_frame<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    parent: v8::Local<'s, v8::Context>,
    previous: Option<v8::Local<'s, v8::Context>>,
    initialize: bool,
) -> Result<v8::Local<'s, v8::Object>, JsErrorBox> {
    if parent.get_slot::<FrameRetired>().is_some() {
        return Err(JsErrorBox::generic(
            "cannot create a child of an inactive document",
        ));
    }
    let depth = parent
        .get_slot::<FrameDepth>()
        .map_or(1, |depth| depth.0 + 1);
    if depth > 12 {
        return Err(JsErrorBox::generic("iframe depth limit reached"));
    }
    let token = parent.get_security_token(scope);
    let state = JsRuntime::op_state_from(scope);
    let saved_core = state
        .borrow()
        .borrow::<FrameContexts>()
        .core
        .clone()
        .ok_or_else(|| JsErrorBox::generic("frame host core unavailable"))?;
    let core = v8::Local::new(scope, saved_core);
    #[cfg(turbo_surf_native_proxy)]
    let context = window_proxy::context(scope, previous);
    #[cfg(not(turbo_surf_native_proxy))]
    let context = {
        let _ = previous;
        v8::Context::new(scope, Default::default())
    };
    crate::script_inventory::register_frame(scope, context);
    context.set_slot(Rc::new(FrameContextMarker));
    context.set_slot(Rc::new(FrameDepth(depth)));
    context.set_slot(Rc::new(FrameParent(
        crate::context_ownership::parent(scope, context, parent)
            .ok_or_else(|| JsErrorBox::generic("Frame parent ownership interrupted"))?,
    )));
    context.set_security_token(token);
    if let Some(capability) = parent.get_slot::<SharedMemoryCapability>() {
        context.set_slot(Rc::new((*capability).clone()));
    }
    // deno_core's isolate-wide Promise/import callbacks use these public slots.
    // Borrow the main realm's allocation; child contexts never own/decrement it.
    for slot in [
        deno_core::CONTEXT_STATE_SLOT_INDEX,
        deno_core::MODULE_MAP_SLOT_INDEX,
    ] {
        let pointer = parent.get_aligned_pointer_from_embedder_data(slot);
        // SAFETY: the main realm owns these pointers until this isolate drops.
        unsafe {
            context.set_aligned_pointer_in_embedder_data(slot, pointer);
        }
    }
    let global = context.global(scope);
    if initialize {
        let scope = &mut v8::ContextScope::new(scope, context);
        if let Some(parent_policy) = parent.get_slot::<CspHandle>() {
            let mut csp = CspHandle::new("about:blank", &[], "");
            csp.partition = parent_policy.partition.clone();
            let inherited = parent_policy.policy.borrow().current().clone();
            {
                let mut policy = csp.policy.borrow_mut();
                policy.header = inherited.clone();
                policy.initial_base.inherit_headers(&inherited);
                policy.active = inherited;
                policy.referrer_policy = parent_policy.policy.borrow().referrer_policy;
                policy.reporting_endpoints =
                    parent_policy.policy.borrow().reporting_endpoints.clone();
                policy.embedder_policy = parent_policy.policy.borrow().embedder_policy;
                policy.permissions_policy =
                    parent_policy.policy.borrow().permissions_policy.clone();
            }
            context.set_slot(Rc::new(csp.clone()));
            JsRuntime::op_state_from(scope)
                .borrow_mut()
                .borrow_mut::<CspContexts>()
                .0
                .insert(csp.id, csp);
        }
        crate::browser_env::defer_key_order(scope);
        crate::browser_env::install_independent_html(
            scope,
            "<!DOCTYPE html><html><head></head><body></body></html>",
        );
        install_css_authorization(scope)?;
        let facts = DocumentFacts::inherited(scope, context, parent);
        install_document_facts(scope, facts);
        #[cfg(turbo_surf_native_proxy)]
        window_proxy::install_document(scope);

        let name = v8::String::new(scope, "ts").unwrap();
        let namespace_key = v8::Symbol::for_key(scope, name);
        let namespace = global
            .get(scope, namespace_key.into())
            .and_then(|v| v.to_object(scope))
            .ok_or_else(|| {
                storage_events::retire(context);
                context.set_slot(Rc::new(FrameRetired));
                crate::context_ownership::release_parent(scope, context);
                JsErrorBox::generic("frame initialization was interrupted")
            })?;
        let name = v8::String::new(scope, "isFrame").unwrap();
        let flag = v8::Boolean::new(scope, true);
        namespace.set(scope, name.into(), flag.into());
        context.set_slot(Rc::new(FrameEnvironment(
            crate::context_ownership::object(scope, namespace)
                .ok_or_else(|| JsErrorBox::generic("Frame environment ownership interrupted"))?,
        )));
        let depth_key = v8::String::new(scope, "__depth").unwrap();
        let depth = v8::Integer::new_from_unsigned(scope, depth);
        global.set(scope, depth_key.into(), depth.into());
        install_environment(scope, core).map_err(JsErrorBox::generic)?;
        let secure = secure_context(scope);
        let crypto_core = core
            .to_object(scope)
            .ok_or_else(|| JsErrorBox::generic("Native crypto core unavailable"))?;
        crate::crypto_backend::install(scope, crypto_core, secure).map_err(JsErrorBox::generic)?;
        crate::audio_backend::install(scope, crypto_core).map_err(JsErrorBox::generic)?;
        crate::geometry_backend::install(scope, crypto_core).map_err(JsErrorBox::generic)?;
        #[cfg(turbo_surf_native_proxy)]
        window_proxy::capture_exception(scope);
        crate::browser_env::finish_key_order(scope);
        run_frame_init_script(scope)?;
        // Host capabilities are passed privately and never installed on the global.
    }
    let retained = v8::Global::new(scope, context);
    JsRuntime::op_state_from(scope)
        .borrow_mut()
        .borrow_mut::<FrameContexts>()
        .contexts
        .push(retained);
    Ok(global)
}

#[op2(nofast, reentrant)]
fn op_run_local_timers<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    max: v8::Local<'s, v8::Value>,
) -> u32 {
    let Some(context) = document.get_creation_context(scope) else {
        return 0;
    };
    let _task = WindowTimerTask::new(context);
    let active = &mut v8::ContextScope::new(scope, context);
    call_frame_helper(active, "runLocalTimers", &[max])
        .and_then(|value| value.uint32_value(active))
        .unwrap_or(0)
}

#[op2(fast, reentrant)]
fn op_run_frame_timers(scope: &mut v8::PinScope) -> u32 {
    run_frame_timers(scope, 5000)
}

fn run_frame_timers(scope: &mut v8::PinScope, max: u32) -> u32 {
    let frames = JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<FrameContexts>()
        .contexts
        .clone();
    let mut fired = 0;
    for context in frames {
        let context = v8::Local::new(scope, context);
        if context.get_slot::<FrameRetired>().is_some() {
            continue;
        }
        let scope = &mut v8::ContextScope::new(scope, context);
        let code = v8::String::new(
            scope,
            &format!("globalThis[Symbol.for('ts')].runTimers({max})"),
        )
        .unwrap();
        if let Some(result) =
            v8::Script::compile(scope, code, None).and_then(|script| script.run(scope))
        {
            fired += result.uint32_value(scope).unwrap_or(0);
        }
    }
    fired
}

#[op2(fast, reentrant)]
fn op_frame_pending(scope: &mut v8::PinScope) -> u32 {
    let frames = JsRuntime::op_state_from(scope)
        .borrow()
        .borrow::<FrameContexts>()
        .contexts
        .clone();
    let mut pending = 0;
    for context in frames {
        let context = v8::Local::new(scope, context);
        if context.get_slot::<FrameRetired>().is_some() {
            continue;
        }
        let scope = &mut v8::ContextScope::new(scope, context);
        let code = v8::String::new(
            scope,
            "globalThis[Symbol.for('ts')].pendingWork() === '1' ? 1 : 0",
        )
        .unwrap();
        if let Some(result) =
            v8::Script::compile(scope, code, None).and_then(|script| script.run(scope))
        {
            pending += result.uint32_value(scope).unwrap_or(0);
        }
    }
    pending
}

#[op2(reentrant)]
fn op_frame_module<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window: v8::Local<v8::Object>,
    index: u32,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    if !cfg!(turbo_surf_native_proxy) {
        return Err(JsErrorBox::generic(
            "Child-frame ES modules require the exact native SDK",
        ));
    }
    let context = window
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::generic("iframe context unavailable"))?;
    if context.get_slot::<FrameRetired>().is_some()
        || context
            .get_slot::<FrameSandbox>()
            .is_some_and(|flags| flags.scripts_blocked)
    {
        return Err(JsErrorBox::generic("frame module execution is blocked"));
    }
    let policy = context
        .get_slot::<FrameScriptPolicy>()
        .ok_or_else(|| JsErrorBox::generic("frame script policy unavailable"))?;
    if !policy
        .scripts
        .iter()
        .any(|script| script.module && script.index == index as usize)
    {
        return Err(JsErrorBox::generic(
            "frame module is not authorized by its document",
        ));
    }
    let scope = &mut v8::ContextScope::new(scope, context);
    let index = v8::Integer::new_from_unsigned(scope, index);
    call_frame_helper(scope, "executeFrameModule", &[index.into()])
        .ok_or_else(|| JsErrorBox::generic("frame module dispatch failed"))
}

#[op2(reentrant)]
fn op_queue_frame_script<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    state: &mut OpState,
    window: v8::Local<'s, v8::Object>,
    source: v8::Local<'s, v8::String>,
    #[string] mut source_url: String,
    response_id: u32,
    script_index: u32,
) -> Result<v8::Local<'s, v8::Promise>, JsErrorBox> {
    let mut code = crate::browser_env::string_code_units(scope, source);
    let caller = v8::Global::new(scope, scope.get_entered_or_microtask_context());
    let caller_local = v8::Local::new(scope, &caller);
    let context = window
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::generic("iframe context unavailable"))?;
    if context.get_slot::<FrameRetired>().is_some() {
        return Err(JsErrorBox::generic("iframe document has been replaced"));
    }
    if context
        .get_slot::<FrameSandbox>()
        .is_some_and(|flags| flags.scripts_blocked)
    {
        return Err(JsErrorBox::generic(
            "script execution is blocked by the iframe sandbox",
        ));
    }
    let same_origin = caller_local
        .get_security_token(scope)
        .strict_equals(context.get_security_token(scope));
    let response = if response_id != 0 {
        Some(
            state
                .borrow_mut::<FrameContexts>()
                .scripts
                .remove(&response_id)
                .ok_or_else(|| JsErrorBox::generic("frame script response unavailable"))?,
        )
    } else {
        None
    };
    let muted_errors = response
        .as_ref()
        .is_some_and(|response| response.muted_errors);
    if !same_origin {
        // Parent-side guest parsers can be replaced by page code. Cross-origin
        // execution must match the host's authenticated document/network bytes.
        let policy = context
            .get_slot::<FrameScriptPolicy>()
            .ok_or_else(|| JsErrorBox::generic("frame script policy unavailable"))?;
        if let Some(response) = response {
            if !policy.external.contains(&response.request_url) {
                return Err(JsErrorBox::generic(
                    "frame script URL is not authorized by its document",
                ));
            }
            code = response.code.encode_utf16().collect();
            source_url = response.url;
        } else {
            let candidate = normalized_frame_source(&code);
            if !policy
                .inline
                .iter()
                .any(|trusted| normalized_frame_source(trusted) == candidate)
            {
                return Err(JsErrorBox::generic(
                    "frame script body is not authorized by its document",
                ));
            }
            source_url = context
                .get_slot::<FrameDocumentSourceUrl>()
                .map_or_else(|| "about:blank".to_string(), |url| url.0.clone());
        }
    }
    let element = {
        let scope = &mut v8::ContextScope::new(scope, context);
        let index = v8::Integer::new_from_unsigned(scope, script_index);
        call_frame_helper(scope, "frameScriptElement", &[index.into()])
            .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
            .map(|value| v8::Global::new(scope, value))
    };
    let document = {
        let scope = &mut v8::ContextScope::new(scope, context);
        let document = call_frame_helper(scope, "frameScriptDocument", &[])
            .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
            .ok_or_else(|| JsErrorBox::generic("frame script document unavailable"))?;
        v8::Global::new(scope, document)
    };
    let resolver = v8::PromiseResolver::new(scope)
        .ok_or_else(|| JsErrorBox::generic("frame script completion allocation failed"))?;
    let promise = resolver.get_promise(scope);
    let context = v8::Global::new(scope, context);
    let queue = state.borrow_mut::<ClassicScripts>();
    queue.jobs.push_back(ClassicScriptJob {
        resolver: v8::Global::new(scope, resolver),
        source: source_url,
        code,
        context,
        waiter_context: caller,
        document,
        muted_errors,
        element,
    });
    Ok(promise)
}

fn normalized_frame_source(units: &[u16]) -> Vec<u16> {
    let mut source = Vec::with_capacity(units.len());
    let mut previous_cr = false;
    for &unit in units {
        if unit == 10 && previous_cr {
            previous_cr = false;
            continue;
        }
        previous_cr = unit == 13;
        source.push(if previous_cr { 10 } else { unit });
    }
    let start = source
        .iter()
        .take_while(|&&unit| char::from_u32(u32::from(unit)).is_some_and(char::is_whitespace))
        .count();
    source.drain(..start);
    source
}

fn frame_document_url(scope: &mut v8::PinScope, state: &OpState) -> String {
    scope
        .get_entered_or_microtask_context()
        .get_slot::<FrameDocumentUrl>()
        .map(|url| url.0.clone())
        .unwrap_or_else(|| state.borrow::<Base>().0.clone())
}

struct DocumentRuntimeStateAccessor(v8::Weak<v8::Function>);
#[op2(fast)]
fn op_set_document_runtime_state_accessor(
    scope: &mut v8::PinScope,
    callback: v8::Local<v8::Function>,
) {
    if let Some(context) = callback.get_creation_context(scope) {
        let Some(callback) = crate::context_ownership::function(scope, callback) else {
            return;
        };
        context.set_slot(Rc::new(DocumentRuntimeStateAccessor(callback)));
    }
}
#[op2(reentrant)]
fn op_document_runtime_state<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
) -> Result<v8::Local<'s, v8::Object>, JsErrorBox> {
    document_runtime_state(scope, document)
}

fn document_runtime_state<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
) -> Result<v8::Local<'s, v8::Object>, JsErrorBox> {
    let context = document
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::type_error("Illegal invocation"))?;
    let accessor = context
        .get_slot::<DocumentRuntimeStateAccessor>()
        .ok_or_else(|| JsErrorBox::type_error("Document context unavailable"))?;
    let callback = crate::context_ownership::local(scope, &accessor.0);
    let receiver = v8::undefined(scope).into();
    callback
        .call(scope, receiver, &[document.into()])
        .and_then(|value| value.to_object(scope))
        .ok_or_else(|| JsErrorBox::type_error("Illegal invocation"))
}

#[op2(reentrant)]
fn op_frame_owner_window<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    if document
        .get_creation_context(scope)
        .is_none_or(|context| context.get_slot::<FrameRetired>().is_some())
    {
        return Ok(v8::null(scope).into());
    }
    let state = document_runtime_state(scope, document)?;
    let key = v8::String::new(scope, "window").unwrap();
    state
        .get(scope, key.into())
        .ok_or_else(|| JsErrorBox::generic("Frame owner interrupted"))
}

#[op2(fast)]
fn op_document_cookie_allowed(scope: &mut v8::PinScope, document: v8::Local<v8::Object>) -> bool {
    document
        .get_creation_context(scope)
        .is_some_and(|context| !window_is_opaque(scope, context))
}

#[op2(fast)]
fn op_set_inert_document_initializer(
    scope: &mut v8::PinScope,
    function: v8::Local<v8::Function>,
) -> Result<(), JsErrorBox> {
    if crate::browser_env::set_inert_document_initializer(scope, function) {
        Ok(())
    } else {
        Err(JsErrorBox::generic(
            "Inert document initializer installation interrupted",
        ))
    }
}

/// The document's parser scripts in parse order (declarative shadow roots included).
#[op2(reentrant)]
fn op_parser_scripts<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
) -> v8::Local<'s, v8::Array> {
    let scripts = crate::browser_env::parser_scripts(scope, document)
        .into_iter()
        .map(Into::into)
        .collect::<Vec<v8::Local<v8::Value>>>();
    v8::Array::new_with_elements(scope, &scripts)
}

#[op2(reentrant)]
fn op_parse_html_document<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    html: v8::Local<v8::String>,
    sanitize: bool,
) -> v8::Local<'s, v8::Object> {
    let units = crate::browser_env::string_code_units(scope, html);
    crate::browser_env::parse_html_document_units(scope, &units, sanitize)
}

#[op2(reentrant)]
fn op_document_create_element<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    #[string] tag: &str,
) -> Result<v8::Local<'s, v8::Object>, JsErrorBox> {
    crate::browser_env::create_document_element(scope, document, tag)
        .ok_or_else(|| JsErrorBox::type_error("Illegal invocation"))
}

#[op2(reentrant)]
fn op_document_import_node<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    node: v8::Local<'s, v8::Object>,
    deep: bool,
) -> Result<v8::Local<'s, v8::Object>, JsErrorBox> {
    crate::browser_env::import_document_node(scope, document, node, deep)
        .map_err(JsErrorBox::type_error)
}

#[op2(fast)]
fn op_document_has_active_window(
    scope: &mut v8::PinScope,
    document: v8::Local<v8::Object>,
) -> bool {
    document
        .get_creation_context(scope)
        .is_some_and(|context| context.get_slot::<FrameRetired>().is_none())
}

#[op2]
#[string]
fn op_document_location_url(
    scope: &mut v8::PinScope,
    state: &mut OpState,
    document: v8::Local<v8::Object>,
) -> String {
    let Some(context) = document.get_creation_context(scope) else {
        return state.borrow::<Base>().0.clone();
    };
    #[cfg(turbo_surf_native_proxy)]
    if let Some(url) = window_proxy::location_url(context) {
        return url;
    }
    context.get_slot::<FrameDocumentLocation>().map_or_else(
        || state.borrow::<Base>().0.clone(),
        |location| location.0.clone(),
    )
}

fn frame_referrer_policy_token(
    scope: &mut v8::PinScope,
    host: v8::Local<v8::Object>,
) -> Option<String> {
    let token =
        crate::browser_env::node_attribute(scope, host, "referrerpolicy")?.to_ascii_lowercase();
    turbo_surf_core::referrer_policy::ReferrerPolicy::parse(&token).map(|_| token)
}

#[op2]
#[string]
fn op_frame_referrer_policy(scope: &mut v8::PinScope, host: v8::Local<v8::Object>) -> String {
    frame_referrer_policy_token(scope, host).unwrap_or_default()
}

#[op2]
#[string]
fn op_document_referrer(scope: &mut v8::PinScope, document: v8::Local<v8::Object>) -> String {
    document
        .get_creation_context(scope)
        .and_then(|context| context.get_slot::<CspHandle>())
        .map_or_else(String::new, |handle| {
            handle.response_metadata.borrow().referrer.clone()
        })
}

#[op2]
#[string]
fn op_document_request_url(scope: &mut v8::PinScope, state: &mut OpState) -> String {
    frame_document_url(scope, state)
}

#[op2]
#[string]
fn op_frame_owner_metadata(
    scope: &mut v8::PinScope,
    state: &mut OpState,
    document: v8::Local<v8::Object>,
) -> Result<String, JsErrorBox> {
    let context = document
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::type_error("Frame owner document unavailable"))?;
    let location = context.get_slot::<FrameDocumentLocation>().map_or_else(
        || state.borrow::<Base>().0.clone(),
        |location| location.0.clone(),
    );
    let origin = window_origin(scope, context);
    serde_json::to_string(&(location, origin))
        .map_err(|error| JsErrorBox::generic(error.to_string()))
}

#[op2]
#[string]
fn op_document_request_source(
    scope: &mut v8::PinScope,
    state: &mut OpState,
    document: v8::Local<v8::Object>,
) -> String {
    document
        .get_creation_context(scope)
        .and_then(|context| context.get_slot::<FrameDocumentUrl>())
        .map_or_else(|| state.borrow::<Base>().0.clone(), |url| url.0.clone())
}

fn frame_cookie_site_is_opaque<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    mut context: v8::Local<'s, v8::Context>,
) -> bool {
    let mut origins = Vec::new();
    for _ in 0..64 {
        if window_is_opaque(scope, context) {
            return true;
        }
        origins.push(window_origin(scope, context));
        let Some(parent) = context.get_slot::<FrameParent>() else {
            return origins.last().is_none_or(|top| {
                origins
                    .iter()
                    .any(|origin| !turbo_surf_core::request_context::same_site(origin, top))
            });
        };
        context = crate::context_ownership::local(scope, &parent.0);
    }
    true
}

#[op2]
#[serde]
fn op_document_cookie_site<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    state: &mut OpState,
    document: v8::Local<'s, v8::Object>,
) -> serde_json::Value {
    let opaque = document
        .get_creation_context(scope)
        .is_none_or(|context| frame_cookie_site_is_opaque(scope, context));
    serde_json::json!({"top_level_source":state.borrow::<Base>().0,"cookie_site_is_opaque":opaque})
}

#[op2]
#[string]
fn op_frame_cookie_get(
    scope: &mut v8::PinScope,
    state: &mut OpState,
    document: v8::Local<v8::Object>,
) -> String {
    apply_report_cookies(state);
    let url = document
        .get_creation_context(scope)
        .and_then(|context| context.get_slot::<FrameDocumentUrl>())
        .map(|url| url.0.clone())
        .unwrap_or_else(|| state.borrow::<Base>().0.clone());
    let jar = document
        .get_creation_context(scope)
        .and_then(|context| context.get_slot::<CspHandle>())
        .and_then(|handle| handle.partition.clone())
        .map_or_else(
            || state.borrow::<Jar>().clone(),
            |partition| partition.jar.clone(),
        );
    let value = jar.borrow().document_cookie(&url, cookie_now());
    value
}

#[op2(fast)]
fn op_frame_cookie_set(
    scope: &mut v8::PinScope,
    state: &mut OpState,
    document: v8::Local<v8::Object>,
    #[string] line: &str,
) {
    apply_report_cookies(state);
    let url = document
        .get_creation_context(scope)
        .and_then(|context| context.get_slot::<FrameDocumentUrl>())
        .map(|url| url.0.clone())
        .unwrap_or_else(|| state.borrow::<Base>().0.clone());
    let jar = document
        .get_creation_context(scope)
        .and_then(|context| context.get_slot::<CspHandle>())
        .and_then(|handle| handle.partition.clone())
        .map_or_else(
            || state.borrow::<Jar>().clone(),
            |partition| partition.jar.clone(),
        );
    jar.borrow_mut().set_from_document(&url, line, cookie_now());
}

type StorageEntries = Vec<(Vec<u16>, Vec<u16>)>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct StorageKey {
    partition: u32,
    site: String,
    origin: String,
    cross_site_ancestor: bool,
    session: bool,
    tab: u64,
}

#[derive(Default)]
struct StorageAreas {
    next: u32,
    next_tab: u64,
    by_origin: std::collections::BTreeMap<StorageKey, u32>,
    values: std::collections::BTreeMap<u32, StorageEntries>,
    subscribers: Vec<std::sync::Weak<std::sync::Mutex<storage_events::Inbox>>>,
    /// Private State Token issuers each top-frame origin has queried.
    private_token_issuers: std::collections::BTreeMap<String, Vec<String>>,
}

/// Native storage backing for one browsing tab across document navigations.
/// Independent handles have separate state. Frames share only within the same
/// top-level schemeful site, origin and credentialless partition.
struct StorageTab {
    areas: std::sync::Arc<std::sync::Mutex<StorageAreas>>,
    id: u64,
}
impl Drop for StorageTab {
    fn drop(&mut self) {
        let Ok(mut areas) = self.areas.lock() else {
            return;
        };
        let ids = areas
            .by_origin
            .iter()
            .filter_map(|(key, id)| (key.session && key.tab == self.id).then_some(*id))
            .collect::<Vec<_>>();
        areas
            .by_origin
            .retain(|key, _| !key.session || key.tab != self.id);
        for id in ids {
            areas.values.remove(&id);
        }
    }
}
#[derive(Clone)]
pub struct BrowserStorage(
    std::sync::Arc<std::sync::Mutex<StorageAreas>>,
    turbo_surf_core::http_cache::HttpCache,
    std::sync::Arc<StorageTab>,
    crate::csp_reporting::Reporter,
);
impl Default for BrowserStorage {
    fn default() -> Self {
        let areas = std::sync::Arc::new(std::sync::Mutex::new(StorageAreas::default()));
        Self(
            areas.clone(),
            turbo_surf_core::http_cache::HttpCache::default(),
            std::sync::Arc::new(StorageTab { areas, id: 0 }),
            crate::csp_reporting::Reporter::default(),
        )
    }
}
impl BrowserStorage {
    /// Share the profile's native HTTP cache with host navigation/resource callers.
    pub fn http_cache(&self) -> turbo_surf_core::http_cache::HttpCache {
        self.1.clone()
    }
    /// A new tab shares local storage and HTTP cache, with its own session namespace.
    /// Clones retain this tab's identity across document navigation.
    pub fn new_tab(&self) -> Result<Self, String> {
        let id = {
            let mut areas = self.0.lock().map_err(|_| "Storage backing unavailable")?;
            areas.next_tab = areas
                .next_tab
                .checked_add(1)
                .ok_or("Too many browsing tabs")?;
            areas.next_tab
        };
        Ok(Self(
            self.0.clone(),
            self.1.clone(),
            std::sync::Arc::new(StorageTab {
                areas: self.0.clone(),
                id,
            }),
            self.3.clone(),
        ))
    }
    /// Storage side effects for actual native HTTP response headers. This
    /// captures native storage only; it cannot retain a profile/report owner.
    /// Call before guest response policy, never for a replayed cache entry.
    pub fn response_storage_effects(
        &self,
    ) -> std::sync::Arc<turbo_surf_core::net::ResponseSideEffects<'static>> {
        self.partition_response_effects(0)
    }
    fn partition_response_effects(
        &self,
        partition: u32,
    ) -> std::sync::Arc<turbo_surf_core::net::ResponseSideEffects<'static>> {
        let storage = self.0.clone();
        let cache = self.1.clone();
        std::sync::Arc::new(move |context, target, headers| {
            cache.observe_response(context, target, headers);
            clear_storage_from_response(&storage, partition, context, target, headers);
        })
    }
    #[cfg(test)]
    fn clear_from_response(
        &self,
        partition: u32,
        context: &RequestContext,
        target: &str,
        headers: &[(String, String)],
    ) {
        clear_storage_from_response(&self.0, partition, context, target, headers)
    }
}

fn clear_storage_from_response(
    storage: &std::sync::Mutex<StorageAreas>,
    partition: u32,
    context: &RequestContext,
    target: &str,
    headers: &[(String, String)],
) {
    let Ok(url) = url::Url::parse(target) else {
        return;
    };
    let loopback = match url.host() {
        Some(url::Host::Domain(host)) => host == "localhost" || host.ends_with(".localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    let trustworthy = url.scheme() == "https" || url.scheme() == "http" && loopback;
    if !trustworthy || !context.sends_credentials(target) {
        return;
    }
    let clears = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("clear-site-data"))
        .any(|(_, value)| {
            value
                .split(',')
                .map(str::trim)
                .any(|token| matches!(token, "\"storage\"" | "\"*\""))
        });
    if !clears {
        return;
    }
    let top_navigation =
        context.destination == Destination::Document && context.mode == RequestMode::Navigate;
    let source = if top_navigation {
        target
    } else {
        context
            .top_level_source
            .as_deref()
            .or(context.source.as_deref())
            .unwrap_or(target)
    };
    let cross_site_ancestor = if top_navigation {
        false
    } else {
        context.cookie_site_is_opaque
            || context.destination == Destination::Iframe
                && !turbo_surf_core::request_context::same_site(source, target)
    };
    let Some(site) = turbo_surf_core::request_context::schemeful_site(source) else {
        return;
    };
    let Some(origin) = turbo_surf_core::request_context::origin(target) else {
        return;
    };
    let Ok(mut areas) = storage.lock() else {
        return;
    };
    let ids = areas
        .by_origin
        .iter()
        .filter(|(key, _)| {
            key.partition == partition
                && key.site == site
                && key.origin == origin
                && key.cross_site_ancestor == cross_site_ancestor
        })
        .map(|(key, id)| (*id, key.session))
        .collect::<Vec<_>>();
    let mut changed = Vec::new();
    for (id, session) in ids {
        if let Some(values) = areas.values.get_mut(&id) {
            // Chrome clears session namespaces and notifies their windows
            // even if empty; local storage only reports an actual change.
            if session || !values.is_empty() {
                values.clear();
                changed.push(id);
            }
        }
    }
    areas
        .subscribers
        .retain(|receiver| receiver.strong_count() != 0);
    let mut wake = Vec::new();
    for receiver in &areas.subscribers {
        let Some(receiver) = receiver.upgrade() else {
            continue;
        };
        let Ok(mut inbox) = receiver.lock() else {
            continue;
        };
        for area in &changed {
            inbox.push(*area);
        }
        if !changed.is_empty() {
            if let Some(waiter) = inbox.take_waiter() {
                wake.push(waiter);
            }
        }
    }
    drop(areas);
    for waiter in wake {
        waiter.wake();
    }
}

impl BrowserStorage {
    /// Host-only private transport injection for localhost certificate fixtures.
    #[doc(hidden)]
    pub fn set_csp_report_transport_for_testing(
        &mut self,
        client: turbo_surf_core::net::ReportClient,
    ) {
        self.3 = crate::csp_reporting::Reporter::with_client(client);
    }
    /// Native raw response effects. Capture storage/cache backing, never this
    /// profile or a V8/OpState owner, so pending IO cannot retain its sender.
    pub fn set_csp_report_response_effects(
        &self,
        effects: std::sync::Arc<turbo_surf_core::net::ResponseSideEffects<'static>>,
    ) {
        self.3.set_response_effects(effects);
    }
    #[doc(hidden)]
    pub fn csp_report_diagnostics_for_testing(&self) -> serde_json::Value {
        self.3.diagnostics()
    }
}

#[derive(Default)]
struct ReportCookieTargets(std::collections::HashMap<u64, std::rc::Weak<RefCell<CookieJar>>>);
fn apply_report_cookies(state: &mut OpState) {
    if !state.has::<BrowserStorage>() || !state.has::<ReportCookieTargets>() {
        return;
    }
    for (target, url, lines) in state.borrow::<BrowserStorage>().3.drain_cookies() {
        if let Some(jar) = state
            .borrow::<ReportCookieTargets>()
            .0
            .get(&target)
            .and_then(std::rc::Weak::upgrade)
        {
            jar.borrow_mut()
                .set_from_response(&url, &lines, cookie_now());
        }
    }
    state
        .borrow_mut::<ReportCookieTargets>()
        .0
        .retain(|_, jar| jar.strong_count() > 0);
}

#[op2(fast)]
fn op_storage_area(
    scope: &mut v8::PinScope,
    state: &mut OpState,
    window: v8::Local<v8::Object>,
    session: bool,
    unpartitioned: bool,
) -> Result<u32, JsErrorBox> {
    storage_area(scope, state, window, session, unpartitioned)
}
fn storage_area(
    scope: &mut v8::PinScope,
    state: &mut OpState,
    window: v8::Local<v8::Object>,
    session: bool,
    unpartitioned: bool,
) -> Result<u32, JsErrorBox> {
    let context = window
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::type_error("Illegal invocation"))?;
    if !window.strict_equals(context.global(scope).into()) {
        return Err(JsErrorBox::type_error("Illegal invocation"));
    }
    if window_is_opaque(scope, context) {
        return Err(JsErrorBox::type_error(
            "Storage is unavailable to an opaque origin",
        ));
    }
    let origin = window_origin(scope, context);
    let source = if unpartitioned {
        &origin
    } else {
        &state.borrow::<Base>().0
    };
    let site =
        turbo_surf_core::request_context::schemeful_site(source).unwrap_or_else(|| source.clone());
    let partition = context
        .get_slot::<CspHandle>()
        .and_then(|handle| handle.partition.clone())
        .map_or(0, |partition| partition.id);
    if !state.has::<BrowserStorage>() {
        state.put(BrowserStorage::default());
    }
    let backing = state.borrow::<BrowserStorage>().clone();
    let key = StorageKey {
        partition,
        site,
        origin,
        cross_site_ancestor: !unpartitioned && frame_cookie_site_is_opaque(scope, context),
        session,
        tab: if session { backing.2.id } else { 0 },
    };
    let mut areas = backing
        .0
        .lock()
        .map_err(|_| JsErrorBox::generic("Storage backing unavailable"))?;
    if let Some(id) = areas.by_origin.get(&key) {
        return Ok(*id);
    }
    areas.next = areas
        .next
        .checked_add(1)
        .ok_or_else(|| JsErrorBox::range_error("Too many storage areas"))?;
    let id = areas.next;
    areas.by_origin.insert(key, id);
    areas.values.insert(id, Vec::new());
    Ok(id)
}

struct StorageFactory(v8::Weak<v8::Function>);
#[op2(fast)]
fn op_storage_factory(scope: &mut v8::PinScope, callback: v8::Local<v8::Function>) {
    if let Some(context) = callback.get_creation_context(scope) {
        if let Some(callback) = crate::context_ownership::function(scope, callback) {
            context.set_slot(Rc::new(StorageFactory(callback)));
        }
    }
}
#[op2(reentrant)]
fn op_storage_object<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window: v8::Local<'s, v8::Object>,
    id: u32,
    session: bool,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let context = window
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::type_error("Illegal invocation"))?;
    if !window.strict_equals(context.global(scope).into()) {
        return Err(JsErrorBox::type_error("Illegal invocation"));
    }
    let factory = context
        .get_slot::<StorageFactory>()
        .ok_or_else(|| JsErrorBox::generic("Storage context unavailable"))?;
    let callback = crate::context_ownership::local(scope, &factory.0);
    let scope = &mut v8::ContextScope::new(scope, context);
    let id = v8::Integer::new_from_unsigned(scope, id).into();
    let session = v8::Boolean::new(scope, session).into();
    let receiver = v8::undefined(scope).into();
    callback
        .call(scope, receiver, &[id, session])
        .ok_or_else(|| JsErrorBox::generic("Storage object creation interrupted"))
}

struct StorageAccessFactory(v8::Weak<v8::Function>);

#[op2(fast)]
fn op_storage_access_factory(scope: &mut v8::PinScope, callback: v8::Local<v8::Function>) {
    if let Some(context) = callback.get_creation_context(scope) {
        if let Some(callback) = crate::context_ownership::function(scope, callback) {
            context.set_slot(Rc::new(StorageAccessFactory(callback)));
        }
    }
}

#[op2(reentrant)]
fn op_storage_access_handle<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    types: v8::Local<'s, v8::Object>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let context = document
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::type_error("Illegal invocation"))?;
    let factory = context
        .get_slot::<StorageAccessFactory>()
        .ok_or_else(|| JsErrorBox::generic("Storage access context unavailable"))?;
    let callback = crate::context_ownership::local(scope, &factory.0);
    let scope = &mut v8::ContextScope::new(scope, context);
    let receiver = v8::undefined(scope).into();
    callback
        .call(scope, receiver, &[types.into()])
        .ok_or_else(|| JsErrorBox::generic("Storage handle creation interrupted"))
}

#[op2]
#[serde]
fn op_storage_access_state(
    scope: &mut v8::PinScope,
    document: v8::Local<v8::Object>,
) -> serde_json::Value {
    let Some(context) = document.get_creation_context(scope) else {
        return serde_json::Value::Null;
    };
    let opaque = window_is_opaque(scope, context);
    let credentialless = context
        .get_slot::<CspHandle>()
        .is_some_and(|handle| handle.partition.is_some());
    let policy = context.get_slot::<CspHandle>().is_none_or(|handle| {
        handle
            .policy
            .borrow()
            .permissions_policy
            .allows_self("storage-access", &window_origin(scope, context))
    });
    let sandbox = context
        .get_slot::<FrameSandbox>()
        .is_some_and(|flags| flags.storage_access_blocked);
    serde_json::json!({"opaque":opaque,"credentialless":credentialless,"allowed":policy && !sandbox,"frame":context.get_slot::<FrameParent>().is_some()})
}

fn storage_string<'s>(scope: &mut v8::PinScope<'s, '_>, units: &[u16]) -> v8::Local<'s, v8::Value> {
    v8::String::new_from_two_byte(scope, units, v8::NewStringType::Normal)
        .unwrap()
        .into()
}

#[path = "storage_events.rs"]
mod storage_events;

enum StorageReply {
    Null,
    Value(Vec<u16>),
    Keys(Vec<Vec<u16>>),
}

enum StorageEdit {
    Set(Option<usize>, Vec<u16>, Vec<u16>),
    Remove(usize),
    Clear,
}
impl StorageEdit {
    fn apply(self, values: &mut StorageEntries) {
        match self {
            Self::Set(Some(index), _, value) => values[index].1 = value,
            Self::Set(None, key, value) => values.push((key, value)),
            Self::Remove(index) => {
                values.remove(index);
            }
            Self::Clear => values.clear(),
        }
    }
}

#[op2]
fn op_storage_call<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    state: &mut OpState,
    id: u32,
    receiver: v8::Local<'s, v8::Object>,
    #[string] command: &str,
    key: v8::Local<'s, v8::String>,
    value: v8::Local<'s, v8::String>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    const QUOTA_UNITS: usize = 5 * 1024 * 1024;
    let owner = receiver
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::type_error("Illegal invocation"))?;
    if storage_id(scope, receiver)? != id {
        return Err(JsErrorBox::type_error("Illegal invocation"));
    }
    #[cfg(turbo_surf_native_proxy)]
    let location = window_proxy::location_url(owner);
    #[cfg(not(turbo_surf_native_proxy))]
    let location = None;
    let url = location
        .or_else(|| {
            owner
                .get_slot::<FrameDocumentLocation>()
                .map(|url| url.0.clone())
        })
        .unwrap_or_else(|| state.borrow::<Base>().0.clone());
    if command == "set" && key.length() + value.length() > QUOTA_UNITS {
        return Err(JsErrorBox::range_error("Storage quota exceeded"));
    }
    // An oversized key cannot have been stored. Avoid materializing a guest rope
    // that cannot possibly match an entry or pass the write quota.
    if key.length() > QUOTA_UNITS && matches!(command, "get" | "delete") {
        let backing = state.borrow::<BrowserStorage>().clone();
        let valid = backing
            .0
            .lock()
            .map_err(|_| JsErrorBox::generic("Storage backing unavailable"))?
            .values
            .contains_key(&id);
        if !valid {
            return Err(JsErrorBox::type_error("Storage area unavailable"));
        }
        return Ok(v8::null(scope).into());
    }
    let recipients = storage_events::recipients(scope, state, Some(owner), id);
    let key = crate::browser_env::string_code_units(scope, key);
    let value = crate::browser_env::string_code_units(scope, value);
    let (reply, wake) = {
        let backing = state.borrow::<BrowserStorage>().clone();
        let mut areas = backing
            .0
            .lock()
            .map_err(|_| JsErrorBox::generic("Storage backing unavailable"))?;
        let StorageAreas {
            values,
            subscribers,
            ..
        } = &mut *areas;
        let values = values
            .get_mut(&id)
            .ok_or_else(|| JsErrorBox::type_error("Storage area unavailable"))?;
        let mut edit = None;
        let reply = match command {
            "get" => values
                .iter()
                .find(|(name, _)| *name == key)
                .map_or(StorageReply::Null, |(_, value)| {
                    StorageReply::Value(value.clone())
                }),
            "set" => {
                let other_units: usize = values
                    .iter()
                    .filter(|(name, _)| *name != key)
                    .map(|(name, value)| name.len() + value.len())
                    .sum();
                if other_units + key.len() + value.len() > QUOTA_UNITS {
                    return Err(JsErrorBox::range_error("Storage quota exceeded"));
                }
                let index = values.iter().position(|(name, _)| *name == key);
                if index.is_none_or(|index| values[index].1 != value) {
                    let mutation = storage_events::Mutation {
                        key: Some(key.clone()),
                        old: index.map(|index| values[index].1.clone()),
                        new: Some(value.clone()),
                        url,
                    };
                    edit = Some((StorageEdit::Set(index, key, value), mutation));
                }
                StorageReply::Null
            }
            "delete" => {
                if let Some(index) = values.iter().position(|(name, _)| *name == key) {
                    edit = Some((
                        StorageEdit::Remove(index),
                        storage_events::Mutation {
                            key: Some(key),
                            old: Some(values[index].1.clone()),
                            new: None,
                            url,
                        },
                    ));
                }
                StorageReply::Null
            }
            "clear" => {
                if !values.is_empty() {
                    edit = Some((
                        StorageEdit::Clear,
                        storage_events::Mutation {
                            key: None,
                            old: None,
                            new: None,
                            url,
                        },
                    ));
                }
                StorageReply::Null
            }
            "keys" => StorageReply::Keys(values.iter().map(|(name, _)| name.clone()).collect()),
            _ => return Err(JsErrorBox::type_error("Unknown storage operation")),
        };
        let wake = if let Some((edit, mutation)) = edit {
            storage_events::commit(state, recipients, subscribers, id, mutation, || {
                edit.apply(values)
            })?
        } else {
            Vec::new()
        };
        (reply, wake)
    };
    for waiter in wake {
        waiter.wake();
    }
    // V8 allocation may collect a credentialless realm whose native partition
    // destructor reclaims this storage; no backing lock can survive that call.
    Ok(match reply {
        StorageReply::Null => v8::null(scope).into(),
        StorageReply::Value(units) => storage_string(scope, &units),
        StorageReply::Keys(keys) => {
            let array = v8::Array::new(scope, keys.len() as i32);
            for (index, key) in keys.iter().enumerate() {
                let key = storage_string(scope, key);
                array.set_index(scope, index as u32, key);
            }
            array.into()
        }
    })
}

#[op2(fast)]
fn op_storage_bind(scope: &mut v8::PinScope, object: v8::Local<v8::Object>, id: u32) {
    let name = v8::String::new(scope, "turbo-surf.Storage.area").unwrap();
    let key = v8::Private::for_api(scope, Some(name));
    let value = v8::Integer::new_from_unsigned(scope, id);
    object.set_private(scope, key, value.into());
}

#[op2(reentrant)]
fn op_interface_record_get<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
) -> v8::Local<'s, v8::Value> {
    crate::browser_env::get_interface_record(scope, object)
        .unwrap_or_else(|| v8::undefined(scope).into())
}

#[op2(fast, reentrant)]
fn op_interface_record_set<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    record: v8::Local<'s, v8::Value>,
) -> bool {
    crate::browser_env::set_interface_record(scope, object, record)
}

#[op2(fast)]
fn op_storage_id(
    scope: &mut v8::PinScope,
    object: v8::Local<v8::Object>,
) -> Result<u32, JsErrorBox> {
    storage_id(scope, object)
}
fn storage_id(scope: &mut v8::PinScope, object: v8::Local<v8::Object>) -> Result<u32, JsErrorBox> {
    let name = v8::String::new(scope, "turbo-surf.Storage.area").unwrap();
    let key = v8::Private::for_api(scope, Some(name));
    object
        .get_private(scope, key)
        .filter(|value| value.is_uint32())
        .and_then(|value| value.uint32_value(scope))
        .filter(|id| *id != 0)
        .ok_or_else(|| JsErrorBox::type_error("Illegal invocation"))
}

struct RuntimeCore(v8::Weak<v8::Object>);

#[op2(fast)]
fn op_capture_runtime_core(scope: &mut v8::PinScope, core: v8::Local<v8::Object>) {
    let context = scope.get_current_context();
    let Some(owned_core) = crate::context_ownership::object(scope, core) else {
        return;
    };
    context.set_slot(Rc::new(RuntimeCore(owned_core)));
    let core_value: v8::Local<v8::Value> = core.into();
    JsRuntime::op_state_from(scope)
        .borrow_mut()
        .borrow_mut::<FrameContexts>()
        .core = Some(v8::Global::new(scope, core_value));
}

fn install_environment<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    core: v8::Local<'s, v8::Value>,
) -> Result<(), String> {
    // A navigation can be interrupted while trusted bootstrap is compiling. The
    // retained WindowProxy must never expose bootstrap-only host capabilities.
    v8::tc_scope!(let scope, scope);
    let web_source = v8::String::new(scope, crate::web_streams_backend::SOURCE).unwrap();
    let web_factory = v8::Script::compile(scope, web_source, None)
        .and_then(|script| script.run(scope))
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
        .ok_or("Native stream initialization failed")?;
    let code = v8::String::new(scope, ENV_BOOTSTRAP).unwrap();
    let function = v8::Script::compile(scope, code, None)
        .and_then(|script| script.run(scope))
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok());
    if let Some(function) = function {
        let receiver = v8::undefined(scope);
        if function
            .call(scope, receiver.into(), &[core, web_factory.into()])
            .is_some()
        {
            crate::browser_env::capture_popover_scheduler(scope);
            crate::browser_env::capture_window_event_interfaces(scope);
            scope
                .get_current_context()
                .set_slot(Rc::new(crate::browser_env::PointerLockPolicy {
                    activation: host_user_activation,
                    sandboxed: pointer_lock_sandboxed,
                }));
            crate::browser_env::capture_file_host(scope);
            #[cfg(turbo_surf_native_proxy)]
            install_code_generation_authorization(scope)?;
            return Ok(());
        }
    }
    Err(scope
        .exception()
        .map(|error| error.to_rust_string_lossy(scope))
        .unwrap_or_else(|| "Native environment initialization failed".into()))
}

/// The realm's sandboxed pointer lock flag, read when a lock request runs.
fn pointer_lock_sandboxed(context: v8::Local<v8::Context>) -> bool {
    context
        .get_slot::<FrameSandbox>()
        .is_some_and(|flags| flags.pointer_lock_blocked)
}

pub(crate) fn native_pointer_media_capability(context: v8::Local<v8::Context>) -> bool {
    context.get_slot::<HostInputDriver>().is_some()
}

pub(crate) fn native_frame_layout_host<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) -> Option<v8::Local<'s, v8::Object>> {
    if context.get_slot::<FrameRetired>().is_some() {
        return None;
    }
    context.get_slot::<FrameHost>()?.0.to_local(scope)
}

pub(crate) fn native_frame_is_retired(context: v8::Local<v8::Context>) -> bool {
    context.get_slot::<FrameRetired>().is_some()
}

#[op2(fast)]
fn op_window_media_state(scope: &mut v8::PinScope, window: v8::Local<v8::Object>) -> u32 {
    let Some(context) = window.get_creation_context(scope) else {
        return 0;
    };
    if context.get_slot::<FrameDirectRetirement>().is_some() {
        0
    } else if context.get_slot::<FrameRetired>().is_some() {
        1
    } else {
        2
    }
}

#[op2(nofast)]
fn op_script_navigation_admitted(scope: &mut v8::PinScope, #[string] target: String) {
    script_navigation::record(
        scope.get_entered_or_microtask_context(),
        scope.get_current_context(),
        target,
    );
}

struct HostInputDriver {
    input: v8::Weak<v8::Function>,
    cancel: v8::Weak<v8::Function>,
}

#[op2(fast)]
fn op_set_host_input_driver(
    scope: &mut v8::PinScope,
    callback: v8::Local<v8::Function>,
    cancel: v8::Local<v8::Function>,
) {
    let context = callback
        .get_creation_context(scope)
        .expect("host input callback realm");
    let Some(input) = crate::context_ownership::function(scope, callback) else {
        return;
    };
    let Some(cancel) = crate::context_ownership::function(scope, cancel) else {
        return;
    };
    context.set_slot(Rc::new(HostInputDriver { input, cancel }));
}

#[op2(nofast, reentrant)]
fn op_event_trust(scope: &mut v8::PinScope, event: v8::Local<v8::Object>, timestamp: f64) {
    crate::browser_env::mark_event_trusted(scope, event, (timestamp >= 0.0).then_some(timestamp));
}

/// The host input driver's pointer capture / lock steps on the binding's native state.
#[op2]
fn op_host_pointer<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    #[string] command: String,
) -> v8::Local<'s, v8::Value> {
    crate::browser_env::host_pointer(scope, document, &command)
}

#[op2(nofast, reentrant)]
fn op_dispatch_host_event(
    scope: &mut v8::PinScope,
    event: v8::Local<v8::Object>,
    target: v8::Local<v8::Object>,
) {
    crate::browser_env::dispatch_host_event(scope, event, target);
}

#[op2(nofast, reentrant)]
fn op_event_callback(
    scope: &mut v8::PinScope,
    event: v8::Local<v8::Object>,
    target: v8::Local<v8::Object>,
    active: bool,
) {
    crate::browser_env::event_callback_state(scope, event, target, active);
}

#[op2]
fn op_message_source<'s>(scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Object> {
    let context = scope.get_entered_or_microtask_context();
    context.global(scope)
}

deno_core::extension!(
    turbo_dom,
    ops = [
        op_create_frame,
        op_frame_host_helper,
        op_windows_same_origin,
        op_window_parent,
        op_frame_owner_element,
        op_window_top,
        op_window_post_message,
        op_frame_set_document,
        op_retire_frame,
        op_window_active,
        op_window_media_state,
        op_window_message_pending,
        op_native_window_proxy,
        op_native_location_installed,
        op_location_object,
        op_location_seed,
        op_location_ancestor_origins,
        op_window_opaque,
        op_document_scripts_allowed,
        op_set_frame_navigator,
        op_window_child_frames,
        op_frame_move_candidates,
        op_frame_finish,
        op_frame_dom_ready,
        op_frame_parser_done,
        op_frame_scripts,
        op_frame_resources,
        op_timer_clock,
        op_timer_offset,
        op_window_timers_realtime,
        op_timer_advance,
        op_timer_nesting,
        op_timer_set_nesting,
        op_run_local_timers,
        op_run_frame_timers,
        op_frame_pending,
        op_queue_frame_script,
        op_frame_module,
        op_message_source,
        op_event_trust,
        op_set_host_input_driver,
        op_host_user_activation,
        op_host_close_request,
        op_host_light_dismiss,
        op_host_modal_backdrop,
        op_user_activation_snapshot,
        op_user_activation_install,
        op_user_activation_navigator,
        op_user_activation_read,
        op_script_navigation_admitted,
        op_capture_runtime_core,
        op_set_host_capabilities,
        op_form_data_entries,
        op_form_data_finish,
        op_form_data_event,
        op_storage_area,
        op_storage_factory,
        op_storage_object,
        op_storage_access_factory,
        op_storage_access_handle,
        op_storage_access_state,
        op_storage_call,
        storage_events::op_storage_events_factory,
        storage_events::op_storage_events,
        storage_events::op_storage_event_create,
        op_storage_bind,
        op_interface_record_get,
        op_interface_record_set,
        op_storage_id,
        op_dispatch_host_event,
        op_host_pointer,
        crate::stream_transfers::op_stream_transfer_install,
        crate::web_streams_backend::op_web_abort_bind,
        crate::web_streams_backend::op_web_abort_listener,
        crate::web_streams_backend::op_web_abort_dispatch,
        op_event_callback,
        op_install_microtask_queue,
        op_report_exception,
        op_value_is_promise,
        op_frame_cookie_get,
        op_document_location_url,
        op_frame_request_partition,
        op_document_request_url,
        op_document_referrer,
        op_frame_referrer_policy,
        op_document_request_source,
        op_frame_owner_metadata,
        op_frame_owner_window,
        op_document_cookie_site,
        op_set_document_runtime_state_accessor,
        op_document_runtime_state,
        op_document_has_active_window,
        op_document_cookie_allowed,
        op_parse_html_document,
        op_parser_scripts,
        op_trusted_type_string,
        op_timer_code_allowed,
        op_run_timer_code,
        op_set_inert_document_initializer,
        op_document_create_element,
        op_document_import_node,
        op_frame_cookie_set,
        op_queue_classic_script,
        op_install_resource_consumer,
        op_prepare_resource_script,
        op_retire_resource_document,
        op_execute_inline_script,
        op_cookie_get,
        op_cookie_set,
        fetch_streams::op_fetch_prepare,
        fetch_streams::op_fetch_upload_start,
        fetch_streams::op_fetch_upload_write,
        fetch_streams::op_fetch_upload_finish,
        fetch_streams::op_fetch_watch_body,
        fetch_streams::op_fetch_cancel,
        fetch_streams::op_fetch_abort,
        fetch_streams::op_fetch_release,
        fetch_streams::op_fetch_read,
        op_fetch,
        op_load_image,
        op_dictionary_active,
        op_dictionary_headers,
        op_dictionary_links,
        op_dictionary_prepare,
        op_user_agent,
        op_fingerprint,
        op_browser_brands,
        op_browser_device_memory,
        op_is_secure_context,
        op_shared_memory_allowed,
        op_now_perf,
        op_now_epoch,
        op_inline_module,
        op_module_credentials,
        op_csp_context,
        op_csp_base_url,
        op_permissions_policy,
        op_document_feature_enabled,
        op_private_token_issuer,
        op_private_token_query,
        op_origin_agent_cluster,
        op_document_origin_opaque,
        op_iframe_permissions_policy,
        op_csp_meta,
        op_referrer_meta,
        op_csp_finish_parser,
        op_csp_inline,
        op_csp_request,
        op_frame_script_allowed,
        op_frame_script_event,
        op_csp_script,
        op_csp_nonce,
        op_module_staging_enabled,
        op_heap_statistics,
        op_text_decoder_new,
        op_text_decode,
        op_text_decoder_drop,
        op_text_encode,
        op_random_fill,
        op_surf_url_parse,
        op_url_update,
        op_url_params_parse,
        op_url_params_serialize,
        op_measure_text,
        op_raster_png,
        op_raster_rgba,
        op_webgl_metadata,
        op_webgl_readback,
        op_webgl_available
    ],
);

// Non-DOM browser globals, layered over the ops AFTER the native DOM binding is
// installed (`browser_env` owns document/Element/window/navigator/Event/etc.; this
// adds what a network-free test env lacks and overrides a few brand/host values).
// Window timers are queued here and selected one task at a time by the host,
// with microtask checkpoints between callbacks. Virtual mode advances deadlines;
// real-time mode leaves future timers queued. `fetch`/XHR use the tier-1 net stack.
//
// Wrapped in an IIFE so it is RE-RUNNABLE on a reused isolate: a persistent
// runtime (see `run_with_dom`) re-installs the page per call, which re-runs this;
// top-level `const`/`let` would throw "already declared" the second time, but
// inside the IIFE they're per-invocation. Globals are assigned to `globalThis`
// (idempotent) and the cookie bridge re-applies to the current `document`.
const ENV_BOOTSTRAP: &str = concat!(
    r##"((__core, __webFactory) => {
"use strict";
// Core is a private embedder argument; no page global temporarily exposes ops.
const ops = __core.ops;
const __print = __core.print;
let __web;
ops.op_capture_runtime_core(__core);
if(!ops.op_shared_memory_allowed())delete globalThis.SharedArrayBuffer;
const __NS = Symbol.for("ts");
const __G = globalThis[__NS] || (Object.defineProperty(globalThis, __NS, { value: {}, configurable: true, enumerable: false, writable: false }), globalThis[__NS]);
const __hostEvent=globalThis.Event;
let __hostErrorEvent;
let __markDocumentInteractive;
// Interface records use a native, isolate-owned ephemeron map. Same-origin
// borrowed methods accept foreign-realm objects without exposing a brand setter.
const __stringify = JSON.stringify, __String = String;
const __framePromiseAll = Promise.all.bind(Promise);
const __recordOwn = Object.hasOwn, __recordCreate = Object.create, __recordDefine = Object.defineProperty;
const __interfaceMap = brand => ({
  has(receiver) { return this.get(receiver) !== undefined; },
  get(receiver) {
    if (receiver===null || (typeof receiver!=='object'&&typeof receiver!=='function')) return undefined;
    const record=ops.op_interface_record_get(receiver);
    return record && __recordOwn(record,'runtime') ? record.runtime[brand] : undefined;
  },
  set(receiver,value) {
    let record=ops.op_interface_record_get(receiver);
    if (!record) {record=__recordCreate(null);record.kind='runtime';if(!ops.op_interface_record_set(receiver,record))throw new TypeError('Interface state unavailable');}
    if (!__recordOwn(record,'runtime')) __recordDefine(record,'runtime',{__proto__:null,value:__recordCreate(null)});
    record.runtime[brand]=value;return this;
  },
});
globalThis.self = globalThis;
// DOMException carries branded Web-IDL accessors, including the names/codes used
// by origin checks and clone failures. The upstream interface shell has no state.
const __domExceptionState = __interfaceMap("DOMException");
const __domExceptionCodes = {IndexSizeError:1, HierarchyRequestError:3, WrongDocumentError:4,
  InvalidCharacterError:5, NoModificationAllowedError:7, NotFoundError:8, NotSupportedError:9,
  InUseAttributeError:10, InvalidStateError:11, SyntaxError:12, InvalidModificationError:13,
  NamespaceError:14, InvalidAccessError:15, TypeMismatchError:17, SecurityError:18,
  NetworkError:19, AbortError:20, URLMismatchError:21, QuotaExceededError:22,
  TimeoutError:23, InvalidNodeTypeError:24, DataCloneError:25};
const __exceptionStringConstructor = String, __exceptionTypeError = TypeError;
const __exceptionState = (exception) => {
  const state = __domExceptionState.get(exception);
  if (!state) throw new __exceptionTypeError("Illegal invocation");
  return state;
};
const __exceptionString = value => {
  if (typeof value === "symbol") throw new __exceptionTypeError("Cannot convert a Symbol to a string");
  return __exceptionStringConstructor(value);
};
globalThis.DOMException = class DOMException {
  constructor(message = "", name = "Error") {
    const text = __exceptionString(message), kind = __exceptionString(name);
    __domExceptionState.set(this, {message: text, name: kind});
  }
  get name() { return __exceptionState(this).name; }
  get message() { return __exceptionState(this).message; }
  get code() { return __domExceptionCodes[__exceptionState(this).name] || 0; }
};
Object.setPrototypeOf(DOMException.prototype, Error.prototype);
for (const name of ["name", "message", "code"]) {
  const descriptor = Object.getOwnPropertyDescriptor(DOMException.prototype, name);
  Object.defineProperty(DOMException.prototype, name, {...descriptor, enumerable: true});
}
Object.defineProperty(DOMException.prototype, Symbol.toStringTag, {value: "DOMException", configurable: true});
ops.op_register_dom_exception(DOMException);
for (const [name, code] of Object.entries({INDEX_SIZE_ERR:1, DOMSTRING_SIZE_ERR:2,
  HIERARCHY_REQUEST_ERR:3, WRONG_DOCUMENT_ERR:4, INVALID_CHARACTER_ERR:5, NO_DATA_ALLOWED_ERR:6,
  NO_MODIFICATION_ALLOWED_ERR:7, NOT_FOUND_ERR:8, NOT_SUPPORTED_ERR:9, INUSE_ATTRIBUTE_ERR:10,
  INVALID_STATE_ERR:11, SYNTAX_ERR:12, INVALID_MODIFICATION_ERR:13, NAMESPACE_ERR:14,
  INVALID_ACCESS_ERR:15, VALIDATION_ERR:16, TYPE_MISMATCH_ERR:17, SECURITY_ERR:18,
  NETWORK_ERR:19, ABORT_ERR:20, URL_MISMATCH_ERR:21, QUOTA_EXCEEDED_ERR:22, TIMEOUT_ERR:23,
  INVALID_NODE_TYPE_ERR:24, DATA_CLONE_ERR:25})) {
  // Configurable until the binding's key-order step moves and freezes them.
  for (const object of [DOMException, DOMException.prototype]) Object.defineProperty(object, name, {value: code, enumerable: true, configurable: true});
}

// Deno's error builders live for the whole isolate. Register them in the main
// realm before frame bootstrap can install crypto, so an initial iframe never
// becomes a permanent constructor/context root in the shared native core.
if (!__G.isFrame) {
  const NativeDOMException = globalThis.DOMException;
  for (const name of [
    "DataCloneError",
    "DataError",
    "InvalidAccessError",
    "NotSupportedError",
    "OperationError",
    "SyntaxError",
    "QuotaExceededError",
    "TypeMismatchError",
  ]) {
    const nativeName = "DOMException" + name;
    if (__core.buildCustomError(nativeName, "") === undefined)
      __core.registerErrorBuilder(nativeName, (message) => new NativeDOMException(message, name));
  }
}

// Present a real Chrome (macOS) navigator so page JS that profiles the browser
// (consistency-only anti-bot gates, feature detection) sees Chrome, not the old
// `turbo-surf`/`turbo-test` tell. Kept in sync with the tier-1 HTTP UA in
// turbo-surf-core (fingerprint::default_profile): same Chrome major + macOS, so navigator
// and the request headers agree (a UA/platform mismatch is itself a bot signal).
// This is no-Chromium env emulation — it satisfies passive/consistency probes,
// not active canvas/WebGL/audio draw-and-hash or PoW challenges.
// `onLine: true` matters: auth SDKs (PropelAuth) only auto-refresh the session from the
// cookie when the browser reports online — an undefined/falsy onLine made a cold load of
// an authed page skip the refresh and render nothing.
// Runtime fingerprint overrides (JSON object from op_fingerprint). Every navigator
// field below has a current configured Chrome release default and is overridable by the matching key —
// settable per process via `set_fingerprint` (MCP `set_fingerprint` tool).
const __fp = (() => { try { return JSON.parse(ops.op_fingerprint()); } catch (e) { return {}; } })();
const __pick = (k, d) => (__fp[k] !== undefined ? __fp[k] : d);
const __ua = __pick("userAgent",
  (ops.op_user_agent && ops.op_user_agent()) || __fp.defaultUserAgent || "");
// Derive the major from the ACTUAL UA string so navigator.userAgentData.brands always
// agree with navigator.userAgent (a UA-string `Chrome/153` with `brands` v154 is a hard
// tell). An explicit chromeMajor override still wins; else parse `Chrome/<major>` from
// __ua, falling back to the registry default major from op_fingerprint.
const __major = String(__pick("chromeMajor", ((__ua.match(/Chrome\/(\d+)/) || [])[1]) || __fp.defaultMajor));
// Full build for high-entropy UA-CH (fullVersionList / uaFullVersion) — real Chrome
// reports the FULL version here (e.g. 154.0.8037.98), NOT the reduced <major>.0.0.0
// the UA string carries. op_fingerprint always supplies the registry default release's
// full version (the fallback is unreachable); a caller rotating to a non-default major should pass fullVersion too (the
// session pushes it from the wire profile so both layers stay coherent).
const __fullVersion = String(__pick("fullVersion", `${__major}.0.0.0`));
const __brands=JSON.parse(ops.op_browser_brands(Number(__major), __major, false));
const __fullBrands=JSON.parse(ops.op_browser_brands(Number(__major), __fullVersion, true));
// Chrome ships exactly these five PDF-viewer plugins, all aliased to the internal
// viewer; `navigator.plugins.length === 0` is a classic headless giveaway.
const __plugin = (name) => ({ name, filename: "internal-pdf-viewer", description: "Portable Document Format", length: 1 });
const __plugins = ["PDF Viewer", "Chrome PDF Viewer", "Chromium PDF Viewer", "Microsoft Edge PDF Viewer", "WebKit built-in PDF"].map(__plugin);
// Chrome's two PDF MIME types, both bound to the internal viewer. `navigator.mimeTypes`
// empty while `plugins` is populated is an INCOHERENCE tell (real Chrome: plugins imply
// their mimeTypes) — google's homepage reads mimeTypes.length. enabledPlugin cross-links
// back to a plugin, as in a real MimeType.
const __mimeTypes = [
  { type: "application/pdf", suffixes: "pdf", description: "Portable Document Format", enabledPlugin: __plugins[0] },
  { type: "text/pdf", suffixes: "pdf", description: "Portable Document Format", enabledPlugin: __plugins[0] },
];
const __langs = __pick("languages", ["en-US", "en"]);
const __platform = __pick("platform", "MacIntel");
const __uaPlatform = __pick("uaPlatform", "macOS");
// Mac wide-gamut/HDR displays report 30-bit color; other platforms 24. Param-driven with a
// platform-aware default so a caller can override, and a Windows/Linux profile stays honest.
const __isMac = __uaPlatform === "macOS" || /mac/i.test(String(__platform));
const __colorDepth = __pick("colorDepth", __isMac ? 30 : 24);
const __pixelDepth = __pick("pixelDepth", __colorDepth);
// The navigator's data + method IMPLEMENTATIONS. Assembled here, then hung off a real
// `Navigator.prototype` (below) so `navigator` is a prototype-backed instance with ZERO own
// properties — exactly like Chrome (a plain object literal had 26 own props + a 15-member proto,
// vs Chrome's 0 own + 84-member Navigator.prototype: a structural tell).
const __navData = {
  userAgent: __ua,
  appVersion: __ua.replace(/^Mozilla\//, ""),
  appName: "Netscape", appCodeName: "Mozilla", product: "Gecko", productSub: "20030107",
  platform: __platform, vendor: __pick("vendor", "Google Inc."), vendorSub: "",
  language: __langs[0] || "en-US", languages: __langs, onLine: true,
  // Automation tell: real Chrome exposes this as `false`, never `true`/undefined.
  webdriver: false,
  hardwareConcurrency: __pick("hardwareConcurrency", 8),
  deviceMemory: __pick("deviceMemory", ops.op_browser_device_memory(Number(__major))),
  maxTouchPoints: __pick("maxTouchPoints", 0),
  cookieEnabled: true, doNotTrack: null,
  // Fire-and-forget telemetry beacon: real Chrome exposes it, and its absence is a
  // headless tell (google's homepage reads `navigator.sendBeacon` before hydrating).
  // Anti-bot collectors (Botguard/reCAPTCHA-class) and analytics POST their COLLECTED
  // TOKEN back over sendBeacon — so a no-op that merely returns `true` fingerprints
  // fine yet never completes the exchange (the token is dropped on the floor). Actually
  // send it: a fire-and-forget POST over the tier-1 net stack (`globalThis.fetch` →
  // `op_fetch`), which shares the page cookie jar and is counted in `__pendingFetches`
  // so the hydration drain waits for it to land before serializing. Returns `true`
  // synchronously, per the spec (the boolean reports whether the UA QUEUED the transfer,
  // not whether it succeeded). `globalThis.fetch`/Blob/etc. are defined later in this
  // bootstrap, but this runs only at call time, by when they exist.
  sendBeacon: (url, data = undefined) => {
    try {
      if (url == null || url === "") return false;
      // Normalize the payload + its implicit content-type the way the spec's
      // `extractBody` does for the common beacon body types.
      let body = null;
      let contentType = "";
      if (data != null) {
        const B = globalThis.Blob, USP = globalThis.URLSearchParams, FD = globalThis.FormData;
        if (typeof data === "string") { body = data; contentType = "text/plain;charset=UTF-8"; }
        else if (B && data instanceof B) { body = data._s != null ? String(data._s) : ""; contentType = data.type || ""; }
        else if (USP && data instanceof USP) { body = data.toString(); contentType = "application/x-www-form-urlencoded;charset=UTF-8"; }
        else if (FD && data instanceof FD) {
          // No multipart encoder headless; serialize the fields url-encoded (Blob/File
          // parts become their string content) — enough for a token round-trip.
          const p = new USP();
          data.forEach((v, k) => p.append(k, typeof v === "string" ? v : (v && v._s != null ? String(v._s) : "")));
          body = p.toString(); contentType = "application/x-www-form-urlencoded;charset=UTF-8";
        }
        else if (data instanceof ArrayBuffer || (data && data.buffer instanceof ArrayBuffer)) {
          try { body = new globalThis.TextDecoder().decode(data instanceof ArrayBuffer ? new Uint8Array(data) : data); }
          catch (_e) { body = ""; }
        }
        else { try { body = String(data); } catch (_e) { body = ""; } }
      }
      const headers = contentType ? { "content-type": contentType } : {};
      // Direct (not deferred) call so `fetch` bumps `__pendingFetches` synchronously and
      // the drain can't quiesce before the beacon completes. `keepalive` mirrors the real
      // beacon flag (informational here).
      try { globalThis.fetch(String(url), { method: "POST", body, headers, keepalive: true, mode: "no-cors", credentials: "include" }); } catch (_e) {}
      return true;
    } catch (_e) { return true; }
  },
  plugins: __plugins, mimeTypes: __mimeTypes,
  // Real Chrome exposes navigator.pdfViewerEnabled === true (it ships the internal PDF
  // viewer); its absence is a headless tell google's homepage reads.
  pdfViewerEnabled: __pick("pdfViewerEnabled", true),
  // NetworkInformation — real Chrome exposes it; anti-bot scripts (found via the
  // `probe` example on a real Akamai sensor) read it, and its absence is a tell.
  connection: __pick("connection", { effectiveType: "4g", rtt: 50, downlink: 10, saveData: false }),
  // UA-Client-Hints high-entropy surface, consistent with the UA above.
  userAgentData: __pick("userAgentData", {
    // Rust owns the same major-seeded GREASE generation used by HTTP headers.
    brands: __brands,
    mobile: false,
    platform: __uaPlatform,
    getHighEntropyValues: async (hints) => {
      const values = {
      architecture: __pick("architecture", "arm"), bitness: "64", model: "", wow64: false,
      platform: __uaPlatform, platformVersion: __pick("platformVersion", "15.0.0"), uaFullVersion: __fullVersion,
      mobile: false,
      brands: __brands,
      fullVersionList: __fullBrands,
      formFactors: ["Desktop"],
      };
      const result = {brands:values.brands, mobile:values.mobile, platform:values.platform};
      for (const key of hints) if (Object.prototype.hasOwnProperty.call(values, key)) result[key] = values[key];
      return result;
    },
  }),
  // In-memory clipboard: an app that writeText()s a value (e.g. a copy-link button)
  // and reads it back round-trips, with no OS clipboard.
  clipboard: (() => {
    let v = "";
    const ErrorType = globalThis.DOMException;
    const check = name => {if (!ops.op_permissions_policy(__document,name,"",2)) throw new ErrorType("Clipboard access is disabled by Permissions Policy","NotAllowedError");};
    return {writeText:async t => {check("clipboard-write");v=String(t == null ? "" : t);},readText:async () => {check("clipboard-read");return v;}};
  })(),
  // Descriptor conversion rejects through the returned Promise, including
  // getters and enum failures, as measured in the normal Chrome control.
  permissions: (() => {
    const defaults = { __proto__:null, geolocation:"prompt", notifications:"prompt", push:"prompt",
      "persistent-storage":"prompt", "background-sync":"granted", midi:"prompt",
      camera:"prompt", microphone:"prompt", "clipboard-read":"prompt", "clipboard-write":"granted",
      accelerometer:"granted", gyroscope:"granted", magnetometer:"granted", "background-fetch":"granted",
      "captured-surface-control":"prompt", "display-capture":"prompt", "idle-detection":"prompt",
      "local-fonts":"prompt", "payment-handler":"granted", "periodic-background-sync":"denied",
      "screen-wake-lock":"granted", "storage-access":"granted", "window-management":"prompt",
      "keyboard-lock":"granted", "pointer-lock":"granted", "local-network-access":"prompt", "loopback-network":"prompt" };
    const subtypes = {__proto__:null,push:true,midi:true,camera:true,"clipboard-read":true,"clipboard-write":true};
    const has = Object.prototype.hasOwnProperty, apply=Reflect.apply, string=String,
      ErrorType=TypeError, Exception=DOMException;
    return {
      query: async (descriptor) => {
        if (descriptor === null || descriptor === undefined ||
          (typeof descriptor !== "object" && typeof descriptor !== "function"))
          throw new ErrorType("Permission descriptor must be a dictionary");
        const rawName=descriptor.name;
        if(rawName===undefined || typeof rawName==="symbol")throw new ErrorType("Permission name is required");
        const name=string(rawName);
        if(!apply(has,defaults,[name]))throw new ErrorType("Invalid PermissionName");
        if(apply(has,subtypes,[name])) {
          const repeated=descriptor.name;
          if(repeated===undefined || typeof repeated==="symbol" || !apply(has,defaults,[string(repeated)]))
            throw new ErrorType("Invalid PermissionName");
        }
        if(name==="push" && !descriptor.userVisibleOnly)throw new Exception("Push requires userVisibleOnly", "NotSupportedError");
        // Reading subtype members runs the original dictionary conversion even
        // where this native engine supplies only the measured permission state.
        if(name==="midi")void !!descriptor.sysex;
        if(name==="camera")void !!descriptor.panTiltZoom;
        if(name==="clipboard-read" || name==="clipboard-write") {
          void !!descriptor.allowWithoutGesture; void !!descriptor.allowWithoutSanitization;
        }
        return __createPermissionStatus(name,defaults[name]);
      },
    };
  })(),
};
// Build a real `Navigator` interface: `window.Navigator` + a prototype holding all 84 members
// Chrome exposes (getters for data props, functions for methods), then a zero-own-property
// instance. Members we implement use __navData; the rest get plausible stubs so presence/`in`/
// typeof checks + the member count match Chrome. Getters/methods are arrows (no own `prototype`)
// and get native-masked in the toString IIFE. The exact 84-name list is from a live Chrome 154.
(() => {
  const GET = ("appCodeName appName appVersion bluetooth clipboard connection cookieEnabled cpuPerformance " +
    "credentials deprecatedRunAdAuctionEnforcesKAnonymity deviceMemory devicePosture doNotTrack geolocation gpu " +
    "hardwareConcurrency hid ink keyboard language languages locks login managed maxTouchPoints mediaCapabilities " +
    "mediaDevices mediaSession mimeTypes onLine pdfViewerEnabled permissions platform plugins presentation product " +
    "productSub protectedAudience scheduling serial serviceWorker storage storageBuckets usb userActivation " +
    "userAgent userAgentData vendor vendorSub virtualKeyboard wakeLock webdriver webkitPersistentStorage " +
    "webkitTemporaryStorage windowControlsOverlay xr").split(/\s+/);
  const FN = ("adAuctionComponents canLoadAdAuctionFencedFrame canShare clearAppBadge clearOriginJoinedAdInterestGroups " +
    "createAuctionNonce deprecatedReplaceInURN deprecatedURNToURL getBattery getGamepads getInstalledRelatedApps " +
    "getInterestGroupAdAuctionData getUserMedia javaEnabled joinAdInterestGroup leaveAdInterestGroup " +
    "registerProtocolHandler requestMIDIAccess requestMediaKeySystemAccess runAdAuction sendBeacon setAppBadge share " +
    "unregisterProtocolHandler updateAdInterestGroups vibrate webkitGetUserMedia").split(/\s+/);
  // Plausible stubs for members we don't back with real data (presence/shape checks pass).
  const P = () => Promise.resolve();
  const REJ = () => Promise.reject(new DOMException("Not supported", "NotSupportedError"));
  const GET_STUB = {
    geolocation: { getCurrentPosition() {}, watchPosition() { return 0; }, clearWatch() {} },
    mediaDevices: { enumerateDevices: () => Promise.resolve([]), getUserMedia: REJ, getSupportedConstraints: () => ({}), addEventListener() {}, removeEventListener() {} },
    serviceWorker: { register: REJ, getRegistration: () => Promise.resolve(undefined), getRegistrations: () => Promise.resolve([]), ready: new Promise(() => {}), controller: null, addEventListener() {}, removeEventListener() {} },
    storage: { estimate: () => Promise.resolve({ quota: 0, usage: 0 }), persisted: () => Promise.resolve(false), persist: () => Promise.resolve(false) },
    credentials: { get: () => Promise.resolve(null), create: () => Promise.resolve(null), store: P, preventSilentAccess: P },
    wakeLock: { request: REJ },
    keyboard: { getLayoutMap: () => Promise.resolve(new Map()), lock: P, unlock() {} },
    locks: { request: P, query: () => Promise.resolve({ held: [], pending: [] }) },
    mediaCapabilities: { decodingInfo: () => Promise.resolve({ supported: true, smooth: true, powerEfficient: true }), encodingInfo: () => Promise.resolve({ supported: true, smooth: true, powerEfficient: true }) },
    mediaSession: { metadata: null, playbackState: "none", setActionHandler() {}, setPositionState() {} },
    // React's scheduler probes navigator.scheduling.isInputPending — must be an object with the fn.
    scheduling: { isInputPending: () => false },
    presentation: { defaultRequest: null, receiver: null },
    webkitTemporaryStorage: { queryUsageAndQuota() {}, requestQuota() {} },
    webkitPersistentStorage: { queryUsageAndQuota() {}, requestQuota() {} },
  };
  const navProto = {};
  const def = (name, desc) => { try { Object.defineProperty(navProto, name, desc); } catch (e) {} };
  for (const name of GET) {
    const has = Object.prototype.hasOwnProperty.call(__navData, name);
    // Default an unlisted interface getter to a fresh {} (not null): real Chrome returns an object
    // for these, and `navigator.X.foo` must read `undefined` rather than throw on null. A fresh
    // object per property (not one shared ref) so `navigator.a !== navigator.b`.
    const val = has ? __navData[name] : (Object.prototype.hasOwnProperty.call(GET_STUB, name) ? GET_STUB[name] : {});
    const getter = Object.getOwnPropertyDescriptor({get [name]() {
      if (this !== navigator) throw new TypeError("Illegal invocation");
      return val;
    }}, name).get;
    def(name, { get: getter, enumerable: true, configurable: true });
  }
  const FN_IMPL = {
    javaEnabled: () => false,
    vibrate: () => true,
    canShare: () => false,
    share: REJ,
    getBattery: () => Promise.resolve({ charging: true, chargingTime: 0, dischargingTime: Infinity, level: 1, addEventListener() {}, removeEventListener() {} }),
    getGamepads: () => [null, null, null, null],
    getInstalledRelatedApps: () => Promise.resolve([]),
    requestMIDIAccess: REJ,
    requestMediaKeySystemAccess: REJ,
    getUserMedia: (_c, _ok, err) => { try { if (err) err(new DOMException("Permission denied", "NotAllowedError")); } catch (e) {} },
    webkitGetUserMedia: (_c, _ok, err) => { try { if (err) err(new DOMException("Permission denied", "NotAllowedError")); } catch (e) {} },
    setAppBadge: P, clearAppBadge: P,
    registerProtocolHandler: () => {}, unregisterProtocolHandler: () => {},
  };
  const FN_LENGTH = { adAuctionComponents: 1, clearOriginJoinedAdInterestGroups: 1, deprecatedReplaceInURN: 2,
    deprecatedURNToURL: 1, getInterestGroupAdAuctionData: 1, getUserMedia: 3, joinAdInterestGroup: 1,
    registerProtocolHandler: 2, requestMediaKeySystemAccess: 2, runAdAuction: 1, sendBeacon: 1,
    unregisterProtocolHandler: 2, vibrate: 1, webkitGetUserMedia: 3 };
  for (const name of FN) {
    const impl = Object.prototype.hasOwnProperty.call(__navData, name) && typeof __navData[name] === "function"
      ? __navData[name]
      : (FN_IMPL[name] || (() => undefined));
    // One distinct function per operation (shared stubs would report each other's names) with
    // Chrome 156's arity and receiver check.
    const operation = ({[name](...args) {
      if (this !== navigator) throw new TypeError("Illegal invocation");
      return Reflect.apply(impl, this, args);
    }})[name];
    Object.defineProperty(operation, "length", { value: FN_LENGTH[name] || 0, configurable: true });
    def(name, { value: operation, writable: true, enumerable: true, configurable: true });
  }
  function Navigator() { throw new TypeError("Illegal constructor"); }
  Navigator.prototype = navProto;
  def("constructor", { value: Navigator, writable: true, configurable: true });
  Object.defineProperty(navProto, Symbol.toStringTag, { value: "Navigator", configurable: true });
  globalThis.Navigator = Navigator;
  const navigator = Object.create(navProto); // zero own properties, like Chrome
  globalThis.navigator = navigator;
})();
// `screen` — overridable as a unit; defaults to a common 1080p desktop.
{
  const __scr = __pick("screen", { width: 1920, height: 1080 });
  const __w = __scr.width || 1920, __h = __scr.height || 1080;
  // availWidth/Height default to the screen minus OS chrome (macOS menubar ≈ 25px tall, no
  // side reservation); each is independently overridable via the `screen` object or __pick.
  const __availW = __pick("availWidth", __scr.availWidth || __w);
  const __availH = __pick("availHeight", __scr.availHeight || (__h - (__isMac ? 25 : 0)));
  globalThis.screen = {
    width: __w, height: __h, availWidth: __availW, availHeight: __availH,
    // Real Chrome exposes availLeft/availTop (0 on a primary, top-left display); their
    // absence (undefined) is a headless tell.
    availLeft: __pick("availLeft", 0), availTop: __pick("availTop", 0),
    colorDepth: __colorDepth, pixelDepth: __pixelDepth,
    // ScreenOrientation is an EventTarget — apps listen for orientation changes;
    // a missing `addEventListener` throws and can trip a component during hydration.
    orientation: {
      type: "landscape-primary", angle: 0,
      addEventListener() {}, removeEventListener() {}, dispatchEvent() { return false; },
      onchange: null,
    },
  };
  globalThis.devicePixelRatio = __pick("devicePixelRatio", 2);
}
// document.fonts (FontFaceSet) — an EventTarget with a `ready` promise; web-font
// loaders (`document.fonts.ready`, `.addEventListener('loadingdone')`) touch it, and
// a missing one throws mid-hydration. No real font pipeline here, so `ready` resolves
// immediately and load/check report success/emptiness.
if (globalThis.document && !globalThis.document.fonts) {
  try {
    const __fonts = {
      ready: Promise.resolve(), status: "loaded", size: 0,
      add() {}, delete() { return false; }, clear() {}, has() { return false; },
      check() { return true; }, load() { return Promise.resolve([]); },
      forEach() {}, values() { return [][Symbol.iterator](); }, keys() { return [][Symbol.iterator](); },
      [Symbol.iterator]() { return [][Symbol.iterator](); },
      addEventListener() {}, removeEventListener() {}, dispatchEvent() { return false; },
      onloading: null, onloadingdone: null, onloadingerror: null,
    };
    Object.defineProperty(globalThis.document, "fonts", { configurable: true, get() { return __fonts; } });
  } catch (e) {}
}
// `window.chrome` presence (with loadTimes/csi/app, but no extension `runtime`) is
// what a plain Chrome page exposes; its absence flags a non-Chrome/headless client.
// Key insertion order matches real Chrome (`Object.keys(chrome)` = loadTimes, csi, app)
// — a reordered set is a checkable tell.
globalThis.chrome = globalThis.chrome || {
  loadTimes: function () { return {}; },
  csi: function () { return {}; },
  app: { isInstalled: false },
};
globalThis.location = globalThis.location || { href: "about:blank", protocol: "about:", host: "", pathname: "blank" };
// Storage areas live in the host and are keyed by the actual Window's origin.
// Private IDs remain attached to retained Storage objects across navigation.
const __storageApply=Reflect.apply,__storageCharCode=String.prototype.charCodeAt,__storageGetters=__recordCreate(null);
const __storageWindows = new WeakMap(),__storageWindowGet=WeakMap.prototype.get.bind(__storageWindows),__storageWindowSet=WeakMap.prototype.set.bind(__storageWindows),__storageNumber=Number;
const __storageMap = receiver => ops.op_storage_id(receiver);
const __storageString = value => {if(typeof value==='symbol')throw new __exceptionTypeError('Cannot convert a Symbol to a string');return __String(value);};
const __storageRangeError=RangeError,__storageException=DOMException;
const __storageNativeCall=(id,receiver,command,key='',value='')=>{
 try{return ops.op_storage_call(id,receiver,command,key,value);}catch(error){
  if(error instanceof __storageRangeError&&error.message==='Storage quota exceeded')throw new __storageException(error.message,'QuotaExceededError');
  throw error;
 }
};
const __storageCall = (receiver, command, key='', value='') => __storageNativeCall(__storageMap(receiver),receiver,command,key,value);
function Storage() {throw new __exceptionTypeError('Illegal constructor');}
const __storageMethods = {
 getItem(key){if(!arguments.length)throw new __exceptionTypeError('A key is required');return __storageCall(this,'get',__storageString(key));},
 setItem(key,value){if(arguments.length<2)throw new __exceptionTypeError('A key and value are required');__storageCall(this,'set',__storageString(key),__storageString(value));},
 removeItem(key){if(!arguments.length)throw new __exceptionTypeError('A key is required');__storageCall(this,'delete',__storageString(key));},
 clear(){__storageCall(this,'clear');},
 key(index){if(!arguments.length)throw new __exceptionTypeError('An index is required');return __storageCall(this,'keys')[__storageNumber(index)>>>0]??null;},
 get length(){return __storageCall(this,'keys').length;},
};
delete Storage.prototype.constructor;
for(const name of ['length','clear','getItem','key','removeItem','setItem']) {
 const descriptor=Object.getOwnPropertyDescriptor(__storageMethods,name);
 if(descriptor.get)descriptor.get=__G.makeNativeFunction(descriptor.get,'get '+name,0);
 if(descriptor.value)descriptor.value=__G.makeNativeFunction(descriptor.value,name,descriptor.value.length);
 Object.defineProperty(Storage.prototype,name,descriptor);
}
const __StorageImplementation=Storage;
Storage=__G.makeNativeFunction(__StorageImplementation,'Storage',0,true,true);
Storage.prototype=__StorageImplementation.prototype;
Object.defineProperty(Storage.prototype,'constructor',{value:Storage,writable:true,configurable:true});
Object.defineProperty(Storage,'prototype',{writable:false});
Object.defineProperty(Storage.prototype,Symbol.toStringTag,{value:'Storage',configurable:true});
globalThis.Storage=Storage;
const __mkStorage = id => {
 const call=(command,key='',value='')=>__storageNativeCall(id,storage,command,key,value);
 const storage=__G.makeNamedObject(Storage.prototype,function(command,key,value){
  if(command==='get')return call('get',key)??undefined;
  if(command==='has')return call('get',key)!==null;
  if(command==='keys')return call('keys');
  if(command==='descriptor'){const value=call('get',key);return value===null?undefined:{value,writable:true,enumerable:true,configurable:true};}
  if(command==='set'){call('set',key,__storageString(value));return true;}
  if(command==='delete'){call('delete',key);return true;}
  if(command==='define'){if('get'in value||'set'in value)throw new __exceptionTypeError('Storage cannot define an accessor');call('set',key,__storageString(value.value));return true;}
  throw new __exceptionTypeError('Unknown Storage property operation');
 });
 ops.op_storage_bind(storage,id);return storage;
};
ops.op_storage_factory((id,session)=>{
 let areas=__storageWindowGet(globalThis);if(!areas){areas=__recordCreate(null);__storageWindowSet(globalThis,areas);}
 const name=session?'sessionStorage':'localStorage';
 if(areas[name]?.id!==id)areas[name]={id,value:__mkStorage(id)};return areas[name].value;
});
for(const [name,session]of [['localStorage',false],['sessionStorage',true]]){
 const getter=Object.getOwnPropertyDescriptor({get [name](){
  if(ops.op_window_opaque(this))throw new DOMException('Storage is unavailable to an opaque origin','SecurityError');
  const id=ops.op_storage_area(this,session,false);
  return ops.op_storage_object(this,id,session);
 }},name).get;
 __storageGetters[name]=getter;
 Object.defineProperty(globalThis,name,{get:__G.makeNativeFunction(getter,'get '+name,0),enumerable:true,configurable:true});
}

const __storageEventRecords=__interfaceMap('StorageEvent');
const __storageEventState=event=>{const state=__storageEventRecords.get(event);if(!state)throw new __exceptionTypeError('Illegal invocation');return state;};
const __storageEventInit=__hostEvent.prototype.initEvent;
const __storageEventPhase=Object.getOwnPropertyDescriptor(__hostEvent.prototype,'eventPhase').get;
const __storageUSV=value=>{const text=__storageString(value);let result='';for(let index=0;index<text.length;index++){const unit=__storageApply(__storageCharCode,text,[index]);if(unit>=0xd800&&unit<=0xdbff){const next=__storageApply(__storageCharCode,text,[index+1]);if(next>=0xdc00&&next<=0xdfff){result+=text[index]+text[++index];}else result+='\ufffd';}else result+=unit>=0xdc00&&unit<=0xdfff?'\ufffd':text[index];}return result;};
const __storageEventValues=(key,oldValue,newValue,url,storageArea)=>{
 key=key==null?null:__storageString(key);oldValue=oldValue==null?null:__storageString(oldValue);newValue=newValue==null?null:__storageString(newValue);url=__storageUSV(url);
 if(storageArea!=null)__storageMap(storageArea);
 return {key,oldValue,newValue,url,storageArea:storageArea??null};
};
class StorageEvent extends __hostEvent {
 constructor(type,init=undefined){
  if(!arguments.length)throw new __exceptionTypeError('StorageEvent requires a type');
  type=__storageString(type);
  if(init!=null&&typeof init!=='object'&&typeof init!=='function')throw new __exceptionTypeError('Event options must be a dictionary');
  init=init??{__proto__:null};
  const bubbles=!!init.bubbles,cancelable=!!init.cancelable,composed=!!init.composed;
  let key=init.key;key=key==null?null:__storageString(key);
  let newValue=init.newValue;newValue=newValue==null?null:__storageString(newValue);
  let oldValue=init.oldValue;oldValue=oldValue==null?null:__storageString(oldValue);
  let storageArea=init.storageArea;storageArea=storageArea??null;if(storageArea!==null)__storageMap(storageArea);
  let url=init.url;url=url===undefined?'':__storageUSV(url);
  super(type,{__proto__:null,bubbles,cancelable,composed});
  __storageEventRecords.set(this,{key,oldValue,newValue,url,storageArea});
 }
 get key(){return __storageEventState(this).key;}
 get oldValue(){return __storageEventState(this).oldValue;}
 get newValue(){return __storageEventState(this).newValue;}
 get url(){return __storageEventState(this).url;}
 get storageArea(){return __storageEventState(this).storageArea;}
 initStorageEvent(type,bubbles=false,cancelable=false,key=null,oldValue=null,newValue=null,url='',storageArea=null){
  const state=__storageEventState(this);
  if(!arguments.length)throw new __exceptionTypeError('A type is required');
  type=__storageString(type);
  const values=__storageEventValues(key,oldValue,newValue,url,storageArea);
  if(__storageApply(__storageEventPhase,this,[])!==0)return;
  __storageApply(__storageEventInit,this,[type,!!bubbles,!!cancelable]);
  state.key=values.key;state.oldValue=values.oldValue;state.newValue=values.newValue;state.url=values.url;state.storageArea=values.storageArea;
 }
}
const __StorageEventImplementation=StorageEvent;
delete StorageEvent.prototype.constructor;
for(const name of ['key','oldValue','newValue','url','storageArea','initStorageEvent']){
 const descriptor=Object.getOwnPropertyDescriptor(StorageEvent.prototype,name);descriptor.enumerable=true;
 if(descriptor.get)descriptor.get=__G.makeNativeFunction(descriptor.get,'get '+name,0);
 if(descriptor.value)descriptor.value=__G.makeNativeFunction(descriptor.value,name,1);
 Object.defineProperty(StorageEvent.prototype,name,descriptor);
}
StorageEvent=__G.makeNativeFunction(__StorageEventImplementation,'StorageEvent',1,true,true);
StorageEvent.prototype=__StorageEventImplementation.prototype;
Object.setPrototypeOf(StorageEvent,__hostEvent);
Object.defineProperty(StorageEvent.prototype,'constructor',{value:StorageEvent,writable:true,configurable:true});
Object.defineProperty(StorageEvent,'prototype',{writable:false});
Object.defineProperty(StorageEvent.prototype,Symbol.toStringTag,{value:'StorageEvent',configurable:true});
globalThis.StorageEvent=StorageEvent;
ops.op_storage_events_factory((key,oldValue,newValue,url,session)=>{
 const event=new __StorageEventImplementation('storage',{__proto__:null,key,oldValue,newValue,url,storageArea:__storageApply(__storageGetters[session?'sessionStorage':'localStorage'],globalThis,[])});
 ops.op_event_trust(event,-1);ops.op_dispatch_host_event(event,globalThis);
},()=>new __StorageEventImplementation(''));

// indexedDB — a real Chrome window global (IDBFactory). `typeof indexedDB === "undefined"`
// is a classic headless tell. A shape stub (no real persistence) that answers presence +
// class-tag + the IDBFactory method surface; requests stay pending (no backing store).
if (typeof globalThis.indexedDB === "undefined") {
  const __idbReq = () => ({ onsuccess: null, onerror: null, onupgradeneeded: null, onblocked: null, result: null, error: null, readyState: "pending", addEventListener() {}, removeEventListener() {}, dispatchEvent() { return false; } });
  const __idb = {
    open() { return __idbReq(); },
    deleteDatabase() { return __idbReq(); },
    databases() { return Promise.resolve([]); },
    cmp(a, b) { return a < b ? -1 : a > b ? 1 : 0; },
  };
  try { Object.defineProperty(__idb, Symbol.toStringTag, { value: "IDBFactory", configurable: true }); } catch (e) {}
  globalThis.indexedDB = __idb;
}
const __log = (...a) => __print(a.map(String).join(" ") + "\n");
globalThis.console = { log: __log, info: __log, warn: __log, error: __log, debug: () => {} };
const __timers = [];
Object.setPrototypeOf(__timers, null);
const __timerPush = Array.prototype.push.bind(__timers);
const __timerSplice = Array.prototype.splice.bind(__timers);
const __timerFindIndex = Array.prototype.findIndex.bind(__timers);
const __timerApply = Reflect.apply;
const __timerMax = Math.max;
let __tid = 1;
// Counts timer tasks, so per-task clocks (the document timeline) hold within one task.
let __taskEpoch = 0;
// One native clock is shared by the agent's Window realms. A timer's deadline is
// its actual scheduling time plus delay; the drain advances the shared virtual
// offset to each fired deadline. This makes the env behave like
// wall-clock for SELF-RESCHEDULING timers: a `setTimeout(poll, 1000)` that reschedules
// itself fires at virtual 1000, 2000, 3000… so over the virtual budget it fires a
// browser-like number of times (~tens), not thousands. Previously `delay` was only a
// sort key, so a polling loop (analytics SDKs like PostHog do this) fired until the raw
// count cap — spinning the entire render budget and starving the real commit. Delay-0
// work (microtasks / the React scheduler) still drains promptly (it never advances the
// clock); only delayed polls are time-gated.
const __timerNow = () => ops.op_timer_clock();
const __realWindowTimers = ops.op_window_timers_realtime();
// Virtual-time ceiling, RELATIVE to the start of the current pump/drain (`__budgetBase`).
// Once the clock passes base+budget, delayed timers stop firing so a never-idle poll can't
// hold a drain open. RELATIVE (not absolute) is essential: virtual time accumulates across a
// long session (each modal transition / poll advances it), so an absolute ceiling would,
// late in a flow, refuse to fire even a brand-new short timer — e.g. a closing MUI modal's
// 195ms Fade-exit timer never fires, so the modal never unmounts and `waitFor(hidden)` /
// subsequent `[role=dialog].first()` break. Resetting the base per drain gives every
// interaction a fresh window so its transitions complete, while still capping runaway polls.
const __VIRTUAL_BUDGET_MS = 15000;
let __budgetBase = __timerNow();
__G.resetTimerBudget = () => { __budgetBase = __timerNow(); };
const __timerDocument = globalThis.document;
const __activeTimers = new Map();
const __timerGet = Map.prototype.get.bind(__activeTimers), __timerSet = Map.prototype.set.bind(__activeTimers), __timerDelete = Map.prototype.delete.bind(__activeTimers), __timerClear = Map.prototype.clear.bind(__activeTimers);
const __createTimer = (name, repeat, handler, delay, args) => {
  const source=typeof handler === "function" ? null : ops.op_trusted_type_string(__timerDocument,handler,"TrustedScript","Window "+name,"Failed to execute '"+name+"' on 'Window': ");
  // Window timers take a signed32-bit delay before negative values clamp to zero.
  delay=(+delay)|0;if(delay<0)delay=0;
  const id=__tid++;
  if(source!==null&&!ops.op_timer_code_allowed(__timerDocument,source))return id;
  const level=ops.op_timer_nesting(__timerDocument);
  const nesting=level<7?level+1:7;
  const timer={__proto__:null,id,fn:typeof handler==="function"?handler:null,source,due:__timerNow()+(nesting>6?__timerMax(4,delay):delay),delay,nesting,repeat,args};
  __timerSet(id,timer);__timerPush(timer);return id;
};
globalThis.setTimeout=(handler,delay=0,...args)=>__createTimer("setTimeout",false,handler,delay,args);
globalThis.setInterval=(handler,delay=0,...args)=>__createTimer("setInterval",true,handler,delay,args);
globalThis.clearTimeout=(id)=>{
  id=(+id)|0;__timerDelete(id);const index=__timerFindIndex(timer=>timer.id===id);if(index>=0)__timerSplice(index,1);
};
globalThis.clearInterval=globalThis.clearTimeout;
ops.op_set_host_capabilities({cancelDocumentTasks:()=>{__timers.length=0;__timerClear();ops.op_timer_set_nesting(__timerDocument,0);}});
// The origin-relative clock combines elapsed native monotonic time with the virtual
// timer scheduler. Timers advance their scheduled delay; synchronous work consumes
// real elapsed time. Date/rAF/Event timestamps share this navigation origin.
const __hrNow = () => { try { return ops.op_now_perf(); } catch (e) { return 0; } };
const __hr0 = __hrNow();               // monotonic anchor at bootstrap
const __epoch0 = ops.op_now_epoch();      // wall epoch at bootstrap
const __perfTimeOrigin = __epoch0;
const __observed = () => (__hrNow() - __hr0) + ops.op_timer_offset();
let __perfLast = 0;
const __perfNow = () => {
  let t = Math.floor(__observed() * 10) / 10; // Chrome 100µs grid
  if (t < __perfLast) t = __perfLast;          // strictly monotonic
  __perfLast = t;
  return t;
};
// Redefine Date.now() coherently with performance.now (real epoch + real monotonic + virtual).
// The native epoch stays available to the host (cookies/TLS live in Rust, not this isolate).
const __coherentEpoch = () => Math.floor(__perfTimeOrigin + __observed());
try {
  Object.defineProperty(Date, "now", {
    value: () => __coherentEpoch(),
    configurable: true, writable: true,
  });
} catch (e) {}
// Coherence of the CONSTRUCTOR too: `new Date()` / `+new Date()` / `Date()` must read the SAME
// coherent clock as `Date.now()`, else they diverge by the accumulated virtual time after a timer
// drain (real Chrome keeps them equal — the delta is a tell). Only the zero-arg path is retimed;
// every other form (`new Date(ms)`, `new Date(y,m,d,…)`, `new Date(str)`) delegates unchanged.
// Wrap the real constructor, preserving statics, prototype identity, `instanceof`, the
// prototype.constructor back-link; the final native factory embeds the delegate.
try {
  const __RealDate = Date, __dateConstruct = Reflect.construct;
  function DateShim(...a) {
    if (!new.target) return new __RealDate(__coherentEpoch()).toString();
    // Reflect.construct with new.target so `class X extends Date {}` keeps its prototype chain
    // (`new X() instanceof X`); a plain `new __RealDate(...)` would hand back a bare Date instance
    // and break subclassing — itself a builtin-integrity tell. Zero-arg → the coherent epoch.
    return __dateConstruct(__RealDate, a.length ? a : [__coherentEpoch()], new.target);
  }
  // Real Date.prototype is non-writable; match that attribute (writable:true→false is allowed
  // even on the function's non-configurable `prototype` slot).
  Object.defineProperty(DateShim, "prototype", { value: __RealDate.prototype, writable: false });
  Object.defineProperty(__RealDate.prototype, "constructor", {
    value: DateShim, configurable: true, writable: true,
  });
  for (const name of ["now", "parse", "UTC"]) Object.defineProperty(DateShim, name,
    Object.getOwnPropertyDescriptor(__RealDate, name));
  Object.defineProperty(DateShim, "name", { value: "Date", configurable: true });
  Object.defineProperty(DateShim, "length", { value: 7, configurable: true }); // real Date.length === 7
  globalThis.Date = DateShim;
} catch (e) {}
// rAF: BATCH like a real browser (measured against Chrome). All callbacks scheduled for a frame
// fire together with the SAME fractional, origin-relative DOMHighResTimeStamp (~16.6ms cadence +
// sub-ms jitter, never before __perfNow()); a callback that re-schedules runs on the NEXT frame.
// A per-callback-incrementing clock (the naive shim) is a tell — Chrome gives every rAF in one
// frame an identical timestamp. requestAnimationFrame returns an integer id; cancelAnimationFrame
// removes only that pending callback (not clearTimeout — rAF ids are their own namespace).
let __rafClock = null, __rafId = 0, __rafScheduled = false;
let __rafQueue = [], __frameSteps = [];
// Post-animation-frame rendering steps (resize/intersection observers.js, view transitions);
// a step may queue more, so snapshot-and-clear each frame.
let __renderingSteps = [], __viewTransitionSteps = [], __renderingSuppressed = false;
// Rendering frames are engine tasks; page replacement of setTimeout cannot stall them.
const __frameTimer = globalThis.setTimeout;
// Web Animations frame host: the binding's handler for the next frame, the running frame's
// time and the last frame time the between-frames document timeline advances from.
let __animationFrame = null, __frameNow = null, __frameLast = null, __clockEpoch = -1, __clockTime = 0;
// A frame stage's time also stands for the rest of its task, its microtasks included.
const __frameStage = (ts) => { __frameNow = ts; __clockEpoch = __taskEpoch; __clockTime = ts; };
// A task that runs before any other due task, so a frame's stages stay contiguous while
// each still ends with its own microtask checkpoint.
const __frameTask = (fn) => {
  const id = __tid++;
  const timer = {__proto__:null,id,fn,source:null,due:__timerNow(),delay:0,nesting:1,repeat:false,args:[]};
  __timerSet(id,timer);__timerSplice(0,0,timer);
};
// Update-the-rendering order: queued engine steps (scroll events), then "update animations
// and send events" (update, then event dispatch, each its own task so microtask checkpoints
// separate them), then the frame's animation callbacks, which callbacks requested by those
// steps join; pending animations commit in a following task, after the callbacks' microtasks.
const __scheduleFrame = () => {
  if (__rafScheduled || __renderingSuppressed) return;
  __rafScheduled = true;
  __frameTimer(() => {
    if (__renderingSuppressed) { __rafScheduled = false; return; }
    const floor = __perfNow();
    __rafClock = __rafClock == null ? floor : __rafClock + 16.6 + (Math.random() * 0.8 - 0.4);
    if (__rafClock < floor) __rafClock = floor;
    const ts = Math.round(__rafClock * 10) / 10; // one timestamp for the whole frame
    __frameLast = ts;
    __frameStage(ts);
    const steps = __frameSteps;
    __frameSteps = [];
    for (let i = 0; i < steps.length; i++) { try { steps[i](); } catch (e) {} }
    const animation = __animationFrame;
    __animationFrame = null;
    if (animation) { try { animation.update(); } catch (e) {} }
    __frameNow = null;
    const callbacks = () => {
      __frameStage(ts);
      __rafScheduled = false;
      const batch = __rafQueue;
      __rafQueue = [];
      for (let i = 0; i < batch.length; i++) { try { batch[i].fn(ts); } catch (e) {} }
      // Observer delivery (Intersection/Resize/Performance) and view-transition steps run after
      // rAF callbacks; a step may queue more, so snapshot and clear first.
      if (!__renderingSuppressed) {
        // Persistent observer delivery (resize/intersection/performance) runs every frame.
        for (let i = 0; i < __renderingSteps.length; i++) { try { __renderingSteps[i](ts); } catch (e) {} }
        // One-shot view-transition steps: snapshot and clear; a step may queue the next.
        const vt = __viewTransitionSteps;
        __viewTransitionSteps = [];
        for (let i = 0; i < vt.length; i++) { try { vt[i](ts); } catch (e) {} }
      }
      __frameNow = null;
      if (animation) __frameTask(() => { __frameStage(ts); try { animation.commit(); } catch (e) {} __frameNow = null; });
      if (__frameSteps.length || __animationFrame || __viewTransitionSteps.length) __scheduleFrame();
    };
    // Animation events dispatch in their own task so their microtasks settle first.
    if (animation) __frameTask(() => { __frameStage(ts); try { animation.dispatch(); } catch (e) {} __frameNow = null; __frameTask(callbacks); });
    else callbacks();
  }, 16);
};
// The document timeline holds one time per task (Blink's AnimationClock): a frame stage's
// time, else the last frame time advanced in whole frame intervals, so it never runs ahead
// of the next frame's time.
const __animationNow = () => {
  if (__frameNow !== null) { __clockEpoch = __taskEpoch; __clockTime = __frameNow; return __frameNow; }
  if (__clockEpoch === __taskEpoch) return __clockTime;
  const now = __perfNow();
  let time = now;
  if (__frameLast !== null) { const frames = Math.floor((now - __frameLast) / 16.666); time = frames > 0 ? __frameLast + frames * 16.666 : __frameLast; }
  __clockEpoch = __taskEpoch; __clockTime = time;
  return time;
};
if (typeof __G.installAnimationHost === "function") __G.installAnimationHost({
  now: __animationNow,
  frame(handler) { __animationFrame = handler; __scheduleFrame(); },
  task(callback) { __frameTimer(callback, 0); },
});
globalThis.requestAnimationFrame = (fn) => {
  const id = ++__rafId;
  __rafQueue[__rafQueue.length] = { id, fn };
  __scheduleFrame();
  return id;
};
ops.op_capture_frame_scheduler((step) => { __frameSteps[__frameSteps.length] = step; __scheduleFrame(); });
// The vendored binding's view-transition rendering seam. A page here is never hidden: the
// engine has no tab or occlusion model and reports document.visibilityState "visible".
ops.op_capture_view_transition_host((command, argument) => {
  if (command === "frame") { __viewTransitionSteps[__viewTransitionSteps.length] = argument; __scheduleFrame(); }
  else if (command === "suppress") {
    __renderingSuppressed = argument === true;
    if (!__renderingSuppressed && (__rafQueue.length || __frameSteps.length || __renderingSteps.length || __viewTransitionSteps.length)) __scheduleFrame();
  } else if (command === "hidden") return false;
});
globalThis.cancelAnimationFrame = (id) => { __rafQueue = __rafQueue.filter((c) => c.id !== id); };
// Native V8 jobs share the real Promise/observer microtask checkpoint and the
// host watchdog. Their callbacks retain private realm ownership until delivery.
ops.op_install_microtask_queue();
const __nextLocalTimer = () => {
  if (!__timers.length || ops.op_window_message_pending()) return -1;
  let index=0;
  for(let i=1;i<__timers.length;i++)if(__timers[i].due<__timers[index].due)index=i;
  const due=__timers[index].due;
  return !__realWindowTimers && due>__timerNow() && due-__budgetBase>__VIRTUAL_BUDGET_MS ? -1 : index;
};
const __runLocalTimers = (max=100000,keepTask=false) => {
  let fired=0;
  while(fired<max){
    const index=__nextLocalTimer();if(index<0)break;
    const timer=__timers[index];
    // Every caller, including hydration and child-frame drains, obeys this policy.
    if(__realWindowTimers && timer.due>__timerNow())break;
    __timerSplice(index,1);ops.op_timer_advance(timer.due);fired++;__taskEpoch++;
    if(!timer.repeat)__timerDelete(timer.id);
    const previous=ops.op_timer_nesting(__timerDocument);
    ops.op_timer_set_nesting(__timerDocument,timer.nesting);
    try{
      try{if(timer.fn)__timerApply(timer.fn,globalThis,timer.args);else ops.op_run_timer_code(timer.source);}catch(error){__print("timer error: "+(error&&error.stack?error.stack:error)+"\n");}
      if(timer.repeat&&__timerGet(timer.id)===timer){
        if(timer.nesting<7)timer.nesting++;
        timer.due=__timerNow()+(timer.nesting>6?__timerMax(4,timer.delay):timer.delay);__timerPush(timer);
      }
    }finally{if(!keepTask)ops.op_timer_set_nesting(__timerDocument,previous);}
  }
  return fired;
};
__G.runTimers = (max=100000) => {
  let fired=ops.op_storage_events();
  if(fired)return fired;
  fired=ops.op_run_local_timers(__timerDocument,max);
  if(!__G.isFrame)fired+=ops.op_run_frame_timers();
  return fired;
};
ops.op_set_host_capabilities({
  timerNextDue:()=>{const index=__nextLocalTimer();return index<0?null:__timers[index].due;},
  runNextTimer:()=>__runLocalTimers(1,true),
  runLocalTimers:max=>__runLocalTimers(max)
});
// NOTE: getElementsByTagName/ClassName/Name, lastChild/previous*/nextElementSibling,
// and document.write/writeln are provided by the vendored binding (browser_env.js,
// turbo-test ≥ 71477ba) — real-world bundles (jQuery's load-time support probe,
// document.write-driven pages) depend on them. They live upstream, not here.
//
// Runtime document state stays outside the native node. Use the native brand
// accessor before routing a borrowed method/getter to another same-origin realm.
const __document = globalThis.document;
const __documentType = Object.getOwnPropertyDescriptor(Node.prototype, "nodeType").get;
const __documentApply = Reflect.apply;
const __DocumentMemberMap=Map,__DocumentMemberForEach=Map.prototype.forEach,__DocumentMemberGet=Map.prototype.get,__DocumentMemberSet=Map.prototype.set,__DocumentMemberDefine=Object.defineProperty;
const __newDocumentMembers=(source)=> {
  const result=new __DocumentMemberMap();
  if(source)__documentApply(__DocumentMemberForEach,source,[(value,key)=>__documentApply(__DocumentMemberSet,result,[key,value])]);
  __DocumentMemberDefine(result,"get",{__proto__:null,value:key=>__documentApply(__DocumentMemberGet,result,[key])});
  __DocumentMemberDefine(result,"set",{__proto__:null,value:(key,value)=>__documentApply(__DocumentMemberSet,result,[key,value])});
  return result;
};
const __parsedDocumentStates = __interfaceMap("Document.parsed");
const __documentRuntimeState = {__proto__:null,document: __document, window: globalThis, frame: null, referrer: ops.op_document_referrer(__document), createElement: null, readyState: "complete", currentScript: null, fonts: __document.fonts, handlers: new Map()};
const __documentState = (receiver) => {
  if (__documentApply(__documentType,receiver,[]) !== 9) throw new TypeError("Illegal invocation");
  if (receiver === __document) return __documentRuntimeState;
  if (__parsedDocumentStates.has(receiver)) return __parsedDocumentStates.get(receiver);
  return ops.op_document_runtime_state(receiver);
};
const __documentStateFor = (receiver) => {
  if (__documentApply(__documentType,receiver,[]) !== 9) throw new TypeError("Illegal invocation");
  if (receiver === __document) return __documentRuntimeState;
  if (__parsedDocumentStates.has(receiver)) return __parsedDocumentStates.get(receiver);
  throw new TypeError("Illegal invocation");
};
ops.op_set_document_runtime_state_accessor(__documentStateFor);
delete __G.documentStateFor;
const __documentFunction = (fn, name, length) => typeof __G.makeNativeFunction === "function"
  ? __G.makeNativeFunction(fn, name, length) : fn;
const __defineDocumentAccessor = (name, read, write) => {
  const getter = Object.getOwnPropertyDescriptor({get [name]() {return read(__documentState(this), this);}}, name).get;
  const descriptor = {get: __documentFunction(getter, "get " + name, 0), configurable: true, enumerable: true};
  if (write) {
    const setter = Object.getOwnPropertyDescriptor({set [name](value) {write(__documentState(this), this, value);}}, name).set;
    descriptor.set = __documentFunction(setter, "set " + name, 1);
  }
  Object.defineProperty(Document.prototype, name, descriptor);
  if (Object.hasOwn(__document, name)) delete __document[name];
};
// An opaque origin has no host, so its document.domain is empty. The binding
// derives the domain from the document URL, which a sandbox leaves intact; its
// getter still performs the brand check and the browsing-context resolution.
{
  const domain = Object.getOwnPropertyDescriptor(Document.prototype, "domain");
  const read = domain.get;
  domain.get = __documentFunction(function () {
    const value = __documentApply(read, this, []);
    return ops.op_document_origin_opaque(this) ? "" : value;
  }, "get domain", 0);
  Object.defineProperty(Document.prototype, "domain", domain);
}
const __legacyDocumentCreateEvent=__document.createEvent,__storageEventLower=String.prototype.toLowerCase;
__document.createEvent=({createEvent(type){
 __documentState(this);if(!arguments.length)throw new __exceptionTypeError('An event interface is required');
 type=__storageString(type);
 if(__documentApply(__storageEventLower,type,[])==='storageevent')return ops.op_storage_event_create(this);
 return __documentApply(__legacyDocumentCreateEvent,this,[type]);
}}).createEvent;
// Storage grants use the actual native document/context and its inherited policy.
// The default browsing profile permits unpartitioned cookies; handles expose
// separate unpartitioned DOM storage without switching the document's bucket.
const __storagePromise=Promise,__storageResolveIntrinsic=Promise.resolve,__storageRejectIntrinsic=Promise.reject,__storageAccessException=DOMException,__storageAccessApply=Reflect.apply;
const __storageResolve=value=>__storageAccessApply(__storageResolveIntrinsic,__storagePromise,[value]),__storageReject=value=>__storageAccessApply(__storageRejectIntrinsic,__storagePromise,[value]);
const __storageRealm={resolve:__storageResolve,reject:__storageReject,exception:__storageAccessException};
__documentRuntimeState.storageRealm=__storageRealm;
const __storageAccessTypes=['all','broadcastChannel','caches','cookies','createObjectURL','estimate','getDirectory','indexedDB','localStorage','locks','revokeObjectURL','sessionStorage','sharedWorker'];
const __storageRequested=types=>{for(let index=0;index<__storageAccessTypes.length;index++){if(types[__storageAccessTypes[index]])return true;}return false;};
function StorageAccessHandle(){throw new __exceptionTypeError('Illegal constructor');}
const __storageHandleMap=__interfaceMap('StorageAccessHandle');
const __storageHandleRecord=receiver=>{const value=__storageHandleMap.get(receiver);if(!value)throw new __exceptionTypeError('Illegal invocation');return value;};
for(const name of ['localStorage','sessionStorage','indexedDB','locks','caches']) {
 const getter=Object.getOwnPropertyDescriptor({get [name](){
  const state=__storageHandleRecord(this);
  if(!state.types.all&&!state.types[name])throw new __storageAccessException('Access was not requested','SecurityError');
  if(name!=='localStorage'&&name!=='sessionStorage')throw new __storageAccessException('This storage capability is not implemented','NotSupportedError');
  if(!state[name])state[name]=__mkStorage(ops.op_storage_area(globalThis,name==='sessionStorage',true));
  return state[name];
 }},name).get;
 Object.defineProperty(StorageAccessHandle.prototype,name,{get:__documentFunction(getter,'get '+name,0),enumerable:true,configurable:true});
}
const __StorageAccessHandleImplementation=StorageAccessHandle;
StorageAccessHandle=__G.makeNativeFunction(__StorageAccessHandleImplementation,'StorageAccessHandle',0,true,true);
StorageAccessHandle.prototype=__StorageAccessHandleImplementation.prototype;
Object.defineProperty(StorageAccessHandle.prototype,'constructor',{value:StorageAccessHandle,writable:true,configurable:true});
Object.defineProperty(StorageAccessHandle,'prototype',{writable:false});
Object.defineProperty(StorageAccessHandle.prototype,Symbol.toStringTag,{value:'StorageAccessHandle',configurable:true});
ops.op_storage_access_factory(source=>{const types={__proto__:null};for(let index=0;index<__storageAccessTypes.length;index++){const name=__storageAccessTypes[index];types[name]=source[name];}const handle=__recordCreate(StorageAccessHandle.prototype);__storageHandleMap.set(handle,{types});return handle;});
if(ops.op_is_secure_context()) {
 globalThis.StorageAccessHandle=StorageAccessHandle;
 const __accessError=(state,name,message)=>new (state.storageRealm?.exception||__storageAccessException)(message,name);
 const __accessActive=state=>!state.inactive&&state.window&&ops.op_document_has_active_window(state.document);
 const has=function hasStorageAccess(){
  let state;try{state=__documentState(this);}catch(error){return __storageReject(error);}
  const realm=state.storageRealm||__storageRealm;
  if(!__accessActive(state))return realm.reject(__accessError(state,'InvalidStateError','Document is not fully active'));
  const native=ops.op_storage_access_state(this);
  return realm.resolve(!native.opaque&&!native.credentialless);
 };
 const request=function requestStorageAccess(options){
  let state;try{state=__documentState(this);}catch(error){return __storageReject(error);}
  const realm=state.storageRealm||__storageRealm;
  let types=null;
  try{if(arguments.length&&options!==undefined) {
   if(options!==null&&typeof options!=='object'&&typeof options!=='function')throw new __exceptionTypeError('Storage access options must be a dictionary');
   types={__proto__:null};for(let index=0;index<__storageAccessTypes.length;index++){const name=__storageAccessTypes[index];types[name]=!!options?.[name];}
  }}catch(error){return realm.reject(error);}
  if(!__accessActive(state))return realm.reject(__accessError(state,'InvalidStateError','Document is not fully active'));
  if(types&&!__storageRequested(types))return realm.reject(__accessError(state,'SecurityError','No storage access was requested'));
  const native=ops.op_storage_access_state(this);
  if(native.opaque||native.credentialless||!native.allowed)return realm.reject(__accessError(state,'NotAllowedError','Storage access is denied for this document'));
  return realm.resolve(types&&native.frame?ops.op_storage_access_handle(this,types):undefined);
 };
 for(const [name,method,length]of [['hasStorageAccess',has,0],['hasUnpartitionedCookieAccess',has,0],['requestStorageAccess',request,0]])
  Object.defineProperty(Document.prototype,name,{value:__documentFunction(method,name,length),writable:true,enumerable:true,configurable:true});
}
const __cookieAccess = () => {if (!ops.op_document_cookie_allowed(__document)) throw new DOMException("An opaque document cannot access cookies", "SecurityError");};
__documentRuntimeState.readCookie = () => {__cookieAccess();return __documentRuntimeState.frame ? ops.op_frame_cookie_get(__document) : ops.op_cookie_get();};
__documentRuntimeState.writeCookie = value => {__cookieAccess();return __documentRuntimeState.frame ? ops.op_frame_cookie_set(__document, value) : ops.op_cookie_set(value);};
__defineDocumentAccessor("cookie", state => state.readCookie(),
  (state, _receiver, value) => state.writeCookie(String(value)));
__G.readDocumentCookie = __documentRuntimeState.readCookie;
__G.writeDocumentCookie = __documentRuntimeState.writeCookie;
__G.setDocumentReadyState = value => {__documentRuntimeState.readyState=value;};
__G.setCurrentScript = value => {const previous=__documentRuntimeState.currentScript;__documentRuntimeState.currentScript=value;return previous;};
ops.op_set_host_capabilities({setCurrentScript:__G.setCurrentScript});
__defineDocumentAccessor("fonts", state => state.fonts);
__defineDocumentAccessor("currentScript", state => state.currentScript);
"##,
    include_str!("permissions_policy.js"),
    include_str!("permission_status.js"),
    include_str!("privacy_sandbox.js"),
    r##"

// Parsed HTML documents share the caller's interfaces but own an independent
// native Tree. No bootstrap, script execution, or resource loader is installed.
// Document.parseHTML/parseHTMLUnsafe are the binding's (sanitizer.js); every
// native inert document reaches this initializer for its document state.
const __registerInertDocument = (doc,documentURL,contentType) => {
  const state={__proto__:null,document:doc,window:null,frame:null,url:documentURL,inactive:true,referrer:"",readyState:"complete",currentScript:null,
    fonts:__documentRuntimeState.fonts,handlers:__newDocumentMembers(),members:__newDocumentMembers(__documentRuntimeState.members),
    readCookie:()=>"",writeCookie:()=>{},createElement:tag=>ops.op_document_create_element(doc,tag)};
  state.members.set("contentType",{value:contentType});
  state.members.set("hidden",{value:true});state.members.set("visibilityState",{value:"hidden"});
  __parsedDocumentStates.set(doc,state);
  __DocumentMemberDefine(doc,"location",{__proto__:null,get:__documentFunction(function(){__documentState(this);return null;},"get location",0),enumerable:true,configurable:false});
  return doc;
};
ops.op_set_inert_document_initializer(__registerInertDocument);

// DOMParser (Chrome 156): (TrustedHTML or DOMString) then the SupportedType
// enum are converted before the Trusted Types check. Only "text/html" can be
// parsed by the HTML Tree engine; the XML types fail explicitly.
const __domParserInstances=new WeakSet();
const __domParserTypes=["text/html","text/xml","application/xml","application/xhtml+xml","image/svg+xml"];
const __domParserIsHTML=Function.prototype.call.bind(TrustedTypePolicyFactory.prototype.isHTML),__domParserFactory=globalThis.trustedTypes;
const __domParserString=(value,prefix)=>{
  if(typeof value==="symbol")throw new TypeError(prefix+"Cannot convert a Symbol value to a string");
  return String(value);
};
function __DOMParserImplementation(){
  if(new.target===undefined)throw new TypeError("Failed to construct 'DOMParser': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
  __domParserInstances.add(this);
}
const DOMParser=__G.makeNativeFunction(__DOMParserImplementation,"DOMParser",0,true,true,false);
const __domParserPrototype=__DOMParserImplementation.prototype;
delete __domParserPrototype.constructor;
DOMParser.prototype=__domParserPrototype;
Object.defineProperty(DOMParser,"prototype",{writable:false,enumerable:false,configurable:false});
Object.defineProperty(__domParserPrototype,"parseFromString",{value:__documentFunction(function parseFromString(input,type){
  if(!__domParserInstances.has(this))throw new TypeError("Illegal invocation");
  const prefix="Failed to execute 'parseFromString' on 'DOMParser': ";
  if(arguments.length<2)throw new TypeError(prefix+`2 arguments required, but only ${arguments.length} present.`);
  if(!__domParserIsHTML(__domParserFactory,input))input=__domParserString(input,prefix);
  type=__domParserString(type,prefix);
  if(!__domParserTypes.includes(type))throw new TypeError(prefix+`The provided value '${type}' is not a valid enum value of type SupportedType.`);
  input=ops.op_trusted_type_string(__document,input,"TrustedHTML","DOMParser parseFromString",prefix);
  if(type!=="text/html")throw new DOMException("XML document parsing is unsupported by the HTML Tree engine", "NotSupportedError");
  return __registerInertDocument(ops.op_parse_html_document(input,false),__document.URL,"text/html");
},"parseFromString",2),writable:true,enumerable:true,configurable:true});
Object.defineProperty(__domParserPrototype,"constructor",{value:DOMParser,writable:true,enumerable:false,configurable:true});
Object.defineProperty(__domParserPrototype,Symbol.toStringTag,{value:"DOMParser",configurable:true});
Object.defineProperty(globalThis,"DOMParser",{value:DOMParser,writable:true,enumerable:false,configurable:true});
const __importDocumentNode=({importNode(node,deep=false){
  __documentState(this);
  if(!arguments.length)throw new TypeError("importNode requires a node");
  __documentType.call(node);
  try{return ops.op_document_import_node(this,node,!!deep);}
  catch(error){if(/unsupported/.test(error.message))throw new DOMException(error.message,"NotSupportedError");throw error;}
}}).importNode;
Object.defineProperty(Document.prototype,"importNode",{value:__documentFunction(__importDocumentNode,"importNode",1),writable:true,configurable:true,enumerable:true});

// Browser pair iterators keep private live-collection state and inherit the
// actual V8 Iterator prototype. Their next methods are native embedder callbacks.
const __webIDLIterator = (tag, rows, resumeAfterDone = false) => {
  const TypeError=globalThis.TypeError, create=Object.create, apply=Reflect.apply, slice=Array.prototype.slice;
  const states = __interfaceMap("Iterator:"+tag);
  const prototype = create(Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]())));
  Object.defineProperty(prototype, Symbol.toStringTag, {value: tag, configurable: true});
  const next = function () {
    const state = states.get(this);
    if (!state) throw new TypeError("Illegal invocation");
    if (state.done && !resumeAfterDone) return {done: true, value: undefined};
    const values = rows(state.receiver);
    if (state.index >= values.length) {state.done = true;return {done: true, value: undefined};}
    const pair = values[state.index++];
    return {done: false, value: state.kind === "key" ? pair[0] : state.kind === "value" ? pair[1] : apply(slice,pair,[])};
  };
  Object.defineProperty(prototype, "next", {value: __G.makeNativeFunction(next, "next", 0), writable: true, enumerable: true, configurable: true});
  return (receiver, kind) => {
    rows(receiver); // validate the receiver immediately, before iteration starts
    const iterator = create(prototype);
    states.set(iterator, {receiver, kind, index: 0, done: false});
    return iterator;
  };
};
// Headers — fetch + analytics (PostHog) construct/read these; deno_core ships none.
// Case-insensitive name lookup, per the spec.
let __setHeadersGuard;
{
  const maps = __interfaceMap("Headers.map"), cookies = __interfaceMap("Headers.cookies"), guards = __interfaceMap("Headers.guard");
  const map = (receiver) => {const value = maps.get(receiver); if (!value) throw new TypeError("Illegal invocation");return value;};
  const string = value => {if(typeof value==='symbol')throw new TypeError('Cannot convert a Symbol to a string');return String(value);};
  const name = (value) => {value = string(value);if (!/^[!#$%&'*+.^_`|~0-9a-zA-Z-]+$/.test(value)) throw new TypeError("Invalid header name");return value.toLowerCase();};
  const content = (value) => {value = string(value);if (/[\r\n\0]/.test(value) || [...value].some(c=>c.charCodeAt(0)>255)) throw new TypeError("Invalid header value");return value.replace(/^[\t ]+|[\t ]+$/g, "");};
  const forbidden = key => key.startsWith('sec-')||key.startsWith('proxy-')||['accept-charset','accept-encoding','access-control-request-headers','access-control-request-method','connection','content-length','cookie','cookie2','date','dnt','expect','host','keep-alive','origin','permissions-policy','referer','set-cookie','te','trailer','transfer-encoding','upgrade','via'].includes(key);
  const safe = (key,value) => value.length<=128 && (key==='accept'?!/[\x00-\x08\x0a-\x1f\x7f"():<>?@\[\\\]{}]/.test(value):['accept-language','content-language'].includes(key)?/^[a-zA-Z0-9 *,.\-;=]*$/.test(value):key==='content-type'?!/[\x00-\x08\x0a-\x1f\x7f"():<>?@\[\\\]{}]/.test(value)&&['application/x-www-form-urlencoded','multipart/form-data','text/plain'].includes(value.split(';')[0].trim().toLowerCase()):false);
  const permitted = (receiver,key,value) => {const guard=guards.get(receiver);if(guard==='immutable')throw new TypeError('Headers are immutable');return !guard||!forbidden(key)&&(guard!=='request-no-cors'||safe(key,value));};
  __setHeadersGuard=(receiver,guard)=>{const values=map(receiver);guards.set(receiver,guard);if(guard!=='immutable')for(const [key,value]of values)if(!permitted(receiver,key,value))values.delete(key);};
  const entries = receiver => [...map(receiver)].flatMap(([key,value])=>key==="set-cookie"?cookies.get(receiver).map(value=>[key,value]):[[key,value]]).sort(([a],[b])=>a<b?-1:a>b?1:0);
  const iterate = __webIDLIterator("Headers Iterator", entries);
  globalThis.Headers = class Headers {
    constructor(init = undefined) {
      maps.set(this, new Map());
      cookies.set(this, []);
      if (init == null) return;
      if (typeof init[Symbol.iterator] === "function") {
        for (const pair of init) {const values = Array.from(pair);if(values.length!==2) throw new TypeError("Header pairs require two values");this.append(values[0], values[1]);}
      } else for (const [key,value] of Object.entries(init)) this.append(key,value);
    }
    append(key, value) {const values=map(this);if(arguments.length<2)throw new TypeError("Two arguments required");key=name(key);value=content(value);const joined=values.has(key)?values.get(key)+", "+value:value;if(!permitted(this,key,joined))return;if(key==="set-cookie")cookies.get(this).push(value);values.set(key,joined);}
    set(key, value) {const values=map(this);if(arguments.length<2)throw new TypeError("Two arguments required");key=name(key);value=content(value);if(!permitted(this,key,value))return;if(key==="set-cookie")cookies.set(this,[value]);values.set(key,value);}
    get(key) {const values=map(this);if(!arguments.length)throw new TypeError("An argument is required");return values.get(name(key)) ?? null;}
    has(key) {const values=map(this);if(!arguments.length)throw new TypeError("An argument is required");return values.has(name(key));}
    delete(key) {const values=map(this);if(!arguments.length)throw new TypeError("An argument is required");key=name(key);if(!permitted(this,key,''))return;values.delete(key);if(key==="set-cookie")cookies.set(this,[]);}
    getSetCookie() {map(this);return cookies.get(this).slice();}
    forEach(callback, thisArg = undefined) {map(this);if(typeof callback!=="function")throw new TypeError("A callback is required");for(const [key,value] of iterate(this,"entry"))callback.call(thisArg,value,key,this);}
    keys() {return iterate(this,"key");}
    values() {return iterate(this,"value");}
    entries() {return iterate(this,"entry");}
  };
  Object.defineProperty(Headers.prototype,Symbol.iterator,{value:Headers.prototype.entries,writable:true,configurable:true});
  Object.defineProperty(Headers.prototype,Symbol.toStringTag,{value:"Headers",configurable:true});
}
let __responseFromNetwork, __requestInit, __resourceResponse, __BodyParamsString;
const __blobStates = __interfaceMap("Blob");
const __formDataStates = __interfaceMap("FormData");
// multipart/form-data BodyInit: UTF-8, newline-normalized names and text values,
// Blink's quoted-string escapes and its random WebKitFormBoundary.
const __multipartReplace = String.prototype.replace;
const __multipartBody = entries => {
  const random = new Uint8Array(16), table = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789AB";
  ops.op_random_fill(random);
  let boundary = "----WebKitFormBoundary";
  for (let index = 0; index < random.length; index++) boundary += table[random[index] & 63];
  const chunks = [];
  let size = 0;
  const push = bytes => {chunks[chunks.length] = bytes; size += bytes.length;};
  const text = value => push(ops.op_text_encode(value));
  const lines = value => __baseApply(__multipartReplace, value, [/\r\n|\r|\n/g, "\r\n"]);
  const quote = value => __baseApply(__multipartReplace, value, [/["\r\n]/g, character => character === "\"" ? "%22" : character === "\r" ? "%0D" : "%0A"]);
  for (let index = 0; index < entries.length; index++) {
    const name = entries[index][0], value = entries[index][1];
    text("--" + boundary + "\r\nContent-Disposition: form-data; name=\"" + quote(lines(name)) + "\"");
    if (typeof value === "string") { text("\r\n\r\n" + lines(value) + "\r\n"); continue; }
    const file = __fileStates.get(value), blob = __blobStates.get(value);
    text("; filename=\"" + quote(file.name) + "\"\r\nContent-Type: " + (blob.type || "application/octet-stream") + "\r\n\r\n");
    push(blob.bytes);
    text("\r\n");
  }
  text("--" + boundary + "--\r\n");
  const bytes = new Uint8Array(size);
  let at = 0;
  for (let index = 0; index < chunks.length; index++) {bytes.set(chunks[index], at); at += chunks[index].length;}
  return {bytes, type: "multipart/form-data; boundary=" + boundary};
};
const __bodyByteTag=Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype),Symbol.toStringTag).get;
const __bodyByteChunk=value=>__baseApply(__bodyByteTag,value,[])==="Uint8Array";
const __fetchFailures=__interfaceMap("Fetch.failure");
const __resourceFailure=error=>__fetchFailures.get(error);
// Private body and interface state. Public members are receiver-checked Web-IDL
// accessors; body reads consume once and retain the identity of their stream.
{
  const responses = __interfaceMap("Response"), requests = __interfaceMap("Request");
  const state = (map, receiver) => {const value = map.get(receiver);if (!value) throw new TypeError("Illegal invocation");return value;};
  const bufferLength = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,"byteLength").get;
  const bufferView = ArrayBuffer.isView, Bytes = Uint8Array;
  const applyBody = Reflect.apply;
  const extractBody = raw => {
    if(raw==null)return {raw:null,type:null,sourceStream:false};
    if(__web.readable(raw)){
      if(__web.disturbed(raw)||applyBody(__ReadableLocked,raw,[]))throw new TypeError("Body stream is locked or disturbed");
      return {raw,type:null,sourceStream:true};
    }
    if(__formDataStates.has(raw)){const body=__multipartBody(__formDataStates.get(raw).entries);return {raw:body.bytes,type:body.type,sourceStream:false};}
    const blob=__blobStates.get(raw);
    if(blob)return {raw:new Bytes(blob.bytes),type:blob.type||null,sourceStream:false};
    if(bufferView(raw)){
      applyBody(bufferLength,raw.buffer,[]);
      return {raw:new Bytes(new Bytes(raw.buffer,raw.byteOffset,raw.byteLength)),type:null,sourceStream:false};
    }
    let arrayBuffer=false;
    try{applyBody(bufferLength,raw,[]);arrayBuffer=true;}catch{}
    if(arrayBuffer)return {raw:new Bytes(new Bytes(raw)),type:null,sourceStream:false};
    let params;
    try{params=applyBody(__BodyParamsString,raw,[]);}catch{}
    if(params!==undefined)return {raw:params,type:"application/x-www-form-urlencoded;charset=UTF-8",sourceStream:false};
    if(typeof raw==="symbol")throw new TypeError("Cannot convert a Symbol value to a string");
    return {raw:__String(raw),type:"text/plain;charset=UTF-8",sourceStream:false};
  };
  const bodyState = (raw,sourceStream=false) => ({raw,sourceStream,used:false,stream:null});
  const rawBytes = async raw => {
    if (raw == null) return new Uint8Array();
    if (raw instanceof ArrayBuffer) return new Uint8Array(raw.slice(0));
    if (ArrayBuffer.isView(raw)) return new Uint8Array(raw.buffer.slice(raw.byteOffset, raw.byteOffset + raw.byteLength));
    if (__web.readable(raw)) {
      const reader=applyBody(__ReadableGetReader,raw,[]), chunks=[];let size=0;
      try {while(true){const part=await reader.read();if(part.done)break;if(!__bodyByteChunk(part.value))throw new TypeError("Body stream chunks must be Uint8Array");const bytes=new Bytes(part.value);chunks.push(bytes);size+=bytes.length;}}
      finally {reader.releaseLock();}
      const bytes=new Uint8Array(size);let at=0;for(const part of chunks){bytes.set(part,at);at+=part.length;}return bytes;
    }
    return new TextEncoder().encode(String(raw));
  };
  const body = value => {
    if(value.raw==null)return null;
    if(value.stream)return value.stream;
    value.stream=__web.readable(value.raw)?value.raw:new __FetchReadable({async start(controller){try{controller.enqueue(await rawBytes(value.raw));controller.close();}catch(error){controller.error(error);}}});
    return value.stream;
  };
  const consume = async value => {if(value.used || value.stream && __web.disturbed(value.stream) || value.stream?.locked)throw new TypeError("Body has already been consumed");if(value.raw==null&&!value.stream)return new Uint8Array();value.used=true;return value.stream?rawBytes(value.stream):rawBytes(value.raw);};
  const copyBody = value => {if(value.used || value.stream && __web.disturbed(value.stream) || value.stream?.locked)throw new TypeError("Body has already been consumed");if(value.stream || __web.readable(value.raw)){const streams=applyBody(__ReadableTee,body(value),[]);value.stream=streams[0];value.raw=streams[0];return streams[1];}return value.raw;};
  const transferBody = value => {
    if(value.raw==null)return null;
    if(value.used||value.stream&&(__web.disturbed(value.stream)||applyBody(__ReadableLocked,value.stream,[])))throw new TypeError("Body has already been consumed");
    const transferred=applyBody(__ReadablePipeThrough,body(value),[new __BodyTransform()]);
    value.used=true;
    return transferred;
  };
  const addContentType = (headers, extracted) => {
    if(!headers.has("content-type")&&extracted.type)headers.set("content-type",extracted.type);
  };
  const bodyMethods = map => ({
    async arrayBuffer(){const bytes=await consume(state(map,this).body);return bytes.buffer.slice(bytes.byteOffset,bytes.byteOffset+bytes.byteLength);},
    async bytes(){return consume(state(map,this).body);},
    async text(){return new TextDecoder().decode(await consume(state(map,this).body));},
    async json(){return JSON.parse(new TextDecoder().decode(await consume(state(map,this).body)));},
    async blob(){const value=state(map,this);return new Blob([await consume(value.body)],{type:value.headers.get("content-type") || ""});},
    textStream(){const value=state(map,this).body;if(value.used||value.stream&&__web.disturbed(value.stream)||value.stream?.locked)throw new TypeError("Body has already been consumed");const stream=body(value);value.used=true;const decoder=new TextDecoder(),reader=stream?.getReader();let released=false;const release=()=>{if(reader&&!released){released=true;reader.releaseLock();}};return new __FetchReadable({async pull(controller){try{if(reader){while(true){const part=await reader.read();if(part.done)break;const text=decoder.decode(part.value,{stream:true});if(text){controller.enqueue(text);return;}}}const rest=decoder.decode();if(rest)controller.enqueue(rest);controller.close();release();}catch(error){controller.error(error);release();}},async cancel(reason){try{if(reader)await reader.cancel(reason);}finally{release();}}},{highWaterMark:0});},
    async formData(){const value=state(map,this),type=value.headers.get("content-type")||"",bytes=await consume(value.body);if(!/^application\/x-www-form-urlencoded(?:;|$)/i.test(type))throw new TypeError("Unsupported form encoding");const form=new FormData();for(const [key,item] of new URLSearchParams(new TextDecoder().decode(bytes)))form.append(key,item);return form;}
  });
  const project = (ctor,map,fields) => {
    for(const field of fields)Object.defineProperty(ctor.prototype,field,{get:({["get "+field](){return state(map,this)[field];}})["get "+field],enumerable:true,configurable:true});
    // Getter functions are extracted from descriptors, preserving nonconstructibility.
    Object.defineProperty(ctor.prototype,"body",{get:Object.getOwnPropertyDescriptor({get body(){return body(state(map,this).body);}},"body").get,enumerable:true,configurable:true});
    Object.defineProperty(ctor.prototype,"bodyUsed",{get:Object.getOwnPropertyDescriptor({get bodyUsed(){const value=state(map,this).body;return value.used||!!(value.stream&&__web.disturbed(value.stream));}},"bodyUsed").get,enumerable:true,configurable:true});
    for(const [name,method] of Object.entries(bodyMethods(map)))Object.defineProperty(ctor.prototype,name,{value:method,writable:true,enumerable:true,configurable:true});
  };
  globalThis.Response = class Response {
    constructor(raw=null, init=undefined){init=init||{};const status=init.status==null?200:Number(init.status);if(!Number.isInteger(status)||status<200||status>599)throw new RangeError("Invalid response status");if(raw!=null&&[204,205,304].includes(status))throw new TypeError("Null body status cannot have a body");const headers=new Headers(init.headers),statusText=String(init.statusText||"");if(/[\r\n]/.test(statusText))throw new TypeError("Invalid status text");const extracted=extractBody(raw);addContentType(headers,extracted);responses.set(this,{body:bodyState(extracted.raw,extracted.sourceStream),status,statusText,headers,ok:status>=200&&status<300,type:"default",url:"",redirected:false});}
    static json(data,init=undefined){const text=JSON.stringify(data);if(text===undefined)throw new TypeError("JSON data is not serializable");const headers=new Headers(init?.headers);if(!headers.has("content-type"))headers.set("content-type","application/json");return new Response(text,{...init,headers});}
    static error(){const response=new Response();Object.assign(state(responses,response),{status:0,statusText:"",ok:false,type:"error"});return response;}
    static redirect(url,status=302){const absolute=new URL(String(url),location.href);if(![301,302,303,307,308].includes(Number(status)))throw new RangeError("Invalid redirect status");return new Response(null,{status:Number(status),headers:{location:absolute.href}});}
    clone(){const value=state(responses,this),copied=copyBody(value.body);const clone=new Response([204,205,304].includes(value.status)?null:copied,{status:value.status||200,statusText:value.statusText,headers:value.headers});if([204,205,304].includes(value.status))state(responses,clone).body=bodyState(copied);Object.assign(state(responses,clone),{status:value.status,ok:value.ok,type:value.type,url:value.url,redirected:value.redirected});if(value.type!=="default")__setHeadersGuard(clone.headers,"immutable");return clone;}
  };
  project(Response,responses,["status","statusText","headers","ok","type","url","redirected"]);
  globalThis.Request = class Request {
    constructor(input,init=undefined){if(!arguments.length)throw new TypeError("Request requires an input");init=init||{};const previous=requests.get(input),integrity=init.integrity,signal=__web.dependent(init.signal===undefined?previous?.signal??null:init.signal),absolute=new URL(previous?previous.url:String(input),__baseApply(__baseURI,__document,[]));if(absolute.username||absolute.password)throw new TypeError("Request URL cannot include credentials");const requested=String(init.method||previous?.method||"GET");if(!/^[!#$%&'*+.^_`|~0-9a-zA-Z-]+$/.test(requested)||["CONNECT","TRACE","TRACK"].includes(requested.toUpperCase()))throw new TypeError("Invalid request method");const method=["DELETE","GET","HEAD","OPTIONS","POST","PUT"].includes(requested.toUpperCase())?requested.toUpperCase():requested;const supplied=init.body!=null,extracted=supplied?extractBody(init.body):{raw:previous?.body.raw??null,type:null,sourceStream:previous?.body.sourceStream??false};if(extracted.raw!=null&&["GET","HEAD"].includes(method))throw new TypeError("GET and HEAD cannot have bodies");if(init.duplex!==undefined&&__String(init.duplex)!=="half")throw new TypeError("Invalid request duplex");if(supplied&&extracted.sourceStream&&init.duplex===undefined)throw new TypeError("Stream requests require duplex");const headers=new Headers(init.headers||previous?.headers);addContentType(headers,extracted);const mode=init.mode||previous?.mode||"cors",credentials=init.credentials||previous?.credentials||"same-origin",cache=init.cache||previous?.cache||"default",redirect=init.redirect||previous?.redirect||"follow";if(!["cors","no-cors","same-origin"].includes(mode)||!["omit","same-origin","include"].includes(credentials)||!["default","no-store","reload","no-cache","force-cache","only-if-cached"].includes(cache)||!["follow","error","manual"].includes(redirect)||cache==="only-if-cached"&&mode!=="same-origin")throw new TypeError("Invalid request option");if(mode==="no-cors"&&!["GET","HEAD","POST"].includes(method))throw new TypeError("Invalid no-cors request method");__setHeadersGuard(headers,mode==="no-cors"?"request-no-cors":"request");
      const referrerPolicy=__String(init.referrerPolicy??previous?.referrerPolicy??"");
      if(!["","no-referrer","no-referrer-when-downgrade","same-origin","origin","strict-origin","origin-when-cross-origin","strict-origin-when-cross-origin","unsafe-url"].includes(referrerPolicy))throw new TypeError("Invalid referrer policy");
      let referrer=__String(init.referrer??previous?.referrer??"about:client");
      if(referrer!==""&&referrer!=="about:client"){
        const referrerUrl=new URL(referrer,ops.op_document_request_url());
        const documentUrl=new URL(ops.op_document_request_url());
        referrer=referrerUrl.origin===documentUrl.origin?referrerUrl.href:"about:client";
      }
      const keepalive=!!(init.keepalive??previous?.keepalive);
      if(extracted.sourceStream&&(keepalive||mode==="no-cors"))throw new TypeError("Invalid stream request options");
      const raw=supplied?extracted.raw:previous?transferBody(previous.body):null;
      requests.set(this,{url:absolute.href,method,headers,body:bodyState(raw,extracted.sourceStream),mode,credentials,cache,redirect,referrer,referrerPolicy,integrity:__String(integrity!==undefined?integrity:previous?.integrity??""),keepalive,signal,destination:"",duplex:"half",targetAddressSpace:init.targetAddressSpace??previous?.targetAddressSpace??"unknown",isHistoryNavigation:false,isReloadNavigation:false});}
    clone(){const value=state(requests,this),clone=new Request(value.url,{...value,body:copyBody(value.body)});state(requests,clone).body.sourceStream=value.body.sourceStream;return clone;}
  };
  project(Request,requests,["url","method","headers","mode","credentials","cache","redirect","referrer","referrerPolicy","integrity","keepalive","signal","destination","duplex","targetAddressSpace","isHistoryNavigation","isReloadNavigation"]);
  const __NetworkResponse=Response,__NetworkHeaders=Headers;
  __responseFromNetwork=(raw,options)=>{const filtered=options.type==="opaque"||options.type==="opaqueredirect";const response=new __NetworkResponse(filtered||[204,205,304].includes(options.status)?null:raw,{...options,status:filtered?200:options.status});if(!filtered&&[204,205,304].includes(options.status))state(responses,response).body=bodyState(raw);Object.assign(state(responses,response),filtered?{status:0,statusText:"",ok:false,url:options.type==="opaqueredirect"?options.url:"",redirected:false,type:options.type,headers:new __NetworkHeaders()}:{url:options.url,redirected:options.redirected,type:options.type||"basic"});const item=state(responses,response);__setHeadersGuard(item.headers,"immutable");item.resource=options.resource;return response;};
  __resourceResponse=response=>{const item=state(responses,response);if(!item.resource)throw new TypeError("Not a network response");return item.resource;};
  __requestInit = value => {const item=requests.get(value);return item?{...item,body:item.body.raw,bodyIsStream:item.body.sourceStream}:value;};
}
__web = __webFactory(__core, __G.makeNativeFunction);
const __FetchReadable=ReadableStream;
const __FetchPromise=Promise,__FetchPromiseRace=Promise.race.bind(Promise),__FetchThen=Function.prototype.call.bind(Promise.prototype.then);
const __ReadableGetReader=ReadableStream.prototype.getReader,__ReadableTee=ReadableStream.prototype.tee;
const __ReadablePipeThrough=ReadableStream.prototype.pipeThrough,__BodyTransform=TransformStream;
const __ReadableLocked=Object.getOwnPropertyDescriptor(ReadableStream.prototype,"locked").get;
// fetch over the tier-1 net stack → a real Response (with real headers, so RSC
// client navigation that reads `res.headers.get('content-type')` works).
"##,
    include_str!("compression_dictionary_fetch.js"),
    r##"
const __fetchFromSource = async (url, init, referrerSource) => {
  const source = ops.op_document_request_source(__document);
  const cookieSite = ops.op_document_cookie_site(__document);
  // Marshal the request (method/headers/body) to the op — a Request object carries them
  // on itself; an init object carries them as fields. Headers may be a Headers instance,
  // an array of pairs, or a plain object.
  const req = (url && typeof url === "object") ? url : null;
  const o = { ...(__requestInit(req) || {}), ...(init || {}) };
  let hdrs = o.headers;
  if (hdrs && typeof hdrs.forEach === "function" && !Array.isArray(hdrs)) {
    const obj = {}; hdrs.forEach((v, k) => { obj[k] = v; }); hdrs = obj;
  } else if (Array.isArray(hdrs)) {
    const obj = {}; for (const [k, v] of hdrs) obj[k] = v; hdrs = obj;
  }
  __rejectManualDictionaryHeader(hdrs);
  const signal=o.signal??null;
  const isAborted=()=>signal!==null&&__baseApply(__AbortSignalAborted,signal,[]);
  const abortReason=()=>__baseApply(__AbortSignalReason,signal,[]);
  if(isAborted())throw abortReason();
  const requestTarget=o.url || __String(url);
  const directive=o.destination==="script"?"script-src-elem":o.destination==="style"?"style-src-elem":o.destination==="image"?"img-src":o.destination==="iframe"?"frame-src":"connect-src";
  if(!ops.op_csp_request(__document,requestTarget,directive,o.nonce||""))throw new __exceptionTypeError("Failed to fetch");
  const cancelId=ops.op_fetch_prepare(__document);
  const transport={controller:null,done:false,uploadReader:null};
  let uploadPump=null,uploadError=null,notifyAbort;
  const abortNotice=signal?new __FetchPromise(resolve=>{notifyAbort=resolve;}):null;
  const finish=()=>{if(transport.done)return;transport.done=true;if(signal)__baseApply(__AbortRemove,signal,["abort",onAbort]);};
  const onAbort=()=>{const reason=abortReason();notifyAbort?.(reason);ops.op_fetch_abort(cancelId);if(transport.uploadReader)transport.uploadReader.cancel(reason).catch(()=>{});if(transport.controller)transport.controller.error(reason);finish();};
  if(signal)__baseApply(__AbortAdd,signal,["abort",onAbort,{once:true}]);
  let body = o.body == null ? o._body : o.body,bodyBytes;
  try {
    if(o.bodyIsStream&&new URL(requestTarget).protocol==="http:"){
      const error=new __exceptionTypeError("Streaming requests require HTTP/2 or HTTP/3");
      if(body&&__web.readable(body))body.cancel(error).catch(()=>{});
      throw error;
    }
    if(body!=null&&typeof body!=="string"){
      if(body instanceof ArrayBuffer)bodyBytes=Array.from(new Uint8Array(body));
      else if(ArrayBuffer.isView(body))bodyBytes=Array.from(new Uint8Array(body.buffer,body.byteOffset,body.byteLength));
      else if(typeof body.arrayBuffer==="function")bodyBytes=Array.from(new Uint8Array(await body.arrayBuffer()));
      else if(__web.readable(body)&&o.bodyIsStream){
        const reader=__baseApply(__ReadableGetReader,body,[]);transport.uploadReader=reader;
        ops.op_fetch_upload_start(cancelId);
        uploadPump=(async()=>{try{for(;;){const part=await reader.read();if(isAborted())throw abortReason();if(part.done)break;if(!__bodyByteChunk(part.value))throw new __exceptionTypeError("Request stream chunks must be Uint8Array");for(let at=0;at<part.value.byteLength;at+=65536){const chunk=new Uint8Array(part.value.buffer,part.value.byteOffset+at,Math.min(65536,part.value.byteLength-at));await ops.op_fetch_upload_write(cancelId,chunk);}}ops.op_fetch_upload_finish(cancelId);}catch(error){uploadError=error;ops.op_fetch_cancel(cancelId);}finally{reader.releaseLock();transport.uploadReader=null;}})();
        body=undefined;
      }else if(__web.readable(body)){
        const reader=__baseApply(__ReadableGetReader,body,[]),chunks=[];let size=0;
        try{for(;;){const part=await reader.read();if(isAborted())throw abortReason();if(part.done)break;if(!__bodyByteChunk(part.value))throw new __exceptionTypeError("Request stream chunks must be Uint8Array");const chunk=new Uint8Array(part.value);chunks.push(chunk);size+=chunk.length;}const bytes=new Uint8Array(size);let at=0;for(const part of chunks){bytes.set(part,at);at+=part.length;}bodyBytes=Array.from(bytes);}finally{reader.releaseLock();}
      }else body=__String(body);
      if(bodyBytes)body=undefined;
    }
    if(isAborted())throw abortReason();
  }catch(error){ops.op_fetch_cancel(cancelId);finish();throw error;}
  // Next.js App Router CLIENT navigation (`router.push`/`replace`, e.g. the post-login
  // redirect) fetches the target route's RSC flight with an `RSC` header — a PREFETCH
  // adds `Next-Router-Prefetch`. We don't do in-place RSC soft-nav (it never completes
  // headlessly, so `location`/`history` never advance and a `waitForURL` hangs). Record
  // the navigation target on `__G.rscNav`; the live-session driver re-loads that route as
  // a fresh page (the browser hard-nav equivalent), following the redirect chain hop by
  // hop. Prefetches are ignored.
  try {
    const lc = {};
    for (const k in (hdrs || {})) lc[k.toLowerCase()] = hdrs[k];
    if (lc.rsc && !lc["next-router-prefetch"]) {
      const u = new URL(String((url && url.url) || url), globalThis.location.href);
      if (u.pathname !== globalThis.location.pathname) {
        // Keep the app's own query (e.g. the off-cycle termination flow passes the selected
        // employee as `?employeeIds=`) but drop Next's internal `_rsc` cache-buster — a hard
        // reload carrying `_rsc` returns a flight payload, not HTML.
        u.searchParams.delete("_rsc");
        __G.rscNav = u.pathname + u.search + u.hash;
      }
    }
  } catch (_e) {}
  const initJson = __stringify({ __proto__:null, method: o.method, headers: hdrs || undefined, body, body_bytes:bodyBytes, upload:!!uploadPump, cancel_id:cancelId, stream:!o.destination, cache:o.cache,
    referrer:o.referrer,referrerPolicy:o.referrerPolicy,
    mode: o.mode, credentials: o.credentials, redirect:o.redirect, integrity:o.integrity, destination: o.destination, source, referrer_source:referrerSource,
    opaque_origin: ops.op_window_opaque(__document), ...cookieSite, csp:ops.op_csp_context(__document), nonce:o.nonce });
  // Count in-flight fetches so the hydration/interaction drain doesn't quiesce while a
  // request is outstanding — otherwise a save POST resolves AFTER the drain gives up
  // (the DOM looks "stable" while waiting) and its success re-render (modal close,
  // redirect) is lost.
  globalThis.__pendingFetches = (globalThis.__pendingFetches || 0) + 1;
  let r;
  try {
    const request=ops.op_fetch(__String(requestTarget), initJson);
    r = await (abortNotice?__FetchPromiseRace([request,__FetchThen(abortNotice,reason=>{throw reason;})]):request);
  } catch(error) {ops.op_fetch_cancel(cancelId);finish();throw error;} finally {
    globalThis.__pendingFetches = Math.max(0, (globalThis.__pendingFetches || 1) - 1);
  }
  if(uploadPump){
    if((r.aborted||!r.status||isAborted())&&transport.uploadReader)await transport.uploadReader.cancel(isAborted()?abortReason():new __exceptionTypeError("Failed to fetch")).catch(()=>{});
    await uploadPump;
    if(uploadError&&!isAborted()){ops.op_fetch_cancel(cancelId);finish();throw new __exceptionTypeError("Failed to fetch");}
  }
  if(r.aborted||isAborted()){ops.op_fetch_cancel(cancelId);finish();throw isAborted()?abortReason():new DOMException("The operation was aborted","AbortError");}
  if (!r.status) {ops.op_fetch_cancel(cancelId);finish();const error=new __exceptionTypeError("Failed to fetch");__fetchFailures.set(error,{policyBlocked:r.policy_blocked,integrityBlocked:r.integrity_blocked,started:!r.policy_blocked,crossOrigin:r.cross_origin,responseReceived:r.response_received});throw error;}
  const crossOrigin = r.cross_origin;
  const responseType = o.redirect==="manual"&&[301,302,303,307,308].includes(r.status)?"opaqueredirect":crossOrigin && (o.mode || "cors") === "no-cors" && !o.destination ? "opaque" : crossOrigin && (o.mode || "cors") === "cors" ? "cors" : "basic";
  const headers = (r.headers || []).filter(([name]) => !["set-cookie", "set-cookie2"].includes(name.toLowerCase()));
  if (r.content_type && !headers.some(([name]) => name.toLowerCase() === "content-type")) headers.push(["content-type",r.content_type]);
  let payload=new Uint8Array(r.body_bytes||[]);
  if(r.stream_id){
    if(responseType==="opaque"||responseType==="opaqueredirect"){
      payload=null;
      (async()=>{try{for(;;){const part=await ops.op_fetch_read(r.stream_id);if(part.done||part.aborted||part.error)break;}}catch{}finally{finish();}})();
    }
    else if([204,205,304].includes(r.status)){ops.op_fetch_cancel(cancelId);finish();payload=new __FetchReadable({type:"bytes",start(controller){controller.close();}});}
    else {
      payload=new __FetchReadable({type:"bytes",start(controller){transport.controller=controller;},async pull(controller){
        try{const part=await ops.op_fetch_read(r.stream_id);if(transport.done)return;if(part.aborted||isAborted())throw isAborted()?abortReason():new DOMException("The operation was aborted","AbortError");if(part.error)throw new __exceptionTypeError(part.error);if(part.done){controller.close();finish();}else controller.enqueue(new Uint8Array(part.bytes));}catch(error){controller.error(error);ops.op_fetch_cancel(cancelId);finish();}
      },cancel(){ops.op_fetch_cancel(cancelId);finish();}},{highWaterMark:0});
      ops.op_fetch_watch_body(payload,cancelId);
    }
  }else finish();
  return __responseFromNetwork(payload, {
    status: r.status, statusText:r.status_text, headers, type: responseType, url: r.final_url || __String((url && url.url) || url), redirected: r.redirected,
    resource:{url:r.final_url,status:r.status,ok:r.ok,nosniff:r.nosniff,contentType:r.content_type,headers:r.headers,text:r.body,mutedErrors:!!r.script_errors_muted,originClean:!crossOrigin||o.mode==="cors"},
  });
};
const __FetchRequest=Request;
const __AbortSignalAborted=Object.getOwnPropertyDescriptor(AbortSignal.prototype,"aborted").get;
const __AbortSignalReason=Object.getOwnPropertyDescriptor(AbortSignal.prototype,"reason").get;
const __AbortAdd=EventTarget.prototype.addEventListener,__AbortRemove=EventTarget.prototype.removeEventListener;
globalThis.fetch = async (input, init = undefined) => {
  const request = new __FetchRequest(input, init);
  return __fetchFromSource(request, {...__requestInit(request), destination:""}, ops.op_document_request_url());
};
// V8 clones preserve cycles and built-in types. Supported native keys use a
// private host serializer; no page hook receives their material.
// ArrayBuffer ownership transfers are validated and detached by the native bridge.
{
  const clone=ops.op_surf_structured_clone, owner=globalThis;
  const push=Array.prototype.push, apply=Reflect.apply, iterator=Symbol.iterator, setPrototype=Object.setPrototypeOf;
  const objectLike=value=>value!==null&&(typeof value==="object"||typeof value==="function");
  const sequence=value=>{
    if(!objectLike(value))throw new __exceptionTypeError("Transfer must be an object sequence");
    const method=value[iterator];
    if(typeof method!=="function")throw new __exceptionTypeError("Transfer must be iterable");
    const source=apply(method,value,[]);
    if(!objectLike(source))throw new __exceptionTypeError("Invalid transfer iterator");
    const next=source.next;
    if(typeof next!=="function")throw new __exceptionTypeError("Invalid transfer iterator next");
    const result=[];setPrototype(result,null);
    for(;;){
      const step=apply(next,source,[]);
      if(!objectLike(step))throw new __exceptionTypeError("Invalid transfer iterator result");
      if(step.done)return result;
      const entry=step.value;
      if(!objectLike(entry))throw new __exceptionTypeError("Transfer entries must be objects");
      if(result.length===4096)throw new __exceptionTypeError("Native transfer sequence budget exceeded");
      apply(push,result,[entry]);
    }
  };
  globalThis.structuredClone=__documentFunction(({structuredClone(value, options=undefined){
    if(!arguments.length)throw new __exceptionTypeError("A value is required");
    const receiver=this==null?owner:this;
    if(!ops.op_surf_is_window(receiver))throw new __exceptionTypeError("Illegal invocation");
    if(options!=null && !objectLike(options))throw new __exceptionTypeError("Options must be a dictionary");
    const transfer=options?.transfer;
    return clone(value,receiver,transfer===undefined?undefined:sequence(transfer));
  }}).structuredClone,"structuredClone",1);
}
// NOTE: getComputedStyle is provided by the vendored browser_env.js (a jsdom-style
// getComputedStyle the Playwright shim's cssValue/visibility reads). Do NOT redefine
// IT here — ENV_BOOTSTRAP runs AFTER the binding, so an override would clobber the
// real one and break the shim.
//
// Supported viewport and host-preference queries use the same private native
// owner-window evaluator as CSS @media; guest dimension shadows cannot change
// these results. Other legacy media features retain the bounded evaluator below.
{
  const __mediaWindow = globalThis, __mediaTypeError=TypeError, __mediaDOMException=DOMException;
  const __mqLen = (tok, basis) => {
    // A CSS length in a media feature → px. Supports px and em/rem (16px root).
    const m = String(tok).match(/([\d.]+)\s*(px|r?em)?/);
    if (!m) return NaN;
    const n = parseFloat(m[1]);
    return m[2] === "em" || m[2] === "rem" ? n * 16 : n;
  };
  const __mqClause = (clause) => {
    if (clause.indexOf("print") >= 0) return false;
    const w = typeof globalThis.innerWidth === "number" ? globalThis.innerWidth : 1280;
    const h = typeof globalThis.innerHeight === "number" ? globalThis.innerHeight : 800;
    let ok = true;
    for (const feat of clause.split(/\band\b/)) {
      let m;
      if ((m = feat.match(/min-width\s*:\s*([^)]+)/)) && w < __mqLen(m[1])) ok = false;
      if ((m = feat.match(/max-width\s*:\s*([^)]+)/)) && w > __mqLen(m[1])) ok = false;
      if ((m = feat.match(/min-height\s*:\s*([^)]+)/)) && h < __mqLen(m[1])) ok = false;
      if ((m = feat.match(/max-height\s*:\s*([^)]+)/)) && h > __mqLen(m[1])) ok = false;
      // Desktop defaults: light scheme, landscape, fine pointer + hover available.
      if (/prefers-color-scheme\s*:\s*dark/.test(feat)) ok = false;
      if (/orientation\s*:\s*portrait/.test(feat) && w >= h) ok = false;
      if (/orientation\s*:\s*landscape/.test(feat) && w < h) ok = false;
      if (/hover\s*:\s*none/.test(feat)) ok = false;
      if (/pointer\s*:\s*coarse/.test(feat)) ok = false;
      if (/any-pointer\s*:\s*coarse/.test(feat)) ok = false;
      // Accessibility features all default OFF in a stock Chrome, so their "active"
      // queries must NOT match (a true here reads as a non-default a11y env — a tell).
      if (/prefers-reduced-motion\s*:\s*reduce/.test(feat)) ok = false;
      if (/prefers-reduced-transparency\s*:\s*reduce/.test(feat)) ok = false;
      if (/prefers-reduced-data\s*:\s*reduce/.test(feat)) ok = false;
      if (/prefers-contrast\s*:\s*(more|less|custom)/.test(feat)) ok = false;
      if (/forced-colors\s*:\s*active/.test(feat)) ok = false;
      if (/inverted-colors\s*:\s*inverted/.test(feat)) ok = false;
    }
    return ok;
  };
  const __evalMedia = (q, owner) => {
    if (/(?:forced-colors|prefers-contrast|prefers-color-scheme|prefers-reduced-motion|prefers-reduced-transparency|(?:any-)?pointer|(?:any-)?hover|\b(?:min-|max-)?(?:width|height)\b|aspect-ratio|orientation)/i.test(String(q || ""))) return ops.op_live_media_match(owner, String(q || ""));
    const query = String(q || "").toLowerCase().trim();
    if (!query || query === "all") return true;
    return query.split(",").some((c) => __mqClause(c.trim()));
  };
  globalThis.matchMedia = ({ matchMedia(q) {
    const owner=this==null?__mediaWindow:this;
    if(!ops.op_surf_is_window(owner))throw new __mediaTypeError("Illegal invocation");
    if(!ops.op_windows_same_origin(owner))throw new __mediaDOMException("Cross-origin Window access","SecurityError");
    const media = String(q == null ? "" : q);
    if(ops.op_window_media_state(owner)===0)return null;
    const mql = {
      media,
      onchange: null,
      _l: [],
      addListener(fn) { if (typeof fn === "function") this._l.push(fn); },
      removeListener(fn) { const i = this._l.indexOf(fn); if (i >= 0) this._l.splice(i, 1); },
      addEventListener(_t, fn) { if (typeof fn === "function") this._l.push(fn); },
      removeEventListener(_t, fn) { const i = this._l.indexOf(fn); if (i >= 0) this._l.splice(i, 1); },
      dispatchEvent() { return false; },
    };
    Object.defineProperty(mql, "matches", { get: () => ops.op_window_media_state(owner)===2 && __evalMedia(media, owner), enumerable: true });
    return mql;
  } }).matchMedia;
}
// FormData — auth/login SDKs (PropelAuth) build credential payloads with it; deno_core
// ships none. A spec-shaped impl over an entry list (append keeps duplicates; set
// replaces; field values stringified, File/Blob passed through).
const __formDataTypeError=TypeError, __formDataApply=Reflect.apply, __formDataFreeze=Object.freeze;
const __formDataSplice=Array.prototype.splice;
const __readFormData=receiver=>{const record=__formDataStates.get(receiver);if(!record)throw new __formDataTypeError("Illegal invocation");return record.entries;};
const __formDataIterator=__webIDLIterator("FormData Iterator",__readFormData,true);
const __formDataValue=(value,filename,hasFilename)=>{
  const blob=__blobStates.get(value),file=__fileStates.get(value);
  if(!blob){if(hasFilename)throw new __formDataTypeError("The third FormData argument requires a Blob");return __fileUSV(value);}
  const provided=hasFilename&&filename!==undefined;
  if(file&&!provided)return value;
  return __createPrivateFile(blob,provided?__fileUSV(filename):"blob",file?file.lastModified:__fileNow());
};
if (typeof globalThis.FormData === "undefined") {
  globalThis.FormData = class FormData {
    constructor(form=undefined,submitter=undefined) {
      const rows=ops.op_form_data_entries(form,submitter);
      try {
        const record=__makeFormDataRecord(rows);__formDataStates.set(this,record);
        if(form!==undefined&&!ops.op_form_data_event(form,__aliasFormData(record)))throw new __formDataTypeError("Native formdata event dispatch failed");
      } finally { ops.op_form_data_finish(form); }
    }
    append(name,value,filename=undefined){const rows=__readFormData(this);if(arguments.length<2)throw new __formDataTypeError("FormData.append requires two arguments");name=__fileUSV(name);value=__formDataValue(value,filename,arguments.length>2);rows[rows.length]=[name,value];}
    set(name,value,filename=undefined){const rows=__readFormData(this);if(arguments.length<2)throw new __formDataTypeError("FormData.set requires two arguments");name=__fileUSV(name);value=__formDataValue(value,filename,arguments.length>2);let first=-1;for(let i=0;i<rows.length;i++)if(rows[i][0]===name){if(first<0){first=i;rows[i]=[name,value];}else{__formDataApply(__formDataSplice,rows,[i,1]);i--;}}if(first<0)rows[rows.length]=[name,value];}
    get(name){const rows=__readFormData(this);if(arguments.length<1)throw new __formDataTypeError("FormData.get requires an argument");name=__fileUSV(name);for(let i=0;i<rows.length;i++)if(rows[i][0]===name)return rows[i][1];return null;}
    getAll(name){const rows=__readFormData(this);if(arguments.length<1)throw new __formDataTypeError("FormData.getAll requires an argument");name=__fileUSV(name);const out=[];for(let i=0;i<rows.length;i++)if(rows[i][0]===name)out[out.length]=rows[i][1];return out;}
    has(name){const rows=__readFormData(this);if(arguments.length<1)throw new __formDataTypeError("FormData.has requires an argument");name=__fileUSV(name);for(let i=0;i<rows.length;i++)if(rows[i][0]===name)return true;return false;}
    delete(name){const rows=__readFormData(this);if(arguments.length<1)throw new __formDataTypeError("FormData.delete requires an argument");name=__fileUSV(name);for(let i=rows.length-1;i>=0;i--)if(rows[i][0]===name)__formDataApply(__formDataSplice,rows,[i,1]);}
    forEach(callback,thisArg=undefined){const rows=__readFormData(this);if(typeof callback!=="function")throw new __formDataTypeError("FormData.forEach requires a function");for(let i=0;i<rows.length;i++)__formDataApply(callback,thisArg,[rows[i][1],rows[i][0],this]);}
    keys(){return __formDataIterator(this,"key");}
    values(){return __formDataIterator(this,"value");}
    entries(){return __formDataIterator(this,"entry");}
    [Symbol.iterator](){return __formDataIterator(this,"entry");}
  };
  Object.defineProperty(FormData.prototype,Symbol.toStringTag,{value:"FormData",configurable:true});
  for(const key of ["append","set","get","getAll","has","delete","forEach","keys","values","entries"])Object.defineProperty(FormData.prototype,key,{...Object.getOwnPropertyDescriptor(FormData.prototype,key),enumerable:true});
  const descriptors=Object.getOwnPropertyDescriptors(FormData.prototype);
  for(const key of ["constructor","append","delete","get","getAll","has","set","entries","forEach","keys","values"])delete FormData.prototype[key];
  for(const key of ["append","delete","get","getAll","has","set","entries","forEach","keys","values","constructor"])Object.defineProperty(FormData.prototype,key,descriptors[key]);
  delete FormData.prototype[Symbol.iterator];
  Object.defineProperty(FormData.prototype,Symbol.iterator,{value:FormData.prototype.entries,writable:true,configurable:true});
}
const __capturedFormDataPrototype=globalThis.FormData.prototype;
const __makeFormDataRecord=rows=>{const entries=[];for(let i=0;i<rows.length;i++)entries[entries.length]=[__fileUSV(rows[i][0]),__formDataValue(rows[i][1],undefined,false)];return {entries};};
const __aliasFormData=record=>{const wrapper=__recordCreate(__capturedFormDataPrototype);__formDataStates.set(wrapper,record);return wrapper;};
// Blob / File / FileReader — analytics + upload code (PostHog, file inputs) reference
// these during hydration; deno_core ships none ("File is not defined" aborts PostHog
// init). Private byte buffers preserve binary Blob/File parts and authentic brands
// across same-origin realms; BodyInit reads this native private state directly.
let __readBlobBytes = null;
if (typeof globalThis.Blob === "undefined") {
  const blobs = __blobStates;
  const read = receiver => {const value=blobs.get(receiver);if(!value)throw new TypeError("Illegal invocation");return value;};
  __readBlobBytes = receiver => read(receiver).bytes;
  globalThis.Blob = class Blob {
    constructor(parts = [], options = {}) {
      if(parts==null || typeof parts[Symbol.iterator]!=="function")throw new TypeError("Blob parts must be iterable");
      const chunks=[];let size=0;
      for(const part of parts){let bytes;if(blobs.has(part))bytes=read(part).bytes;else if(part instanceof ArrayBuffer)bytes=new Uint8Array(part);else if(ArrayBuffer.isView(part))bytes=new Uint8Array(part.buffer,part.byteOffset,part.byteLength);else bytes=new TextEncoder().encode(String(part));chunks.push(bytes);size+=bytes.length;}
      const bytes=new Uint8Array(size);let at=0;for(const chunk of chunks){bytes.set(chunk,at);at+=chunk.length;}
      const type=String(options.type||"");blobs.set(this,{bytes,type:/[^\x20-\x7E]/.test(type)?"":type.toLowerCase()});
    }
    get size(){return read(this).bytes.length;}
    get type(){return read(this).type;}
    async arrayBuffer(){return read(this).bytes.slice().buffer;}
    slice(start=0,end=undefined,type=""){const value=read(this),size=value.bytes.length;start=Number(start)||0;end=end===undefined?size:Number(end)||0;start=start<0?Math.max(size+start,0):Math.min(start,size);end=end<0?Math.max(size+end,0):Math.min(end,size);return new Blob([value.bytes.slice(start,Math.max(start,end))],{type});}
    stream(){const bytes=read(this).bytes.slice();return new ReadableStream({start(controller){controller.enqueue(bytes);controller.close();}});}
    async text(){return new TextDecoder().decode(read(this).bytes);}
    async bytes(){return read(this).bytes.slice();}
  };
  for(const name of ["size","type","arrayBuffer","slice","stream","text","bytes"])Object.defineProperty(Blob.prototype,name,{...Object.getOwnPropertyDescriptor(Blob.prototype,name),enumerable:true});
  const constructor=Object.getOwnPropertyDescriptor(Blob.prototype,"constructor");delete Blob.prototype.constructor;Object.defineProperty(Blob.prototype,"constructor",constructor);
  Object.defineProperty(Blob.prototype,Symbol.toStringTag,{value:"Blob",configurable:true});
}
const __fileStates=__interfaceMap("File");
const __fileTypeError=TypeError, __fileNumber=Number, __fileTrunc=Math.trunc;
const __fileDate=Date, __fileNow=Date.now, __fileBigInt=BigInt, __fileInt64=BigInt.asIntN, __fileBytes=Uint8Array;
const __fileApply=Reflect.apply, __fileToWellFormed=String.prototype.toWellFormed;
const __fileUSV=value=>{if(typeof value==="symbol")throw new __fileTypeError("Cannot convert a Symbol value to a string");return __fileApply(__fileToWellFormed,__String(value),[]);};
const __readFile=receiver=>{const record=__fileStates.get(receiver);if(!record)throw new __fileTypeError("Illegal invocation");return record;};
if (typeof globalThis.File === "undefined") {
  globalThis.File = class File extends globalThis.Blob {
    constructor(parts, name, opts = {}) {
      if(arguments.length<2)throw new __fileTypeError("File requires two arguments");
      opts=opts==null?{}:opts;
      const filename=__fileUSV(name), modified=opts.lastModified;
      const number=modified===undefined?__fileNow():+modified;
      const lastModified=number!==number||number===Infinity||number===-Infinity?0:__fileNumber(__fileInt64(64,__fileBigInt(__fileTrunc(number))));
      super(parts, opts);
      __fileStates.set(this,{name:filename,lastModified});
    }
    get name(){return __readFile(this).name;}
    get lastModified(){return __readFile(this).lastModified;}
    get lastModifiedDate(){return new __fileDate(__readFile(this).lastModified);}
    get webkitRelativePath(){__readFile(this);return "";}
  };
  for(const key of ["name","lastModified","lastModifiedDate","webkitRelativePath"])Object.defineProperty(File.prototype,key,{...Object.getOwnPropertyDescriptor(File.prototype,key),enumerable:true});
  const constructor=Object.getOwnPropertyDescriptor(File.prototype,"constructor");delete File.prototype.constructor;Object.defineProperty(File.prototype,"constructor",constructor);
  Object.defineProperty(File.prototype,Symbol.toStringTag,{value:"File",configurable:true});
}
const __capturedFile=globalThis.File, __capturedFilePrototype=__capturedFile.prototype;
const __createPrivateFile=(blob,name,lastModified)=>{const file=__recordCreate(__capturedFilePrototype);__blobStates.set(file,{bytes:blob.bytes,type:blob.type});__fileStates.set(file,{name,lastModified});return file;};
const __snapshotFormValue=value=>{
  const file=__fileStates.get(value);
  if(file)return __formDataFreeze({kind:"file",value,name:file.name});
  const form=__formDataStates.get(value);
  if(!form)return null;
  const rows=[];for(let i=0;i<form.entries.length;i++){const entry=form.entries[i];rows[rows.length]=__formDataFreeze([entry[0],entry[1]]);}
  return __formDataFreeze({kind:"formdata",entries:__formDataFreeze(rows)});
};
ops.op_set_host_capabilities({
  snapshotFormValue:__snapshotFormValue,
  createSubmissionFormData:(rows,charsetIndices,encoding)=>{
    const record=__makeFormDataRecord(rows);
    for(let i=0;i<charsetIndices.length;i++){const index=charsetIndices[i];if(index>=0&&index<record.entries.length)record.entries[index][1]=encoding;}
    return __aliasFormData(record);
  },
  snapshotFormFileName:receiver=>{const file=__fileStates.get(receiver);return file&&__blobStates.has(receiver)?file.name:null;},
  createEmptyFormFile:()=>__createPrivateFile({bytes:new __fileBytes(),type:"application/octet-stream"},"",__fileNow()),
  snapshotFormFile:receiver=>{const file=__fileStates.get(receiver),blob=__blobStates.get(receiver);return file&&blob?{name:file.name,lastModified:file.lastModified,type:blob.type,bytes:new __fileBytes(blob.bytes)}:null;}
});
// The canonical binding owns native custom-element construction, registry state
// and lifecycle reactions in every Window realm.
// (CSSStyleSheet, document.adoptedStyleSheets, and the HTML*Element constructor
// family are all provided by the vendored browser_env binding.)
// MessagePort / MessageChannel — real classes (not object-literal stubs), so `port instanceof
// MessagePort` holds and `port.addEventListener('message', fn)` works, not just `onmessage=`
// (Chrome shape; the object-literal stub was a brand/instanceof tell). React 18's scheduler
// drains its queue by posting to a MessagePort and running the peer's onmessage; google's SERP/
// homepage use the same `new MessageChannel; port1.onmessage=…; port2.postMessage(0)` next-tick
// idiom — delivery routes through the virtual timer queue (setTimeout 0) so the hydration/
// interaction pump drains it. Setting `onmessage` implies `start()` (Chrome); the addEventListener
// path queues until start().
// MessageEvent — a REAL constructor (the vendored binding ships only a hollow `function(){}`
// stub, so `new MessageEvent('message',{data}).data` was undefined). Our postMessage / port /
// BroadcastChannel dispatch build events via this so `ev instanceof MessageEvent` holds and the
// event carries data/origin/source/ports like Chrome. Defined before the ports that use it.
const __messageEventState = __interfaceMap("MessageEvent");
// Assigned by user_activation.js: snapshot construction and the UserActivation brand.
let __userActivationSnapshot = null, __userActivationBrand = null;
const __messageState = event => {
  const state = __messageEventState.get(event);
  if (!state) throw new TypeError("Illegal invocation");
  return state;
};
const __messageString = value => {
  if (typeof value === "symbol") throw new TypeError("Cannot convert a Symbol to a string");
  return String(value);
};
globalThis.MessageEvent = class MessageEvent extends Event {
  constructor(type, init = undefined) {
    if (!arguments.length) throw new TypeError("MessageEvent requires a type");
    super(type, init);
    init = init || {};
    __messageEventState.set(this, {
      data: init.data === undefined ? null : init.data,
      origin: init.origin === undefined ? "" : __messageString(init.origin),
      lastEventId: init.lastEventId === undefined ? "" : __messageString(init.lastEventId),
      source: init.source == null ? null : init.source,
      ports: Object.freeze(init.ports === undefined ? [] : Array.from(init.ports)),
      userActivation: null,
    });
    const userActivation = init.userActivation;
    if (userActivation !== undefined && userActivation !== null) {
      if (!__userActivationBrand(userActivation))
        throw new TypeError("Failed to construct 'MessageEvent': Failed to read the 'userActivation' property from 'MessageEventInit': Failed to convert value to 'UserActivation'.");
      __messageEventState.get(this).userActivation = userActivation;
    }
  }
  get data() { return __messageState(this).data; }
  get origin() { return __messageState(this).origin; }
  get lastEventId() { return __messageState(this).lastEventId; }
  get source() { return __messageState(this).source; }
  get ports() { return __messageState(this).ports; }
  get userActivation() { return __messageState(this).userActivation; }
  initMessageEvent(type, bubbles = false, cancelable = false, data = null, origin = "", lastEventId = "", source = null, ports = []) {
    const state = __messageState(this);
    if (!arguments.length) throw new TypeError("A type is required");
    if (this.eventPhase !== Event.NONE) return;
    Event.prototype.initEvent.call(this, type, bubbles, cancelable);
    state.data = data; state.origin = __messageString(origin); state.lastEventId = __messageString(lastEventId);
    state.source = source; state.ports = Object.freeze(Array.from(ports));
  }
};
for (const name of ["data", "origin", "lastEventId", "source", "ports", "userActivation", "initMessageEvent"]) {
  const descriptor = Object.getOwnPropertyDescriptor(MessageEvent.prototype, name);
  Object.defineProperty(MessageEvent.prototype, name, {...descriptor, enumerable: true});
}
Object.defineProperty(MessageEvent.prototype, Symbol.toStringTag, {value: "MessageEvent", configurable: true});
const __errorEventState = __interfaceMap("ErrorEvent");
const __errorEventRecord = event => {
  const state = __errorEventState.get(event);
  if (!state) throw new TypeError("Illegal invocation");
  return state;
};
globalThis.ErrorEvent = class ErrorEvent extends Event {
  constructor(type, init = undefined) {
    if (!arguments.length) throw new TypeError("ErrorEvent requires a type");
    super(type, init);
    init = init || {};
    __errorEventState.set(this, {
      message: init.message === undefined ? "" : __messageString(init.message),
      filename: init.filename === undefined ? "" : __messageString(init.filename),
      lineno: init.lineno === undefined ? 0 : (+init.lineno) >>> 0,
      colno: init.colno === undefined ? 0 : (+init.colno) >>> 0,
      error: init.error === undefined ? null : init.error,
    });
  }
  get message() {return __errorEventRecord(this).message;}
  get filename() {return __errorEventRecord(this).filename;}
  get lineno() {return __errorEventRecord(this).lineno;}
  get colno() {return __errorEventRecord(this).colno;}
  get error() {return __errorEventRecord(this).error;}
};
for (const name of ["message", "filename", "lineno", "colno", "error"]) {
  const descriptor = Object.getOwnPropertyDescriptor(ErrorEvent.prototype, name);
  Object.defineProperty(ErrorEvent.prototype, name, {...descriptor, enumerable:true});
}
Object.defineProperty(ErrorEvent.prototype, Symbol.toStringTag, {value:"ErrorEvent", configurable:true});
"##,
    include_str!("message_ports.js"),
    r##"
const __streamTransferFactory =
"##,
    include_str!("stream_transfers.js"),
    r##"
{
  const apply=Reflect.apply, get=Object.getOwnPropertyDescriptor;
  const channel=MessageChannel, cp=channel.prototype;
  const first=get(cp,"port1").get, second=get(cp,"port2").get;
  const post=MessagePort.prototype.postMessage, close=MessagePort.prototype.close;
  const start=MessagePort.prototype.start, listen=EventTarget.prototype.addEventListener;
  const data=get(MessageEvent.prototype,"data").get;
  const api=__web.transferBackend(), Exception=DOMException;
  api.channel=()=>{const pair=new channel();return [apply(first,pair,[]),apply(second,pair,[])];};
  api.post=(port,value)=>apply(post,port,[value]);
  api.close=port=>apply(close,port,[]);
  api.listen=(port,receive,error)=>{
    apply(listen,port,["message",event=>receive(apply(data,event,[]))]);
    apply(listen,port,["messageerror",()=>error(new Exception("Stream chunk could not be cloned","DataCloneError"))]);
    apply(start,port,[]);
  };
  const bridge=__streamTransferFactory(api);
  const validate=bridge.validate, prepare=bridge.prepare, commit=bridge.commit;
  const abandon=bridge.abandon, restore=bridge.restore;
  ops.op_stream_transfer_install((mode,a,b,c)=>{
    if(mode===0)return validate(a);
    if(mode===1)return prepare(a);
    if(mode===2)return commit(a,b);
    if(mode===3)return restore(a,b,c);
    if(mode===4)return abandon(a);
  });
}

// window.postMessage — deliver a `message` event to THIS realm's window listeners
// (async, via the timer queue so the hydration/interaction drain processes it). Real
// same-window `postMessage(msg)` fires `message` handlers with `{data, origin, source}`
// where `origin` is the sender's (i.e. our) origin. This is load-bearing for the
// reCAPTCHA/BotGuard host protocol: after the VM installs the grecaptcha API it drives
// its own scheduler by posting to the window and running work in the `message` handler —
// a bare `postMessage(...)` call (window.postMessage) with no shim throws
// "postMessage is not a function" and aborts the VM before any token path runs. The
// vendored binding supplies window `addEventListener`/`dispatchEvent`; we only add the
// poster. MessageEvent is a bare stub here, so build a plain event carrying the fields
// collectors read (data/origin/source/ports).
const __postingWindow=globalThis, __postingException=DOMException;
globalThis.postMessage = ({postMessage(message, targetOrigin = "/", transfer) {
  if (arguments.length === 0) throw new __exceptionTypeError("A message is required");
  const receiver=this==null?__postingWindow:this;
  if(!ops.op_surf_is_window(receiver))throw new __exceptionTypeError("Illegal invocation");
  let includeActivation=false;
  if(targetOrigin===null || typeof targetOrigin==="object") {
    const options=targetOrigin||{};
    includeActivation=!!options.includeUserActivation;
    const origin=options.targetOrigin;
    targetOrigin=origin===undefined?"/":origin;
    transfer=options.transfer;
  }
  if(typeof targetOrigin==="symbol")throw new __exceptionTypeError("Cannot convert a Symbol to a string");
  targetOrigin=__String(targetOrigin);
  const transferList=transfer===undefined?undefined:__portTransferSequence(transfer);
  if(targetOrigin!=="*" && targetOrigin!=="/") {
    try {ops.op_surf_url_parse(targetOrigin,"");}
    catch(_){throw new __postingException("Invalid target origin","SyntaxError");}
  }
  ops.op_window_post_message(receiver, message, targetOrigin, transferList, includeActivation);
}}).postMessage;
const __windowPostMessageNative=globalThis.postMessage, __windowPostingApply=Reflect.apply;
ops.op_set_host_capabilities({postWindowMessage:(target,message,origin,transfer)=>__windowPostingApply(__windowPostMessageNative,target,[message,origin,transfer])});
"##,
    include_str!("security_policy_event.js"),
    r##"
const __windowViews = __interfaceMap("Window.view");
__G.windowView = (target) => {
  if (target==null)return null;
  if (ops.op_native_window_proxy()) return target;
  if (ops.op_windows_same_origin(target)) return target;
  if (__windowViews.has(target)) return __windowViews.get(target);
  const deny = () => { throw new DOMException("Blocked a frame with a different origin", "SecurityError"); };
  const allowed = Object.create(null);
  const view = new Proxy(allowed, {
    get(_object, name) {
      if (name === Symbol.toStringTag) return "Window";
      if (name === "window" || name === "self" || name === "frames") return view;
      if (name === "postMessage") return allowed.postMessage;
      if (name === "parent") return __G.windowView(ops.op_window_parent(target));
      if (name === "top") return __G.windowView(ops.op_window_top(target));
      if (name === "closed") return !ops.op_window_active(target);
      if (name === "blur" || name === "focus" || name === "close") return allowed[name];
      return deny();
    },
    set() { return deny(); },
    getPrototypeOf() { return null; },
    ownKeys() { return ["window", "self", "frames", "parent", "top", "postMessage", "closed", "blur", "focus", "close"]; },
  });
  allowed.postMessage = (...args) => __windowPostingApply(__windowPostMessageNative,target,args);
  for (const name of ["blur", "focus", "close"]) allowed[name] = () => {};
  __windowViews.set(target, view);
  return view;
};
const __hostMessageEvent=globalThis.MessageEvent, __hostMessageSchedule=globalThis.setTimeout;
const __deliverWindowMessage = (message, sender, origin, ports, failed, activation) => {
  "use strict";
  const source = __G.windowView(sender);
  __hostMessageSchedule(() => {
    const event = __hostPortMessage(failed ? "messageerror" : "message", message, ports, source, origin, activation);
    ops.op_event_trust(event, -1);
    ops.op_dispatch_host_event(event, globalThis);
  }, 0);
};
ops.op_set_host_capabilities({deliverWindowMessage:__deliverWindowMessage});
// window `on*` event-handler slots default to `null` in a real browser (never
// undefined). The reCAPTCHA VM reads `window.onmessage` while wiring its postMessage
// handshake; an undefined read (vs null) is both a functional gap and a headless tell.
for (const __on of ["onmessage", "onerror", "onmessageerror", "ononline", "onoffline", "onpopstate", "onhashchange", "onbeforeunload", "onunload", "onload"]) {
  try { if (globalThis[__on] === undefined) globalThis[__on] = null; } catch (_e) {}
}
// document.contentType — real Chrome reports "text/html" for an HTML document; the
// reCAPTCHA VM reads it. The vendored binding doesn't expose it, so add an own accessor.
try {
  if (globalThis.document && globalThis.document.contentType === undefined) {
    Object.defineProperty(globalThis.document, "contentType", { configurable: true, get() { return "text/html"; } });
  }
} catch (_e) {}
// User Timing stores real entries; navigation records only phases this runtime observes.
const __performanceEntries = __interfaceMap("PerformanceEntry");
const __performanceTimeline = [];
// Each hard navigation owns its identifier; timeline entries retain that identity.
// Chromium's navigation generator initializes the same public range, 100..10000.
const __navigationRandom = new Uint32Array(1);
do { ops.op_random_fill(new Uint8Array(__navigationRandom.buffer)); } while (__navigationRandom[0] >= 4294967296 - 4294967296 % 9901);
const __navigationId = 100 + __navigationRandom[0] % 9901;
const __performanceFields = ['navigationStart','unloadEventStart','unloadEventEnd','redirectStart','redirectEnd','fetchStart','domainLookupStart','domainLookupEnd','connectStart','secureConnectionStart','connectEnd','requestStart','responseStart','responseEnd','domLoading','domInteractive','domContentLoadedEventStart','domContentLoadedEventEnd','domComplete','loadEventStart','loadEventEnd'];
const __navigationPhases = Object.fromEntries(__performanceFields.map(name=>[name,0]));
__navigationPhases.navigationStart=Math.floor(__perfTimeOrigin);
__navigationPhases.domLoading=Math.floor(__perfTimeOrigin+__perfNow());
const __performanceString = value => {if(typeof value==='symbol')throw new TypeError('Cannot convert a Symbol to a string');return String(value);};
const __performanceNumber = value => {const n=Number(value);if(!Number.isFinite(n)||n<0)throw new TypeError('A nonnegative finite timestamp is required');return n;};
const __entryState = receiver => {const state=__performanceEntries.get(receiver);if(!state)throw new TypeError('Illegal invocation');return state;};
const __performanceOwner=globalThis;
const __performanceDetail = detail => detail===undefined?null:ops.op_surf_structured_clone(detail,__performanceOwner);
globalThis.PerformanceEntry = class PerformanceEntry {
  constructor(){throw new TypeError('Illegal constructor');}
  get name(){return __entryState(this).name;}
  get entryType(){return __entryState(this).entryType;}
  get startTime(){return __entryState(this).startTime;}
  get duration(){return __entryState(this).duration;}
  get navigationId(){return __entryState(this).navigationId;}
  toJSON(){const state=__entryState(this);const result={name:state.name,entryType:state.entryType,startTime:state.startTime,duration:state.duration,navigationId:state.navigationId};if(state.entryType==='navigation')Object.assign(result,__navigationRelative());return result;}
};
globalThis.PerformanceMark = class PerformanceMark {
  constructor(name,options=undefined){if(!arguments.length)throw new TypeError('A mark name is required');name=__performanceString(name);if(__performanceFields.includes(name))throw new DOMException('Reserved timing name','SyntaxError');if(options!=null&&typeof options!=='object'&&typeof options!=='function')throw new TypeError('Mark options must be a dictionary');options=options||{};__performanceEntries.set(this,{name,navigationId:__navigationId,entryType:'mark',startTime:options.startTime===undefined?__perfNow():__performanceNumber(options.startTime),duration:0,detail:__performanceDetail(options.detail)});}
  get detail(){const state=__entryState(this);if(state.entryType!=='mark')throw new TypeError('Illegal invocation');return state.detail;}
};
globalThis.PerformanceMeasure = class PerformanceMeasure {
  constructor(){throw new TypeError('Illegal constructor');}
  get detail(){const state=__entryState(this);if(state.entryType!=='measure')throw new TypeError('Illegal invocation');return state.detail;}
};
globalThis.PerformanceNavigationTiming = class PerformanceNavigationTiming {constructor(){throw new TypeError('Illegal constructor');}};
for(const name of ['PerformanceMark','PerformanceMeasure','PerformanceNavigationTiming']){Object.setPrototypeOf(globalThis[name].prototype,PerformanceEntry.prototype);Object.setPrototypeOf(globalThis[name],PerformanceEntry);}
for(const name of ['PerformanceEntry','PerformanceMark','PerformanceMeasure','PerformanceNavigationTiming']){
  const proto=globalThis[name].prototype;Object.defineProperty(proto,Symbol.toStringTag,{value:name,configurable:true});
  for(const key of Object.getOwnPropertyNames(proto)){if(key==='constructor')continue;const d=Object.getOwnPropertyDescriptor(proto,key);Object.defineProperty(proto,key,{...d,enumerable:true});}
}
const __navigationRelative = () => Object.fromEntries(__performanceFields.filter(name=>name!=='navigationStart'&&name!=='domLoading').map(name=>[name,__navigationPhases[name]?Math.max(0,__navigationPhases[name]-__navigationPhases.navigationStart):0]));
const __navigationEntry=Object.create(PerformanceNavigationTiming.prototype);
__performanceEntries.set(__navigationEntry,{name:location.href,navigationId:__navigationId,entryType:'navigation',startTime:0,duration:0});
for(const name of __performanceFields.filter(name=>name!=='navigationStart'&&name!=='domLoading'))Object.defineProperty(PerformanceNavigationTiming.prototype,name,{get(){if(this!==__navigationEntry)throw new TypeError('Illegal invocation');return __navigationRelative()[name];},enumerable:true,configurable:true});
for(const [name,value]of Object.entries({type:'navigate',redirectCount:0}))Object.defineProperty(PerformanceNavigationTiming.prototype,name,{get(){if(this!==__navigationEntry)throw new TypeError('Illegal invocation');return value;},enumerable:true,configurable:true});
__performanceTimeline.push(__navigationEntry);
__G.performancePhase = name => {if(__performanceFields.includes(name)){__navigationPhases[name]=Math.floor(__perfTimeOrigin+__perfNow());if(name==='loadEventEnd')__entryState(__navigationEntry).duration=__navigationRelative().loadEventEnd;}};
const __latestMark = value => {
  if(typeof value==='number')return __performanceNumber(value);
  const name=__performanceString(value);
  if(__performanceFields.includes(name)){if(name==='navigationStart')return 0;if(!__navigationPhases[name])throw new DOMException('Timing phase has not occurred','InvalidAccessError');return __navigationPhases[name]-__navigationPhases.navigationStart;}
  for(let i=__performanceTimeline.length-1;i>=0;i--){const entry=__performanceTimeline[i];if(entry.entryType==='mark'&&entry.name===name)return entry.startTime;}
  throw new DOMException('The mark does not exist','SyntaxError');
};
const __clearPerformanceEntries = (type,name) => {for(let i=__performanceTimeline.length-1;i>=0;i--)if(__performanceTimeline[i].entryType===type&&(name===undefined||__performanceTimeline[i].name===name))__performanceTimeline.splice(i,1);};
const __eventCountsData=__interfaceMap("EventCounts");
globalThis.EventCounts=class EventCounts {
  constructor(){throw new TypeError('Illegal constructor');}
  get size(){return __counts(this).size;}
  entries(){return __counts(this).entries();}
  keys(){return __counts(this).keys();}
  values(){return __counts(this).values();}
  get(key){if(!arguments.length)throw new TypeError('A key is required');return __counts(this).get(__performanceString(key));}
  has(key){if(!arguments.length)throw new TypeError('A key is required');return __counts(this).has(__performanceString(key));}
  forEach(callback,thisArg=undefined){const data=__counts(this);if(typeof callback!=='function')throw new TypeError('A callback is required');data.forEach((value,key)=>callback.call(thisArg,value,key,this));}
  [Symbol.iterator](){return this.entries();}
};
const __counts=receiver=>{const data=__eventCountsData.get(receiver);if(!data)throw new TypeError('Illegal invocation');return data;};
Object.defineProperty(EventCounts.prototype,Symbol.toStringTag,{value:'EventCounts',configurable:true});
for(const key of Reflect.ownKeys(EventCounts.prototype)){if(key==='constructor'||key===Symbol.toStringTag)continue;const descriptor=Object.getOwnPropertyDescriptor(EventCounts.prototype,key);Object.defineProperty(EventCounts.prototype,key,{...descriptor,enumerable:true});}
const __eventCounts=Object.create(EventCounts.prototype);
__eventCountsData.set(__eventCounts,new Map(['auxclick','beforeinput','click','compositionend','compositionstart','compositionupdate','contextmenu','dblclick','dragend','dragenter','dragleave','dragover','dragstart','drop','gotpointercapture','input','keydown','keypress','keyup','lostpointercapture','mousedown','mouseenter','mouseleave','mouseout','mouseover','mouseup','pointercancel','pointerdown','pointerenter','pointerleave','pointerout','pointerover','pointerup','touchcancel','touchend','touchstart'].map(name=>[name,0])));
let __resourceTimingBufferSize=250,__resourceBufferFullHandler=null;
{
  const P=globalThis.performance||(globalThis.performance={});
  Object.defineProperties(P,{now:{value:__perfNow,configurable:true,writable:true},timeOrigin:{value:__perfTimeOrigin,configurable:true,enumerable:true},eventCounts:{value:__eventCounts,configurable:true,enumerable:true},interactionCount:{value:0,configurable:true,enumerable:true},onresourcetimingbufferfull:{get(){return __resourceBufferFullHandler;},set(value){__resourceBufferFullHandler=typeof value==='function'?value:null;},configurable:true,enumerable:true}});
  P.mark=function(name,options=undefined){if(!arguments.length)throw new TypeError('A mark name is required');const entry=new PerformanceMark(name,options);__performanceTimeline.push(entry);return entry;};
  P.measure=function(name,startOrOptions=undefined,endMark=undefined){
    if(!arguments.length)throw new TypeError('A measure name is required');name=__performanceString(name);
    let start=0,end=__perfNow(),detail=null;
    const options=startOrOptions&&typeof startOrOptions==='object'?startOrOptions:null;
    if(options&&['start','end','duration','detail'].some(key=>options[key]!==undefined)){
      const hasStart=options.start!==undefined,hasEnd=options.end!==undefined,hasDuration=options.duration!==undefined;
      if(endMark!==undefined||(!hasStart&&!hasEnd)||(hasStart&&hasEnd&&hasDuration))throw new TypeError('Invalid measure options');
      if(hasStart)start=__latestMark(options.start);if(hasEnd)end=__latestMark(options.end);
      if(hasDuration){const duration=__performanceNumber(options.duration);if(hasStart)end=start+duration;else start=end-duration;}
      detail=__performanceDetail(options.detail);
    }else{if(startOrOptions!==undefined&&!options)start=__latestMark(__performanceString(startOrOptions));if(endMark!==undefined)end=__latestMark(__performanceString(endMark));}
    const entry=Object.create(PerformanceMeasure.prototype);__performanceEntries.set(entry,{name,navigationId:__navigationId,entryType:'measure',startTime:start,duration:end-start,detail});__performanceTimeline.push(entry);return entry;
  };
  P.clearMarks=function(name=undefined){__clearPerformanceEntries('mark',name===undefined?undefined:__performanceString(name));};
  P.clearMeasures=function(name=undefined){__clearPerformanceEntries('measure',name===undefined?undefined:__performanceString(name));};
  P.getEntries=function(){return __performanceTimeline.slice().sort((a,b)=>a.startTime-b.startTime);};
  P.getEntriesByType=function(type){if(!arguments.length)throw new TypeError('An entry type is required');type=__performanceString(type);return P.getEntries().filter(entry=>entry.entryType===type);};
  P.getEntriesByName=function(name,type=undefined){if(!arguments.length)throw new TypeError('An entry name is required');name=__performanceString(name);if(type!==undefined)type=__performanceString(type);return P.getEntries().filter(entry=>entry.name===name&&(type===undefined||entry.entryType===type));};
  P.clearResourceTimings=function(){__clearPerformanceEntries('resource');};
  P.setResourceTimingBufferSize=function(size){if(!arguments.length)throw new TypeError('A buffer size is required');__resourceTimingBufferSize=Number(size)>>>0;};
  P.toJSON=function(){return {timeOrigin:__perfTimeOrigin};};
  const timing={};for(const name of __performanceFields)Object.defineProperty(timing,name,{get(){return __navigationPhases[name];},enumerable:true,configurable:true});timing.toJSON=function(){return Object.fromEntries(__performanceFields.map(name=>[name,__navigationPhases[name]]));};
  Object.defineProperty(P,'timing',{value:timing,configurable:true,enumerable:true});
  const navigation={type:0,redirectCount:0};for(const [name,value]of Object.entries({TYPE_NAVIGATE:0,TYPE_RELOAD:1,TYPE_BACK_FORWARD:2,TYPE_RESERVED:255}))Object.defineProperty(navigation,name,{value,enumerable:true});navigation.toJSON=function(){return {type:this.type,redirectCount:this.redirectCount};};
  Object.defineProperty(P,'navigation',{value:navigation,configurable:true,enumerable:true});
  Object.defineProperty(P,'memory',{get(){return ops.op_heap_statistics();},configurable:true,enumerable:true});
  __G.recordResourceTiming=(name,initiatorType,startTime,endTime,encodedSize=0)=>{
    const count=__performanceTimeline.filter(entry=>entry.entryType==='resource').length;
    if(count>=__resourceTimingBufferSize){if(__resourceBufferFullHandler)__resourceBufferFullHandler.call(P,new __hostEvent('resourcetimingbufferfull'));return;}
    const entry=Object.create(PerformanceEntry.prototype);__performanceEntries.set(entry,{name:String(name),navigationId:__navigationId,entryType:'resource',startTime,duration:Math.max(0,endTime-startTime)});Object.defineProperties(entry,{initiatorType:{value:String(initiatorType),enumerable:true},encodedBodySize:{value:encodedSize,enumerable:true},decodedBodySize:{value:encodedSize,enumerable:true}});__performanceTimeline.push(entry);
  };
}
// A quirks document scrolls through its body unless that body is itself a
// scroll container (CSSOM View), decided by the native layout. Like the Window
// offsets, this getter never throws: a document native layout rejects keeps the
// body, whose overflow then remains unproven.
__defineDocumentAccessor("scrollingElement", state => {
  const document = state.document;
  if (document.compatMode !== "BackCompat") return document.documentElement;
  const body = document.body;
  try {
    return body !== null && ops.op_live_scroll_container(body) ? null : body;
  } catch {
    return body;
  }
});
// window.scheduler — the Prioritized Task Scheduling API (Chrome). google's homepage reads
// window.scheduler; deno_core doesn't ship it. postTask runs the callback through the virtual
// timer queue (honoring `delay`) and resolves with its result; yield() defers to a macrotask.
if (typeof globalThis.scheduler === "undefined") {
  globalThis.scheduler = {
    postTask(cb, opts) {
      return new Promise((resolve, reject) => {
        const delay = (opts && +opts.delay) || 0;
        setTimeout(() => { try { resolve(typeof cb === "function" ? cb() : undefined); } catch (e) { reject(e); } }, delay);
      });
    },
    yield() { return new Promise((r) => setTimeout(r, 0)); },
  };
}
// Notification API: real Chrome exposes `Notification` with a `permission` static ("default"
// until granted). Missing `Notification`/`permission` is a headless tell google's homepage reads.
if (typeof globalThis.Notification === "undefined") {
  const N = function Notification() {};
  N.permission = "default";
  N.maxActions = 2;
  N.requestPermission = function (cb) { if (typeof cb === "function") cb("default"); return Promise.resolve("default"); };
  globalThis.Notification = N;
} else if (globalThis.Notification.permission === undefined) {
  try { globalThis.Notification.permission = "default"; } catch (e) {}
}
// CSS feature detection uses private native syntax and selector validation.
if (typeof globalThis.CSS === "undefined") {
  const cssString = String, cssTypeError = TypeError;
  const cssEscape = (value) => {
    const s = String(value);
    let out = "";
    for (let i = 0; i < s.length; i++) {
      const c = s.charCodeAt(i);
      if (c === 0) { out += "�"; continue; }
      // control chars, or a leading digit (or a digit right after a leading '-') → hex escape
      if ((c >= 0x1 && c <= 0x1f) || c === 0x7f ||
          (i === 0 && c >= 0x30 && c <= 0x39) ||
          (i === 1 && c >= 0x30 && c <= 0x39 && s.charCodeAt(0) === 0x2d)) {
        out += "\\" + c.toString(16) + " "; continue;
      }
      // a lone leading '-'
      if (i === 0 && c === 0x2d && s.length === 1) { out += "\\-"; continue; }
      // ident-safe: alphanumerics, '-', '_', and non-ASCII pass through unescaped
      if (c >= 0x80 || c === 0x2d || c === 0x5f ||
          (c >= 0x30 && c <= 0x39) || (c >= 0x41 && c <= 0x5a) || (c >= 0x61 && c <= 0x7a)) {
        out += s[i]; continue;
      }
      out += "\\" + s[i]; // everything else is backslash-escaped
    }
    return out;
  };
  globalThis.CSS = {
    supports(condition) {
      if (!arguments.length) throw new cssTypeError("Failed to execute 'supports' on 'CSS': 1 argument required, but only 0 present.");
      const text = value => { if (typeof value === "symbol") throw new cssTypeError("Cannot convert a Symbol value to a string"); return cssString(value); };
      const source = text(condition);
      return arguments.length > 1
        ? ops.op_native_css_supports(source, text(arguments[1]), true)
        : ops.op_native_css_supports(source, "", false);
    },
    escape: cssEscape,
  };
  // CSS Typed OM numeric factories (CSS.px() …) come from the binding; Chrome's namespace
  // lists its operations in code-unit order.
  const factories = globalThis[Symbol.for("ts")]?.typedOmFactories?.();
  if (factories) {
    const members = [["escape", globalThis.CSS.escape], ["supports", globalThis.CSS.supports], ...factories].sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
    const namespace = {};
    for (const [name, value] of members) Object.defineProperty(namespace, name, { value, writable: true, enumerable: true, configurable: true });
    Object.defineProperty(namespace, Symbol.toStringTag, { value: "CSS", configurable: true });
    Object.defineProperty(globalThis, "CSS", { value: namespace, writable: true, enumerable: false, configurable: true });
  }
}
// Encoding/crypto/base64 web globals deno_core doesn't ship but app bundles use.
// WHATWG encodings use a native incremental decoder, including BOM/fatal state.
{
 const encoders=new WeakSet(),decoders=new WeakMap();
 const record=receiver=>{const value=decoders.get(receiver);if(!value)throw new TypeError("Illegal invocation");return value;};
 const cleanup=typeof FinalizationRegistry==="function"?new FinalizationRegistry(id=>ops.op_text_decoder_drop(id)):null;
 const text=value=>{if(typeof value==="symbol")throw new TypeError("A Symbol cannot be converted to a string");return String(value);};
 globalThis.TextEncoder=class TextEncoder {
  constructor(){encoders.add(this);}
  get encoding(){if(!encoders.has(this))throw new TypeError("Illegal invocation");return "utf-8";}
  encode(input=""){if(!encoders.has(this))throw new TypeError("Illegal invocation");return ops.op_text_encode(text(input));}
  encodeInto(input,destination){if(!encoders.has(this))throw new TypeError("Illegal invocation");if(arguments.length<2)throw new TypeError("Two arguments required");const getter=Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype),Symbol.toStringTag).get;if(getter.call(destination)!=="Uint8Array")throw new TypeError("Destination must be a Uint8Array");input=text(input);let read=0,written=0;for(const char of input){const bytes=ops.op_text_encode(char);if(written+bytes.length>destination.length)break;destination.set(bytes,written);written+=bytes.length;read+=char.length;}return {read,written};}
 };
 globalThis.TextDecoder=class TextDecoder {
  constructor(label="utf-8",options={}){const value=ops.op_text_decoder_new(text(label),!!options.fatal,!!options.ignoreBOM);Object.assign(value,{fatal:!!options.fatal,ignoreBOM:!!options.ignoreBOM});decoders.set(this,value);cleanup?.register(this,value.id);}
  get encoding(){return record(this).encoding;}
  get fatal(){return record(this).fatal;}
  get ignoreBOM(){return record(this).ignoreBOM;}
  decode(input=undefined,options={}){const value=record(this);let bytes;if(input===undefined)bytes=new Uint8Array();else if(ArrayBuffer.isView(input))bytes=new Uint8Array(input.buffer,input.byteOffset,input.byteLength);else if(input instanceof ArrayBuffer || typeof SharedArrayBuffer==="function"&&input instanceof SharedArrayBuffer)bytes=new Uint8Array(input);else throw new TypeError("Input must be a BufferSource");return ops.op_text_decode(value.id,bytes,!!options.stream);}
 };
 for(const [ctor,name] of [[TextEncoder,"TextEncoder"],[TextDecoder,"TextDecoder"]])Object.defineProperty(ctor.prototype,Symbol.toStringTag,{value:name,configurable:true});
}
"##,
    include_str!("blob_text_stream.js"),
    include_str!("observable.js"),
    r##"
// The host installs native WebCrypto after the DOM environment is initialized.
// Channels in a realm deliver asynchronously through native EventTarget dispatch.
if (typeof globalThis.BroadcastChannel === "undefined") {
  const channels = new Map(), records = __interfaceMap("BroadcastChannel");
  const schedule = globalThis.setTimeout, clone = globalThis.structuredClone;
  const Message = globalThis.MessageEvent, dispatch = EventTarget.prototype.dispatchEvent;
  const listen = EventTarget.prototype.addEventListener, unlisten = EventTarget.prototype.removeEventListener;
  const record = channel => { const value=records.get(channel); if(!value)throw new TypeError("Illegal invocation");return value; };
  globalThis.BroadcastChannel = class BroadcastChannel extends EventTarget {
    constructor(name) {
      if (!arguments.length) throw new TypeError("A channel name is required");
      super(); name=__messageString(name);
      records.set(this,{name,closed:false,onmessage:null,onmessageerror:null});
      if(!channels.has(name))channels.set(name,new Set());channels.get(name).add(this);
    }
    get name(){return record(this).name;}
    get onmessage(){return record(this).onmessage;}
    set onmessage(value){const state=record(this);if(state.onmessage)unlisten.call(this,"message",state.onmessage);state.onmessage=typeof value==="function"?value:null;if(state.onmessage)listen.call(this,"message",state.onmessage);}
    get onmessageerror(){return record(this).onmessageerror;}
    set onmessageerror(value){const state=record(this);if(state.onmessageerror)unlisten.call(this,"messageerror",state.onmessageerror);state.onmessageerror=typeof value==="function"?value:null;if(state.onmessageerror)listen.call(this,"messageerror",state.onmessageerror);}
    postMessage(data) {
      const state=record(this);if(state.closed)throw new DOMException("Channel is closed","InvalidStateError");
      if(!arguments.length)throw new TypeError("A message is required");
      const snapshot=clone(data);
      for(const channel of channels.get(state.name)||[])if(channel!==this)schedule(()=>{
        if(!record(channel).closed)dispatch.call(channel,new Message("message",{data:clone(snapshot),origin:location.origin}));
      },0);
    }
    close(){const state=record(this);state.closed=true;channels.get(state.name)?.delete(this);}
  };
  Object.defineProperty(BroadcastChannel.prototype,Symbol.toStringTag,{value:"BroadcastChannel",configurable:true});
  for(const name of ["name","onmessage","onmessageerror","postMessage","close"]){const descriptor=Object.getOwnPropertyDescriptor(BroadcastChannel.prototype,name);Object.defineProperty(BroadcastChannel.prototype,name,{...descriptor,enumerable:true});}
}
if (typeof globalThis.btoa === "undefined") {
  const __B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  globalThis.btoa = (s) => {
    s = String(s); let out = "";
    for (let i = 0; i < s.length; i += 3) {
      const a = s.charCodeAt(i), b = s.charCodeAt(i + 1), c = s.charCodeAt(i + 2);
      const n = (a << 16) | ((isNaN(b) ? 0 : b) << 8) | (isNaN(c) ? 0 : c);
      out += __B64[(n >> 18) & 63] + __B64[(n >> 12) & 63] + (isNaN(b) ? "=" : __B64[(n >> 6) & 63]) + (isNaN(c) ? "=" : __B64[n & 63]);
    }
    return out;
  };
  globalThis.atob = (s) => {
    s = String(s).replace(/=+$/, ""); let out = "";
    for (let i = 0, bits = 0, val = 0; i < s.length; i++) {
      val = (val << 6) | __B64.indexOf(s[i]); bits += 6;
      if (bits >= 8) { bits -= 8; out += String.fromCharCode((val >> bits) & 0xff); }
    }
    return out;
  };
}
// Abort primitives come from the canonical private EventTarget implementation.
// ReadableStream — the RSC client reads the flight payload as a stream. A queue-backed
// impl supporting start/pull/cancel + getReader().read() {value,done}.
//
// CRITICAL for streaming producers (Next's RSC flight): the controller is filled
// ASYNCHRONOUSLY — `enqueue` is called as `__next_f` rows arrive and `close` fires on
// DOMContentLoaded, both LATER than the first `read()`. So a `read()` that finds the
// queue empty-but-open must NOT report EOF — it must PARK until the next enqueue/close.
// (Returning {done:true} there truncates the flight payload mid-stream → React keeps
// retrying the desynced reader → the render never converges.) Parked reads are held in
// `_waiters` and settled by enqueue/close/error.
if (typeof globalThis.ReadableStream === "undefined") {
  globalThis.ReadableStream = class ReadableStream {
    constructor(source = {}, _strategy) {
      this._q = [];
      this._closed = false;
      this._err = null;
      this._source = source || {};
      this._locked = false;
      this._waiters = []; // pending {resolve,reject} for reads that outran the producer
      const settleNext = () => {
        if (!this._waiters.length) return false;
        if (this._q.length) { this._waiters.shift().resolve({ value: this._q.shift(), done: false }); return true; }
        if (this._err) { this._waiters.shift().reject(this._err); return true; }
        if (this._closed) { this._waiters.shift().resolve({ value: undefined, done: true }); return true; }
        return false;
      };
      const drain = () => { while (settleNext()) {} };
      const c = {
        enqueue: (chunk) => { this._q.push(chunk); drain(); },
        close: () => { this._closed = true; drain(); },
        error: (e) => { this._err = e; this._closed = true; drain(); },
        get desiredSize() { return 1; },
      };
      this._ctrl = c;
      try { if (typeof this._source.start === "function") this._source.start(c); } catch (e) { this._err = e; }
    }
    get locked() { return this._locked; }
    getReader() {
      const self = this;
      if (self._locked) throw new TypeError("Stream is locked");
      self._locked = true;
      const pump = async () => {
        if (!self._q.length && !self._closed && typeof self._source.pull === "function") {
          await self._source.pull(self._ctrl);
        }
      };
      return {
        async read() {
          await pump();
          if (self._q.length) return { value: self._q.shift(), done: false };
          if (self._err) throw self._err;
          if (self._closed) return { value: undefined, done: true };
          // Empty but still open: park until enqueue/close/error settles us.
          return new Promise((resolve, reject) => self._waiters.push({ resolve, reject }));
        },
        releaseLock() { self._locked = false; },
        async cancel(r) { self._closed = true; if (typeof self._source.cancel === "function") await self._source.cancel(r); },
      };
    }
    async cancel(r) { this._closed = true; if (typeof this._source.cancel === "function") await this._source.cancel(r); }
    pipeThrough(t) { return t && t.readable ? t.readable : this; }
    pipeTo() { return Promise.resolve(); }
    tee() {
      const reader = this.getReader(), controllers = [], canceled = [false,false], reasons = [];
      let reading = false;
      const pull = async () => {
        if (reading) return; reading = true;
        try {const part = await reader.read();for(let i=0;i<2;i++)if(!canceled[i]){if(part.done)controllers[i].close();else controllers[i].enqueue(part.value instanceof Uint8Array ? part.value.slice() : part.value);}}
        catch(error){for(let i=0;i<2;i++)if(!canceled[i])controllers[i].error(error);}
        finally {reading = false;}
      };
      return [0,1].map(index => new ReadableStream({start(controller){controllers[index]=controller;},pull,cancel(reason){canceled[index]=true;reasons[index]=reason;if(canceled.every(Boolean))return reader.cancel(reasons);}}));
    }
  };
}
// requestIdleCallback must invoke the callback with an IdleDeadline ({didTimeout, timeRemaining()});
// the bare shim passed nothing, so `deadline.timeRemaining()` threw — a correctness bug + tell.
globalThis.requestIdleCallback = (fn) => globalThis.setTimeout(() => {
  const start = __perfNow();
  fn({ didTimeout: false, timeRemaining: () => Math.max(0, 50 - (__perfNow() - start)) });
}, 1);
globalThis.cancelIdleCallback = (id) => globalThis.clearTimeout(id);

// WHATWG URL + URLSearchParams — deno_core ships neither, but app bundles (Next.js,
// the PropelAuth SDK, …) use `new URL(...)` while hydrating, so without these the
// page crashes with "URL is not defined" before rendering. Native parsing handles
// Unicode hosts, dot segments and default-port normalization.
const __urlParamStates = __interfaceMap("URLSearchParams"), __urlParamOwners = __interfaceMap("URLSearchParams.owner");
const __urlPairs = receiver => {const pairs=__urlParamStates.get(receiver);if(!pairs)throw new TypeError("Illegal invocation");return pairs;};
const __urlParamsChanged = receiver => {const owner=__urlParamOwners.get(receiver);if(owner)owner();};
const __urlIterate = __webIDLIterator("URLSearchParams Iterator", __urlPairs);
if (typeof globalThis.URLSearchParams === "undefined") {
  globalThis.URLSearchParams = class URLSearchParams {
    constructor(init = "") {
      let pairs=[];
      if(init instanceof URLSearchParams)pairs=__urlPairs(init).map(pair=>pair.slice());
      else if(init != null && typeof init === "object") {
        if(typeof init[Symbol.iterator] === "function") for(const pair of init){const values=Array.from(pair);if(values.length!==2)throw new TypeError("Search parameter pairs require two values");pairs.push(values.map(String));}
        else pairs=Object.entries(init).map(([key,value])=>[key,String(value)]);
      } else pairs=ops.op_url_params_parse(String(init).replace(/^\?/,""));
      __urlParamStates.set(this,pairs);
    }
    append(key,value) {if(arguments.length<2)throw new TypeError("Two arguments required");__urlPairs(this).push([String(key),String(value)]);__urlParamsChanged(this);}
    delete(key,value = undefined) {if(!arguments.length)throw new TypeError("An argument required");key=String(key);__urlParamStates.set(this,__urlPairs(this).filter(pair=>pair[0]!==key||(value!==undefined&&pair[1]!==String(value))));__urlParamsChanged(this);}
    get(key) {if(!arguments.length)throw new TypeError("An argument required");return __urlPairs(this).find(pair=>pair[0]===String(key))?.[1]??null;}
    getAll(key) {if(!arguments.length)throw new TypeError("An argument required");return __urlPairs(this).filter(pair=>pair[0]===String(key)).map(pair=>pair[1]);}
    has(key,value = undefined) {if(!arguments.length)throw new TypeError("An argument required");return __urlPairs(this).some(pair=>pair[0]===String(key)&&(value===undefined||pair[1]===String(value)));}
    set(key,value) {if(arguments.length<2)throw new TypeError("Two arguments required");key=String(key);value=String(value);let found=false;const pairs=[];for(const pair of __urlPairs(this)){if(pair[0]!==key)pairs.push(pair);else if(!found){pairs.push([key,value]);found=true;}}if(!found)pairs.push([key,value]);__urlParamStates.set(this,pairs);__urlParamsChanged(this);}
    sort() {__urlPairs(this).sort((a,b)=>a[0]<b[0]?-1:a[0]>b[0]?1:0);__urlParamsChanged(this);}
    forEach(callback,thisArg = undefined) {if(typeof callback!=="function")throw new TypeError("A callback required");for(let index=0;index<__urlPairs(this).length;index++){const pair=__urlPairs(this)[index];callback.call(thisArg,pair[1],pair[0],this);}}
    keys() {return __urlIterate(this,"key");}
    values() {return __urlIterate(this,"value");}
    entries() {return __urlIterate(this,"entry");}
    get size() {return __urlPairs(this).length;}
    toString() {return ops.op_url_params_serialize(JSON.stringify(__urlPairs(this)));}
  };
  Object.defineProperty(URLSearchParams.prototype,Symbol.iterator,{value:URLSearchParams.prototype.entries,writable:true,configurable:true});
  Object.defineProperty(URLSearchParams.prototype,Symbol.toStringTag,{value:"URLSearchParams",configurable:true});
}
if (typeof globalThis.URL === "undefined") {
  const states = __interfaceMap("URL");
  const state = (receiver) => { const value = states.get(receiver); if (!value) throw new TypeError("Illegal invocation"); return value; };
  const parse = (input, base = "") => ops.op_surf_url_parse(String(input), base);
  const update = (receiver, key, value) => {
    const current = state(receiver);
    current.parts = parse(ops.op_url_update(receiver.href, key, String(value)));
    // URL.searchParams keeps its identity across URL mutations.
    const replacement = new URLSearchParams(current.parts.search);
    __urlParamStates.set(current.params,__urlPairs(replacement));
  };
  globalThis.URL = class URL {
    constructor(url, base = undefined) {
      if (!arguments.length) throw new TypeError("URL requires an input");
      const parts = parse(url, base === undefined ? "" : String(base));
      const params=new URLSearchParams(parts.search),current={parts,params};
      states.set(this,current);
      __urlParamOwners.set(params,()=>{current.parts=parse(ops.op_url_update(current.parts.href,"search",params.toString()));});
    }
    get href() {return state(this).parts.href;}
    set href(value) { const current = state(this), parts = parse(value); current.parts = parts; __urlParamStates.set(current.params,__urlPairs(new URLSearchParams(parts.search))); }
    get origin() { return state(this).parts.origin; }
    get searchParams() { return state(this).params; }
    get search() {return state(this).parts.search;}
    set search(value) { update(this, "search", value); }
    toString() { return this.href; }
    toJSON() { return this.href; }
    static canParse(url, base = undefined) { if (!arguments.length) throw new TypeError("URL requires an input"); try { new URL(url, base); return true; } catch { return false; } }
    static parse(url, base = undefined) { if (!arguments.length) throw new TypeError("URL requires an input"); try { return new URL(url, base); } catch { return null; } }
  };
  for (const key of ["protocol", "username", "password", "host", "hostname", "port", "pathname", "hash"]) {
    const descriptor = Object.getOwnPropertyDescriptor({get [key]() {return state(this).parts[key];}}, key);
    descriptor.set = Object.getOwnPropertyDescriptor({set [key](value) {update(this, key, value);}}, key).set;
    descriptor.enumerable = true;
    Object.defineProperty(URL.prototype, key, descriptor);
  }
  Object.defineProperty(URL.prototype, Symbol.toStringTag, {value: "URL", configurable: true});
  globalThis.URL.createObjectURL = () => "blob:turbo-surf";
  globalThis.URL.revokeObjectURL = () => {};
}
__BodyParamsString=URLSearchParams.prototype.toString;
"##,
    include_str!("event_handler_attributes.js"),
    include_str!("xhr.js"),
    include_str!("file_reader.js"),
    include_str!("websocket.js"),
    r##"
// Object-URL registry + download capture. The common client-side export pattern is
// `URL.createObjectURL(blob)` → set it on a `<a download href=…>` → `link.click()`. A real
// createObjectURL (keyed store) lets us recover the blob bytes when the anchor is clicked,
// and a wrapped HTMLAnchorElement.prototype.click records {filename, content} so the shim
// can resolve `page.waitForEvent('download')` + `download.path()`. Runs after browser_env
// installed the anchor prototype (ENV_BOOTSTRAP runs last).
(() => {
  const blobs = new Map();
  const decodeBlobBytes = TextDecoder.prototype.decode.bind(new TextDecoder());
  let bid = 0;
  globalThis.URL.createObjectURL = (obj) => {
    const url = "blob:turbo-surf/" + bid++;
    try { blobs.set(url, obj); } catch (_e) {}
    return url;
  };
  globalThis.URL.revokeObjectURL = () => {}; // keep the blob for a later .path() read
  __G.readBlobUrl = (url) => {
    const b = blobs.get(url);
    if (b == null) return null;
    return __readBlobBytes ? decodeBlobBytes(__readBlobBytes(b)) : "";
  };
  __G.downloads = __G.downloads || [];
  const record = (el) => {
    try {
      let n = el;
      while (n && n.nodeType === 1) {
        if (n.tagName === "A" && n.getAttribute) {
          let dl = n.getAttribute("download");
          if (dl == null && n.download) dl = n.download;
          if (dl != null) {
            const href = (n.getAttribute("href") || n.href || "");
            const content = __G.readBlobUrl(String(href));
            __G.downloads.push({ filename: dl || "download", url: String(href), content: content == null ? "" : content });
            return true;
          }
        }
        n = n.parentElement;
      }
    } catch (_e) {}
    return false;
  };
  // A download's default action runs after all cancellation handlers. The same
  // event can reach both the element and document listeners; distinct clicks
  // still represent distinct downloads, even when their URL is identical.
  const events = new WeakSet();
  const finish = (event) => {
    if (!event || events.has(event)) return;
    events.add(event);
    if (!event.defaultPrevented) record(event.target);
  };
  const capture = (event) => queueMicrotask(() => finish(event));
  const add = EventTarget.prototype.addEventListener;
  const remove = EventTarget.prototype.removeEventListener;
  add.call(document, "click", capture, true);
  const proto = globalThis.HTMLElement && globalThis.HTMLElement.prototype;
  if (proto && typeof proto.click === "function") {
    const orig = proto.click;
    const namespace = Object.getOwnPropertyDescriptor(Element.prototype, "namespaceURI").get;
    const click = function () {
      if (namespace.call(this) !== "http://www.w3.org/1999/xhtml") throw new TypeError("Illegal invocation");
      let dispatched;
      const observe = (event) => { dispatched = event; };
      add.call(this, "click", observe, true);
      try {
        const result = orig.call(this);
        finish(dispatched);
        return result;
      } finally { remove.call(this, "click", observe, true); }
    };
    Object.defineProperty(proto, "click", {value: __documentFunction(click, "click", 0),
      writable: true, enumerable: true, configurable: true});
    const anchor = globalThis.HTMLAnchorElement && globalThis.HTMLAnchorElement.prototype;
    if (anchor) delete anchor.click;
  }
})();

"##,
    include_str!("window_location.js"),
    r##"

// Document metadata is live and owned by Document.prototype; native body/head
// accessors keep their original tree callbacks. location remains an own property
// as in HTML documents, while inherited metadata checks the actual receiver.
__defineDocumentAccessor("referrer", state => state.referrer);
__defineDocumentAccessor("URL", state => state.inactive ? state.url : state.frame ? state.frame.location : state.window.location.href);
__defineDocumentAccessor("documentURI", state => state.inactive ? state.url : state.frame ? state.frame.location : state.window.location.href);
"##,
    include_str!("document_base.js"),
    include_str!("iframe_permissions_policy.js"),
    r##"
(() => {
  const d = globalThis.document;
  if (!d) return;
  try { if (typeof d.hasFocus !== "function") d.hasFocus = () => true; } catch (_) {}
  try { if (typeof d.open !== "function") d.open = () => d; } catch (_) {}
  try { if (typeof d.close !== "function") d.close = () => {}; } catch (_) {}
})();

// document.createTreeWalker + NodeFilter — focus-management code (MUI's DataGrid / focus
// trap, ARIA widgets) walks the tree with these; deno_core's binding has neither, so a
// page with a data grid threw "createTreeWalker is not a function" and rendered blank.
// A document-order DFS honoring whatToShow + the accept filter (REJECT skips the subtree,
// SKIP skips the node but descends) — enough for focusable-element scans.
(() => {
  const d = globalThis.document;
  if (!d || typeof d.createTreeWalker === "function") return;
  globalThis.NodeFilter = globalThis.NodeFilter || {
    SHOW_ALL: 0xffffffff, SHOW_ELEMENT: 1, SHOW_TEXT: 4, SHOW_COMMENT: 128,
    FILTER_ACCEPT: 1, FILTER_REJECT: 2, FILTER_SKIP: 3,
  };
  const ACCEPT = 1, REJECT = 2;
  // createTreeWalker(root, optional whatToShow, optional filter).length === 1.
  d.createTreeWalker = function (root) {
    const whatToShow = arguments[1], filter = arguments[2];
    const show = whatToShow == null ? 0xffffffff : whatToShow >>> 0;
    const accept = (n) => {
      const t = n.nodeType || 1;
      const bit = t === 1 ? 1 : t === 3 ? 4 : t === 8 ? 128 : 1 << (t - 1);
      if ((show & bit) === 0) return 3; // SKIP (wrong type)
      const fn = filter && (typeof filter === "function" ? filter : filter.acceptNode);
      if (typeof fn === "function") {
        try { return fn.call(filter, n); } catch (_e) { return 1; }
      }
      return 1;
    };
    const w = { root, whatToShow: show, filter, currentNode: root };
    w.nextNode = function () {
      let node = this.currentNode;
      for (;;) {
        let child = node.firstChild;
        let descend = true;
        while (descend && child) {
          node = child;
          const r = accept(node);
          if (r === ACCEPT) { this.currentNode = node; return node; }
          if (r === REJECT) { descend = false; } // don't descend; fall to sibling search
          else child = node.firstChild; // SKIP → keep descending
        }
        let t = node;
        while (t && t !== this.root) {
          if (t.nextSibling) { node = t.nextSibling; break; }
          t = t.parentNode;
        }
        if (!t || t === this.root) return null;
        const r = accept(node);
        if (r === ACCEPT) { this.currentNode = node; return node; }
      }
    };
    w.firstChild = function () {
      let c = this.currentNode.firstChild;
      while (c) { const r = accept(c); if (r === ACCEPT) { this.currentNode = c; return c; } c = c.nextSibling; }
      return null;
    };
    w.nextSibling = function () {
      let s = this.currentNode.nextSibling;
      while (s) { const r = accept(s); if (r === ACCEPT) { this.currentNode = s; return s; } s = s.nextSibling; }
      return null;
    };
    w.parentNode = function () {
      let p = this.currentNode.parentNode;
      while (p && p !== this.root) { if (accept(p) === ACCEPT) { this.currentNode = p; return p; } p = p.parentNode; }
      return null;
    };
    w.previousNode = function () { return null; }; // rarely used by focus scans
    return w;
  };
  // NodeIterator (same filter model, linear) — some libs use it instead of TreeWalker.
  if (typeof d.createNodeIterator !== "function") {
    d.createNodeIterator = function (root) {
      const whatToShow = arguments[1], filter = arguments[2];
      const tw = d.createTreeWalker(root, whatToShow, filter);
      return { nextNode: () => tw.nextNode(), previousNode: () => null, detach() {} };
    };
  }
})();

// Viewport / screen globals — no real rendering surface here, but analytics and
// responsive code read them (PostHog `.split`/`.height` on undefined throws → loops
// forever). Sensible desktop defaults.
(() => {
  const set = (k, v) => {
    if (typeof globalThis[k] === "undefined") {
      try { globalThis[k] = v; } catch (_e) {}
    }
  };
  // Window/viewport geometry, kept physically coherent with the screen:
  //   screen.height >= availHeight >= outerHeight >= innerHeight   (the window fits ON the
  // screen; the viewport fits IN the window under the tab strip + omnibox). Widths are equal
  // (Chrome has no left/right window chrome; the scrollbar lives inside inner). A window taller
  // than its screen — or equal inner/outer height — is impossible and a tell. `screen` was set
  // earlier in this bootstrap, so derive the defaults from it and clamp to fit.
  const __chromeH = 88; // ~40px tab strip + ~48px toolbar (no bookmarks bar)
  const __scr = globalThis.screen || {};
  const __availW = typeof __scr.availWidth === "number" ? __scr.availWidth : 1920;
  const __availH = typeof __scr.availHeight === "number" ? __scr.availHeight : 1055;
  // Outer window: default to the available screen area (a maximized-ish window), never larger.
  const __ow = Math.min(__pick("outerWidth", Math.min(1280, __availW)), __availW);
  const __oh = Math.min(__pick("outerHeight", Math.min(800 + __chromeH, __availH)), __availH);
  // Inner viewport: window minus chrome (height) and equal width; never larger than the window.
  const __iw = Math.min(__pick("innerWidth", __ow), __ow);
  const __ih = Math.min(__pick("innerHeight", Math.max(__oh - __chromeH, 0)), __oh);
  set("innerWidth", __iw);
  set("innerHeight", __ih);
  set("outerWidth", __ow);
  set("outerHeight", __oh);
  set("devicePixelRatio", 1);
  set("screenX", 0);
  set("screenY", 0);
  set("scrollX", 0);
  set("scrollY", 0);
  set("pageXOffset", 0);
  set("pageYOffset", 0);
  set("scroll", () => {});
  set("scrollTo", () => {});
  set("scrollBy", () => {});
  set("screen", {
    width: 1280, height: 800, availWidth: 1280, availHeight: 800,
    colorDepth: __colorDepth, pixelDepth: __pixelDepth,
    orientation: { type: "landscape-primary", angle: 0, addEventListener() {}, removeEventListener() {} },
  });
  // visualViewport tracks the layout viewport (unpinched) — must equal innerWidth/innerHeight,
  // else `visualViewport.width !== innerWidth` is an incoherence tell. Derive from the values
  // set just above rather than repeating a stale 1280x800.
  set("visualViewport", {
    width: __iw, height: __ih, scale: 1, offsetLeft: 0, offsetTop: 0, pageLeft: 0, pageTop: 0,
    addEventListener() {}, removeEventListener() {}, dispatchEvent() { return false; },
  });
})();
"##,
    include_str!("window_viewport.js"),
    r##"

// `Node.prototype.replaceChild(new, old)` — jQuery's `replaceWith`/`domManip` call
// it; the DOM binding omits it. All wrapped elements share one template prototype,
// so define `replaceChild` there (from the `insertBefore` + `removeChild` the
// binding does provide). Guard against polluting `Object.prototype`.
(() => {
  var d = globalThis.document;
  if (!d || typeof d.createElement !== "function") return;
  var probe = d.createElement("div");
  if (!probe || typeof probe.insertBefore !== "function") return;
  var proto = Object.getPrototypeOf(probe);
  if (!proto || proto === Object.prototype) return;
  if (typeof proto.replaceChild !== "function") {
    proto.replaceChild = function (newChild, oldChild) {
      this.insertBefore(newChild, oldChild);
      this.removeChild(oldChild);
      return oldChild;
    };
  }
})();

// DOM interface constructors — React / emotion / many bundles do
// `x instanceof HTMLElement` (etc.) at load. If the constructor global is
// undefined, `instanceof` throws ("Right-hand side of 'instanceof' is not an
// object") and aborts the ENTIRE script bundle (Nike's whole React app failed this
// way). Define them (only if absent) with a duck-typed `Symbol.hasInstance` so the
// checks resolve correctly against our wrapped nodes by `nodeType`, rather than
// throwing.
(() => {
  var def = function (k, pred) {
    if (typeof globalThis[k] !== "undefined") return;
    var C = function () {};
    try { Object.defineProperty(C, Symbol.hasInstance, { value: pred, configurable: true }); } catch (_e) {}
    try { globalThis[k] = C; } catch (_e) {}
  };
  var nodeAt = function (t) { return function (x) { return !!x && x.nodeType === t; }; };
  var element = nodeAt(1);
  def("EventTarget", function (x) { return !!x && typeof x.addEventListener === "function"; });
  def("Node", function (x) { return !!x && typeof x.nodeType === "number"; });
  def("Element", element);
  def("CharacterData", function (x) { return !!x && (x.nodeType === 3 || x.nodeType === 8); });
  def("Text", nodeAt(3));
  def("Comment", nodeAt(8));
  def("Document", nodeAt(9));
  def("DocumentFragment", nodeAt(11));
  def("Window", function (x) { return x === globalThis; });
  def("Event", function (x) { return !!x && typeof x.type === "string" && ("target" in x || "bubbles" in x); });
  // Every concrete HTML*Element frameworks branch on is just an Element here.
  [
    "HTMLElement", "HTMLUnknownElement", "HTMLDivElement", "HTMLSpanElement",
    "HTMLAnchorElement", "HTMLInputElement", "HTMLButtonElement", "HTMLImageElement",
    "HTMLIFrameElement", "HTMLCanvasElement", "HTMLFormElement", "HTMLScriptElement",
    "HTMLStyleElement", "HTMLLinkElement", "HTMLTemplateElement", "HTMLTextAreaElement",
    "HTMLSelectElement", "HTMLOptionElement", "HTMLUListElement", "HTMLOListElement",
    "HTMLLIElement", "HTMLHeadingElement", "HTMLParagraphElement", "HTMLTableElement",
    "HTMLTableRowElement", "HTMLTableCellElement", "HTMLLabelElement", "HTMLPreElement",
    "SVGElement", "SVGSVGElement",
  ].forEach(function (k) { def(k, element); });
})();

// `import.meta` shim. A turbopack/webpack DEV runtime injects scripts that read
// `import.meta.url` (and friends), but the pump first compiles ordinary scripts as
// classic V8 scripts, which reject `import.meta` with a SyntaxError ("Cannot use
// 'import.meta' outside a module"), which aborted the whole chunk. There's no module
// record to attach a real `import.meta` to, so expose a stand-in global the script
// rewrite below maps `import.meta` onto. `url` is the page URL; `env` is empty (no
// build-time define table headless); `resolve` echoes the spec back as an absolute URL.
__G.importMeta = {
  get url() { return (globalThis.location && globalThis.location.href) || ""; },
  env: {},
  resolve(spec) { try { return new globalThis.URL(spec, globalThis.location.href).href; } catch (_e) { return String(spec); } },
};

// esbuild's `keepNames` helper: transpiled code calls `__name(fn, "fn")` to restore
// Function.name after minification. esbuild emits a local `var __name = ...` per module,
// but injected/eval'd snippets (e.g. a test harness's tsx-transpiled addInitScript, or a
// bundle chunk that expects the helper hoisted) can reference it free. Provide a no-op
// passthrough global so such code doesn't ReferenceError. A module's own local `__name`
// shadows this; this only catches the free-reference case.
if (typeof globalThis.__name === "undefined") {
  globalThis.__name = function (fn) { return fn; };
}

// --- hydration pump: the browser's script-loading model -----------------------
// Real SPAs (Next.js/webpack) don't ship their code inline — they BOOT a tiny
// runtime that injects more <script src> chunks at runtime and waits for each
// chunk's `onload` before continuing (webpack's `__webpack_require__.e`). A node
// DOM that merely *appends* the <script> node never runs it, so the loader
// promise hangs and the app never mounts. So: execute each <script> element once
// (inline → host compilation; external → fetch then host compilation), and fire
// load/error so the loader resolves. `__G.hydrate()` drives this to quiescence.
const __resourceJSONParse = JSON.parse;
const __resourceThen = Function.prototype.call.bind(Promise.prototype.then);
// Parse order, including parser scripts inside declarative shadow roots.
const __initialResourceScripts = ops.op_parser_scripts(__document);
let __orderedDynamicScripts = Promise.resolve();
const __resources = new WeakMap();
const __resourcePromises = new Set();
const __resourceSheetConstructor = CSSStyleSheet;
const __resourceSheetRules = ops.op_interface_record_get(document).cssHost.readRules;
const __resourceStyleRequests = new WeakMap();
const __resourceCSSHosts = new WeakMap();
const __resourceCompatMode=Object.getOwnPropertyDescriptor(Document.prototype,"compatMode").get;
const __resourceAttribute=Element.prototype.getAttribute;
const __resourceTagName=Object.getOwnPropertyDescriptor(Element.prototype,"tagName").get;
const __resourceLocalName=Object.getOwnPropertyDescriptor(Element.prototype,"localName").get;
const __resourceIndexOf=Function.prototype.call.bind(Array.prototype.indexOf);
const __resourceListLength=Object.getOwnPropertyDescriptor(NodeList.prototype,"length").get;
const __resourceListItem=NodeList.prototype.item;
const __resourceListApply=Reflect.apply;
const __resourceScriptAsync=Object.getOwnPropertyDescriptor(HTMLScriptElement.prototype,"async").get;
const __resourceText=Object.getOwnPropertyDescriptor(Node.prototype,"textContent").get;
const __resourceNativeOwner=Function.prototype.call.bind(Object.getOwnPropertyDescriptor(Node.prototype,"ownerDocument").get);
const __resourceNativeBase=Function.prototype.call.bind(Object.getOwnPropertyDescriptor(Node.prototype,"baseURI").get);
const __resourceDocumentQuery=Document.prototype.querySelectorAll;
const __resourceCompare=Node.prototype.compareDocumentPosition;
let __resourceMutationEpoch = 0;
const __resourceBaseURL=Object.getOwnPropertyDescriptor(Node.prototype,"baseURI").get;
"##,
    include_str!("compression_dictionary.js"),
    r##"
function __cssHost(doc) {let host=__resourceCSSHosts.get(doc);if(!host){host=ops.op_interface_record_get(doc).cssHost;__resourceCSSHosts.set(doc,host);}return host;}
function __cssOriginClean(response) {return response.originClean===true;}
function __validSheetResponse(response, doc, clean) {const mime=(response.contentType||"").split(";")[0].trim().toLowerCase();return mime==="text/css" || !response.nosniff && (__resourceCompatMode.call(doc)==="BackCompat" && clean || __cssHost(doc).looksLikeCSS(response.text));}
function __fireResourceEvent(el, kind, win = globalThis) {
  let ev;
  try { ev = new __hostEvent(kind); } catch (_) { ev = {type:kind}; }
  try { ops.op_dispatch_host_event(ev, el); } catch (_) {}
}
function __trackResource(promise) {
  __resourcePromises.add(promise);
  promise.finally(() => __resourcePromises.delete(promise));
  return promise;
}
function __resourceNow() { return __perfNow(); }
function __recordResource(win, url, kind, start) {
  const ns = win[Symbol.for("ts")];
  if (ns && typeof ns.recordResourceTiming === "function") ns.recordResourceTiming(url, kind, start, __resourceNow(win));
}
async function __loadSheetImports(sheet, source, win, depth = 0) {
  if (!sheet || depth >= 16) return;
  const request=__resourceStyleRequests.get(sheet)||{mode:"no-cors",credentials:"include",document:__document};
  const rules=__resourceSheetRules(sheet);
  for(let index=0;index<rules.length;index++) {
    const rule=rules[index];
    const info=__cssHost(request.document).readRule(rule);
    if (info.type !== 3 || !info.href) continue;
    const url = new URL(info.href, source).href;
    const start = __resourceNow(win);
    let recorded = false;
    try {
      const response = __resourceResponse(await __fetchFromSource(url, {destination:"style",mode:"no-cors",credentials:request.credentials}, source));
      if (!response.ok || !__validSheetResponse(response,request.document,__cssOriginClean(response,request.document.URL,"no-cors"))) { __recordResource(win, url, "css", start); recorded=true; continue; }
      const text = response.text;
      __recordResource(win, url, "css", start);recorded=true;
      const child = new __resourceSheetConstructor();
      __cssHost(request.document).attachImport(rule,child,response.url||url,text,request.originClean!==false && __cssOriginClean(response,request.document.URL,"no-cors"));
      __resourceStyleRequests.set(child,{...request,originClean:request.originClean!==false&&__cssOriginClean(response,request.document.URL,"no-cors")});
      await __loadSheetImports(child, url, win, depth + 1);
    } catch (_) { if(!recorded)__recordResource(win, url, "css", start); }
  }
}
function __loadImage(el, win = globalThis) {
  const src = el.getAttribute("src") || "";
  const previous = __resources.get(el);
  if (previous && previous.src === src) return previous.promise;
  const state = {src,complete:!src,width:0,height:0,currentSrc:"",promise:null};
  __resources.set(el,state);
  if (!src) return Promise.resolve();
  const doc = __resourceNativeOwner(el);
  const source = __resourceNativeBase(el);
  state.promise = __trackResource((async () => {
    let result;
    const url = new URL(src, source).href, start = __resourceNow(win);
    try {
      await 0;
      if (ops.op_csp_request(doc,url,"img-src","")) result = await ops.op_load_image(url, source,ops.op_csp_context(doc),JSON.stringify({...ops.op_document_cookie_site(doc),opaque_origin:ops.op_window_opaque(doc)}));
    } catch (_) {}
    __recordResource(win, url, "img", start);
    if (__resources.get(el) !== state) return;
    state.complete=true;
    if (result && result.ok) {state.width=result.width;state.height=result.height;state.currentSrc=result.url;}
    __fireResourceEvent(el, result && result.ok ? "load" : "error", win);
  })());
  return state.promise;
}
function __patchImage(el, win = globalThis) {
  if (el.__tcImagePatched) return;
  el.__tcImagePatched=true;
  for (const [name, key, fallback] of [["complete","complete",true],["naturalWidth","width",0],["naturalHeight","height",0],["currentSrc","currentSrc",""]]) {
    Object.defineProperty(el,name,{configurable:true,get(){const state=__resources.get(this);return state?state[key]:fallback;}});
  }
  Object.defineProperty(el,"src",{configurable:true,get(){const src=this.getAttribute("src")||"";try{return src?new URL(src,(this.ownerDocument&&this.ownerDocument.URL)||win.location.href).href:"";}catch(_){return src;}},set(value){this.setAttribute("src",String(value));__loadImage(this,win);}});
  el.decode=function(){return __loadImage(this,win).then(()=>{if(!this.naturalWidth){const error=new Error("The source image cannot be decoded");error.name="EncodingError";throw error;}});};
}
const __createResourceElement = document.createElement.bind(document);
document.createElement = function(tag, options) {
  const el=__createResourceElement(tag,options);
  if (String(tag).toLowerCase()==="img") __patchImage(el);
  return el;
};
globalThis.Image = function Image(width,height) {
  const el=document.createElement("img");
  if(width!=null)el.width=width;if(height!=null)el.height=height;
  return el;
};
globalThis.Image.prototype=globalThis.HTMLImageElement.prototype;
__G.loadDocumentResources = function(doc = __document, win = globalThis, before = null) {
  if(doc!==__document || __documentType.call(doc)!==9)throw new TypeError("Resource loading requires the owning native Document");
  __discoverDictionaries();
  const styles=[];
  // The query is already in document order. Avoid a whole-tree position walk
  // for each parser resource, unless a resource hook changed that order.
  const epoch=__resourceMutationEpoch;
  const selected=__resourceListApply(__resourceDocumentQuery,doc,[before?"link,img,style,script":"link,img,style"]);
  const length=__resourceListApply(__resourceListLength,selected,[]);
  const elements={__proto__:null,length};
  for(let index=0;index<length;index++)elements[index]=__resourceListApply(__resourceListItem,selected,[index]);
  const cutoff=before?__resourceIndexOf(elements,before):-1;
  for (let index=0;index<elements.length;index++) {
    const el=elements[index];
    if(before){
      if(cutoff>=0&&epoch===__resourceMutationEpoch){if(index>=cutoff)break;}
      else if((__resourceCompare.call(el,before)&4)===0)continue;
    }
    const tag=__resourceTagName.call(el);
    if(tag==="SCRIPT"||__resourceLocalName.call(el)==="script")continue;
    if (tag==="IMG") {__patchImage(el,win);__loadImage(el,win);continue;}
    if(tag==="STYLE"){
      const sheet=__cssHost(doc).sheet(el);if(!sheet)continue;
      const source=__resourceText.call(el);let state=__resources.get(el);
      if(!state||state.src!==source){state={src:source,promise:null};__resources.set(el,state);__resourceStyleRequests.set(sheet,{mode:"no-cors",credentials:"include",document:doc,originClean:true});state.promise=__trackResource(__loadSheetImports(sheet,__resourceBaseURL.call(doc)||ops.op_document_request_url(),win));}
      styles.push(state.promise);continue;
    }
    if (!(__resourceAttribute.call(el,"rel")||"").toLowerCase().split(/\s+/).includes("stylesheet") || el.disabled) continue;
    const href=__resourceAttribute.call(el,"href");if(!href)continue;
    const url=new URL(href,__resourceBaseURL.call(doc)||ops.op_document_request_url()).href;
    let state=__resources.get(el);
    if(!state||state.src!==url){
      state={src:url,promise:null};__resources.set(el,state);
      state.promise=__trackResource((async()=>{
        const start = __resourceNow(win);
        let recorded = false,receivedResponse=false;
        try {
          const crossOrigin=__resourceAttribute.call(el,"crossorigin");
          const request={mode:crossOrigin===null?"no-cors":"cors",credentials:crossOrigin===null||crossOrigin.toLowerCase()==="use-credentials"?"include":"same-origin",document:doc};
          const response=__resourceResponse(await __fetchFromSource(url,{destination:"style",mode:request.mode,credentials:request.credentials,referrerPolicy:__resourceAttribute.call(el,"referrerpolicy")||"",integrity:__resourceAttribute.call(el,"integrity")||""},ops.op_document_request_url()));
          receivedResponse=true;
          const text=response.text;
          __recordResource(win, url, "link", start);recorded=true;
          if(!response.ok||!__validSheetResponse(response,doc,__cssOriginClean(response,doc.URL||win.location.href,request.mode)))throw new Error("Stylesheet request failed");
          if(__resources.get(el)!==state)return;
          const sheet=__cssHost(doc).attachLinked(el,text,response.url||url,__cssOriginClean(response,doc.URL||win.location.href,request.mode));
          __resourceStyleRequests.set(sheet,{...request,originClean:__cssOriginClean(response,doc.URL||win.location.href,request.mode)});
          await __loadSheetImports(sheet,url,win);
          __fireResourceEvent(el,"load",win);
        }catch(error){if(!recorded)__recordResource(win,url,"link",start);if(__resources.get(el)===state){const failure=__resourceFailure(error);if(!failure?.integrityBlocked&&(receivedResponse||failure?.responseReceived&&!failure.policyBlocked))__cssHost(doc).attachLinked(el,"",url,false);__fireResourceEvent(el,"error",win);}}
      })());
    }
    styles.push(state.promise);
  }
  return __framePromiseAll(styles);
};
__G.pendingResourceCount=()=>__resourcePromises.size+__dictionaryPending+__dictionaryTaskCount;
__G.documentLoadReady=()=>__resourcePromises.size===0 && ops.op_frame_pending()===0;
__G.waitForDocumentResources=()=>__framePromiseAll(Array.from(__resourcePromises));
function __fireScriptEvent(el, kind, err) {
  // Use the same private native event path as image and stylesheet loads.
  // Plain records are not Events and cannot reach addEventListener handlers.
  __fireResourceEvent(el, kind, globalThis);
}
// Rewrite `import.meta` (a SyntaxError in a classic script) onto the namespaced importMeta
// stub the dev HMR runtime reads (`.url`/`.env`). Whether a chunk is a REAL ES module is
// NOT decided by a regex here — a regex matches `import`/`export` inside comments + strings
// too (e.g. a vendored package's JSDoc `import {X} from 'y'`), which would wrongly route a
// CLASSIC turbopack chunk through the deno_core module pump and load it in a SEPARATE
// module graph → a DUPLICATE module instance (a second react-dom, whose event-system keys
// don't match the DOM's → portal/delegated onClick never fires). Instead `__G.execScriptEl`
// first queues classic compilation; only a genuine module-syntax SyntaxError (thrown at PARSE, before any
// code runs) routes the chunk to the module pump. Let V8 be the parser, not a regex.
__G.rewriteEsmForClassic = function (code) {
  if (typeof code !== "string" || !code) return code;
  if (/import\s*\.\s*meta/.test(code)) {
    code = code.replace(/import\s*\.\s*meta/g, "globalThis[Symbol.for('ts')].importMeta");
  }
  return code;
}
ops.op_set_host_capabilities({
  reportClassicScriptError:(error,details)=>{
    const event=new __hostErrorEvent("error",{cancelable:true,error,...details});
    ops.op_dispatch_host_event(event,globalThis);
  }});
const __frameScriptElements=Array.from(__resourceDocumentQuery.call(__document,"script"));
ops.op_set_host_capabilities({fireFrameScript:(index,kind)=>{const element=__frameScriptElements[index];if(element)__fireScriptEvent(element,kind);},frameScriptElement:index=>__frameScriptElements[index] || null,frameScriptDocument:()=>__document});
const __moduleStaging = ops.op_module_staging_enabled();
const __moduleWaiters = new Map();
__G.cancelModuleGraphs = () => {for(const waiters of __moduleWaiters.values()){for(const waiter of waiters){if(!waiter.done){waiter.done=true;waiter.resolve();}}}__moduleWaiters.clear();};
function __reportModuleError(waiter, details) {
  "use strict";
  if (waiter.reported) return;
  const error = details?.error || waiter.error;
  if (!error) return;
  waiter.reported = true;
  const location = String(error.stack || "").match(/(https?:\/\/[^\s)]+):(\d+):(\d+)/);
  const event = new __hostErrorEvent("error", {
    cancelable:true, error,
    message:"Uncaught " + String(error),
    filename:details?.filename || location?.[1] || waiter.specifier,
    lineno:details?.lineno || (location ? Number(location[2]) : 0),
    colno:details?.colno || (location ? Number(location[3]) : 0),
  });
  ops.op_dispatch_host_event(event, globalThis);
}
function __completeModuleWaiter(waiter, ready, details) {
  if (waiter.done) return;
  waiter.done = true; waiter.ready = ready;
  if (ready) {
    __reportModuleError(waiter, details);
    if (waiter.external) __fireScriptEvent(waiter.element, "load");
  } else __fireScriptEvent(waiter.element, "error");
  waiter.resolve();
}
const __completeModuleGraph = (specifier, ready, details) => {
  const waiters = __moduleWaiters.get(specifier);
  if (!waiters) return;
  __moduleWaiters.delete(specifier);
  for (const waiter of waiters) __completeModuleWaiter(waiter, ready, details);
};
ops.op_set_host_capabilities({completeModuleGraph:__completeModuleGraph});
function __resourceScriptAttributes(prepared) {
  const attributes = __recordCreate(null);
  for (let index = 0; index < prepared.attributes.length; index++) {
    const pair = prepared.attributes[index];
    attributes[pair[0]] = pair[1];
  }
  return attributes;
}
function __prepareResourceScript(el) {
  return ops.op_prepare_resource_script(__document, el);
}
__G.execScriptEl = async function (el, preparation = null, executionGate = null) {
  if (!el || !ops.op_document_scripts_allowed(__document)) return;
  const prepared = preparation || __prepareResourceScript(el);
  if (!prepared) return;
  const attributes = __resourceScriptAttributes(prepared);
  if (prepared.kind === "importmap" || prepared.kind === "speculationrules") return;
  if (prepared.src === "") { __fireScriptEvent(el, "error"); return; }
  const get = (name) => attributes[name] ?? null;
  // Module-capable browsers SKIP `<script nomodule>` (they run the module build
  // instead). We support module scripts, so honor it: otherwise we force-run a
  // page's legacy polyfill bundle (e.g. Next's core-js `polyfill-nomodule`), which
  // overwrites native Promise/queueMicrotask with impls whose microtask scheduler
  // is inert in this env — promises never settle and the render never commits.
  if (prepared.kind !== "module" && get("nomodule") !== null) return;
  const parserType=prepared.kind;
  if(prepared.parserInserted) {
    // Claim above before awaiting styles, so load handlers cannot run this script twice.
    // Later parser resources cannot dispatch before this script installs its listeners.
    const external=!!get("src"),module=parserType==="module";
    const deferred=!prepared.asynchronous&&(module||external&&get("defer")!==null);
    const resources=__G.loadDocumentResources(__document,globalThis,deferred?null:el);
    if(!prepared.asynchronous||!(external||module))await resources;
  }
  const absoluteSource=get("src")?new URL(get("src"),__baseApply(__resourceBaseURL,__document,[])).href:"";
  const parserIndex=Array.prototype.indexOf.call(document.querySelectorAll("script"),el);
  if(!ops.op_csp_script(__document,el,parserIndex<0?4294967295:parserIndex,absoluteSource,prepared.source)){
    if(absoluteSource)__fireScriptEvent(el,"error");return;
  }
  // ES modules (`<script type="module">`) run through the Rust module pump (a real
  // module graph + loader), with concurrent graph loading rather than classic eval.
  if (prepared.kind === "module") {
    const src = get("src");
    const specifier = src ? new URL(src, __baseApply(__resourceBaseURL,__document,[])).href
      : ops.op_inline_module(prepared.source, __baseApply(__resourceBaseURL,__document,[]), document.URL);
    ops.op_module_credentials(el,specifier);
    // The SDK loader observes actual graph/evaluation transitions independently
    // of import()'s promise, which waits for top-level await. The published SDK
    // fallback still waits for that promise and has partial TLA lifecycle support.
    const waiter = {element:el, external:!!src, specifier, done:false, ready:false, error:null, reported:false};
    const gate = new Promise(resolve => {waiter.resolve = resolve;});
    if (__moduleStaging) {
      const waiters = __moduleWaiters.get(specifier) || [];
      waiters.push(waiter); __moduleWaiters.set(specifier, waiters);
    }
    import(specifier).then(() => {
      if (!__moduleStaging) __completeModuleWaiter(waiter, true, null);
    }, error => {
      waiter.error = error;
      if (!__moduleStaging) __completeModuleWaiter(waiter, false, null);
      else if (waiter.done && waiter.ready) __reportModuleError(waiter, null);
      __print("module error (" + specifier + "): " + error + "\n");
    });
    return __trackResource(gate);
  }
  // Native preparation has already checked all classic JavaScript MIME aliases.
  const src = get("src");
  let downloaded=false,mutedErrors=false;
  try {
    let code, sourceURL=document.URL;
    if (src) {
      const abs = new URL(src, globalThis.location.href).href;
      const start = __resourceNow(globalThis);
      try {
        const cross=get("crossorigin");
        const res = await __fetchFromSource(abs, { destination: "script", mode: cross===null?"no-cors":"cors", credentials:cross!==null&&cross.toLowerCase()!=="use-credentials"?"same-origin":"include",nonce:ops.op_csp_nonce(el),integrity:get("integrity")||"" }, ops.op_document_request_url());
        const resource=__resourceResponse(res);
        if (!resource.ok) { __fireScriptEvent(el, "error"); return; }
        code = resource.text; sourceURL=resource.url; downloaded=true; mutedErrors=resource.mutedErrors;
      } finally { __recordResource(globalThis, abs, "script", start); }
    } else {
      code = prepared.source;
    }
    if (executionGate) await executionGate;
    // Legacy classic bundles retain their import.meta compatibility rewrite.
    // Import/export parse failures move the original text to Deno's real graph;
    // explicit module elements use the separate standards path above.
    const __orig = code;
    code = __G.rewriteEsmForClassic(code);
    // Set document.currentScript to THIS element during execution, like a browser.
    // Turbopack/webpack chunk runtimes do `TURBOPACK.push([document.currentScript, …])`
    // to correlate each chunk with the element that loaded it — a single static
    // currentScript makes every chunk look identical and the module graph never
    // resolves. Restore the prior value after (nested injects during eval).
    try {
      await ops.op_queue_classic_script(__document, code, sourceURL, !src, mutedErrors, el, prepared.inShadowTree);
    } catch (e) {
      // A genuine ES module? Classic V8 throws a module-syntax SyntaxError at PARSE time —
      // before ANY code runs — so it's safe to re-run through the module pump (a real module
      // graph + loader). This is the ONLY signal we route on: text that merely LOOKS like
      // import/export (in a comment/string of a classic turbopack chunk) parses + runs fine
      // here, so it stays classic (avoids a duplicate module instance — see __G.rewriteEsmForClassic).
      const msg = String((e && e.message) || e);
      if (e instanceof SyntaxError && /\b(import|export)\b|module/i.test(msg)) {
        // Keep the element: the module pump points document.currentScript at it during eval
        // so turbopack TURBOPACK.push([document.currentScript, …]) derives the right path.
        const abs = src ? sourceURL : "";
        (__G.esmSrcQueue || (__G.esmSrcQueue = [])).push({ src: abs, code: __orig, el: el });
        __fireScriptEvent(el, "load");
        return;
      }
      throw e; // real runtime error
    }
    if (src) __fireScriptEvent(el, "load");
  } catch (e) {
    if(src)__fireScriptEvent(el,downloaded?"load":"error");
    __print("script error (" + (src || "inline") + "): " + e + "\n");
  }
};
function __startResourceScript(element, prepared) {
  if (!prepared || prepared.kind === "importmap" || prepared.kind === "speculationrules") return;
  const attributes = __resourceScriptAttributes(prepared);
  if (prepared.kind !== "module" && __recordOwn(attributes,"nomodule")) return;
  if (prepared.src === "") { __fireScriptEvent(element,"error"); return; }
  if (!prepared.src && prepared.kind !== "module" && !prepared.parserInserted) {
    if (!ops.op_document_scripts_allowed(__document) || !ops.op_csp_script(__document, element, 4294967295, "", prepared.source)) return;
    ops.op_execute_inline_script(__document, element, prepared.source, ops.op_document_request_url(), prepared.inShadowTree);
    return;
  }
  if (prepared.src && prepared.kind !== "module" && !prepared.parserInserted && !prepared.asynchronous) {
    const next = __G.execScriptEl(element, prepared, __orderedDynamicScripts);
    __orderedDynamicScripts = __resourceThen(next, undefined, () => {});
    __trackResource(next);
  } else __trackResource(__G.execScriptEl(element, prepared));
}
ops.op_set_host_capabilities({retireResourceDocument:()=>{__initialResourceScripts.length=0;__frameScriptElements.length=0;__retireCspViolationTasks();__dictionaryRetireTasks();ops.op_retire_resource_document(__document);ops.op_install_resource_consumer(__document,event=>__documentBaseMutation(event,()=>{}),__documentRuntimeState);}});
const __cspStyleSheetGetter=(()=>{
  for(let prototype=HTMLStyleElement.prototype;prototype;prototype=Object.getPrototypeOf(prototype)){
    const descriptor=Object.getOwnPropertyDescriptor(prototype,"sheet");
    if(descriptor && descriptor.get)return descriptor.get;
  }
  return null;
})();
const __cspStyleQuery=Element.prototype.querySelectorAll;
function __prepareCspStyleMutation(event){
  if(!__cspStyleSheetGetter)return;
  const prepare=element=>{if(__htmlPolicyElement(element,"style"))__baseApply(__cspStyleSheetGetter,element,[]);};
  for(let index=0;index<event.ancestors.length;index++)prepare(event.ancestors[index]);
  if(event.type==="childList")for(let index=0;index<event.added.length;index++){
    const node=event.added[index];
    if(__baseApply(__documentType,node,[])!==1)continue;
    prepare(node);
    const children=__baseApply(__cspStyleQuery,node,["style"]);
    for(let child=0;child<children.length;child++)prepare(children[child]);
  }
}
__initializeDocumentBase();
const __loadMutatedResources = __G.loadDocumentResources;
const __resourceApply = Reflect.apply;
const __resourceElementQuery = Element.prototype.querySelector;
const __resourceMutationAttributes = {__proto__: null, src:true, href:true, rel:true,
  type:true, media:true, disabled:true, integrity:true, crossorigin:true, referrerpolicy:true};
function __resourceElementKind(node) {
  return __resourceApply(__documentType,node,[]) === 1
    ? __resourceApply(__resourceTagName,node,[]) : "";
}
function __containsInsertedResource(node) {
  const kind=__resourceElementKind(node);
  return kind === "LINK" || kind === "IMG" || kind === "STYLE" ||
    kind !== "" && __resourceApply(__resourceElementQuery,node,["link,img,style"]) !== null;
}
function __resourceMutation(event) {
  if(event.type==="childList")for(let index=0;index<event.removed.length;index++)__dictionaryRemoved(event.removed[index]);
  let connected = false, relevant = false;
  for (let index = 0; index < event.ancestors.length; index++) {
    const ancestor=event.ancestors[index];
    if (ancestor === __document) connected = true;
    if (event.type === "characterData" && __resourceElementKind(ancestor) === "STYLE") relevant=true;
  }
  if (!connected) return;
  if (event.type === "attributes") {
    const kind=__resourceElementKind(event.target);
    relevant = (kind === "LINK" || kind === "IMG" || kind === "STYLE") &&
      __recordOwn(__resourceMutationAttributes,event.attributeName);
  } else if (event.type === "childList") {
    relevant=__resourceElementKind(event.target) === "STYLE";
    for (let index=0; !relevant && index<event.added.length; index++)
      relevant=__containsInsertedResource(event.added[index]);
  }
  // Native IO can resume after a hydration round has ended. Admit newly
  // connected resources at their mutation; ordinary text/attribute edits do
  // not trigger a whole-document resource scan.
  if (relevant) __loadMutatedResources();
}
ops.op_install_resource_consumer(__document, event => {
  __resourceMutationEpoch++;
  __prepareCspStyleMutation(event);
  __documentBaseMutation(event,element=>{
    const prepared=__prepareResourceScript(element);
    if(prepared)__startResourceScript(element,prepared);
  });
  __resourceMutation(event);
}, __documentRuntimeState);
// Run every not-yet-run <script> in DOM order, drain timers, repeat while new
// scripts appear or timers keep firing. Bounded by maxRounds (+ the render budget).
__G.hydrate = async function (maxRounds = 300, timerBudget = 200000, fireTimers = true) {
  let timersLeft = timerBudget; // total timer-callback budget across rounds — an app
  // whose scheduler never reaches idle (e.g. React polling a backend that never
  // answers) would otherwise spin until the render budget; cap it and return the
  // best-effort DOM rendered so far.
  for (let round = 0; round < maxRounds && timersLeft > 0; round++) {
    let ranScript = false;
    // Honor `defer`: a browser runs parser-blocking scripts as it reaches them and
    // holds `defer` scripts until after parsing, THEN runs them in document order.
    // Running in raw DOM order instead breaks the common pattern where an app's
    // `<script defer>` bundle sits EARLY in <head> but its (non-defer) framework
    // vendor script sits later — e.g. Nike defers `main.js` (byte ~88k) while
    // `react.js` is non-defer near </body> (~608k). Raw order runs `main` first →
    // `React is not defined` → the whole app never hydrates. So: non-defer scripts
    // first (document order), then `defer` scripts (document order). `async` isn't
    // deferred. New scripts injected mid-round re-partition on the next pass.
    const all = __initialResourceScripts.splice(0);
    if(!__G.isFrame)__frameScriptElements.length=0;
    const isDeferred = element => __htmlPolicyElement(element,"script") && !__resourceScriptAsync.call(element) &&
      ((__resourceAttribute.call(element,"type") || "").trim().toLowerCase() === "module" ||
        __resourceAttribute.call(element,"src") !== null && __resourceAttribute.call(element,"defer") !== null);
    const ordered = all.filter(element => !isDeferred(element)).concat(all.filter(isDeferred));
    for (const element of ordered) {
      const prepared = __prepareResourceScript(element);
      if (!prepared) continue;
      ranScript = true;
      if (__G.initialLoading && prepared.parserInserted && isDeferred(element)) __markDocumentInteractive();
      if (prepared.asynchronous && (prepared.src || prepared.kind === "module")) __trackResource(__G.execScriptEl(element, prepared));
      else await __G.execScriptEl(element, prepared);
    }
    __G.loadDocumentResources();
    if (__G.initialLoading) __markDocumentInteractive();
    const fired = fireTimers ? __G.runTimers(Math.min(timersLeft, 5000)) : 0;
    timersLeft -= fired;
    if (fired) await undefined;
    if (!ranScript && fired === 0) break;
  }
};
// The compatibility module queue contains only native-prepared classic scripts
// whose V8 parse failed on module syntax. Explicit modules use the import path.
__G.takeModuleScript = function () {
  // src chunks whose body was ESM, already fetched + queued by __G.execScriptEl. Both
  // `src` (its URL identity, for import resolution) and `code` (the fetched body) set.
  const q = __G.esmSrcQueue;
  if (q && q.length) {
    const item = q.shift();
    __G.currentModuleEl = item.el || null; // for document.currentScript during eval
    __G.moduleDescriptor = JSON.stringify({ src: item.src, code: item.code });
    return;
  }
  __G.currentModuleEl = null;
  __G.moduleDescriptor = "";
};
// getByRole/getByText/getByLabel resolved IN the LIVE isolate, returning each match's
// document-order index (querySelectorAll('*') position) so the Playwright shim can dispatch
// on `*`[idx] in the SAME context. The shim used to resolve these over a re-serialized
// snapshot (turbo-surf-view's by_role/by_text/by_label) and dispatch the snapshot's idx onto
// the live DOM — but serialize→reparse can reorder elements (e.g. portal'd MUI Autocomplete
// options / dialogs), so the idx pointed at the WRONG live node (a wrapper, not the option),
// and the click never reached the option's onClick. Resolving here over the live DOM keeps
// the idx and the dispatch in one context. Mirrors the Rust matchers (aria.rs/locator.rs).
__G.tcGetBy = function (kind, value, name, root) {
  // `idx` is always the GLOBAL document-order position (so the shim dispatches on `*`[idx]).
  // `root` scopes matching to within elements matching that selector (descendant-or-self) —
  // backs `parentLocator.getByRole/getByText/getByLabel(...)`.
  const all = Array.prototype.slice.call(document.querySelectorAll("*"));
  let inScope = null;
  if (root) {
    inScope = new Set();
    const roots = Array.prototype.slice.call(document.querySelectorAll(root));
    for (let ri = 0; ri < roots.length; ri++) {
      inScope.add(roots[ri]);
      const sub = roots[ri].querySelectorAll("*");
      for (let si = 0; si < sub.length; si++) inScope.add(sub[si]);
    }
  }
  const attr = (el, n) => (el && el.getAttribute ? el.getAttribute(n) : null) || "";
  const roleOf = (el) => {
    const r = attr(el, "role");
    if (r) return r;
    const tag = (el.tagName || "").toLowerCase();
    if (tag === "input") {
      const t = attr(el, "type").toLowerCase();
      return t === "checkbox" ? "checkbox" : t === "radio" ? "radio"
        : (t === "button" || t === "submit" || t === "reset") ? "button" : "textbox";
    }
    return ({ a: "link", button: "button", select: "combobox", textarea: "textbox" })[tag] || "generic";
  };
  const accName = (el) => {
    const cands = [attr(el, "aria-label").trim(), (el.textContent || "").trim(),
      attr(el, "placeholder").trim(), attr(el, "value").trim(), attr(el, "title").trim()];
    for (const c of cands) if (c) return c;
    return "";
  };
  // Substring match, or a `/pattern/flags` regex literal (mirrors turbo-surf-view).
  const tmatch = (v, want) => {
    if (want == null) return true;
    const m = /^\/(.*)\/([a-z]*)$/.exec(want);
    if (m) { try { return new RegExp(m[1], m[2]).test(v); } catch (_e) { return false; } }
    return String(v).indexOf(want) >= 0;
  };
  const idxOf = (el) => all.indexOf(el);
  let hits = [];
  if (kind === "role") {
    for (const el of all) if (roleOf(el) === value && tmatch(accName(el), name)) hits.push(el);
  } else if (kind === "label") {
    const seen = new Set();
    const push = (el) => { if (el && !seen.has(el)) { seen.add(el); hits.push(el); } };
    for (const lab of document.querySelectorAll("label")) {
      if (!tmatch((lab.textContent || "").trim(), value)) continue;
      const forId = attr(lab, "for");
      if (forId) push(document.getElementById(forId));
      const id = attr(lab, "id");
      if (id) for (const t of document.querySelectorAll('[aria-labelledby~="' + id + '"]')) push(t);
      push(lab.querySelector("input,select,textarea"));
    }
    for (const el of document.querySelectorAll("[aria-label]")) if (tmatch(attr(el, "aria-label"), value)) push(el);
  } else if (kind === "text") {
    // Leaf-text match: the element's text matches AND no descendant element also matches
    // (so we target the tightest node, like turbo-surf-view's by_text).
    for (const el of all) {
      if (!tmatch((el.textContent || "").trim(), value)) continue;
      let childMatch = false;
      for (const c of el.querySelectorAll("*")) { if (tmatch((c.textContent || "").trim(), value)) { childMatch = true; break; } }
      if (!childMatch) hits.push(el);
    }
  }
  if (inScope) hits = hits.filter((el) => inScope.has(el));
  globalThis.__RESULT = JSON.stringify(hits.map((el) => ({ idx: idxOf(el) })));
};

// Resolve a locator through an nth-aware scope CHAIN, then match its leaf. `scope` is an
// ordered list of {sel, idx}: at each level, querySelectorAll(sel) within the current set,
// and when idx != null keep only that one (the `.nth(i)` of a parent locator). The leaf is
// either {selector} (getByTestId/locator) or {getBy:{kind,value,name}} (getByRole/Text/Label).
// idx in the output is the GLOBAL document-order position so the shim dispatches on `*`[idx].
// Needed because a CSS-concat selector can't express "the nth match's subtree".
__G.tcResolveScoped = function (scope, leaf) {
  let cur = [document];
  for (let si = 0; si < (scope || []).length; si++) {
    const s = scope[si];
    let next = [];
    for (let ci = 0; ci < cur.length; ci++) {
      let m;
      try {
        m = cur[ci].querySelectorAll(s.sel);
      } catch (e) {
        m = [];
      }
      for (let mi = 0; mi < m.length; mi++) next.push(m[mi]);
    }
    // Apply a Locator.filter({hasText|hasNotText}) at this level BEFORE indexing, so a
    // `parent.filter(...).first()/nth()` scopes children to the same element the static
    // read path picks (a CSS-concat selector can't express the text filter).
    if (s.filter) {
      next = next.filter((el) => {
        const txt = (el.textContent || "");
        if (s.filter.hasText != null && txt.indexOf(s.filter.hasText) === -1) return false;
        if (s.filter.hasNotText != null && txt.indexOf(s.filter.hasNotText) !== -1) return false;
        return true;
      });
    }
    if (s.idx != null) {
      const i = s.idx < 0 ? next.length + s.idx : s.idx;
      next = next[i] ? [next[i]] : [];
    }
    cur = next;
  }
  const all = Array.prototype.slice.call(document.querySelectorAll("*"));
  if (leaf && leaf.selector) {
    let res = [];
    for (let ci = 0; ci < cur.length; ci++) {
      let m;
      try {
        m = cur[ci].querySelectorAll(leaf.selector);
      } catch (e) {
        m = [];
      }
      for (let mi = 0; mi < m.length; mi++) res.push(m[mi]);
    }
    globalThis.__RESULT = JSON.stringify(res.map((el) => ({ idx: all.indexOf(el) })));
    return;
  }
  if (leaf && leaf.getBy) {
    // Reuse __G.tcGetBy's role/text/label matcher by marking the resolved roots and scoping to
    // them (descendant-or-self via the [data-tc-scope] root selector).
    for (let ci = 0; ci < cur.length; ci++) if (cur[ci].setAttribute) cur[ci].setAttribute("data-tc-scope", "");
    __G.tcGetBy(leaf.getBy.kind, leaf.getBy.value, leaf.getBy.name, "[data-tc-scope]");
    const out = globalThis.__RESULT;
    for (let ci = 0; ci < cur.length; ci++) if (cur[ci].removeAttribute) cur[ci].removeAttribute("data-tc-scope");
    globalThis.__RESULT = out;
    return;
  }
  globalThis.__RESULT = "[]";
};

// Apply CSS `:hover` styles for a hovered element. turbo-dom's cascade has no pointer state,
// so content revealed only by `.trigger:hover .menu { display:block }` (hover dropdowns/menus
// — e.g. the app's UserMenu, overridden to open on hover) stays display:none and waitFor
// (state:'visible') hangs. We simulate hover: mark the hovered chain (the element, its
// ancestors, and its deepest-first-child descendant path — the nodes a pointer over the
// element is "on") with [data-tc-hover], then for every `:hover` style rule (from <style>
// text AND any constructable/inserted sheets), rewrite `:hover` → `[data-tc-hover]`, match it
// live, and apply the rule's declarations INLINE on the matched elements — inline style feeds
// both this env's getComputedStyle and rtdom's native cascade (is_visible), so the reveal is
// observable. Best-effort + flat-rule only (skips nested @media); enough for hover menus.
__G.tcApplyHover = function (el) {
  if (!el || !el.setAttribute) return;
  const mark = (n) => {
    if (n && n.setAttribute) n.setAttribute("data-tc-hover", "");
  };
  // A pointer over `el` is "on" el, all its ancestors, and (since we have no layout to know
  // which leaf the cursor lands on) any of its descendants — mark all three so a `:hover`
  // rule anchored on a nested trigger (e.g. MUI's `&:hover` on the menu root inside the
  // hovered wrapper) still matches.
  mark(el);
  for (let a = el.parentElement; a; a = a.parentElement) mark(a);
  try {
    const desc = el.querySelectorAll("*");
    for (let i = 0; i < desc.length; i++) mark(desc[i]);
  } catch (e) {}
  const cssTexts = [];
  try {
    const styles = document.querySelectorAll("style");
    for (let i = 0; i < styles.length; i++) cssTexts.push(styles[i].textContent || "");
  } catch (e) {}
  try {
    const sheets = document.styleSheets || [];
    for (let si = 0; si < sheets.length; si++) {
      let rules;
      try {
        rules = sheets[si].cssRules || [];
      } catch (e) {
        rules = [];
      }
      for (let ri = 0; ri < rules.length; ri++) cssTexts.push(String(rules[ri].cssText || ""));
    }
  } catch (e) {}
  // Flatten nested CSS (emotion serializes `.css-x{ base; &:hover .menu{ … } }`) into flat
  // (selector, decls) rules, resolving `&` to the parent selector. A flat-regex parse can't
  // do this — the reveal rule is nested under the trigger's class, with `&` standing in for it.
  const flat = [];
  const resolveSel = (parent, sel) =>
    sel
      .split(",")
      .map((p) => {
        p = p.trim();
        if (!parent) return p;
        return p.indexOf("&") >= 0 ? p.replace(/&/g, parent) : parent + " " + p;
      })
      .join(",");
  const flatten = (css) => {
    let pos = 0;
    const parse = (parentSel) => {
      let buf = "";
      let declBuf = "";
      while (pos < css.length) {
        const ch = css[pos++];
        if (ch === "}") {
          if (buf.indexOf(":") >= 0) declBuf += buf;
          if (declBuf.trim() && parentSel) flat.push({ sel: parentSel, decls: declBuf.trim() });
          return;
        }
        if (ch === ";") {
          declBuf += buf + ";";
          buf = "";
          continue;
        }
        if (ch === "{") {
          parse(resolveSel(parentSel, buf.trim()));
          buf = "";
          continue;
        }
        buf += ch;
      }
      if (declBuf.trim() && parentSel) flat.push({ sel: parentSel, decls: declBuf.trim() });
    };
    parse("");
  };
  for (let ci = 0; ci < cssTexts.length; ci++) flatten(cssTexts[ci]);
  for (let fi = 0; fi < flat.length; fi++) {
    const sel = flat[fi].sel;
    if (sel.indexOf(":hover") < 0) continue;
    const parts = sel.split(",");
    for (let pi = 0; pi < parts.length; pi++) {
      const s = parts[pi].trim();
      if (s.indexOf(":hover") < 0) continue;
      const rw = s.replace(/:hover/g, "[data-tc-hover]");
      let targets;
      try {
        targets = document.querySelectorAll(rw);
      } catch (e) {
        continue;
      }
      for (let ti = 0; ti < targets.length; ti++) {
        const t = targets[ti];
        const decls = flat[fi].decls.split(";");
        for (let di = 0; di < decls.length; di++) {
          const c = decls[di].indexOf(":");
          if (c < 0) continue;
          const prop = decls[di].slice(0, c).trim();
          const val = decls[di].slice(c + 1).trim();
          if (!prop) continue;
          try {
            t.style.setProperty(prop, val);
          } catch (e) {
            try {
              t.style[prop] = val;
            } catch (e2) {}
          }
        }
      }
    }
  }
};

// Pending-work signal for the Rust pump loop: "1" while timers are queued, a
// <script> hasn't run, an ES module is unclaimed, or a fetch is in-flight (more to do
// after the next async drain), else "0".
const __pendingWindowTimer = () => {
  if(!__realWindowTimers)return __timers.length>0;
  const index=__nextLocalTimer();
  return index>=0 && __timers[index].due<=__timerNow();
};
__G.pendingWork = () =>
  (!__G.isFrame && ops.op_frame_pending() > 0) || (globalThis.__pendingFetches || 0) > 0 || __resourcePromises.size > 0 || __dictionaryPending > 0 || __dictionaryTaskCount > 0 || __pendingWindowTimer() || ((__G.esmSrcQueue || []).length) > 0 || (!__G.isFrame && __initialResourceScripts.length > 0) ? "1" : "0";
// In-flight fetch count — the interaction drain must keep pumping while > 0 even if
// the visible tree looks stable (the response's re-render hasn't happened yet).
__G.pendingFetchCount = () => String(globalThis.__pendingFetches || 0);

// A cheap "has the DOM changed?" signal for the interaction drain: element count + the
// total length of input values (so a controlled-input edit registers). Lets the drain
// stop once the render has SETTLED even though background timers (analytics polling,
// React's idle scheduler) never stop — otherwise an interaction would always run to the
// full budget. Not for correctness, just to detect quiescence of the visible tree.
__G.domSig = (() => {
  const apply=Reflect.apply,query=Document.prototype.querySelectorAll;
  const all=Document.prototype.getElementsByTagName;
  const tag=Object.getOwnPropertyDescriptor(Element.prototype,"tagName").get;
  const values={
    INPUT:Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").get,
    TEXTAREA:Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,"value").get,
    SELECT:Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype,"value").get,
  };
  return () => {
    try {
      const els = apply(all,document,["*"]);
      let n = els.length, vlen = 0;
      const inputs = apply(query,document,["input,textarea,select"]);
      for (let i = 0; i < inputs.length; i++) {
        const element=inputs[i],getter=values[apply(tag,element,[])];
        vlen += (apply(getter,element,[]) || "").length;
      }
      return n + ":" + vlen + ":" + (globalThis.location ? globalThis.location.href.length : 0);
    } catch (_e) {
      return "0";
    }
  };
})();

(function () {
  // Every iframe is a native DOM node. Its browsing context exists only while
  // connected; lifecycle records never become enumerable guest properties.
  const frameRecords = __interfaceMap("HTMLIFrameElement.lifecycle"), frameHosts = new Set();
  const frameApply = Reflect.apply, frameHostAdd = Set.prototype.add, frameHostEach = Set.prototype.forEach, frameHostDelete = Set.prototype.delete, frameHostHas = Set.prototype.has;
  const frameOwnerDocument = __G.domAccessor("ownerDocument", false, "Node");
  const frameDefaultView = function(){return ops.op_frame_owner_window(this);};
  const frameIsConnected = __G.domAccessor("isConnected", false, "Node");
  const frameNodeType = __G.domAccessor("nodeType", false, "Node");
  const frameParentNode = __G.domAccessor("parentNode", false, "Node");
  const frameAttribute = Element.prototype.getAttribute, frameParse = JSON.parse;
  const frameURL = URL;
  const frameURLHref = Object.getOwnPropertyDescriptor(frameURL.prototype,"href").get;
  const frameURLOrigin = Object.getOwnPropertyDescriptor(frameURL.prototype,"origin").get;
  const frameDocumentQueryOne = Document.prototype.querySelector;
  const frameListLength = Object.getOwnPropertyDescriptor(NodeList.prototype,"length").get;
  const frameListItem = NodeList.prototype.item;
  const frameResolvedURL = (source,base) => frameApply(frameURLHref,new frameURL(source,base),[]);
  const frameBaseURI = node => frameApply(__baseURI,frameDocument(node),[]);
  const frameDocumentQuery = Document.prototype.querySelectorAll;
  const frameElementQuery = Element.prototype.querySelectorAll;
  const frameFragmentQuery = DocumentFragment.prototype.querySelectorAll;
  const frameQuery = (root, selector) => {
    const kind = frameApply(frameNodeType, root, []);
    if (kind !== 1 && kind !== 9 && kind !== 11) return null;
    return frameApply(kind === 9 ? frameDocumentQuery : kind === 11 ? frameFragmentQuery : frameElementQuery, root, [selector]);
  };
  const frameDocument = node => frameApply(frameNodeType, node, []) === 9 ? node : frameApply(frameOwnerDocument, node, []);
  const frameOwner = el => frameApply(frameDefaultView, frameApply(frameOwnerDocument, el, []), []);
  const frameConnected = el => frameApply(frameIsConnected, el, []);
  const visitFrameHosts = callback => frameApply(frameHostEach, frameHosts, [callback]);
  const frameWindowView = __G.windowView;
  const frameDepth = (globalThis.__depth || 0) + 1;
  const iframeTag = __G.domAccessor("tagName", false, "Element");
  const recordFor = (el) => {
    if (frameApply(iframeTag, el, []) !== "IFRAME") throw new TypeError("Illegal invocation");
    if (Object.hasOwn(el, "__iframeWired")) {
      for (const name of ["contentWindow", "contentDocument", "__realm", "__iframeWired"]) {
        if (Object.hasOwn(el, name)) delete el[name];
      }
    }
    const owner = frameOwner(el);
    if (!owner) return null;
    if (owner !== globalThis) return ops.op_frame_host_helper(frameDocument(el), "frameRecordFor", [el]);
    let record = frameRecords.get(el);
    if (!record || record.owner !== owner) {
      record = {__proto__:null, owner, depth: frameDepth, realm: null, started: false, connected: false, epoch: 0, loaded: false};
      frameRecords.set(el, record);
    }
    if (frameConnected(el)) frameApply(frameHostAdd, frameHosts, [el]);
    return record;
  };
  const FRAME_DEPTH_CAP = 12;
  const FRAME_STUB_HOST_RE = /posthog|propelauth/i;
  const frameSrcOf = el => frameApply(frameAttribute, el, ["src"]) || "";
  const frameSrcdocOf = el => frameApply(frameAttribute, el, ["srcdoc"]);
  const frameWantsRealm = el => !!frameSrcdocOf(el) || !FRAME_STUB_HOST_RE.test(frameSrcOf(el));
  const resolveFrameUrl = el => {
    const source = frameSrcOf(el);
    if (!source) return "";
    try { return frameResolvedURL(source,frameBaseURI(el)); } catch (_) { return source; }
  };
  const fireIframeLoad = (el, epoch) => {
    const record = recordFor(el);
    if (!record || record.loaded || record.epoch !== epoch || !frameConnected(el)) return;
    record.loaded = true;
    globalThis.setTimeout(() => {
      if (record.epoch !== epoch || !frameConnected(el)) return;
      const event = new __hostEvent("load");
      ops.op_event_trust(event, -1);
      ops.op_dispatch_host_event(event, el);
    }, 0);
  };
  let frameCount = globalThis.length || 0;
  for (let i = 0; i < frameCount; i++) { try { delete globalThis[i]; } catch (_) {} }
  globalThis.length = 0; globalThis.frames = globalThis;
  const refreshFrameTree = () => {
    const windows = ops.op_window_child_frames(__document);
    for (let i = windows.length; i < frameCount; i++) { try { delete globalThis[i]; } catch (_) {} }
    for (let index = 0; index < windows.length; index++) {
      const window = windows[index];
      __recordDefine(globalThis, index, {__proto__:null,
        configurable: true, get() { return frameWindowView(window); }
      });
    }
    frameCount = windows.length; globalThis.length = frameCount;
  };

  // Compile in the child's V8 context, preserving its global lexical bindings and
  // native intrinsics. Function wrappers would leak the parent realm and stacks.
  const runVmInRealm = (cw, code, sourceUrl, responseId = 0, scriptIndex = 4294967295) =>
    ops.op_queue_frame_script(cw, code, sourceUrl || "about:blank", responseId, scriptIndex);
  // Extract + run the frame document's scripts in the realm, in document order: inline
  // bodies inline, and `<script src>` fetched over op_fetch (resolved against the frame URL,
  // shared cookie jar).
  const loadFrameScripts = async (cw, frameUrl) => {
    const scripts = ops.op_frame_scripts(cw), deferred = [], asynchronous = [];
    const execute = async (script) => {
      let downloaded=false;
      try {
        if(script.module){await ops.op_frame_module(cw,script.index);return;}
        if(!ops.op_frame_script_allowed(cw,script.index)){if(script.src)ops.op_frame_script_event(cw,script.index,false);return;}
        if (script.src) {
          const response = await ops.op_fetch(script.src, __stringify({ __proto__:null, destination: "script", source: frameUrl, frame_script: true, integrity:script.integrity, ...ops.op_document_cookie_site(cw), csp:ops.op_csp_context(cw), opaque_origin:ops.op_window_opaque(cw), mode:script.crossorigin===null?"no-cors":"cors", credentials:script.crossorigin!==null&&script.crossorigin.toLowerCase()!=="use-credentials"?"same-origin":"include" }));
          if (response && response.ok) {
            downloaded=true;
            await runVmInRealm(cw, response.body, response.final_url || script.src, response.frame_script || 0, script.index);
            ops.op_frame_script_event(cw,script.index,true);
          } else ops.op_frame_script_event(cw,script.index,false);
        } else if (script.code.trim()) {
          const located = "\n".repeat(script.line) + " ".repeat(script.column) + script.code;
          await runVmInRealm(cw, located, script.source, 0, script.index);
        }
      } catch (error) { if(script.src)ops.op_frame_script_event(cw,script.index,downloaded); __print("frame script error: " + (error && error.stack ? error.stack : error) + "\n"); }
    };
    for (const script of scripts) {
      if (script.asynchronous && (script.src || script.module)) asynchronous.push(execute(script));
      else if (script.module || (script.src && script.deferred)) deferred.push(script);
      else await execute(script);
    }
    ops.op_frame_parser_done(cw);
    for (const script of deferred) await execute(script);
    return {asynchronous};
  };
  __G.cspFrameScript = index => {
    const element=__frameScriptElements[index];if(!element)return false;
    const prepared=__prepareResourceScript(element);if(!prepared || prepared.src==="")return false;
    if (prepared.kind!=="module" && __recordOwn(__resourceScriptAttributes(prepared),"nomodule"))return false;
    const src=prepared.src;
    return ops.op_csp_script(__document,element,index,src?new URL(src,__baseApply(__resourceBaseURL,__document,[])).href:"",prepared.source);
  };
  __G.executeFrameModule = index => {const element=__frameScriptElements[index];if(!element)throw new TypeError("Frame module element unavailable");return __G.execScriptEl(element);};
  // Fire the child realm's DOMContentLoaded + load so the frame VM's init runs.
  __G.configureFrame = (metadata, parentWin, hostEl) => {
    for (const [name,read] of [["parent",ops.op_window_parent],["top",ops.op_window_top],["frameElement",ops.op_frame_owner_element]]) {
      const getter=__documentFunction(function(){if(!ops.op_surf_is_window(this))throw new __exceptionTypeError("Illegal invocation");const value=read(this);return name==="frameElement"?value:frameWindowView(value);},"get "+name,0);
      Object.defineProperty(globalThis,name,{__proto__:null,get:getter,configurable:true,enumerable:true});
    }
    globalThis.__depth = metadata.depth;
    __G.setDocumentReadyState("loading");
    globalThis.origin = metadata.origin;
    location.href = metadata.location;
    __documentRuntimeState.frame = metadata;
    __initializeDocumentBase();
    __documentRuntimeState.referrer = ops.op_document_referrer(__document);
    globalThis.frames = globalThis;
    installFrameFactory(globalThis, document, metadata.depth + 1);
  };
  function fireFrameEvent(target, type, bubbles) {
    "use strict";
    const event = new __hostEvent(type, {bubbles});
    ops.op_event_trust(event, -1);
    try { ops.op_dispatch_host_event(event, target); } catch (error) { __print("frame event error: " + error + "\n"); }
    return event;
  };
  const frameParsingDone = () => {
    ops.op_csp_finish_parser(__document);
    __G.performancePhase("domInteractive");
    __G.setDocumentReadyState("interactive");
    fireFrameEvent(document, "readystatechange");
  };
  const frameDomReady = () => {
    __initialResourceScripts.length=0;
    __G.performancePhase("domContentLoadedEventStart");
    fireFrameEvent(document, "DOMContentLoaded", true);
    __G.performancePhase("domContentLoadedEventEnd");
  };
  function fireFrameLoad() {
    "use strict";
    return new Promise((resolve) => {
    setTimeout(async () => {
      await __G.waitForDocumentResources();
      __frameScriptElements.length=0;
      __G.performancePhase("domComplete");
      __G.setDocumentReadyState("complete");
      fireFrameEvent(document, "readystatechange");
      __G.performancePhase("loadEventStart");
      fireFrameEvent(globalThis, "load");
      __G.performancePhase("loadEventEnd");
      fireFrameEvent(globalThis, "pageshow");
      resolve();
    }, 0);
  });
  }
  ops.op_set_host_capabilities({frameParsingDone,frameDomReady,fireFrameLoad});
  const frameMetadata = (el) => {
    const record = recordFor(el), source = frameParse(ops.op_frame_owner_metadata(frameDocument(el)));
    const inherited = record.navigationURL ? record.navigationURL === "about:blank" : !frameSrcOf(el) || frameSrcOf(el) === "about:blank" || frameSrcdocOf(el) !== null;
    const url = record.navigationURL || (inherited ? source[0] : resolveFrameUrl(el));
    const origin = inherited ? source[1] : frameApply(frameURLOrigin,new frameURL(url),[]);
    const location = record.navigationURL || (frameSrcdocOf(el) !== null ? "about:srcdoc" : inherited ? "about:blank" : url);
    return {__proto__:null, url, origin, location, inherited, depth: record.depth, referrer: source[0]};
  };
  const frameRealm = (el) => {
    if (!ops.op_window_active(__document)) return null;
    const record = recordFor(el);
    if (!record || !ops.op_window_active(__document) || !frameConnected(el) || record.depth > FRAME_DEPTH_CAP || !frameWantsRealm(el)) return null;
    if (record.realm && ops.op_window_active(record.realm)) return record.realm;
    // An unsuccessful native commit retires its unpublished replacement. A
    // later host navigation must allocate a fresh realm rather than reuse it.
    record.realm = null;
    let window = ops.op_create_frame(globalThis);
    record.metadata = frameMetadata(el);
    window = ops.op_frame_set_document(window, record.owner, el,
      "<!DOCTYPE html><html><head></head><body></body></html>",
      __stringify({__proto__:null,...record.metadata, inherited: true, initial: true}), 0);
    record.realm = window;
    refreshFrameTree();
    return window;
  };
  const loadFrameImpl = async (el) => {
    const record = recordFor(el);
    if (!record || record.owner !== globalThis || record.started || !frameConnected(el) || !ops.op_window_active(__document)) return;
    record.started = true;
    const epoch = record.epoch;
    let window = frameRealm(el);
    if (!window) { fireIframeLoad(el, epoch); return; }
    globalThis.__pendingFetches = (globalThis.__pendingFetches || 0) + 1;
    let frameUrl = record.metadata.url;
    try {
      // Insertion creates the initial empty document synchronously. Navigation
      // commits on a later microtask, so srcdoc cannot appear during appendChild.
      await Promise.resolve();
      if (record.epoch !== epoch || !frameConnected(el)) return;
      let html = "", responseId = 0;
      const srcdoc = record.navigationURL ? null : frameSrcdocOf(el);
      const blank = srcdoc === null && (record.navigationURL ? record.navigationURL === "about:blank" : !frameSrcOf(el) || frameSrcOf(el) === "about:blank");
      if (srcdoc !== null) html = srcdoc;
      else {
        const url = record.navigationURL || resolveFrameUrl(el);
        if (url && url !== "about:blank") {
          const response = await ops.op_fetch(url, __stringify({__proto__:null,destination: "iframe", referrerPolicy:ops.op_frame_referrer_policy(el), partition:ops.op_frame_request_partition(__document,el), source: record.navigationSource || ops.op_document_request_url(),csp:ops.op_csp_context(__document)}));
          if (record.epoch !== epoch || !frameConnected(el)) return;
          html = response.body || ""; responseId = response.frame_document || 0;
          frameUrl = response.final_url || url;
        }
      }
      if (record.epoch !== epoch || !frameConnected(el)) return;
      if (!blank || record.navigation) {
        window = ops.op_frame_set_document(window, record.owner, el,
          html || "<!DOCTYPE html><html><head></head><body></body></html>",
          __stringify({__proto__:null,...record.metadata, url: frameUrl}), responseId);
        record.realm = window; record.navigation = false;
        refreshFrameTree();
      }
      await ops.op_frame_resources(window);
      const work = await loadFrameScripts(window, frameUrl);
      await ops.op_frame_dom_ready(window);
      await __framePromiseAll(work.asynchronous);
      await ops.op_frame_finish(window);
    } catch (error) {
      if (record.epoch === epoch) __print("frame error: " + (error && error.stack ? error.stack : error) + "\n");
    } finally {
      globalThis.__pendingFetches = Math.max(0, (globalThis.__pendingFetches || 1) - 1);
      fireIframeLoad(el, epoch);
    }
  };
  const loadFrame = el => __trackResource(loadFrameImpl(el));
  const retire = (el) => {
    const record = recordFor(el);
    if (!record) return;
    if (record.realm) ops.op_retire_frame(record.realm, record.owner);
    record.realm = null; record.started = false; record.loaded = false; record.epoch++;
  };
  const prepareFrameAdoption = (hosts) => {
    const records = __recordCreate(null); let count = 0;
    for (let index = 0; index < hosts.length; index++) {
      const host = hosts[index], record = recordFor(host);
      if (record) records[count++] = {__proto__:null, record, host,
        parent:frameApply(frameParentNode,host,[]), document:frameDocument(host), connected:frameConnected(host)};
    }
    return (completed = true) => {
      for (let index = 0; index < count; index++) {
        const snapshot = records[index], record = snapshot.record;
        if (!completed && frameApply(frameParentNode,snapshot.host,[]) === snapshot.parent
          && frameDocument(snapshot.host) === snapshot.document && frameConnected(snapshot.host) === snapshot.connected) continue;
        if (record.realm) ops.op_retire_frame(record.realm, record.owner);
        record.realm = null; record.started = false; record.loaded = false; record.epoch++;
      }
      refreshFrameTree();
    };
  };
  const adoptNode = Document.prototype.adoptNode;
  if (typeof adoptNode === "function") {
    const delegate = function(node) {
      frameApply(__documentType, this, []);
      if (!arguments.length) return frameApply(adoptNode, this, []);
      frameApply(__documentType, node, []);
      const source = frameApply(frameOwnerDocument, node, []);
      const owner = source && frameApply(frameDefaultView, source, []);
      const hosts = owner ? ops.op_frame_move_candidates(source, [node]) : [];
      const finish = owner ? ops.op_frame_host_helper(source, "prepareFrameAdoption", [hosts]) : null;
      const result = frameApply(adoptNode, this, [node]);
      if (finish) finish();
      return result;
    };
    Object.defineProperty(Document.prototype, "adoptNode", {value: __documentFunction(delegate, "adoptNode", 1), writable: true, configurable: true, enumerable: true});
  }
  const changedSource = (el) => {
    const record = recordFor(el);
    if (!record) return;
    record.navigationURL = null; record.navigationSource = null;
    if (!frameConnected(el)) return;
    if (ops.op_native_window_proxy()) {
      record.started = false; record.loaded = false; record.epoch++; record.navigation = true;
      record.metadata = frameMetadata(el);
    } else { retire(el); }
    loadFrame(el);
  };
  const syncConnections = (root = document) => {
    try { if (frameApply(frameNodeType, root, []) === 1 && frameApply(iframeTag, root, []) === "IFRAME") recordFor(root); } catch (_) {}
    const descendants = frameQuery(root, "iframe");
    if(descendants)for (let index = 0,length=frameApply(frameListLength,descendants,[]); index < length; index++) recordFor(frameApply(frameListItem,descendants,[index]));
    visitFrameHosts(el => {
      const record = recordFor(el), connected = frameConnected(el);
      if (!record || record.owner !== globalThis) {frameApply(frameHostDelete, frameHosts, [el]);return;}
      if (!connected && record.connected) retire(el);
      record.connected = connected;
      if (!connected) {frameApply(frameHostDelete, frameHosts, [el]);return;}
      if (!record.started) loadFrame(el);
    });
    refreshFrameTree();
  };
  const recaptchaBframeHandshake = (el, url) => {
    if (url != null) el.setAttribute("src", String(url));
    if (frameConnected(el)) return loadFrame(el);
  };
  const frameReceiver = (el) => {
    frameApply(iframeTag, el, []);
    if (frameApply(iframeTag, el, []) !== "IFRAME") throw new TypeError("Illegal invocation");
    return frameOwner(el);
  };
  const accessFrame = (el, documentOnly) => {
    const owner = frameReceiver(el);
    if (!owner) return null;
    if (owner !== globalThis) return ops.op_frame_host_helper(frameDocument(el), "accessFrame", [el, documentOnly]);
    const window = frameRealm(el);
    if (!window) return null;
    return documentOnly ? (ops.op_windows_same_origin(window) ? window.document : null) : frameWindowView(window);
  };
  const iframeProto = HTMLIFrameElement.prototype;
  for (const [name, read] of [["contentWindow", el => accessFrame(el, false)], ["contentDocument", el => accessFrame(el, true)],
    ["src", el => {frameReceiver(el); return frameSrcOf(el) ? resolveFrameUrl(el) : "";}],
    ["srcdoc", el => {frameReceiver(el); return frameSrcdocOf(el) || "";}]]) {
    const getter = Object.getOwnPropertyDescriptor({get [name]() {return read(this);}}, name).get;
    const descriptor = {get: __documentFunction(getter, "get " + name, 0), enumerable: true, configurable: true};
    if (name === "src" || name === "srcdoc") {
      const originalSetter = Object.getOwnPropertyDescriptor(iframeProto, name)?.set;
      const setter = Object.getOwnPropertyDescriptor({set [name](value) {
        frameReceiver(this);
        if (originalSetter) {
          frameApply(originalSetter, this, [value]);
          const owner=frameOwner(this);
          if(owner)ops.op_frame_host_helper(frameDocument(this), "frameSourceChanged", [this]);
        } else this.setAttribute(name, String(value));
      }}, name).set;
      descriptor.set = __documentFunction(setter, "set " + name, 1);
    }
    Object.defineProperty(iframeProto, name, descriptor);
  }
  // Mutations may connect a whole subtree/fragment, remove a frame, or move it
  // into a shadow tree. Forward to the native DOM first, then update contexts.
  const mutators = ["appendChild", "insertBefore", "removeChild", "append", "prepend", "before", "after", "remove", "replaceWith", "replaceChildren", "replaceChild", "insertAdjacentElement", "insertAdjacentHTML"];
  for (const proto of [Node.prototype, Element.prototype, Document.prototype, DocumentFragment.prototype, CharacterData.prototype]) {
    for (const name of mutators) {
      const descriptor = Object.getOwnPropertyDescriptor(proto, name);
      if (!descriptor || typeof descriptor.value !== "function") continue;
      const original = descriptor.value;
      const delegate = function(...args) {
        const receiverDocument = frameDocument(this);
        if (!frameApply(frameDefaultView, receiverDocument, [])) return frameApply(original, this, args);
        const parent = frameApply(frameParentNode, this, []);
        const moved = ["appendChild","insertBefore","replaceChild"].includes(name) ? [args[0]]
          : name === "insertAdjacentElement" ? [args[1]]
          : ["append","prepend","before","after","replaceWith","replaceChildren"].includes(name) ? args : [];
        const finishes = __recordCreate(null), seen = new Set();let finishCount = 0;
        for (let index = 0; index < moved.length; index++) {
          let source;
          try {source = frameDocument(moved[index]);} catch (_) {continue;}
          const candidates = ops.op_frame_move_candidates(source, [moved[index]]);
          for (let candidate = 0; candidate < candidates.length; candidate++) {
            const host = candidates[candidate];
            if (frameApply(frameHostHas, seen, [host])) continue;
            frameApply(frameHostAdd, seen, [host]);
            finishes[finishCount++] = ops.op_frame_host_helper(source, "prepareFrameAdoption", [[host]]);
          }
        }
        let completed = false;
        try {
          const result = frameApply(original, this, args);
          completed = true;
          return result;
        } finally {
          // Variadic conversion can detach frames before insertion throws. Native
          // preflight failures leave them intact; preserve the original exception.
          try {
            for (let index = 0; index < finishCount; index++) if (finishes[index]) finishes[index](completed);
            const owner = frameApply(frameDefaultView, frameDocument(this), []);
            if (owner) {
              ops.op_frame_host_helper(frameDocument(this), "syncFrameConnections", [this]);
              if (parent && parent !== this) ops.op_frame_host_helper(frameDocument(parent), "syncFrameConnections", [parent]);
            }
          } catch (error) {if (completed) throw error;}
        }
      };
      Object.defineProperty(proto, name, {...descriptor, value: __documentFunction(delegate, name, original.length)});
    }
    for (const name of ["innerHTML", "outerHTML", "textContent"]) {
      const descriptor = Object.getOwnPropertyDescriptor(proto, name);
      if (!descriptor || !descriptor.set) continue;
      const original = descriptor.set;
      const delegate = function(value) {
        const parent = frameApply(frameParentNode, this, []);
        const result = frameApply(original, this, [value]);
        const owner = frameApply(frameDefaultView, frameDocument(this), []);
        if (!owner) return result;
        ops.op_frame_host_helper(frameDocument(this), "syncFrameConnections", [this]);
        if (parent) ops.op_frame_host_helper(frameDocument(parent), "syncFrameConnections", [parent]);
        return result;
      };
      Object.defineProperty(proto, name, {...descriptor, set: __documentFunction(delegate, "set " + name, 1)});
    }
  }
  for (const name of ["setAttribute", "removeAttribute", "setAttributeNS", "removeAttributeNS"]) {
    const descriptor = Object.getOwnPropertyDescriptor(Element.prototype, name);
    if (!descriptor || typeof descriptor.value !== "function") continue;
    const original = descriptor.value;
    const delegate = function(...args) {
      const result = frameApply(original, this, args);
      const attribute = String(args[name.endsWith("NS") ? 1 : 0]).toLowerCase();
      if (frameApply(iframeTag, this, []) === "IFRAME" && (attribute === "src" || attribute === "srcdoc")) {
        const owner = frameOwner(this);
        if (owner) ops.op_frame_host_helper(frameDocument(this), "frameSourceChanged", [this]);
      }
      return result;
    };
    Object.defineProperty(Element.prototype, name, {...descriptor, value: __documentFunction(delegate, name, original.length)});
  }
  const navigateFrame = (el,url,source) => {
    const record = recordFor(el);
    if (!record || !frameConnected(el) || !record.realm) return;
    record.navigationURL = url; record.navigationSource = source;
    record.started = false; record.loaded = false; record.epoch++; record.navigation = true;
    record.metadata = frameMetadata(el);
    loadFrame(el);
  };
  ops.op_set_frame_navigator(navigateFrame);
  const nativeCreate = document.createElement.bind(document);
  document.createElement = function(tag, options) {
    const el = nativeCreate(tag, options);
    if (String(tag).toLowerCase() === "iframe") recordFor(el);
    return el;
  };
  // Retained for child configuration; DOM ownership already determines depth.
  const installFrameFactory = () => {};
  syncConnections();

  ops.op_set_host_capabilities({frameRecordFor:recordFor, accessFrame,
    prepareFrameAdoption, syncFrameConnections:syncConnections,
    frameSourceChanged:changedSource, recaptchaBframeHandshake});
  for (const name of ["frameRecordFor", "frameRealm", "loadFrame", "prepareFrameAdoption",
    "syncFrameConnections", "accessFrame", "frameSourceChanged", "recaptchaBframeHandshake"]) delete __G[name];
})();
"##,
    include_str!("web_geometry.js"),
    r##"
// Web-IDL functions are embedded through V8 FunctionTemplates. Keep the
// intrinsic Function.prototype.toString intact; guest delegates remain live.
(() => {
  const orig = Function.prototype.toString;
  const native = new WeakSet();
  const nativeBindings = new WeakMap();
  const factory = __G.makeNativeFunction;
  const nativize = (delegate, name, len) => {
    if (typeof delegate !== "function") return delegate;
    if (nativeBindings.has(delegate)) return nativeBindings.get(delegate);
    if (orig.call(delegate).includes("[native code]")) return delegate;
    if (typeof factory !== "function") return delegate;
    const fn = factory(delegate, name || delegate.name, len == null ? delegate.length : len);
    native.add(fn);
    nativeBindings.set(delegate, fn);
    return fn;
  };
  // NB: `nativize` stays a closure-local — exposing it as a global (window.__nativize) would itself
  // be a tamper tell. Every shim site that needs it is inside this same IIFE.
  const mark = (fn, name) => {
    if (typeof fn !== "function") return;
    // Don't rename an already-marked function: setInterval/clearInterval/
    // cancelAnimationFrame alias setTimeout/clearTimeout (same object), so the
    // canonical name set first must win.
    if (name && !native.has(fn)) {
      try { Object.defineProperty(fn, "name", { value: name, configurable: true }); } catch (e) {}
    }
    native.add(fn);
  };
  mark(setTimeout, "setTimeout");
  mark(clearTimeout, "clearTimeout");
  mark(requestAnimationFrame, "requestAnimationFrame");
  mark(queueMicrotask, "queueMicrotask");
  mark(fetch, "fetch");
  mark(setInterval); mark(clearInterval);
  mark(cancelAnimationFrame, "cancelAnimationFrame"); // now its own fn, not a clearTimeout alias
  // Event API: the vendored addEventListener/removeEventListener/dispatchEvent are `function`
  // shims — their `.toString()` leaks source AND they carry an own `prototype` (native methods
  // don't). WORSE, they sit as WINDOW OWN-PROPS: real Chrome inherits them from
  // EventTarget.prototype, so `window.hasOwnProperty('addEventListener')` is FALSE in Chrome but
  // was TRUE for us — a checkable tell. Fix: native-shape the methods on EventTarget.prototype,
  // wire window's prototype chain window -> Window.prototype -> EventTarget.prototype (as Chrome
  // does), and DELETE the window (and document) own copies so they're inherited, not own.
  const __evNames = ["addEventListener", "removeEventListener", "dispatchEvent"];
  (() => { try {
    const ET = globalThis.EventTarget;
    if (!ET || !ET.prototype) return;
    for (const name of __evNames) {
      // Prefer a working impl (window's own, else the proto's) and native-shape it on the proto.
      const impl = ET.prototype[name]
        || (Object.prototype.hasOwnProperty.call(globalThis, name) && globalThis[name])
        || (globalThis.document && globalThis.document[name]);
      if (typeof impl === "function") {
        try { ET.prototype[name] = nativize(impl, name, name === "dispatchEvent" ? 1 : 2); } catch (e) {}
      }
    }
    // window -> Window.prototype -> EventTarget.prototype (Chrome's chain). If Window is absent,
    // fall back to inheriting EventTarget.prototype directly so the methods still resolve.
    const W = globalThis.Window;
    try {
      if (W && W.prototype) {
        const properties = Object.create(ET.prototype);
        Object.defineProperty(properties, Symbol.toStringTag, {value:"WindowProperties",configurable:true});
        Object.setPrototypeOf(W.prototype, properties);
        Object.defineProperty(W.prototype, Symbol.toStringTag, {value:"Window",configurable:true});
        // Window inherits from EventTarget and carries the FileSystem constants; instanceof
        // follows the real prototype chain instead of a predicate.
        Object.setPrototypeOf(W, ET);
        delete W[Symbol.hasInstance];
        for (const [name, value] of [["TEMPORARY", 0], ["PERSISTENT", 1]])
          for (const object of [W, W.prototype]) Object.defineProperty(object, name, {value, writable:false, enumerable:true, configurable:false});
        if (Object.hasOwn(globalThis, Symbol.toStringTag)) delete globalThis[Symbol.toStringTag];
        if (Object.getPrototypeOf(globalThis) !== W.prototype) Object.setPrototypeOf(globalThis, W.prototype);
      } else if (Object.getPrototypeOf(globalThis) !== ET.prototype) {
        Object.setPrototypeOf(globalThis, ET.prototype);
      }
    } catch (e) {}
    // Remove the WINDOW own copies so they're inherited via the chain (Chrome: hasOwnProperty
    // false on window). Document keeps native-shaped OWN copies (rtdom's document has no
    // EventTarget proto chain to inherit through; a prototype-less native-marked own method still
    // matches the native-fn SHAPE a collector reads on document.addEventListener).
    for (const name of __evNames) {
      try {
        if (Object.prototype.hasOwnProperty.call(globalThis, name)) delete globalThis[name];
      } catch (e) {}
      try {
        const d = globalThis.document;
        if (d && Object.prototype.hasOwnProperty.call(d, name) && typeof d[name] === "function") {
          d[name] = nativize(d[name], name, name === "dispatchEvent" ? 1 : 2);
        }
      } catch (e) {}
    }
  } catch (e) {} })();
  if (globalThis.Headers) mark(globalThis.Headers, "Headers");
  const nav = globalThis.navigator;
  if (nav && nav.clipboard) { mark(nav.clipboard.writeText, "writeText"); mark(nav.clipboard.readText, "readText"); }
  // Navigator.prototype now holds all 84 members (getters + methods). Native-mask every one so a
  // collector reading `Object.getOwnPropertyDescriptor(Navigator.prototype, k).get.toString()` or
  // a method's `.toString()` sees "[native code]", not JS source. (The getters/methods are arrows,
  // so they already have no own `prototype` — matching native shape.)
  try {
    const np = globalThis.Navigator && globalThis.Navigator.prototype;
    if (np) {
      mark(globalThis.Navigator, "Navigator");
      for (const k of Object.getOwnPropertyNames(np)) {
        const d = Object.getOwnPropertyDescriptor(np, k);
        if (!d) continue;
        if (typeof d.get === "function") mark(d.get, "get " + k);
        if (typeof d.set === "function") mark(d.set, "set " + k);
        if (typeof d.value === "function") mark(d.value, k);
      }
    }
  } catch (e) {}
  // window.postMessage uses a native facade; a collector reading
  // their `.toString()` must see native source, like every other shim.
  if (typeof globalThis.postMessage === "function") mark(globalThis.postMessage, "postMessage");
  // Native-mark the message constructors — they're defined (not created by the NAMES loop), so
  // their `.toString()` would otherwise leak JS source (a tamper tell). Also mark MessagePort's
  // prototype methods so `port.postMessage.toString()` reads native.
  for (const __c of [globalThis.MessageChannel, globalThis.MessagePort, globalThis.BroadcastChannel, globalThis.MessageEvent]) {
    if (typeof __c === "function") mark(__c, __c.name);
  }
  if (globalThis.MessagePort && globalThis.MessagePort.prototype) {
    for (const __m of ["postMessage", "start", "close", "addEventListener", "removeEventListener"]) {
      mark(globalThis.MessagePort.prototype[__m], __m);
    }
  }
  if (globalThis.trustedTypes && typeof globalThis.trustedTypes.createPolicy === "function") mark(globalThis.trustedTypes.createPolicy, "createPolicy");

  // ── Structural browser-surface fidelity ──────────────────────────────────────
  // A no-Chromium engine exposes navigator/screen/document as plain object literals
  // (brand "[object Object]", data props, no host prototypes). A deep fingerprinter
  // (Botguard/reCAPTCHA-class) reads WebIDL brands (Object.prototype.toString), native
  // accessor getters up the prototype chain, and native method sources — all of which a
  // synthetic DOM fails. This block re-homes our synthetic globals behind correctly-named
  // host prototypes carrying NATIVE-marked accessor getters (via the same toString WeakSet
  // above), so the passive/consistency surface reads like real Chrome. It cannot forge the
  // active render tier (canvas/WebGL pixels, real layout cascade, font metrics) — those stay
  // honest gaps. Every section is independently guarded so a failure can't break hydration.
  const G = globalThis;
  const canonicalSubmitEvent = typeof G.SubmitEvent === "function" ? G.SubmitEvent : null;
  const guard = (fn) => { try { fn(); } catch (e) {} };
  const tag = (obj, name) => { if (obj) Object.defineProperty(obj, Symbol.toStringTag, { value: name, configurable: true }); };
  // Canonical Event owns its nonconfigurable isTrusted getter and keeps the
  // host-dispatch record private; writable page flags must never confer trust.
  // Lifecycle transitions remain receiver-local in reused isolates and frames.
  __defineDocumentAccessor("readyState", state => state.readyState);
  function lifecycleEvent(target, type, bubbles) {
    "use strict";
    const event = new __hostEvent(type, {bubbles});
    ops.op_event_trust(event, -1);
    ops.op_dispatch_host_event(event, target);

  };
  __markDocumentInteractive = () => {
    if (!__G.initialLoading) return;
    __G.initialLoading = false;
    ops.op_csp_finish_parser(__document);
    __G.performancePhase("domInteractive");
    __G.setDocumentReadyState("interactive");
    lifecycleEvent(document, "readystatechange");
  };
  const dispatchDocumentInteractive = () => {
    __markDocumentInteractive();
    __G.performancePhase("domContentLoadedEventStart");
    lifecycleEvent(document, "DOMContentLoaded", true);
    __G.performancePhase("domContentLoadedEventEnd");
  };
  let __documentCompleted = false;
  const dispatchDocumentComplete = () => {
    if (__documentCompleted) return;
    __documentCompleted = true;
    __G.performancePhase("domComplete");
    __G.setDocumentReadyState("complete");
    lifecycleEvent(document, "readystatechange");
    __G.performancePhase("loadEventStart");
    lifecycleEvent(globalThis, "load", false);
    __G.performancePhase("loadEventEnd");
    lifecycleEvent(globalThis, "pageshow");
  };
  ops.op_set_host_capabilities({documentLoading:()=>{__G.initialLoading=true;__G.setDocumentReadyState("loading");},markDocumentInteractive:__markDocumentInteractive,dispatchDocumentInteractive,dispatchDocumentComplete});

  // NB: navigator is now a full `Navigator` instance built earlier — zero own props, all 84
  // members (incl. userAgent/platform/webdriver) as native-marked getters on `Navigator.prototype`
  // (marked in the loop above). So the old `hostInterface(nav, "Navigator", …)` re-parenting is
  // GONE — it replaced the real 84-member prototype with a thin 13-getter one (a structural tell).
  // `webdriver` is already a native getter returning false with no own property.


  // WebGL identity uses the host adapter, alongside its pixel executor. Extension
  // and limit compatibility still belongs to the emulated WebGL layer below.
  guard(() => {
    const canvasProto = Object.getPrototypeOf(G.document.createElement("canvas"));
    if (!canvasProto || typeof canvasProto.getContext !== "function" || native.has(canvasProto.getContext)) return;
    const adapter = JSON.parse(ops.op_webgl_metadata());
    // WebGL extension lists for this backend (webgl vs webgl2 differ).
    const EXT1 = ["ANGLE_instanced_arrays","EXT_blend_minmax","EXT_clip_control","EXT_color_buffer_half_float","EXT_depth_clamp","EXT_disjoint_timer_query","EXT_float_blend","EXT_frag_depth","EXT_polygon_offset_clamp","EXT_sRGB","EXT_shader_texture_lod","EXT_texture_compression_bptc","EXT_texture_compression_rgtc","EXT_texture_filter_anisotropic","EXT_texture_mirror_clamp_to_edge","KHR_parallel_shader_compile","OES_element_index_uint","OES_fbo_render_mipmap","OES_standard_derivatives","OES_texture_float","OES_texture_float_linear","OES_texture_half_float","OES_texture_half_float_linear","OES_vertex_array_object","WEBGL_blend_func_extended","WEBGL_color_buffer_float","WEBGL_compressed_texture_astc","WEBGL_compressed_texture_etc","WEBGL_compressed_texture_etc1","WEBGL_compressed_texture_pvrtc","WEBGL_compressed_texture_s3tc","WEBGL_compressed_texture_s3tc_srgb","WEBGL_debug_renderer_info","WEBGL_debug_shaders","WEBGL_depth_texture","WEBGL_draw_buffers","WEBGL_lose_context","WEBGL_multi_draw","WEBGL_polygon_mode"];
    const EXT2 = ["EXT_clip_control","EXT_color_buffer_float","EXT_color_buffer_half_float","EXT_conservative_depth","EXT_depth_clamp","EXT_disjoint_timer_query_webgl2","EXT_float_blend","EXT_polygon_offset_clamp","EXT_render_snorm","EXT_texture_compression_bptc","EXT_texture_compression_rgtc","EXT_texture_filter_anisotropic","EXT_texture_mirror_clamp_to_edge","EXT_texture_norm16","KHR_parallel_shader_compile","NV_shader_noperspective_interpolation","OES_draw_buffers_indexed","OES_sample_variables","OES_shader_multisample_interpolation","OES_texture_float_linear","WEBGL_blend_func_extended","WEBGL_clip_cull_distance","WEBGL_compressed_texture_astc","WEBGL_compressed_texture_etc","WEBGL_compressed_texture_etc1","WEBGL_compressed_texture_pvrtc","WEBGL_compressed_texture_s3tc","WEBGL_compressed_texture_s3tc_srgb","WEBGL_debug_renderer_info","WEBGL_debug_shaders","WEBGL_lose_context","WEBGL_multi_draw","WEBGL_polygon_mode","WEBGL_provoking_vertex","WEBGL_render_shared_exponent","WEBGL_stencil_texturing"];
    // Full WebGL enum constants (captured from live Chrome: 298 for WebGL1, +255 for WebGL2).
    // A real gl context exposes ALL of these on its prototype; ours had only the handful patchGl
    // set for getParameter — so `gl.VERTEX_SHADER` etc. were undefined, both a fingerprint tell
    // AND a functional break (real WebGL code + the GPU bridge get `undefined` enum args → the
    // draw fails and readPixels returns the clear color). Applied to the context prototype below.
    const GL_CONST1 = {ACTIVE_ATTRIBUTES:35721,ACTIVE_TEXTURE:34016,ACTIVE_UNIFORMS:35718,ALIASED_LINE_WIDTH_RANGE:33902,
      ALIASED_POINT_SIZE_RANGE:33901,ALPHA:6406,ALPHA_BITS:3413,ALWAYS:519,ARRAY_BUFFER:34962,
      ARRAY_BUFFER_BINDING:34964,ATTACHED_SHADERS:35717,BACK:1029,BLEND:3042,BLEND_COLOR:32773,
      BLEND_DST_ALPHA:32970,BLEND_DST_RGB:32968,BLEND_EQUATION:32777,BLEND_EQUATION_ALPHA:34877,
      BLEND_EQUATION_RGB:32777,BLEND_SRC_ALPHA:32971,BLEND_SRC_RGB:32969,BLUE_BITS:3412,BOOL:35670,BOOL_VEC2:35671,
      BOOL_VEC3:35672,BOOL_VEC4:35673,BROWSER_DEFAULT_WEBGL:37444,BUFFER_SIZE:34660,BUFFER_USAGE:34661,BYTE:5120,
      CCW:2305,CLAMP_TO_EDGE:33071,COLOR_ATTACHMENT0:36064,COLOR_BUFFER_BIT:16384,COLOR_CLEAR_VALUE:3106,
      COLOR_WRITEMASK:3107,COMPILE_STATUS:35713,COMPRESSED_TEXTURE_FORMATS:34467,CONSTANT_ALPHA:32771,
      CONSTANT_COLOR:32769,CONTEXT_LOST_WEBGL:37442,CULL_FACE:2884,CULL_FACE_MODE:2885,CURRENT_PROGRAM:35725,
      CURRENT_VERTEX_ATTRIB:34342,CW:2304,DECR:7683,DECR_WRAP:34056,DELETE_STATUS:35712,DEPTH_ATTACHMENT:36096,
      DEPTH_BITS:3414,DEPTH_BUFFER_BIT:256,DEPTH_CLEAR_VALUE:2931,DEPTH_COMPONENT:6402,DEPTH_COMPONENT16:33189,
      DEPTH_FUNC:2932,DEPTH_RANGE:2928,DEPTH_STENCIL:34041,DEPTH_STENCIL_ATTACHMENT:33306,DEPTH_TEST:2929,
      DEPTH_WRITEMASK:2930,DITHER:3024,DONT_CARE:4352,DST_ALPHA:772,DST_COLOR:774,DYNAMIC_DRAW:35048,
      ELEMENT_ARRAY_BUFFER:34963,ELEMENT_ARRAY_BUFFER_BINDING:34965,EQUAL:514,FASTEST:4353,FLOAT:5126,
      FLOAT_MAT2:35674,FLOAT_MAT3:35675,FLOAT_MAT4:35676,FLOAT_VEC2:35664,FLOAT_VEC3:35665,FLOAT_VEC4:35666,
      FRAGMENT_SHADER:35632,FRAMEBUFFER:36160,FRAMEBUFFER_ATTACHMENT_OBJECT_NAME:36049,
      FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE:36048,FRAMEBUFFER_ATTACHMENT_TEXTURE_CUBE_MAP_FACE:36051,
      FRAMEBUFFER_ATTACHMENT_TEXTURE_LEVEL:36050,FRAMEBUFFER_BINDING:36006,FRAMEBUFFER_COMPLETE:36053,
      FRAMEBUFFER_INCOMPLETE_ATTACHMENT:36054,FRAMEBUFFER_INCOMPLETE_DIMENSIONS:36057,
      FRAMEBUFFER_INCOMPLETE_MISSING_ATTACHMENT:36055,FRAMEBUFFER_UNSUPPORTED:36061,FRONT:1028,FRONT_AND_BACK:1032,
      FRONT_FACE:2886,FUNC_ADD:32774,FUNC_REVERSE_SUBTRACT:32779,FUNC_SUBTRACT:32778,GENERATE_MIPMAP_HINT:33170,
      GEQUAL:518,GREATER:516,GREEN_BITS:3411,HIGH_FLOAT:36338,HIGH_INT:36341,
      IMPLEMENTATION_COLOR_READ_FORMAT:35739,IMPLEMENTATION_COLOR_READ_TYPE:35738,INCR:7682,INCR_WRAP:34055,
      INT:5124,INT_VEC2:35667,INT_VEC3:35668,INT_VEC4:35669,INVALID_ENUM:1280,INVALID_FRAMEBUFFER_OPERATION:1286,
      INVALID_OPERATION:1282,INVALID_VALUE:1281,INVERT:5386,KEEP:7680,LEQUAL:515,LESS:513,LINEAR:9729,
      LINEAR_MIPMAP_LINEAR:9987,LINEAR_MIPMAP_NEAREST:9985,LINES:1,LINE_LOOP:2,LINE_STRIP:3,LINE_WIDTH:2849,
      LINK_STATUS:35714,LOW_FLOAT:36336,LOW_INT:36339,LUMINANCE:6409,LUMINANCE_ALPHA:6410,
      MAX_COMBINED_TEXTURE_IMAGE_UNITS:35661,MAX_CUBE_MAP_TEXTURE_SIZE:34076,MAX_FRAGMENT_UNIFORM_VECTORS:36349,
      MAX_RENDERBUFFER_SIZE:34024,MAX_TEXTURE_IMAGE_UNITS:34930,MAX_TEXTURE_SIZE:3379,MAX_VARYING_VECTORS:36348,
      MAX_VERTEX_ATTRIBS:34921,MAX_VERTEX_TEXTURE_IMAGE_UNITS:35660,MAX_VERTEX_UNIFORM_VECTORS:36347,
      MAX_VIEWPORT_DIMS:3386,MEDIUM_FLOAT:36337,MEDIUM_INT:36340,MIRRORED_REPEAT:33648,NEAREST:9728,
      NEAREST_MIPMAP_LINEAR:9986,NEAREST_MIPMAP_NEAREST:9984,NEVER:512,NICEST:4354,NONE:0,NOTEQUAL:517,NO_ERROR:0,
      ONE:1,ONE_MINUS_CONSTANT_ALPHA:32772,ONE_MINUS_CONSTANT_COLOR:32770,ONE_MINUS_DST_ALPHA:773,
      ONE_MINUS_DST_COLOR:775,ONE_MINUS_SRC_ALPHA:771,ONE_MINUS_SRC_COLOR:769,OUT_OF_MEMORY:1285,
      PACK_ALIGNMENT:3333,POINTS:0,POLYGON_OFFSET_FACTOR:32824,POLYGON_OFFSET_FILL:32823,
      POLYGON_OFFSET_UNITS:10752,RED_BITS:3410,RENDERBUFFER:36161,RENDERBUFFER_ALPHA_SIZE:36179,
      RENDERBUFFER_BINDING:36007,RENDERBUFFER_BLUE_SIZE:36178,RENDERBUFFER_DEPTH_SIZE:36180,
      RENDERBUFFER_GREEN_SIZE:36177,RENDERBUFFER_HEIGHT:36163,RENDERBUFFER_INTERNAL_FORMAT:36164,
      RENDERBUFFER_RED_SIZE:36176,RENDERBUFFER_STENCIL_SIZE:36181,RENDERBUFFER_WIDTH:36162,RENDERER:7937,
      REPEAT:10497,REPLACE:7681,RGB:6407,RGB565:36194,RGB5_A1:32855,RGB8:32849,RGBA:6408,RGBA4:32854,RGBA8:32856,
      SAMPLER_2D:35678,SAMPLER_CUBE:35680,SAMPLES:32937,SAMPLE_ALPHA_TO_COVERAGE:32926,SAMPLE_BUFFERS:32936,
      SAMPLE_COVERAGE:32928,SAMPLE_COVERAGE_INVERT:32939,SAMPLE_COVERAGE_VALUE:32938,SCISSOR_BOX:3088,
      SCISSOR_TEST:3089,SHADER_TYPE:35663,SHADING_LANGUAGE_VERSION:35724,SHORT:5122,SRC_ALPHA:770,
      SRC_ALPHA_SATURATE:776,SRC_COLOR:768,STATIC_DRAW:35044,STENCIL_ATTACHMENT:36128,STENCIL_BACK_FAIL:34817,
      STENCIL_BACK_FUNC:34816,STENCIL_BACK_PASS_DEPTH_FAIL:34818,STENCIL_BACK_PASS_DEPTH_PASS:34819,
      STENCIL_BACK_REF:36003,STENCIL_BACK_VALUE_MASK:36004,STENCIL_BACK_WRITEMASK:36005,STENCIL_BITS:3415,
      STENCIL_BUFFER_BIT:1024,STENCIL_CLEAR_VALUE:2961,STENCIL_FAIL:2964,STENCIL_FUNC:2962,STENCIL_INDEX8:36168,
      STENCIL_PASS_DEPTH_FAIL:2965,STENCIL_PASS_DEPTH_PASS:2966,STENCIL_REF:2967,STENCIL_TEST:2960,
      STENCIL_VALUE_MASK:2963,STENCIL_WRITEMASK:2968,STREAM_DRAW:35040,SUBPIXEL_BITS:3408,TEXTURE:5890,
      TEXTURE0:33984,TEXTURE1:33985,TEXTURE10:33994,TEXTURE11:33995,TEXTURE12:33996,TEXTURE13:33997,
      TEXTURE14:33998,TEXTURE15:33999,TEXTURE16:34000,TEXTURE17:34001,TEXTURE18:34002,TEXTURE19:34003,
      TEXTURE2:33986,TEXTURE20:34004,TEXTURE21:34005,TEXTURE22:34006,TEXTURE23:34007,TEXTURE24:34008,
      TEXTURE25:34009,TEXTURE26:34010,TEXTURE27:34011,TEXTURE28:34012,TEXTURE29:34013,TEXTURE3:33987,
      TEXTURE30:34014,TEXTURE31:34015,TEXTURE4:33988,TEXTURE5:33989,TEXTURE6:33990,TEXTURE7:33991,TEXTURE8:33992,
      TEXTURE9:33993,TEXTURE_2D:3553,TEXTURE_BINDING_2D:32873,TEXTURE_BINDING_CUBE_MAP:34068,
      TEXTURE_CUBE_MAP:34067,TEXTURE_CUBE_MAP_NEGATIVE_X:34070,TEXTURE_CUBE_MAP_NEGATIVE_Y:34072,
      TEXTURE_CUBE_MAP_NEGATIVE_Z:34074,TEXTURE_CUBE_MAP_POSITIVE_X:34069,TEXTURE_CUBE_MAP_POSITIVE_Y:34071,
      TEXTURE_CUBE_MAP_POSITIVE_Z:34073,TEXTURE_MAG_FILTER:10240,TEXTURE_MIN_FILTER:10241,TEXTURE_WRAP_S:10242,
      TEXTURE_WRAP_T:10243,TRIANGLES:4,TRIANGLE_FAN:6,TRIANGLE_STRIP:5,UNPACK_ALIGNMENT:3317,
      UNPACK_COLORSPACE_CONVERSION_WEBGL:37443,UNPACK_FLIP_Y_WEBGL:37440,UNPACK_PREMULTIPLY_ALPHA_WEBGL:37441,
      UNSIGNED_BYTE:5121,UNSIGNED_INT:5125,UNSIGNED_SHORT:5123,UNSIGNED_SHORT_4_4_4_4:32819,
      UNSIGNED_SHORT_5_5_5_1:32820,UNSIGNED_SHORT_5_6_5:33635,VALIDATE_STATUS:35715,VENDOR:7936,VERSION:7938,
      VERTEX_ATTRIB_ARRAY_BUFFER_BINDING:34975,VERTEX_ATTRIB_ARRAY_ENABLED:34338,
      VERTEX_ATTRIB_ARRAY_NORMALIZED:34922,VERTEX_ATTRIB_ARRAY_POINTER:34373,VERTEX_ATTRIB_ARRAY_SIZE:34339,
      VERTEX_ATTRIB_ARRAY_STRIDE:34340,VERTEX_ATTRIB_ARRAY_TYPE:34341,VERTEX_SHADER:35633,VIEWPORT:2978,ZERO:0};
    const GL_CONST2 = {ACTIVE_UNIFORM_BLOCKS:35382,ALREADY_SIGNALED:37146,ANY_SAMPLES_PASSED:35887,
      ANY_SAMPLES_PASSED_CONSERVATIVE:36202,COLOR:6144,COLOR_ATTACHMENT1:36065,COLOR_ATTACHMENT10:36074,
      COLOR_ATTACHMENT11:36075,COLOR_ATTACHMENT12:36076,COLOR_ATTACHMENT13:36077,COLOR_ATTACHMENT14:36078,
      COLOR_ATTACHMENT15:36079,COLOR_ATTACHMENT2:36066,COLOR_ATTACHMENT3:36067,COLOR_ATTACHMENT4:36068,
      COLOR_ATTACHMENT5:36069,COLOR_ATTACHMENT6:36070,COLOR_ATTACHMENT7:36071,COLOR_ATTACHMENT8:36072,
      COLOR_ATTACHMENT9:36073,COMPARE_REF_TO_TEXTURE:34894,CONDITION_SATISFIED:37148,COPY_READ_BUFFER:36662,
      COPY_READ_BUFFER_BINDING:36662,COPY_WRITE_BUFFER:36663,COPY_WRITE_BUFFER_BINDING:36663,CURRENT_QUERY:34917,
      DEPTH:6145,DEPTH24_STENCIL8:35056,DEPTH32F_STENCIL8:36013,DEPTH_COMPONENT24:33190,DEPTH_COMPONENT32F:36012,
      DRAW_BUFFER0:34853,DRAW_BUFFER1:34854,DRAW_BUFFER10:34863,DRAW_BUFFER11:34864,DRAW_BUFFER12:34865,
      DRAW_BUFFER13:34866,DRAW_BUFFER14:34867,DRAW_BUFFER15:34868,DRAW_BUFFER2:34855,DRAW_BUFFER3:34856,
      DRAW_BUFFER4:34857,DRAW_BUFFER5:34858,DRAW_BUFFER6:34859,DRAW_BUFFER7:34860,DRAW_BUFFER8:34861,
      DRAW_BUFFER9:34862,DRAW_FRAMEBUFFER:36009,DRAW_FRAMEBUFFER_BINDING:36006,DYNAMIC_COPY:35050,
      DYNAMIC_READ:35049,FLOAT_32_UNSIGNED_INT_24_8_REV:36269,FRAGMENT_SHADER_DERIVATIVE_HINT:35723,
      FRAMEBUFFER_ATTACHMENT_ALPHA_SIZE:33301,FRAMEBUFFER_ATTACHMENT_BLUE_SIZE:33300,
      FRAMEBUFFER_ATTACHMENT_COLOR_ENCODING:33296,FRAMEBUFFER_ATTACHMENT_COMPONENT_TYPE:33297,
      FRAMEBUFFER_ATTACHMENT_DEPTH_SIZE:33302,FRAMEBUFFER_ATTACHMENT_GREEN_SIZE:33299,
      FRAMEBUFFER_ATTACHMENT_RED_SIZE:33298,FRAMEBUFFER_ATTACHMENT_STENCIL_SIZE:33303,
      FRAMEBUFFER_ATTACHMENT_TEXTURE_LAYER:36052,FRAMEBUFFER_DEFAULT:33304,
      FRAMEBUFFER_INCOMPLETE_MULTISAMPLE:36182,HALF_FLOAT:5131,INTERLEAVED_ATTRIBS:35980,INT_2_10_10_10_REV:36255,
      INT_SAMPLER_2D:36298,INT_SAMPLER_2D_ARRAY:36303,INT_SAMPLER_3D:36299,INT_SAMPLER_CUBE:36300,
      INVALID_INDEX:4294967295,MAX:32776,MAX_3D_TEXTURE_SIZE:32883,MAX_ARRAY_TEXTURE_LAYERS:35071,
      MAX_CLIENT_WAIT_TIMEOUT_WEBGL:37447,MAX_COLOR_ATTACHMENTS:36063,
      MAX_COMBINED_FRAGMENT_UNIFORM_COMPONENTS:35379,MAX_COMBINED_UNIFORM_BLOCKS:35374,
      MAX_COMBINED_VERTEX_UNIFORM_COMPONENTS:35377,MAX_DRAW_BUFFERS:34852,MAX_ELEMENTS_INDICES:33001,
      MAX_ELEMENTS_VERTICES:33000,MAX_ELEMENT_INDEX:36203,MAX_FRAGMENT_INPUT_COMPONENTS:37157,
      MAX_FRAGMENT_UNIFORM_BLOCKS:35373,MAX_FRAGMENT_UNIFORM_COMPONENTS:35657,MAX_PROGRAM_TEXEL_OFFSET:35077,
      MAX_SAMPLES:36183,MAX_SERVER_WAIT_TIMEOUT:37137,MAX_TEXTURE_LOD_BIAS:34045,
      MAX_TRANSFORM_FEEDBACK_INTERLEAVED_COMPONENTS:35978,MAX_TRANSFORM_FEEDBACK_SEPARATE_ATTRIBS:35979,
      MAX_TRANSFORM_FEEDBACK_SEPARATE_COMPONENTS:35968,MAX_UNIFORM_BLOCK_SIZE:35376,
      MAX_UNIFORM_BUFFER_BINDINGS:35375,MAX_VARYING_COMPONENTS:35659,MAX_VERTEX_OUTPUT_COMPONENTS:37154,
      MAX_VERTEX_UNIFORM_BLOCKS:35371,MAX_VERTEX_UNIFORM_COMPONENTS:35658,MIN:32775,MIN_PROGRAM_TEXEL_OFFSET:35076,
      OBJECT_TYPE:37138,PACK_ROW_LENGTH:3330,PACK_SKIP_PIXELS:3332,PACK_SKIP_ROWS:3331,PIXEL_PACK_BUFFER:35051,
      PIXEL_PACK_BUFFER_BINDING:35053,PIXEL_UNPACK_BUFFER:35052,PIXEL_UNPACK_BUFFER_BINDING:35055,
      QUERY_RESULT:34918,QUERY_RESULT_AVAILABLE:34919,R11F_G11F_B10F:35898,R16F:33325,R16I:33331,R16UI:33332,
      R32F:33326,R32I:33333,R32UI:33334,R8:33321,R8I:33329,R8UI:33330,R8_SNORM:36756,RASTERIZER_DISCARD:35977,
      READ_BUFFER:3074,READ_FRAMEBUFFER:36008,READ_FRAMEBUFFER_BINDING:36010,RED:6403,RED_INTEGER:36244,
      RENDERBUFFER_SAMPLES:36011,RG:33319,RG16F:33327,RG16I:33337,RG16UI:33338,RG32F:33328,RG32I:33339,
      RG32UI:33340,RG8:33323,RG8I:33335,RG8UI:33336,RG8_SNORM:36757,RGB10_A2:32857,RGB10_A2UI:36975,RGB16F:34843,
      RGB16I:36233,RGB16UI:36215,RGB32F:34837,RGB32I:36227,RGB32UI:36209,RGB8I:36239,RGB8UI:36221,RGB8_SNORM:36758,
      RGB9_E5:35901,RGBA16F:34842,RGBA16I:36232,RGBA16UI:36214,RGBA32F:34836,RGBA32I:36226,RGBA32UI:36208,
      RGBA8I:36238,RGBA8UI:36220,RGBA8_SNORM:36759,RGBA_INTEGER:36249,RGB_INTEGER:36248,RG_INTEGER:33320,
      SAMPLER_2D_ARRAY:36289,SAMPLER_2D_ARRAY_SHADOW:36292,SAMPLER_2D_SHADOW:35682,SAMPLER_3D:35679,
      SAMPLER_BINDING:35097,SAMPLER_CUBE_SHADOW:36293,SEPARATE_ATTRIBS:35981,SIGNALED:37145,
      SIGNED_NORMALIZED:36764,SRGB:35904,SRGB8:35905,SRGB8_ALPHA8:35907,STATIC_COPY:35046,STATIC_READ:35045,
      STENCIL:6146,STREAM_COPY:35042,STREAM_READ:35041,SYNC_CONDITION:37139,SYNC_FENCE:37142,SYNC_FLAGS:37141,
      SYNC_FLUSH_COMMANDS_BIT:1,SYNC_GPU_COMMANDS_COMPLETE:37143,SYNC_STATUS:37140,TEXTURE_2D_ARRAY:35866,
      TEXTURE_3D:32879,TEXTURE_BASE_LEVEL:33084,TEXTURE_BINDING_2D_ARRAY:35869,TEXTURE_BINDING_3D:32874,
      TEXTURE_COMPARE_FUNC:34893,TEXTURE_COMPARE_MODE:34892,TEXTURE_IMMUTABLE_FORMAT:37167,
      TEXTURE_IMMUTABLE_LEVELS:33503,TEXTURE_MAX_LEVEL:33085,TEXTURE_MAX_LOD:33083,TEXTURE_MIN_LOD:33082,
      TEXTURE_WRAP_R:32882,TIMEOUT_EXPIRED:37147,TIMEOUT_IGNORED:-1,TRANSFORM_FEEDBACK:36386,
      TRANSFORM_FEEDBACK_ACTIVE:36388,TRANSFORM_FEEDBACK_BINDING:36389,TRANSFORM_FEEDBACK_BUFFER:35982,
      TRANSFORM_FEEDBACK_BUFFER_BINDING:35983,TRANSFORM_FEEDBACK_BUFFER_MODE:35967,
      TRANSFORM_FEEDBACK_BUFFER_SIZE:35973,TRANSFORM_FEEDBACK_BUFFER_START:35972,TRANSFORM_FEEDBACK_PAUSED:36387,
      TRANSFORM_FEEDBACK_PRIMITIVES_WRITTEN:35976,TRANSFORM_FEEDBACK_VARYINGS:35971,UNIFORM_ARRAY_STRIDE:35388,
      UNIFORM_BLOCK_ACTIVE_UNIFORMS:35394,UNIFORM_BLOCK_ACTIVE_UNIFORM_INDICES:35395,UNIFORM_BLOCK_BINDING:35391,
      UNIFORM_BLOCK_DATA_SIZE:35392,UNIFORM_BLOCK_INDEX:35386,UNIFORM_BLOCK_REFERENCED_BY_FRAGMENT_SHADER:35398,
      UNIFORM_BLOCK_REFERENCED_BY_VERTEX_SHADER:35396,UNIFORM_BUFFER:35345,UNIFORM_BUFFER_BINDING:35368,
      UNIFORM_BUFFER_OFFSET_ALIGNMENT:35380,UNIFORM_BUFFER_SIZE:35370,UNIFORM_BUFFER_START:35369,
      UNIFORM_IS_ROW_MAJOR:35390,UNIFORM_MATRIX_STRIDE:35389,UNIFORM_OFFSET:35387,UNIFORM_SIZE:35384,
      UNIFORM_TYPE:35383,UNPACK_IMAGE_HEIGHT:32878,UNPACK_ROW_LENGTH:3314,UNPACK_SKIP_IMAGES:32877,
      UNPACK_SKIP_PIXELS:3316,UNPACK_SKIP_ROWS:3315,UNSIGNALED:37144,UNSIGNED_INT_10F_11F_11F_REV:35899,
      UNSIGNED_INT_24_8:34042,UNSIGNED_INT_2_10_10_10_REV:33640,UNSIGNED_INT_5_9_9_9_REV:35902,
      UNSIGNED_INT_SAMPLER_2D:36306,UNSIGNED_INT_SAMPLER_2D_ARRAY:36311,UNSIGNED_INT_SAMPLER_3D:36307,
      UNSIGNED_INT_SAMPLER_CUBE:36308,UNSIGNED_INT_VEC2:36294,UNSIGNED_INT_VEC3:36295,UNSIGNED_INT_VEC4:36296,
      UNSIGNED_NORMALIZED:35863,VERTEX_ARRAY_BINDING:34229,VERTEX_ATTRIB_ARRAY_DIVISOR:35070,
      VERTEX_ATTRIB_ARRAY_INTEGER:35069,WAIT_FAILED:37149};
    const UNMASKED_VENDOR = 0x9245, UNMASKED_RENDERER = 0x9246, ANISO = 0x84ff;
    const patchGl = (gl, is2) => {
      const M = new Map();
      const set = (name, lit, val) => { const k = (gl[name] !== undefined ? gl[name] : lit); M.set(k, val); };
      set("VENDOR", 0x1f00, "WebKit");
      set("RENDERER", 0x1f01, "WebKit WebGL");
      set("VERSION", 0x1f02, is2 ? "WebGL 2.0 (OpenGL ES 3.0 Chromium)" : "WebGL 1.0 (OpenGL ES 2.0 Chromium)");
      set("SHADING_LANGUAGE_VERSION", 0x8b8c, is2 ? "WebGL GLSL ES 3.00 (OpenGL ES GLSL ES 3.0 Chromium)" : "WebGL GLSL ES 1.0 (OpenGL ES GLSL ES 1.0 Chromium)");
      M.set(UNMASKED_VENDOR, adapter.vendor || "turbo-surf");
      M.set(UNMASKED_RENDERER, adapter.renderer || "turbo-surf (GPU unavailable)");
      set("MAX_TEXTURE_SIZE", 0x0d33, 16384);
      set("MAX_CUBE_MAP_TEXTURE_SIZE", 0x851c, 16384);
      set("MAX_RENDERBUFFER_SIZE", 0x84e8, 16384);
      set("MAX_VIEWPORT_DIMS", 0x0d3a, new Int32Array([16384, 16384]));
      set("MAX_VERTEX_ATTRIBS", 0x8869, 16);
      set("MAX_VERTEX_UNIFORM_VECTORS", 0x8dfb, 1024);
      set("MAX_FRAGMENT_UNIFORM_VECTORS", 0x8dfd, 1024);
      set("MAX_VARYING_VECTORS", 0x8dfc, 30);
      set("MAX_VERTEX_TEXTURE_IMAGE_UNITS", 0x8b4c, 16);
      set("MAX_TEXTURE_IMAGE_UNITS", 0x8872, 16);
      set("MAX_COMBINED_TEXTURE_IMAGE_UNITS", 0x8b4d, 32);
      set("ALIASED_LINE_WIDTH_RANGE", 0x846e, new Float32Array([1, 1]));
      set("ALIASED_POINT_SIZE_RANGE", 0x846d, new Float32Array([1, 511]));
      set("RED_BITS", 0x0d52, 8); set("GREEN_BITS", 0x0d53, 8); set("BLUE_BITS", 0x0d54, 8);
      set("ALPHA_BITS", 0x0d55, 8); set("DEPTH_BITS", 0x0d56, 24); set("STENCIL_BITS", 0x0d57, 0);
      M.set(ANISO, 16);
      if (is2) {
        set("MAX_3D_TEXTURE_SIZE", 0x8073, 2048);
        set("MAX_ARRAY_TEXTURE_LAYERS", 0x88ff, 2048);
        set("MAX_DRAW_BUFFERS", 0x8824, 8);
        set("MAX_COLOR_ATTACHMENTS", 0x8cdf, 8);
        set("MAX_SAMPLES", 0x8d57, 4);
        set("MAX_UNIFORM_BUFFER_BINDINGS", 0x8a2f, 32);
      }
      const origGetParam = gl.getParameter ? gl.getParameter.bind(gl) : () => null;
      gl.getParameter = function (p) { return M.has(p) ? M.get(p) : origGetParam(p); };
      gl.getSupportedExtensions = function () { return (is2 ? EXT2 : EXT1).slice(); };
      const origGetExt = gl.getExtension ? gl.getExtension.bind(gl) : () => null;
      gl.getExtension = function (name) {
        if (name === "WEBGL_debug_renderer_info") return { UNMASKED_VENDOR_WEBGL: UNMASKED_VENDOR, UNMASKED_RENDERER_WEBGL: UNMASKED_RENDERER };
        if (name === "EXT_texture_filter_anisotropic") return { MAX_TEXTURE_MAX_ANISOTROPY_EXT: ANISO, TEXTURE_MAX_ANISOTROPY_EXT: 0x84fe };
        const known = (is2 ? EXT2 : EXT1).indexOf(name) >= 0;
        const r = origGetExt(name);
        return r || (known ? {} : null);
      };
    };
    // Native-mark every own function on a context instance (they're per-instance own funcs).
    const markOwn = (obj) => {
      if (!obj) return obj;
      for (const k of Object.getOwnPropertyNames(obj)) {
        let d; try { d = Object.getOwnPropertyDescriptor(obj, k); } catch (e) { continue; }
        if (d && typeof d.value === "function") mark(d.value, k);
      }
      return obj;
    };
    // Live WebGL→GPU bridge: override the drawing/state methods to RECORD each call with its
    // REAL args (buffer bytes + shader source — captured at the live call site, so nothing is
    // lossy) into a batch, and on readPixels flush the batch to the host GPU executor
    // (op_webgl_readback) and fill the destination from genuine GPU pixels. Only installed when a
    // host executor is present; otherwise the vendored synthetic context is left as-is. getShader/
    // Program*Parameter report success so a page's compile/link checks pass. Returns real GPU
    // pixels for the common fingerprint draw (shaders + a vertex buffer + drawArrays + readPixels).
    const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    const enc64 = (u8) => {
      let o = "";
      for (let i = 0; i < u8.length; i += 3) {
        const a = u8[i], b = i + 1 < u8.length ? u8[i + 1] : 0, c = i + 2 < u8.length ? u8[i + 2] : 0;
        o += B64[a >> 2] + B64[((a & 3) << 4) | (b >> 4)];
        o += i + 1 < u8.length ? B64[((b & 15) << 2) | (c >> 6)] : "=";
        o += i + 2 < u8.length ? B64[c & 63] : "=";
      }
      return o;
    };
    const dec64 = (s) => {
      const L = {}; for (let i = 0; i < 64; i++) L[B64[i]] = i;
      const out = []; let buf = 0, bits = 0;
      for (let i = 0; i < s.length; i++) {
        const v = L[s[i]]; if (v === undefined) continue;
        buf = (buf << 6) | v; bits += 6;
        if (bits >= 8) { bits -= 8; out.push((buf >> bits) & 0xff); }
      }
      return out;
    };
    const installWebglRecorder = (gl) => {
      const op = (ops && ops.op_webgl_readback) || null;
      // op_webgl_readback is ALWAYS registered, so `typeof op` can't tell whether a GPU executor
      // is installed. Only override the (vendored synthetic) WebGL context when one actually is —
      // otherwise the default build would replace the context with recording stubs for nothing.
      const avail = ops && ops.op_webgl_available;
      if (!op || !avail || !avail()) return;
      const rec = [];
      let hid = 0, lid = 0;
      const H = () => ({ __h: ++hid });
      const bytesOf = (view) => {
        try {
          if (view instanceof ArrayBuffer) return new Uint8Array(view);
          if (view && view.buffer) return new Uint8Array(view.buffer, view.byteOffset || 0, view.byteLength);
        } catch (e) {}
        return new Uint8Array(0);
      };
      const bufType = (v) => (v instanceof Float32Array ? "f32" : v instanceof Uint16Array ? "u16" : "u8");
      const put = (m, a, id) => { const o = { m, a }; if (id != null) o.id = id; rec.push(o); };
      const origReadPixels = gl.readPixels;
      gl.createShader = function (type) { const h = H(); put("createShader", [type], h.__h); return h; };
      gl.shaderSource = function (sh, src) { put("shaderSource", [sh && sh.__h, String(src)]); };
      gl.compileShader = function (sh) { put("compileShader", [sh && sh.__h]); };
      gl.getShaderParameter = function () { return true; };
      gl.getProgramParameter = function () { return true; };
      gl.getShaderInfoLog = function () { return ""; };
      gl.getProgramInfoLog = function () { return ""; };
      gl.createProgram = function () { const h = H(); put("createProgram", [], h.__h); return h; };
      gl.attachShader = function (p, s) { put("attachShader", [p && p.__h, s && s.__h]); };
      gl.linkProgram = function (p) { put("linkProgram", [p && p.__h]); };
      gl.useProgram = function (p) { put("useProgram", [p && p.__h]); };
      gl.createBuffer = function () { const h = H(); put("createBuffer", [], h.__h); return h; };
      gl.bindBuffer = function (t, b) { put("bindBuffer", [t, b ? b.__h : 0]); };
      gl.bufferData = function (t, data, usage) {
        if (typeof data === "number") put("bufferData", [t, { b64: "", t: "u8" }, usage]);
        else put("bufferData", [t, { b64: enc64(bytesOf(data)), t: bufType(data) }, usage]);
      };
      gl.getAttribLocation = function (p, name) { const loc = lid++; put("attribLocation", [p && p.__h, loc, String(name)]); return loc; };
      gl.enableVertexAttribArray = function (loc) { put("enableVertexAttribArray", [loc]); };
      gl.vertexAttribPointer = function (loc, size, type, norm, stride, offset) { put("vertexAttribPointer", [loc, size, type, !!norm, stride | 0, offset | 0]); };
      gl.getUniformLocation = function (p, name) { return { __p: p && p.__h, __u: String(name) }; };
      const uni = (kind) => function (loc) { const vals = Array.prototype.slice.call(arguments, 1); if (loc) put("uniform", [loc.__p, loc.__u, kind, vals]); };
      gl.uniform1f = uni("1f"); gl.uniform2f = uni("2f"); gl.uniform3f = uni("3f"); gl.uniform4f = uni("4f"); gl.uniform1i = uni("1i");
      gl.uniformMatrix4fv = function (loc, transpose, val) { if (loc) put("uniform", [loc.__p, loc.__u, "Matrix4fv", Array.prototype.slice.call(val || [])]); };
      gl.viewport = function (x, y, w, h) { put("viewport", [x, y, w, h]); };
      gl.clearColor = function (r, g, b, a) { put("clearColor", [r, g, b, a]); };
      gl.clear = function (mask) { put("clear", [mask]); };
      gl.drawArrays = function (mode, first, count) { put("drawArrays", [mode, first, count]); };
      gl.drawElements = function (mode, count, type, offset) { put("drawElements", [mode, count, type, offset | 0]); };
      gl.readPixels = function (x, y, w, h, format, type, dst) {
        try {
          const W = gl.drawingBufferWidth || (gl.canvas && gl.canvas.width) || 300;
          const Hh = gl.drawingBufferHeight || (gl.canvas && gl.canvas.height) || 150;
          const out = op(W, Hh, JSON.stringify(rec));
          if (out && dst && dst.length) {
            const bin = dec64(out); // framebuffer RGBA8, top-left origin, W×Hh
            for (let row = 0; row < h; row++) {
              const srcRow = Hh - 1 - (y + row); // readPixels is bottom-left origin → flip
              for (let col = 0; col < w; col++) {
                const si = (srcRow * W + (x + col)) * 4, di = (row * w + col) * 4;
                for (let c = 0; c < 4; c++) dst[di + c] = si + c < bin.length ? bin[si + c] : 0;
              }
            }
            return;
          }
        } catch (e) {}
        if (typeof origReadPixels === "function") { try { return origReadPixels.call(gl, x, y, w, h, format, type, dst); } catch (e) {} }
      };
    };
    const tagNameGetter = __G.domAccessor("tagName", false, "Element");
    const requireCanvas = receiver => {
      if (tagNameGetter.call(receiver) !== "CANVAS") throw new TypeError("Illegal invocation");
    };
    for (const [key, fallback] of [["width",300],["height",150]]) {
      const get = Object.getOwnPropertyDescriptor({get [key]() {
        requireCanvas(this);
        const raw = this.getAttribute(key), match = raw != null && /^[\t\n\f\r ]*\+?(\d+)/.exec(raw);
        const value = match ? Number(match[1]) : fallback;
        return value <= 2147483647 ? value : fallback;
      }},key).get;
      const set = Object.getOwnPropertyDescriptor({set [key](value) {
        requireCanvas(this);const size=Number(value)>>>0;
        this.setAttribute(key,String(size));
        if(this.__ctx2d && this.__ctx2d._ops)this.__ctx2d._ops.length=0;
      }},key).set;
      mark(get,"get "+key);mark(set,"set "+key);
      Object.defineProperty(canvasProto,key,{get,set,enumerable:true,configurable:true});
    }
    const origGetContext = canvasProto.getContext;
    // getContext returns the SAME context object on every call for a given (canvas, kind),
    // so a page that calls it twice would otherwise re-wrap getParameter/readPixels over our
    // own overrides — double-recording draws and eventually stack-overflowing the bind chain.
    // Track patched contexts so each is spoofed + instrumented exactly once.
    const glPatched = new WeakSet();
    const wrapped = function getContext(kind, opts) {
      requireCanvas(this);
      if (!arguments.length) throw new TypeError("A context type is required");
      const ctx = origGetContext.call(this, kind, opts);
      if (ctx) {
        const k = String(kind || "");
        if ((k === "webgl" || k === "experimental-webgl" || k === "webgl2") && !glPatched.has(ctx)) {
          glPatched.add(ctx);
          try { patchGl(ctx, k === "webgl2"); } catch (e) {}
          try { installWebglRecorder(ctx); } catch (e) {}
          // Give the context its OWN dedicated prototype carrying its methods, then wire it to
          // the named global constructor, so `gl instanceof WebGLRenderingContext` holds and
          // `WebGLRenderingContext.prototype.getParameter` resolves natively. We must NOT reuse
          // the vendored context's shared fallback prototype (a generic object also backing
          // `location` etc.) as the constructor prototype — that would make `location instanceof
          // WebGLRenderingContext` true and collapse the webgl/webgl2 protos onto one, both worse
          // tells. The context methods are per-instance bound closures (they don't read `this`),
          // so moving them onto a private per-context prototype is behaviour-preserving.
          try {
            const ctorName = k === "webgl2" ? "WebGL2RenderingContext" : "WebGLRenderingContext";
            const C = globalThis[ctorName];
            if (C) {
              const proto = {};
              for (const nm of Object.getOwnPropertyNames(ctx)) {
                const dsc = Object.getOwnPropertyDescriptor(ctx, nm);
                if (dsc && typeof dsc.value === "function") {
                  // Prototype-less native shape (no own `prototype`), prototype-resident + marked.
                  const nf = nativize(dsc.value, nm);
                  Object.defineProperty(proto, nm, { value: nf, configurable: true, writable: true });
                  try { delete ctx[nm]; } catch (e) {}
                }
              }
              // The full WebGL enum constants live on the prototype (like Chrome), so `gl.X`
              // resolves for real WebGL code + the recorder captures real enum args (not undefined).
              const consts = k === "webgl2" ? Object.assign({}, GL_CONST1, GL_CONST2) : GL_CONST1;
              for (const cn in consts) {
                if (!(cn in proto)) {
                  Object.defineProperty(proto, cn, { value: consts[cn], enumerable: false, configurable: true, writable: false });
                }
              }
              Object.setPrototypeOf(proto, Object.getPrototypeOf(ctx));
              Object.defineProperty(proto, Symbol.toStringTag, { value: ctorName, configurable: true });
              Object.defineProperty(proto, "constructor", { value: C, configurable: true, writable: true });
              C.prototype = proto;
              Object.setPrototypeOf(ctx, proto);
            }
          } catch (e) {}
        }
        // 2D context: replace the vendored getImageData (an OWN instance method returning synthetic
        // bytes — `fillRect(red); getImageData` read back NON-red, a broken-canvas tell) with one
        // that reads the ACTUAL rendered pixels back through the raw-RGBA rasterizer. Raw pixels
        // have no PNG encoder in the loop, so shapes/solids read back exactly like a real browser.
        if (k === "2d") {
          // Class tag: real Chrome is "[object CanvasRenderingContext2D]"; the vendored ctx tagged
          // as "[object DOMImplementation]" (its shared fallback proto) — a tell. Brand the instance
          // (not the shared proto — that would mistag every other object backed by it, incl.
          // `location`, like the WebGL-proto lesson).
          try {
            Object.defineProperty(ctx, Symbol.toStringTag, { value: "CanvasRenderingContext2D", configurable: true });
          } catch (e) {}
          // getContextAttributes: real Chrome exposes it; its absence is a tell. Return the default
          // 2D context attributes (native-masked), matching a fresh getContext('2d').
          if (typeof ctx.getContextAttributes !== "function") {
            ctx.getContextAttributes = nativize(({ getContextAttributes() {
              return { alpha: true, colorSpace: "srgb", colorType: "unorm8", desynchronized: false, toneMapping: { mode: "standard" }, willReadFrequently: false };
            } }).getContextAttributes, "getContextAttributes", 0);
          }
          // measureText: the vendored width was a synthetic approximation (a font-metrics tell —
          // font-detection fingerprints compare measureText width across families). Route the WIDTH
          // through the host's real system-font measurer (op_measure_text), which measures the same
          // CoreText face Chrome does → width matches Chrome exactly. The bounding-box fields are
          // derived from the size with Arial-typical ratios (the measurer returns width+height only;
          // real per-glyph boxes would need a richer measurer — width is the load-bearing metric).
          const measureOp = ops && ops.op_measure_text;
          if (typeof ctx.measureText === "function" && measureOp) {
            const origMT = ctx.measureText;
            ctx.measureText = nativize(({ measureText(text) {
              try {
                const font = String(this.font || "10px sans-serif");
                const m = /(\d+(?:\.\d+)?)px\s+(.+)$/.exec(font);
                const size = m ? parseFloat(m[1]) : 10;
                const fam = m ? m[2].replace(/^['"]|['"]$/g, "").split(",")[0].trim() : "sans-serif";
                const r = JSON.parse(measureOp(String(text == null ? "" : text), fam, size));
                if (r && r.length === 2) {
                  const w = r[0];
                  const round2 = (n) => Math.round(n * 100) / 100;
                  const asc = round2(size * 0.875), desc = round2(size * 0.212);
                  return {
                    width: w,
                    actualBoundingBoxLeft: 0, actualBoundingBoxRight: w,
                    actualBoundingBoxAscent: round2(size * 0.72), actualBoundingBoxDescent: round2(size * 0.16),
                    fontBoundingBoxAscent: asc, fontBoundingBoxDescent: desc,
                    emHeightAscent: asc, emHeightDescent: desc,
                    hangingBaseline: round2(asc * 0.8), alphabeticBaseline: 0, ideographicBaseline: -desc,
                  };
                }
              } catch (e) {}
              return origMT.call(this, text);
            } }).measureText, "measureText", 1);
          }
        }
        if (k === "2d" && Object.prototype.hasOwnProperty.call(ctx, "getImageData")) {
          const origGID = ctx.getImageData;
          const rgbaOp2 = ops && ops.op_raster_rgba;
          const b64ToBytes = (b64) => { try { const s = atob(b64); const u = new Uint8Array(s.length); for (let i = 0; i < s.length; i++) u[i] = s.charCodeAt(i); return u; } catch (e) { return null; } };
          ctx.getImageData = nativize(({ getImageData(sx, sy, sw, sh) {
            try {
              const cv = this.canvas, ops = this._ops;
              const W = (cv && cv.width) || 300, H = (cv && cv.height) || 150;
              if (rgbaOp2 && cv && ops && sw > 0 && sh > 0) {
                const full = b64ToBytes(rgbaOp2(W, H, JSON.stringify(ops)));
                if (full && full.length >= W * H * 4) {
                  sx = sx | 0; sy = sy | 0; sw = sw | 0; sh = sh | 0;
                  const out = new Uint8ClampedArray(sw * sh * 4);
                  for (let row = 0; row < sh; row++) {
                    for (let col = 0; col < sw; col++) {
                      const yy = sy + row, xx = sx + col, di = (row * sw + col) * 4;
                      if (yy >= 0 && yy < H && xx >= 0 && xx < W) {
                        const si = (yy * W + xx) * 4;
                        out[di] = full[si]; out[di + 1] = full[si + 1]; out[di + 2] = full[si + 2]; out[di + 3] = full[si + 3];
                      }
                    }
                  }
                  // The stub `ImageData` ctor ignores its args (no `.data`), so build the object
                  // directly and brand it so `Object.prototype.toString.call(img)` reads ImageData.
                  const img = { data: out, width: sw, height: sh, colorSpace: "srgb" };
                  try { Object.defineProperty(img, Symbol.toStringTag, { value: "ImageData", configurable: true }); } catch (e) {}
                  return img;
                }
              }
            } catch (e) {}
            return origGID ? origGID.call(this, sx, sy, sw, sh) : { data: new Uint8ClampedArray(Math.max(0, sw | 0) * Math.max(0, sh | 0) * 4), width: sw | 0, height: sh | 0 };
          } }).getImageData, "getImageData", 4);
        }
        markOwn(ctx);
      }
      return ctx;
    };
    // Prototype-less native shape (native methods have no own `prototype`); getContext.length === 1.
    canvasProto.getContext = nativize(wrapped, "getContext", 1);
    // Raster-backed toDataURL: replay the vendored 2D display list (ctx._ops) through the
    // host rasterizer (op_raster_png) into a REAL PNG. The vendored stub returns a ~94-byte
    // synthetic blob — an impossible size for rendered content, a canvas-fingerprint tell.
    // Falls back to the vendored blob when no rasterizer is installed or on any error.
    const rasterOp = (ops && ops.op_raster_png) || null;
    const origToDataURL = canvasProto.toDataURL;
    if (typeof origToDataURL === "function") {
      // Concise-method form (no own `prototype`, native shape); toDataURL.length === 0.
      canvasProto.toDataURL = nativize(({ toDataURL(type) {
        requireCanvas(this);
        if (this.width === 0 || this.height === 0) return "data:,";
        // The raster op only encodes PNG. For a non-PNG request (e.g. the webp-support probe
        // toDataURL('image/webp') or an explicit image/jpeg) defer to the vendored path, which
        // labels the data URL with the requested MIME — returning PNG for a webp request would be
        // both wrong bytes and a tell.
        const mime = typeof type === "string" && type ? type.toLowerCase() : "image/png";
        try {
          const ctx = this.__ctx2d;
          if (mime === "image/png" && rasterOp && ctx && ctx._ops) {
            const b64 = rasterOp(this.width || 300, this.height || 150, JSON.stringify(ctx._ops));
            if (b64) return "data:image/png;base64," + b64;
          }
        } catch (e) {}
        return origToDataURL.call(this, type);
      } }).toDataURL, "toDataURL", 0);
    }
    if (typeof canvasProto.toBlob === "function") canvasProto.toBlob = nativize(canvasProto.toBlob, "toBlob", 1);
    // Canvas inherits the genuine Element geometry callback; do not shadow it.

  });

  // The canonical binding owns HTMLDocument → Document → Node. Re-home only
  // this runtime's resource/frame wrapper, without inserting another prototype.
  __documentRuntimeState.createElement = G.document.createElement;
  const createElement = ({createElement(tag, options = undefined) {
    const state = __documentState(this);
    if (!arguments.length) throw new TypeError("A tag name is required");
    return state.createElement(String(tag), options);
  }}).createElement;
  Object.defineProperty(G.Document.prototype, "createElement", {value: __documentFunction(createElement, "createElement", 1),
    writable: true, configurable: true, enumerable: true});
  delete G.document.createElement;
  if (Object.hasOwn(G.document, Symbol.toStringTag)) delete G.document[Symbol.toStringTag];

  // window / history / console brands (WINDOW_BRANDS: note console's brand is lowercase).
  guard(() => { if(G.Window?.prototype)tag(G.Window.prototype,"Window"); });
  guard(() => { if (G.console) { tag(G.console, "console"); ["log", "info", "warn", "error", "debug"].forEach((m) => mark(G.console[m], m)); } });
  guard(() => {
    const object = G.performance;
    if (!object) return;
    function Performance() { throw new TypeError("Illegal constructor"); }
    const proto = Object.create(G.EventTarget.prototype);
    Object.defineProperty(proto, "constructor", {value: Performance, configurable: true, writable: true});
    Performance.prototype = proto;
    Object.defineProperty(Performance, "prototype", {writable: false});
    tag(proto, "Performance"); mark(Performance, "Performance");
    for (const key of Reflect.ownKeys(object)) {
      if (typeof key !== "string") {delete object[key];continue;}
      const own = Object.getOwnPropertyDescriptor(object,key);
      if (own?.get) {
        const getter = Object.getOwnPropertyDescriptor({get value(){if(this!==object)throw new TypeError("Illegal invocation");return own.get.call(object);}},"value").get;
        Object.defineProperty(getter,"name",{value:"get "+key,configurable:true});
        const setter=own.set?Object.getOwnPropertyDescriptor({set value(value){if(this!==object)throw new TypeError("Illegal invocation");own.set.call(object,value);}},"value").set:undefined;
        if(setter)Object.defineProperty(setter,"name",{value:"set "+key,configurable:true});
        Object.defineProperty(proto,key,{get:getter,set:setter,enumerable:true,configurable:true});delete object[key];continue;
      }
      const value = object[key];
      if (typeof value === "function") {
        const method = ({[key](...args) {
          if (this !== object) throw new TypeError("Illegal invocation");
          return value.apply(object, args);
        }})[key];
        Object.defineProperty(method, "length", {value: value.length, configurable: true});
        mark(method, key);
        Object.defineProperty(proto, key, {value: method, writable: true, configurable: true, enumerable: true});
      } else {
        const getter = Object.getOwnPropertyDescriptor({get [key]() {
          if (this !== object) throw new TypeError("Illegal invocation");
          return value;
        }}, key).get;
        mark(getter, "get " + key);
        Object.defineProperty(proto, key, {get: getter, configurable: true, enumerable: true});
      }
      delete object[key];
    }
    Object.setPrototypeOf(object, proto);
    G.Performance = Performance;
  });
  // Normalize clock names here; the final native factory binds both the
  // callable Date constructor and its coherent now callback.
  guard(() => { if (G.Date) mark(G.Date, "Date"); });
  guard(() => { if (G.Date && G.Date.now) mark(G.Date.now, "now"); });

  // createElement was re-wrapped as a JS closure above; re-mark it native (create-element-not-native).
  guard(() => { if (G.document && G.document.createElement) mark(G.document.createElement, "createElement"); });

  guard(() => { if (G.CSS) mark(G.CSS.supports, "supports"); });

  // Constructed-object host interfaces we own in JS (brand + native method). The 9 rtdom-native
  // element/Range/Text brands need vendored-binding work and stay as residual struct fails.
  const defClass = (name, method, ctorArgsOk) => guard(() => {
    let C = G[name];
    if (typeof C !== "function") { C = function () {}; Object.defineProperty(C, "name", { value: name, configurable: true }); G[name] = C; }
    mark(C, name);
    C.prototype = C.prototype || {};
    tag(C.prototype, name);
    if (method && typeof C.prototype[method] !== "function") { C.prototype[method] = function () {}; }
    if (method) { try { Object.defineProperty(C.prototype[method], "name", { value: method, configurable: true }); } catch (e) {} mark(C.prototype[method]); }
  });
  defClass("Blob", "slice");
  defClass("Headers", null);
  defClass("XMLHttpRequest", "open");
  // defClass native-marks only the one probed method (`open`); mark the rest of the XHR
  // surface too, so `.toString()` on any of them reads native.
  guard(() => {
    const xp = G.XMLHttpRequest && G.XMLHttpRequest.prototype;
    if (xp) ["send", "setRequestHeader", "getResponseHeader", "getAllResponseHeaders",
      "abort", "addEventListener", "removeEventListener", "dispatchEvent"].forEach((m) => {
      if (typeof xp[m] === "function") mark(xp[m], m);
    });
  });
  defClass("URL", null);
  defClass("Event", null);
  defClass("DOMParser", "parseFromString");


  // rtdom nodes/elements expose DISTINCT per-tag prototypes reachable from JS, so brand each
  // (Symbol.toStringTag on the tag's own prototype) and native-mark its checked method IN PLACE
  // (mark the existing function, never shadow it) — clears struct-brand-table + struct-method-not-native
  // without touching the vendored DOM binding.
  guard(() => {
    const brandNode = (obj, name, method) => {
      if (!obj) return;
      let proto = Object.getPrototypeOf(obj);
      // NEVER brand a shared root proto: some rtdom singletons (e.g. document.implementation) have
      // Object.prototype as their direct proto, so tagging the proto would set Symbol.toStringTag on
      // Object.prototype — making Object.prototype.toString.call({}) / [] / new Date() all read the
      // wrong brand (a trivial, high-severity bot tell). Brand the INSTANCE itself in that case.
      if (proto === Object.prototype || proto === null) {
        tag(obj, name);
        // Brand the instance's OWN constructor too (a singleton like document.implementation has
        // Object.prototype as its proto, so it would otherwise report `.constructor === Object`).
        const ctor = ({ [name]: function () {} })[name];
        try { Object.defineProperty(obj, "constructor", { value: ctor, configurable: true }); mark(ctor, name); } catch (e) {}
        proto = null;
      }
      if (proto) {
        tag(proto, name);
        const ctor = typeof G[name] === "function" ? G[name] : ({ [name]: function () {} })[name];
        try { Object.defineProperty(proto, "constructor", { value: ctor, configurable: true }); mark(ctor, name); } catch (e) {}
      }
      if (method) {
        let fn; try { fn = obj[method]; } catch (e) {}
        if (typeof fn !== "function" && proto) { proto[method] = function () {}; fn = proto[method]; try { Object.defineProperty(fn, "name", { value: method, configurable: true }); } catch (e) {} }
        if (typeof fn === "function") mark(fn);
      }
    };
    const d = G.document;
    if (d) {
      brandNode(d.implementation, "DOMImplementation", null);
      brandNode(d.createRange && d.createRange(), "Range", "cloneRange");
    }
  });
  // The canonical binding supplies distinct branded interface prototypes for
  // elements, Text, Comment and DocumentFragment. Do not replace their constant
  // tags with the old shared-prototype fallback getter.

  // Presence of universal host constructors (no-webgl-context / no-audio-context / surface-missing).
  const ensureCtor = (name) => guard(() => { if (typeof G[name] !== "function") { const c = function () {}; Object.defineProperty(c, "name", { value: name, configurable: true }); G[name] = c; } mark(G[name], name); });
  ["WebGLRenderingContext", "WebGL2RenderingContext",
   "IntersectionObserver", "ResizeObserver", "AbortController", "URL", "PerformanceObserver",
   "MutationObserver", "Worker"].forEach(ensureCtor);
  guard(() => { ["fetch", "requestAnimationFrame", "queueMicrotask", "matchMedia"].forEach((n) => { if (typeof G[n] === "function") mark(G[n], n); }); });

  // Document encoding accessors — a real Chrome document reports characterSet/charset/
  // inputEncoding = "UTF-8"; the vendored no-layout document leaves them undefined, a tell
  // an anti-bot collector reads (`document.characterSet`). Define on the document's own
  // prototype so all documents (incl. child realms) inherit; skip if already present.
  guard(() => {
    const d = G.document; if (!d) return;
    for (const key of ["characterSet", "charset", "inputEncoding"]) __defineDocumentAccessor(key, () => "UTF-8");
    for (const key of ["onchange", "onclick", "oninput", "onkeydown", "onkeyup", "onreadystatechange", "onvisibilitychange", "onfullscreenchange", "onfullscreenerror", "onpointerlockchange", "onpointerlockerror", "onsecuritypolicyviolation", "onbeforecopy", "onbeforecut", "onbeforepaste", "onfreeze", "onresume", "onsearch"]) {
      __defineDocumentAccessor(key, state => state.handlers.get(key) || null,
        (state, _receiver, value) => {
          const callback=typeof value === "function" ? value : null;
          const type=key.slice(2), previous=state.handlers.get(key);
          if(previous===callback)return;
          state.handlers.set(key,callback);
          state.handlerListeners ||= new Map();
          const listener=state.handlerListeners.get(key);
          if(callback&&!listener){
            const listener=function(event){const callback=state.handlers.get(key);if(callback)callback.call(this,event);};
            state.handlerListeners.set(key,listener);state.document.addEventListener(type,listener);
          }else if(!callback&&listener){state.document.removeEventListener(type,listener);state.handlerListeners.delete(key);}
        });
    }
  });

  // Keep creation-context delegates private: a retained Document must not resolve
  // these through a WindowProxy that may already point at a newer navigation.
  // getSelection is not delegated: the binding (selection.js) defines it on
  // Document.prototype over per-document native state, so a retained or inert
  // document already gets its own answer (null without a browsing context).
  __documentRuntimeState.members = __newDocumentMembers();
  for (const name of ["close", "open", "write", "writeln", "createEvent", "createNodeIterator", "createRange", "createTreeWalker", "elementFromPoint", "elementsFromPoint", "hasFocus", "implementation", "adoptedStyleSheets", "styleSheets", "contentType", "hidden", "visibilityState", "fullscreenElement"]) {
    const descriptor = Object.getOwnPropertyDescriptor(__document, name);
    if (!descriptor || descriptor.configurable === false) continue;
    __documentRuntimeState.members.set(name, descriptor);
    if (typeof descriptor.value === "function") {
      const method = ({[name](...args) {
        const state = __documentState(this);
        return state.members.get(name).value.apply(this, args);
      }})[name];
      Object.defineProperty(Document.prototype, name, {value: __documentFunction(method, name, descriptor.value.length),
        configurable: true, enumerable: true, writable: true});
      delete __document[name];
    } else {
      __defineDocumentAccessor(name, (state, receiver) => {
        const member = state.members.get(name);
        return member.get ? member.get.call(receiver) : member.value;
      }, descriptor.set ? (state, receiver, value) => state.members.get(name).set.call(receiver, value)
        : name==="adoptedStyleSheets" && descriptor.writable ? (state, _receiver, value) => {state.members.get(name).value=value;} : undefined);
    }
  }
  // A member the binding already defines on Document.prototype (fullscreenElement)
  // is not an own document property; capture its native getter before guest code runs.
  const __aliasPrototypeGetters = new Map();
  for (const name of ["hidden", "visibilityState", "fullscreenElement"]) {
    const getter = Object.getOwnPropertyDescriptor(Document.prototype, name)?.get;
    if (getter) __aliasPrototypeGetters.set(name, getter);
  }
  for (const [alias, name] of Object.entries({webkitHidden:"hidden", webkitVisibilityState:"visibilityState",
    webkitFullscreenElement:"fullscreenElement", webkitCurrentFullScreenElement:"fullscreenElement",
    webkitIsFullScreen:"fullscreenElement"})) {
    __defineDocumentAccessor(alias, (state, receiver) => {
      const member=state.members.get(name);
      const value=member ? (member.get ? member.get.call(receiver) : member.value)
        : __aliasPrototypeGetters.get(name).call(receiver);
      return alias==="webkitIsFullScreen" ? value!=null : value;
    });
  }
  __defineDocumentAccessor("defaultView", state => ops.op_document_has_active_window(state.document) ? state.window : null);
  __defineDocumentAccessor("documentElement", (_state, receiver) => receiver.firstElementChild);
  __defineDocumentAccessor("head", (_state, receiver) => {
    const root = receiver.documentElement;
    return root ? Array.from(root.children).find(child => child.localName === "head") || null : null;
  });
  __defineDocumentAccessor("body", (_state, receiver) => {
    const root = receiver.documentElement;
    return root ? Array.from(root.children).find(child => child.localName === "body" || child.localName === "frameset") || null : null;
  }, (_state, receiver, value) => {
    if (value != null) __documentType.call(value);
    if (!value || !["body", "frameset"].includes(value.localName) || value.namespaceURI !== "http://www.w3.org/1999/xhtml")
      throw new DOMException("The new body is not a body or frameset element", "HierarchyRequestError");
    const current = receiver.body;
    if (value === current) return;
    if (current) current.parentNode.replaceChild(value, current);
    else if (receiver.documentElement) receiver.documentElement.appendChild(value);
    else throw new DOMException("The document has no document element", "HierarchyRequestError");
  });

  // Native-mask the Permissions API method: real `navigator.permissions.query.toString()`
  // reports "[native code]"; our JS closure would leak source (an anti-tamper tell).
  guard(() => {
    const permissions = G.navigator && G.navigator.permissions, query = permissions && permissions.query;
    if (!permissions || typeof query !== "function") return;
    function Permissions() {throw new TypeError("Illegal constructor");}
    const proto = Object.create(Object.prototype);
    Object.defineProperty(proto, "constructor", {value: Permissions, configurable: true, writable: true});
    Permissions.prototype = proto; Object.defineProperty(Permissions, "prototype", {writable: false});
    tag(proto, "Permissions");mark(Permissions, "Permissions");
    const ErrorType = G.TypeError, apply = Reflect.apply,
      promiseReject = G.Promise.reject, PromiseType=G.Promise,
      records=__interfaceMap("Permissions.query");
    records.set(permissions,query);
    const method = ({query(descriptor) {
      const implementation=records.get(this);
      if (!implementation || !arguments.length)
        return apply(promiseReject,PromiseType,[new ErrorType("Illegal invocation")]);
      return apply(implementation, this, [descriptor]);
    }}).query;
    mark(method, "query");
    Object.defineProperty(proto, "query", {value: method, enumerable: true, writable: true, configurable: true});
    delete permissions.query; Object.setPrototypeOf(permissions, proto); G.Permissions = Permissions;
  });


  // offsetWidth / offsetHeight via the host's real font measurer (op_measure_text, system fonts).
  // A no-layout DOM reports neither, so a font-detection probe (span metrics per font-family) sees
  // fontCount 0 (no-system-fonts). Defined on the base element/node prototype so all elements inherit;
  // measures the node's own text under its computed font. No host measurer installed => 0 (honest).
  guard(() => {
    const measure = (elm) => {
      try {
        const t = elm && elm.textContent; if (!t) return null;
        const op = ops.op_measure_text; if (!op) return null;
        const cs = G.getComputedStyle(elm);
        const fam = (cs && cs.fontFamily) || "sans-serif";
        const size = parseFloat((cs && cs.fontSize) || "16") || 16;
        const r = JSON.parse(op(String(t), String(fam), size));
        return r && r.length === 2 ? r : null;
      } catch (e) { return null; }
    };
    const p = G.HTMLElement?.prototype;
    if (p) {
      const valid = receiver => {
        if (__documentType.call(receiver) !== 1 || receiver.namespaceURI !== "http://www.w3.org/1999/xhtml") throw new TypeError("Illegal invocation");
      };
      for (const [name, dimension] of [["offsetWidth", 0], ["offsetHeight", 1]]) {
        if (!Object.getOwnPropertyDescriptor(p, name)) Object.defineProperty(p, name, {
          configurable: true, enumerable: true, get() {
            valid(this);
            if (!this.isConnected) return 0;
            const value = measure(this);
            return value ? Math.round(value[dimension]) : 0;
          },
        });
      }
    }
  });

  // Chrome-desktop feature markers (ua-version-spoof): each must be PRESENT for a claimed major of
  // 139..148; existence-only stubs (probed via `in`), never invoked. NB: deliberately NOT adding any
  // Gecko/WebKit marker (e.g. GestureEvent) that would re-trip the engine check.
  guard(() => {
    const ns = (path) => { const segs = path.split("."); let o = G;
      for (let i = 0; i < segs.length - 1; i++) { const s = segs[i]; if (o[s] == null) o[s] = (s === "prototype") ? {} : function () {}; o = o[s]; }
      const last = segs[segs.length - 1]; if (!(last in o)) o[last] = function () {}; };
    ["Object.groupBy", "Promise.withResolvers", "Array.fromAsync", "Uint8Array.fromBase64",
     "IDBObjectStore.prototype.getAllRecords",
     "PerformanceResourceTiming.prototype.contentEncoding", "Map.prototype.getOrInsert"].forEach(ns);
    if (!("Temporal" in G)) G.Temporal = {};
    if (typeof Math.sumPrecise !== "function") Math.sumPrecise = function sumPrecise() { return 0; };
  });

  // Pad window's own-property count above the fake-DOM floor (window-prop-count-low, <600). These are
  // genuine Chrome global interface names; define any absent as a stub constructor (also improves the
  // window-surface realism). Not load-bearing — a weak, corroborating tell.
  guard(() => {
    const NAMES = ("HTMLElement HTMLDivElement HTMLSpanElement HTMLBodyElement HTMLHeadElement HTMLHtmlElement " +
      "HTMLParagraphElement HTMLImageElement HTMLButtonElement HTMLSelectElement HTMLOptionElement HTMLTextAreaElement " +
      "HTMLLabelElement HTMLFormElement HTMLTableElement HTMLTableRowElement HTMLTableCellElement HTMLUListElement " +
      "HTMLOListElement HTMLLIElement HTMLHeadingElement HTMLScriptElement HTMLStyleElement HTMLLinkElement HTMLMetaElement " +
      "HTMLIFrameElement HTMLCanvasElement HTMLVideoElement HTMLAudioElement HTMLMediaElement HTMLSourceElement " +
      "HTMLTrackElement HTMLPictureElement HTMLTemplateElement HTMLSlotElement HTMLDetailsElement HTMLDialogElement " +
      "SVGElement SVGSVGElement SVGRectElement SVGCircleElement SVGPathElement SVGGElement SVGTextElement SVGUseElement " +
      "CSSStyleSheet CSSStyleRule CSSMediaRule CSSKeyframesRule CSSKeyframeRule CSSSupportsRule CSSFontFaceRule " +
      "CSSStyleDeclaration StyleSheet MediaQueryList DOMRect DOMRectReadOnly DOMPoint DOMMatrix DOMTokenList NamedNodeMap " +
      "NodeList HTMLCollection Attr CharacterData ProcessingInstruction CDATASection ShadowRoot CustomEvent MouseEvent " +
      "KeyboardEvent PointerEvent TouchEvent WheelEvent FocusEvent InputEvent UIEvent DragEvent ClipboardEvent " +
      "AnimationEvent TransitionEvent ProgressEvent MessageEvent CloseEvent ErrorEvent PopStateEvent HashChangeEvent " +
      "StorageEvent PageTransitionEvent BeforeUnloadEvent GamepadEvent SubmitEvent FormDataEvent " +
      "AbortSignal EventTarget FileReader File FormData ReadableStream WritableStream TransformStream " +
      "TextEncoder TextDecoder TextEncoderStream TextDecoderStream CompressionStream DecompressionStream " +
      "BroadcastChannel MessageChannel MessagePort WebSocket XMLHttpRequestUpload XMLSerializer XPathEvaluator " +
      "XPathResult NodeIterator TreeWalker MutationRecord PerformanceEntry PerformanceMark PerformanceMeasure " +
      "PerformanceNavigationTiming PerformanceResourceTiming PerformanceObserverEntryList PerformancePaintTiming " +
      "IntersectionObserverEntry ResizeObserverEntry ReportingObserver CacheStorage Cache " +
      "ServiceWorker ServiceWorkerContainer ServiceWorkerRegistration Notification PushManager Permissions PermissionStatus " +
      "Geolocation MediaStream MediaStreamTrack RTCPeerConnection RTCDataChannel AudioBuffer AudioNode GainNode OscillatorNode " +
      "AnalyserNode BiquadFilterNode DynamicsCompressorNode Image Audio Option Path2D ImageData ImageBitmap OffscreenCanvas " +
      "IDBDatabase IDBTransaction IDBObjectStore IDBIndex IDBCursor IDBKeyRange IDBRequest IDBFactory " +
      "VisualViewport Screen History Location Navigator BarProp CSSFontFeatureValuesRule " +
      "SVGLineElement SVGPolygonElement SVGPolylineElement SVGEllipseElement SVGImageElement SVGDefsElement " +
      "SVGClipPathElement SVGLinearGradientElement SVGRadialGradientElement SVGStopElement SVGSymbolElement " +
      "SVGMarkerElement SVGPatternElement SVGMaskElement SVGFilterElement SVGTitleElement SVGDescElement " +
      "SVGAnimateElement SVGForeignObjectElement SVGTextPathElement SVGTSpanElement SVGViewElement SVGSwitchElement " +
      "HTMLAreaElement HTMLBaseElement HTMLBRElement HTMLDataElement HTMLDataListElement HTMLDListElement " +
      "HTMLEmbedElement HTMLFieldSetElement HTMLHRElement HTMLLegendElement HTMLMapElement HTMLMenuElement " +
      "HTMLMeterElement HTMLModElement HTMLObjectElement HTMLOptGroupElement HTMLOutputElement HTMLParamElement " +
      "HTMLPreElement HTMLProgressElement HTMLQuoteElement HTMLTableCaptionElement HTMLTableColElement " +
      "HTMLTableSectionElement HTMLTimeElement HTMLTitleElement HTMLUnknownElement HTMLMarqueeElement HTMLFontElement " +
      "CSSConditionRule CSSGroupingRule CSSImportRule CSSNamespaceRule CSSPageRule CSSCounterStyleRule CSSLayerBlockRule " +
      "CSSLayerStatementRule CSSPropertyRule CSSNestedDeclarations CSSPositionTryRule CSSTransition CSSAnimation " +
      "CSSNumericValue CSSUnitValue CSSKeywordValue CSSMathSum CSSTransformValue CSSUnparsedValue CSSVariableReferenceValue " +
      "DOMRectList DOMQuad DOMStringList DOMStringMap DOMException DOMImplementation DOMParser Range StaticRange " +
      "AbstractRange Selection Comment Text CDATASection DocumentType DocumentFragment ShadowRoot Element " +
      "IntersectionObserver ResizeObserver ReportingObserver PerformanceObserver MutationObserver " +
      "Worklet PaintWorkletGlobalScope AudioWorklet AudioWorkletNode Blob FileSystemHandle FileSystemFileHandle " +
      "FileSystemDirectoryHandle FileSystemWritableFileStream StorageManager NavigatorUAData Sanitizer TrustedHTML " +
      "TrustedScript TrustedScriptURL TrustedTypePolicy TrustedTypePolicyFactory Highlight HighlightRegistry " +
      "EyeDropper FragmentDirective NavigateEvent Navigation NavigationHistoryEntry NavigationTransition " +
      "CookieStore CookieChangeEvent PressureObserver ScreenDetails ScreenDetailed WakeLock WakeLockSentinel " +
      "MediaQueryListEvent PictureInPictureEvent PictureInPictureWindow RemotePlayback TextTrack TextTrackCue " +
      "VTTCue TextTrackList TimeRanges MediaError MediaEncryptedEvent SourceBuffer MediaSource " +
      "AudioData VideoFrame EncodedAudioChunk EncodedVideoChunk ImageTrack ImageDecoder GPUDevice GPUAdapter " +
      "GPUBuffer GPUTexture GPUCanvasContext WebGLBuffer WebGLProgram WebGLShader WebGLTexture WebGLFramebuffer " +
      "WebGLRenderbuffer WebGLUniformLocation WebGLActiveInfo WebGLContextEvent WebGLVertexArrayObject " +
      "CanvasGradient CanvasPattern CanvasRenderingContext2D OffscreenCanvasRenderingContext2D Path2D TextMetrics").split(/\s+/);
    for (const n of NAMES) { if (n && typeof G[n] === "undefined") { const c = function () {}; try { Object.defineProperty(c, "name", { value: n, configurable: true }); } catch (e) {} G[n] = c; mark(c, n); } }
  });

  // Extend the window surface to real Chrome's full breadth (measured live: 1235 own props vs our
  // ~527) so `X in window` / `typeof window.X` presence checks pass for the ~740 globals we lacked.
  // Three groups from a Chrome-vs-turbo-surf diff: interface constructors (stub fns), on* event-
  // handler slots (null, like Chrome until assigned), and misc methods/props/bar objects. Weak
  // corroborating surface (BotGuard reads presence), not load-bearing.
  guard(() => {
    const IFACES = ("AbsoluteOrientationSensor Accelerometer AnimationTrigger AudioDecoder AudioEncoder AudioListener " +
      "AudioParamMap AudioPlaybackStats AudioProcessingEvent AudioSinkInfo AuthenticatorAssertionResponse " +
      "AuthenticatorAttestationResponse AuthenticatorResponse BackgroundFetchManager BackgroundFetchRecord " +
      "BackgroundFetchRegistration BarcodeDetector BatteryManager BeforeInstallPromptEvent BlobEvent " +
      "Bluetooth BluetoothCharacteristicProperties BluetoothDevice BluetoothRemoteGATTCharacteristic " +
      "BluetoothRemoteGATTDescriptor BluetoothRemoteGATTServer BluetoothRemoteGATTService BluetoothUUID " +
      "BrowserCaptureMediaStreamTrack CSPViolationReportBody CSSContainerRule CSSFontPaletteValuesRule " +
      "CSSFunctionDeclarations CSSFunctionDescriptors CSSFunctionRule CSSImageValue CSSMarginRule " +
      "CSSMathClamp CSSMathInvert CSSMathMax CSSMathMin CSSMathNegate CSSMathProduct CSSMathValue " +
      "CSSMatrixComponent CSSNumericArray CSSPerspective CSSPositionTryDescriptors CSSPositionValue " +
      "CSSPseudoElement CSSRotate CSSScale CSSScopeRule CSSSkew CSSSkewX CSSSkewY CSSStartingStyleRule " +
      "CSSStyleValue CSSTransformComponent CSSTranslate CSSViewTransitionRule " +
      "CanvasCaptureMediaStreamTrack CaptureController ChannelMergerNode ChannelSplitterNode " +
      "ChapterInformation CharacterBoundsUpdateEvent Clipboard ClipboardChangeEvent ClipboardItem " +
      "CloseWatcher ConstantSourceNode ContentVisibilityAutoStateChangeEvent ConvolverNode " +
      "CookieStoreManager CrashReportContext CreateMonitor Credential CredentialsContainer CropTarget " +
      "DOMError DelegatedInkTrailPresenter " +
      "DeviceMotionEvent DeviceMotionEventAcceleration DeviceMotionEventRotationRate " +
      "DeviceOrientationEvent DevicePosture DigitalCredential DocumentPictureInPicture " +
      "DocumentPictureInPictureEvent EditContext EventSource External FederatedCredential Fence " +
      "FencedFrameConfig FetchLaterResult FileSystemObserver FontData FontFace FontFaceSet " +
      "FontFaceSetLoadEvent GPU GPUAdapterInfo GPUBindGroup GPUBindGroupLayout GPUBufferUsage " +
      "GPUColorWrite GPUCommandBuffer GPUCommandEncoder GPUCompilationInfo GPUCompilationMessage " +
      "GPUComputePassEncoder GPUComputePipeline GPUDeviceLostInfo GPUError GPUExternalTexture " +
      "GPUInternalError GPUMapMode GPUOutOfMemoryError GPUPipelineError GPUPipelineLayout GPUQuerySet " +
      "GPUQueue GPURenderBundle GPURenderBundleEncoder GPURenderPassEncoder GPURenderPipeline GPUSampler " +
      "GPUShaderModule GPUShaderStage GPUSupportedFeatures GPUSupportedLimits GPUTextureUsage " +
      "GPUTextureView GPUUncapturedErrorEvent GPUValidationError Gamepad GamepadButton " +
      "GamepadHapticActuator GeolocationCoordinates GeolocationPosition GeolocationPositionError " +
      "GravitySensor Gyroscope HID HIDConnectionEvent HIDDevice HIDInputReportEvent HTMLCameraElement " +
      "HTMLDirectoryElement HTMLFencedFrameElement HTMLFrameElement HTMLFrameSetElement " +
      "HTMLGeolocationElement HTMLMicrophoneElement HTMLOptionsCollection HTMLSelectedContentElement " +
      "HTMLUserMediaElement IDBCursorWithValue IDBOpenDBRequest IDBRecord IDBVersionChangeEvent " +
      "IIRFilterNode IdentityCredential IdentityCredentialError IdentityProvider IdleDeadline IdleDetector " +
      "ImageBitmapRenderingContext ImageCapture ImageTrackList Ink InputDeviceCapabilities InputDeviceInfo " +
      "IntegrityViolationReportBody InteractionContentfulPaint InterestEvent Keyboard KeyboardLayoutMap " +
      "LanguageDetector LanguageModel LargestContentfulPaint LaunchParams LaunchQueue LayoutShift " +
      "LayoutShiftAttribution LinearAccelerationSensor Lock LockManager MIDIAccess MIDIConnectionEvent " +
      "MIDIInput MIDIInputMap MIDIMessageEvent MIDIOutput MIDIOutputMap MIDIPort MathMLElement " +
      "MediaCapabilities MediaDeviceInfo MediaDevices MediaElementAudioSourceNode MediaKeyMessageEvent " +
      "MediaKeySession MediaKeyStatusMap MediaKeySystemAccess MediaKeys MediaMetadata MediaRecorder " +
      "MediaSession MediaSourceHandle MediaStreamAudioDestinationNode MediaStreamAudioSourceNode " +
      "MediaStreamEvent MediaStreamTrackAudioStats MediaStreamTrackEvent MediaStreamTrackGenerator " +
      "MediaStreamTrackProcessor MediaStreamTrackVideoStats MimeType MimeTypeArray NavigationActivation " +
      "NavigationCurrentEntryChangeEvent NavigationDestination NavigationPrecommitController " +
      "NavigationPreloadManager NavigatorLogin NavigatorManagedData NetworkInformation " +
      "NotRestoredReasonDetails NotRestoredReasons OTPCredential OpaqueRange OrientationSensor " +
      "Origin OverconstrainedError PageRevealEvent PageSwapEvent PannerNode PasswordCredential " +
      "PaymentAddress PaymentManager PaymentMethodChangeEvent PaymentRequest PaymentRequestUpdateEvent " +
      "PaymentResponse PerformanceElementTiming PerformanceEventTiming PerformanceLongAnimationFrameTiming " +
      "PerformanceLongTaskTiming PerformanceNavigation PerformanceScriptTiming PerformanceServerTiming " +
      "PerformanceSoftNavigation PerformanceTiming PerformanceTimingConfidence PeriodicSyncManager " +
      "PeriodicWave Plugin PluginArray Presentation PresentationAvailability PresentationConnection " +
      "PresentationConnectionAvailableEvent PresentationConnectionCloseEvent PresentationConnectionList " +
      "PresentationReceiver PresentationRequest PressureRecord Profiler PromiseRejectionEvent " +
      "ProtectedAudience PublicKeyCredential PushSubscription PushSubscriptionOptions QuotaExceededError " +
      "RTCCertificate RTCDTMFSender RTCDTMFToneChangeEvent RTCDataChannelEvent RTCDtlsTransport " +
      "RTCEncodedAudioFrame RTCEncodedVideoFrame RTCError RTCErrorEvent RTCIceCandidate RTCIceTransport " +
      "RTCPeerConnectionIceErrorEvent RTCPeerConnectionIceEvent RTCRtpReceiver RTCRtpScriptTransform " +
      "RTCRtpSender RTCRtpTransceiver RTCSctpTransport RTCSessionDescription RTCStatsReport RTCTrackEvent " +
      "RelativeOrientationSensor ReportBody ResizeObserverSize RestrictionTarget SVGAElement SVGAngle " +
      "SVGAnimateMotionElement SVGAnimateTransformElement SVGAnimatedAngle SVGAnimatedBoolean " +
      "SVGAnimatedEnumeration SVGAnimatedInteger SVGAnimatedLength SVGAnimatedLengthList SVGAnimatedNumber " +
      "SVGAnimatedNumberList SVGAnimatedPreserveAspectRatio SVGAnimatedRect SVGAnimatedString " +
      "SVGAnimatedTransformList SVGAnimationElement SVGComponentTransferFunctionElement SVGFEBlendElement " +
      "SVGFEColorMatrixElement SVGFEComponentTransferElement SVGFECompositeElement " +
      "SVGFEConvolveMatrixElement SVGFEDiffuseLightingElement SVGFEDisplacementMapElement " +
      "SVGFEDistantLightElement SVGFEDropShadowElement SVGFEFloodElement SVGFEFuncAElement " +
      "SVGFEFuncBElement SVGFEFuncGElement SVGFEFuncRElement SVGFEGaussianBlurElement SVGFEImageElement " +
      "SVGFEMergeElement SVGFEMergeNodeElement SVGFEMorphologyElement SVGFEOffsetElement " +
      "SVGFEPointLightElement SVGFESpecularLightingElement SVGFESpotLightElement SVGFETileElement " +
      "SVGFETurbulenceElement SVGGeometryElement SVGGradientElement SVGGraphicsElement SVGLength " +
      "SVGLengthList SVGMPathElement SVGMatrix SVGMetadataElement SVGNumber SVGNumberList SVGPoint " +
      "SVGPointList SVGPreserveAspectRatio SVGRect SVGScriptElement SVGSetElement SVGStringList " +
      "SVGStyleElement SVGTextContentElement SVGTextPositioningElement SVGTransform SVGTransformList " +
      "SVGUnitTypes Scheduler Scheduling ScreenOrientation ScriptProcessorNode ScrollTimeline Sensor " +
      "SensorErrorEvent Serial SerialPort SnapEvent SourceBufferList SpeechGrammar SpeechGrammarList " +
      "SpeechRecognition SpeechRecognitionErrorEvent SpeechRecognitionEvent SpeechRecognitionPhrase " +
      "SpeechSynthesis SpeechSynthesisErrorEvent SpeechSynthesisEvent SpeechSynthesisUtterance " +
      "SpeechSynthesisVoice StorageBucket StorageBucketManager StylePropertyMap StylePropertyMapReadOnly " +
      "Summarizer SyncManager TaskAttributionTiming TaskController TaskPriorityChangeEvent " +
      "TaskSignal TextEvent TextFormat TextFormatUpdateEvent TextTrackCueList TextUpdateEvent " +
      "TimelineTrigger TimelineTriggerRange TimelineTriggerRangeList Touch TouchList TrackEvent Translator " +
      "URLPattern USB USBAlternateInterface USBConfiguration USBConnectionEvent USBDevice USBEndpoint " +
      "USBInTransferResult USBInterface USBIsochronousInTransferPacket USBIsochronousInTransferResult " +
      "USBIsochronousOutTransferPacket USBIsochronousOutTransferResult USBOutTransferResult " +
      "VideoColorSpace VideoDecoder VideoEncoder VideoPlaybackQuality ViewTimeline " +
      "Viewport VirtualKeyboard VirtualKeyboardGeometryChangeEvent " +
      "VisibilityStateEntry WGSLLanguageFeatures WebGLObject WebGLQuery WebGLSampler " +
      "WebGLShaderPrecisionFormat WebGLSync WebGLTransformFeedback WebKitMutationObserver WebSocketError " +
      "WebSocketStream WebTransport WebTransportBidirectionalStream WebTransportDatagramDuplexStream " +
      "WebTransportError WindowControlsOverlay WindowControlsOverlayGeometryChangeEvent " +
      "XMLHttpRequestEventTarget XRAnchor XRAnchorSet XRBoundedReferenceSpace XRCPUDepthInformation " +
      "XRCamera XRCompositionLayer XRCubeLayer XRCylinderLayer XRDOMOverlayState XRDepthInformation " +
      "XREquirectLayer XRFrame XRHand XRHitTestResult XRHitTestSource XRInputSource XRInputSourceArray " +
      "XRInputSourceEvent XRInputSourcesChangeEvent XRJointPose XRJointSpace XRLayer XRLayerEvent " +
      "XRLightEstimate XRLightProbe XRPlane XRPlaneSet XRPose XRProjectionLayer XRQuadLayer XRRay " +
      "XRReferenceSpace XRReferenceSpaceEvent XRRenderState XRRigidTransform XRSession XRSessionEvent " +
      "XRSpace XRSubImage XRSystem XRTransientInputHitTestResult XRTransientInputHitTestSource XRView " +
      "XRViewerPose XRViewport XRVisibilityMaskChangeEvent XRWebGLBinding XRWebGLDepthInformation " +
      "XRWebGLLayer XRWebGLSubImage XSLTProcessor").split(/\s+/);
    for (const n of IFACES) { if (n && typeof G[n] === "undefined") { const c = function () {}; try { Object.defineProperty(c, "name", { value: n, configurable: true }); } catch (e) {} G[n] = c; mark(c, n); } }
    const ON = ("onabort onafterprint onanimationcancel onanimationend onanimationiteration onanimationstart " +
      "onappinstalled onauxclick onbeforeinput onbeforeinstallprompt onbeforematch onbeforeprint " +
      "onbeforetoggle onbeforexrselect onblur oncancel oncanplay oncanplaythrough onchange onclick onclose " +
      "oncommand oncontentvisibilityautostatechange oncontextlost oncontextmenu oncontextrestored " +
      "oncuechange ondblclick ondevicemotion ondeviceorientation ondeviceorientationabsolute ondrag " +
      "ondragend ondragenter ondragleave ondragover ondragstart ondrop ondurationchange onemptied onended " +
      "onfocus onformdata ongamepadconnected ongamepaddisconnected ongotpointercapture oninput oninvalid " +
      "onkeydown onkeypress onkeyup onlanguagechange onloadeddata onloadedmetadata onloadstart " +
      "onlostpointercapture onmousedown onmouseenter onmouseleave onmousemove onmouseout onmouseover " +
      "onmouseup onmousewheel onpagehide onpagereveal onpageshow onpageswap onpause onplay onplaying " +
      "onpointercancel onpointerdown onpointerenter onpointerleave onpointermove onpointerout onpointerover " +
      "onpointerrawupdate onpointerup onprogress onratechange onrejectionhandled onreset onresize onscroll " +
      "onscrollend onscrollsnapchange onscrollsnapchanging onsearch onsecuritypolicyviolation onseeked " +
      "onseeking onselect onselectionchange onselectstart onslotchange onstalled onstorage onsubmit " +
      "onsuspend ontimeupdate ontoggle ontransitioncancel ontransitionend ontransitionrun ontransitionstart " +
      "onunhandledrejection onvolumechange onwaiting onwebkitanimationend onwebkitanimationiteration " +
      "onwebkitanimationstart onwebkittransitionend onwheel").split(/\s+/);
    for (const n of ON) { if (n && !(n in G)) { try { Object.defineProperty(G, n, { value: null, writable: true, enumerable: true, configurable: true }); } catch (e) {} } }
    const bar = () => ({ visible: true });
    const misc = {
      closed: false, length: 0, name: "", opener: null, status: "", crossOriginIsolated: ops.op_shared_memory_allowed(),
      isSecureContext: ops.op_is_secure_context(), offscreenBuffering: true, credentialless: false,
      origin: (G.location && G.location.origin) || "https://www.google.com", clientInformation: G.navigator,
      locationbar: bar(), menubar: bar(), personalbar: bar(), scrollbars: bar(), statusbar: bar(), toolbar: bar(),
      // These real Web APIs are FEATURE-DETECTED by page code (`if (window.X)`) which then calls
      // their methods — a bare {} is truthy but methodless, so e.g. `navigation.entries()` throws
      // (before, an undefined `navigation` was skipped). Give each its commonly-called methods.
      caches: { open: () => Promise.resolve({ match: () => Promise.resolve(undefined), put: () => Promise.resolve(), keys: () => Promise.resolve([]) }), has: () => Promise.resolve(false), keys: () => Promise.resolve([]), match: () => Promise.resolve(undefined), delete: () => Promise.resolve(false) },
      cookieStore: { get: () => Promise.resolve(null), getAll: () => Promise.resolve([]), set: () => Promise.resolve(), delete: () => Promise.resolve(), addEventListener() {}, removeEventListener() {} },
      navigation: { entries: () => [], currentEntry: null, canGoBack: false, canGoForward: false, navigate() {}, reload() {}, traverseTo() {}, back() {}, forward() {}, updateCurrentEntry() {}, addEventListener() {}, removeEventListener() {} },
      speechSynthesis: { getVoices: () => [], speak() {}, cancel() {}, pause() {}, resume() {}, pending: false, speaking: false, paused: false, addEventListener() {}, removeEventListener() {} },
      viewport: {}, launchQueue: { setConsumer() {} },
      external: {}, fence: null, crashReport: {}, documentPictureInPicture: {}, event: undefined,
      styleMedia: { type: "screen", matchMedium: nativize(() => false, "matchMedium") }, screenLeft: 0, screenTop: 0,
      webkitURL: G.URL, webkitMediaStream: G.MediaStream, webkitRTCPeerConnection: G.RTCPeerConnection,
    };
    for (const k in misc) { if (!(k in G)) { try { G[k] = misc[k]; } catch (e) {} } }
    // The realm's actual agent cluster keying, from the response that created it.
    const originAgentCluster = __documentFunction(function(){
      if(!ops.op_surf_is_window(this))throw new TypeError("Illegal invocation");
      return ops.op_origin_agent_cluster(this);
    }, "get originAgentCluster", 0);
    Object.defineProperty(G,"originAgentCluster",{__proto__:null,get:originAgentCluster,configurable:true,enumerable:true});
    if (!ops.op_native_window_proxy()) {
      const closed = __documentFunction(function(){
        if(!ops.op_surf_is_window(this))throw new TypeError("Illegal invocation");
        return !ops.op_window_active(this);
      }, "get closed", 0);
      Object.defineProperty(G,"closed",{__proto__:null,get:closed,configurable:true,enumerable:true});
    }
    const METHODS = ("alert blur captureEvents close confirm createImageBitmap fetchLater find focus " +
      "getScreenDetails moveBy moveTo open print prompt queryLocalFonts releaseEvents reportError requestResize " +
      "resizeBy resizeTo showDirectoryPicker showOpenFilePicker showSaveFilePicker stop webkitCancelAnimationFrame " +
      "webkitRequestAnimationFrame webkitRequestFileSystem webkitResolveLocalFileSystemURL " +
      "webkitSpeechGrammar webkitSpeechGrammarList webkitSpeechRecognition " +
      "webkitSpeechRecognitionError webkitSpeechRecognitionEvent").split(/\s+/);
    for (const n of METHODS) { if (typeof G[n] !== "function") { try { G[n] = nativize(() => undefined, n); } catch (e) {} } }
  });

  guard(() => {
    for (const name of ["Navigator","Performance","Permissions","Storage","Request","Response","Headers","URL","URLSearchParams","DOMException","MessageEvent","StorageEvent","Event","CustomEvent","SubmitEvent","UIEvent","MouseEvent","KeyboardEvent","InputEvent","FocusEvent","PointerEvent","WheelEvent"]) {
      const ctor=G[name];if(typeof ctor!=="function")continue;
      Object.defineProperty(G,name,{value:ctor,writable:true,configurable:true,enumerable:false});
      const descriptor=Object.getOwnPropertyDescriptor(ctor,"prototype");
      if(descriptor && descriptor.writable)Object.defineProperty(ctor,"prototype",{writable:false});
    }
    Object.setPrototypeOf(G.Performance,G.EventTarget);
    Object.setPrototypeOf(G.DOMException,Function.prototype);
    tag(G.Request.prototype,"Request");tag(G.Response.prototype,"Response");
  });

  if (!canonicalSubmitEvent) {
  const submitEventRecords = __interfaceMap("SubmitEvent");
  G.SubmitEvent = class SubmitEvent extends G.Event {
    constructor(type, init = {}) {
      if (!arguments.length) throw new TypeError("A type is required");
      super(type, init);
      const submitter = (init || {}).submitter || null;
      if (submitter !== null && !(submitter instanceof G.HTMLElement)) throw new TypeError("submitter must be an HTMLElement");
      submitEventRecords.set(this, submitter);
    }
    get submitter() {
      if (!submitEventRecords.has(this)) throw new TypeError("Illegal invocation");
      return submitEventRecords.get(this);
    }
  };
  Object.defineProperty(G.SubmitEvent.prototype, "submitter", {...Object.getOwnPropertyDescriptor(G.SubmitEvent.prototype, "submitter"), enumerable: true});
  Object.defineProperty(G.SubmitEvent.prototype, Symbol.toStringTag, {value: "SubmitEvent", configurable: true});

  }

  // Bind the implemented JS interfaces through genuine native callbacks. The
  // class bridge uses Reflect.construct captured by the embedder, preserving
  // private state, subclass new.target and the original exception behavior.
  if (typeof factory === "function") {
    const replacements = new Map();
    const constructors = ["Date", "DOMParser", "Navigator", "Performance", "Permissions", "PermissionsPolicy", "PermissionStatus", "Storage", "Headers",
      "Request", "Response", "URL", "URLSearchParams", "DOMException", "MessageEvent", "ErrorEvent",
      "Event", "SecurityPolicyViolationEvent", "CustomEvent", "SubmitEvent", "FormDataEvent", "UIEvent", "MouseEvent", "KeyboardEvent", "InputEvent", "FocusEvent", "PointerEvent", "WheelEvent", "MessageChannel", "MessagePort", "BroadcastChannel",
      "Blob", "File", "FormData", "XMLHttpRequest", "AbortController", "AbortSignal",
      "TextEncoder", "TextDecoder", "Screen", "Window", "Crypto", "SubtleCrypto",
      "IntersectionObserver", "ResizeObserver", "MutationObserver", "PerformanceObserver",
      "PerformanceEntry", "PerformanceMark", "PerformanceMeasure", "PerformanceNavigationTiming", "EventCounts", "DOMPointReadOnly", "DOMPoint", "DOMMatrixReadOnly", "DOMMatrix"];
    const bindProperties = (object, skip = []) => {
      if (!object) return;
      for (const key of Reflect.ownKeys(object)) {
        if (skip.includes(key)) continue;
        const descriptor = Object.getOwnPropertyDescriptor(object, key);
        if (!descriptor || !descriptor.configurable) continue;
        if (typeof descriptor.value === "function") descriptor.value = nativize(descriptor.value, typeof key === "string" ? key : descriptor.value.name);
        if (typeof descriptor.get === "function") descriptor.get = nativize(descriptor.get, "get " + String(key), 0);
        if (typeof descriptor.set === "function") descriptor.set = nativize(descriptor.set, "set " + String(key), 1);
        Object.defineProperty(object, key, descriptor);
      }
    };
    for (const name of constructors) {
      const original = G[name];
      if (typeof original !== "function" || !original.prototype) continue;
      bindProperties(original.prototype, ["constructor"]);
      bindProperties(original, ["prototype", "name", "length", "arguments", "caller"]);
      if (orig.call(original).includes("[native code]")) continue;
      const wrapper = factory(original, name, original.length, true, true, name !== "Date");
      Object.defineProperty(wrapper, "prototype", {value: original.prototype, writable: false});
      for (const key of Reflect.ownKeys(original)) {
        if (["prototype", "name", "length", "arguments", "caller"].includes(key)) continue;
        Object.defineProperty(wrapper, key, Object.getOwnPropertyDescriptor(original, key));
      }
      Object.defineProperty(original.prototype, "constructor", {value: wrapper, writable: true, configurable: true});
      Object.defineProperty(G, name, {value: wrapper, writable: true, enumerable: false, configurable: true});
      replacements.set(original, wrapper);
    }
    for (const [original, wrapper] of replacements) {
      const parent = Object.getPrototypeOf(original);
      Object.setPrototypeOf(wrapper, replacements.get(parent) || parent);
    }
    for (const key of Object.getOwnPropertyNames(G)) {
      const descriptor = Object.getOwnPropertyDescriptor(G, key);
      if (!descriptor || !descriptor.configurable || typeof descriptor.value !== "function") continue;
      const fn = descriptor.value;
      if (replacements.has(fn)) descriptor.value = replacements.get(fn);
      else if (!Object.hasOwn(fn, "prototype")) descriptor.value = nativize(fn, key);
      else if (native.has(fn) && orig.call(fn).includes("[native code]") === false && !["Date"].includes(key)) descriptor.value = nativize(fn, key);
      Object.defineProperty(G, key, descriptor);
    }
    for (const name of ["UIEvent", "StorageEvent", "SubmitEvent", "FormDataEvent"]) {
      if (typeof G[name] === "function") Object.setPrototypeOf(G[name], G.Event);
    }
    for (const object of [G.HTMLElement?.prototype, G.HTMLScriptElement?.prototype, G.Navigator?.prototype, G.Performance?.prototype,
      G.Permissions?.prototype, G.HTMLCanvasElement?.prototype, G.CSS, G.console,
      nav?.clipboard, G.trustedTypes, G.crypto, G.crypto?.subtle]) bindProperties(object, ["constructor"]);
  }

})();
"##,
    include_str!("observers.js"),
    include_str!("interface_shapes.js"),
    include_str!("user_activation.js"),
    include_str!("host_input.js"),
    r##"
__hostErrorEvent=globalThis.ErrorEvent;
})"##
);

/// Initialize the V8 platform ONCE, on a dedicated thread that lives for the whole
/// process. Since V8 11.6 every `JsRuntime` must share the thread that first initialized
/// the platform; deno_core does this lazily on whichever thread creates the FIRST runtime
/// (jsruntime.rs: "all runtimes must have a common parent thread that initialized the V8
/// platform"). This engine creates isolates on many threads — the `evaluate` pool,
/// `render`'s per-call thread, the pooled render worker, each live session's thread — and
/// `render` SPAWNS-then-JOINS its thread, so if that transient thread parents the platform
/// and then exits, a runtime later created on another thread faults (SIGBUS on Linux;
/// macOS tolerates it).
///
/// The parent must be both STABLE (outlives every runtime) and NOT the napi addon's Node
/// main thread (which already runs Node's own V8 — initializing deno_core's V8 platform
/// there interferes). So spawn a dedicated "v8-platform" keeper thread that calls
/// `init_platform` and then parks forever; the platform is parented on a thread that
/// never dies and never touches Node's V8. Blocks until init is done so the first runtime
/// can't race ahead of it.
pub fn ensure_platform() {
    use std::sync::mpsc::channel;
    use std::sync::OnceLock;
    static KEEPER: OnceLock<()> = OnceLock::new();
    KEEPER.get_or_init(|| {
        pin_timezone();
        let (ready_tx, ready_rx) = channel::<()>();
        std::thread::Builder::new()
            .name("v8-platform".into())
            .spawn(move || {
                JsRuntime::init_platform(None);
                let _ = ready_tx.send(());
                // Park forever: the platform's parent thread must outlive every runtime.
                loop {
                    std::thread::park();
                }
            })
            .expect("spawn v8 platform keeper");
        let _ = ready_rx.recv(); // platform initialized before any runtime is built
    });
}

// Pin a coherent timezone for the synthetic browser — OPT-IN via `TURBO_SURF_TZ`. An isolate that
// reports the host machine's zone (leaked via `Intl.DateTimeFormat().resolvedOptions().timeZone` +
// `Date.getTimezoneOffset`, incoherent with the en-US identity) is a fingerprint tell, but ICU
// reads the timezone from the `TZ` env, and mutating a process-global env var is a data race with
// other threads + would silently change the HOST process's timezone when this crate is embedded in
// the napi addon / PyO3 wheel. So we set `TZ` ONLY when the operator explicitly opts in with
// `TURBO_SURF_TZ` (e.g. `America/New_York`), once, before the platform initializes; we never touch
// it by default. The per-isolate `date_time_configuration_change_notification(Redetect)` (see
// make_runtime) then re-reads it. Without the opt-in, the isolate uses the host zone.
fn pin_timezone() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        if let Ok(tz) = std::env::var("TURBO_SURF_TZ") {
            if !tz.is_empty() {
                std::env::set_var("TZ", tz);
            }
        }
    });
}

fn make_runtime(base: &str, cookies: &str, ua: &str) -> OwnedRuntime {
    make_runtime_with_hints(base, cookies, ua, ClientHints::default())
}

fn make_runtime_with_hints(
    base: &str,
    cookies: &str,
    ua: &str,
    client_hints: ClientHints,
) -> OwnedRuntime {
    make_runtime_with_storage(base, cookies, ua, client_hints, BrowserStorage::default())
}
fn make_runtime_with_storage(
    base: &str,
    cookies: &str,
    ua: &str,
    client_hints: ClientHints,
    storage: BrowserStorage,
) -> OwnedRuntime {
    make_runtime_with_storage_and_options(
        base,
        cookies,
        ua,
        client_hints,
        storage,
        PageSessionOptions::default(),
    )
}

fn make_runtime_with_storage_and_options(
    base: &str,
    cookies: &str,
    ua: &str,
    client_hints: ClientHints,
    storage: BrowserStorage,
    options: PageSessionOptions,
) -> OwnedRuntime {
    #[cfg(turbo_surf_native_proxy)]
    assert_eq!(v8::V8::get_version(), env!("TURBO_SURF_EXPECTED_NATIVE_V8"), "the exact-engine runtime requires its verified native SDK; use the matching engine build helper");
    pin_timezone();
    // Build the shared cookie jar first so it backs BOTH page `fetch` (op_state) and the
    // ES-module loader (`<script type=module>` import graphs) — same session, one jar.
    let jar: Jar = Rc::new(RefCell::new(if cookies.is_empty() {
        CookieJar::new()
    } else {
        CookieJar::from_storage_state(cookies)
    }));
    let inline_sources = InlineModules::default();
    let module_credentials = ModuleCredentials::default();
    let module_origins = Rc::new(ModuleOrigins::new(
        turbo_surf_core::request_context::origin(base).is_none(),
        turbo_surf_core::request_context::origin(base).is_none(),
    ));
    let csp = CspHandle::new(base, &[], "");
    #[cfg(turbo_surf_native_proxy)]
    let module_stages = ModuleStages::default();
    let mut rt = JsRuntime::new(RuntimeOptions {
        inspector: std::env::var_os("TURBO_SURF_JS_TRACE").is_some(),
        extensions: {
            let extension = turbo_dom::init();
            #[cfg(turbo_surf_native_proxy)]
            let extension = {
                let mut extension = extension;
                extension.global_template_middleware = Some(window_proxy::template);
                extension
            };
            let mut extensions = crate::crypto_backend::extensions();
            extensions.push(crate::crypto_clone::extension());
            extensions.push(crate::message_ports::extension());
            extensions.push(crate::audio_backend::extension());
            extensions.push(crate::geometry_backend::extension());
            extensions.push(extension);
            extensions
        },
        // Exact-engine children own native module maps. Published SDK children
        // fail closed; actual context retirement and sandbox flags gate imports.
        validate_import_attributes_cb: Some(Box::new(|scope, _attributes, _context| {
            let current = scope.get_current_context();
            if current.get_slot::<FrameContextMarker>().is_some()
                && (!cfg!(turbo_surf_native_proxy)
                    || current.get_slot::<FrameRetired>().is_some()
                    || current
                        .get_slot::<FrameSandbox>()
                        .is_some_and(|policy| policy.scripts_blocked))
            {
                let message = v8::String::new(
                    scope,
                    "ES module imports in child frames are not supported by the native runtime",
                )
                .unwrap();
                let error = v8::Exception::type_error(scope, message);
                scope.throw_exception(error);
            }
        })),
        module_loader: Some(Rc::new(NetModuleLoader {
            base: base.to_string(),
            source: base.to_string(),
            top_level_source: base.to_string(),
            origins: module_origins.clone(),
            jar: jar.clone(),
            ua: ua.to_string(),
            client_hints: client_hints.clone(),
            http_cache: storage.http_cache(),
            storage_effects: storage.partition_response_effects(
                csp.partition.as_ref().map_or(0, |partition| partition.id),
            ),
            inline_sources: inline_sources.clone(),
            credentials: module_credentials.clone(),
            csp: csp.clone(),
            #[cfg(turbo_surf_native_proxy)]
            module_stages: module_stages.clone(),
        })),
        ..Default::default()
    });
    // Force ICU to re-detect the timezone from the (now-pinned) TZ env for THIS isolate, so
    // Date/Intl report the coherent zone rather than a cached host default.
    rt.v8_isolate()
        .date_time_configuration_change_notification(v8::TimeZoneDetection::Redetect);
    #[cfg(turbo_surf_native_proxy)]
    rt.v8_isolate()
        .set_failed_access_check_callback(window_proxy::failed_access);
    {
        deno_core::scope!(scope, &mut rt);
        let context = scope.get_current_context();
        context.set_slot(Rc::new(module_credentials));
        context.set_slot(module_origins);
        context.set_slot(Rc::new(csp.clone()));
        let global = context.global(scope);
        let name = v8::String::new(scope, "Deno").unwrap();
        let deno = global
            .get(scope, name.into())
            .and_then(|value| value.to_object(scope))
            .expect("Deno core initialization must provide its private bootstrap object");
        let name = v8::String::new(scope, "core").unwrap();
        let core = deno
            .get(scope, name.into())
            .and_then(|value| value.to_object(scope))
            .expect("Deno core initialization must provide its private op table");
        context.set_slot(Rc::new(RuntimeCore(
            crate::context_ownership::object(scope, core)
                .expect("new runtime owns its private core before guest execution"),
        )));
        for name in ["Deno", "__bootstrap"] {
            let key = v8::String::new(scope, name).unwrap();
            global.delete(scope, key.into());
        }
    }
    let state = rt.op_state();
    let mut state = state.borrow_mut();
    state.put(NetworkLog::default());
    state.put(form_submission::Queue::default());
    state.put(storage);
    state.put(ResponseHeaders::default());
    state.put(crate::DocumentResponseMetadata::default());
    state.put(CspContexts(std::collections::HashMap::from([(
        csp.id, csp,
    )])));
    state.put(inline_sources);
    #[cfg(turbo_surf_native_proxy)]
    state.put(module_stages);
    state.put(ClassicScripts::default());
    state.put(FrameContexts::default());
    state.put(crate::document_facts::AgentClusters::default());
    state.put(TimerClock {
        mode: options.window_timer_mode,
        ..TimerClock::default()
    });
    state.put::<Base>(Base(base.to_string()));
    let root_cookie_target = Rc::downgrade(&jar);
    state.put(ReportCookieTargets(std::collections::HashMap::from([(
        0,
        root_cookie_target,
    )])));
    state.put::<Jar>(jar);
    state.put::<Ua>(Ua(ua.to_string()));
    state.put::<ClientHints>(client_hints);
    drop(state);
    {
        deno_core::scope!(scope, &mut rt);
        crate::storage_gc::install(scope);
    }
    let key = ActivationKey::new(rt.v8_isolate());
    OwnedRuntime(rt, false, key)
}

/// Graft the native DOM binding onto the runtime's context (parsing `html` into the
/// tree), then layer the non-DOM env globals over the ops. After this the page
/// script runs against a real `document`.
fn install_dom(rt: &mut JsRuntime, html: &str, base: &str) -> Result<(), String> {
    {
        let state = rt.op_state();
        let mut state = state.borrow_mut();
        let frames = state.borrow_mut::<FrameContexts>();
        let init_script = std::mem::take(&mut frames.init_script);
        *frames = FrameContexts {
            init_script,
            ..Default::default()
        };
        // A new top-level document starts a new browsing context group.
        state.put(crate::document_facts::AgentClusters::default());
        let queue = state.borrow_mut::<ClassicScripts>();
        queue.html = html.to_string();
        queue.used_inline_offsets.clear();
        queue.jobs.clear();
    }
    let context = rt.main_context();
    {
        let scope = v8::HandleScope::new(rt.v8_isolate());
        let scope = std::pin::pin!(scope);
        let mut scope = scope.init();
        let context = v8::Local::new(&scope, context);
        let mut scope = v8::ContextScope::new(&mut scope, context);
        set_origin_token(&mut scope, context, base);
        if let Some(csp) = context.get_slot::<CspHandle>() {
            let headers = JsRuntime::op_state_from(&scope)
                .borrow()
                .borrow::<ResponseHeaders>()
                .0
                .clone();
            let response_metadata = JsRuntime::op_state_from(&scope)
                .borrow()
                .borrow::<crate::DocumentResponseMetadata>()
                .clone();
            csp.response_metadata.replace(response_metadata);
            csp.dictionaries
                .replace(compression_dictionary::Discovery::new(base, &headers));
            *csp.policy.borrow_mut() = crate::csp::DocumentPolicy::new(base, &headers, html);
            context.remove_slot::<FrameSandbox>();
            if csp.policy.borrow().header.sandbox_restrictions().is_some() {
                let mut flags = FrameSandbox {
                    opaque: !context.get_security_token(&scope).is_string(),
                    ..Default::default()
                };
                flags.apply_csp(&csp.policy.borrow().header);
                if flags.origin_restricted {
                    context.use_default_security_token();
                }
                context.set_slot(Rc::new(flags));
            }
            if let Some(origins) = context.get_slot::<ModuleOrigins>() {
                origins.opaque.set(window_is_opaque(&mut scope, context));
                origins
                    .cookie_site_opaque
                    .set(frame_cookie_site_is_opaque(&mut scope, context));
            }
            let origin = window_origin(&mut scope, context);
            csp.policy.borrow_mut().permissions_policy =
                crate::permissions_policy::PermissionsPolicy::from_headers(&origin, &headers);
            let policy = csp.policy.borrow();
            let allowed = trustworthy_url(base)
                && crate::resource_policy::isolated_opener(&headers)
                && policy.embedder_policy != crate::resource_policy::EmbedderPolicy::UnsafeNone
                && policy
                    .permissions_policy
                    .allows_self("cross-origin-isolated", &origin);
            context.set_slot(Rc::new(SharedMemoryCapability {
                allowed,
                group: csp.id,
                origin,
            }));
        }
        #[cfg(turbo_surf_native_proxy)]
        window_proxy::seed_location(&mut scope, base);
        let source_positions = context.get_slot::<CspHandle>().is_some_and(|handle| {
            let policy = handle.policy.borrow();
            policy.initial_base.has_policy() || policy.report_only.has_policy()
        });
        // Chrome's interface key order is applied once Surf's own interfaces exist too.
        crate::browser_env::defer_key_order(&mut scope);
        if source_positions {
            crate::browser_env::install_with_html_source_positions(&mut scope, html);
        } else {
            crate::browser_env::install_html(&mut scope, html);
        }
        install_css_authorization(&mut scope).map_err(|error| error.to_string())?;
        // Before the environment bootstrap, which already creates the parsed iframes.
        let headers = JsRuntime::op_state_from(&scope)
            .borrow()
            .borrow::<ResponseHeaders>()
            .0
            .clone();
        let facts = DocumentFacts::response(&mut scope, context, base, &headers);
        install_document_facts(&mut scope, facts);
    }
    let origin = ModuleSpecifier::parse(base)
        .map(|url| url.origin().ascii_serialization())
        .unwrap_or_else(|_| "null".to_string());
    let actual_origin = {
        deno_core::scope!(scope, rt);
        let context = scope.get_current_context();
        window_origin(scope, context)
    };
    rt.execute_script("<location>", format!("location.href = {base:?}; location.origin = {origin:?}; globalThis.origin = {actual_origin:?}"))
        .map_err(|e| e.to_string())?;
    {
        deno_core::scope!(scope, rt);
        let context = scope.get_current_context();
        let core = context
            .get_slot::<RuntimeCore>()
            .ok_or("Native crypto core unavailable")?;
        let core = crate::context_ownership::local(scope, &core.0);
        crate::crypto_backend::capture_shared(scope, core)?;
        install_environment(scope, core.into())?;
        crate::crypto_backend::install(scope, core, trustworthy_url(base))?;
        crate::audio_backend::install(scope, core)?;
        crate::geometry_backend::install(scope, core)?;
    }
    #[cfg(turbo_surf_native_proxy)]
    {
        let context = rt.main_context();
        let scope = std::pin::pin!(v8::HandleScope::new(rt.v8_isolate()));
        let mut scope = scope.init();
        let context = v8::Local::new(&scope, context);
        let scope = &mut v8::ContextScope::new(&mut scope, context);
        window_proxy::capture_exception(scope);
        window_proxy::install_location(scope, base);
    }
    {
        deno_core::scope!(scope, rt);
        crate::browser_env::finish_key_order(scope);
    }
    Ok(())
}

fn read_string(rt: &mut JsRuntime, global: v8::Global<v8::Value>) -> Result<String, String> {
    let context = rt.main_context();
    let scope = v8::HandleScope::new(rt.v8_isolate());
    let scope = std::pin::pin!(scope);
    let mut scope = scope.init();
    let context = v8::Local::new(&scope, context);
    let scope = v8::ContextScope::new(&mut scope, context);
    let local = v8::Local::new(&scope, global);
    Ok(local.to_rust_string_lossy(&scope))
}

fn read_json(rt: &mut JsRuntime, global: v8::Global<v8::Value>) -> Result<String, String> {
    let context = rt.main_context();
    deno_core::scope!(scope, rt);
    let context = v8::Local::new(scope, context);
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope,scope);
    let local = v8::Local::new(scope, global);
    // Use the engine serializer rather than a page-replaceable JSON method.
    if let Some(value) = v8::json::stringify(scope, local) {
        let json = value.to_rust_string_lossy(scope);
        if json != "undefined" {
            return Ok(json);
        }
    }
    if !scope.is_execution_terminating() {
        if let Some(error) = scope.exception() {
            return Err(error.to_rust_string_lossy(scope));
        }
    }
    Err("Expression result is not JSON serializable".into())
}

thread_local! {
    /// A document's repeated evaluations retain its runtime and native DOM. Changing
    /// the supplied HTML discards the isolate, including global lexical bindings and
    /// mutated intrinsics that cannot be removed by deleting global-object properties.
    static EVAL_RT: RefCell<Option<(OwnedRuntime, String)>> = const { RefCell::new(None) };
}

fn sync_task_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(|| {
        // deno_core captures this handle for delayed V8 foreground tasks (GC,
        // compilation). It must remain driven between synchronous calls and
        // outlive the thread-local isolate, independently of a caller's runtime.
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("v8-sync-tasks")
            .enable_all()
            .build()
            .expect("create V8 synchronous task runtime")
    })
}

/// Evaluate `script` against `html`'s DOM, returning its result as a string
/// (Playwright `page.evaluate`-ish; synchronous, no event loop). Reuses a
/// thread-persistent isolate AND the installed DOM across calls on the SAME page
/// (see [`EVAL_RT`]): the page is parsed + installed once, then repeated
/// `page.evaluate`s on it just run script (~0.5 ms) instead of re-parsing the
/// document (~5 ms). Changed HTML receives a fresh isolate, DOM, lexical environment
/// and intrinsics. Same-HTML evaluates share the page's globals/DOM, which matches
/// Playwright's page-scoped `evaluate` semantics.
pub fn run_with_dom(html: &str, script: &str) -> Result<String, String> {
    let _task_context = sync_task_runtime().enter();
    EVAL_RT.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.as_ref().is_none_or(|(_, installed)| installed != html) {
            if let Some((previous, _)) = slot.take() {
                drop(previous);
            }
            let mut rt = make_runtime("about:blank", "", "");
            {
                let _activation = rt.activation_key().enter();
                if let Err(error) = install_dom(&mut rt, html, "about:blank") {
                    crate::browser_env::reset_isolate(rt.v8_isolate());
                    crate::browser_env::clear_interface_records(rt.v8_isolate());
                    return Err(error);
                }
            }
            rt.park();
            *slot = Some((rt, html.to_string()));
        }
        let (rt, _) = slot.as_mut().expect("eval runtime present");
        let _activation = rt.activation_key().enter();
        let global = rt
            .execute_script(page_script_name("about:blank"), script.to_string())
            .map_err(|e| e.to_string())?;
        let result = read_string(rt, global);
        crate::storage_gc::boundary(rt.v8_isolate());
        result
    })
}

/// Run page `script` against `html`, drain virtual timers, and return the hydrated
/// document HTML. The Lane B render contract: JS-gated page in, HTML after the
/// page's own scripts ran out. (Sync; no event loop — see [`render_page`].)
pub fn render_html(html: &str, script: &str) -> Result<String, String> {
    let _task_context = sync_task_runtime().enter();
    let mut rt = make_runtime("about:blank", "", "");
    let _activation = rt.activation_key().enter();
    let out = run_sync(&mut rt, html, script);
    crate::storage_gc::boundary(rt.v8_isolate());
    crate::browser_env::reset_isolate(rt.v8_isolate());
    out
}

fn run_sync(rt: &mut JsRuntime, html: &str, script: &str) -> Result<String, String> {
    install_dom(rt, html, "about:blank")?;
    rt.execute_script(page_script_name("about:blank"), script.to_string())
        .map_err(|e| e.to_string())?;
    run_window_timers(rt, 100000)?;
    Ok(crate::browser_env::document_html())
}

struct ClassicScriptFailure {
    interrupted: bool,
    error: Option<v8::Global<v8::Value>>,
    message: String,
    line: usize,
    column: usize,
}

impl ClassicScriptFailure {
    fn host(message: &str) -> Self {
        Self {
            interrupted: false,
            error: None,
            message: message.into(),
            line: 0,
            column: 0,
        }
    }
}

fn execute_frame_script(
    rt: &mut JsRuntime,
    context: v8::Global<v8::Context>,
    source_url: &str,
    code: &[u16],
) -> Result<(), ClassicScriptFailure> {
    let scope = std::pin::pin!(v8::HandleScope::new(rt.v8_isolate()));
    let mut scope = scope.init();
    let context = v8::Local::new(&scope, context);
    if context.get_slot::<FrameRetired>().is_some() {
        return Err(ClassicScriptFailure::host(
            "iframe document has been replaced",
        ));
    }
    let mut scope = v8::ContextScope::new(&mut scope, context);
    let scope = std::pin::pin!(v8::TryCatch::new(&mut scope));
    let scope = &mut scope.init();
    let source = v8::String::new_from_two_byte(scope, code, v8::NewStringType::Normal)
        .ok_or_else(|| ClassicScriptFailure::host("script source allocation failed"))?;
    let resource = v8::String::new(scope, source_url)
        .ok_or_else(|| ClassicScriptFailure::host("script URL allocation failed"))?;
    let origin = v8::ScriptOrigin::new(
        scope,
        resource.into(),
        0,
        0,
        false,
        0,
        None,
        false,
        false,
        false,
        None,
    );
    if let Some(script) = v8::Script::compile(scope, source, Some(&origin)) {
        if script.run(scope).is_some() {
            return Ok(());
        }
    }
    if scope.has_terminated() || scope.is_execution_terminating() {
        let mut failure = ClassicScriptFailure::host("script execution was interrupted");
        failure.interrupted = true;
        return Err(failure);
    }
    let Some(error) = scope.exception() else {
        return Err(ClassicScriptFailure::host(
            "script execution was interrupted",
        ));
    };
    let message = scope.message();
    Err(ClassicScriptFailure {
        interrupted: false,
        error: Some(v8::Global::new(scope, error)),
        message: message
            .map(|message| message.get(scope).to_rust_string_lossy(scope))
            .unwrap_or_else(|| "Uncaught exception".into()),
        line: message
            .and_then(|message| message.get_line_number(scope))
            .unwrap_or(0),
        column: message
            .map(|message| message.get_start_column() + 1)
            .unwrap_or(0),
    })
}

async fn drain_event_loop(rt: &mut JsRuntime) -> Result<(), String> {
    drain_event_loop_until(rt, None).await
}

// Parsing/deferred scripts gate DOMContentLoaded; unrelated image requests do not.
// Returning as soon as their promise settles preserves that distinction while
// leaving other ops alive for the later load/interaction drain.
fn deliver_window_message(rt: &mut JsRuntime, item: WindowMessage) -> Result<(), String> {
    let scope = v8::HandleScope::new(rt.v8_isolate());
    let scope = std::pin::pin!(scope);
    let mut scope = scope.init();
    let target = v8::Local::new(&scope, item.target);
    if target.get_slot::<FrameRetired>().is_some() {
        return Ok(());
    }
    let scope = &mut v8::ContextScope::new(&mut scope, target);
    if !item.target_origin.matches(scope, target) {
        return Ok(());
    }
    let source = v8::Local::new(scope, item.source);
    let source_window = source.global(scope);
    let crate::crypto_clone::MessageClone {
        value: data,
        ports,
        failed,
    } = crate::crypto_clone::deserialize_message(scope, &item.data)
        .map_err(|error| error.to_string())?;
    let port_values = ports
        .into_iter()
        .map(|port| port.into())
        .collect::<Vec<_>>();
    let port_array = v8::Array::new_with_elements(scope, &port_values);
    let origin = v8::String::new(scope, &item.origin).unwrap();
    let failed = v8::Boolean::new(scope, failed);
    let activation = v8::Integer::new(scope, item.activation);
    call_frame_helper(
        scope,
        "deliverWindowMessage",
        &[
            data,
            source_window.into(),
            origin.into(),
            port_array.into(),
            failed.into(),
            activation.into(),
        ],
    )
    .ok_or("message dispatch failed")?;
    Ok(())
}

async fn drain_event_loop_until(rt: &mut JsRuntime, condition: Option<&str>) -> Result<(), String> {
    drain_event_loop_until_completion(rt, condition, None).await
}

fn promise_settled(rt: &mut JsRuntime, value: &v8::Global<v8::Value>) -> bool {
    deno_core::scope!(scope, rt);
    let value = v8::Local::new(scope, value);
    v8::Local::<v8::Promise>::try_from(value)
        .map(|promise| promise.state() != v8::PromiseState::Pending)
        .unwrap_or(true)
}

async fn drain_event_loop_until_completion(
    rt: &mut JsRuntime,
    condition: Option<&str>,
    completion: Option<&v8::Global<v8::Value>>,
) -> Result<(), String> {
    let real_timers =
        rt.op_state().borrow().borrow::<TimerClock>().mode == WindowTimerMode::RealTime;
    let mut frame_timer_budget = 200000u32;
    let mut window_timer_wake: Option<(
        tokio::time::Instant,
        std::pin::Pin<Box<tokio::time::Sleep>>,
    )> = None;
    let storage_waiter = storage_events::waiter(&rt.op_state().borrow());
    // Even a synchronous result ends a script job. Observer and Promise
    // microtasks must run before the operation-completion shortcut can return.
    rt.v8_isolate().perform_microtask_checkpoint();
    if rt.v8_isolate().is_execution_terminating() {
        return Err("Microtask execution was interrupted".into());
    }
    loop {
        // Operation completion is independent of unrelated timers, fetches and scripts.
        // The native Promise stays private, so guest code cannot forge this gate.
        if completion.is_some_and(|promise| promise_settled(rt, promise)) {
            return Ok(());
        }
        #[cfg(turbo_surf_native_proxy)]
        {
            let stage = rt
                .op_state()
                .borrow()
                .borrow::<ModuleStages>()
                .0
                .borrow_mut()
                .pop_front();
            if let Some(stage) = stage {
                let context = stage.context;
                let scope = std::pin::pin!(v8::HandleScope::new(rt.v8_isolate()));
                let mut scope = scope.init();
                let context = v8::Local::new(&scope, context);
                let scope = &mut v8::ContextScope::new(&mut scope, context);
                if context.get_slot::<FrameRetired>().is_some() {
                    continue;
                }
                let specifier = v8::String::new(scope, &stage.specifier).unwrap();
                let ready = v8::Boolean::new(scope, stage.ready);
                let details: v8::Local<v8::Value> =
                    if let Some((error, filename, line, column)) = stage.exception {
                        let details = v8::Object::new(scope);
                        for (name, value) in [
                            ("error", v8::Local::new(scope, error)),
                            (
                                "filename",
                                v8::String::new(scope, &filename).unwrap().into(),
                            ),
                            ("lineno", v8::Integer::new_from_unsigned(scope, line).into()),
                            (
                                "colno",
                                v8::Integer::new_from_unsigned(scope, column).into(),
                            ),
                        ] {
                            let key = v8::String::new(scope, name).unwrap();
                            details.set(scope, key.into(), value);
                        }
                        details.into()
                    } else {
                        v8::null(scope).into()
                    };
                call_frame_helper(
                    scope,
                    "completeModuleGraph",
                    &[specifier.into(), ready.into(), details],
                )
                .ok_or("module completion failed")?;
                continue;
            }
        }
        let message = {
            let state = rt.op_state();
            let mut state = state.borrow_mut();
            state.borrow_mut::<FrameContexts>().messages.pop_front()
        };
        if let Some(message) = message {
            deliver_window_message(rt, message)?;
            continue;
        }

        let job = {
            let state = rt.op_state();
            let mut state = state.borrow_mut();
            state.borrow_mut::<ClassicScripts>().jobs.pop_front()
        };
        if let Some(ClassicScriptJob {
            resolver,
            source,
            code,
            context: script_context,
            waiter_context,
            document,
            muted_errors,
            element,
        }) = job
        {
            let previous_script = {
                deno_core::scope!(scope, rt);
                let context = v8::Local::new(scope, &script_context);
                let scope = &mut v8::ContextScope::new(scope, context);
                let document = v8::Local::new(scope, &document);
                let value = match &element {
                    Some(element) => v8::Local::new(scope, element).into(),
                    None => v8::null(scope).into(),
                };
                resource_lifecycle::set_current_script(scope, document, value)
                    .map(|value| v8::Global::new(scope, value))
            };
            let result = execute_frame_script(rt, script_context.clone(), &source, &code);
            if rt.v8_isolate().is_execution_terminating()
                || result
                    .as_ref()
                    .err()
                    .is_some_and(|failure| failure.interrupted)
            {
                rt.v8_isolate().cancel_terminate_execution();
                {
                    deno_core::scope!(scope, rt);
                    let context = v8::Local::new(scope, &script_context);
                    let scope = &mut v8::ContextScope::new(scope, context);
                    if let Some(previous) = previous_script {
                        let document = v8::Local::new(scope, &document);
                        let previous = v8::Local::new(scope, previous);
                        resource_lifecycle::restore_current_script(scope, document, previous);
                    }
                    let context = v8::Local::new(scope, waiter_context);
                    let scope = &mut v8::ContextScope::new(scope, context);
                    let resolver = v8::Local::new(scope, resolver);
                    let message =
                        v8::String::new(scope, "Classic script execution was interrupted")
                            .ok_or("Classic interruption allocation failed")?;
                    let error = v8::Exception::error(scope, message);
                    resolver.reject(scope, error);
                }
                rt.v8_isolate().terminate_execution();
                return Err("Classic script execution was interrupted".into());
            }
            let scope = std::pin::pin!(v8::HandleScope::new(rt.v8_isolate()));
            let mut scope = scope.init();
            let failure = match result {
                Ok(()) => None,
                Err(failure) => {
                    let context = v8::Local::new(&scope, &script_context);
                    let scope = &mut v8::ContextScope::new(&mut scope, context);
                    let error = match failure.error {
                        Some(error) => v8::Local::new(scope, error),
                        None => {
                            let message = v8::String::new(scope, &failure.message)
                                .ok_or("Classic error allocation failed")?;
                            v8::Exception::error(scope, message)
                        }
                    };
                    let details = serde_json::json!({"message":failure.message,
                        "filename":source,"lineno":failure.line,"colno":failure.column});
                    let json = v8::String::new(scope, &details.to_string())
                        .ok_or("Classic error metadata allocation failed")?;
                    let details =
                        v8::json::parse(scope, json).ok_or("Invalid classic error metadata")?;
                    if muted_errors {
                        let json=v8::String::new(scope,"{\"message\":\"Script error.\",\"filename\":\"\",\"lineno\":0,\"colno\":0}")
                            .ok_or("Muted error metadata allocation failed")?;
                        let details =
                            v8::json::parse(scope, json).ok_or("Invalid muted error metadata")?;
                        let muted = v8::null(scope);
                        call_frame_helper(
                            scope,
                            "reportClassicScriptError",
                            &[muted.into(), details],
                        );
                    } else {
                        call_frame_helper(scope, "reportClassicScriptError", &[error, details]);
                    }
                    Some(v8::Global::new(scope, error))
                }
            };
            if let Some(previous) = previous_script {
                let context = v8::Local::new(&scope, &script_context);
                let scope = &mut v8::ContextScope::new(&mut scope, context);
                let previous = v8::Local::new(scope, previous);
                let document = v8::Local::new(scope, &document);
                resource_lifecycle::restore_current_script(scope, document, previous);
            }
            let context = v8::Local::new(&scope, waiter_context);
            let scope = &mut v8::ContextScope::new(&mut scope, context);
            let resolver = v8::Local::new(scope, resolver);
            match failure {
                Some(error) => {
                    let error = v8::Local::new(scope, error);
                    resolver.reject(scope, error);
                }
                None => {
                    let value = v8::undefined(scope);
                    resolver.resolve(scope, value.into());
                }
            }
            continue;
        }
        let result = std::future::poll_fn(|cx| {
            if let Some(waiter) = &storage_waiter {
                waiter.register(cx);
            }
            let result = rt
                .poll_event_loop(cx, deno_core::PollEventLoopOptions::default())
                .map(|result| result.map_err(|error| error.to_string()));
            if completion.is_some_and(|promise| promise_settled(rt, promise)) {
                return std::task::Poll::Ready(Some(Ok(())));
            }
            #[cfg(turbo_surf_native_proxy)]
            if !rt
                .op_state()
                .borrow()
                .borrow::<ModuleStages>()
                .0
                .borrow()
                .is_empty()
            {
                return std::task::Poll::Ready(None);
            }
            if !rt
                .op_state()
                .borrow()
                .borrow::<FrameContexts>()
                .messages
                .is_empty()
            {
                return std::task::Poll::Ready(None);
            }
            // Polling may resume an async fetch and enqueue its script. Yield to
            // Rust immediately rather than waiting on the script's JS promise.
            if !rt
                .op_state()
                .borrow()
                .borrow::<ClassicScripts>()
                .jobs
                .is_empty()
            {
                return std::task::Poll::Ready(None);
            }

            // Storage tasks have their own source and can arrive while a
            // Promise waits without any Window timer. Dispatch one task before
            // selecting timers, then finish its microtasks before the next task.
            if storage_events::pending(&rt.op_state().borrow()) > 0 {
                let delivered = {
                    deno_core::scope!(scope, rt);
                    storage_events::deliver_next(scope)
                };
                match delivered {
                    Ok(0) => {}
                    Ok(_) => {
                        rt.v8_isolate().perform_microtask_checkpoint();
                        if rt.v8_isolate().is_execution_terminating() {
                            return std::task::Poll::Ready(Some(Err(
                                "Microtask execution was interrupted".into(),
                            )));
                        }
                        return std::task::Poll::Ready(None);
                    }
                    Err(error) => {
                        return std::task::Poll::Ready(Some(Err(error.to_string())));
                    }
                }
            }

            // A completed parser/load gate returns to its host lifecycle turn
            // before selecting another Window task. Pending gates still drive
            // timers below, including resource errors during blocking scripts.
            if let Some(condition) = condition {
                let value = rt
                    .execute_script("<load-gate>", condition.to_string())
                    .map_err(|error| error.to_string());
                match value.and_then(|value| read_string(rt, value)) {
                    Ok(value) if value == "1" => {
                        return std::task::Poll::Ready(Some(Ok(())));
                    }
                    Err(error) => return std::task::Poll::Ready(Some(Err(error))),
                    _ => {}
                }
            }
            // Already-due timer tasks precede background networking tasks; a
            // resource event must not advance the clock to a future timer.
            let mut window_deadline = None;
            if frame_timer_budget > 0 {
                let turn = run_next_window_timer(rt, false);
                window_deadline = turn.deadline;
                frame_timer_budget = frame_timer_budget.saturating_sub(turn.fired);
                if turn.fired > 0 {
                    if let Err(error) = checkpoint_window_timer(rt) {
                        return std::task::Poll::Ready(Some(Err(error)));
                    }
                    return std::task::Poll::Ready(None);
                }
            }
            if run_next_background_resource_task(rt) > 0 {
                rt.v8_isolate().perform_microtask_checkpoint();
                if rt.v8_isolate().is_execution_terminating() {
                    return std::task::Poll::Ready(Some(Err(
                        "Resource task microtasks were interrupted".into(),
                    )));
                }
                return std::task::Poll::Ready(None);
            }
            // Native I/O and official system timers use real monotonic time.
            // Do not leap a Window timer past their earlier wake-up. Keep one
            // precise wake so a stalled native request cannot starve the timer.
            let await_window_timer = real_timers && (completion.is_some() || condition.is_some());
            if frame_timer_budget > 0 && (result.is_pending() || await_window_timer) {
                if let Some(deadline) = window_deadline {
                    if window_timer_wake
                        .as_ref()
                        .is_none_or(|(previous, _)| *previous != deadline)
                    {
                        window_timer_wake =
                            Some((deadline, Box::pin(tokio::time::sleep_until(deadline))));
                    }
                    if std::future::Future::poll(window_timer_wake.as_mut().unwrap().1.as_mut(), cx)
                        .is_ready()
                    {
                        cx.waker().wake_by_ref();
                        return std::task::Poll::Ready(None);
                    }
                } else {
                    window_timer_wake = None;
                }
            } else if frame_timer_budget > 0 && !real_timers {
                window_timer_wake = None;
                let turn = run_next_window_timer(rt, true);
                frame_timer_budget = frame_timer_budget.saturating_sub(turn.fired);
                if turn.fired > 0 {
                    if let Err(error) = checkpoint_window_timer(rt) {
                        return std::task::Poll::Ready(Some(Err(error)));
                    }
                    return std::task::Poll::Ready(None);
                }
            }
            // A Window timer is not a Deno op. Keep an explicit awaited task alive,
            // but leave unrelated future timers queued when an idle pump returns.
            if await_window_timer && window_timer_wake.is_some() {
                return std::task::Poll::Pending;
            }
            // A JS Promise can await another native worker's storage task even
            // when Deno has no pending op. The inbox wakes this host future.
            if completion.is_some()
                && storage_waiter.is_some()
                && matches!(result, std::task::Poll::Ready(Ok(())))
            {
                return std::task::Poll::Pending;
            }
            result.map(Some)
        })
        .await;
        match result {
            None => continue,
            Some(Ok(())) => return Ok(()),
            Some(Err(e)) => {
                let text = e.to_string();
                if text.contains("Uncaught (in promise)") || text.contains("Unhandled") {
                    eprintln!("[render] nonfatal promise rejection: {text}");
                    return Ok(());
                }
                return Err(text);
            }
        }
    }
}

/// Like [`render_html`] but drives deno_core's event loop, so a page script that
/// hydrates asynchronously (`Promise`/`async`-`await`/microtasks, and timer
/// callbacks that themselves await) resolves before serialization. This is the
/// fidelity step real SPA frameworks need.
pub async fn render_html_async(html: &str, script: &str) -> Result<String, String> {
    render_page(html, "about:blank", script).await
}

/// Default render execution budget (eval-guard). A page script that loops forever
/// (sync) or never settles (async) is terminated past this.
pub const DEFAULT_RENDER_BUDGET_MS: u64 = 10_000;

/// Async render with a page base URL — relative `fetch` resolves against it and
/// the `document.cookie` bridge is scoped to it. Drives the event loop so
/// `fetch`-driven and promise-based hydration completes before serialization.
/// Bounded by [`DEFAULT_RENDER_BUDGET_MS`].
pub async fn render_page(html: &str, base: &str, script: &str) -> Result<String, String> {
    render_page_with_budget(html, base, script, DEFAULT_RENDER_BUDGET_MS).await
}

/// `render_page` with an explicit execution budget (ms). The V8 isolate is a true
/// isolate (host heap unreachable from guest); this adds a runaway-execution guard:
/// a watchdog thread terminates the isolate if the script exceeds `budget_ms`.
pub async fn render_page_with_budget(
    html: &str,
    base: &str,
    script: &str,
    budget_ms: u64,
) -> Result<String, String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let mut rt = make_runtime(base, "", "");
    let handle = rt.v8_isolate().thread_safe_handle();
    let done = Arc::new(AtomicBool::new(false));
    let watch = done.clone();
    let exceeded = Arc::new(AtomicBool::new(false));
    let timeout = exceeded.clone();
    let watchdog = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        while !watch.load(Ordering::Relaxed) {
            if start.elapsed() >= std::time::Duration::from_millis(budget_ms) {
                timeout.store(true, Ordering::Relaxed);
                handle.terminate_execution();
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    });

    rt.park();
    let key = rt.activation_key();
    let result = key.during(run_async(&mut rt, html, base, script)).await;
    let _activation = key.enter();
    done.store(true, Ordering::Relaxed);
    let _ = watchdog.join();
    let out = if exceeded.load(Ordering::Relaxed) {
        Err(format!("render budget exceeded ({budget_ms}ms)"))
    } else {
        result.map_err(|e| budget_msg(&e, budget_ms))
    };
    crate::browser_env::reset_isolate(rt.v8_isolate());
    out
}

/// Full async render that ALSO returns the isolate's earned cookies (the shared jar
/// as a storage_state JSON string). Seeds the jar from `cookies` (storage_state or
/// "") and the navigator UA from `ua`, runs the page's own scripts to completion
/// (dynamic `<script>` injection + `op_fetch` + timers), then reads the jar back —
/// the in-isolate anti-bot recon path (did the page's integrity JS set a session
/// cookie, e.g. google's `__Secure-ENID`, with no browser?). Cookie read-back
/// happens even on a budget kill so a partial run's cookies aren't lost.
pub async fn render_capture_cookies(
    html: &str,
    base: &str,
    ua: &str,
    cookies: &str,
    script: &str,
    budget_ms: u64,
) -> Result<(String, String), String> {
    render_capture_cookies_with_request_state(
        html,
        base,
        ua,
        cookies,
        script,
        budget_ms,
        ClientHints::default(),
    )
    .await
}

/// Hydrate authored HTML under its actual response policy, then run an optional
/// host tool script. This preserves nonce/module metadata instead of concatenating
/// fetched page scripts into a second, policy-free program.
#[allow(clippy::too_many_arguments)]
pub async fn render_capture_cookies_with_response_headers(
    html: &str,
    base: &str,
    ua: &str,
    cookies: &str,
    script: &str,
    budget_ms: u64,
    client_hints: ClientHints,
    response_headers: &[(String, String)],
) -> Result<(String, String), String> {
    render_capture_cookies_with_browsing_state(
        html,
        base,
        ua,
        cookies,
        script,
        budget_ms,
        client_hints,
        response_headers,
        BrowserStorage::default(),
    )
    .await
}

/// Hydrate with origin-scoped native storage retained by the owning browser session.
#[allow(clippy::too_many_arguments)]
pub async fn render_capture_cookies_with_browsing_state(
    html: &str,
    base: &str,
    ua: &str,
    cookies: &str,
    script: &str,
    budget_ms: u64,
    client_hints: ClientHints,
    response_headers: &[(String, String)],
    storage: BrowserStorage,
) -> Result<(String, String), String> {
    render_capture_cookies_with_document_response(
        html,
        base,
        ua,
        cookies,
        script,
        budget_ms,
        client_hints,
        response_headers,
        storage,
        crate::DocumentResponseMetadata::default(),
    )
    .await
}

/// Hydrate with the actual navigation response metadata and persistent browser state.
#[allow(clippy::too_many_arguments)]
pub async fn render_capture_cookies_with_document_response(
    html: &str,
    base: &str,
    ua: &str,
    cookies: &str,
    script: &str,
    budget_ms: u64,
    client_hints: ClientHints,
    response_headers: &[(String, String)],
    storage: BrowserStorage,
    response_metadata: crate::DocumentResponseMetadata,
) -> Result<(String, String), String> {
    let mut page = PageSession::open_with_document_response(
        html,
        base,
        cookies,
        ua,
        budget_ms,
        "",
        client_hints,
        response_headers,
        storage,
        response_metadata,
    )
    .await?;
    for program in script.split(SCRIPT_BOUNDARY) {
        if !program.trim().is_empty() {
            page.eval(program).await?;
        }
    }
    let result = (page.serialize(), page.cookies());
    page.close();
    Ok(result)
}

/// Like [`render_capture_cookies`], sharing the browsing session's Accept-CH state.
pub async fn render_capture_cookies_with_request_state(
    html: &str,
    base: &str,
    ua: &str,
    cookies: &str,
    script: &str,
    budget_ms: u64,
    client_hints: ClientHints,
) -> Result<(String, String), String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let mut rt = make_runtime_with_hints(base, cookies, ua, client_hints);
    let handle = rt.v8_isolate().thread_safe_handle();
    let done = Arc::new(AtomicBool::new(false));
    let watch = done.clone();
    let watchdog = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        while !watch.load(Ordering::Relaxed) {
            if start.elapsed() >= std::time::Duration::from_millis(budget_ms) {
                handle.terminate_execution();
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    });

    rt.park();
    let key = rt.activation_key();
    let result = key.during(run_async(&mut rt, html, base, script)).await;
    let _activation = key.enter();
    done.store(true, Ordering::Relaxed);
    let _ = watchdog.join();
    // Read the earned cookies out of the shared jar regardless of run outcome.
    let storage = {
        let state = rt.op_state();
        let st = state.borrow();
        let jar = st.borrow::<Jar>();
        let out = jar.borrow().storage_state();
        out
    };
    let doc = result.map_err(|e| budget_msg(&e, budget_ms));
    crate::browser_env::reset_isolate(rt.v8_isolate());
    Ok((doc?, storage))
}

/// Compatibility entry point for the render worker. Each navigation gets a fresh
/// isolate: deleting global properties cannot reset lexical bindings or mutations
/// to built-in prototypes. The worker thread remains reusable, the page realm does not.
pub async fn render_page_pooled(
    html: &str,
    base: &str,
    script: &str,
    budget_ms: u64,
) -> Result<String, String> {
    render_page_with_budget(html, base, script, budget_ms).await
}

/// The boundary a page-script bundle places between successive `<script>` bodies so
/// the render tier can run each as a SEPARATE top-level program. A browser isolates
/// scripts: an uncaught error in one `<script>` does not abort the rest, while every
/// script's top-level `var`/`function`/`let`/`const` still populate the shared realm
/// scope (successive `execute_script`s reuse the same V8 context). A bundle with no
/// boundary is a single script (arbitrary callers) and runs whole, as before.
pub const SCRIPT_BOUNDARY: &str = "\n/*__ts_script_boundary_9f3a__*/\n";

/// Human-input synthesizer (installer). Evaluating this defines `globalThis.__hi`, a small
/// API that generates a pointer path A→B and dispatches a coherent script-created
/// pointer/mouse/keyboard sequence into the DOM the page's own listeners observe.
/// This models input ordering and scheduled interaction cadence. Two path modes:
/// `"straight"` is a linear A→B at uniform cadence (a baseline / sanity path). `"human"` is a
/// cubic-Bézier arc bowed off the A→B line (curvature) with per-step Gaussian coordinate noise, a
/// slow→fast→slow velocity profile (non-uniform time deltas), and an occasional overshoot+settle
/// near the target.
/// Script-created input events remain untrusted, with origin-relative native-backed
/// timestamps. Writable guest flags cannot mark an event trusted.
///
/// Callers that need to leave no enumerable global should `delete globalThis.__hi` after use;
/// tests read `__hi.path(...)` directly. This is generation + dispatch only — it does NOT defeat
/// GPU/audio/server-side scoring; it addresses the input-entropy + interaction-gate signal.
pub const HUMAN_INPUT_JS: &str = r#"(() => {
  const rand = Math.random;
  // Gaussian noise (Box–Muller) — real cursor coordinates jitter, they don't lie on an ideal curve.
  const gauss = (sd) => { let u = 0, v = 0; while (!u) u = rand(); while (!v) v = rand(); return sd * Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * v); };
  const bez = (p0, p1, p2, p3, t) => { const m = 1 - t; return m*m*m*p0 + 3*m*m*t*p1 + 3*m*t*t*p2 + t*t*t*p3; };
  const ease = (u) => (u < 0.5 ? 2*u*u : 1 - Math.pow(-2*u + 2, 2) / 2); // ease-in-out (velocity)

  // Generate an ordered [{x,y,t}] path from A to B. Sampled the way a real browser samples a
  // pointer GESTURE: NOT one event per pixel/ms, but a coordinate read every ~12ms (± a few ms
  // stddev) along the trajectory — so inter-event Δt clusters near the ~10–15ms sampling rate and
  // SPEED shows up as distance-per-sample (small near the ends, large mid-flight), not as Δt.
  // `t` is an origin-relative ms offset from gesture start. mode: "straight" | "human".
  function path(ax, ay, bx, by, mode) {
    const dist = Math.hypot(bx - ax, by - ay) || 1;
    // Gesture duration scales with distance (a longer throw takes longer), human range.
    const dur = mode === "human" ? 120 + dist * (0.7 + rand() * 0.6) : Math.max(50, dist * 0.5);
    let c1x, c1y, c2x, c2y;
    if (mode === "human") {
      const nx = -(by - ay) / dist, ny = (bx - ax) / dist;          // unit normal to A→B
      const bow = (rand() * 0.5 + 0.15) * dist * (rand() < 0.5 ? -1 : 1); // arc height, either side
      c1x = ax + (bx - ax) / 3 + nx * bow * 0.7; c1y = ay + (by - ay) / 3 + ny * bow * 0.7;
      c2x = ax + 2 * (bx - ax) / 3 + nx * bow;   c2y = ay + 2 * (by - ay) / 3 + ny * bow;
    } else {
      c1x = ax + (bx - ax) / 3; c1y = ay + (by - ay) / 3;
      c2x = ax + 2 * (bx - ax) / 3; c2y = ay + 2 * (by - ay) / 3;
    }
    const pts = [];
    let t = 0;
    while (t < dur) {
      const u = t / dur;
      const e = mode === "human" ? ease(u) : u;                     // ease → varying speed
      let x = bez(ax, c1x, c2x, bx, e), y = bez(ay, c1y, c2y, by, e);
      if (mode === "human" && t > 0) { x += gauss(1.2); y += gauss(1.2); }
      pts.push({ x: Math.round(x), y: Math.round(y), t: Math.round(t * 10) / 10 });
      // Browser pointer sampling interval: ~12ms with a few ms of stddev, floored so it never
      // degenerates to a per-ms firehose.
      t += Math.max(6, 12 + gauss(2.5));
    }
    // Final sample lands on B. Its timestamp must be STRICTLY greater than the last loop
    // sample's: the loop's last point sits just under `dur`, and rounding both to 0.1ms can
    // collide (dur - t_last < 0.05 → equal rounded t → a duplicate/non-increasing timestamp).
    // Clamp to last + 0.1ms so pointer timestamps are always strictly monotonic.
    const __lastT = pts.length ? pts[pts.length - 1].t : 0;
    pts.push({ x: bx, y: by, t: Math.max(Math.round(dur * 10) / 10, __lastT + 0.1) });
    if (mode === "human" && rand() < 0.6) {                         // overshoot + settle
      let tt = dur + Math.max(6, 12 + gauss(2.5));
      pts.push({ x: Math.round(bx + gauss(3)), y: Math.round(by + gauss(3)), t: Math.round(tt * 10) / 10 });
      tt += 16 + rand() * 12; pts.push({ x: bx, y: by, t: Math.round(tt * 10) / 10 });
    }
    return pts;
  }

  // A jittered programmatic delay: base ms plus a random offset in [0, jitter]. Humans don't
  // react instantly (bots fire at t=0) — used as the START delay before an interaction, and as
  // the reaction gap between move-end and click.
  const delay = (base, jitter) => Math.max(0, (base || 0) + rand() * (jitter || 0));

  // Human keyboard timeline: for each character emit keydown → keypress → input → keyup, spaced
  // by a realistic inter-key gap (dwell + flight). Gap ~90–170ms, +extra after space/punctuation,
  // with an occasional "think" pause; each key is held ~40–90ms (keydown→keyup). `base` is the
  // origin-relative ms at which typing starts. Returns { events:[{name,type,props,ts}], end }.
  // Realistic KeyboardEvent init for a character or named key. Real Chrome events carry
  // key/code/keyCode/which (plus location) — a `{key}`-only event is a tell (BotGuard reads
  // keyCode/code). `code` is the physical key ("KeyR"/"Digit1"/"Enter"/"Space"); keyCode/which
  // are the legacy numeric codes (kept equal, as Chrome does).
  function keyInfo(ch) {
    const NAMED = {
      Enter: { code: "Enter", keyCode: 13 }, Tab: { code: "Tab", keyCode: 9 },
      Backspace: { code: "Backspace", keyCode: 8 }, Escape: { code: "Escape", keyCode: 27 },
      " ": { code: "Space", keyCode: 32 },
    };
    if (NAMED[ch]) return { key: ch === " " ? " " : ch, code: NAMED[ch].code, keyCode: NAMED[ch].keyCode, which: NAMED[ch].keyCode };
    let code;
    if (/[a-z]/i.test(ch)) code = "Key" + ch.toUpperCase();
    else if (/[0-9]/.test(ch)) code = "Digit" + ch;
    else code = "";
    const kc = ch.length === 1 ? ch.toUpperCase().charCodeAt(0) : 0;
    return { key: ch, code, keyCode: kc, which: kc };
  }

  function typePlan(text, base) {
    const events = [];
    let t = base || 0;
    const s = String(text);
    for (let i = 0; i < s.length; i++) {
      const ch = s[i];
      let gap = 90 + rand() * 80;                          // base inter-key flight
      if (i > 0 && (s[i - 1] === " " || /[.,!?;:]/.test(s[i - 1]))) gap += 60 + rand() * 90;
      if (rand() < 0.06) gap += 250 + rand() * 450;        // occasional hesitation
      t += gap;
      const hold = 40 + rand() * 50;                       // key dwell time
      const k = keyInfo(ch);
      // Full per-character sequence a browser fires for a printable key: keydown → keypress →
      // beforeinput → input → keyup, each with the real key identifiers.
      events.push({ name: "KeyboardEvent", type: "keydown", props: k, ts: Math.round(t * 10) / 10 });
      events.push({ name: "KeyboardEvent", type: "keypress", props: { key: k.key, code: k.code, keyCode: k.which, which: k.which, charCode: k.which }, ts: Math.round((t + 1) * 10) / 10 });
      events.push({ name: "InputEvent", type: "beforeinput", props: { data: ch, inputType: "insertText" }, ts: Math.round((t + 1.5) * 10) / 10 });
      events.push({ name: "InputEvent", type: "input", props: { data: ch, inputType: "insertText" }, ts: Math.round((t + 2) * 10) / 10 });
      events.push({ name: "KeyboardEvent", type: "keyup", props: k, ts: Math.round((t + hold) * 10) / 10 });
    }
    return { events, end: t };
  }

  // A single named-key press (e.g. "Enter") — keydown → keypress (for keys that produce one) →
  // keyup, with the real key identifiers. Returns { events, end } like typePlan.
  function pressPlan(keyName, base) {
    let t = (base || 0) + 30 + rand() * 40;                 // reaction before the press
    const k = keyInfo(keyName);
    const hold = 40 + rand() * 50;
    const events = [
      { name: "KeyboardEvent", type: "keydown", props: k, ts: Math.round(t * 10) / 10 },
    ];
    // keypress fires for Enter + printable keys (not for Tab/Escape/arrows).
    if (keyName === "Enter" || k.key.length === 1) {
      events.push({ name: "KeyboardEvent", type: "keypress", props: { key: k.key, code: k.code, keyCode: k.which, which: k.which, charCode: keyName === "Enter" ? 13 : k.which }, ts: Math.round((t + 1) * 10) / 10 });
    }
    events.push({ name: "KeyboardEvent", type: "keyup", props: k, ts: Math.round((t + hold) * 10) / 10 });
    return { events, end: t + hold };
  }

  // Script-created input events remain untrusted. Their native-backed timestamp
  // comes from creation time; scheduled callbacks provide the interaction cadence.
  function ev(name, type, props, ts) {
    const C = globalThis[name] || globalThis.Event;
    const e = new C(type, Object.assign({ bubbles: true, cancelable: true }, props || {}));
    return e;
  }
  const fire = (target, e) => { try { (target || document).dispatchEvent(e); } catch (_) {} };

  // Dispatch a pointer/mouse move stream along `pts`, each stamped base + p.t (human timeline).
  function move(target, pts, buttons, base) {
    const b = base || 0;
    for (const p of pts) {
      const props = { clientX: p.x, clientY: p.y, pageX: p.x, pageY: p.y, buttons: buttons || 0 };
      fire(target, ev("PointerEvent", "pointermove", props, b + p.t));
      fire(target, ev("MouseEvent", "mousemove", props, b + p.t));
    }
    return pts.length ? b + pts[pts.length - 1].t : b;
  }

  // Type `text` into `target` on the human keyboard timeline starting at `base`. Returns end ts.
  function type(target, text, base) {
    const plan = typePlan(text, base);
    for (const k of plan.events) fire(target, ev(k.name, k.type, k.props, k.ts));
    return plan.end;
  }

  // Full human interaction (synchronous dispatch, human-timeline timestamps): optional start
  // delay → curved move to (bx,by) → reaction gap → press/focus → type → release/click.
  function moveAndClick(target, ax, ay, bx, by, text, opts) {
    opts = opts || {};
    const base = (performance.now ? performance.now() : 0) + delay(opts.startDelay || 0, opts.startJitter || 0);
    const pts = path(ax, ay, bx, by, "human");
    let t = move(target, pts, 0, base);
    t += delay(40, 120);                                    // reaction before the click
    const at = { clientX: bx, clientY: by, pageX: bx, pageY: by };
    fire(target, ev("PointerEvent", "pointerdown", Object.assign({ buttons: 1 }, at), t));
    fire(target, ev("MouseEvent", "mousedown", Object.assign({ buttons: 1 }, at), t + 1));
    fire(target, ev("FocusEvent", "focus", {}, t + 2));
    if (text) t = type(target, text, t + delay(120, 180));
    fire(target, ev("PointerEvent", "pointerup", at, t + 30));
    fire(target, ev("MouseEvent", "mouseup", at, t + 31));
    fire(target, ev("MouseEvent", "click", at, t + 32));
    return { points: pts, start: base, end: t + 32 };
  }

  // Async variant: schedule the same interaction over the VIRTUAL event loop via setTimeout, so
  // events are genuinely SPACED in (virtual) time — the collector sees them arrive over ~seconds,
  // not all in one drain — while still stamped on the human timeline. Resolves when done.
  function play(target, o) {
    o = o || {};
    return new Promise((resolve) => {
      const start = delay(o.startDelay == null ? 300 : o.startDelay, o.startJitter == null ? 500 : o.startJitter);
      const perfBase = (performance.now ? performance.now() : 0) + start;
      const pts = path(o.startX || 0, o.startY || 0, o.toX || 0, o.toY || 0, "human");
      const clickAt = pts.length ? pts[pts.length - 1].t : 0;
      const at = { clientX: o.toX || 0, clientY: o.toY || 0, pageX: o.toX || 0, pageY: o.toY || 0 };
      const typeStart = clickAt + delay(150, 200);
      const plan = o.text ? typePlan(o.text, typeStart) : { events: [], end: typeStart };
      const sched = (wait, name, type, props, ts) =>
        setTimeout(() => fire(target, ev(name, type, props, perfBase + ts)), Math.max(0, Math.round(start + wait)));
      for (const p of pts) {
        sched(p.t, "PointerEvent", "pointermove", { clientX: p.x, clientY: p.y, pageX: p.x, pageY: p.y }, p.t);
        sched(p.t, "MouseEvent", "mousemove", { clientX: p.x, clientY: p.y, pageX: p.x, pageY: p.y }, p.t);
      }
      sched(clickAt, "PointerEvent", "pointerdown", Object.assign({ buttons: 1 }, at), clickAt);
      sched(clickAt, "MouseEvent", "mousedown", Object.assign({ buttons: 1 }, at), clickAt);
      sched(clickAt, "FocusEvent", "focus", {}, clickAt);
      for (const k of plan.events) sched(k.ts, k.name, k.type, k.props, k.ts);
      sched(plan.end + 30, "MouseEvent", "click", at, plan.end + 30);
      setTimeout(() => resolve({ start: perfBase, end: perfBase + plan.end + 30 }), Math.max(0, Math.round(start + plan.end + 40)));
    });
  }

  // Composable interaction SEQUENCE — the general API. Plays an ordered list of steps over the
  // virtual event loop, threading the cursor position and a running clock so every event is both
  // temporally spaced (setTimeout) and stamped on one continuous human timeline. Steps:
  //   { move:{toX,toY} }  curved human move from the current cursor to the target
  //   { click:true }      pointerdown+mousedown → a real press DWELL (~60–140ms) → up → click
  //   { focus:true }      a focus event
  //   { type:"text" }     human keyboard typing (per-key rhythm) at the current focus
  //   { wait:ms }         an explicit pause
  // e.g. move → click (search box) → focus → type("query") → move → click (button):
  //   __hi.sequence(document, [ {move:{toX:400,toY:60}}, {click:true}, {focus:true},
  //     {type:"weather"}, {move:{toX:520,toY:62}}, {click:true} ], { startDelay:400, startJitter:600 });
  function sequence(target, steps, o) {
    o = o || {};
    return new Promise((resolve) => {
      let cx = o.startX || 0, cy = o.startY || 0, t = 0;
      let focused = null;                                   // the element currently holding focus
      let hovered = null;                                   // the element currently under the cursor
      const startDelay = delay(o.startDelay == null ? 300 : o.startDelay, o.startJitter == null ? 500 : o.startJitter);
      const perfBase = (performance.now ? performance.now() : 0) + startDelay;
      const jobs = [];
      const at = () => ({ clientX: cx, clientY: cy, pageX: cx, pageY: cy });
      // Events fire at the element when one is given (focus/blur/click target it, and bubble to
      // document/window); otherwise at `target` (document) at the cursor.
      const job = (tgt, name, type, props, ts) => jobs.push({ tgt: tgt || target, name, type, props, ts });
      // Blur fires both blur (non-bubbling) and focusout (bubbling), like real Chrome.
      const blurIfFocused = (ts) => {
        if (focused) {
          job(focused, "FocusEvent", "blur", {}, ts);
          job(focused, "FocusEvent", "focusout", { bubbles: true }, ts + 0.1);
          focused = null;
        }
      };
      for (const step of steps || []) {
        if (step.move) {
          const el = step.move.el || null;
          // Leaving the previously-hovered element: mouseout/pointerout bubble; mouseleave/
          // pointerleave don't (fire on the element), at the OLD cursor position.
          if (hovered && hovered !== el) {
            job(target, "MouseEvent", "mouseout", at(), t);
            job(hovered, "MouseEvent", "mouseleave", at(), t);
            job(hovered, "PointerEvent", "pointerout", at(), t);
            job(hovered, "PointerEvent", "pointerleave", at(), t);
          }
          const pts = path(cx, cy, step.move.toX, step.move.toY, "human");
          for (const p of pts) {
            const mp = { clientX: p.x, clientY: p.y, pageX: p.x, pageY: p.y };
            job(target, "PointerEvent", "pointermove", mp, t + p.t);
            job(target, "MouseEvent", "mousemove", mp, t + p.t);
          }
          t += pts.length ? pts[pts.length - 1].t : 0;
          cx = step.move.toX; cy = step.move.toY;
          // Arriving over a target element: mouseover/pointerover bubble; mouseenter/pointerenter
          // don't (fire on the element), at the NEW cursor position.
          if (el && hovered !== el) {
            job(target, "MouseEvent", "mouseover", at(), t);
            job(el, "MouseEvent", "mouseenter", at(), t);
            job(target, "PointerEvent", "pointerover", at(), t);
            job(el, "PointerEvent", "pointerenter", at(), t);
            hovered = el;
          }
        } else if (step.click) {
          t += delay(60, 120);                              // reaction before press
          // A mousedown elsewhere steals focus — a real user clicking the search button blurs
          // the input first. Blur any prior focus at press time (unless clicking that same el).
          if (focused && focused !== step.click) blurIfFocused(t);
          job(target, "PointerEvent", "pointerdown", Object.assign({ buttons: 1 }, at()), t);
          job(step.click === true ? target : step.click, "MouseEvent", "mousedown", Object.assign({ buttons: 1 }, at()), t + 1);
          t += 60 + rand() * 80;                            // real DWELL between down and up
          job(target, "PointerEvent", "pointerup", at(), t);
          job(step.click === true ? target : step.click, "MouseEvent", "mouseup", at(), t + 1);
          job(step.click === true ? target : step.click, "MouseEvent", "click", at(), t + 2);
          t += 2;
        } else if (step.focus) {
          if (focused && focused !== step.focus) blurIfFocused(t);   // focus change blurs the old
          const el = step.focus === true ? target : step.focus;
          job(el, "FocusEvent", "focus", {}, t);                     // non-bubbling
          job(el, "FocusEvent", "focusin", { bubbles: true }, t + 0.1); // bubbling
          focused = el; t += delay(20, 60);
        } else if (step.blur) {
          blurIfFocused(t); t += delay(10, 40);
        } else if (step.type != null) {
          t += delay(120, 180);
          const el = focused || target;
          const plan = typePlan(step.type, t);
          // Update the focused field's value as each character's `input` event fires — a real
          // browser mutates .value on keystroke (the value the `input` handler and a later form
          // submit read). Without this, typing is cosmetic (events only) and a submitted form
          // carries an empty field.
          const chars = String(step.type);
          let acc = "";
          try { if (el && el !== target && el.value != null) acc = String(el.value); } catch (e) {}
          let ci = 0;
          for (const k of plan.events) {
            const j = { tgt: el, name: k.name, type: k.type, props: k.props, ts: k.ts };
            if (k.type === "input") { acc += chars[ci++] || ""; j.setValue = acc; }
            jobs.push(j);
          }
          t = plan.end;
        } else if (step.press != null) {
          // A named-key press (e.g. Enter to submit a search) at the focused field — fires the
          // real keydown/keypress/keyup so the page's key handler runs (google's Enter handler
          // builds the /search URL); the nav-follow then loads it.
          const plan = pressPlan(String(step.press), t);
          for (const k of plan.events) job(focused || target, k.name, k.type, k.props, k.ts);
          t = plan.end;
        } else if (step.wait != null) {
          t += step.wait;
        }
      }
      blurIfFocused(t);                                     // leave nothing focused at the end
      for (const j of jobs) setTimeout(() => {
        // Apply the typed value just before its `input` event, so the handler + a later
        // form submit see the updated field value (real-browser order).
        if (j.setValue != null) { try { j.tgt.value = j.setValue; } catch (e) {} }
        fire(j.tgt, ev(j.name, j.type, j.props, perfBase + j.ts));
      }, Math.max(0, Math.round(startDelay + j.ts)));
      setTimeout(() => resolve({ start: perfBase, end: perfBase + t }), Math.max(0, Math.round(startDelay + t + 20)));
    });
  }

  globalThis.__hi = { path, typePlan, pressPlan, keyInfo, delay, move, type, moveAndClick, play, sequence, ev };
})()"#;

/// Run a page-script bundle the browser way: each boundary-delimited part as its own
/// top-level `execute_script`, so a throwing script is isolated from the others
/// (logged + skipped) instead of aborting every later script. Only a real isolate
/// termination (the render-budget watchdog / cancellation) stops the loop and
/// propagates — a plain JS throw leaves the isolate healthy to run the next part.
// Name inline page scripts after the document URL, the way a real Chrome inline <script>
// appears in Error.stack frames (`at https://site/:12:5`). Our old fixed `<page>` name was a
// tell (BotGuard parses stack frame sources). about:blank / empty keep a plausible name.
fn page_script_name(base: &str) -> String {
    if base.is_empty() || base == "about:blank" {
        "about:blank".to_string()
    } else {
        base.to_string()
    }
}

fn exec_page_scripts(rt: &mut JsRuntime, bundle: &str, base: &str) -> Result<(), String> {
    for part in bundle.split(SCRIPT_BOUNDARY) {
        if part.trim().is_empty() {
            continue;
        }
        if let Err(e) = rt.execute_script(page_script_name(base), part.to_string()) {
            if rt.v8_isolate().is_execution_terminating() {
                return Err(e.to_string()); // budget / termination — stop the page
            }
            // Browser semantics: a later `<script>` still runs after one throws.
            eprintln!("script error: {e}");
        }
    }
    Ok(())
}

// Page-load lifecycle phases, driven by the async render loop so the main document/window fire
// the real sequence a browser does — a collector that gates init on these (google's homepage
// registers on DOMContentLoaded/load) otherwise never runs. `document.readyState` reads private
// receiver-local state; each phase updates it and dispatches the matching trusted
// events. All fully guarded so a lifecycle hiccup can't break a render.
fn run_lifecycle(rt: &mut JsRuntime, phase: &str) -> Result<(), String> {
    let context = rt.main_context();
    let scope = std::pin::pin!(v8::HandleScope::new(rt.v8_isolate()));
    let mut scope = scope.init();
    let context = v8::Local::new(&scope, context);
    let scope = &mut v8::ContextScope::new(&mut scope, context);
    call_frame_helper(scope, phase, &[])
        .map(|_| ())
        .ok_or_else(|| format!("Native lifecycle {phase} failed"))
}

async fn run_async(
    rt: &mut JsRuntime,
    html: &str,
    base: &str,
    script: &str,
) -> Result<String, String> {
    install_dom(rt, html, base)?;
    run_lifecycle(rt, "documentLoading")?; // readyState "loading" while scripts run
    exec_page_scripts(rt, script, base)?;
    run_lifecycle(rt, "dispatchDocumentInteractive")?; // interactive + DOMContentLoaded
    drain_event_loop(rt).await?; // DCL handlers + promises/microtasks + fetch from the page
    run_window_timers(rt, 100000)?;
    drain_event_loop(rt).await?; // promises queued by timer callbacks
    drain_to_quiescence_with_scripts(rt, false).await?;
    run_lifecycle(rt, "dispatchDocumentComplete")?; // complete + window load + pageshow
    drain_event_loop(rt).await?; // load handlers' async work
    Ok(crate::browser_env::document_html())
}

/// Hydrate a page by running ITS OWN scripts the way a browser does — execute each
/// `<script>` (inline + dynamically-injected chunks), fetching + running external
/// `src` and firing `onload` so a webpack-style chunk loader resolves and the app
/// mounts. No bundle concatenation by the caller, no framework runtime from us: the
/// page's own bundle drives itself. Bounded by [`DEFAULT_RENDER_BUDGET_MS`].
pub async fn render_hydrate(html: &str, base: &str) -> Result<String, String> {
    render_hydrate_with_budget(html, base, "", "", DEFAULT_RENDER_BUDGET_MS).await
}

/// [`render_hydrate`] with the page's cookies (a `storageState` JSON string, "" for
/// none) seeded into the jar so session-authenticated hydration works, a custom
/// User-Agent ("" for the default), plus an explicit execution budget (ms) + watchdog.
pub async fn render_hydrate_with_budget(
    html: &str,
    base: &str,
    cookies: &str,
    ua: &str,
    budget_ms: u64,
) -> Result<String, String> {
    let mut rt = make_runtime(base, cookies, ua);
    {
        let _activation = rt.activation_key().enter();
        prepare_hydration(&mut rt, html, base)?;
    }
    let watchdog = Watchdog::start(rt.v8_isolate().thread_safe_handle(), budget_ms);
    rt.park();
    let key = rt.activation_key();
    let result = key
        .during(hydrate_installed_document(&mut rt, base, ""))
        .await;
    let _activation = key.enter();
    drop(watchdog);
    // Best-effort on a budget-exceed: a dev-mode SPA (Next `next dev`) can loop past the
    // budget without ever reaching idle, yet the partial DOM rendered so far is exactly
    // what a probe (readable-React-error diagnosis) needs. Mirror `PageSession.eval`:
    // clear the terminate state so the isolate is usable, then return the reached DOM
    // instead of discarding it. Genuine non-budget errors (install/parse) still propagate.
    let out = best_effort_on_budget(&mut rt, result, budget_ms);
    crate::browser_env::reset_isolate(rt.v8_isolate());
    out
}

// Resolve a hydrate-path result into best-effort HTML: a clean `Ok` passes through;
// a budget-terminate is downgraded to the partial serialized DOM (terminate state
// cleared first so the read can run); any other error propagates relabeled.
fn best_effort_on_budget(
    rt: &mut JsRuntime,
    result: Result<String, String>,
    budget_ms: u64,
) -> Result<String, String> {
    match result {
        Ok(html) => Ok(html),
        Err(e) if e.contains("terminat") || e.contains("execution") => {
            rt.v8_isolate().cancel_terminate_execution();
            Ok(crate::browser_env::document_html())
        }
        // A genuine JS error mid-hydration (a page script that throws, an unhandled
        // rejection) used to discard everything. But by this point the DOM is
        // installed and partially mutated by the scripts that DID run — partial
        // hydration beats none (e.g. a jQuery site whose skin JS reparents the DOM
        // before a later analytics script throws). Return the reached DOM if there
        // is one; only propagate when nothing was rendered (install/parse failure).
        Err(e) => {
            rt.v8_isolate().cancel_terminate_execution();
            let dom = crate::browser_env::document_html();
            if dom.trim().is_empty() {
                Err(budget_msg(&e, budget_ms))
            } else {
                Ok(dom)
            }
        }
    }
}

/// Run `script` over `html`'s DOM, drive the event loop + hydration drain, then
/// return the string value of `globalThis.__RESULT`. Backs the MCP `run_playwright`
/// tool: the caller frames a program that runs a Playwright-style script and stashes
/// a JSON result in `__RESULT`. Bounded by [`DEFAULT_RENDER_BUDGET_MS`].
pub async fn eval_async(html: &str, base: &str, script: &str) -> Result<String, String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let mut rt = make_runtime(base, "", "");
    let handle = rt.v8_isolate().thread_safe_handle();
    let done = Arc::new(AtomicBool::new(false));
    let watch = done.clone();
    let watchdog = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        while !watch.load(Ordering::Relaxed) {
            if start.elapsed() >= std::time::Duration::from_millis(DEFAULT_RENDER_BUDGET_MS) {
                handle.terminate_execution();
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    });
    rt.park();
    let key = rt.activation_key();
    let result = key
        .during(async {
            install_dom(&mut rt, html, base)?;
            rt.execute_script("<script>", script.to_string())
                .map_err(|e| e.to_string())?;
            drain_event_loop(&mut rt).await?;
            run_window_timers(&mut rt, 100000)?;
            drain_event_loop(&mut rt).await?;
            let g = rt
                .execute_script("<result>", "String(globalThis.__RESULT || '')")
                .map_err(|e| e.to_string())?;
            read_string(&mut rt, g)
        })
        .await;
    let _activation = key.enter();
    done.store(true, Ordering::Relaxed);
    let _ = watchdog.join();
    let out = result.map_err(|e| budget_msg(&e, DEFAULT_RENDER_BUDGET_MS));
    crate::browser_env::reset_isolate(rt.v8_isolate());
    out
}

// Trusted setup must finish before the guest budget starts. Otherwise a short
// budget can interrupt bootstrap before its private capabilities and hiding run,
// and best-effort hydration would return a half-initialized live session.
fn prepare_hydration(rt: &mut JsRuntime, html: &str, base: &str) -> Result<(), String> {
    let result = install_dom(rt, html, base).and_then(|()| {
        deno_core::scope!(scope, rt);
        let context = scope.get_current_context();
        if context.get_slot::<HostInputDriver>().is_none() {
            return Err("Trusted host input driver initialization failed".into());
        }
        Ok(())
    });
    if result.is_err() {
        crate::browser_env::reset_isolate(rt.v8_isolate());
        crate::browser_env::clear_interface_records(rt.v8_isolate());
    }
    result
}

async fn hydrate_installed_document(
    rt: &mut JsRuntime,
    base: &str,
    init_script: &str,
) -> Result<String, String> {
    rt.op_state().borrow_mut().put(HydrationProgress::default());
    hydration_phase(rt, HydrationPhase::DocumentLoading);
    run_lifecycle(rt, "documentLoading").map_err(|error| error.to_string())?;
    rt.execute_script(
        "<current-script>",
        "globalThis[Symbol.for('ts')].setCurrentScript(null);",
    )
    .map_err(|e| e.to_string())?;
    hydration_phase(rt, HydrationPhase::InitScript);
    if !init_script.is_empty() {
        rt.execute_script("<init-script>", init_script.to_string())
            .map_err(|e| e.to_string())?;
    }
    hydration_phase(rt, HydrationPhase::ParserAndDeferredScripts);
    rt.execute_script(
        "<initial-hydrate>",
        "globalThis[Symbol.for('ts')].initialHydrated=false;globalThis[Symbol.for('ts')].tcHydrate=globalThis[Symbol.for('ts')].hydrate(300,200000,false).finally(()=>{globalThis[Symbol.for('ts')].initialHydrated=true});",
    )
    .map_err(|error| error.to_string())?;
    drain_event_loop_until(
        rt,
        Some("globalThis[Symbol.for('ts')].initialHydrated ? '1' : '0'"),
    )
    .await?;
    hydration_phase(rt, HydrationPhase::ModuleScripts);
    drain_module_scripts(rt, base).await?;
    hydration_phase(rt, HydrationPhase::DocumentInteractive);
    run_lifecycle(rt, "dispatchDocumentInteractive").map_err(|error| error.to_string())?;
    // HTML load waits for script graph completion, images/styles and frames,
    // independently of unrelated fetches or a module's pending top-level await.
    hydration_phase(rt, HydrationPhase::LoadResources);
    drain_event_loop_until(
        rt,
        Some("globalThis[Symbol.for('ts')].documentLoadReady() ? '1' : '0'"),
    )
    .await?;
    hydration_phase(rt, HydrationPhase::DocumentComplete);
    run_lifecycle(rt, "dispatchDocumentComplete")?;
    // Unified event-loop pump. A single "run scripts+timers, then drain" pass isn't
    // enough for a real SPA: React kicks a fetch, yields, the fetch resolves, React
    // schedules MORE work (a timer via its MessageChannel), which schedules another
    // fetch… So loop — run the hydration pump, drain async ops (microtasks + fetches +
    // injected-script loads), check whether JS still has queued work — until it
    // quiesces. The watchdog bounds wall time; MAX_PUMPS bounds a pathological spin.
    const MAX_PUMPS: usize = 500;
    hydration_phase(rt, HydrationPhase::PostLoadHydration);
    for round in 0..MAX_PUMPS {
        rt.execute_script(
            "<hydrate>",
            "globalThis[Symbol.for('ts')].tcHydrate = globalThis[Symbol.for('ts')].hydrate(300,200000,false);",
        )
        .map_err(|e| e.to_string())?;
        drain_event_loop(rt).await?;
        drain_module_scripts(rt, base).await?;
        drain_event_loop(rt).await?;
        rt.op_state()
            .borrow_mut()
            .borrow_mut::<HydrationProgress>()
            .completed_pumps = round + 1;
        let pending = rt
            .execute_script("<pending>", "globalThis[Symbol.for('ts')].pendingWork()")
            .map_err(|e| e.to_string())?;
        if read_string(rt, pending)? != "1" {
            break;
        }
    }
    run_lifecycle(rt, "dispatchDocumentComplete").map_err(|error| error.to_string())?;
    hydration_phase(rt, HydrationPhase::PostLoadQuiescence);
    drain_to_quiescence(rt).await?;
    hydration_phase(rt, HydrationPhase::Complete);
    Ok(crate::browser_env::document_html())
}

// Compatibility only: evaluate module syntax in classic script elements (claimed via `globalThis[Symbol.for('ts')].takeModuleScript`) through
// deno_core's real module graph: inline modules load from their own code, `src` modules
// load by URL (the `NetModuleLoader` fetches them + their imports over the host net).
// This is the path a Next dev / turbopack build (served as ES modules) needs to hydrate.
async fn drain_module_scripts(rt: &mut JsRuntime, base: &str) -> Result<(), String> {
    for n in 0..1000usize {
        rt.execute_script(
            "<take-mod>",
            "globalThis[Symbol.for('ts')].takeModuleScript();",
        )
        .map_err(|e| e.to_string())?;
        let g = rt
            .execute_script(
                "<take-mod-r>",
                "String(globalThis[Symbol.for('ts')].moduleDescriptor || '')",
            )
            .map_err(|e| e.to_string())?;
        let desc = read_string(rt, g)?;
        if desc.is_empty() {
            break;
        }
        let v: deno_core::serde_json::Value =
            deno_core::serde_json::from_str(&desc).unwrap_or(deno_core::serde_json::Value::Null);
        let src = v.get("src").and_then(|s| s.as_str()).unwrap_or("");
        let code = v
            .get("code")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let spec_str = if src.is_empty() {
            format!("{}#tcmod-{n}", base.split('#').next().unwrap_or(base))
        } else {
            resolve(base, src).unwrap_or_else(|| src.to_string())
        };
        let Ok(spec) = ModuleSpecifier::parse(&spec_str) else {
            continue;
        };
        // Code in hand (inline module, OR a `<script src>` chunk whose fetched body was
        // ESM — `globalThis[Symbol.for('ts')].execScriptEl` already fetched it and queued it) → evaluate from that
        // code with `spec` as its identity so the import graph still resolves relative to
        // the src URL. Only a bare `src` with no body re-fetches through the loader.
        let loaded = if code.is_empty() {
            rt.load_side_es_module(&spec).await
        } else {
            rt.load_side_es_module_from_code(&spec, code).await
        };
        match loaded {
            Ok(id) => {
                // Point document.currentScript at the chunk's element for the duration of
                // evaluation: turbopack chunks self-register via TURBOPACK.push([document
                // .currentScript, …]) and key the chunk by currentScript.src. Without this
                // an ESM chunk registers under a stale path and the entry's chunk-load
                // Promise.all never resolves (→ the app never hydrates, no error).
                rt.execute_script(
                    "<mod-cs>",
                    "globalThis[Symbol.for('ts')].setCurrentScript(globalThis[Symbol.for('ts')].currentModuleEl || null);",
                )
                .map_err(|e| e.to_string())?;
                let ev = rt.mod_evaluate(id).await;
                rt.execute_script(
                    "<mod-cs0>",
                    "globalThis[Symbol.for('ts')].setCurrentScript(null);",
                )
                .map_err(|e| e.to_string())?;
                if let Err(e) = ev {
                    eprintln!("module eval error ({spec_str}): {e}");
                }
            }
            Err(e) => eprintln!("module load error ({spec_str}): {e}"),
        }
    }
    Ok(())
}

// RAII runaway-execution watchdog: terminates the isolate if an op runs past the
// budget, and is cancelled (thread joined) on drop. Replaces the hand-rolled
// done-flag + spawn + join that each one-shot entry point repeats.
struct Watchdog {
    exceeded: std::sync::Arc<std::sync::atomic::AtomicBool>,
    done: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Watchdog {
    fn start(handle: v8::IsolateHandle, budget_ms: u64) -> Self {
        use std::sync::atomic::Ordering;
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let watch = done.clone();
        let exceeded = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let timed_out = exceeded.clone();
        let thread = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            while !watch.load(Ordering::Relaxed) {
                if start.elapsed() >= std::time::Duration::from_millis(budget_ms) {
                    timed_out.store(true, Ordering::Relaxed);
                    // Nested native callbacks can clear V8's termination state.
                    // Keep the deadline enforced until the owner joins this guard.
                    handle.terminate_execution();
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        });
        Watchdog {
            exceeded,
            done,
            thread: Some(thread),
        }
    }
}
impl Drop for Watchdog {
    fn drop(&mut self) {
        self.done.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// The V8 watchdog interrupts synchronous work; an opted-in async deadline also
// wakes a session awaiting a timer or native I/O beyond that same budget.
async fn with_async_execution_budget<T>(
    enforce_deadline: bool,
    budget_ms: u64,
    exceeded: &std::sync::atomic::AtomicBool,
    future: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    if !enforce_deadline {
        return future.await;
    }
    match tokio::time::timeout(std::time::Duration::from_millis(budget_ms), future).await {
        Ok(result) => result,
        Err(_) => {
            exceeded.store(true, std::sync::atomic::Ordering::Relaxed);
            Err(format!("render budget exceeded ({budget_ms}ms)"))
        }
    }
}

// Drive the event loop to quiescence: drain async ops (microtasks + fetches), fire any
// queued virtual timers (React's scheduler posts work through them), and repeat until
// nothing is pending. Used after an interaction event re-enters the running app (the
// handler may setState → schedule a re-render → fetch → schedule more).
async fn drain_to_quiescence(rt: &mut JsRuntime) -> Result<(), String> {
    drain_to_quiescence_with_scripts(rt, true).await
}

async fn drain_to_quiescence_with_scripts(
    rt: &mut JsRuntime,
    execute_document_scripts: bool,
) -> Result<(), String> {
    // Fresh virtual-time window for this interaction so its transitions (e.g. a closing
    // MUI modal's Fade-exit timer) fire + complete even when the clock is already large.
    rt.execute_script(
        "<reset-budget>",
        "globalThis[Symbol.for('ts')].resetTimerBudget && globalThis[Symbol.for('ts')].resetTimerBudget();",
    )
    .map_err(|e| e.to_string())?;
    const MAX_ROUNDS: usize = 500;
    // Stop early once the visible tree has been STABLE for this many rounds even though
    // timers keep firing: a real app's analytics/idle-scheduler never stops posting
    // timers, so "no timers queued" alone never holds — wait for the DOM to settle.
    const STABLE_ROUNDS: usize = 6;
    let mut stable = 0usize;
    let mut last_sig = String::new();
    for _ in 0..MAX_ROUNDS {
        // RUN any newly-injected <script>s, then drain. An interaction can pull a chunk at
        // runtime — Next's `dynamic(() => import('…'))` (lazy modals: AddVacationTimeModal,
        // etc.) appends a <script src> when the component first renders. Without running it
        // the chunk never executes, the import() promise never resolves, and the modal never
        // appears. Native script flags and mutation admission prevent repeat execution.
        if execute_document_scripts {
            rt.execute_script(
                "<hydrate>",
                "globalThis[Symbol.for('ts')].tcHydrate = globalThis[Symbol.for('ts')].hydrate(300,200000,false);",
            )
            .map_err(|e| e.to_string())?;
        }
        drain_event_loop(rt).await?;
        if execute_document_scripts {
            let base = rt.op_state().borrow().borrow::<Base>().0.clone();
            drain_module_scripts(rt, &base).await?;
        }
        let fired = run_window_timers(rt, 2000)?;
        drain_event_loop(rt).await?;
        let pending = rt
            .execute_script("<pending>", "globalThis[Symbol.for('ts')].pendingWork()")
            .map_err(|e| e.to_string())?;
        let sig_v = rt
            .execute_script("<domsig>", "globalThis[Symbol.for('ts')].domSig()")
            .map_err(|e| e.to_string())?;
        let fetches = rt
            .execute_script(
                "<fetches>",
                "globalThis[Symbol.for('ts')].pendingFetchCount()",
            )
            .map_err(|e| e.to_string())?;
        let still = read_string(rt, pending)? == "1";
        let fired_any = fired != 0;
        let awaiting_fetch = read_string(rt, fetches)? != "0";
        if !still && !fired_any {
            break; // genuinely idle
        }
        // A request is outstanding: keep pumping (don't let the stable-DOM early-out
        // fire) so the response's re-render — a modal close, a redirect — lands.
        if awaiting_fetch {
            stable = 0;
            continue;
        }
        let sig = read_string(rt, sig_v)?;
        if sig == last_sig {
            stable += 1;
            if stable >= STABLE_ROUNDS {
                break; // render settled; remaining timers are background churn
            }
        } else {
            stable = 0;
            last_sig = sig;
        }
    }
    Ok(())
}

/// Stable host lifecycle labels; page scripts cannot replace these diagnostics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HydrationPhase {
    #[default]
    DocumentLoading,
    InitScript,
    ParserAndDeferredScripts,
    ModuleScripts,
    DocumentInteractive,
    LoadResources,
    DocumentComplete,
    PostLoadHydration,
    PostLoadQuiescence,
    Complete,
}

/// Last host lifecycle gate entered, retained even when guest execution is interrupted.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct HydrationProgress {
    pub phase: HydrationPhase,
    pub completed_pumps: usize,
}

fn hydration_phase(rt: &mut JsRuntime, phase: HydrationPhase) {
    let state = rt.op_state();
    let mut state = state.borrow_mut();
    if let Some(progress) = state.try_borrow_mut::<HydrationProgress>() {
        progress.phase = phase;
    }
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct HydrationDiagnostics {
    pub budget_exceeded: bool,
    pub progress: HydrationProgress,
    pub open_error: Option<String>,
    pub unsupported_csp: Vec<String>,
}

/// A LIVE page: a persistent [`JsRuntime`] whose hydrated DOM + running JS (React, the
/// app's closures, its delegated event listeners) stay ALIVE across operations. Unlike
/// the one-shot `render_*`/`render_hydrate` paths — which serialize the DOM to a string
/// and `reset()` the binding after each call, destroying the running app — a session
/// keeps the app mounted so interactions dispatch REAL DOM events into it and the
/// re-render is observable. This is the browserless analog of a Playwright page.
///
/// The V8 isolate + the binding's thread-local DOM are NOT `Send`: a session must be
/// created and driven from a single owning thread (the napi layer pins one thread per
/// session). `close()` (or drop) resets the binding while the isolate is still alive.
pub struct PageSession {
    script_inventory: Option<crate::script_inventory::ScriptInventory>,
    navigation_count: u64,
    rt: OwnedRuntime,
    hydration_diagnostics: HydrationDiagnostics,
    budget_ms: u64,
    options: PageSessionOptions,
    closed: bool,
}

impl PageSession {
    /// Build the runtime, install + hydrate the page to quiescence, and KEEP IT ALIVE.
    pub async fn open(
        html: &str,
        base: &str,
        cookies: &str,
        ua: &str,
        budget_ms: u64,
    ) -> Result<Self, String> {
        Self::open_with_init_script(html, base, cookies, ua, budget_ms, "").await
    }

    /// Like [`Self::open`], with a script run after environment installation and before
    /// any page script. Intended for diagnostic instrumentation; no hook runs by default.
    pub async fn open_with_init_script(
        html: &str,
        base: &str,
        cookies: &str,
        ua: &str,
        budget_ms: u64,
        init_script: &str,
    ) -> Result<Self, String> {
        Self::open_with_request_state(
            html,
            base,
            cookies,
            ua,
            budget_ms,
            init_script,
            ClientHints::default(),
        )
        .await
    }

    /// Open with the session's shared HTTP client-hint negotiation state. This must
    /// be supplied before hydration, because initial page scripts can fetch resources.
    #[allow(clippy::too_many_arguments)]
    pub async fn open_with_request_state(
        html: &str,
        base: &str,
        cookies: &str,
        ua: &str,
        budget_ms: u64,
        init_script: &str,
        client_hints: ClientHints,
    ) -> Result<Self, String> {
        Self::open_with_response_headers(
            html,
            base,
            cookies,
            ua,
            budget_ms,
            init_script,
            client_hints,
            &[],
        )
        .await
    }

    /// Hydrate the actual HTML with the native response headers that delivered it.
    #[allow(clippy::too_many_arguments)]
    pub async fn open_with_response_headers(
        html: &str,
        base: &str,
        cookies: &str,
        ua: &str,
        budget_ms: u64,
        init_script: &str,
        client_hints: ClientHints,
        response_headers: &[(String, String)],
    ) -> Result<Self, String> {
        Self::open_with_browsing_state(
            html,
            base,
            cookies,
            ua,
            budget_ms,
            init_script,
            client_hints,
            response_headers,
            BrowserStorage::default(),
        )
        .await
    }

    /// Open with persistent private storage and the response policy for this document.
    #[allow(clippy::too_many_arguments)]
    pub async fn open_with_browsing_state(
        html: &str,
        base: &str,
        cookies: &str,
        ua: &str,
        budget_ms: u64,
        init_script: &str,
        client_hints: ClientHints,
        response_headers: &[(String, String)],
        storage: BrowserStorage,
    ) -> Result<Self, String> {
        Self::open_with_document_response(
            html,
            base,
            cookies,
            ua,
            budget_ms,
            init_script,
            client_hints,
            response_headers,
            storage,
            crate::DocumentResponseMetadata::default(),
        )
        .await
    }

    /// Open with the status and selected referrer of the actual host navigation.
    #[allow(clippy::too_many_arguments)]
    pub async fn open_with_document_response(
        html: &str,
        base: &str,
        cookies: &str,
        ua: &str,
        budget_ms: u64,
        init_script: &str,
        client_hints: ClientHints,
        response_headers: &[(String, String)],
        storage: BrowserStorage,
        response_metadata: crate::DocumentResponseMetadata,
    ) -> Result<Self, String> {
        Self::open_with_document_response_and_activation(
            html,
            base,
            cookies,
            ua,
            budget_ms,
            init_script,
            client_hints,
            response_headers,
            storage,
            response_metadata,
            None,
        )
        .await
    }

    /// Seed same-origin sticky activation before any destination scripts execute.
    #[allow(clippy::too_many_arguments)]
    pub async fn open_with_document_response_and_activation(
        html: &str,
        base: &str,
        cookies: &str,
        ua: &str,
        budget_ms: u64,
        init_script: &str,
        client_hints: ClientHints,
        response_headers: &[(String, String)],
        storage: BrowserStorage,
        response_metadata: crate::DocumentResponseMetadata,
        activation: Option<NavigationActivationSnapshot>,
    ) -> Result<Self, String> {
        Self::open_with_document_response_and_options(
            html,
            base,
            cookies,
            ua,
            budget_ms,
            init_script,
            client_hints,
            response_headers,
            storage,
            response_metadata,
            activation,
            PageSessionOptions::default(),
        )
        .await
    }

    /// Install host scheduling policy before hydration. Existing constructors retain virtual time.
    #[allow(clippy::too_many_arguments)]
    pub async fn open_with_document_response_and_options(
        html: &str,
        base: &str,
        cookies: &str,
        ua: &str,
        budget_ms: u64,
        init_script: &str,
        client_hints: ClientHints,
        response_headers: &[(String, String)],
        storage: BrowserStorage,
        response_metadata: crate::DocumentResponseMetadata,
        activation: Option<NavigationActivationSnapshot>,
        options: PageSessionOptions,
    ) -> Result<Self, String> {
        let mut session = Self::open_with_document_response_unfollowed(
            html,
            base,
            cookies,
            ua,
            budget_ms,
            init_script,
            client_hints,
            response_headers,
            storage,
            response_metadata,
            activation,
            options,
        )
        .await?;
        session.follow_form_navigation().await?;
        Ok(session)
    }

    #[allow(clippy::too_many_arguments)]
    async fn open_with_document_response_unfollowed(
        html: &str,
        base: &str,
        cookies: &str,
        ua: &str,
        budget_ms: u64,
        init_script: &str,
        client_hints: ClientHints,
        response_headers: &[(String, String)],
        storage: BrowserStorage,
        response_metadata: crate::DocumentResponseMetadata,
        activation: Option<NavigationActivationSnapshot>,
        options: PageSessionOptions,
    ) -> Result<Self, String> {
        crate::live_layout_backend::initialize_system_fonts_async().await?;
        let mut rt = make_runtime_with_storage_and_options(
            base,
            cookies,
            ua,
            client_hints,
            storage,
            options,
        );
        user_activation::seed(&mut rt, base, activation);
        let mut script_inventory = std::env::var_os("TURBO_SURF_JS_TRACE")
            .map(|root| {
                crate::script_inventory::ScriptInventory::start(
                    &mut rt,
                    std::path::Path::new(&root),
                )
            })
            .transpose()?;
        rt.op_state()
            .borrow_mut()
            .borrow_mut::<FrameContexts>()
            .init_script = init_script.to_owned();
        rt.op_state().borrow_mut().put(response_metadata);
        rt.op_state().borrow_mut().borrow_mut::<ResponseHeaders>().0 = response_headers.to_vec();
        {
            let _activation = rt.activation_key().enter();
            prepare_hydration(&mut rt, html, base)?;
        }
        let enforce_initial_deadline = options.window_timer_mode == WindowTimerMode::RealTime
            || options.initial_hydration_budget_ms.is_some();
        let initial_budget_ms = options
            .initial_hydration_budget_ms
            .map_or(budget_ms, |limit| limit.min(budget_ms));
        let options = PageSessionOptions {
            initial_hydration_budget_ms: None,
            ..options
        };
        let watchdog = Watchdog::start(rt.v8_isolate().thread_safe_handle(), initial_budget_ms);
        let exceeded = watchdog.exceeded.clone();
        rt.park();
        let key = rt.activation_key();
        let result = with_async_execution_budget(
            enforce_initial_deadline,
            initial_budget_ms,
            &exceeded,
            key.during(hydrate_installed_document(&mut rt, base, init_script)),
        )
        .await;
        drop(watchdog);
        if let Some(inventory) = &mut script_inventory {
            let _activation = key.enter();
            inventory.stop_profile();
            inventory.flush();
        }
        let budget_exceeded = exceeded.load(std::sync::atomic::Ordering::Relaxed);
        let progress = rt.op_state().borrow().borrow::<HydrationProgress>().clone();
        let unsupported_csp = {
            let _activation = rt.activation_key().enter();
            let context = rt.main_context();
            let scope = std::pin::pin!(v8::HandleScope::new(rt.v8_isolate()));
            let scope = &mut scope.init();
            let context = v8::Local::new(scope, context);
            context
                .get_slot::<CspHandle>()
                .map_or_else(Vec::new, |handle| handle.policy.borrow().unsupported())
        };
        match result {
            Ok(_) => Ok(PageSession {
                script_inventory,
                navigation_count: 0,
                rt,
                hydration_diagnostics: HydrationDiagnostics {
                    budget_exceeded,
                    progress,
                    open_error: budget_exceeded.then(|| {
                        format!("Hydration exceeded its {initial_budget_ms}ms execution budget")
                    }),
                    unsupported_csp,
                },
                budget_ms,
                options,
                closed: false,
            }),
            // Best-effort on a budget-exceed: a dev-mode SPA whose hydration never reaches
            // idle still produced a partial DOM and a live app. Clear the terminate state
            // (so later `eval`s run) and KEEP the session alive — discarding it would lose
            // the partial render and force the caller back to the static HTML. Genuine
            // non-budget errors (install/parse) still fail the open.
            Err(e) if budget_exceeded || rt.v8_isolate().is_execution_terminating() => {
                rt.v8_isolate().cancel_terminate_execution();
                Ok(PageSession {
                    script_inventory,
                    navigation_count: 0,
                    rt,
                    hydration_diagnostics: HydrationDiagnostics {
                        budget_exceeded,
                        progress,
                        open_error: Some(if budget_exceeded {
                            format!("render budget exceeded ({initial_budget_ms}ms): {e}")
                        } else {
                            e
                        }),
                        unsupported_csp,
                    },
                    budget_ms,
                    options,
                    closed: false,
                })
            }
            Err(e) => {
                let _activation = rt.activation_key().enter();
                crate::browser_env::reset_isolate(rt.v8_isolate());
                Err(e)
            }
        }
    }

    /// Run `script` in the LIVE isolate, then drain the event loop to quiescence so any
    /// work the script triggered (event handlers, re-render, fetch) completes. Returns
    /// `String(globalThis.__RESULT || '')` — scripts that need to return a value stash
    /// it there.
    pub async fn eval(&mut self, script: &str) -> Result<String, String> {
        let key = self.rt.activation_key();
        let result = key.during(self.eval_active(script)).await;
        self.follow_form_navigation().await?;
        result
    }

    async fn eval_active(&mut self, script: &str) -> Result<String, String> {
        const READ: &str = "String(globalThis.__RESULT == null ? '' : globalThis.__RESULT)";
        let budget = self.budget_ms;
        let drained = {
            let watchdog = Watchdog::start(self.rt.v8_isolate().thread_safe_handle(), budget);
            with_async_execution_budget(
                self.options.window_timer_mode == WindowTimerMode::RealTime,
                budget,
                &watchdog.exceeded,
                async {
                    self.rt
                        .execute_script("<session-eval>", script.to_string())
                        .map_err(|e| e.to_string())?;
                    drain_to_quiescence(&mut self.rt).await
                },
            )
            .await
        };
        // The watchdog may have terminated mid-drain. Clear that terminate state so the
        // isolate is usable again, then read the result BEST-EFFORT: an interaction's
        // important effects (the login POST, a client navigation) land early in the
        // drain — the budget is normally hit later on background churn (analytics
        // polling, React's idle scheduler). Returning the reached state beats throwing.
        self.rt.v8_isolate().cancel_terminate_execution();
        match drained {
            Err(e) if !(e.contains("terminat") || e.contains("execution")) => Err(e),
            _ => self
                .rt
                .execute_script("<result>", READ)
                .map_err(|e| budget_msg(&e.to_string(), budget))
                .and_then(|g| read_string(&mut self.rt, g)),
        }
    }

    /// Evaluate an expression and await its own result without a page-global result slot.
    /// Background page work does not postpone a completed collector Promise.
    pub async fn eval_expression(&mut self, expression: &str) -> Result<String, String> {
        self.eval_expression_result(expression, false).await
    }

    /// Await a collector and serialize its actual value with V8's intrinsic JSON.
    pub async fn eval_json_expression(&mut self, expression: &str) -> Result<String, String> {
        self.eval_expression_result(expression, true).await
    }

    async fn eval_expression_result(
        &mut self,
        expression: &str,
        json: bool,
    ) -> Result<String, String> {
        let key = self.rt.activation_key();
        let result = key
            .during(self.eval_expression_active(expression, json))
            .await;
        self.follow_form_navigation().await?;
        result
    }

    async fn eval_expression_active(
        &mut self,
        expression: &str,
        json: bool,
    ) -> Result<String, String> {
        if self.closed {
            return Err("Page session is closed".into());
        }
        let watchdog = Watchdog::start(self.rt.v8_isolate().thread_safe_handle(), self.budget_ms);
        let result =
            tokio::time::timeout(std::time::Duration::from_millis(self.budget_ms), async {
                let value = self
                    .rt
                    .execute_script("<session-expression>", expression.to_owned())
                    .map_err(|error| error.to_string())?;
                let completion = self.rt.resolve(value.clone());
                self.rt
                    .execute_script(
                        "<expression-budget>",
                        "globalThis[Symbol.for('ts')].resetTimerBudget();",
                    )
                    .map_err(|error| error.to_string())?;
                drain_event_loop_until_completion(&mut self.rt, None, Some(&value)).await?;
                let value = self
                    .rt
                    .with_event_loop_promise(completion, Default::default())
                    .await
                    .map_err(|error| error.to_string())?;
                if json {
                    read_json(&mut self.rt, value)
                } else {
                    read_string(&mut self.rt, value)
                }
            })
            .await;
        let exceeded = watchdog.exceeded.load(std::sync::atomic::Ordering::Relaxed);
        drop(watchdog);
        self.rt.v8_isolate().cancel_terminate_execution();
        if exceeded {
            return Err("Expression exceeded the session execution budget".into());
        }
        result.map_err(|_| "Expression exceeded the session execution budget".to_owned())?
    }

    /// Deliver embedder input through a private native dispatch capability.
    /// Guest dispatchEvent still creates untrusted input; this entry point is
    /// available only to the Rust session owner. Supports the harness step schema.
    pub async fn input(&mut self, steps: &serde_json::Value) -> Result<(), String> {
        let key = self.rt.activation_key();
        let result = key.during(self.input_active(steps)).await;
        self.follow_form_navigation().await?;
        result
    }

    async fn input_active(&mut self, steps: &serde_json::Value) -> Result<(), String> {
        if self.closed {
            return Err("Page session is closed".into());
        }
        let _watchdog = Watchdog::start(self.rt.v8_isolate().thread_safe_handle(), self.budget_ms);
        let result =
            tokio::time::timeout(std::time::Duration::from_millis(self.budget_ms), async {
                let promise = {
                    deno_core::scope!(scope, self.rt);
                    let context = scope.get_current_context();
                    let driver = context
                        .get_slot::<HostInputDriver>()
                        .ok_or("Host input driver unavailable")?;
                    let callback = crate::context_ownership::local(scope, &driver.input);
                    let arguments = deno_core::serde_v8::to_v8(scope, steps)
                        .map_err(|error| error.to_string())?;
                    let receiver = v8::undefined(scope).into();
                    let result = callback
                        .call(scope, receiver, &[arguments])
                        .ok_or("Host input dispatch failed")?;
                    v8::Global::new(scope, result)
                };
                if _watchdog
                    .exceeded
                    .load(std::sync::atomic::Ordering::Relaxed)
                {
                    return Err("Host input exceeded the session execution budget".into());
                }
                let completion = self.rt.resolve(promise.clone());
                self.rt
                    .execute_script(
                        "<input-budget>",
                        "globalThis[Symbol.for('ts')].resetTimerBudget();",
                    )
                    .map_err(|error| error.to_string())?;
                drain_event_loop_until_completion(&mut self.rt, None, Some(&promise)).await?;
                self.rt
                    .with_event_loop_promise(completion, Default::default())
                    .await
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            })
            .await;
        let exceeded = _watchdog
            .exceeded
            .load(std::sync::atomic::Ordering::Relaxed);
        drop(_watchdog);
        self.rt.v8_isolate().cancel_terminate_execution();
        if exceeded || !matches!(&result, Ok(Ok(()))) {
            deno_core::scope!(scope, self.rt);
            let context = scope.get_current_context();
            if let Some(driver) = context.get_slot::<HostInputDriver>() {
                let callback = crate::context_ownership::local(scope, &driver.cancel);
                let receiver = v8::undefined(scope).into();
                callback.call(scope, receiver, &[]);
            }
        }
        if exceeded {
            return Err("Host input exceeded the session execution budget".into());
        }
        result.map_err(|_| "Host input exceeded the session execution budget".to_string())?
    }

    /// Host-only retention diagnostics; never installed on the guest global.
    #[doc(hidden)]
    pub fn native_heap_snapshot_for_testing(&mut self) -> Vec<u8> {
        let _activation = self.rt.activation_key().enter();
        let mut snapshot = Vec::new();
        self.rt.v8_isolate().take_heap_snapshot(|chunk| {
            snapshot.extend_from_slice(chunk);
            true
        });
        snapshot
    }

    /// Host-only queued-task diagnostic for native lifecycle contracts.
    /// Wait for native storage tasks without retaining a VM handle in the signal.
    pub fn storage_event_signal(&self) -> crate::StorageEventSignal {
        storage_events::signal(&self.rt.op_state().borrow())
    }

    #[doc(hidden)]
    pub fn pending_storage_events_for_testing(&self) -> usize {
        storage_events::pending(&self.rt.op_state().borrow())
    }

    /// Host-only deterministic collection for native lifecycle contract tests.
    #[doc(hidden)]
    pub fn collect_message_port_garbage_for_testing(&mut self) {
        let _activation = self.rt.activation_key().enter();
        {
            deno_core::scope!(scope, &mut self.rt);
            crate::message_ports::sweep(scope);
        }
        self.rt.v8_isolate().clear_kept_objects();
        self.rt.v8_isolate().low_memory_notification();
        {
            deno_core::scope!(scope, &mut self.rt);
            crate::message_ports::sweep(scope);
            crate::web_streams_backend::sweep(scope);
        }
        crate::browser_env::sweep_collected_documents();
    }

    #[doc(hidden)]
    pub fn native_storage_diagnostics_for_testing(&mut self) -> serde_json::Value {
        let _activation = self.rt.activation_key().enter();
        crate::storage_gc::diagnostics(self.rt.v8_isolate())
    }

    /// Host-only context registry accounting; never installed on guest globals.
    #[doc(hidden)]
    pub fn context_registry_diagnostics_for_testing(&mut self) -> serde_json::Value {
        let _activation = self.rt.activation_key().enter();
        crate::context_ownership::diagnostics(self.rt.v8_isolate())
    }

    /// Inject one native replacement-bootstrap failure before guest publication.
    #[doc(hidden)]
    pub fn fail_next_frame_initialization_for_testing(&mut self) {
        let _activation = self.rt.activation_key().enter();
        deno_core::scope!(scope, self.rt);
        scope
            .get_current_context()
            .set_slot(Rc::new(FailFrameInitializationForTesting));
    }

    /// Consume the native test fault receipt without publishing guest state.
    #[doc(hidden)]
    pub fn take_frame_initialization_failure_for_testing(&mut self) -> bool {
        let _activation = self.rt.activation_key().enter();
        deno_core::scope!(scope, self.rt);
        scope
            .get_current_context()
            .remove_slot::<FailedFrameInitializationForTesting>()
            .is_some()
    }

    #[doc(hidden)]
    pub fn frame_lifecycle_diagnostics_for_testing(&mut self) -> serde_json::Value {
        let _activation = self.rt.activation_key().enter();
        deno_core::scope!(scope, self.rt);
        let state = JsRuntime::op_state_from(scope);
        let state = state.borrow();
        let frames = state.borrow::<FrameContexts>();
        let retired = frames
            .contexts
            .iter()
            .filter(|context| {
                v8::Local::new(scope, *context)
                    .get_slot::<FrameRetired>()
                    .is_some()
            })
            .count();
        serde_json::json!({"hostContexts":frames.contexts.len(),"retiredHostContexts":retired,"queuedWindowMessages":frames.messages.len(),"queuedWindowBytes":frames.messages.iter().map(|message|message.data.size_bytes()).sum::<usize>()})
    }

    #[doc(hidden)]
    pub fn set_window_message_limits_for_testing(&mut self, max_messages: usize, max_bytes: usize) {
        self.rt
            .op_state()
            .borrow_mut()
            .borrow_mut::<FrameContexts>()
            .message_limits = WindowMessageLimits {
            max_messages,
            max_bytes,
        };
    }
    #[doc(hidden)]
    pub fn set_message_port_limits_for_testing(&mut self, max_messages: usize, max_bytes: usize) {
        let _activation = self.rt.activation_key().enter();
        deno_core::scope!(scope, self.rt);
        crate::message_ports::set_limits(scope, max_messages, max_bytes);
    }
    #[doc(hidden)]
    pub fn message_port_diagnostics_for_testing(&mut self) -> serde_json::Value {
        let _activation = self.rt.activation_key().enter();
        deno_core::scope!(scope, self.rt);
        crate::message_ports::diagnostics(scope)
    }

    /// Host-only response records; page JS cannot read raw opaque resource bodies.
    pub fn drain_network_log(&mut self) -> String {
        let state = self.rt.op_state();
        let mut state = state.borrow_mut();
        fetch_streams::refresh_logs(&mut state);
        let rows: Vec<_> = state.borrow_mut::<NetworkLog>().0.drain(..).collect();
        serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into())
    }

    /// Snapshot the opt-in interface tracer inside each active native child context.
    pub fn frame_interface_snapshots(&mut self) -> Vec<serde_json::Value> {
        let _activation = self.rt.activation_key().enter();
        let watchdog = Watchdog::start(self.rt.v8_isolate().thread_safe_handle(), self.budget_ms);
        let contexts = self
            .rt
            .op_state()
            .borrow()
            .borrow::<FrameContexts>()
            .contexts
            .clone();
        let mut rows = Vec::new();
        for context in contexts {
            deno_core::scope!(scope, self.rt);
            let context = v8::Local::new(scope, context);
            if context.get_slot::<FrameRetired>().is_some() {
                continue;
            }
            let scope = &mut v8::ContextScope::new(scope, context);
            let scope = std::pin::pin!(v8::TryCatch::new(scope));
            let scope = &mut scope.init();
            let Some(code)=v8::String::new(scope,"JSON.stringify({url:document.URL,interfaces:globalThis[Symbol.for('turbo-surf.interface-trace')]?.snapshot()||null})")else{rows.push(serde_json::json!({"error":"child snapshot budget exceeded"}));break;};
            let name = v8::String::new(scope, "<turbo-cdp-host>").unwrap();
            let origin = v8::ScriptOrigin::new(
                scope,
                name.into(),
                0,
                0,
                false,
                0,
                None,
                false,
                false,
                false,
                None,
            );
            let result = v8::Script::compile(scope, code, Some(&origin))
                .and_then(|script| script.run(scope));
            match result {
                Some(value) => match serde_json::from_str(&value.to_rust_string_lossy(scope)) {
                    Ok(row) => rows.push(row),
                    Err(error) => rows.push(serde_json::json!({"error":error.to_string()})),
                },
                None => rows.push(serde_json::json!({"error":"child interface snapshot failed"})),
            }
        }
        drop(watchdog);
        self.rt.v8_isolate().cancel_terminate_execution();
        rows
    }

    /// Native hydration outcome; guest page scripts cannot overwrite this state.
    pub fn hydration_diagnostics(&self) -> &HydrationDiagnostics {
        &self.hydration_diagnostics
    }

    /// Host-owned installed response state; guest URL/referrer getters are never read.
    /// The count includes same-URL form commits and initial-script submissions.
    pub fn navigation_snapshot(&self) -> (u64, String, crate::DocumentResponseMetadata) {
        let state = self.rt.op_state();
        let state = state.borrow();
        (
            self.navigation_count,
            state.borrow::<Base>().0.clone(),
            state.borrow::<crate::DocumentResponseMetadata>().clone(),
        )
    }

    /// Activation is captured when a script navigation is admitted, before timers run.
    pub fn script_nav_activation_snapshot(&mut self) -> Option<(String, bool)> {
        let _activation = self.rt.activation_key().enter();
        deno_core::scope!(scope, self.rt);
        script_navigation::snapshot(scope.get_current_context())
    }

    /// Native sticky activation and source origin for a subsequent document navigation.
    pub fn navigation_activation_snapshot(&mut self) -> NavigationActivationSnapshot {
        let source = self.rt.op_state().borrow().borrow::<Base>().0.clone();
        let _activation = self.rt.activation_key().enter();
        deno_core::scope!(scope, self.rt);
        NavigationActivationSnapshot::capture(source, scope.get_current_context())
    }

    /// Snapshot the host-owned headers of the installed response after navigation.
    pub fn response_headers_snapshot(&self) -> Vec<(String, String)> {
        self.rt
            .op_state()
            .borrow()
            .borrow::<ResponseHeaders>()
            .0
            .clone()
    }

    /// Serialize the CURRENT live DOM to HTML (no reset — the page stays alive).
    pub fn serialize(&self) -> String {
        let _activation = self.rt.activation_key().enter();
        crate::browser_env::document_html()
    }

    /// The page's cookies as a `storageState` JSON string (includes HttpOnly session
    /// cookies the in-isolate `document.cookie` can't see) — so a later navigation can
    /// carry the session established during this page's lifetime (e.g. after login).
    pub fn cookies(&self) -> String {
        apply_report_cookies(&mut self.rt.op_state().borrow_mut());
        let op_state = self.rt.op_state();
        let jar = op_state.borrow().borrow::<Jar>().clone();
        let s = jar.borrow().storage_state();
        s
    }

    /// Tear down: reset the binding while the isolate is still alive, then drop it.
    pub fn close(mut self) {
        let _activation = self.rt.activation_key().enter();
        self.script_inventory.take();
        self.closed = true;
    }
}

impl Drop for PageSession {
    fn drop(&mut self) {
        let _activation = self.rt.activation_key().enter();
        self.script_inventory.take();
    }
}

// A terminated isolate surfaces as a generic execution error; relabel it.
fn budget_msg(e: &str, budget_ms: u64) -> String {
    if e.contains("terminated") || e.contains("execution") {
        format!("render budget exceeded ({budget_ms}ms)")
    } else {
        e.to_string()
    }
}

#[cfg(test)]
mod credentialless_partition_tests {
    use super::*;

    #[test]
    fn expired_credentialless_partition_reclaims_only_its_storage_areas() {
        let storage = BrowserStorage::default();
        {
            let mut areas = storage.0.lock().unwrap();
            for (id, partition, session) in [(1, 0, false), (2, 42, false), (3, 42, true)] {
                areas.by_origin.insert(
                    StorageKey {
                        partition,
                        site: "https://top.test".into(),
                        origin: "https://origin.test".into(),
                        cross_site_ancestor: true,
                        session,
                        tab: 0,
                    },
                    id,
                );
            }
            for id in 1..=3 {
                areas.values.insert(
                    id,
                    vec![(
                        "key".encode_utf16().collect(),
                        "value".encode_utf16().collect(),
                    )],
                );
            }
        }
        let partition = CredentiallessPartition {
            id: 42,
            jar: Rc::new(RefCell::new(CookieJar::new())),
            storage: storage.clone(),
        };
        drop(partition);
        let areas = storage.0.lock().unwrap();
        assert_eq!(areas.by_origin.len(), 1);
        assert_eq!(areas.values.len(), 1);
        assert!(areas.values.contains_key(&1));
    }
}

#[cfg(test)]
#[path = "storage_response_tests.rs"]
mod storage_response_tests;

#[cfg(test)]
mod browsing_tab_tests {
    use super::{BrowserStorage, StorageKey};

    #[test]
    fn session_areas_live_until_the_last_tab_clone_and_local_areas_remain_shared() {
        let profile = BrowserStorage::default();
        let tab = profile.new_tab().unwrap();
        let retained = tab.clone();
        {
            let mut areas = profile.0.lock().unwrap();
            for (id, session, tab_id) in [(1, true, 0), (2, true, tab.2.id), (3, false, 0)] {
                areas.by_origin.insert(
                    StorageKey {
                        partition: 0,
                        site: "https://tabs.test".into(),
                        origin: "https://tabs.test".into(),
                        cross_site_ancestor: false,
                        session,
                        tab: tab_id,
                    },
                    id,
                );
                areas.values.insert(id, vec![(vec![1], vec![2])]);
            }
        }
        drop(tab);
        assert!(profile.0.lock().unwrap().values.contains_key(&2));
        drop(retained);
        let areas = profile.0.lock().unwrap();
        assert!(!areas.values.contains_key(&2));
        assert!(areas.values.contains_key(&1));
        assert!(areas.values.contains_key(&3));
        assert_eq!(areas.by_origin.len(), 2);
        drop(areas);
        profile.0.lock().unwrap().next_tab = u64::MAX;
        assert!(profile.new_tab().is_err());
    }
}

#[cfg(test)]
mod window_timer_deadline_tests {
    use super::{window_timer_deadline, TimerClock};
    use std::time::Duration;

    #[test]
    fn unrepresentable_window_timer_deadlines_do_not_panic() {
        let clock = TimerClock::default();
        assert_eq!(
            window_timer_deadline(&clock, 1000.0),
            Some(tokio::time::Instant::from_std(
                clock.started + Duration::from_secs(1)
            ))
        );
        for due in [f64::NAN, f64::INFINITY, f64::MAX, 1e300, 1e22] {
            assert_eq!(window_timer_deadline(&clock, due), None, "deadline {due}");
        }
    }
}

#[cfg(test)]
mod window_timer_task_tests {
    use super::*;

    async fn page(budget: u64) -> PageSession {
        PageSession::open(
            "<body><iframe id=frame></iframe></body>",
            "https://timer.test/",
            "",
            "",
            budget,
        )
        .await
        .unwrap()
    }

    async fn chain(kind: &str) {
        let mut page = page(5000).await;
        let _activation = page.rt.activation_key().enter();
        page.rt
            .execute_script(
                "<timer-chain>",
                format!(
                    r#"(()=>{{
                      const kind={kind:?},frame=document.querySelector('#frame').contentWindow;
                      const rows=globalThis.timerRows=[];
                      let previous=performance.now(),id;
                      const schedule=()=>{{
                        const target=kind.startsWith('crossRealm')&&rows.length%2?frame:window;
                        const created=performance.now();
                        target.setTimeout(()=>{{
                          rows.push(performance.now()-created);
                          if(rows.length===12)return;
                          if(kind.endsWith('Promise')||kind==='promise')Promise.resolve().then(schedule);
                          else if(kind.endsWith('QueueMicrotask')||kind==='queueMicrotask')queueMicrotask(schedule);
                          else schedule();
                        }},0);
                      }};
                      if(kind==='interval')id=setInterval(()=>{{
                        const now=performance.now();rows.push(now-previous);previous=now;
                        if(rows.length===12)clearInterval(id);
                      }},0);else schedule();
                    }})()"#
                ),
            )
            .unwrap();
        checkpoint_window_timer(&mut page.rt).unwrap();
        for index in 0..12 {
            let turn = run_next_window_timer(&mut page.rt, true);
            assert_eq!(turn.fired, 1, "{kind} task {index}");
            let nesting = turn._task.as_ref().unwrap().nesting.clone();
            let expected = if kind.starts_with("crossRealm") {
                1
            } else {
                (index + 1).min(7)
            };
            assert_eq!(nesting.level.get(), expected, "{kind} task {index}");
            checkpoint_window_timer(&mut page.rt).unwrap();
            assert_eq!(nesting.level.get(), expected, "{kind} checkpoint {index}");
            drop(turn);
            assert_eq!(nesting.level.get(), 0, "{kind} finished task {index}");
        }
        let value = page
            .rt
            .execute_script("<timer-rows>", "JSON.stringify(timerRows)")
            .unwrap();
        let rows: Vec<f64> =
            serde_json::from_str(&read_string(&mut page.rt, value).unwrap()).unwrap();
        assert_eq!(rows.len(), 12);
        if !kind.starts_with("crossRealm") && kind != "interval" {
            // The public clock has a 100us grid. No wall-time upper bound is
            // imposed on the six structurally unclamped tasks or on the host.
            assert!(
                rows[6..].iter().all(|delay| *delay >= 3.9 - 1e-6),
                "{kind}: {rows:?}"
            );
        }
        assert_eq!(run_next_window_timer(&mut page.rt, true).fired, 0);
    }

    async fn retired_task() {
        let mut page = page(5000).await;
        let _activation = page.rt.activation_key().enter();
        page.rt.execute_script("<retired-timer>", "const frame=document.querySelector('#frame');frame.contentWindow.setTimeout(()=>{frame.remove();Promise.resolve().then(()=>setTimeout(()=>{},0));},0)").unwrap();
        let turn = run_next_window_timer(&mut page.rt, true);
        assert_eq!(turn.fired, 1);
        let retired = turn._task.as_ref().unwrap().nesting.clone();
        checkpoint_window_timer(&mut page.rt).unwrap();
        drop(turn);
        assert_eq!(retired.level.get(), 0);
        assert!(page
            .rt
            .op_state()
            .borrow()
            .borrow::<FrameContexts>()
            .contexts
            .is_empty());
        let next = run_next_window_timer(&mut page.rt, true);
        assert_eq!(next.fired, 1);
        assert_eq!(next._task.as_ref().unwrap().nesting.level.get(), 1);
        checkpoint_window_timer(&mut page.rt).unwrap();
    }

    async fn legacy_reentrancy() {
        let mut page = page(5000).await;
        let _activation = page.rt.activation_key().enter();
        page.rt.execute_script("<nested-legacy-batch>", "setTimeout(()=>{setTimeout(()=>{},0);globalThis[Symbol.for('ts')].runTimers(1);setTimeout(()=>{},0);},0)").unwrap();
        let turn = run_next_window_timer(&mut page.rt, true);
        assert_eq!(turn.fired, 1);
        assert_eq!(turn._task.as_ref().unwrap().nesting.level.get(), 1);
        checkpoint_window_timer(&mut page.rt).unwrap();
        drop(turn);
        let next = run_next_window_timer(&mut page.rt, true);
        assert_eq!(next.fired, 1);
        assert_eq!(next._task.as_ref().unwrap().nesting.level.get(), 2);
        checkpoint_window_timer(&mut page.rt).unwrap();
    }

    async fn interrupted_checkpoint(legacy: bool) {
        let mut page = page(100).await;
        let expression = if legacy {
            "(()=>{setTimeout(()=>{while(true){}},0);globalThis[Symbol.for('ts')].runTimers();return 'unreachable';})()"
        } else {
            "new Promise(()=>setTimeout(()=>Promise.resolve().then(()=>{while(true){}}),0))"
        };
        let error = page.eval_json_expression(expression).await.unwrap_err();
        assert!(error.contains("budget"), "{error}");
        let _activation = page.rt.activation_key().enter();
        let nesting = {
            deno_core::scope!(scope, page.rt);
            window_timer_nesting(scope.get_current_context())
        };
        assert_eq!(nesting.level.get(), 0);
        page.rt
            .execute_script("<after-interruption>", "setTimeout(()=>{},0)")
            .unwrap();
        let turn = run_next_window_timer(&mut page.rt, true);
        assert_eq!(turn.fired, 1);
        assert_eq!(turn._task.as_ref().unwrap().nesting.level.get(), 1);
        checkpoint_window_timer(&mut page.rt).unwrap();
    }

    #[test]
    fn timer_nesting_is_per_window_and_survives_only_its_task_checkpoint() {
        const CHILD: &str = "TURBO_SURF_TIMER_TASK_TEST_CHILD";
        if std::env::var_os(CHILD).is_some() {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    for kind in [
                        "direct",
                        "promise",
                        "queueMicrotask",
                        "crossRealm",
                        "crossRealmPromise",
                        "crossRealmQueueMicrotask",
                        "interval",
                    ] {
                        chain(kind).await;
                    }
                    retired_task().await;
                    legacy_reentrancy().await;
                    interrupted_checkpoint(false).await;
                    interrupted_checkpoint(true).await;
                });
            return;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "runtime::window_timer_task_tests::timer_nesting_is_per_window_and_survives_only_its_task_checkpoint", "--nocapture"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "timer task subprocess failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
