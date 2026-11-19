//! Code conversion cannot admit execution after its actual document retires.
use serde_json::{json, Value};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn trusted_script_conversion_rechecks_its_document_after_user_policy_runs() {
    let mut page = PageSession::open_with_response_headers(
        "<!doctype html><body><p id=alive>root-live</p></body>",
        "https://conversion.test/",
        "",
        "",
        5000,
        "",
        Default::default(),
        &[(
            "Content-Security-Policy".into(),
            "script-src 'self' 'unsafe-eval'; require-trusted-types-for 'script'; trusted-types default"
                .into(),
        )],
    )
    .await
    .unwrap();
    let observed: Value = serde_json::from_str(
        &page
            .eval_json_expression(
                r#"(()=>{
                  const frame=document.createElement('iframe');document.body.append(frame);
                  const child=frame.contentWindow;
                  let conversions=0;
                  child.trustedTypes.createPolicy('default',{createScript(source){
                    conversions++;frame.remove();return source;
                  }});
                  let failure;
                  try{child.eval('parent.retiredCompilationRan=true')}catch(error){
                    failure=[error.name,error.message];
                  }
                  return {conversions,detached:!frame.isConnected,failure,
                    ran:globalThis.retiredCompilationRan===true,
                    root:document.querySelector('#alive').textContent};
                })()"#,
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(observed["conversions"], 1);
    assert_eq!(observed["detached"], true);
    assert_eq!(observed["failure"][0], "EvalError");
    assert_eq!(
        observed["failure"][1],
        "Code-generation document changed during conversion"
    );
    assert_eq!(observed["ran"], false);
    assert_eq!(observed["root"], "root-live");
    let recovery: Value = serde_json::from_str(
        &page
            .eval_json_expression(
                r#"(()=>{const frame=document.createElement('iframe');document.body.append(frame);
                  const child=frame.contentWindow;
                  child.trustedTypes.createPolicy('default',{createScript:s=>s});
                  return [child.eval('6*7'),document.querySelector('#alive').textContent];})()"#,
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(recovery, json!([42, "root-live"]));
    page.close();
}
