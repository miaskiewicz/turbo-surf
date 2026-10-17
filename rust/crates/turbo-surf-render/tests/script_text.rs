//! Real script child-text reflection drives the native preparation pipeline.
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn native_script_text_execution_matches_independent_chrome_control() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://script-text.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let value = page
        .eval_expression(&format!(
            "JSON.stringify({})",
            include_str!("fixtures/script_text_execution_probe.js")
                .trim_end()
                .trim_end_matches(';')
        ))
        .await
        .unwrap();
    let actual: serde_json::Value = serde_json::from_str(&value).unwrap();
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/script_text_execution_chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, expected["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn native_script_text_compiles_original_units_and_keeps_started_state() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://script-text.test/source.html",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let value=page.eval_expression(r#"(()=>{
      const lone=String.fromCharCode(0xd800),script=document.createElement('script');
      script.text="globalThis.exactScriptText='"+lone+"';globalThis.scriptTextFunction=function original(){return '"+lone+"'};globalThis.scriptTextRuns=(globalThis.scriptTextRuns||0)+1";
      document.body.append(script);const source=scriptTextFunction.toString();
      script.text='globalThis.scriptTextRuns=100';script.remove();document.body.append(script);
      return JSON.stringify([exactScriptText.charCodeAt(0),scriptTextFunction().charCodeAt(0),source.charCodeAt(source.indexOf("'")+1),scriptTextRuns,script.childNodes.length,Object.hasOwn(script,'text')]);
    })()"#).await.unwrap();
    assert_eq!(value, "[55296,55296,55296,1,1,false]");
}
