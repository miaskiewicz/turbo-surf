use serde_json::Value;
use turbo_surf_render::PageSession;

async fn probe(poison: bool) -> Value {
    let mut page = PageSession::open(
        "<!doctype html><head></head><body><div id=m><span id=d>logout</span></div></body>",
        "http://127.0.0.1/",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let source = include_str!("fixtures/hover_sheet_snapshot.js")
        .strip_prefix("export ")
        .unwrap();
    serde_json::from_str(
        &page
            .eval_engine(&format!(
                "globalThis.__RESULT = JSON.stringify(({source})({poison}));"
            ))
            .await
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn hover_snapshots_the_live_sheet_list_once() {
    let result = probe(false).await;
    assert_eq!(result["states"], serde_json::json!(["visible", "hidden"]));
    assert!(result["reads"].as_u64().unwrap() <= 1, "{result}");
}

#[tokio::test(flavor = "current_thread")]
async fn hover_ignores_authored_sheet_list_accessors() {
    let result = probe(true).await;
    assert_eq!(result["states"], serde_json::json!(["visible", "hidden"]));
}
