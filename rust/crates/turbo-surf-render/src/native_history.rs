//! Actor-owned history. No guest-supplied target or logical-group token is authority.
use super::session_history::{Change, Draft, History, Limits};
use super::*;
use deno_error::JsErrorClass;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use turbo_dom_browser_env::host_internals::{self as binding, AuxiliaryContextId};

#[path = "native_history_document.rs"]
mod documents;
pub(super) use documents::identity as physical_document_identity;

#[path = "native_history_transaction.rs"]
mod transactions;

#[path = "native_history_ops.rs"]
mod ops;
pub(super) use ops::{
    op_history_ack, op_history_notify, op_history_scroll_restoration, op_history_select,
    op_history_status,
};

type Ledger = History<Rc<crate::crypto_clone::CloneSnapshot>>;
#[derive(Default)]
struct Histories {
    targets: BTreeMap<u64, Ledger>,
    transactions: transactions::Transactions,
    origins: BTreeMap<u64, String>,
    next_group: u64,
    initial_empty: BTreeSet<u64>,
    next_task: u64,
    tasks: VecDeque<HistoryTask>,
    waiters: BTreeSet<u64>,
    completed: BTreeMap<u64, Result<Option<u32>, String>>,
}
// Short-lived active-document tasks are canceled on retirement; stored history
// state itself contains only owned CloneSnapshot bytes, never these JS handles.
struct HistoryTask {
    id: u64,
    binding: Binding,
    callback: v8::Global<v8::Function>,
}

#[derive(Clone, Copy)]
struct Binding {
    target: u64,
    document: u64,
    group: u64,
}
struct Restoring(std::cell::Cell<bool>);
pub(super) struct InitialEmptyDocument;
struct Owner<'s> {
    context: v8::Local<'s, v8::Context>,
    binding: Binding,
}

fn store(state: &mut OpState) -> &mut Histories {
    if !state.has::<Histories>() {
        state.put(Histories::default());
    }
    state.borrow_mut::<Histories>()
}
fn failure(message: impl Into<std::borrow::Cow<'static, str>>) -> JsErrorBox {
    JsErrorBox::generic(message)
}
fn limits(state: &OpState) -> Limits {
    let policy = state.borrow::<auxiliary_pages::Pages>().policy;
    Limits {
        entries: policy.max_history_entries,
        url_bytes: policy.max_history_url_bytes,
        state_bytes: policy.max_history_state_bytes,
    }
}
fn actual<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
) -> Result<Option<(v8::Local<'s, v8::Context>, u64, u64)>, JsErrorBox> {
    let context = document
        .get_creation_context(scope)
        .ok_or_else(|| failure("Native history document has no realm"))?;
    if context.get_slot::<FrameRetired>().is_some() {
        return Err(failure("Native history document is retired"));
    }
    let active = context
        .get_slot::<ActiveDocument>()
        .and_then(|active| active.0.to_local(scope));
    if active != Some(document) {
        return Err(failure("Native history document ownership mismatch"));
    }
    if context.get_slot::<FrameParent>().is_some()
        || !JsRuntime::op_state_from(scope)
            .borrow()
            .borrow::<auxiliary_pages::Pages>()
            .enabled
    {
        return Ok(None);
    }
    let Some(identity) = binding::auxiliary_context_identity(context) else {
        return Ok(None);
    };
    if identity.closed {
        return Err(failure("Native history target is closed"));
    }
    let selected = auxiliary_pages::context(scope, identity.id).map_err(failure)?;
    if selected != context {
        return Err(failure(
            "Native history target no longer owns this document",
        ));
    }
    Ok(Some((
        context,
        identity.id.0,
        physical_document_identity(scope, context, document)?,
    )))
}
pub(super) fn document_id_for_script<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
) -> Result<String, JsErrorBox> {
    match actual(scope, document)? {
        Some((_, _, identity)) => Ok(identity.to_string()),
        None => Ok(binding::document_id_of(scope, document).to_string()),
    }
}

fn document_identity_for_commit<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    document: v8::Local<'s, v8::Object>,
) -> Result<u64, String> {
    physical_document_identity(scope, context, document).map_err(|error| error.to_string())
}

fn owner<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
) -> Result<Option<Owner<'s>>, JsErrorBox> {
    let Some((context, target, physical)) = actual(scope, document)? else {
        return Ok(None);
    };
    if let Some(bound) = context.get_slot::<Binding>() {
        if bound.target != target || bound.document != physical {
            return Err(failure("Native history binding ownership mismatch"));
        }
        return Ok(Some(Owner {
            context,
            binding: *bound,
        }));
    }
    let url = window_proxy::location_url(context)
        .ok_or_else(|| failure("Native history Location unavailable"))?;
    let origin = window_origin(scope, context);
    let shared = JsRuntime::op_state_from(scope);
    let mut state = shared.borrow_mut();
    let policy = limits(&state);
    let histories = store(&mut state);
    if histories
        .targets
        .get(&target)
        .and_then(Ledger::current)
        .is_some_and(|entry| entry.group.is_some())
    {
        return Err(failure(
            "Native history backing was not authenticated before bootstrap",
        ));
    }
    let group = histories
        .next_group
        .checked_add(1)
        .ok_or_else(|| failure("Native history group identity exhausted"))?;
    if let Some(history) = histories.targets.get_mut(&target) {
        if history.current().is_none() {
            history
                .placeholder(url, Some(group), Change::Push, policy)
                .map_err(failure)?;
        }
    } else {
        let mut history = Ledger::new(target);
        history
            .placeholder(url, Some(group), Change::Push, policy)
            .map_err(failure)?;
        histories.targets.insert(target, history);
    }
    histories.next_group = group;
    histories.origins.insert(group, origin);
    if context.get_slot::<InitialEmptyDocument>().is_some() {
        histories.initial_empty.insert(target);
    }
    let bound = Binding {
        target,
        document: physical,
        group,
    };
    context.set_slot(Rc::new(bound));
    context.set_slot(Rc::new(Restoring(std::cell::Cell::new(false))));
    Ok(Some(Owner {
        context,
        binding: bound,
    }))
}
fn same_owner(before: Binding, after: Binding) -> Result<(), JsErrorBox> {
    if before.target != after.target
        || before.document != after.document
        || before.group != after.group
    {
        return Err(failure(
            "Native history ownership changed during state preparation",
        ));
    }
    Ok(())
}
fn own<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    if object.is_proxy() {
        return Err(failure(
            "Native history metadata must be an own data record",
        ));
    }
    let key = v8::String::new(scope, name)
        .ok_or_else(|| failure("Native history metadata allocation failed"))?;
    let descriptor = object
        .get_own_property_descriptor(scope, key.into())
        .and_then(|value| value.to_object(scope))
        .ok_or_else(|| failure("Native history metadata is incomplete"))?;
    let value = v8::String::new(scope, "value").unwrap();
    if descriptor.has_own_property(scope, value.into()) != Some(true) {
        return Err(failure(
            "Native history metadata accessors are not accepted",
        ));
    }
    descriptor
        .get(scope, value.into())
        .ok_or_else(|| failure("Native history data is unavailable"))
}
fn text<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
    max: usize,
) -> Result<String, JsErrorBox> {
    let value = own(scope, object, name)?;
    let value = v8::Local::<v8::String>::try_from(value)
        .map_err(|_| failure("Native history metadata is not a string"))?;
    if value.length() > max {
        return Err(failure("Native history metadata exceeds its byte budget"));
    }
    let value = value.to_rust_string_lossy(scope);
    if value.len() > max {
        return Err(failure("Native history metadata exceeds its byte budget"));
    }
    Ok(value)
}
fn metadata(history: &Ledger, bound: Binding) -> serde_json::Value {
    serde_json::json!({"revision":history.revision().to_string(),"entries":history.entries().iter().map(|entry| serde_json::json!({"row":entry.row.to_string(),"group":entry.group.map(|group|group.to_string()),"local":entry.local,"url":entry.url})).collect::<Vec<_>>(),"index":history.index(),"document":bound.document.to_string(),"group":bound.group.to_string(),"closed":false})
}
fn pending_metadata(
    history: &Ledger,
    bound: Binding,
    ticket: transactions::Ticket,
) -> serde_json::Value {
    let mut snapshot = metadata(history, bound);
    snapshot["pendingTransaction"] = ticket.token().into();
    snapshot
}

fn private_metadata(history: &Ledger, bound: Binding) -> serde_json::Value {
    let entries = history
        .entries()
        .iter()
        .filter(|entry| entry.group == Some(bound.group))
        .collect::<Vec<_>>();
    let selected = history.current().map(|entry| entry.row);
    let index = entries
        .iter()
        .position(|entry| Some(entry.row) == selected)
        .unwrap_or(0);
    serde_json::json!({"group":bound.group.to_string(),"document":bound.document.to_string(),"serial":history.serial(bound.group),"index":index,"length":history.entries().len(),"globalIndex":history.index(),"entries":entries.iter().map(|entry|serde_json::json!({"id":entry.local,"url":entry.url,"key":entry.key,"navigationId":entry.navigation_id,"scroll":entry.scroll})).collect::<Vec<_>>()})
}
fn rewritable(from: &str, to: &str) -> bool {
    let (Ok(from), Ok(to)) = (ModuleSpecifier::parse(from), ModuleSpecifier::parse(to)) else {
        return false;
    };
    if from.scheme() != to.scheme()
        || from.username() != to.username()
        || from.password() != to.password()
        || from.host_str() != to.host_str()
        || from.port() != to.port()
    {
        return false;
    }
    match from.scheme() {
        "http" | "https" => true,
        "file" => from.path() == to.path(),
        _ => {
            let mut from = from;
            let mut to = to;
            from.set_fragment(None);
            to.set_fragment(None);
            from == to
        }
    }
}

#[op2(reentrant)]
#[string]
pub(super) fn op_history_initialize<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    entry: v8::Local<'s, v8::Object>,
) -> Result<String, JsErrorBox> {
    let Some(owner) = owner(scope, document)? else {
        return Ok(String::new());
    };
    let key = text(scope, entry, "key", 128)?;
    let navigation_id = text(scope, entry, "id", 128)?;
    let shared = JsRuntime::op_state_from(scope);
    let mut state = shared.borrow_mut();
    let policy = limits(&state);
    let log = state.borrow::<auxiliary_pages::Pages>().history_log.clone();
    let history = store(&mut state)
        .targets
        .get_mut(&owner.binding.target)
        .ok_or_else(|| failure("Native target history disappeared"))?;
    if history.current().is_some_and(|entry| entry.local == 0) {
        let url = history.current().unwrap().url.clone();
        let local = history
            .serial(owner.binding.group)
            .checked_add(1)
            .ok_or_else(|| failure("Native history local identity exhausted"))?;
        let plan = history
            .prepare(
                Draft {
                    group: Some(owner.binding.group),
                    local,
                    url,
                    key,
                    navigation_id,
                    scroll: "auto".into(),
                    state: None,
                    bytes: 0,
                },
                Change::Bind,
                policy,
            )
            .map_err(failure)?;
        log.publish_with(&owner.binding.target.to_string(), || {
            history.commit(plan)?;
            Ok(metadata(history, owner.binding))
        })
        .map_err(failure)?;
    } else {
        log.update(
            &owner.binding.target.to_string(),
            metadata(history, owner.binding),
        )
        .map_err(failure)?;
    }
    Ok(private_metadata(history, owner.binding).to_string())
}

struct Preparation {
    revision: u64,
    available: usize,
    local: u32,
    limits: Limits,
}

fn preparation(state: &OpState, bound: Binding, change: Change) -> Result<Preparation, JsErrorBox> {
    let policy = limits(state);
    let histories = state.borrow::<Histories>();
    let history = histories
        .targets
        .get(&bound.target)
        .ok_or_else(|| failure("Native history target is unavailable"))?;
    let retained = histories
        .targets
        .iter()
        .try_fold(0usize, |bytes, (target, history)| {
            bytes.checked_add(if *target == bound.target {
                history.retained_bytes(change)
            } else {
                history.used_bytes()
            })
        })
        .ok_or_else(|| failure("Native history state budget overflow"))?;
    let available = policy
        .state_bytes
        .checked_sub(retained)
        .ok_or_else(|| failure("Native history state budget exceeded"))?;
    let local = if matches!(change, Change::Update) {
        history
            .current()
            .ok_or_else(|| failure("Native history current entry unavailable"))?
            .local
    } else {
        history
            .serial(bound.group)
            .checked_add(1)
            .ok_or_else(|| failure("Native history local identity exhausted"))?
    };
    Ok(Preparation {
        revision: history.revision(),
        available,
        local,
        limits: policy,
    })
}

fn cloned_draft<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    before: &Owner<'s>,
    entry: v8::Local<'s, v8::Object>,
    prep: &Preparation,
) -> Result<Draft<Rc<crate::crypto_clone::CloneSnapshot>>, JsErrorBox> {
    let url = text(scope, entry, "url", prep.limits.url_bytes)?;
    let key = text(scope, entry, "key", 128)?;
    let navigation_id = text(scope, entry, "id", 128)?;
    let scroll = text(scope, entry, "scroll", 6)?;
    let payload = own(scope, entry, "payload")?;
    let snapshot = {
        let scope = &mut v8::ContextScope::new(scope, before.context);
        Rc::new(crate::crypto_clone::serialize_history_state(
            scope,
            payload,
            prep.available,
            "",
        )?)
    };
    let bytes = snapshot.size_bytes();
    Ok(Draft {
        group: Some(before.binding.group),
        local: prep.local,
        url,
        key,
        navigation_id,
        scroll,
        state: Some(snapshot),
        bytes,
    })
}

fn mutation_response<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    bound: Binding,
    local: u32,
) -> Result<(transactions::Ticket, v8::Local<'s, v8::Value>), JsErrorBox> {
    let shared = JsRuntime::op_state_from(scope);
    let ticket = store(&mut shared.borrow_mut())
        .transactions
        .begin(transactions::Identity {
            target: bound.target,
            document: bound.document,
        })
        .map_err(failure)?;
    let response = (|| {
        let record = v8::Object::new(scope);
        let null = v8::null(scope);
        record.set_prototype(scope, null.into());
        let local = v8::Integer::new_from_unsigned(scope, local);
        let token = v8::String::new(scope, &ticket.token())
            .ok_or_else(|| failure("Native history transaction allocation failed"))?;
        let fields: [(&str, v8::Local<v8::Value>); 2] =
            [("local", local.into()), ("transaction", token.into())];
        for (key, value) in fields {
            let key = v8::String::new(scope, key).unwrap();
            if record.create_data_property(scope, key.into(), value) != Some(true) {
                return Err(failure("Native history transaction allocation failed"));
            }
        }
        result_record(scope, Ok(record.into()))
    })();
    if response.is_err() {
        let _ = store(&mut shared.borrow_mut()).transactions.abort(ticket);
    }
    response.map(|response| (ticket, response))
}

fn mutation_failed(
    state: &mut OpState,
    ticket: transactions::Ticket,
    result: Result<(), String>,
) -> Result<(), JsErrorBox> {
    if result.is_err() {
        let _ = store(state).transactions.abort(ticket);
    }
    result.map_err(failure)
}

fn commit_prepared<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    before: Binding,
    prep: Preparation,
    draft: Draft<Rc<crate::crypto_clone::CloneSnapshot>>,
    change: Change,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let after = owner(scope, document)?
        .ok_or_else(|| failure("Native history target retired during state serialization"))?;
    same_owner(before, after.binding)?;
    if scope.is_execution_terminating() {
        return Err(failure("Native history state preparation was interrupted"));
    }
    let current_url = window_proxy::location_url(after.context)
        .ok_or_else(|| failure("Native history Location unavailable"))?;
    if !rewritable(&current_url, &draft.url) {
        return Err(JsErrorBox::new(
            "DOMExceptionSecurityError",
            "History URL cannot be rewritten by this document",
        ));
    }
    let shared = JsRuntime::op_state_from(scope);
    let url = draft.url.clone();
    let local = draft.local;
    let (log, plan) = {
        let mut state = shared.borrow_mut();
        let current = preparation(&state, after.binding, change)?;
        if current.revision != prep.revision {
            return Err(failure("Native history changed during state serialization"));
        }
        if draft.bytes > current.available {
            return Err(JsErrorBox::new(
                "DOMExceptionDataCloneError",
                "Actor session history state storage budget exceeded",
            ));
        }
        let log = state.borrow::<auxiliary_pages::Pages>().history_log.clone();
        let history = store(&mut state)
            .targets
            .get_mut(&after.binding.target)
            .unwrap();
        (
            log,
            history
                .prepare(draft, change, prep.limits)
                .map_err(failure)?,
        )
    };
    let (ticket, success) = mutation_response(scope, after.binding, local)?;
    let mut state = shared.borrow_mut();
    let Histories {
        targets,
        transactions,
        ..
    } = store(&mut state);
    let history = targets.get_mut(&after.binding.target).unwrap();
    let result = log
        .publish_with(&after.binding.target.to_string(), || {
            history.commit(plan)?;
            transactions.committed(ticket)?;
            let scope = &mut v8::ContextScope::new(scope, after.context);
            window_proxy::set_location_url(scope, &url);
            after.context.set_slot(Rc::new(FrameDocumentLocation(url)));
            Ok(pending_metadata(history, after.binding, ticket))
        })
        .map(|_| ());
    mutation_failed(&mut state, ticket, result)?;
    prune_origins(store(&mut state));
    drop(state);
    interrupt_committed_for_testing(scope, after.binding);
    Ok(success)
}

fn normalize_change(state: &OpState, target: u64, change: Change) -> Change {
    if matches!(change, Change::Push) && state.borrow::<Histories>().initial_empty.contains(&target)
    {
        Change::Replace
    } else {
        change
    }
}

fn precommit<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    kind: &str,
    entry: v8::Local<'s, v8::Object>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let Some(before) = owner(scope, document)? else {
        let value = v8::Integer::new(scope, 0);
        return result_record(scope, Ok(value.into()));
    };
    let change = match kind {
        "push" => Change::Push,
        "replace" => Change::Replace,
        "update" => Change::Update,
        _ => return Err(failure("Unsupported native history mutation")),
    };
    let shared = JsRuntime::op_state_from(scope);
    let change = normalize_change(&shared.borrow(), before.binding.target, change);
    let prep = preparation(&shared.borrow(), before.binding, change)?;
    let draft = cloned_draft(scope, &before, entry, &prep)?;
    commit_prepared(scope, document, before.binding, prep, draft, change)
}

fn result_record<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    result: Result<v8::Local<'s, v8::Value>, JsErrorBox>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let record = v8::Object::new(scope);
    let null = v8::null(scope);
    record.set_prototype(scope, null.into());
    let ok = v8::String::new(scope, "ok").unwrap();
    let flag = v8::Boolean::new(scope, result.is_ok());
    record.create_data_property(scope, ok.into(), flag.into());
    match result {
        Ok(value) => {
            let key = v8::String::new(scope, "value").unwrap();
            record.create_data_property(scope, key.into(), value);
        }
        Err(error) => {
            let class = error.get_class();
            let name = class.strip_prefix("DOMException").unwrap_or(&class);
            let message = error.get_message();
            for (key, value) in [("name", name), ("message", message.as_ref())] {
                let key = v8::String::new(scope, key).unwrap();
                let value = v8::String::new(scope, value)
                    .ok_or_else(|| failure("Native history error allocation failed"))?;
                record.create_data_property(scope, key.into(), value.into());
            }
        }
    }
    Ok(record.into())
}

#[op2(reentrant)]
pub(super) fn op_history_precommit<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    #[string] kind: &str,
    entry: v8::Local<'s, v8::Object>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let context = document
        .get_creation_context(scope)
        .ok_or_else(|| failure("Native history document realm unavailable"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope, scope);
    let result = precommit(scope, document, kind, entry);
    if scope.has_caught() || scope.has_terminated() {
        scope.rethrow();
        return Ok(v8::undefined(scope).into());
    }
    match result {
        Ok(record) => Ok(record),
        Err(error) => result_record(scope, Err(error)),
    }
}

fn storage_clone<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    value: v8::Local<'s, v8::Value>,
    kind: &str,
    operation: &str,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let prefix = match operation {
        "pushState" => "Failed to execute 'pushState' on 'History': ",
        "replaceState" => "Failed to execute 'replaceState' on 'History': ",
        _ => return Err(failure("Unsupported native History method")),
    };
    let before = owner(scope, document)?;
    let change = if kind == "replace" {
        Change::Replace
    } else {
        Change::Push
    };
    let limit = limits(&JsRuntime::op_state_from(scope).borrow()).state_bytes;
    let snapshot = crate::crypto_clone::serialize_history_state(scope, value, limit, prefix)?;
    let value = crate::crypto_clone::deserialize(scope, &snapshot)?;
    if let Some(before) = before {
        let after = owner(scope, document)?
            .ok_or_else(|| failure("Native history document retired during state cloning"))?;
        same_owner(before.binding, after.binding)?;
        let shared = JsRuntime::op_state_from(scope);
        let change = normalize_change(&shared.borrow(), after.binding.target, change);
        let fresh = preparation(&shared.borrow(), after.binding, change)?;
        if snapshot.size_bytes() > fresh.available {
            return Err(JsErrorBox::new(
                "DOMExceptionDataCloneError",
                format!("{prefix}Actor session history state storage budget exceeded"),
            ));
        }
    }
    Ok(value)
}

#[op2(reentrant)]
pub(super) fn op_history_clone<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    value: v8::Local<'s, v8::Value>,
    #[string] kind: &str,
    #[string] operation: &str,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let context = document
        .get_creation_context(scope)
        .ok_or_else(|| failure("Native history clone document realm unavailable"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope, scope);
    let result = storage_clone(scope, document, value, kind, operation);
    if scope.has_caught() || scope.has_terminated() {
        scope.rethrow();
        return Ok(v8::undefined(scope).into());
    }
    result_record(scope, result)
}

/// Explicit raw driver URLs are not invented same-document groups or state.
pub(super) fn initialize_driver_history(
    state: &mut OpState,
    target: u64,
    url: &str,
    history: Option<AuxiliaryDriverHistory>,
) -> Result<(), String> {
    let Some(history) = history else {
        return Ok(());
    };
    let policy = limits(state);
    if history.entries.is_empty()
        || history.index >= history.entries.len()
        || history.entries[history.index] != url
    {
        return Err("Driver history does not identify its actual initial document".into());
    }
    let mut ledger = Ledger::new(target);
    for url in history.entries {
        ModuleSpecifier::parse(&url).map_err(|_| "Driver history contains an invalid URL")?;
        ledger.placeholder(url, None, Change::Push, policy)?;
    }
    let selected = ledger.entries()[history.index].row;
    ledger.select(selected)?;
    let histories = store(state);
    if histories.targets.contains_key(&target) {
        return Err("Native target history is already initialized".into());
    }
    histories.targets.insert(target, ledger);
    Ok(())
}

pub(super) fn activate(
    scope: &mut v8::PinScope,
    context: v8::Local<v8::Context>,
) -> Result<(), String> {
    if context.get_slot::<FrameParent>().is_some()
        || !JsRuntime::op_state_from(scope)
            .borrow()
            .borrow::<auxiliary_pages::Pages>()
            .enabled
    {
        return Ok(());
    }
    let scope = &mut v8::ContextScope::new(scope, context);
    let value = call_frame_helper(scope, "historyActivate", &[])
        .ok_or("Native history activation capability unavailable")?;
    if !value.is_true() {
        return Err("Owned native history activation was refused".into());
    }
    Ok(())
}

pub(super) fn materialize(
    scope: &mut v8::PinScope,
    context: v8::Local<v8::Context>,
) -> Result<(), String> {
    let Some(bound) = context.get_slot::<Binding>().map(|bound| *bound) else {
        return Ok(());
    };
    let Some(restoring) = context.get_slot::<Restoring>() else {
        return Err("Native history restore ownership unavailable".into());
    };
    if !restoring.0.get() {
        return Ok(());
    }
    let (revision, snapshots) = {
        let shared = JsRuntime::op_state_from(scope);
        let state = shared.borrow();
        let history = state
            .borrow::<Histories>()
            .targets
            .get(&bound.target)
            .ok_or("Native history restore target unavailable")?;
        (
            history.revision(),
            history
                .entries()
                .iter()
                .filter(|entry| entry.group == Some(bound.group))
                .map(|entry| (entry.local, entry.state.clone()))
                .collect::<Vec<_>>(),
        )
    };
    // No OpState borrow or metadata lock spans destination class factories.
    let scope = &mut v8::ContextScope::new(scope, context);
    let values = v8::Array::new(scope, snapshots.len() as i32);
    let null = v8::null(scope);
    values.set_prototype(scope, null.into());
    for (at, (local, snapshot)) in snapshots.iter().enumerate() {
        let payload = if let Some(snapshot) = snapshot {
            crate::crypto_clone::deserialize(scope, snapshot).map_err(|error| error.to_string())?
        } else {
            v8::null(scope).into()
        };
        let record = v8::Object::new(scope);
        let null = v8::null(scope);
        record.set_prototype(scope, null.into());
        let key = v8::String::new(scope, "local").unwrap();
        let local = v8::Integer::new_from_unsigned(scope, *local);
        record.create_data_property(scope, key.into(), local.into());
        let key = v8::String::new(scope, "payload").unwrap();
        record.create_data_property(scope, key.into(), payload);
        let key = v8::String::new(scope, &at.to_string())
            .ok_or("Native history restore index allocation failed")?;
        values.create_data_property(scope, key.into(), record.into());
    }
    validate_restore(scope, context, bound, revision)?;
    let result = call_frame_helper(scope, "restoreHistoryState", &[values.into()])
        .ok_or("Native history state restore failed")?;
    if !result.is_true() {
        return Err("Native history state restore was refused".into());
    }
    restoring.0.set(false);
    Ok(())
}

fn validate_restore(
    scope: &mut v8::PinScope,
    context: v8::Local<v8::Context>,
    bound: Binding,
    revision: u64,
) -> Result<(), String> {
    if scope.is_execution_terminating() || context.get_slot::<FrameRetired>().is_some() {
        return Err("Native history restore owner was retired".into());
    }
    let document = context
        .get_slot::<ActiveDocument>()
        .and_then(|active| active.0.to_local(scope))
        .ok_or("Native history restore document disappeared")?;
    let current = actual(scope, document)
        .map_err(|error| error.to_string())?
        .ok_or("Native history restore owner disappeared")?;
    if current.0 != context || current.1 != bound.target || current.2 != bound.document {
        return Err("Native history restore owner changed".into());
    }
    let shared = JsRuntime::op_state_from(scope);
    let state = shared.borrow();
    let history = state
        .borrow::<Histories>()
        .targets
        .get(&bound.target)
        .ok_or("Native history restore ledger disappeared")?;
    if history.revision() != revision
        || history.current().and_then(|entry| entry.group) != Some(bound.group)
    {
        return Err("Native history changed during state materialization".into());
    }
    Ok(())
}

fn prune_origins(histories: &mut Histories) {
    histories.origins.retain(|group, _| {
        histories.targets.values().any(|history| {
            history
                .entries()
                .iter()
                .any(|entry| entry.group == Some(*group))
        })
    });
}

pub(super) fn close(state: &mut OpState, target: u64) {
    if let Some(histories) = state.try_borrow_mut::<Histories>() {
        histories.targets.remove(&target);
        histories.initial_empty.remove(&target);
        let canceled = histories
            .tasks
            .iter()
            .filter(|task| task.binding.target == target)
            .map(|task| task.id)
            .collect::<Vec<_>>();
        histories.tasks.retain(|task| task.binding.target != target);
        for id in canceled {
            histories.waiters.remove(&id);
            histories.completed.remove(&id);
        }
        prune_origins(histories);
    }
    let _ = state
        .borrow::<auxiliary_pages::Pages>()
        .history_log
        .close(&target.to_string());
}

pub(super) fn resolve_mode(
    state: &OpState,
    target: u64,
    mode: &str,
    reload: bool,
) -> Result<auxiliary_navigation::HistoryMode, String> {
    use auxiliary_navigation::HistoryMode;
    if reload {
        return Ok(HistoryMode::Reload);
    }
    match mode {
        "push" => Ok(HistoryMode::Push),
        "replace" => Ok(HistoryMode::Replace),
        "back" | "forward" => {
            let history = state
                .try_borrow::<Histories>()
                .and_then(|histories| histories.targets.get(&target))
                .ok_or("Native target history is unavailable")?;
            let entry = history
                .adjacent(if mode == "back" { -1 } else { 1 })
                .ok_or("Native target history traversal is out of range")?;
            Ok(HistoryMode::Traverse(entry.row))
        }
        _ => Err(format!("Unsupported native history operation: {mode}")),
    }
}

pub(super) fn traversal_url(state: &OpState, target: u64, row: u64) -> Result<String, String> {
    let history = state
        .try_borrow::<Histories>()
        .and_then(|histories| histories.targets.get(&target))
        .ok_or("Native target history is unavailable")?;
    let at = history
        .row_index(row)
        .ok_or("Native traversal entry is no longer present")?;
    Ok(history.entries()[at].url.clone())
}

pub(super) fn admit_navigation(
    state: &OpState,
    target: u64,
    mode: auxiliary_navigation::HistoryMode,
    url: &str,
) -> Result<auxiliary_navigation::HistoryMode, String> {
    use auxiliary_navigation::HistoryMode;
    let policy = limits(state);
    if url.len() > policy.url_bytes {
        return Err("Native session history URL budget exceeded".into());
    }
    let history = state
        .try_borrow::<Histories>()
        .and_then(|histories| histories.targets.get(&target))
        .ok_or("Native target history has not been activated")?;
    history
        .current()
        .ok_or("Native target history has no active entry")?;
    let initial_empty = state.borrow::<Histories>().initial_empty.contains(&target);
    let mode = if initial_empty && matches!(mode, HistoryMode::Push) {
        HistoryMode::Replace
    } else {
        mode
    };
    if policy.entries == 0
        || (matches!(mode, HistoryMode::Push) && history.index() + 1 >= policy.entries)
    {
        return Err("Native target session history limit exceeded".into());
    }
    if let HistoryMode::Traverse(row) = mode {
        traversal_url(state, target, row)?;
    }
    Ok(mode)
}

fn restored_group(
    histories: &Histories,
    target: u64,
    url: &str,
    origin: &str,
    mode: auxiliary_navigation::HistoryMode,
) -> Result<Option<(u64, u64, String)>, String> {
    use auxiliary_navigation::HistoryMode;
    let Some(history) = histories.targets.get(&target) else {
        return Err("Native target history is unavailable".into());
    };
    let selected = match mode {
        HistoryMode::Traverse(row) => history
            .row_index(row)
            .and_then(|at| history.entries().get(at))
            .ok_or("Native history traversal target disappeared")?,
        HistoryMode::Reload => history
            .current()
            .ok_or("Native reload history entry is unavailable")?,
        _ => return Ok(None),
    };
    let Some(group) = selected.group else {
        return Ok(None);
    };
    let same_resource = match (
        ModuleSpecifier::parse(&selected.url),
        ModuleSpecifier::parse(url),
    ) {
        (Ok(mut before), Ok(mut after)) => {
            before.set_fragment(None);
            after.set_fragment(None);
            before == after
        }
        _ => false,
    };
    // An origin-changing redirect is a fresh document, never a state-clone capability.
    if !same_resource
        || histories
            .origins
            .get(&group)
            .is_none_or(|stored| stored != origin)
    {
        return Ok(None);
    }
    Ok(Some((group, selected.row, selected.url.clone())))
}

fn committed_document<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) -> Result<(AuxiliaryContextId, u64), String> {
    let identity = binding::auxiliary_context_identity(context)
        .ok_or("Committed native history target has no identity")?;
    let document = context
        .get_slot::<ActiveDocument>()
        .and_then(|active| active.0.to_local(scope))
        .ok_or("Committed history document is unavailable")?;
    let selected = auxiliary_pages::context(scope, identity.id)?;
    if selected != context || identity.closed {
        return Err("Committed native history document has no active owner".into());
    }
    let physical = document_identity_for_commit(scope, context, document)?;
    Ok((identity.id, physical))
}

pub(super) fn commit_navigation<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    url: &str,
    mode: auxiliary_navigation::HistoryMode,
) -> Result<String, String> {
    use auxiliary_navigation::HistoryMode;
    let (target, physical) = committed_document(scope, context)?;
    let origin = window_origin(scope, context);
    let shared = JsRuntime::op_state_from(scope);
    let mut state = shared.borrow_mut();
    let policy = limits(&state);
    let histories = store(&mut state);
    let restored = restored_group(histories, target.0, url, &origin, mode)?;
    let (group, active_url, restoring) = if let Some((group, row, url)) = restored {
        histories.targets.get_mut(&target.0).unwrap().select(row)?;
        (group, url, true)
    } else {
        let group = histories
            .next_group
            .checked_add(1)
            .ok_or("Native history group identity exhausted")?;
        let history = histories
            .targets
            .get_mut(&target.0)
            .ok_or("Native target history disappeared")?;
        if let HistoryMode::Traverse(row) = mode {
            history.select(row)?;
        }
        let change = if matches!(mode, HistoryMode::Push) {
            Change::Push
        } else {
            Change::Bind
        };
        history.placeholder(url.into(), Some(group), change, policy)?;
        histories.next_group = group;
        histories.origins.insert(group, origin);
        (group, url.to_owned(), false)
    };
    histories.initial_empty.remove(&target.0);
    prune_origins(histories);
    context.set_slot(Rc::new(Binding {
        target: target.0,
        document: physical,
        group,
    }));
    context.set_slot(Rc::new(Restoring(std::cell::Cell::new(restoring))));
    context.set_slot(Rc::new(FrameDocumentLocation(active_url.clone())));
    Ok(active_url)
}

fn queue_task<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    callback: v8::Local<'s, v8::Function>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let before = owner(scope, document)?
        .ok_or_else(|| failure("Native history task has no registered owner"))?;
    if callback.get_creation_context(scope) != Some(before.context) || callback.is_proxy() {
        return Err(failure("Native history task is not owned by its document"));
    }
    let shared = JsRuntime::op_state_from(scope);
    let mut state = shared.borrow_mut();
    let limit = state
        .borrow::<auxiliary_pages::Pages>()
        .policy
        .max_pending_navigations;
    let histories = store(&mut state);
    if histories.tasks.len() >= limit {
        return Err(failure("Native history task budget exceeded"));
    }
    let id = histories
        .next_task
        .checked_add(1)
        .ok_or_else(|| failure("Native history task identity exhausted"))?;
    let value = v8::String::new(scope, &id.to_string())
        .ok_or_else(|| failure("Native history task allocation failed"))?;
    histories.next_task = id;
    histories.tasks.push_back(HistoryTask {
        id,
        binding: before.binding,
        callback: v8::Global::new(scope, callback),
    });
    driver_sync::changed(&mut state);
    Ok(value.into())
}

#[op2(reentrant)]
pub(super) fn op_history_queue_task<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    callback: v8::Local<'s, v8::Function>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let context = document
        .get_creation_context(scope)
        .ok_or_else(|| failure("Native history task document realm unavailable"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope,scope);
    let result = queue_task(scope, document, callback);
    if scope.has_caught() || scope.has_terminated() {
        scope.rethrow();
        return Ok(v8::undefined(scope).into());
    }
    result_record(scope, result)
}

pub(super) fn has_tasks(state: &OpState) -> bool {
    state
        .try_borrow::<Histories>()
        .is_some_and(|histories| !histories.tasks.is_empty())
}

fn execute_task(rt: &mut JsRuntime, task: &HistoryTask) -> Result<Option<u32>, String> {
    deno_core::scope!(scope, rt);
    let context = auxiliary_pages::context(scope, AuxiliaryContextId(task.binding.target))?;
    let Some(bound) = context.get_slot::<Binding>() else {
        return Ok(None);
    };
    if bound.document != task.binding.document
        || bound.group != task.binding.group
        || context.get_slot::<FrameRetired>().is_some()
    {
        return Ok(None);
    }
    let callback = v8::Local::new(scope, &task.callback);
    if callback.get_creation_context(scope) != Some(context) {
        return Ok(None);
    }
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope,scope);
    let receiver = v8::undefined(scope);
    let value = callback.call(scope, receiver.into(), &[]).ok_or_else(|| {
        scope
            .exception()
            .map(|error| error.to_rust_string_lossy(scope))
            .unwrap_or_else(|| "Native history traversal task was interrupted".into())
    })?;
    if scope.has_terminated() || scope.is_execution_terminating() {
        return Err("Native history traversal task was interrupted".into());
    }
    Ok(value
        .is_uint32()
        .then(|| value.uint32_value(scope))
        .flatten()
        .filter(|id| *id > 0))
}

pub(super) fn run_task_for(rt: &mut JsRuntime, target: Option<u64>) -> Result<bool, String> {
    let task = {
        let shared = rt.op_state();
        let mut state = shared.borrow_mut();
        let Some(histories) = state.try_borrow_mut::<Histories>() else {
            return Ok(false);
        };
        let Some(at) = histories
            .tasks
            .iter()
            .position(|task| target.is_none_or(|target| task.binding.target == target))
        else {
            return Ok(false);
        };
        histories.tasks.remove(at).unwrap()
    };
    let result = execute_task(rt, &task);
    let shared = rt.op_state();
    let mut state = shared.borrow_mut();
    let histories = store(&mut state);
    if histories.waiters.contains(&task.id) {
        histories.completed.insert(task.id, result);
    } else {
        result?;
    }
    // A queued task of a now-retired document is canceled, never run in its replacement.
    Ok(true)
}

/// Called only after the owning command has stopped its watchdog and cleared termination.
pub(super) fn finish_command<T>(
    rt: &mut JsRuntime,
    result: Result<T, String>,
) -> Result<T, String> {
    let affected = {
        let shared = rt.op_state();
        let mut state = shared.borrow_mut();
        state
            .try_borrow_mut::<Histories>()
            .map(|histories| histories.transactions.take_unacknowledged())
            .unwrap_or_default()
    };
    if affected.is_empty() {
        return result;
    }
    let mut retired = false;
    let mut retirement_failed = false;
    deno_core::scope!(scope, rt);
    for identity in affected {
        let Ok(context) = auxiliary_pages::context(scope, AuxiliaryContextId(identity.target))
        else {
            continue;
        };
        let scope = &mut v8::ContextScope::new(scope, context);
        let document = context
            .get_slot::<ActiveDocument>()
            .and_then(|active| active.0.to_local(scope));
        let Some(document) = document else {
            continue;
        };
        let current =
            physical_document_identity(scope, context, document).map_err(|error| error.to_string());
        if current != Ok(identity.document) {
            continue;
        }
        if auxiliary_pages::close(scope, context) {
            retired = true;
        } else {
            retirement_failed = true;
        }
    }
    if retirement_failed {
        result.and(Err(
            "Native history mutation retirement failed for its actual owner".into(),
        ))
    } else if retired {
        result.and(Err("Native history mutation was interrupted before private acknowledgement; affected target retired".into()))
    } else {
        result.and(Err(
            "Native history mutation was not acknowledged; current documents were not retired"
                .into(),
        ))
    }
}

struct InterruptHistoryCommitForTesting(Option<transactions::Identity>);

fn interrupt_committed_for_testing(scope: &mut v8::PinScope, bound: Binding) {
    let interrupt = {
        let shared = JsRuntime::op_state_from(scope);
        let mut state = shared.borrow_mut();
        state
            .try_borrow_mut::<InterruptHistoryCommitForTesting>()
            .is_some_and(|fault| {
                if fault.0
                    == Some(transactions::Identity {
                        target: bound.target,
                        document: bound.document,
                    })
                {
                    fault.0 = None;
                    true
                } else {
                    false
                }
            })
    };
    if interrupt {
        scope.terminate_execution();
    }
}

impl PageSession {
    pub(super) fn finish_history_command<T>(
        &mut self,
        result: Result<T, String>,
    ) -> Result<T, String> {
        let _activation = self.rt.activation_key().enter();
        finish_command(&mut self.rt, result)
    }

    /// Finalize only real unacknowledged history mutations after an owned command stopped.
    /// Callers retaining a partially opened actor must keep its surviving peer Pages.
    pub fn reconcile_interrupted_history(&mut self) -> Result<(), String> {
        self.finish_history_command(Ok(()))
    }

    #[doc(hidden)]
    pub fn interrupt_next_history_commit_for_testing(&mut self, target: u64) -> Result<(), String> {
        let _activation = self.rt.activation_key().enter();
        deno_core::scope!(scope, self.rt);
        let context = auxiliary_pages::context(scope, AuxiliaryContextId(target))?;
        let bound = context
            .get_slot::<Binding>()
            .ok_or("Native history test target has no document binding")?;
        JsRuntime::op_state_from(scope)
            .borrow_mut()
            .put(InterruptHistoryCommitForTesting(Some(
                transactions::Identity {
                    target: bound.target,
                    document: bound.document,
                },
            )));
        Ok(())
    }
}

pub(super) fn run_task(rt: &mut JsRuntime) -> Result<bool, String> {
    run_task_for(rt, None)
}

impl PageSession {
    pub(super) fn traverse_same_history_group(
        &mut self,
        target: u64,
        row: u64,
        budget: u64,
    ) -> Result<Option<String>, String> {
        let selected = {
            let state = self.rt.op_state();
            let state = state.borrow();
            let history = state
                .try_borrow::<Histories>()
                .and_then(|histories| histories.targets.get(&target))
                .ok_or("Native target history unavailable")?;
            let current = history
                .current()
                .ok_or("Native history current entry unavailable")?;
            let at = history
                .row_index(row)
                .ok_or("Native history traversal target disappeared")?;
            let selected = &history.entries()[at];
            if selected.group.is_none() || selected.group != current.group {
                return Ok(None);
            }
            selected.local
        };
        let key = self.rt.activation_key();
        let _activation = key.enter();
        let watchdog = Watchdog::start(self.rt.v8_isolate(), budget);
        let result = self.perform_history_traversal(target, row, selected);
        let exceeded = watchdog.exceeded.load(std::sync::atomic::Ordering::Relaxed);
        drop(watchdog);
        self.rt.v8_isolate().cancel_terminate_execution();
        auxiliary_navigation::retire_interrupted_frames(&mut self.rt);
        let result = if exceeded {
            Err(format!("Native history traversal timeout after {budget}ms"))
        } else {
            result.map(Some)
        };
        self.finish_history_command(result)
    }

    fn perform_history_traversal(
        &mut self,
        target: u64,
        row: u64,
        local: u32,
    ) -> Result<String, String> {
        let activity = self
            .rt
            .op_state()
            .borrow()
            .borrow::<NetworkLog>()
            .begin_page_navigation(&target.to_string());
        let token = auxiliary_pages::target_call(
            &mut self.rt,
            AuxiliaryContextId(target),
            "historyTraverse",
            &[HostArg::Number(f64::from(local))],
        )?;
        let token = {
            deno_core::scope!(scope, self.rt);
            v8::Local::new(scope, &token)
                .to_rust_string_lossy(scope)
                .parse::<u64>()
                .map_err(|_| "Native protected history traversal was refused")?
        };
        store(&mut self.rt.op_state().borrow_mut())
            .waiters
            .insert(token);
        let result = self.finish_history_task(target, token, local);
        {
            let shared = self.rt.op_state();
            let mut state = shared.borrow_mut();
            let histories = store(&mut state);
            histories.waiters.remove(&token);
            histories.completed.remove(&token);
        }
        result?;
        drop(activity);
        let shared = self.rt.op_state();
        let state = shared.borrow();
        let history = state
            .borrow::<Histories>()
            .targets
            .get(&target)
            .ok_or("Native history target closed during traversal")?;
        Ok(serde_json::json!({"sameDocument":true,"finalUrl":history.current().ok_or("Native history traversal has no active entry")?.url,"historyRow":row.to_string()}).to_string())
    }

    fn finish_history_task(&mut self, target: u64, token: u64, local: u32) -> Result<(), String> {
        loop {
            let result = store(&mut self.rt.op_state().borrow_mut())
                .completed
                .remove(&token);
            if let Some(result) = result {
                if result? == Some(local) {
                    return Ok(());
                }
                return Err(
                    "Native same-document history traversal was canceled before completion".into(),
                );
            }
            if !run_task_for(&mut self.rt, Some(target))? {
                return Err("Native protected history traversal task disappeared".into());
            }
        }
    }
}

fn request_traversal<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    delta: i32,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let Some(owner) = owner(scope, document)? else {
        return Ok(v8::Integer::new(scope, 0).into());
    };
    let destination = {
        let shared = JsRuntime::op_state_from(scope);
        let state = shared.borrow();
        let history = state
            .borrow::<Histories>()
            .targets
            .get(&owner.binding.target)
            .ok_or_else(|| failure("Native history traversal target unavailable"))?;
        let entry = if delta == 0 {
            history.current()
        } else {
            history.adjacent(delta)
        };
        entry.map(|entry| (entry.row, entry.group, entry.local, entry.url.clone()))
    };
    let Some((row, group, local, url)) = destination else {
        return Ok(v8::Integer::new(scope, 0).into());
    };
    if delta != 0 && group == Some(owner.binding.group) {
        if local == 0 {
            return Err(failure(
                "Native same-document traversal identity unavailable",
            ));
        }
        return Ok(v8::Integer::new_from_unsigned(scope, local).into());
    }
    let mode = if delta == 0 {
        auxiliary_navigation::HistoryMode::Reload
    } else {
        auxiliary_navigation::HistoryMode::Traverse(row)
    };
    auxiliary_pages::queue_history_navigation(
        scope,
        owner.context,
        AuxiliaryContextId(owner.binding.target),
        url,
        mode,
    )
    .map_err(failure)?;
    Ok(v8::Integer::new(scope, 0).into())
}

#[op2(reentrant)]
pub(super) fn op_history_traverse<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    delta: i32,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let context = document
        .get_creation_context(scope)
        .ok_or_else(|| failure("Native history traversal realm unavailable"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope,scope);
    let result = request_traversal(scope, document, delta);
    if scope.has_caught() || scope.has_terminated() {
        scope.rethrow();
        return Ok(v8::undefined(scope).into());
    }
    result_record(scope, result)
}
