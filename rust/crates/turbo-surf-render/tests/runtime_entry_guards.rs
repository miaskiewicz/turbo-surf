use turbo_surf_render::PageSession;

async fn page() -> PageSession {
    PageSession::open(
        "<body><p id=alive>live</p></body>",
        "https://entry.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn synchronous_driver_read_rejects_promises_without_losing_document() {
    let mut page = page().await;
    assert_eq!(
        page.read_engine("return Promise.resolve(42)").unwrap_err(),
        "Engine read must return a synchronous value"
    );
    assert_eq!(
        page.read_engine("return document.querySelector('#alive').textContent")
            .unwrap(),
        "\"live\""
    );
}

#[tokio::test(flavor = "current_thread")]
async fn driver_json_failures_leave_the_same_native_document_usable() {
    let mut page = page().await;
    let cycle = page
        .read_engine("const value={};value.self=value;return value")
        .unwrap_err();
    assert!(cycle.contains("circular"), "{cycle}");
    let bigint = page.read_engine("return 1n").unwrap_err();
    assert!(bigint.contains("BigInt"), "{bigint}");
    assert_eq!(
        page.read_engine("return undefined").unwrap_err(),
        "Expression result is not JSON serializable"
    );
    assert_eq!(
        page.read_engine("return document.querySelectorAll('#alive').length")
            .unwrap(),
        "1"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn failed_driver_execution_does_not_poison_the_next_collection() {
    let mut page = page().await;
    let error = page
        .read_engine("throw new Error('driver-read-failed')")
        .unwrap_err();
    assert!(error.contains("driver-read-failed"), "{error}");
    let error = page
        .collect_engine("throw new Error('driver-collect-failed')")
        .await
        .unwrap_err();
    assert!(error.contains("driver-collect-failed"), "{error}");
    assert_eq!(
        page.collect_engine("globalThis.__RESULT=document.querySelector('#alive').textContent")
            .await
            .unwrap(),
        "live"
    );
}
