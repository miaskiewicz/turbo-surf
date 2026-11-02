//! `queueMicrotask` brand-checks its receiver, validates its callback, and keeps
//! working on a window whose iframe was removed. Expected values: Chrome 156
//! (156.0.8078.x headless, the same scripts on a file:// page).
use serde_json::{json, Value};
use turbo_surf_render::PageSession;

const TRY: &str =
    "globalThis.attempt=(f)=>{try{f();return 'ok'}catch(e){return e.name+':'+e.message}};";

async fn run(script: &str, expression: &str) -> Value {
    let html = format!(
        "<!doctype html><body><iframe id=f></iframe><script>{TRY}globalThis.out=[];{script}</script></body>"
    );
    let mut page = PageSession::open(&html, "https://microtasks.test/", "", "", 30_000)
        .await
        .unwrap();
    serde_json::from_str(&page.eval_json_expression(expression).await.unwrap()).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn queue_microtask_brand_checks_and_runs_on_a_removed_iframes_window() {
    let value = run(
        "const f=document.getElementById('f');const w=f.contentWindow;const q=w.queueMicrotask;
         globalThis.r1=attempt(()=>queueMicrotask.call({},()=>out.push('foreign-this')));
         f.remove();
         globalThis.r3=attempt(()=>q.call(w,()=>out.push('detached')));
         queueMicrotask(()=>out.push('main'));",
        "({r1, r3, out})",
    )
    .await;
    assert_eq!(
        value,
        json!({"r1": "TypeError:Illegal invocation", "r3": "ok", "out": ["detached", "main"]})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn queue_microtask_rejects_a_non_function_with_the_webidl_message() {
    let value = run("globalThis.r2=attempt(()=>queueMicrotask(5));", "r2").await;
    assert_eq!(
        value,
        "TypeError:Failed to execute 'queueMicrotask' on 'Window': parameter 1 is not of type 'Function'."
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_iframes_queue_microtask_brand_checks_its_receiver() {
    let value = run(
        "const q=document.getElementById('f').contentWindow.queueMicrotask;
         globalThis.r=attempt(()=>q.call({},()=>out.push('foreign-this')));",
        "({r, out})",
    )
    .await;
    assert_eq!(
        value,
        json!({"r": "TypeError:Illegal invocation", "out": []})
    );
}
