//! Posted-message tasks and nested timers share the running task's clock policy.
use serde_json::json;
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn posted_message_precedes_later_nested_timer_after_a_loaded_task() {
    let mut page = PageSession::open("<body>", "http://127.0.0.1/", "", "", 5000)
        .await
        .unwrap();
    let probe = include_str!("fixtures/window_message_task_clock_probe.js");
    let actual: serde_json::Value =
        serde_json::from_str(&page.eval_json_expression(probe).await.unwrap()).unwrap();
    assert_eq!(
        actual,
        json!({"order":["message","timer"],"message":{"data":"task-clock-message","sourceIsWindow":true,"currentTargetIsChild":true,"originMatches":true,"trusted":true}})
    );
}
