//! Native V8 dynamic-compilation admission for the actual guest document.
//! Admission is installed on every actual native document realm.
use super::*;
struct Installed;
pub(super) fn installed(context: v8::Local<v8::Context>) -> bool {
    context.get_slot::<Installed>().is_some()
}
fn authorization_document<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
) -> Option<v8::Local<'s, v8::Object>> {
    if context.get_slot::<FrameRetired>().is_some()
        || context
            .get_slot::<private_boundary::Infrastructure>()
            .is_some()
    {
        return None;
    }
    let document = context.get_slot::<ActiveDocument>()?.0.to_local(scope)?;
    (document.get_creation_context(scope) == Some(context)).then_some(document)
}

fn csp_eval_sample_units(scope: &v8::PinScope, source: v8::Local<v8::String>) -> Vec<u16> {
    let mut units = vec![0; source.length().min(40)];
    source.write_v2(scope, 0, &mut units, v8::WriteFlags::empty());
    csp_eval_sample(&units).to_vec()
}

fn deny_code_generation<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    message: &str,
) -> v8::ModifyCodeGenerationFromStringsResult<'s> {
    if let Some(message) = v8::String::new(scope, message) {
        context.set_error_message_for_code_generation_from_strings(message);
    }
    v8::ModifyCodeGenerationFromStringsResult::default()
}
fn trusted_types_sink_label(scope: &v8::PinScope, source: v8::Local<v8::String>) -> &'static str {
    let mut prefix = vec![0; source.length().min(32)];
    source.write_v2(scope, 0, &mut prefix, v8::WriteFlags::empty());
    let prefix = String::from_utf16_lossy(&prefix);
    // Chrome applies this observable label even to eval text mimicking a constructor.
    // It does not identify the native compilation kind or grant admission.
    if [
        "(function anonymous",
        "(async function anonymous",
        "(function* anonymous",
        "(async function* anonymous",
    ]
    .iter()
    .any(|candidate| prefix.starts_with(candidate))
    {
        "Function"
    } else {
        "eval"
    }
}

fn report_code_generation<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    document: v8::Local<'s, v8::Object>,
    source: v8::Local<'s, v8::String>,
    policy: &crate::csp::Policy,
    report_only: &crate::csp::Policy,
) {
    let sample = csp_eval_sample_units(scope, source);
    let enforced = policy.requires_trusted_types();
    for (disposition, records) in [
        (
            "enforce",
            policy.eval_violations_with_trusted_types(enforced),
        ),
        (
            "report",
            report_only.eval_violations_with_trusted_types(enforced),
        ),
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
}

fn authorize_code_generation<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    input: v8::Local<'s, v8::Value>,
    _is_code_like: bool,
) -> v8::ModifyCodeGenerationFromStringsResult<'s> {
    let scope = &mut v8::ContextScope::new(scope, context);
    let Some(document) = authorization_document(scope, context) else {
        return deny_code_generation(
            scope,
            context,
            "Code generation requires an active guest document",
        );
    };
    let typed = crate::browser_env::trusted_script_source(scope, input);
    let original = match typed.or_else(|| v8::Local::<v8::String>::try_from(input).ok()) {
        Some(source) => source,
        None => {
            return v8::ModifyCodeGenerationFromStringsResult {
                codegen_allowed: true,
                modified_source: None,
            };
        }
    };
    let Some(handle) = context.get_slot::<CspHandle>() else {
        return v8::ModifyCodeGenerationFromStringsResult {
            codegen_allowed: true,
            modified_source: Some(original),
        };
    };
    let policy = handle.policy.borrow().current().clone();
    // Chrome's keyword bypass is conditional on enforcing Trusted Types and
    // leaves even branded non-string eval arguments unchanged.
    if policy.bypasses_trusted_types_eval() {
        if input.is_string() {
            let report_only = handle.policy.borrow().report_only.clone();
            report_code_generation(scope, context, document, original, &policy, &report_only);
        }
        return v8::ModifyCodeGenerationFromStringsResult {
            codegen_allowed: !scope.is_execution_terminating(),
            modified_source: None,
        };
    }
    let sink = trusted_types_sink_label(scope, original);
    v8::tc_scope!(let scope,scope);
    let source =
        crate::browser_env::trusted_type_string(scope, document, input, "TrustedScript", sink, "");
    let message=policy.eval_error_message().unwrap_or_else(||"Evaluating a string as JavaScript violates this document's Trusted Type assignment requirements.".into());
    let Some(source) = source else {
        if scope.has_terminated() || scope.is_execution_terminating() {
            return v8::ModifyCodeGenerationFromStringsResult::default();
        }
        // Chrome translates a failed or throwing default policy into this EvalError.
        scope.reset();
        return deny_code_generation(scope, context, &message);
    };
    if crate::browser_env::string_code_units(scope, source)
        != crate::browser_env::string_code_units(scope, original)
    {
        return deny_code_generation(scope, context, &message);
    }
    if authorization_document(scope, context) != Some(document) {
        return deny_code_generation(
            scope,
            context,
            "Code-generation document changed during conversion",
        );
    }
    let policy = handle.policy.borrow().current().clone();
    let report_only = handle.policy.borrow().report_only.clone();
    let message = policy.eval_error_message().unwrap_or_else(|| "Evaluating a string as JavaScript violates this document's Trusted Type assignment requirements.".into());
    report_code_generation(scope, context, document, source, &policy, &report_only);
    if scope.has_terminated() || scope.is_execution_terminating() {
        return v8::ModifyCodeGenerationFromStringsResult::default();
    }
    if policy.allows_eval_with_trusted_types(policy.requires_trusted_types()) {
        v8::ModifyCodeGenerationFromStringsResult {
            codegen_allowed: true,
            modified_source: Some(source),
        }
    } else {
        deny_code_generation(scope, context, &message)
    }
}
pub(super) fn install_code_generation_authorization(
    scope: &mut v8::PinScope,
) -> Result<(), String> {
    let context = scope.get_current_context();
    if context
        .get_slot::<private_boundary::Infrastructure>()
        .is_some()
    {
        return Ok(());
    }
    authorization_document(scope, context)
        .ok_or("Native code-generation document owner unavailable")?;
    scope.set_modify_code_generation_from_strings_callback(authorize_code_generation);
    context.set_slot(Rc::new(Installed));
    context.set_allow_generation_from_strings(false);
    Ok(())
}
