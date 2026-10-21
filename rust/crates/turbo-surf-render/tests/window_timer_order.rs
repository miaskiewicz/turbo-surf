//! Independent Chrome controls for tasks across connected Window realms.
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn iframe_load_tasks_precede_parent_timeout_on_plain_and_inert_documents() {
    let mut page = PageSession::open(
        "<!doctype html><body></body>",
        "https://timer.test/document.html",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/frame-load-order-probe.js")
        .trim()
        .trim_end_matches(';');
    let value = page
        .eval_expression(&format!("(async()=>JSON.stringify(await({probe})))()"))
        .await
        .unwrap();
    let actual: serde_json::Value = serde_json::from_str(&value).unwrap();
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/frame-load-order-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, expected["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn initial_timer_tasks_finish_deep_microtasks_and_observers_before_the_next_timer() {
    let html = r#"<!doctype html><body><div id="target"></div><script>
      globalThis.timerEvents=[];
      const target=document.querySelector('#target');
      new MutationObserver(()=>timerEvents.push('observer')).observe(target,{attributes:true});
      document.addEventListener('DOMContentLoaded',()=>{
        setTimeout(()=>{
          timerEvents.push('first');
          Promise.resolve().then(()=>{
            timerEvents.push('microtask');
            return Promise.resolve();
          }).then(()=>{
            timerEvents.push('deep microtask');
            target.setAttribute('data-ready','true');
          });
        },0);
        setTimeout(()=>timerEvents.push('second'),0);
      });
    </script></body>"#;
    let mut page = PageSession::open(html, "https://timer.test/", "", "", 10000)
        .await
        .unwrap();
    let actual: serde_json::Value =
        serde_json::from_str(&page.eval_json_expression("timerEvents").await.unwrap()).unwrap();
    assert_eq!(
        actual,
        serde_json::json!(["first", "microtask", "deep microtask", "observer", "second"])
    );
}

#[tokio::test(flavor = "current_thread")]
async fn initial_timer_selection_uses_deadlines_across_parent_and_frame_realms() {
    let mut page = PageSession::open(
        r#"<!doctype html><body><iframe></iframe><script>
          globalThis.timerEvents=[];
          document.addEventListener('DOMContentLoaded',()=>{
            document.querySelector('iframe').contentWindow.setTimeout(()=>timerEvents.push('frame'),0);
            setTimeout(()=>timerEvents.push('parent'),100);
          });
        </script></body>"#,
        "https://timer.test/",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let actual: serde_json::Value =
        serde_json::from_str(&page.eval_json_expression("timerEvents").await.unwrap()).unwrap();
    assert_eq!(actual, serde_json::json!(["frame", "parent"]));
}

#[test]
fn synchronous_timer_driver_finishes_the_initial_job_before_timer_tasks() {
    let html = turbo_surf_render::render_html(
        "<!doctype html><body>",
        r#"const events=[];
          const push=value=>{events.push(value);document.body.setAttribute('data-events',events.join(','));};
          Promise.resolve().then(()=>push('initial microtask'));
          setTimeout(()=>{push('first');Promise.resolve().then(()=>Promise.resolve()).then(()=>push('deep timer microtask'));},0);
          setTimeout(()=>push('second'),0);"#,
    )
    .unwrap();
    assert!(
        html.contains("data-events=\"initial microtask,first,deep timer microtask,second\""),
        "{html}"
    );
}
