//! Offline tests for the diagnostic init hook and shared interface tracer.
use turbo_surf_render::PageSession;

const TRACE: &str = include_str!("../../../../harness/detection/interface-trace.js");
const INPUT_STATE: &str = include_str!("../../../../harness/detection/input-state.js");

#[tokio::test(flavor = "current_thread")]
async fn query_diagnostics_skip_hidden_fields_before_and_after_real_controls() {
    let mut page = PageSession::open("<body></body>", "https://example.test/", "", "", 5000)
        .await
        .unwrap();
    let expression = format!(
        r#"(() => {{
          const captureInputState = {INPUT_STATE}
          const cases = [
            '<input type="HIDDEN" name="q" value="fixture-hidden"><input name="q" value="rust lang">',
            '<input name="q" value="rust lang"><input type="hidden" name="q" value="fixture-hidden">',
            '<input type="hidden" name="q" value="fixture-hidden"><textarea name="q">rust lang</textarea>',
            '<textarea name="q">rust lang</textarea><input type="hidden" name="q" value="fixture-hidden">',
            '<input type="hidden" name="q" value="fixture-hidden">',
            ''
          ];
          return cases.map(html => {{
            document.body.innerHTML = html;
            return captureInputState(document, '#W0wltc');
          }});
        }})()"#
    );
    let raw = page.eval_json_expression(&expression).await.unwrap();
    let states: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(states.as_array().unwrap().len(), 6);
    for (index, state) in states.as_array().unwrap().iter().enumerate() {
        assert_eq!(state["selection"], "first-q-control-with-non-hidden-type");
        assert_eq!(state["consentPresent"], false);
        if index < 4 {
            assert_eq!(state["value"], "rust lang");
            assert_eq!(state["queryControlCount"], 2);
            assert_eq!(state["excludedHiddenQueryControls"], 1);
            assert_eq!(
                state["controlTag"],
                if index < 2 { "INPUT" } else { "TEXTAREA" }
            );
            // textarea.type is the Chrome-parity "textarea" getter (input_members).
            assert_eq!(
                state["controlType"],
                if index < 2 { "text" } else { "textarea" }
            );
        } else {
            assert!(state["value"].is_null());
            assert!(state["controlTag"].is_null());
            assert!(state["controlType"].is_null());
            assert_eq!(state["queryControlCount"], usize::from(index == 4));
            assert_eq!(
                state["excludedHiddenQueryControls"],
                usize::from(index == 4)
            );
        }
    }
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn page_cannot_read_or_overwrite_http_only_cookies() {
    let mut jar = turbo_surf_core::cookies::CookieJar::new();
    jar.set_from_response(
        "https://example.test/",
        &[
            "secret=server; HttpOnly; Path=/".into(),
            "visible=1; Path=/".into(),
            "expired=old; Expires=Sat, 01 Jan 2000 00:00:00 GMT; Path=/".into(),
        ],
        0.0,
    );
    let mut page = PageSession::open(
        "<body></body>",
        "https://example.test/",
        &jar.storage_state(),
        "",
        5000,
    )
    .await
    .unwrap();
    let value=page.eval("document.cookie='secret=guest; Path=/'; document.cookie='fake=x; HttpOnly'; globalThis.__RESULT=document.cookie;").await.unwrap();
    assert_eq!(value, "visible=1");
    let jar = turbo_surf_core::cookies::CookieJar::from_storage_state(&page.cookies());
    let headers = jar.cookie_header("https://example.test/", 0.0);
    assert!(headers.contains("secret=server"));
    assert!(!headers.contains("fake="));
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn trace_runs_before_page_scripts_and_preserves_receivers_and_exceptions() {
    let html = r#"<html><body><script>
      const ua=navigator.userAgent;
      const c=document.createElement('canvas'); c.getContext('2d');
      document.body.setAttribute('data-ua',ua);
      document.body.setAttribute('data-query',String(document.querySelector('body')===document.body));
    </script></body></html>"#;
    let mut page =
        PageSession::open_with_init_script(html, "https://example.test/", "", "", 5000, TRACE)
            .await
            .unwrap();
    let raw=page.eval("globalThis.__RESULT=JSON.stringify(globalThis[Symbol.for('turbo-surf.interface-trace')].snapshot());").await.unwrap();
    let report: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(report["accesses"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["name"] == "Navigator.userAgent"));
    assert!(report["accesses"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["name"] == "document.createElement" || a["name"] == "Document.createElement"));
    assert!(page.serialize().contains("data-query=\"true\""));
    assert!(page.serialize().contains("data-ua=\"Mozilla/"));
    assert!(report["installed"].as_array().unwrap().len() > 10);
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn init_failure_does_not_run_page_code_or_leave_binding_alive() {
    assert!(PageSession::open_with_init_script(
        "<body></body>",
        "https://example.test/",
        "",
        "",
        5000,
        "throw Error('bad init');"
    )
    .await
    .is_err());
    let page = PageSession::open("<body>fresh</body>", "https://example.test/", "", "", 5000)
        .await
        .unwrap();
    assert!(page.serialize().contains("fresh"));
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn classic_scripts_have_document_locations_and_no_dispatch_stack() {
    let html = r#"<html><body>
<script>
  let lexical = 41;
  globalThis.scriptStack = new Error('origin').stack;
  globalThis.scriptElement = document.currentScript;
</script>
<script>
  globalThis.lexicalResult = lexical + 1;
  globalThis.functionSource = Function.prototype.toString.call(function pageFunction() {});
</script>
<script>throw Error('isolated');</script>
<script>globalThis.afterError = true;</script>
</body></html>"#;
    let mut page = PageSession::open(html, "https://example.test/path", "", "", 5000)
        .await
        .unwrap();
    let raw = page.eval("globalThis.__RESULT = JSON.stringify({stack:scriptStack,result:lexicalResult,after:afterError,source:functionSource,element:scriptElement.tagName,current:document.currentScript===null});").await.unwrap();
    let data: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(
        data["stack"]
            .as_str()
            .unwrap()
            .contains("https://example.test/path:4:"),
        "{data}"
    );
    for wrapper in ["eval at", "execScriptEl", "<env>", "<hydrate>", "op_queue"] {
        assert!(!data["stack"].as_str().unwrap().contains(wrapper), "{data}");
    }
    assert_eq!(data["result"], 42);
    assert_eq!(data["after"], true);
    assert_eq!(data["element"], "SCRIPT");
    assert_eq!(data["current"], true);
    assert!(data["source"]
        .as_str()
        .unwrap()
        .contains("function pageFunction()"));
    page.close();
}

#[tokio::test(flavor = "current_thread")]
async fn high_entropy_js_uses_the_same_profile_as_request_headers() {
    let profile = turbo_surf_core::fingerprint::default_profile();
    let mut page = PageSession::open("<body></body>", "https://example.test/", "", "", 5000)
        .await
        .unwrap();
    let raw = page.eval("navigator.userAgentData.getHighEntropyValues(['architecture','platformVersion','uaFullVersion']).then(v=>{globalThis.__RESULT=JSON.stringify(v)});").await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(value["architecture"], profile.architecture());
    assert_eq!(value["platformVersion"], profile.platform_version());
    assert_eq!(value["uaFullVersion"], profile.full_version);
    assert!(value.get("bitness").is_none());
    assert!(value.get("fullVersionList").is_none());
    page.close();
}
