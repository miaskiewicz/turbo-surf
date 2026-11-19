//! Driver synchronization follows native changes, including changes outside the main tree.
use serde_json::Value;
use turbo_surf_render::PageSession;

async fn tracked(page: &mut PageSession, expression: &str) -> Value {
    serde_json::from_str(&page.eval_expression_tracked(expression).await.unwrap()).unwrap()
}

fn acknowledge(page: &mut PageSession) {
    page.serialize_for_driver();
    page.cookies_for_driver();
}

#[tokio::test(flavor = "current_thread")]
async fn retained_collectors_skip_only_acknowledged_native_state() {
    let mut page = PageSession::open("<h1>before</h1>", "https://example.test/", "", "", 60_000)
        .await
        .unwrap();
    assert_eq!(tracked(&mut page, "'first'").await["needsSync"], true);
    acknowledge(&mut page);
    let read = tracked(&mut page, "document.querySelector('h1').textContent").await;
    assert_eq!(read["value"], "before");
    assert_eq!(read["needsSync"], false);
    assert_eq!(
        tracked(
            &mut page,
            "document.querySelector('h1').textContent='after'"
        )
        .await["needsSync"],
        true
    );
    // Unacknowledged mutations stay dirty across a subsequent read-only collector.
    assert_eq!(tracked(&mut page, "'read'").await["needsSync"], true);
    acknowledge(&mut page);
    assert_eq!(tracked(&mut page, "'read'").await["needsSync"], false);
    assert_eq!(
        tracked(&mut page, "document.cookie='only=cookie;path=/'").await["needsSync"],
        true
    );
    acknowledge(&mut page);
    assert_eq!(tracked(&mut page, "'read'").await["needsSync"], false);
    assert_eq!(
        tracked(&mut page, "history.pushState({},'',location.href)").await["needsSync"],
        true
    );
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn child_document_changes_invalidate_without_main_tree_writes() {
    let mut page = PageSession::open(
        "<iframe srcdoc='<p>before</p>'></iframe>",
        "https://example.test/",
        "",
        "",
        60_000,
    )
    .await
    .unwrap();
    acknowledge(&mut page);
    assert_eq!(
        tracked(
            &mut page,
            "frames[0].document.querySelector('p').textContent='after'"
        )
        .await["needsSync"],
        true
    );
    acknowledge(&mut page);
    assert_eq!(tracked(&mut page, "'read'").await["needsSync"], false);
    page.close();
}
