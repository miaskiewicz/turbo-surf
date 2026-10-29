//! Host-side session snapshots: the installed response's headers as delivered, and a
//! V8 heap snapshot (the format DevTools loads) of the page's isolate.
use turbo_surf_core::request_context::ClientHints;
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn response_headers_and_heap_snapshot_describe_the_live_page() {
    let headers = vec![
        (
            "Content-Type".to_string(),
            "text/html; charset=utf-8".to_string(),
        ),
        ("X-Trace".to_string(), "abc".to_string()),
    ];
    let mut page = PageSession::open_with_response_headers(
        "<body><script>globalThis.retainedMarker={label:'surf-heap-marker'}</script>",
        "https://snapshots.test/",
        "",
        "",
        5000,
        "",
        ClientHints::default(),
        &headers,
    )
    .await
    .unwrap();
    assert_eq!(page.response_headers_snapshot(), headers);
    let snapshot: serde_json::Value =
        serde_json::from_slice(&page.native_heap_snapshot_for_testing()).unwrap();
    let meta = &snapshot["snapshot"]["meta"];
    assert!(meta["node_fields"].as_array().unwrap().len() > 3, "{meta}");
    assert!(snapshot["nodes"].as_array().unwrap().len() > 1000);
    let strings = snapshot["strings"].as_array().unwrap();
    assert!(strings.iter().any(|value| value == "surf-heap-marker"));
}
