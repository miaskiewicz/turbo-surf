//! Supported standalone render entrypoints retain their own error and cleanup contracts.
use turbo_surf_render::{eval_async, render_hydrate, render_page, DEFAULT_RENDER_BUDGET_MS};

#[tokio::test(flavor = "current_thread")]
async fn standalone_module_failure_retains_reached_dom_and_does_not_poison_next_render() {
    let html = render_hydrate(
        "<!doctype html><body><p id=state>initial</p><script type=module>document.getElementById('state').textContent='module reached';throw new Error('standalone module sentinel');</script>",
        "https://standalone-module.test/",
    )
    .await
    .unwrap();
    assert!(html.contains("module reached"), "{html}");
    let next = render_hydrate(
        "<!doctype html><body><p id=state>fresh</p><script type=module>document.getElementById('state').textContent += ':healthy';</script>",
        "https://standalone-module.test/next",
    )
    .await
    .unwrap();
    assert!(next.contains("fresh:healthy"), "{next}");
    assert!(!next.contains("module reached"), "{next}");
}

#[tokio::test(flavor = "current_thread")]
async fn standalone_unhandled_rejection_keeps_reached_document_and_next_runtime_healthy() {
    let html = render_page(
        "<!doctype html><body><p id=state>initial</p>",
        "https://standalone-rejection.test/",
        "globalThis.priorRuntimeMarker='discarded';document.getElementById('state').textContent='rejection reached';Promise.reject(new Error('standalone rejection sentinel'));",
    )
    .await
    .unwrap();
    assert!(html.contains("rejection reached"), "{html}");
    let next = eval_async(
        "<!doctype html><body><p>fresh</p>",
        "https://standalone-rejection.test/next",
        "globalThis.__RESULT=JSON.stringify({text:document.body.textContent,hasPrior:Object.hasOwn(globalThis,'priorRuntimeMarker')});",
    )
    .await
    .unwrap();
    assert_eq!(next, r#"{"text":"fresh","hasPrior":false}"#);
}

#[tokio::test(flavor = "current_thread")]
async fn standalone_eval_watchdog_rejects_runaway_and_next_eval_is_fresh() {
    let error = eval_async(
        "<!doctype html><body><p>runaway</p>",
        "https://standalone-watchdog.test/",
        "globalThis.priorRuntimeMarker='discarded';while(true){}",
    )
    .await
    .unwrap_err();
    assert!(error.contains("budget exceeded"), "{error}");
    assert!(
        error.contains(&DEFAULT_RENDER_BUDGET_MS.to_string()),
        "{error}"
    );
    let next = eval_async(
        "<!doctype html><body><p>fresh</p>",
        "https://standalone-watchdog.test/next",
        "globalThis.__RESULT=JSON.stringify({text:document.body.textContent,marker:typeof priorRuntimeMarker});",
    )
    .await
    .unwrap();
    assert_eq!(next, r#"{"text":"fresh","marker":"undefined"}"#);
}
