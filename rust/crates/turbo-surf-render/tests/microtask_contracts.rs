use serde_json::{json, Value};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn native_microtask_order_and_callable_window_shape_match_chrome() {
    let mut page = PageSession::open("<body></body>", "https://microtask.test/", "", "", 5000)
        .await
        .unwrap();
    let probe = include_str!("../../../../harness/detection/microtask-order-probe.js")
        .trim()
        .trim_end_matches(';');
    let actual: Value = serde_json::from_str(
        &page
            .eval_expression(&format!("(async()=>JSON.stringify(await({probe})))()"))
            .await
            .unwrap(),
    )
    .unwrap();
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/microtask-order-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, control["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn native_microtasks_follow_the_callback_realm_when_an_iframe_retires() {
    let mut page = PageSession::open("<body></body>", "https://microtask.test/", "", "", 5000)
        .await
        .unwrap();
    let probe = include_str!("../../../../harness/detection/microtask-frame-probe.js")
        .trim()
        .trim_end_matches(';');
    let actual: Value = serde_json::from_str(
        &page
            .eval_expression(&format!("(async()=>JSON.stringify(await({probe})))()"))
            .await
            .unwrap(),
    )
    .unwrap();
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/microtask-frame-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, control["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn native_microtask_exception_reports_a_trusted_error_and_preserves_later_jobs() {
    let mut page = PageSession::open("<body></body>", "https://microtask.test/", "", "", 5000)
        .await
        .unwrap();
    let actual:Value=serde_json::from_str(&page.eval_expression(r#"(async()=>{
      const rows=[];addEventListener('error',event=>{rows.push([event.type,event.isTrusted,event.error instanceof TypeError,event.message.includes('microtask failure'),event.lineno>0,event.colno>0]);event.preventDefault();},{once:true});
      queueMicrotask(()=>{throw new TypeError('microtask failure');});
      queueMicrotask(()=>rows.push('later'));
      await new Promise(resolve=>queueMicrotask(resolve));
      return JSON.stringify(rows);
    })()"#).await.unwrap()).unwrap();
    assert_eq!(
        actual,
        json!([["error", true, true, true, true, true], "later"])
    );
    assert_eq!(page.eval_expression("40+2").await.unwrap(), "42");
}

#[tokio::test(flavor = "current_thread")]
async fn native_self_rescheduling_microtasks_obey_the_watchdog_and_leave_a_usable_session() {
    let mut page = PageSession::open("<body></body>", "https://microtask.test/", "", "", 100)
        .await
        .unwrap();
    let error = page
        .eval_expression("queueMicrotask(function repeat(){queueMicrotask(repeat)});'queued'")
        .await
        .unwrap_err();
    assert!(error.contains("budget"), "{error}");
    assert_eq!(page.eval_expression("40+2").await.unwrap(), "42");
}
