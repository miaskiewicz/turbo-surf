//! Authenticated same-document selection and readonly history observations.
use super::*;

struct Selection {
    revision: u64,
    row: u64,
    url: String,
}

fn selected(state: &OpState, bound: Binding, local: u32) -> Result<Selection, JsErrorBox> {
    let history = state
        .try_borrow::<Histories>()
        .and_then(|histories| histories.targets.get(&bound.target))
        .ok_or_else(|| failure("Native target history is not initialized"))?;
    selection(history, bound.group, local)
}

fn selection(history: &Ledger, group: u64, local: u32) -> Result<Selection, JsErrorBox> {
    let current = history
        .current()
        .ok_or_else(|| failure("Native history has no current entry"))?;
    if local == 0 || current.group != Some(group) {
        return Err(failure(
            "Native history selection has no owned document group",
        ));
    }
    let entry = history
        .entries()
        .iter()
        .find(|entry| entry.group == Some(group) && entry.local == local)
        .ok_or_else(|| failure("Native history local entry is unavailable"))?;
    Ok(Selection {
        revision: history.revision(),
        row: entry.row,
        url: entry.url.clone(),
    })
}

fn bound_owner<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
) -> Result<Option<Owner<'s>>, JsErrorBox> {
    let Some((context, _, _)) = actual(scope, document)? else {
        return Ok(None);
    };
    if context.get_slot::<Binding>().is_none() {
        return Err(failure("Native target history is not initialized"));
    }
    // Reuse the full owner check without its lazy initial-binding branch.
    owner(scope, document)
}

fn number_record<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: u32,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let value = v8::Integer::new_from_unsigned(scope, value);
    result_record(scope, Ok(value.into()))
}

fn select<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    local: u32,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let Some(before) = bound_owner(scope, document)? else {
        return number_record(scope, 0);
    };
    let shared = JsRuntime::op_state_from(scope);
    let prepared = selected(&shared.borrow(), before.binding, local)?;
    let (ticket, success) = mutation_response(scope, before.binding, local)?;
    let mut state = shared.borrow_mut();
    let result = (|| {
        let fresh = selected(&state, before.binding, local).map_err(|error| error.to_string())?;
        if fresh.revision != prepared.revision || fresh.row != prepared.row {
            return Err("Native history changed during selection preparation".into());
        }
        let log = state.borrow::<auxiliary_pages::Pages>().history_log.clone();
        let Histories {
            targets,
            transactions,
            ..
        } = store(&mut state);
        let history = targets
            .get_mut(&before.binding.target)
            .ok_or("Native target history disappeared")?;
        log.publish_with(&before.binding.target.to_string(), || {
            history.select(prepared.row)?;
            transactions.committed(ticket)?;
            let scope = &mut v8::ContextScope::new(scope, before.context);
            window_proxy::set_location_url(scope, &prepared.url);
            before
                .context
                .set_slot(Rc::new(FrameDocumentLocation(prepared.url)));
            Ok(pending_metadata(history, before.binding, ticket))
        })
        .map(|_| ())
    })();
    mutation_failed(&mut state, ticket, result)?;
    drop(state);
    interrupt_committed_for_testing(scope, before.binding);
    Ok(success)
}

fn notify<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    local: u32,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let Some(before) = bound_owner(scope, document)? else {
        return number_record(scope, 0);
    };
    let success = number_record(scope, local)?;
    let after = bound_owner(scope, document)?
        .ok_or_else(|| failure("Native history notification lost its owner"))?;
    same_owner(before.binding, after.binding)?;
    let shared = JsRuntime::op_state_from(scope);
    let (events, event) = {
        let state = shared.borrow();
        let history = state
            .try_borrow::<Histories>()
            .and_then(|histories| histories.targets.get(&after.binding.target))
            .ok_or_else(|| failure("Native target history is not initialized"))?;
        let entry = history
            .current()
            .ok_or_else(|| failure("Native history has no current entry"))?;
        if local == 0 || entry.group != Some(after.binding.group) || entry.local != local {
            return Err(failure(
                "Native history notification does not identify the selected entry",
            ));
        }
        let event = auxiliary_pages::AuxiliaryPageEvent::HistoryChanged {
            target: after.binding.target.to_string(),
            document: after.binding.document.to_string(),
            group: after.binding.group.to_string(),
            revision: history.revision().to_string(),
            row: entry.row.to_string(),
            local,
            url: entry.url.clone(),
        };
        (
            state.borrow::<auxiliary_pages::Pages>().events.clone(),
            event,
        )
    };
    events.push(event);
    Ok(success)
}

fn status_value<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    length: u32,
    index: u32,
    group: u64,
    local: u32,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let record = v8::Object::new(scope);
    let null = v8::null(scope);
    record.set_prototype(scope, null.into());
    for (name, value) in [("length", length), ("index", index), ("local", local)] {
        let key = v8::String::new(scope, name)
            .ok_or_else(|| failure("Native history status allocation failed"))?;
        let value = v8::Integer::new_from_unsigned(scope, value);
        if record.create_data_property(scope, key.into(), value.into()) != Some(true) {
            return Err(failure("Native history status allocation failed"));
        }
    }
    let key = v8::String::new(scope, "group")
        .ok_or_else(|| failure("Native history status allocation failed"))?;
    let value = v8::String::new(scope, &group.to_string())
        .ok_or_else(|| failure("Native history status allocation failed"))?;
    if record.create_data_property(scope, key.into(), value.into()) != Some(true) {
        return Err(failure("Native history status allocation failed"));
    }
    result_record(scope, Ok(record.into()))
}

fn status<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let Some(owner) = bound_owner(scope, document)? else {
        let null = v8::null(scope);
        return result_record(scope, Ok(null.into()));
    };
    let shared = JsRuntime::op_state_from(scope);
    let (length, index, local) = {
        let state = shared.borrow();
        let history = state
            .try_borrow::<Histories>()
            .and_then(|histories| histories.targets.get(&owner.binding.target))
            .ok_or_else(|| failure("Native target history is not initialized"))?;
        let current = history
            .current()
            .ok_or_else(|| failure("Native history has no current entry"))?;
        if current.group != Some(owner.binding.group) || current.local == 0 {
            return Err(failure("Native history status has no owned document group"));
        }
        let count = if state
            .borrow::<Histories>()
            .initial_empty
            .contains(&owner.binding.target)
        {
            0
        } else {
            history.entries().len()
        };
        let length = u32::try_from(count)
            .map_err(|_| failure("Native history length exceeds its interface range"))?;
        let index = u32::try_from(history.index())
            .map_err(|_| failure("Native history index exceeds its interface range"))?;
        (length, index, current.local)
    };
    status_value(scope, length, index, owner.binding.group, local)
}

#[op2(reentrant)]
pub(in crate::runtime) fn op_history_select<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    local: u32,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let context = document
        .get_creation_context(scope)
        .ok_or_else(|| failure("Native history document realm unavailable"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope, scope);
    let result = select(scope, document, local);
    if scope.has_caught() || scope.has_terminated() {
        scope.rethrow();
        return Ok(v8::undefined(scope).into());
    }
    match result {
        Ok(record) => Ok(record),
        Err(error) => result_record(scope, Err(error)),
    }
}

#[op2(reentrant)]
pub(in crate::runtime) fn op_history_notify<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    local: u32,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let context = document
        .get_creation_context(scope)
        .ok_or_else(|| failure("Native history document realm unavailable"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope, scope);
    let result = notify(scope, document, local);
    if scope.has_caught() || scope.has_terminated() {
        scope.rethrow();
        return Ok(v8::undefined(scope).into());
    }
    match result {
        Ok(record) => Ok(record),
        Err(error) => result_record(scope, Err(error)),
    }
}

#[op2(reentrant)]
pub(in crate::runtime) fn op_history_status<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let context = document
        .get_creation_context(scope)
        .ok_or_else(|| failure("Native history document realm unavailable"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope, scope);
    let result = status(scope, document);
    if scope.has_caught() || scope.has_terminated() {
        scope.rethrow();
        return Ok(v8::undefined(scope).into());
    }
    match result {
        Ok(record) => Ok(record),
        Err(error) => result_record(scope, Err(error)),
    }
}

fn scroll_restoration<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    local: u32,
    value: &str,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let Some(before) = bound_owner(scope, document)? else {
        return number_record(scope, 0);
    };
    if !matches!(value, "auto" | "manual") {
        return Err(failure("Native history scroll restoration mode is invalid"));
    }
    let (ticket, success) = mutation_response(scope, before.binding, local)?;
    let shared = JsRuntime::op_state_from(scope);
    let mut state = shared.borrow_mut();
    let log = state.borrow::<auxiliary_pages::Pages>().history_log.clone();
    let Histories {
        targets,
        transactions,
        ..
    } = store(&mut state);
    let history = targets
        .get_mut(&before.binding.target)
        .ok_or_else(|| failure("Native target history is not initialized"))?;
    let result = log
        .publish_with(&before.binding.target.to_string(), || {
            history.set_scroll(before.binding.group, local, value)?;
            transactions.committed(ticket)?;
            Ok(pending_metadata(history, before.binding, ticket))
        })
        .map(|_| ());
    mutation_failed(&mut state, ticket, result)?;
    drop(state);
    interrupt_committed_for_testing(scope, before.binding);
    Ok(success)
}

#[op2(reentrant)]
pub(in crate::runtime) fn op_history_scroll_restoration<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    local: u32,
    #[string] value: &str,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let context = document
        .get_creation_context(scope)
        .ok_or_else(|| failure("Native history document realm unavailable"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope, scope);
    let result = scroll_restoration(scope, document, local, value);
    if scope.has_caught() || scope.has_terminated() {
        scope.rethrow();
        return Ok(v8::undefined(scope).into());
    }
    match result {
        Ok(record) => Ok(record),
        Err(error) => result_record(scope, Err(error)),
    }
}

fn acknowledge<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    token: &str,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let before = bound_owner(scope, document)?
        .ok_or_else(|| failure("Native history acknowledgement has no owner"))?;
    let value = v8::Boolean::new(scope, true);
    let success = result_record(scope, Ok(value.into()))?;
    let after = bound_owner(scope, document)?
        .ok_or_else(|| failure("Native history acknowledgement lost its owner"))?;
    same_owner(before.binding, after.binding)?;
    let shared = JsRuntime::op_state_from(scope);
    let mut state = shared.borrow_mut();
    let log = state.borrow::<auxiliary_pages::Pages>().history_log.clone();
    let Histories {
        targets,
        transactions,
        ..
    } = store(&mut state);
    let history = targets
        .get(&after.binding.target)
        .ok_or_else(|| failure("Native history ACK target disappeared"))?;
    log.publish_with(&after.binding.target.to_string(), || {
        transactions.ack(
            transactions::Identity {
                target: after.binding.target,
                document: after.binding.document,
            },
            token,
        )?;
        Ok(metadata(history, after.binding))
    })
    .map_err(failure)?;
    Ok(success)
}

#[op2(reentrant)]
pub(in crate::runtime) fn op_history_ack<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    #[string] token: &str,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let context = document
        .get_creation_context(scope)
        .ok_or_else(|| failure("Native history document realm unavailable"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope, scope);
    let result = acknowledge(scope, document, token);
    if scope.has_caught() || scope.has_terminated() {
        scope.rethrow();
        return Ok(v8::undefined(scope).into());
    }
    match result {
        Ok(record) => Ok(record),
        Err(error) => result_record(scope, Err(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bind(history: &mut Ledger, group: u64, local: u32, change: Change) {
        let draft = Draft {
            group: Some(group),
            local,
            url: "https://history.test/same".into(),
            key: format!("key-{group}-{local}"),
            navigation_id: format!("id-{local}"),
            scroll: "auto".into(),
            state: None,
            bytes: 0,
        };
        let plan = history
            .prepare(
                draft,
                change,
                Limits {
                    entries: 8,
                    url_bytes: 128,
                    state_bytes: 128,
                },
            )
            .unwrap();
        history.commit(plan).unwrap();
    }
    #[test]
    fn selected_entry_uses_actual_group_and_local_identity_not_equal_urls() {
        let mut history = Ledger::new(41);
        bind(&mut history, 10, 1, Change::Push);
        let old = history.current().unwrap().row;
        bind(&mut history, 10, 2, Change::Push);
        let selected = selection(&history, 10, 1).unwrap();
        assert_eq!(selected.row, old);
        assert_eq!(selected.revision, history.revision());
        bind(&mut history, 20, 1, Change::Push);
        assert!(selection(&history, 10, 1).is_err());
        assert!(selection(&history, 20, 2).is_err());
        assert!(selection(&history, 20, 0).is_err());
        assert_ne!(selection(&history, 20, 1).unwrap().row, old);
    }
}

#[cfg(test)]
#[path = "native_history_ops_coverage_contracts.rs"]
mod coverage_contracts;
