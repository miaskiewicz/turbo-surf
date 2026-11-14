//! Session infrastructure is never an authored document or a visible Page.
use super::*;

struct GuestRoot(Option<v8::Weak<v8::Context>>);
pub(super) struct GuestDocument;

pub(super) fn enabled(rt: &JsRuntime) -> bool {
    rt.op_state().borrow().try_borrow::<GuestRoot>().is_some()
}

pub(super) fn context(rt: &mut JsRuntime) -> Result<v8::Global<v8::Context>, String> {
    let state = rt.op_state();
    let state = state.borrow();
    let Some(initial) = state.try_borrow::<GuestRoot>() else {
        return Ok(rt.main_context());
    };
    let initial = initial.0.clone();
    let root = state.borrow::<auxiliary_pages::Pages>().events.root();
    drop(state);
    deno_core::scope!(scope, rt);
    if let Some(root) = root {
        let id = root
            .parse::<u64>()
            .map_err(|_| "Invalid guest root ownership".to_owned())?;
        let context = auxiliary_pages::context(
            scope,
            turbo_dom_browser_env::host_internals::AuxiliaryContextId(id),
        )?;
        if context.get_slot::<FrameRetired>().is_some() {
            return Err("Guest root document is closed".into());
        }
        return Ok(v8::Global::new(scope, context));
    }
    let context = initial
        .and_then(|initial| initial.to_local(scope))
        .ok_or("Guest root ownership unavailable")?;
    Ok(v8::Global::new(scope, context))
}

pub(super) fn prepare(rt: &mut JsRuntime, html: &str, base: &str) -> Result<(), String> {
    rt.op_state()
        .borrow_mut()
        .put(private_boundary::Infrastructure);
    {
        deno_core::scope!(scope, rt);
        scope
            .get_current_context()
            .set_slot(Rc::new(private_boundary::Infrastructure));
    }
    rt.op_state()
        .borrow_mut()
        .borrow_mut::<auxiliary_pages::Pages>()
        .enabled = false;
    install_dom(
        rt,
        "<!doctype html><html><head></head><body></body></html>",
        "about:blank",
    )?;
    let guest = {
        deno_core::scope!(scope, rt);
        let source = scope.get_current_context();
        let guest =
            turbo_dom_browser_env::host_internals::create_auxiliary_window_context(scope, None);
        for slot in [
            deno_core::CONTEXT_STATE_SLOT_INDEX,
            deno_core::MODULE_MAP_SLOT_INDEX,
        ] {
            let pointer = source.get_aligned_pointer_from_embedder_data(slot);
            // The runtime owns these pointers; guest contexts borrow them.
            unsafe {
                guest.set_aligned_pointer_in_embedder_data(slot, pointer);
            }
        }
        crate::script_inventory::register_guest(scope, guest);
        let core = source
            .get_slot::<RuntimeCore>()
            .ok_or("Private runtime core unavailable")?;
        guest.set_slot(core);
        guest.set_slot(Rc::new(GuestDocument));
        user_activation::inherit_initial(source, guest);
        inherit_parent_csp(scope, guest, source);
        if let Some(origins) = source.get_slot::<ModuleOrigins>() {
            guest.set_slot(Rc::new(ModuleOrigins::new(
                origins.opaque.get(),
                origins.cookie_site_opaque.get(),
            )));
        }
        v8::Global::new(scope, guest)
    };
    {
        deno_core::scope!(scope, rt);
        let local = v8::Local::new(scope, &guest);
        let weak = v8::Weak::new(scope, local);
        JsRuntime::op_state_from(scope)
            .borrow_mut()
            .put(GuestRoot(Some(weak)));
    }
    rt.op_state()
        .borrow_mut()
        .borrow_mut::<auxiliary_pages::Pages>()
        .enabled = true;
    let result = install_dom(rt, html, base);
    if result.is_ok() {
        rt.op_state().borrow_mut().borrow_mut::<GuestRoot>().0 = None;
    }
    result
}

pub(super) fn configure_guest(
    scope: &mut v8::PinScope,
    context: v8::Local<v8::Context>,
    base: &str,
) -> Result<(), String> {
    if context.get_slot::<GuestDocument>().is_none() {
        return Ok(());
    }
    if turbo_dom_browser_env::host_internals::auxiliary_context_identity(context).is_none() {
        return Err("Guest root native identity unavailable".into());
    }
    let namespace =
        crate::browser_env::engine_namespace(scope).ok_or("Guest namespace unavailable")?;
    let name = v8::String::new(scope, "isAuxiliary").unwrap();
    let yes = v8::Boolean::new(scope, true);
    if namespace.create_data_property(scope, name.into(), yes.into()) != Some(true) {
        return Err("Guest scheduling policy installation failed".into());
    }
    frame_modules::register_frame_modules(scope, base).map_err(|error| error.to_string())
}

pub(super) fn execute(rt: &mut JsRuntime, source: &str) -> Result<v8::Global<v8::Value>, String> {
    let context = context(rt)?;
    deno_core::scope!(scope, rt);
    let context = v8::Local::new(scope, context);
    let scope = &mut v8::ContextScope::new(scope, context);
    v8::tc_scope!(let scope, scope);
    let source = v8::String::new(scope, source).ok_or("Guest script allocation failed")?;
    let value = v8::Script::compile(scope, source, None).and_then(|script| script.run(scope));
    value
        .map(|value| v8::Global::new(scope, value))
        .ok_or_else(|| {
            scope
                .exception()
                .map(|error| error.to_rust_string_lossy(scope))
                .unwrap_or_else(|| "Guest script failed".into())
        })
}
