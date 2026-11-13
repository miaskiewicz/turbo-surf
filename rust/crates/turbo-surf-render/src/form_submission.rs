//! Host-owned form admission. Accepted requests retain native snapshots, not realms.
use super::*;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Payload {
    action: Option<String>,
    method: String,
    enctype: String,
    target: String,
    accept_charset: Option<String>,
    #[serde(skip)]
    entries: Vec<(String, String)>,
}

pub(super) struct Pending {
    pub target: Option<u64>,
    pub plan: form_navigation::Submission,
    pub source: String,
    pub user_activation: bool,
    pub policy: crate::csp::Policy,
    pub referrer_policy: turbo_surf_core::referrer_policy::ReferrerPolicy,
}
#[derive(Default)]
pub(super) struct Queue(pub Option<Pending>);

pub(super) fn install(scope: &mut v8::PinScope) -> Result<(), JsErrorBox> {
    let callback = v8::Function::new(scope, submit)
        .ok_or_else(|| JsErrorBox::generic("native form callback initialization interrupted"))?;
    if !crate::browser_env::install_form_submission_handler(scope, callback) {
        return Err(JsErrorBox::generic(
            "native form authorization initialization interrupted",
        ));
    }
    Ok(())
}

fn fail(scope: &mut v8::PinScope, message: &str) {
    let text = v8::String::new(scope, message).unwrap();
    let error = v8::Exception::type_error(scope, text);
    scope.throw_exception(error);
}

fn submit<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut result: v8::ReturnValue,
) {
    let operation = args.get(0).to_rust_string_lossy(scope);
    let context = scope.get_current_context();
    let active = context.get_slot::<FrameRetired>().is_none();
    if operation == "active" {
        result.set_bool(active);
        return;
    }
    if operation != "submit" || !active {
        return;
    }
    let Ok(form) = v8::Local::<v8::Object>::try_from(args.get(1)) else {
        return;
    };
    let outcome = match collect(scope, context, form, args.get(3)) {
        Ok(Some(submission)) => navigate(scope, context, form, submission),
        Ok(None) => Ok(false),
        Err(error) => Err(error),
    };
    match outcome {
        Ok(handled) => {
            if handled {
                result.set_bool(true);
            }
        }
        Err(error) => fail(scope, &error),
    }
}

/// A submission's form data set and request shape, before its target is chosen.
struct Collected {
    payload: Payload,
    source: String,
    action: String,
    target: String,
    encoding: &'static encoding_rs::Encoding,
    files: Vec<Option<form_navigation::FilePart>>,
}

/// The submission's entries after the mutable `formdata` event. `Ok(None)` when the
/// event or a capability aborted it (any exception is already pending).
fn collect<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    form: v8::Local<'s, v8::Object>,
    raw: v8::Local<'s, v8::Value>,
) -> Result<Option<Collected>, String> {
    let mut payload: Payload = deno_core::serde_v8::from_v8(scope, raw).map_err(|_| {
        String::from("Native form entries require supported text or private File values")
    })?;
    payload.entries = entries(scope, context, raw)?;
    let (source, action) = submission_urls(scope, context, form, &payload)?;
    let target = if payload.target.is_empty() {
        crate::browser_env::form_submission_base_target(scope, form).unwrap_or_default()
    } else {
        payload.target.clone()
    };
    let state = JsRuntime::op_state_from(scope);
    let encoding = document_encoding(
        &state.borrow(),
        payload.accept_charset.as_deref().unwrap_or(""),
    );
    let Some(snapshot) = dispatch_formdata(scope, context, form, raw, encoding) else {
        return Ok(None);
    };
    payload.entries = entries(scope, context, snapshot)?;
    // Charset replacement (`_charset_` entries) already preceded the mutable formdata
    // event, so the binding's charset entry indices are not applied again.
    let files = if payload.method == "post" && payload.enctype == "multipart/form-data" {
        file_parts(scope, context, snapshot)?
    } else {
        Vec::new()
    };
    Ok(Some(Collected {
        payload,
        source,
        action,
        target,
        encoding,
        files,
    }))
}

/// The submitting document's URL and the resolved action (the document URL when
/// the action is empty).
fn submission_urls(
    scope: &mut v8::PinScope,
    context: v8::Local<v8::Context>,
    form: v8::Local<v8::Object>,
    payload: &Payload,
) -> Result<(String, String), String> {
    let state = JsRuntime::op_state_from(scope);
    let source = state.borrow().borrow::<Base>().0.clone();
    let source = window_proxy::location_url(context)
        .or_else(|| {
            context
                .get_slot::<FrameDocumentUrl>()
                .map(|url| url.0.clone())
        })
        .unwrap_or(source);
    let base = approved_base(scope, context, form, &source);
    let action = match payload.action.as_deref().filter(|value| !value.is_empty()) {
        Some(action) => resolve(&base, action),
        None => Some(source.clone()),
    }
    .ok_or("Native form action cannot be resolved")?;
    Ok((source, action))
}

/// Fires `formdata` over a fresh submission FormData and snapshots its entries.
fn dispatch_formdata<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    form: v8::Local<'s, v8::Object>,
    raw: v8::Local<'s, v8::Value>,
    encoding: &'static encoding_rs::Encoding,
) -> Option<v8::Local<'s, v8::Value>> {
    let data = submission_form_data(scope, context, raw, encoding.name())?;
    if !crate::browser_env::dispatch_formdata_event(scope, form, data) {
        return None;
    }
    call_capability(scope, context, "snapshotFormValue", &[data])
}

/// Plans the navigation in the chosen navigable. `Ok(true)` when the submission was
/// handled (including policy and sandbox blocks, which Chrome drops silently).
fn navigate<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    form: v8::Local<'s, v8::Object>,
    submission: Collected,
) -> Result<bool, String> {
    let Collected {
        payload,
        source,
        action,
        target,
        encoding,
        files,
    } = submission;
    if !csp_request_for_context(scope, context, form, &action, "form-action", "") {
        return Ok(true);
    }
    let javascript = url::Url::parse(&action).is_ok_and(|url| url.scheme() == "javascript");
    let Some(navigable) = choose_navigable(scope, context, &target) else {
        // Chrome opens a new window for `_blank` and unknown names. A no-browser
        // engine has none: a javascript: URL there runs outside this frame tree,
        // while a navigation cannot be honored.
        return if javascript {
            Ok(true)
        } else {
            Err("Native form submission to another browsing context is not supported".into())
        };
    };
    if sandbox_blocks(scope, context, navigable) {
        return Ok(true);
    }
    if javascript {
        // A javascript: URL runs only in a navigable of the initiator's origin.
        if same_origin(scope, context, navigable) {
            queue_javascript_url(scope, navigable, &action);
        }
        return Ok(true);
    }
    let plan = form_navigation::prepare(
        &source,
        &action,
        &payload.method,
        &payload.enctype,
        "",
        &payload.entries,
        &files,
        encoding,
    )?;
    if navigable.get_slot::<FrameHost>().is_some() {
        navigate_child(scope, context, form, navigable, &plan, &source);
        return Ok(true);
    }
    let Some(handle) = context.get_slot::<CspHandle>() else {
        return Ok(false);
    };
    let policy = handle.policy.borrow();
    // A later submission replaces the earlier pending navigation before any wire IO.
    let state = JsRuntime::op_state_from(scope);
    state.borrow_mut().borrow_mut::<Queue>().0 = Some(Pending {
        target: auxiliary_pages::top_level_identity(scope, navigable).map(|id| id.0),
        plan,
        source,
        user_activation: host_user_activation(context),
        policy: policy.active.clone(),
        referrer_policy: policy.referrer_policy,
    });
    Ok(true)
}

/// A sandboxed document navigates only itself and its descendants unless the sandbox
/// grants top navigation (Chrome blocks with a console message).
fn sandbox_blocks<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    navigable: v8::Local<'s, v8::Context>,
) -> bool {
    if navigable == context || !ancestor_of(scope, navigable, context) {
        return false;
    }
    // Every sandbox sets the document.domain flag, whatever its tokens.
    crate::runtime::sandbox_blocks_top_navigation(context, navigable)
        || (navigable.get_slot::<FrameParent>().is_some() && sandboxed(context))
}

fn sandboxed(context: v8::Local<v8::Context>) -> bool {
    context
        .get_slot::<FrameSandbox>()
        .is_some_and(|flags| flags.document_domain_blocked)
}

fn same_origin<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    left: v8::Local<'s, v8::Context>,
    right: v8::Local<'s, v8::Context>,
) -> bool {
    let token = left.get_security_token(scope);
    right.get_security_token(scope).strict_equals(token)
}

/// Navigating to a `javascript:` URL runs its percent-decoded script in a later task of
/// the same document (Chrome 156; the document stays in place, turbo-surf#100), subject
/// to the document's inline-script policy.
fn queue_javascript_url<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    url: &str,
) {
    let scope = &mut v8::ContextScope::new(scope, context);
    let allowed = context.get_slot::<CspHandle>().is_none_or(|handle| {
        handle
            .policy
            .borrow()
            .active
            .allows_inline("script-src-elem", url, "")
    });
    if !allowed {
        return;
    }
    let body = &url[url.find(':').map_or(url.len(), |colon| colon + 1)..];
    let source = percent_encoding::percent_decode_str(body).decode_utf8_lossy();
    let Some(source) = v8::String::new(scope, &source) else {
        return;
    };
    let Some(source) = trusted_javascript_url(scope, context, source) else {
        return;
    };
    let Some(task) = v8::Function::builder(run_javascript_url)
        .data(source.into())
        .build(scope)
    else {
        return;
    };
    let global = context.global(scope);
    let key = v8::String::new(scope, "setTimeout").unwrap();
    let Some(schedule) = global
        .get(scope, key.into())
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    else {
        return;
    };
    let receiver = global.into();
    schedule.call(scope, receiver, &[task.into()]);
}

/// The navigable a form submission targets (HTML "rules for choosing a navigable",
/// as Chrome 156 runs them): this document (`""`, `_self`), its parent or top-level
/// document, or the first frame answering to the name. `None` for a new window
/// (`_blank`, other `_` keywords, and names no navigable it may navigate answers to).
fn choose_navigable<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    target: &str,
) -> Option<v8::Local<'s, v8::Context>> {
    if target.is_empty() || target.eq_ignore_ascii_case("_self") {
        return Some(context);
    }
    if target.eq_ignore_ascii_case("_parent") {
        return Some(parent(scope, context).unwrap_or(context));
    }
    if target.eq_ignore_ascii_case("_top") {
        return Some(top(scope, context));
    }
    if target.starts_with('_') {
        return None;
    }
    named_navigable(scope, context, target)
}

fn parent<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) -> Option<v8::Local<'s, v8::Context>> {
    context
        .get_slot::<FrameParent>()
        .and_then(|parent| parent.0.to_local(scope))
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

/// `root` and its descendant frames in frame-tree order (children in creation
/// order, which Chrome keeps whatever their DOM order).
fn frame_tree<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    root: v8::Local<'s, v8::Context>,
    out: &mut Vec<v8::Local<'s, v8::Context>>,
) {
    out.push(root);
    for child in frame_children(scope, root) {
        frame_tree(scope, child, out);
    }
}

/// Chrome searches the submitter's own subtree first, then the rest of its frame
/// tree, skipping frames the submitter may not navigate. A frame's name is its
/// container's `name` attribute. Other top-level windows do not exist here.
fn named_navigable<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    name: &str,
) -> Option<v8::Local<'s, v8::Context>> {
    let mut candidates = Vec::new();
    frame_tree(scope, context, &mut candidates);
    let own = candidates.len();
    let root = top(scope, context);
    frame_tree(scope, root, &mut candidates);
    let (subtree, rest) = candidates.split_at(own);
    let order: Vec<_> = subtree
        .iter()
        .chain(rest.iter().filter(|frame| !subtree.contains(frame)))
        .copied()
        .collect();
    order.into_iter().find(|&candidate| {
        frame_name(scope, candidate).as_deref() == Some(name)
            && may_navigate(scope, context, candidate)
    })
}

fn frame_name(scope: &mut v8::PinScope, context: v8::Local<v8::Context>) -> Option<String> {
    let host = context.get_slot::<FrameHost>()?;
    let host = host.0.to_local(scope)?;
    crate::browser_env::node_attribute(scope, host, "name")
}

/// Chrome's navigation right: a document navigates a frame it shares an origin with,
/// or whose ancestor it shares one with (which covers its own descendants). A
/// sandboxed document navigates only its descendants; its ancestors are left to
/// `sandbox_blocks`.
fn may_navigate<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    source: v8::Local<'s, v8::Context>,
    target: v8::Local<'s, v8::Context>,
) -> bool {
    if sandboxed(source) {
        return target == source
            || ancestor_of(scope, source, target)
            || ancestor_of(scope, target, source);
    }
    let mut frame = Some(target);
    while let Some(current) = frame {
        if same_origin(scope, source, current) {
            return true;
        }
        frame = parent(scope, current);
    }
    false
}

fn ancestor_of<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    ancestor: v8::Local<'s, v8::Context>,
    mut context: v8::Local<'s, v8::Context>,
) -> bool {
    while let Some(parent) = parent(scope, context) {
        if parent == ancestor {
            return true;
        }
        context = parent;
    }
    false
}

/// A child-frame form navigation in flight: the submitter's `form-action` gate its
/// fetch carries, and where a refused hop is reported.
struct FrameFormNavigation {
    submitter: v8::Weak<v8::Context>,
    form: v8::Weak<v8::Object>,
    gate: std::sync::Arc<crate::csp::FormActionGate>,
    url: String,
}
#[derive(Default)]
struct FrameFormNavigations(std::collections::HashMap<u32, FrameFormNavigation>);

/// Hands a child navigable's form navigation to its parent's frame loader, which
/// fetches it with navigation semantics (`op_fetch`, destination `iframe`) under
/// the parent's policy narrowed by the submitter's `form-action` gate.
fn navigate_child<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    submitter: v8::Local<'s, v8::Context>,
    form: v8::Local<'s, v8::Object>,
    navigable: v8::Local<'s, v8::Context>,
    plan: &form_navigation::Submission,
    source: &str,
) {
    let Some(parent) = parent(scope, navigable) else {
        return;
    };
    let navigator = parent
        .get_slot::<FrameNavigator>()
        .and_then(|navigator| navigator.0.to_local(scope));
    let host = navigable
        .get_slot::<FrameHost>()
        .and_then(|host| host.0.to_local(scope));
    let (Some(navigator), Some(host)) = (navigator, host) else {
        return;
    };
    let gated = gate_navigation(scope, submitter, form, parent, &plan.url);
    // POST navigations revalidate (Chrome's `Cache-Control: max-age=0`).
    let mut request = serde_json::json!({
        "method": plan.method,
        "headers": plan.headers,
        "body_bytes": plan.body,
        "cache": if plan.method == "POST" { "no-cache" } else { "default" },
    });
    if let Some(id) = gated {
        // Later keys win over the loader's own `csp` (the parent's context).
        request["csp"] = id.into();
        request["form_navigation"] = id.into();
    }
    let (Some(url), Some(source), Some(request)) = (
        v8::String::new(scope, &plan.url),
        v8::String::new(scope, source),
        v8::String::new(scope, &request.to_string()),
    ) else {
        return;
    };
    let undefined = v8::undefined(scope);
    let scope = &mut v8::ContextScope::new(scope, parent);
    navigator.call(
        scope,
        undefined.into(),
        &[host.into(), url.into(), source.into(), request.into()],
    );
}

/// Registers a policy context for one child-frame form navigation: the parent's
/// (frame-src, partition, referrer and embedder policy) with the submitter's
/// `form-action` checked on every hop. Its id replaces the loader's `csp`.
fn gate_navigation<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    submitter: v8::Local<'s, v8::Context>,
    form: v8::Local<'s, v8::Object>,
    parent: v8::Local<'s, v8::Context>,
    url: &str,
) -> Option<u32> {
    let own = submitter.get_slot::<CspHandle>()?;
    let embedder = parent.get_slot::<CspHandle>()?;
    let gate = std::sync::Arc::new(crate::csp::FormActionGate::new(
        own.policy.borrow().active.clone(),
    ));
    let fresh = CspHandle::new(url, &[], "");
    {
        let source = embedder.policy.borrow();
        let mut policy = fresh.policy.borrow_mut();
        policy.active = source.active.with_form_action(gate.clone());
        policy.referrer_policy = source.referrer_policy;
        policy.embedder_policy = source.embedder_policy;
    }
    let handle = CspHandle {
        partition: embedder.partition.clone(),
        response_metadata: embedder.response_metadata.clone(),
        dictionaries: embedder.dictionaries.clone(),
        ..fresh
    };
    let id = handle.id;
    let navigation = FrameFormNavigation {
        submitter: v8::Weak::new(scope, submitter),
        form: v8::Weak::new(scope, form),
        gate,
        url: url.to_string(),
    };
    let state = JsRuntime::op_state_from(scope);
    let mut state = state.borrow_mut();
    state.borrow_mut::<CspContexts>().0.insert(id, handle);
    if !state.has::<FrameFormNavigations>() {
        state.put(FrameFormNavigations::default());
    }
    state
        .borrow_mut::<FrameFormNavigations>()
        .0
        .insert(id, navigation);
    Some(id)
}

/// The frame loader's verdict on a fetched child-frame form navigation: `true` when
/// the submitter's `form-action` refused a hop, so the navigation is cancelled (the
/// frame keeps its document and fires no load) and the submitter reports the
/// violation against the navigation's initial URL (Chrome 156). Releases the
/// navigation's policy context either way.
#[op2(nofast, reentrant)]
pub(super) fn op_form_navigation_blocked(
    scope: &mut v8::PinScope,
    #[string] request: &str,
) -> bool {
    let id = serde_json::from_str::<serde_json::Value>(request)
        .ok()
        .and_then(|request| request.get("form_navigation")?.as_u64())
        .and_then(|id| u32::try_from(id).ok());
    let Some(id) = id else {
        return false;
    };
    let navigation = {
        let state = JsRuntime::op_state_from(scope);
        let mut state = state.borrow_mut();
        state.borrow_mut::<CspContexts>().0.remove(&id);
        state
            .try_borrow_mut::<FrameFormNavigations>()
            .and_then(|navigations| navigations.0.remove(&id))
    };
    let Some(navigation) = navigation else {
        return false;
    };
    let Some(refused) = navigation.gate.refused() else {
        return false;
    };
    if let (Some(submitter), Some(form)) = (
        navigation.submitter.to_local(scope),
        navigation.form.to_local(scope),
    ) {
        let scope = &mut v8::ContextScope::new(scope, submitter);
        let blocked = csp_blocked_url(scope, submitter, &navigation.url);
        let records = navigation
            .gate
            .policy()
            .request_violations("form-action", &refused, "");
        queue_csp_violations(
            scope,
            submitter,
            form,
            "form-action",
            &blocked,
            "enforce",
            records,
        );
    }
    true
}

/// Under `require-trusted-types-for 'script'` a `javascript:` URL's script passes the
/// default policy's `createScript` (sink "Location href") or is blocked (Chrome 156).
fn trusted_javascript_url<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    source: v8::Local<'s, v8::String>,
) -> Option<v8::Local<'s, v8::String>> {
    let required = context.get_slot::<CspHandle>().is_some_and(|handle| {
        let policy = handle.policy.borrow();
        policy.current().requires_trusted_types() || policy.report_only.requires_trusted_types()
    });
    if !required {
        return Some(source);
    }
    let key = v8::String::new(scope, "turbo-dom.ownership.window-document")?;
    let key = v8::Private::for_api(scope, Some(key));
    let document = context
        .global(scope)
        .get_private(scope, key)
        .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())?;
    v8::tc_scope!(let scope, scope);
    let converted = crate::browser_env::trusted_type_string(
        scope,
        document,
        source.into(),
        "TrustedScript",
        "Location href",
        "",
    );
    if converted.is_none() {
        scope.reset();
    }
    converted
}

fn run_javascript_url(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    _result: v8::ReturnValue,
) {
    let Ok(source) = v8::Local::<v8::String>::try_from(args.data()) else {
        return;
    };
    // A compile or runtime error propagates to the task runner, like any task's.
    if let Some(script) = v8::Script::compile(scope, source, None) {
        let _ = script.run(scope);
    }
}

fn document_encoding(state: &OpState, accept_charset: &str) -> &'static encoding_rs::Encoding {
    if accept_charset
        .split_ascii_whitespace()
        .any(|value| value.eq_ignore_ascii_case("utf-8"))
    {
        return encoding_rs::UTF_8;
    }
    state
        .borrow::<ResponseHeaders>()
        .0
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .and_then(|(_, value)| {
            value.split(';').skip(1).find_map(|parameter| {
                let (name, value) = parameter.trim().split_once('=')?;
                name.trim()
                    .eq_ignore_ascii_case("charset")
                    .then(|| value.trim().trim_matches(['\'', '"']))
            })
        })
        .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()))
        .filter(|encoding| {
            **encoding != *encoding_rs::UTF_16LE
                && **encoding != *encoding_rs::UTF_16BE
                && **encoding != *encoding_rs::REPLACEMENT
        })
        .unwrap_or(encoding_rs::UTF_8)
}

impl PageSession {
    pub(super) async fn follow_form_navigation(&mut self) -> Result<(), String> {
        for _ in 0..20 {
            let pending = self
                .rt
                .op_state()
                .borrow_mut()
                .borrow_mut::<Queue>()
                .0
                .take();
            let Some(pending) = pending else {
                return Ok(());
            };
            if self
                .rt
                .op_state()
                .borrow()
                .borrow::<auxiliary_pages::Pages>()
                .enabled
            {
                let target = pending
                    .target
                    .ok_or("Form navigation has no native owned Page identity")?;
                self.navigate_form_target(target, pending).await?;
                continue;
            }
            let (jar, ua, hints, storage, init_script) = {
                let state = self.rt.op_state();
                let state = state.borrow();
                (
                    state.borrow::<Jar>().clone(),
                    state.borrow::<Ua>().0.clone(),
                    state.borrow::<ClientHints>().clone(),
                    state.borrow::<BrowserStorage>().clone(),
                    state.borrow::<FrameContexts>().init_script.clone(),
                )
            };
            let mut local = jar.borrow().fork_for_request();
            let mut context =
                RequestContext::navigation(Some(pending.source.clone()), pending.user_activation);
            context.referrer_policy = pending.referrer_policy;
            let final_referrer = std::sync::Mutex::new(String::new());
            let final_location = std::sync::Mutex::new(pending.plan.url.clone());
            let response_policy =
                |context: &RequestContext, target: &str, _headers: &[(String, String)]| {
                    *final_location.lock().unwrap() = target.to_string();
                    *final_referrer.lock().unwrap() = context
                        .referrer_policy
                        .referrer(
                            context.referrer.as_deref().unwrap_or(&pending.source),
                            target,
                        )
                        .unwrap_or_default();
                    true
                };
            let request_policy = |target: &str, redirected: bool| {
                pending
                    .policy
                    .allows_request("form-action", target, "", redirected)
            };
            let cache = storage.http_cache();
            let effects = storage.response_storage_effects();
            let mut headers = pending.plan.headers;
            if !ua.is_empty() {
                headers.insert("user-agent".into(), ua.clone());
            }
            let fetched = tokio::time::timeout(
                std::time::Duration::from_millis(self.budget_ms),
                fetch_html(
                    &pending.plan.url,
                    FetchOptions {
                        method: Some(pending.plan.method.clone()),
                        body_bytes: pending.plan.body,
                        headers,
                        context: Some(context),
                        jar: Some(&mut local),
                        client_hints: Some(&hints),
                        http_cache: Some(&cache),
                        response_side_effects: Some(effects.as_ref()),
                        request_policy: Some(&request_policy),
                        response_policy: Some(&response_policy),
                        max_redirects: Some(20),
                        now: cookie_now(),
                        ..Default::default()
                    },
                ),
            )
            .await;
            jar.borrow_mut().merge_request_changes(local);
            let mut response = fetched
                .map_err(|_| "Native form navigation exceeded the session execution budget")?
                .map_err(|error| format!("Native form navigation failed: {error}"))?;
            response.final_url = final_location.into_inner().unwrap();
            let metadata = crate::DocumentResponseMetadata {
                status: response.status,
                referrer: final_referrer.into_inner().unwrap(),
            };
            let cookies = jar.borrow().storage_state();
            // No ActivationKey guard is live here. Hydrate the response with its real
            // policy/metadata, then dispose the retired isolate after the swap.
            let activation = self.navigation_activation_snapshot();
            let engine_init_script = self
                .rt
                .op_state()
                .borrow()
                .borrow::<FrameContexts>()
                .engine_init_script
                .clone();
            let network = self.network_event_log();
            network.push(serde_json::json!({"kind":"navigation","initiator":"form","url":pending.plan.url,"final_url":response.final_url,"status":response.status,"method":pending.plan.method,"response_received":true}));
            let auxiliary = {
                let state = self.rt.op_state();
                let state = state.borrow();
                let pages = state.borrow::<auxiliary_pages::Pages>();
                AuxiliaryHostServices {
                    bootstrap: pages.bootstrap.clone(),
                    root_bootstrap: pages.root_bootstrap.clone(),
                    events: pages.events.clone(),
                    policy: pages.policy,
                }
            };
            let mut next = PageSession::open_with_document_response_unfollowed(
                &response.html,
                &response.final_url,
                &cookies,
                &ua,
                self.budget_ms,
                &init_script,
                hints,
                &response.headers,
                storage,
                metadata,
                Some(activation),
                self.options,
                &engine_init_script,
                Some(network),
                auxiliary,
            )
            .await?;
            next.navigation_count = self
                .navigation_count
                .checked_add(1)
                .ok_or("Native navigation counter exhausted")?;
            std::mem::swap(self, &mut next);
            drop(next);
        }
        Err("Native form navigation exceeded its redirect/document budget".into())
    }
}

fn approved_base(
    scope: &mut v8::PinScope,
    context: v8::Local<v8::Context>,
    form: v8::Local<v8::Object>,
    source: &str,
) -> String {
    let Some(base) = crate::browser_env::form_submission_base_href(scope, form)
        .and_then(|href| resolve(source, &href))
    else {
        return source.to_string();
    };
    let safe =
        url::Url::parse(&base).is_ok_and(|url| !matches!(url.scheme(), "data" | "javascript"));
    let allowed = context.get_slot::<CspHandle>().is_none_or(|handle| {
        handle
            .policy
            .borrow()
            .active
            .allows_request("base-uri", &base, "", false)
    });
    if safe && allowed {
        base
    } else {
        source.to_string()
    }
}

fn entries<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    payload: v8::Local<'s, v8::Value>,
) -> Result<Vec<(String, String)>, String> {
    let payload = payload
        .to_object(scope)
        .ok_or("Native form snapshot is not an object")?;
    let key = v8::String::new(scope, "entries").unwrap();
    let rows = payload
        .get(scope, key.into())
        .and_then(|value| v8::Local::<v8::Array>::try_from(value).ok())
        .ok_or("Native form entry list is not an array")?;
    let mut result = Vec::with_capacity(rows.length() as usize);
    for i in 0..rows.length() {
        let pair = rows
            .get_index(scope, i)
            .and_then(|value| v8::Local::<v8::Array>::try_from(value).ok())
            .ok_or("Native form entry is not a pair")?;
        let name = pair
            .get_index(scope, 0)
            .filter(|value| value.is_string())
            .ok_or("Native form name is not text")?
            .to_rust_string_lossy(scope);
        let value = pair
            .get_index(scope, 1)
            .ok_or("Native form value is missing")?;
        let text = if value.is_string() {
            value.to_rust_string_lossy(scope)
        } else {
            file_name(scope, context, value)?
        };
        result.push((name, text));
    }
    Ok(result)
}

/// Native snapshots of each File entry's name, type and bytes, for multipart bodies.
fn file_parts<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    payload: v8::Local<'s, v8::Value>,
) -> Result<Vec<Option<form_navigation::FilePart>>, String> {
    let payload = payload
        .to_object(scope)
        .ok_or("Native form snapshot is not an object")?;
    let key = v8::String::new(scope, "entries").unwrap();
    let rows = payload
        .get(scope, key.into())
        .and_then(|value| v8::Local::<v8::Array>::try_from(value).ok())
        .ok_or("Native form entry list is not an array")?;
    let mut result = Vec::with_capacity(rows.length() as usize);
    for i in 0..rows.length() {
        let value = rows
            .get_index(scope, i)
            .and_then(|value| v8::Local::<v8::Array>::try_from(value).ok())
            .and_then(|pair| pair.get_index(scope, 1))
            .ok_or("Native form value is missing")?;
        if value.is_string() {
            result.push(None);
            continue;
        }
        let snapshot = call_capability(scope, context, "snapshotFormFile", &[value])
            .and_then(|snapshot| snapshot.to_object(scope))
            .ok_or("Native form value is not a private File")?;
        let text = |name: &str| {
            let key = v8::String::new(scope, name).unwrap();
            snapshot
                .get(scope, key.into())
                .filter(|value| value.is_string())
                .map(|value| value.to_rust_string_lossy(scope))
                .ok_or("Native File snapshot is incomplete")
        };
        let (name, content_type) = (text("name")?, text("type")?);
        let key = v8::String::new(scope, "bytes").unwrap();
        let bytes = snapshot
            .get(scope, key.into())
            .and_then(|value| v8::Local::<v8::Uint8Array>::try_from(value).ok())
            .ok_or("Native File snapshot has no bytes")?;
        let mut data = vec![0; bytes.byte_length()];
        bytes.copy_contents(&mut data);
        result.push(Some(form_navigation::FilePart {
            name,
            content_type,
            bytes: data,
        }));
    }
    Ok(result)
}

fn file_name<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    value: v8::Local<'s, v8::Value>,
) -> Result<String, String> {
    let reader = context
        .get_slot::<HostCapabilities>()
        .and_then(|capabilities| {
            capabilities
                .0
                .borrow()
                .get("snapshotFormFileName")
                .and_then(|reader| reader.to_local(scope))
        })
        .ok_or("Native File reader is unavailable")?;
    let receiver = v8::undefined(scope).into();
    reader
        .call(scope, receiver, &[value])
        .filter(|value| value.is_string())
        .map(|value| value.to_rust_string_lossy(scope))
        .ok_or("Native form value is not a private File".into())
}

#[op2(reentrant)]
pub(super) fn op_form_data_entries<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    form: v8::Local<'s, v8::Value>,
    submitter: v8::Local<'s, v8::Value>,
) -> v8::Local<'s, v8::Value> {
    crate::browser_env::begin_form_data_constructor_entries(scope, form, submitter)
        .map(Into::into)
        .unwrap_or_else(|| v8::undefined(scope).into())
}

#[op2(nofast, reentrant)]
pub(super) fn op_form_data_finish(scope: &mut v8::PinScope, form: v8::Local<v8::Value>) {
    crate::browser_env::finish_form_data_constructor(scope, form);
}
#[op2(nofast, reentrant)]
pub(super) fn op_form_data_event<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    form: v8::Local<'s, v8::Object>,
    data: v8::Local<'s, v8::Value>,
) -> bool {
    crate::browser_env::dispatch_formdata_event(scope, form, data)
}

fn call_capability<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    name: &str,
    args: &[v8::Local<'s, v8::Value>],
) -> Option<v8::Local<'s, v8::Value>> {
    let callback = context
        .get_slot::<HostCapabilities>()
        .and_then(|capabilities| {
            capabilities
                .0
                .borrow()
                .get(name)
                .and_then(|callback| callback.to_local(scope))
        });
    let Some(callback) = callback else {
        fail(scope, "Native form data capability is unavailable");
        return None;
    };
    let receiver = v8::undefined(scope).into();
    callback.call(scope, receiver, args)
}
fn submission_form_data<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    payload: v8::Local<'s, v8::Value>,
    encoding: &str,
) -> Option<v8::Local<'s, v8::Value>> {
    let payload = payload.to_object(scope)?;
    let entries_key = v8::String::new(scope, "entries").unwrap();
    let entries = payload.get(scope, entries_key.into())?;
    let charset_key = v8::String::new(scope, "charsetEntries").unwrap();
    let charset = payload.get(scope, charset_key.into())?;
    let encoding = v8::String::new(scope, encoding)?.into();
    call_capability(
        scope,
        context,
        "createSubmissionFormData",
        &[entries, charset, encoding],
    )
}
