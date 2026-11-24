//! GC ownership controls use synchronous optimizing compilation in this process.
//! A canceled concurrent V8 compiler job can retain its canonical handles until
//! foreground disposal, independently of document resources or authored roots.
//! Keep stream I/O coverage and production compilation scheduling unchanged.
use turbo_surf_render::PageSession;

fn initialize_gc_fixture() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        deno_core::v8::V8::set_flags_from_string("--no-concurrent-recompilation");
    });
}

#[tokio::test(flavor = "current_thread")]
async fn timeout_abort_listener_survives_collection_until_delivery() {
    initialize_gc_fixture();
    let mut page = PageSession::open("<body>", "https://timeout-lifetime.test/", "", "", 5000)
        .await
        .unwrap();
    page.eval_expression("globalThis.timeoutDelivered=false;(()=>{const signal=AbortSignal.timeout(20);signal.addEventListener('abort',()=>{timeoutDelivered=signal.aborted&&signal.reason.name==='TimeoutError';});})() ").await.unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.eval_expression("new Promise(resolve=>setTimeout(()=>resolve(timeoutDelivered),30))")
            .await
            .unwrap(),
        "true"
    );
}

const RETIRED_TIMEOUT_SETUP: &str = r#"(()=>{
    globalThis.timeoutFrameLoaded=false;
    const frame=document.createElement('iframe');
    frame.addEventListener('load',()=>{timeoutFrameLoaded=true;});
    document.body.append(frame);
    const child=frame.contentWindow;
    const signal=child.AbortSignal.timeout(60000);
    signal.addEventListener('abort',()=>{});
    globalThis.timeoutRealmRefs=[new WeakRef(child),new WeakRef(signal)];
    if(timeoutFrameLoaded)throw new Error('frame must still be initializing at retirement');
    frame.remove();
    return 'prepared';
})()"#;

fn retired_timeout_load_state(page: &mut PageSession) -> serde_json::Value {
    serde_json::from_str(&page.read_engine(
        "return {resources:__tsNs.pendingResourceCount(),fetches:Number(__tsNs.pendingFetchCount()),loaded:timeoutFrameLoaded};"
    ).unwrap()).unwrap()
}

async fn settle_retired_timeout_load(page: &mut PageSession) {
    assert_eq!(
        page.eval_engine_expression("__tsNs.waitForDocumentResources().then(()=>'settled')")
            .await
            .unwrap(),
        "settled"
    );
    assert_eq!(
        retired_timeout_load_state(page),
        serde_json::json!({"resources":0,"fetches":0,"loaded":false})
    );
}

struct RetiredTimeoutFailureDiagnostics {
    frames_before_gc: serde_json::Value,
    contexts_before_gc: serde_json::Value,
    loads_before_settlement: serde_json::Value,
    loads_after_settlement: serde_json::Value,
}

impl Drop for RetiredTimeoutFailureDiagnostics {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "retired-timeout pre-gc native frames={} contexts={} loads_before={} loads_after={}",
                self.frames_before_gc, self.contexts_before_gc,
                self.loads_before_settlement, self.loads_after_settlement,
            );
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn pending_timeout_does_not_root_retired_document() {
    initialize_gc_fixture();
    let mut page = PageSession::open("<body>", "https://timeout-lifetime.test/", "", "", 5000)
        .await
        .unwrap();
    assert_eq!(
        page.eval_expression(RETIRED_TIMEOUT_SETUP).await.unwrap(),
        "prepared"
    );
    let loads_before_settlement = retired_timeout_load_state(&mut page);
    settle_retired_timeout_load(&mut page).await;
    // Observe registry state without another guest turn or WeakRef read.
    let _failure_diagnostics = RetiredTimeoutFailureDiagnostics {
        frames_before_gc: page.frame_lifecycle_diagnostics_for_testing(),
        contexts_before_gc: page.context_registry_diagnostics_for_testing(),
        loads_before_settlement,
        loads_after_settlement: retired_timeout_load_state(&mut page),
    };
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.eval_expression("JSON.stringify(timeoutRealmRefs.map(ref=>ref.deref()===undefined))")
            .await
            .unwrap(),
        "[true,true]"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn retired_timeout_collection_waits_for_actual_initial_frame_load_unwind() {
    initialize_gc_fixture();
    let mut page = PageSession::open("<body>", "https://timeout-lifetime.test/", "", "", 5000)
        .await
        .unwrap();
    assert_eq!(
        page.read_engine(&format!("return {RETIRED_TIMEOUT_SETUP};"))
            .unwrap(),
        "\"prepared\""
    );
    let pending = retired_timeout_load_state(&mut page);
    assert!(pending["resources"].as_u64().unwrap() > 0, "{pending}");
    assert!(pending["fetches"].as_u64().unwrap() > 0, "{pending}");
    assert_eq!(pending["loaded"], false);
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.read_engine("return timeoutRealmRefs.map(ref=>ref.deref()===undefined);")
            .unwrap(),
        "[false,false]",
        "the genuine suspended frame-load continuation still owns its retired realm"
    );
    settle_retired_timeout_load(&mut page).await;
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.eval_expression("JSON.stringify(timeoutRealmRefs.map(ref=>ref.deref()===undefined))")
            .await
            .unwrap(),
        "[true,true]"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_authored_parent_hold_keeps_the_retired_realm_and_signal_alive() {
    initialize_gc_fixture();
    let mut page = PageSession::open("<body>", "https://timeout-lifetime.test/", "", "", 5000)
        .await
        .unwrap();
    let setup = RETIRED_TIMEOUT_SETUP.replace(
        "globalThis.timeoutRealmRefs=",
        "globalThis.heldTimeoutRealm=[child,signal];globalThis.timeoutRealmRefs=",
    );
    assert_eq!(page.eval_expression(&setup).await.unwrap(), "prepared");
    settle_retired_timeout_load(&mut page).await;
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.eval_expression("JSON.stringify(timeoutRealmRefs.map(ref=>ref.deref()===undefined))")
            .await
            .unwrap(),
        "[false,false]"
    );
    assert_eq!(
        page.eval_expression("heldTimeoutRealm=null;'released'")
            .await
            .unwrap(),
        "released"
    );
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.eval_expression("JSON.stringify(timeoutRealmRefs.map(ref=>ref.deref()===undefined))")
            .await
            .unwrap(),
        "[true,true]"
    );
}
