//! Host primitives behind the Observable API (`observable.js`): reporting an
//! exception to its realm's Window and recognizing native promises.
use super::*;

/// "Report the exception": an ErrorEvent at the current realm's Window whose
/// message and location come from V8's own message for `error`, as Blink's
/// `V8ScriptRunner::ReportException` builds them.
#[op2(nofast, reentrant)]
pub(super) fn op_report_exception<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    error: v8::Local<'s, v8::Value>,
) {
    if scope.is_execution_terminating()
        || scope
            .get_current_context()
            .get_slot::<FrameRetired>()
            .is_some()
    {
        return;
    }
    let message = v8::Exception::create_message(scope, error);
    let details = serde_json::json!({
        "message": message.get(scope).to_rust_string_lossy(scope),
        "filename": message
            .get_script_resource_name(scope)
            .map(|name| name.to_rust_string_lossy(scope))
            .unwrap_or_default(),
        "lineno": message.get_line_number(scope).unwrap_or(0),
        "colno": message.get_start_column() + 1,
    });
    let Some(json) = v8::String::new(scope, &details.to_string()) else {
        return;
    };
    let Some(details) = v8::json::parse(scope, json) else {
        return;
    };
    v8::tc_scope!(let catch, scope);
    catch.set_verbose(true);
    call_frame_helper(catch, "reportClassicScriptError", &[error, details]);
}

/// Web IDL `IsPromise`, which page code cannot observe or spoof.
#[op2(nofast)]
pub(super) fn op_value_is_promise(value: v8::Local<v8::Value>) -> bool {
    value.is_promise()
}
