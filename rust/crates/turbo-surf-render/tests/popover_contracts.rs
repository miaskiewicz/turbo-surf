use serde_json::Value;
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn native_popover_lifecycle_matches_saved_chrome155_control() {
    let mut page = PageSession::open(
        "<!doctype html><body></body>",
        "http://127.0.0.1/popover-fixture",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let expected: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/popover-lifecycle-chrome155.json"
    ))
    .unwrap();
    let actual: Value = serde_json::from_str(
        &page
            .eval_expression(include_str!(
                "../../../../harness/detection/popover-lifecycle-probe.js"
            ))
            .await
            .unwrap(),
    )
    .unwrap();
    // Event order/trust, task coalescing, cancellation and the nine ancestry
    // combinations exercise the private host scheduler as well as DOM state.
    assert_eq!(actual, expected["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn frame_popover_tasks_use_their_private_scheduler_and_preserve_event_realm() {
    let mut page = PageSession::open(
        "<body><iframe srcdoc='<body><div popover=manual id=panel></div></body>'></iframe></body>",
        "https://popover.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let value: Value = serde_json::from_str(
        &page
            .eval_expression(
                r#"(async()=>{
      const frame=document.querySelector('iframe').contentWindow;
      const panel=frame.document.querySelector('#panel'),events=[];
      panel.addEventListener('toggle',event=>events.push([
        event.oldState,event.newState,event.isTrusted,
        event instanceof frame.ToggleEvent,event instanceof ToggleEvent,
        event.target===panel,panel.matches(':popover-open')
      ]));
      frame.setTimeout=()=>{throw new Error('guest scheduler replacement')};
      panel.showPopover();
      await new Promise(done=>setTimeout(done,0));
      return JSON.stringify(events);
    })()"#,
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        value,
        serde_json::json!([["closed", "open", true, true, false, true, true]])
    );
}
