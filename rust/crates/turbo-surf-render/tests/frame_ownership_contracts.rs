//! Authored Chrome controls keep iframe lifecycle data behind native capabilities.
use serde_json::Value;

async fn collect(source: &str) -> Value {
    let source = source.trim().trim_end_matches(';');
    let source = format!("(async()=>{{try{{globalThis.__RESULT=await ({source});}}catch(error){{globalThis.__RESULT=JSON.stringify({{collectorError:error.name,message:error.message,stack:error.stack}});}}}})()");
    let result = turbo_surf_render::eval_async(
        "<!doctype html><html><head></head><body>Authored iframe ownership control</body></html>",
        "https://frames.test/",
        &source,
    )
    .await
    .unwrap();
    serde_json::from_str(&result).unwrap_or_else(|_| panic!("invalid iframe result: {result}"))
}

#[tokio::test(flavor = "current_thread")]
async fn private_iframe_lifecycle_ignores_page_maps_queries_and_namespace_helpers() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/frame-ownership-chrome155.json"
    ))
    .unwrap();
    let actual = collect(include_str!(
        "../../../../harness/detection/frame-ownership-probe.js"
    ))
    .await;
    assert_eq!(actual, fixture["ownership"]);
}

#[tokio::test(flavor = "current_thread")]
async fn implicit_foreign_document_adoption_retires_old_window_and_preserves_borrowed_accessors() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/frame-ownership-chrome155.json"
    ))
    .unwrap();
    let actual = collect(include_str!(
        "../../../../harness/detection/frame-adoption-probe.js"
    ))
    .await;
    assert_eq!(actual, fixture["adoption"]);
}

#[tokio::test(flavor = "current_thread")]
async fn native_iframe_source_resolution_ignores_page_url_query_and_attribute_overrides() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/frame-ownership-chrome155.json"
    ))
    .unwrap();
    let actual = collect(include_str!(
        "../../../../harness/detection/frame-native-base-probe.js"
    ))
    .await;
    assert_eq!(actual, fixture["baseResolution"]);
}

#[tokio::test(flavor = "current_thread")]
async fn inactive_parsed_document_has_no_iframe_browsing_context() {
    let actual = collect(
        r#"(()=>{
      const parsed = new DOMParser().parseFromString('<body><iframe></iframe>','text/html');
      const frame = parsed.querySelector('iframe');
      const detached = parsed.createElement('iframe');
      return JSON.stringify({owner:parsed.defaultView===null,window:frame.contentWindow===null,
        document:frame.contentDocument===null,detached:detached.contentWindow===null});
    })()"#,
    )
    .await;
    assert_eq!(
        actual,
        serde_json::json!({"owner":true,"window":true,"document":true,"detached":true})
    );
}
