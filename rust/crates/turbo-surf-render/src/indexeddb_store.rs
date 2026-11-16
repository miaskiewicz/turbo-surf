//! Realm-independent committed IndexedDB data; live operations are weak native edges.
use super::*;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

type Name = Vec<u16>;
type Snapshot = Rc<crate::crypto_clone::CloneSnapshot>;
#[derive(Default)]
struct Database {
    version: f64,
    names: Vec<Name>,
    stores: HashMap<Name, Snapshot>,
}
#[derive(Default)]
struct Store {
    databases: HashMap<u32, Vec<(Name, Database)>>,
    live: HashMap<(u32, Name, u32), Vec<v8::Weak<v8::Object>>>,
    tasks: std::collections::VecDeque<Task>,
}
#[derive(Clone, Default)]
struct Backing(Rc<RefCell<Store>>);
fn backing(scope: &mut v8::PinScope) -> Rc<RefCell<Store>> {
    let shared = JsRuntime::op_state_from(scope);
    let mut state = shared.borrow_mut();
    if !state.has::<Backing>() {
        state.put(Backing::default());
    }
    state.borrow::<Backing>().0.clone()
}
fn partition(
    scope: &mut v8::PinScope,
    owner: v8::Local<v8::Object>,
    unpartitioned: bool,
) -> Result<u32, JsErrorBox> {
    let shared = JsRuntime::op_state_from(scope);
    let result = storage_area(scope, &mut shared.borrow_mut(), owner, false, unpartitioned);
    result
}
fn units(scope: &v8::PinScope, text: v8::Local<v8::String>) -> Name {
    let mut result = vec![0; text.length()];
    text.write_v2(scope, 0, &mut result, v8::WriteFlags::empty());
    result
}
fn text<'s>(scope: &mut v8::PinScope<'s, '_>, name: &[u16]) -> v8::Local<'s, v8::String> {
    v8::String::new_from_two_byte(scope, name, v8::NewStringType::Normal).unwrap()
}
fn put<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    key: &str,
    value: v8::Local<'s, v8::Value>,
) -> Result<(), JsErrorBox> {
    let key = v8::String::new(scope, key).unwrap();
    if object.create_data_property(scope, key.into(), value) != Some(true) {
        return Err(JsErrorBox::generic(
            "IndexedDB native result creation failed",
        ));
    }
    Ok(())
}
fn array<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    values: Vec<v8::Local<'s, v8::Value>>,
) -> Result<v8::Local<'s, v8::Array>, JsErrorBox> {
    let result = v8::Array::new(scope, values.len() as i32);
    for (index, value) in values.into_iter().enumerate() {
        put(scope, result.into(), &index.to_string(), value)?;
    }
    Ok(result)
}
fn database_mut(store: &mut Store, partition: u32, name: Name) -> &mut Database {
    let databases = store.databases.entry(partition).or_default();
    let index = databases
        .iter()
        .position(|(key, _)| *key == name)
        .unwrap_or_else(|| {
            databases.push((name, Database::default()));
            databases.len() - 1
        });
    &mut databases[index].1
}
fn info<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    name: &[u16],
    version: f64,
    names: &[Name],
) -> Result<v8::Local<'s, v8::Object>, JsErrorBox> {
    let result = v8::Object::new(scope);
    let name = text(scope, name);
    put(scope, result, "name", name.into())?;
    let version = v8::Number::new(scope, version);
    put(scope, result, "version", version.into())?;
    let values = names.iter().map(|name| text(scope, name).into()).collect();
    let names = array(scope, values)?;
    put(scope, result, "names", names.into())?;
    Ok(result)
}

#[op2(reentrant)]
pub(super) fn op_idb_clone<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
    owner: v8::Local<'s, v8::Object>,
    #[string] prefix: &str,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let context = platform_object_context(scope, owner)?;
    if !owner.strict_equals(context.global(scope).into())
        || context
            .get_slot::<private_boundary::Infrastructure>()
            .is_some()
    {
        return Err(JsErrorBox::type_error("Invalid IndexedDB clone realm"));
    }
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope, scope);
    let result = crate::crypto_clone::serialize_indexeddb_state(scope, value, prefix)
        .and_then(|snapshot| crate::crypto_clone::deserialize(scope, &snapshot));
    if scope.has_caught() || scope.has_terminated() {
        scope.rethrow();
        return Ok(v8::undefined(scope).into());
    }
    result
}

#[op2(reentrant)]
pub(super) fn op_idb_load<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    owner: v8::Local<'s, v8::Object>,
    unpartitioned: bool,
    name: v8::Local<'s, v8::String>,
    store_name: Option<v8::Local<'s, v8::String>>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let partition = partition(scope, owner, unpartitioned)?;
    let name = units(scope, name);
    let store_name = store_name.map(|name| units(scope, name));
    let backing = backing(scope);
    let (metadata, snapshot) = {
        let state = backing.borrow();
        let database = state
            .databases
            .get(&partition)
            .and_then(|dbs| dbs.iter().find(|(key, _)| *key == name));
        match database {
            Some((_, db)) => (
                Some((db.version, db.names.clone())),
                store_name
                    .as_ref()
                    .and_then(|name| db.stores.get(name).cloned()),
            ),
            None => (None, None),
        }
    };
    let context = owner
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::type_error("Invalid IndexedDB owner"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    if store_name.is_some() {
        return match snapshot {
            Some(snapshot) => crate::crypto_clone::deserialize(scope, &snapshot),
            None => Ok(v8::undefined(scope).into()),
        };
    }
    match metadata {
        Some((version, names)) => Ok(info(scope, &name, version, &names)?.into()),
        None => Ok(v8::undefined(scope).into()),
    }
}

fn snapshot_stores<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    data: v8::Local<'s, v8::Value>,
    names: &[Name],
) -> Result<Vec<(Name, Snapshot)>, JsErrorBox> {
    let data = v8::Local::<v8::Object>::try_from(data)
        .map_err(|_| JsErrorBox::type_error("Invalid IndexedDB transaction data"))?;
    let mut snapshots = Vec::with_capacity(names.len());
    for name in names {
        let key = text(scope, name);
        let value = data
            .get(scope, key.into())
            .ok_or_else(|| JsErrorBox::type_error("Invalid IndexedDB transaction data"))?;
        let snapshot = crate::crypto_clone::serialize_indexeddb_state(scope, value, "")?;
        snapshots.push((name.clone(), Rc::new(snapshot)));
    }
    Ok(snapshots)
}

#[op2(nofast, reentrant)]
pub(super) fn op_idb_save<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    owner: v8::Local<'s, v8::Object>,
    unpartitioned: bool,
    name: v8::Local<'s, v8::String>,
    version: f64,
    names: v8::Local<'s, v8::Array>,
    data: v8::Local<'s, v8::Value>,
    replace: bool,
) -> Result<(), JsErrorBox> {
    let partition = partition(scope, owner, unpartitioned)?;
    let name = units(scope, name);
    let mut selected = Vec::new();
    for index in 0..names.length() {
        let name = names
            .get_index(scope, index)
            .and_then(|value| v8::Local::<v8::String>::try_from(value).ok())
            .ok_or_else(|| JsErrorBox::type_error("Invalid IndexedDB store name"))?;
        selected.push(units(scope, name));
    }
    let context = owner
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::type_error("Invalid IndexedDB owner"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    // Stage every scope first: a failed clone cannot publish a partial transaction.
    let snapshots = snapshot_stores(scope, data, &selected)?;
    let backing = backing(scope);
    let mut state = backing.borrow_mut();
    let db = database_mut(&mut state, partition, name);
    if replace {
        db.stores.clear();
        db.names = selected.clone();
        db.version = version;
    }
    for (name, snapshot) in snapshots {
        db.stores.insert(name, snapshot);
    }
    Ok(())
}

#[op2(nofast, reentrant)]
pub(super) fn op_idb_delete(
    scope: &mut v8::PinScope,
    owner: v8::Local<v8::Object>,
    unpartitioned: bool,
    name: v8::Local<v8::String>,
) -> Result<(), JsErrorBox> {
    let partition = partition(scope, owner, unpartitioned)?;
    let name = units(scope, name);
    if let Some(dbs) = backing(scope).borrow_mut().databases.get_mut(&partition) {
        dbs.retain(|(key, _)| *key != name);
    }
    Ok(())
}

#[op2(reentrant)]
pub(super) fn op_idb_databases<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    owner: v8::Local<'s, v8::Object>,
    unpartitioned: bool,
) -> Result<v8::Local<'s, v8::Array>, JsErrorBox> {
    let partition = partition(scope, owner, unpartitioned)?;
    let backing = backing(scope);
    let metadata = backing
        .borrow()
        .databases
        .get(&partition)
        .map(|dbs| {
            dbs.iter()
                .map(|(name, db)| (name.clone(), db.version))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let context = owner
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::type_error("Invalid IndexedDB owner"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    let mut values = Vec::new();
    for (name, version) in metadata {
        values.push(info(scope, &name, version, &[])?.into());
    }
    array(scope, values)
}

#[op2(reentrant)]
pub(super) fn op_idb_live<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    owner: v8::Local<'s, v8::Object>,
    unpartitioned: bool,
    name: v8::Local<'s, v8::String>,
    kind: u32,
    action: u32,
    value: v8::Local<'s, v8::Value>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let partition = partition(scope, owner, unpartitioned)?;
    let key = (partition, units(scope, name), kind);
    let backing = backing(scope);
    let existing = backing
        .borrow_mut()
        .live
        .entry(key.clone())
        .or_default()
        .iter()
        .filter_map(|weak| weak.to_local(scope))
        .collect::<Vec<_>>();
    let context = owner
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::type_error("Invalid IndexedDB owner"))?;
    let scope = &mut v8::ContextScope::new(scope, context);
    let mut selected = existing;
    let result = match action {
        0 => array(
            scope,
            selected.iter().map(|value| (*value).into()).collect(),
        )?
        .into(),
        1 => {
            let values = v8::Local::<v8::Array>::try_from(value)
                .map_err(|_| JsErrorBox::type_error("Invalid IndexedDB live list"))?;
            selected.clear();
            for index in 0..values.length() {
                if let Some(value) = values
                    .get_index(scope, index)
                    .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
                {
                    selected.push(value);
                }
            }
            v8::undefined(scope).into()
        }
        2 => {
            selected.push(
                v8::Local::<v8::Object>::try_from(value)
                    .map_err(|_| JsErrorBox::type_error("Invalid IndexedDB live entry"))?,
            );
            v8::Integer::new_from_unsigned(scope, selected.len() as u32).into()
        }
        3 => {
            if selected.is_empty() {
                v8::undefined(scope).into()
            } else {
                selected.remove(0).into()
            }
        }
        _ => return Err(JsErrorBox::type_error("Invalid IndexedDB live operation")),
    };
    backing.borrow_mut().live.insert(
        key,
        selected
            .into_iter()
            .map(|value| v8::Weak::new(scope, value))
            .collect(),
    );
    Ok(result)
}

struct Task {
    key: (u32, Name, u32),
    target: u64,
    document: u32,
    context: v8::Weak<v8::Context>,
    entry: v8::Weak<v8::Object>,
}

fn task_for<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    key: (u32, Name, u32),
    entry: v8::Local<'s, v8::Object>,
) -> Option<Task> {
    let context = entry.get_creation_context(scope)?;
    if context.get_slot::<FrameRetired>().is_some() {
        return None;
    }
    let document = context.get_slot::<CspHandle>()?.id;
    let target = auxiliary_pages::top_level_identity(scope, context)?.0;
    Some(Task {
        key,
        target,
        document,
        context: v8::Weak::new(scope, context),
        entry: v8::Weak::new(scope, entry),
    })
}

/// Retiring a document only journals work; surviving documents execute it on their own turns.
pub(super) fn retire_context<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) {
    let shared = JsRuntime::op_state_from(scope);
    let Some(backing) = shared.borrow().try_borrow::<Backing>().cloned() else {
        return;
    };
    let mut affected = Vec::new();
    {
        let mut state = backing.0.borrow_mut();
        for (key, values) in &mut state.live {
            let before = values.len();
            let head_removed = values.first().is_some_and(|entry| {
                entry
                    .to_local(scope)
                    .is_none_or(|entry| entry.get_creation_context(scope) == Some(context))
            });
            values.retain(|value| {
                value
                    .to_local(scope)
                    .is_some_and(|value| value.get_creation_context(scope) != Some(context))
            });
            if values.len() != before && (key.2 != 0 || head_removed) {
                affected.push(key.clone());
            }
        }
    }
    let mut callbacks = Vec::new();
    {
        let state = backing.0.borrow();
        for (partition, name, kind) in affected {
            let next_kind = match kind {
                0 => 0,
                1 => 3,
                2 => 2,
                _ => continue,
            };
            let key = (partition, name, next_kind);
            if let Some(entries) = state.live.get(&key) {
                for entry in entries
                    .iter()
                    .take(if next_kind == 0 { 1 } else { usize::MAX })
                {
                    if let Some(entry) = entry.to_local(scope) {
                        callbacks.push((key.clone(), entry));
                    }
                }
            }
        }
    }
    for (key, entry) in callbacks {
        let Some(task) = task_for(scope, key, entry) else {
            continue;
        };
        let mut state = backing.0.borrow_mut();
        if !state
            .tasks
            .iter()
            .any(|queued| queued.key == task.key && queued.entry.to_local(scope) == Some(entry))
        {
            state.tasks.push_back(task);
        }
    }
}

pub(super) fn has_tasks_for(rt: &JsRuntime, owner: Option<u64>) -> bool {
    let shared = rt.op_state();
    let state = shared.borrow();
    state.try_borrow::<Backing>().is_some_and(|backing| {
        backing
            .0
            .borrow()
            .tasks
            .iter()
            .any(|task| owner.is_none_or(|owner| task.target == owner))
    })
}

fn execute_task(rt: &mut JsRuntime, task: &Task) -> Result<bool, String> {
    deno_core::scope!(scope, rt);
    let Some(context) = task.context.to_local(scope) else {
        return Ok(false);
    };
    if context.get_slot::<FrameRetired>().is_some()
        || context
            .get_slot::<CspHandle>()
            .is_none_or(|csp| csp.id != task.document)
        || auxiliary_pages::top_level_identity(scope, context).is_none_or(|id| id.0 != task.target)
    {
        return Ok(false);
    }
    let Some(entry) = task.entry.to_local(scope) else {
        return Ok(false);
    };
    if entry.get_creation_context(scope) != Some(context) {
        return Ok(false);
    }
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope, scope);
    let callback: v8::Local<v8::Value> = if task.key.2 == 3 {
        entry.into()
    } else {
        let name =
            v8::String::new(scope, if task.key.2 == 0 { "schedule" } else { "pump" }).unwrap();
        entry
            .get(scope, name.into())
            .ok_or_else(|| "IndexedDB continuation is unavailable".to_owned())?
    };
    let callback = v8::Local::<v8::Function>::try_from(callback)
        .map_err(|_| "IndexedDB continuation is invalid".to_owned())?;
    if callback.get_creation_context(scope) != Some(context) {
        return Err("IndexedDB continuation belongs to another document".into());
    }
    let argument = v8::Boolean::new(scope, false);
    let args = if task.key.2 == 0 {
        vec![argument.into()]
    } else {
        Vec::new()
    };
    let result = callback.call(scope, entry.into(), &args).ok_or_else(|| {
        scope
            .exception()
            .map(|error| error.to_rust_string_lossy(scope))
            .unwrap_or_else(|| "IndexedDB continuation was interrupted".into())
    })?;
    if scope.is_execution_terminating() || scope.has_terminated() {
        return Err("IndexedDB continuation was interrupted".into());
    }
    Ok(task.key.2 == 3 && result.is_true())
}

pub(super) fn run_task_for(rt: &mut JsRuntime, owner: Option<u64>) -> Result<bool, String> {
    let shared = rt.op_state();
    let Some(backing) = shared.borrow().try_borrow::<Backing>().cloned() else {
        return Ok(false);
    };
    let task = {
        let mut state = backing.0.borrow_mut();
        let Some(at) = state
            .tasks
            .iter()
            .position(|task| owner.is_none_or(|owner| task.target == owner))
        else {
            return Ok(false);
        };
        state.tasks.remove(at).unwrap()
    };
    if execute_task(rt, &task)? {
        deno_core::scope!(scope, rt);
        if let Some(entries) = backing.0.borrow_mut().live.get_mut(&task.key) {
            let completed = task.entry.to_local(scope);
            entries.retain(|entry| entry.to_local(scope) != completed);
        }
    }
    Ok(true)
}

pub(super) fn diagnostics(rt: &JsRuntime) -> serde_json::Value {
    let shared = rt.op_state();
    let state = shared.borrow();
    let tasks = state
        .try_borrow::<Backing>()
        .map_or_else(Vec::new, |backing| {
            backing
                .0
                .borrow()
                .tasks
                .iter()
                .map(|task| {
                    serde_json::json!({
                        "target":task.target,"document":task.document,"kind":task.key.2
                    })
                })
                .collect::<Vec<_>>()
        });
    serde_json::json!({"pendingContinuations":tasks})
}
