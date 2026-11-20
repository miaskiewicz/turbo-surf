//! Aggregate drain expiry must not interrupt a healthy task's synchronous tail.
use serde_json::Value;
use turbo_surf_render::{
    BrowserStorage, DocumentResponseMetadata, PageSession, PageSessionOptions, WindowTimerMode,
};

async fn page_with_mode(mode: WindowTimerMode) -> PageSession {
    PageSession::open_with_document_response_and_options(
        "<!doctype html><p>Budget boundary</p>",
        "https://budget-boundary.test/",
        "",
        "",
        100,
        "",
        Default::default(),
        &[],
        BrowserStorage::default(),
        DocumentResponseMetadata::default(),
        None,
        PageSessionOptions {
            window_timer_mode: mode,
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

async fn healthy_turns(mode: WindowTimerMode) {
    let mut page = page_with_mode(mode).await;
    page.read_engine(
        r#"
      globalThis.turns = {entered:0, exited:0, active:false};
      globalThis.runBoundedTask = () => {
        turns.entered++; turns.active=true;
        try {
          const until=performance.now()+70;
          while(performance.now()<until) {}
        } finally { turns.active=false; turns.exited++; }
        if(turns.exited<6)setTimeout(runBoundedTask,0);
      };
      setTimeout(runBoundedTask,0);
      return true;
    "#,
    )
    .unwrap();
    let outcome = page.eval_engine("globalThis.__RESULT='reached';").await;
    let value: Value = serde_json::from_str(&page.read_engine("return turns;").unwrap()).unwrap();
    assert!(
        value["entered"].as_u64().unwrap() > 0,
        "actual task must execute: {value}"
    );
    eprintln!("drain outcome={outcome:?}; turns={value}");
    assert!(
        value["exited"].as_u64().unwrap() < 6,
        "aggregate expiry must retain queued work: {value}"
    );
    assert_eq!(
        value["active"], false,
        "cooperative expiry must leave a completed JS stack: {value}"
    );
    assert_eq!(
        value["entered"], value["exited"],
        "task finally must run: {value}"
    );
    assert_eq!(outcome.unwrap(), "reached");
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let value: Value = serde_json::from_str(&page.read_engine("return turns;")?).unwrap();
            if value["exited"] == 6 {
                return Ok::<(), String>(());
            }
            page.eval_engine("globalThis.__RESULT='continuing';")
                .await?;
        }
    })
    .await
    .unwrap()
    .unwrap();
    let value: Value = serde_json::from_str(&page.read_engine("return turns;").unwrap()).unwrap();
    assert_eq!(
        value["entered"], 6,
        "queued tasks must resume exactly once: {value}"
    );
    assert_eq!(value["exited"], 6);
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn cumulative_healthy_tasks_finish_their_synchronous_tail_at_the_drain_deadline() {
    healthy_turns(WindowTimerMode::RealTime).await;
}

#[tokio::test(flavor = "current_thread")]
async fn virtual_timer_mode_keeps_the_same_aggregate_elapsed_deadline() {
    healthy_turns(WindowTimerMode::Virtual).await;
}

#[tokio::test(flavor = "current_thread")]
async fn a_single_unbounded_callback_still_hits_the_unchanged_execution_limit() {
    let mut page = page_with_mode(WindowTimerMode::RealTime).await;
    page.read_engine("setTimeout(()=>{while(true){}},0);return true;")
        .unwrap();
    let failure = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        page.poll_background_work(),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(failure, "Native background task exceeded execution budget");
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn a_hard_interrupted_guest_drain_is_not_reported_as_success() {
    let mut page = page_with_mode(WindowTimerMode::RealTime).await;
    page.read_engine("setTimeout(()=>{while(true){}},0);return true;")
        .unwrap();
    let error = page
        .eval_engine("globalThis.__RESULT='reached';")
        .await
        .unwrap_err();
    assert_eq!(
        error,
        "Execution interrupted by the session execution budget"
    );
    page.close();
}
