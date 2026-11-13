//! Quiescence heuristics retain the live session's existing budget semantics.
use turbo_surf_render::PageSession;

async fn page() -> PageSession {
    PageSession::open("<body></body>", "https://quiescence.test/", "", "", 200)
        .await
        .unwrap()
}

#[tokio::test]
async fn watchdog_during_dom_signature_preserves_reached_interaction_state() {
    let mut page = page().await;
    let reached = page
        .eval_engine(
            "__tsNs.domSig=()=>{while(true){}};document.body.dataset.reached='yes';globalThis.__RESULT='landed';globalThis.signatureTimer=setInterval(()=>{},0);",
        )
        .await
        .expect("signature sampling termination keeps the reached interaction result");
    assert_eq!(reached, "landed");
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
    let mut page = page().await;
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
    let mut page = page().await;
    let error = page
        .eval_engine("__tsNs.domSig=undefined;globalThis.signatureTimer=setInterval(()=>{},0);")
        .await
        .unwrap_err();
    assert_eq!(error, "Namespace function domSig failed");
}
