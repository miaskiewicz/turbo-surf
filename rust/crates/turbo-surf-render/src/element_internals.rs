//! Native CustomStateSet storage and closed shadow ownership for ElementInternals.
use super::*;
struct Updates(v8::Weak<v8::Function>);
pub(super) fn attribute_changed(scope: &mut v8::PinScope, host: v8::Local<v8::Object>, name: &str) {
    let Some(context) = host.get_creation_context(scope) else {
        return;
    };
    let Some(updates) = context.get_slot::<Updates>() else {
        return;
    };
    let Some(callback) = updates.0.to_local(scope) else {
        return;
    };
    let scope = &mut v8::ContextScope::new(scope, context);
    let receiver = v8::undefined(scope);
    let name = v8::String::new(scope, name).unwrap();
    callback.call(scope, receiver.into(), &[host.into(), name.into()]);
}
fn dispatch(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue,
) {
    if args.get(0).to_rust_string_lossy(scope) == "value-snapshot" {
        form_controls_native(scope, args, rv);
        return;
    }
    if args.get(0).to_rust_string_lossy(scope) == "private" {
        let Ok(object) = v8::Local::<v8::Object>::try_from(args.get(1)) else {
            return;
        };
        let family = args.get(2).to_rust_string_lossy(scope);
        let name = v8::String::new(scope, &format!("turbo.element-internals.{family}")).unwrap();
        let key = v8::Private::for_api(scope, Some(name));
        if !args.get(3).is_undefined() {
            object.set_private(scope, key, args.get(3));
        }
        if let Some(value) = object.get_private(scope, key) {
            rv.set(value);
        }
        return;
    }
    let Ok(host) = v8::Local::<v8::Object>::try_from(args.get(1)) else {
        return;
    };
    let _dom = DomScope::for_object(scope, host);
    let Some(handle) = handle_of(scope, host) else {
        return;
    };
    let operation = args.get(0).to_rust_string_lossy(scope);
    if operation == "shadow" {
        if let Some(root) = with_tree(|tree| tree.shadow_root(handle)).flatten() {
            rv.set(wrap(scope, root).into());
        } else {
            rv.set(v8::null(scope).into());
        }
        return;
    }
    if operation == "clear" {
        with_tree_mut(|tree| tree.clear_custom_states(handle));
        return;
    }
    let Some(key) = args.get(2).to_string(scope) else {
        return;
    };
    let key = string_code_units(scope, key);
    match operation.as_str() {
        "add" => {
            with_tree_mut(|tree| tree.set_custom_state_utf16(handle, &key, true));
        }
        "delete" => {
            let present =
                with_tree(|tree| tree.has_custom_state_utf16(handle, &key)).unwrap_or(false);
            with_tree_mut(|tree| tree.set_custom_state_utf16(handle, &key, false));
            rv.set(v8::Boolean::new(scope, present).into());
        }
        _ => {}
    }
}
pub(super) fn install(scope: &mut v8::PinScope) {
    let global = scope.get_current_context().global(scope);
    bind_method(scope, global, "__internalsNative", dispatch);
    run_js(scope, include_str!("element_internals.js"));
    let key = v8::String::new(scope, "__captureInternalsUpdates").unwrap();
    if let Some(callback) = global
        .get(scope, key.into())
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    {
        if let Some(callback) = dom_ownership::helper_function(scope, callback) {
            scope
                .get_current_context()
                .set_slot(std::rc::Rc::new(Updates(callback)));
        }
    }
    global.delete(scope, key.into());
}

#[cfg(test)]
mod tests {
    use super::*;
    // The standalone binding has no networking FormData backend. Model its trusted
    // private-brand/transaction boundary here; production tests use the real backend.
    fn form_data_adapter(scope: &mut v8::PinScope) {
        let global = scope.get_current_context().global(scope);
        bind_method(scope, global, "__fdBegin", |scope, args, mut rv| {
            if let Some(entries) =
                begin_form_data_constructor_entries(scope, args.get(0), args.get(1))
            {
                rv.set(entries.into());
            }
        });
        bind_method(scope, global, "__fdFinish", |scope, args, _rv| {
            finish_form_data_constructor(scope, args.get(0))
        });
        bind_method(scope, global, "__fdEvent", |scope, args, _rv| {
            if let Ok(form) = v8::Local::<v8::Object>::try_from(args.get(0)) {
                dispatch_formdata_event(scope, form, args.get(1));
            }
        });
        let source=v8::String::new(scope,r#"(()=>{const brands=new WeakSet();globalThis.FormData=class FormData{constructor(form,submitter){brands.add(this);if(form!==undefined){__fdBegin(form,submitter);try{const alias=Object.create(FormData.prototype);brands.add(alias);__fdEvent(form,alias);}finally{__fdFinish(form);}}}};return value=>brands.has(value)?Object.freeze({kind:'formdata',entries:Object.freeze([])}):null;})()"#).unwrap();
        let reader = v8::Script::compile(scope, source, None)
            .unwrap()
            .run(scope)
            .unwrap();
        assert!(install_form_value_reader(
            scope,
            v8::Local::<v8::Function>::try_from(reader).unwrap()
        ));
    }
    fn assert_control(cases: &[u8], probe: &str, control: &str, adapter: bool) {
        super::super::tests::init_v8();
        let isolate = &mut v8::Isolate::new(Default::default());
        v8::scope!(let scope,isolate);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        let cases_value: serde_json::Value = serde_json::from_slice(cases).unwrap();
        install_independent_html(scope, cases_value[0]["html"].as_str().unwrap());
        if adapter {
            form_data_adapter(scope);
        }
        v8::tc_scope!(let scope,scope);
        let source = v8::String::new(scope, probe).unwrap();
        let result = v8::Script::compile(scope, source, None).unwrap().run(scope);
        if result.is_none() {
            let error = scope.exception().unwrap().to_rust_string_lossy(scope);
            panic!("{error}");
        }
        let actual = v8::json::stringify(scope, result.unwrap())
            .unwrap()
            .to_rust_string_lossy(scope);
        let actual: serde_json::Value = serde_json::from_str(&actual).unwrap();
        let control: serde_json::Value = serde_json::from_str(control).unwrap();
        use sha2::{Digest, Sha256};
        assert_eq!(control["browser"], "Chrome/155.0.8059.80");
        assert_eq!(control["v8"], "15.5.35.23");
        assert_eq!(control["errors"], serde_json::json!([]));
        assert_eq!(
            control["probeSha256"],
            format!("{:x}", Sha256::digest(probe.as_bytes()))
        );
        assert_eq!(
            control["casesSha256"],
            format!("{:x}", Sha256::digest(cases))
        );
        assert_eq!(
            control["result"].as_array().unwrap().len(),
            cases_value.as_array().unwrap().len()
        );
        assert_eq!(actual, control["result"][0]["result"]);
        reset();
    }
    #[test]
    fn native_form_connectivity_matches_independent_chrome() {
        assert_control(
            include_bytes!("form_connectivity_cases.json"),
            include_str!("form_connectivity_probe.js"),
            include_str!("form_connectivity_chrome155.json"),
            false,
        );
    }
    #[test]
    fn native_internals_core_matches_independent_chrome() {
        assert_control(
            include_bytes!("element_internals_core_cases.json"),
            include_str!("element_internals_core_probe.js"),
            include_str!("element_internals_core_chrome155.json"),
            false,
        );
    }
    #[test]
    fn native_internals_associated_matches_independent_chrome() {
        assert_control(
            include_bytes!("element_internals_associated_cases.json"),
            include_str!("element_internals_associated_probe.js"),
            include_str!("element_internals_associated_chrome155.json"),
            false,
        );
    }
    #[test]
    fn native_internals_state_matches_independent_chrome() {
        assert_control(
            include_bytes!("element_internals_state_cases.json"),
            include_str!("element_internals_state_probe.js"),
            include_str!("element_internals_state_chrome155.json"),
            false,
        );
    }
    #[test]
    fn native_internals_definition_order_matches_chrome() {
        assert_control(
            include_bytes!("element_internals_definition_cases.json"),
            include_str!("element_internals_definition_probe.js"),
            include_str!("element_internals_definition_chrome155.json"),
            false,
        );
    }
    #[test]
    fn native_formdata_event_matches_independent_chrome() {
        assert_control(
            include_bytes!("formdata_event_cases.json"),
            include_str!("formdata_event_probe.js"),
            include_str!("formdata_event_chrome155.json"),
            true,
        );
    }
    #[test]
    fn native_formdata_guards_matches_independent_chrome() {
        assert_control(
            include_bytes!("formdata_guards_cases.json"),
            include_str!("formdata_guards_probe.js"),
            include_str!("formdata_guards_chrome155.json"),
            true,
        );
    }
    #[test]
    fn native_internals_setlike_matches_independent_chrome() {
        assert_control(
            include_bytes!("element_internals_setlike_cases.json"),
            include_str!("element_internals_setlike_probe.js"),
            include_str!("element_internals_setlike_chrome155.json"),
            false,
        );
    }
}
