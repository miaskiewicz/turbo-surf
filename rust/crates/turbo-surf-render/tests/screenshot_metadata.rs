use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn screenshot_metadata_reads_native_load_state_despite_guest_getter_overrides() {
    let source =
        "data:image/svg+xml,<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"7\" height=\"5\"/>";
    let html = format!("<img src='{source}'>");
    let mut page = PageSession::open(&html, "http://fixture.test/", "", "", 10_000)
        .await
        .unwrap();
    page.eval_expression("Object.defineProperty(HTMLImageElement.prototype,'naturalWidth',{configurable:true,get(){return 999}})").await.unwrap();
    let result = page
        .eval_engine("globalThis.__RESULT = JSON.stringify(__tsNs.screenshotImageMetadata());")
        .await
        .unwrap();
    let metadata: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(metadata[source]["width"], 7);
    assert_eq!(metadata[source]["height"], 5);
    assert_eq!(
        page.eval_engine(
            "globalThis.__RESULT = JSON.stringify(__tsNs.clearScreenshotImageMetadata());"
        )
        .await
        .unwrap(),
        "{}"
    );
    assert_eq!(
        page.eval_engine("globalThis.__RESULT = JSON.stringify(__tsNs.screenshotImageMetadata());")
            .await
            .unwrap(),
        "{}"
    );
    assert_eq!(
        page.eval_expression("typeof globalThis.__tsNs")
            .await
            .unwrap(),
        "undefined"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn screenshot_metadata_preserves_ratio_only_svg_dependencies() {
    let source =
        "data:image/svg+xml,<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 3 2\"/>";
    let mut page = PageSession::open(
        &format!("<img src='{source}'>"),
        "http://fixture.test/",
        "",
        "",
        10_000,
    )
    .await
    .unwrap();
    let result = page
        .eval_engine("globalThis.__RESULT = JSON.stringify(__tsNs.screenshotImageMetadata());")
        .await
        .unwrap();
    let metadata: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(metadata[source]["naturalRatio"], 1.5);
    assert!(metadata[source]["width"].as_u64().unwrap() > 0);
    let mut empty = PageSession::open("<body>", "http://fixture.test/", "", "", 10_000)
        .await
        .unwrap();
    assert_eq!(
        empty
            .eval_engine("globalThis.__RESULT = JSON.stringify(__tsNs.screenshotImageMetadata());")
            .await
            .unwrap(),
        "{}"
    );
}
