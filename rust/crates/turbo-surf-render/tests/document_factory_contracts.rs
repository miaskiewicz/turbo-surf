use turbo_dom_browser_env::test_support;
use turbo_surf_render::PageSession;

async fn assert_control(probe: &str, fixture: &str) {
    let mut page = PageSession::open(
        "<!doctype html><html><head></head><body></body></html>",
        "https://factories.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let source = format!("JSON.stringify({})", probe.trim().trim_end_matches(';'));
    let actual = page.eval_expression(&source).await.unwrap();
    page.close();
    let actual: serde_json::Value = serde_json::from_str(&actual).unwrap();
    let expected: serde_json::Value = serde_json::from_str(fixture).unwrap();
    assert_eq!(actual, expected["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn document_factories_match_chrome_with_real_private_inert_runtime_state() {
    assert_control(
        test_support::DOM_IMPLEMENTATION_PROBE_JS,
        test_support::DOM_IMPLEMENTATION_CHROME155_JSON,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn xml_local_names_and_namespaces_match_chrome_through_host_factory_ops() {
    assert_control(
        test_support::XML_FACTORY_ELEMENTS_PROBE_JS,
        test_support::XML_FACTORY_ELEMENTS_CHROME155_JSON,
    )
    .await;
}
