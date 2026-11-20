//! Quiescence heuristics retain the live session's existing budget semantics.
use turbo_surf_render::{PageSession, DEFAULT_RENDER_BUDGET_MS};

async fn page(budget: u64) -> PageSession {
    PageSession::open("<body></body>", "https://quiescence.test/", "", "", budget)
        .await
        .unwrap()
}

#[tokio::test]
async fn watchdog_during_dom_signature_preserves_reached_interaction_state() {
    let mut page = page(200).await;
    let reached = page
        .eval_engine(
            "__tsNs.domSig=()=>{while(true){}};document.body.dataset.reached='yes';globalThis.__RESULT='landed';globalThis.signatureTimer=setInterval(()=>{},0);__tsNs.domSig();",
        )
        .await
        .unwrap_err();
    assert_eq!(
        reached,
        "Execution interrupted by the session execution budget"
    );
    assert_eq!(
        page.read_engine("return document.body.dataset.reached")
            .unwrap(),
        "\"yes\""
    );
    assert_eq!(
        page.eval_engine(
            "clearInterval(signatureTimer);__tsNs.domSig=()=>'';globalThis.__RESULT='usable'"
        )
        .await
        .unwrap(),
        "usable"
    );
}

#[tokio::test]
async fn dom_signature_exception_remains_an_error() {
    // Error controls must reach the helper rather than race the watchdog.
    let mut page = page(DEFAULT_RENDER_BUDGET_MS).await;
    let error = page
        .eval_engine(
            "__tsNs.domSig=()=>{throw new Error('signature fixture rejected')};globalThis.signatureTimer=setInterval(()=>{},0);",
        )
        .await
        .unwrap_err();
    assert!(error.contains("signature fixture rejected"), "{error}");
}

#[tokio::test]
async fn missing_dom_signature_remains_an_error() {
    let mut page = page(DEFAULT_RENDER_BUDGET_MS).await;
    let error = page
        .eval_engine("__tsNs.domSig=undefined;globalThis.signatureTimer=setInterval(()=>{},0);")
        .await
        .unwrap_err();
    assert_eq!(error, "Namespace function domSig failed");
}

#[tokio::test]
async fn watchdog_during_selected_work_predicate_preserves_reached_interaction_state() {
    let mut page = page(200).await;
    let reached = page
        .eval_engine(
            "globalThis.savedPendingWork=__tsNs.pendingWork;__tsNs.pendingWork=()=>{while(true){}};document.body.dataset.reached='predicate';globalThis.__RESULT='landed';",
        )
        .await;
    assert_eq!(
        page.read_engine("return document.body.dataset.reached")
            .unwrap(),
        "\"predicate\""
    );
    let error = reached.unwrap_err();
    assert_eq!(
        error,
        "Execution interrupted by the session execution budget"
    );
    assert_eq!(
        page.eval_engine(
            "__tsNs.pendingWork=savedPendingWork;delete globalThis.savedPendingWork;globalThis.__RESULT='usable'"
        )
        .await
        .unwrap(),
        "usable"
    );
}

#[tokio::test]
async fn missing_selected_work_predicate_remains_an_error_without_an_owned_deadline() {
    let mut page = page(DEFAULT_RENDER_BUDGET_MS).await;
    let error = page
        .eval_engine(
            "globalThis.savedPendingWork=__tsNs.pendingWork;__tsNs.pendingWork=undefined;globalThis.__RESULT='landed';",
        )
        .await
        .unwrap_err();
    assert_eq!(
        error,
        "Selected async document work predicate is unavailable"
    );
    assert_eq!(
        page.eval_engine(
            "__tsNs.pendingWork=savedPendingWork;delete globalThis.savedPendingWork;globalThis.__RESULT='usable'"
        )
        .await
        .unwrap(),
        "usable"
    );
}
