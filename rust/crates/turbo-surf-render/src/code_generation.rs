//! Native V8 code-generation admission (eval / Function): the exact-engine SDK's
//! private callback applies the realm's CSP `script-src` and Trusted Types checks.
//! Compiled only under `cfg(turbo_surf_native_proxy)`; measured in that configuration.
use super::*;
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
pub(super) fn install_code_generation_authorization(
    scope: &mut v8::PinScope,
) -> Result<(), String> {
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
