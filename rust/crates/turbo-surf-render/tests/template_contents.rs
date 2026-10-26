//! Native template ownership, parsing, preparation and frame adoption controls.
use turbo_dom_browser_env::test_support;
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn native_template_ownership_and_lifecycle_match_independent_chrome() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://template.test/document.html",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    for (probe, fixture) in [
        (
            test_support::TEMPLATE_OWNERSHIP_PROBE_JS,
            test_support::TEMPLATE_OWNERSHIP_CHROME155_JSON,
        ),
        (
            test_support::TEMPLATE_LIFECYCLE_PROBE_JS,
            test_support::TEMPLATE_LIFECYCLE_CHROME155_JSON,
        ),
    ] {
        let value = page
            .eval_expression(&format!(
                "JSON.stringify({})",
                probe.trim_end().trim_end_matches(';')
            ))
            .await
            .unwrap();
        let actual: serde_json::Value = serde_json::from_str(&value).unwrap();
        let expected: serde_json::Value = serde_json::from_str(fixture).unwrap();
        assert_eq!(actual, expected["result"]);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn native_template_frame_adoption_preserves_content_identity_and_original_units() {
    if !turbo_surf_render::stable_window_proxy() {
        return;
    }
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://template.test/document.html",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let value=page.eval_expression(r#"(async()=>{
      const lone=String.fromCharCode(0xd800),frame=document.createElement('iframe');
      const ready=new Promise((resolve,reject)=>{frame.onload=resolve;frame.onerror=reject;setTimeout(()=>reject(new Error('template frame timeout')),3000)});
      frame.srcdoc='<template id="host"><template><button onclick="return \''+lone+'\';">'+lone+'</button></template></template>';
      document.body.append(frame);await ready;
      const host=frame.contentDocument.querySelector('template'),content=host.content,nested=content.firstChild,button=nested.content.firstChild;
      const old=content.ownerDocument,parent=document.createElement('template'),parentInert=parent.content.ownerDocument;
      document.adoptNode(host);frame.remove();document.body.append(host);
      const result=[host.content===content,content.ownerDocument===parentInert,content.ownerDocument!==old,nested.content.ownerDocument===parentInert,button.ownerDocument===parentInert,host.childNodes.length,nested.childNodes.length,button.textContent.charCodeAt(0),button.onclick===null];
      document.body.append(button);button.setAttribute('onclick',button.getAttribute('onclick'));result.push(button.onclick.call(button,new Event('click')).charCodeAt(0),button.onclick.toString().charCodeAt(button.onclick.toString().indexOf("'")+1));
      return JSON.stringify(result);
    })()"#).await.unwrap();
    assert_eq!(
        value,
        "[true,true,true,true,true,0,0,55296,true,55296,55296]"
    );
}
