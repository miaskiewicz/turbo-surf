use serde_json::{json, Value};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn expression_waits_for_native_crypto_and_virtual_timers_without_a_global_result_slot() {
    let mut page = PageSession::open("<body></body>", "https://example.test/", "", "", 5000)
        .await
        .unwrap();
    page.eval_expression("(() => { Object.defineProperty(globalThis,'__RESULT',{get(){throw new Error('guest result getter')}}); setInterval(()=>document.body.setAttribute('data-tick',String(performance.now())),1); return 'ready'; })()")
        .await.unwrap();
    let value: Value = serde_json::from_str(&page.eval_expression(r#"(async () => {
      await new Promise(resolve=>setTimeout(resolve,10));
      const digest=await crypto.subtle.digest('SHA-256',new TextEncoder().encode('abc'));
      return JSON.stringify({digest:Array.from(new Uint8Array(digest),value=>value.toString(16).padStart(2,'0')).join(''),timer:true});
    })()"#).await.unwrap()).unwrap();
    assert_eq!(
        value,
        json!({"digest":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", "timer":true})
    );
    assert_eq!(page.eval_expression("42").await.unwrap(), "42");
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn expression_rejection_and_runaway_microtasks_are_bounded_and_leave_the_session_usable() {
    let mut page = PageSession::open("<body></body>", "https://budget.test/", "", "", 100)
        .await
        .unwrap();
    assert!(page
        .eval_expression("Promise.reject(new Error('collector failed'))")
        .await
        .unwrap_err()
        .contains("collector failed"));
    let failure = page
        .eval_expression(
            "Promise.resolve().then(function repeat(){return Promise.resolve().then(repeat)})",
        )
        .await
        .unwrap_err();
    assert!(failure.contains("budget"), "{failure}");
    assert_eq!(page.eval_expression("'alive'").await.unwrap(), "alive");
}

#[tokio::test(flavor = "current_thread")]
async fn reusable_webcrypto_collector_resolves_real_key_operations() {
    let mut page = PageSession::open("<body></body>", "https://example.test/", "", "", 5000)
        .await
        .unwrap();
    let source = include_str!("../../../../harness/detection/webcrypto-probe.js");
    let value: Value = serde_json::from_str(&page.eval_expression(source).await.unwrap()).unwrap();
    assert_eq!(value["ed"]["verify"], true);
    assert_eq!(value["ed"]["publicPromise"], true);
    assert_eq!(value["kem"]["ML-KEM-512"]["error"], "NotSupportedError");
    for algorithm in ["ML-KEM-768", "ML-KEM-1024"] {
        assert_eq!(value["kem"][algorithm]["bitsMatch"], true, "{value}");
        assert_eq!(value["kem"][algorithm]["keyMatch"], true, "{value}");
    }
}
