//! Host primitives behind the Observable API (`observable.js`): reporting an
//! exception to its realm's Window and recognizing native promises.
use super::*;

/// "Report the exception": an ErrorEvent at the current realm's Window whose
/// message and location come from V8's own message for `error`, as Blink's
/// `V8ScriptRunner::ReportException` builds them.
#[op2(nofast, reentrant)]
pub(super) fn op_report_exception<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    error: v8::Local<'s, v8::Value>,
) {
    report_exception(scope, document, error);
}

fn report_exception<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    error: v8::Local<'s, v8::Value>,
) {
    if scope.is_execution_terminating() {
        return;
    }
    let Ok(context) = platform_object_context(scope, document) else {
        return;
    };
    let active = context
        .get_slot::<ActiveDocument>()
        .and_then(|active| active.0.to_local(scope))
        .is_some_and(|active| active.strict_equals(document.into()));
    if !active {
        return;
    }
    // The captured reporting document owns the event even when the thrown value
    // is primitive or belongs to another guest realm.
    let scope = &mut v8::ContextScope::new(scope, context);
    let error = private_boundary::report_value(scope, error);
    let message = v8::Exception::create_message(scope, error);
    let details = serde_json::json!({
        "message": message.get(scope).to_rust_string_lossy(scope),
        "filename": message
            .get_script_resource_name(scope)
            .map(|name| name.to_rust_string_lossy(scope))
            .unwrap_or_default(),
        "lineno": message.get_line_number(scope).unwrap_or(0),
        "colno": message.get_start_column().checked_add(1).filter(|column| *column > 0).unwrap_or(0),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn report_host_value(page: &mut PageSession, source: &'static str) {
        let _activation = page.rt.activation_key().enter();
        let error = page
            .rt
            .execute_script("<host-report-fixture>", source)
            .unwrap();
        let owner = private_infrastructure::context(&mut page.rt).unwrap();
        deno_core::scope!(scope, page.rt);
        let owner = v8::Local::new(scope, owner);
        let document = owner
            .get_slot::<ActiveDocument>()
            .unwrap()
            .0
            .to_local(scope)
            .unwrap();
        let error = v8::Local::new(scope, error);
        report_exception(scope, document, error);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reports_preserve_guest_values_and_rehome_host_errors_synchronously() {
        let mut page = PageSession::open(
            "<body><iframe></iframe>",
            "https://observable.test/",
            "",
            "",
            5000,
        )
        .await
        .unwrap();
        let count = page.eval_expression(r#"(()=>{
          globalThis.reportedErrors=[];globalThis.reportTargets=[];globalThis.reportColumns=[];
          globalThis.foreignReportError=new frames[0].TypeError('foreign');
          addEventListener('error',event=>{event.preventDefault();reportedErrors.push(event.error);reportColumns.push(event.colno);reportTargets.push(event.target===window&&event.isTrusted);});
          new Observable(subscriber=>subscriber.error(7)).subscribe();
          new Observable(subscriber=>subscriber.error(foreignReportError)).subscribe();
          return reportedErrors.length;
        })()"#).await.unwrap();
        assert_eq!(count, "2");
        report_host_value(&mut page, "new TypeError('native report fixture')");
        report_host_value(&mut page, "({})");
        report_host_value(&mut page, "(()=>{const error=new Error('private');Object.setPrototypeOf(error,new Proxy({},{getOwnPropertyDescriptor(){throw new Error('conversion hook ran')}}));return error})()");
        let actual = page.eval_json_expression(r#"({
          count:reportedErrors.length,hostColumns:reportColumns.slice(2),primitive:reportedErrors[0]===7,
          foreign:reportedErrors[1]===foreignReportError,
          native:reportedErrors[2] instanceof TypeError,
          nativeMessage:reportedErrors[2].message,
          nativeOwner:reportedErrors[2].constructor.constructor('return globalThis')()===globalThis,
          unsupported:reportedErrors[3] instanceof Error,
          unsupportedMessage:reportedErrors[3].message,
          unsupportedOwner:reportedErrors[3].constructor.constructor('return globalThis')()===globalThis,
          proxy:reportedErrors[4] instanceof Error&&reportedErrors[4].message==='Unsupported infrastructure exception',
          targets:reportTargets.every(Boolean)
        })"#).await.unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
            serde_json::json!({
                "count":5,"hostColumns":[0,0,0],"primitive":true,"foreign":true,"native":true,
                "nativeMessage":"native report fixture","nativeOwner":true,
                "unsupported":true,"unsupportedMessage":"Unsupported infrastructure exception",
                "unsupportedOwner":true,"proxy":true,"targets":true,
            })
        );
        page.close();
    }
}
