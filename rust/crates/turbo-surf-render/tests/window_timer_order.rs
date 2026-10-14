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
