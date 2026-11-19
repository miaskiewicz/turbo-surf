//! Document execution must preserve reached markup and reject retired targets.
use super::*;

fn installed_document(html: &str) -> OwnedRuntime {
    let mut runtime = make_runtime("https://document-contract.test/", "", "");
    {
        let _activation = runtime.activation_key().enter();
        install_dom(&mut runtime, html, "https://document-contract.test/").unwrap();
    }
    runtime
}

#[tokio::test(flavor = "current_thread")]
async fn hydration_error_preserves_real_mutations_and_allows_the_next_script() {
    let mut runtime = installed_document("<body><p id=state>before</p></body>");
    let _activation = runtime.activation_key().enter();
    let failure = runtime
        .execute_script(
            "https://document-contract.test/skin.js",
            "document.querySelector('#state').textContent='reached';throw Error('skin failed after mutation')",
        )
        .map(|_| crate::browser_env::document_html())
        .map_err(|error| error.to_string());
    assert!(failure
        .as_ref()
        .unwrap_err()
        .contains("skin failed after mutation"));
    let partial = best_effort_on_budget(&mut runtime, failure, 5000).unwrap();
    assert!(partial.contains("<p id=\"state\">reached</p>"), "{partial}");
    let value = runtime
        .execute_script(
            "https://document-contract.test/recovery.js",
            "document.querySelector('#state').textContent+=' again';document.body.textContent",
        )
        .unwrap();
    assert_eq!(read_string(&mut runtime, value).unwrap(), "reached again");
}

#[tokio::test(flavor = "current_thread")]
async fn terminated_hydration_keeps_reached_markup_and_clears_termination() {
    let mut runtime = installed_document("<body><p id=state>before</p></body>");
    let _activation = runtime.activation_key().enter();
    let watchdog = Watchdog::start(runtime.v8_isolate(), 30);
    let failure = runtime
        .execute_script(
            "https://document-contract.test/runaway.js",
            "document.querySelector('#state').textContent='reached';while(true){}",
        )
        .map(|_| crate::browser_env::document_html())
        .map_err(|error| error.to_string());
    let exceeded = watchdog.exceeded.load(std::sync::atomic::Ordering::Relaxed);
    drop(watchdog);
    assert!(
        exceeded,
        "the real watchdog must terminate the authored loop"
    );
    assert!(
        failure
            .as_ref()
            .unwrap_err()
            .contains("execution terminated"),
        "{failure:?}"
    );
    // Deno clears V8 termination while constructing its returned JsError.
    assert!(!runtime.v8_isolate().is_execution_terminating());
    let partial = best_effort_on_budget(&mut runtime, failure, 30).unwrap();
    assert!(partial.contains("<p id=\"state\">reached</p>"), "{partial}");
    assert!(!runtime.v8_isolate().is_execution_terminating());
    let value = runtime
        .execute_script("https://document-contract.test/healthy.js", "6*7")
        .unwrap();
    assert_eq!(read_string(&mut runtime, value).unwrap(), "42");
}

#[tokio::test(flavor = "current_thread")]
async fn an_already_retired_frame_never_executes_a_queued_classic_script() {
    let mut page = PageSession::open(
        "<body><iframe srcdoc='<body>unchanged child</body>'></iframe></body>",
        "https://document-contract.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval("globalThis.retainedChildDocument=document.querySelector('iframe').contentDocument")
        .await
        .unwrap();
    let context = {
        let _activation = page.rt.activation_key().enter();
        let root = private_infrastructure::context(&mut page.rt).unwrap();
        deno_core::scope!(scope, &mut page.rt);
        let root = v8::Local::new(scope, root);
        let scope = &mut v8::ContextScope::new(scope, root);
        let key = v8::String::new(scope, "retainedChildDocument").unwrap();
        let document = root.global(scope).get(scope, key.into()).unwrap();
        let document = document.to_object(scope).unwrap();
        let context = document.get_creation_context(scope).unwrap();
        assert_ne!(context, root);
        assert!(context.get_slot::<FrameRetired>().is_none());
        v8::Global::new(scope, context)
    };
    page.eval("document.querySelector('iframe').remove()")
        .await
        .unwrap();
    {
        let _activation = page.rt.activation_key().enter();
        deno_core::scope!(scope, &mut page.rt);
        let context = v8::Local::new(scope, &context);
        assert!(context.get_slot::<FrameRetired>().is_some());
    }
    let _activation = page.rt.activation_key().enter();
    let code = "document.body.textContent='must not run'"
        .encode_utf16()
        .collect::<Vec<_>>();
    let error = execute_frame_script(
        &mut page.rt,
        context,
        "https://document-contract.test/queued.js",
        &code,
    )
    .expect_err("retired target must reject execution");
    assert_eq!(error.message, "iframe document has been replaced");
    assert!(!error.interrupted);
    assert!(!error.deadline_cancelled);
    assert!(!error.deferred_interruption);
    assert!(error.error.is_none());
    assert_eq!(
        page.read_engine("return retainedChildDocument.body.textContent")
            .unwrap(),
        "\"unchanged child\""
    );
}
