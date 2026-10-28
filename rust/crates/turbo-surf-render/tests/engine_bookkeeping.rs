//! The engine's run-time bookkeeping is not page state (turbo-surf#83): the
//! in-flight fetch count behind idle/settle and the frame nesting depth live in
//! bootstrap-private storage, so page script can neither forge them nor see
//! them as globals. Chrome 156 exposes no such globals (`Object.keys(window)`).
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
#[ignore = "turbo-surf#83: fails until the bookkeeping is private"]
async fn page_globals_cannot_forge_or_observe_engine_bookkeeping() {
    let html = r#"<!doctype html><body><iframe id=f></iframe><script>
window.__pendingFetches = 1e9;
window.__depth = -100;
globalThis.out = {};
fetch('data:text/plain,hi').then(()=>{out.fetched=true},()=>{out.fetched=true});
</script></body>"#;
    let mut page = PageSession::open(html, "https://bookkeeping.test/", "", "", 5000)
        .await
        .unwrap();
    page.eval("globalThis[Symbol.for('ts')].runTimers(200)")
        .await
        .unwrap();
    assert_eq!(
        page.eval_json_expression("globalThis[Symbol.for('ts')].pendingFetchCount()")
            .await
            .unwrap(),
        r#""0""#
    );
    assert_eq!(
        page.eval_json_expression(
            "(()=>{delete window.__pendingFetches;delete window.__depth;const w=document.getElementById('f').contentWindow;\
             return [out.fetched,Object.keys(window).filter(k=>k.startsWith('__')),Object.keys(w).filter(k=>k.startsWith('__')),'__depth' in w];})()"
        )
        .await
        .unwrap(),
        r#"[true,[],[],false]"#
    );
}
