//! Native document tasks for an authenticated async dependency selection.
use super::*;

fn context_target<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) -> Result<u64, String> {
    if context.get_slot::<FrameRetired>().is_some()
        || context
            .get_slot::<private_boundary::Infrastructure>()
            .is_some()
        || context.get_slot::<CspHandle>().is_none()
    {
        return Err("Selected async document is unavailable".into());
    }
    let document = context
        .get_slot::<ActiveDocument>()
        .and_then(|document| document.0.to_local(scope))
        .ok_or("Selected async document is unavailable")?;
    if document.get_creation_context(scope) != Some(context) {
        return Err("Selected async document belongs to another context".into());
    }
    let target = auxiliary_pages::top_level_identity(scope, context)
        .ok_or("Selected async document has no native Page identity")?;
    let root = auxiliary_pages::context(scope, target)?;
    let identity = turbo_dom_browser_env::host_internals::auxiliary_context_identity(root)
        .ok_or("Selected async Page identity is unavailable")?;
    if identity.closed
        || root.get_slot::<FrameRetired>().is_some()
        || !auxiliary_pages::in_target_tree(scope, context, root)
    {
        return Err("Selected async document no longer belongs to its live Page".into());
    }
    Ok(target.0)
}

/// The caller supplies only contexts selected by its native weak-owner graph.
/// This temporary projection does not retain either document or context handles.
pub(super) fn selected_task_targets(
    rt: &mut JsRuntime,
    selected_actual_contexts: &[v8::Global<v8::Context>],
) -> Result<Vec<u64>, String> {
    deno_core::scope!(scope, rt);
    let mut targets = Vec::with_capacity(selected_actual_contexts.len());
    for context in selected_actual_contexts {
        let context = v8::Local::new(scope, context);
        targets.push(context_target(scope, context)?);
    }
    targets.sort_unstable();
    targets.dedup();
    Ok(targets)
}

pub(super) fn run_selected_tasks(rt: &mut JsRuntime, targets: &[u64]) -> Result<bool, String> {
    for target in targets {
        if indexeddb_store::run_task_for(rt, Some(*target))?
            || native_history::run_task_for(rt, Some(*target))?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn has_selected_tasks(rt: &JsRuntime, targets: &[u64]) -> bool {
    targets.iter().any(|target| {
        indexeddb_store::has_tasks_for(rt, Some(*target))
            || native_history::has_tasks_for(&rt.op_state().borrow(), Some(*target))
    })
}
