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
    charset_entries: Vec<usize>,
}

pub(super) struct Pending {
    pub plan: form_navigation::Submission,
    pub source: String,
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
    if context.get_slot::<FrameContextMarker>().is_some() {
        fail(
            scope,
            "Native form navigation from child frames is not supported",
        );
        return;
    }
    let Ok(form) = v8::Local::<v8::Object>::try_from(args.get(1)) else {
        return;
    };
    let mut payload: Payload = match deno_core::serde_v8::from_v8(scope, args.get(3)) {
        Ok(payload) => payload,
        Err(_) => {
            fail(
                scope,
                "Native form entries require supported text or private File values",
            );
            return;
        }
    };
    payload.entries = match entries(scope, context, args.get(3)) {
        Ok(entries) => entries,
        Err(error) => {
            fail(scope, &error);
            return;
        }
    };
    let state = JsRuntime::op_state_from(scope);
    let source = state.borrow().borrow::<Base>().0.clone();
    #[cfg(turbo_surf_native_proxy)]
    let source = window_proxy::location_url(context).unwrap_or(source);
    let base = approved_base(scope, context, form, &source);
    let action = payload.action.as_deref().filter(|value| !value.is_empty());
    let Some(action) = action.map_or_else(|| Some(source.clone()), |action| resolve(&base, action))
    else {
        fail(scope, "Native form action cannot be resolved");
        return;
    };
    let target = if payload.target.is_empty() {
        crate::browser_env::form_submission_base_target(scope, form).unwrap_or_default()
    } else {
        payload.target.clone()
    };
    let encoding = document_encoding(
        &state.borrow(),
        payload.accept_charset.as_deref().unwrap_or(""),
    );
    let Some(data) = submission_form_data(scope, context, args.get(3), encoding.name()) else {
        return;
    };
    if !crate::browser_env::dispatch_formdata_event(scope, form, data) {
        return;
    }
    let Some(snapshot) = call_capability(scope, context, "snapshotFormValue", &[data]) else {
        return;
    };
    payload.entries = match entries(scope, context, snapshot) {
        Ok(entries) => entries,
        Err(error) => {
            fail(scope, &error);
            return;
        }
    };
    // Charset replacement already preceded the mutable formdata event.
    payload.charset_entries.clear();
    if !csp_request_for_context(scope, context, form, &action, "form-action", "") {
        result.set_bool(true);
        return;
    }
    let plan = match form_navigation::prepare(
        &source,
        &action,
        &payload.method,
        &payload.enctype,
        &target,
        &payload.entries,
        &payload.charset_entries,
        encoding,
    ) {
        Ok(plan) => plan,
        Err(error) => {
            fail(scope, &error);
            return;
        }
    };
    let Some(handle) = context.get_slot::<CspHandle>() else {
        return;
    };
    let policy = handle.policy.borrow();
    // A later submission replaces the earlier pending navigation before any wire IO.
    state.borrow_mut().borrow_mut::<Queue>().0 = Some(Pending {
        plan,
        source,
        policy: policy.active.clone(),
        referrer_policy: policy.referrer_policy,
    });
    result.set_bool(true);
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
            let mut context = RequestContext::navigation(Some(pending.source.clone()), false);
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
            )
            .await?;
            let mut logs: Vec<serde_json::Value> =
                serde_json::from_str(&self.drain_network_log()).unwrap_or_default();
            logs.push(serde_json::json!({"kind":"navigation","initiator":"form","url":pending.plan.url,"final_url":response.final_url,"status":response.status,"method":pending.plan.method,"response_received":true}));
            {
                let state = next.rt.op_state();
                let mut state = state.borrow_mut();
                let network = state.borrow_mut::<NetworkLog>();
                logs.extend(network.0.drain(..));
                network.0 = logs.into();
            }
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
