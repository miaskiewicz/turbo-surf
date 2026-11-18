//! Sandboxed form targets preserve unrelated documents while owned actions progress.
#[path = "support/popup_contract.rs"]
pub mod fixture;
use serde_json::{json, Value};
use turbo_surf_render::PageSession;

const TREE: &str = r#"(async()=>{
  const frame=async(doc,name,sandbox)=>{
    const node=doc.createElement('iframe');node.name=name;
    if(sandbox!==undefined)node.setAttribute('sandbox',sandbox);
    const loaded=new Promise(resolve=>node.addEventListener('load',resolve,{once:true}));
    node.srcdoc='<!doctype html><body>retained';doc.body.append(node);await loaded;return node;
  };
  globalThis.sourceHost=await frame(document,'source','allow-scripts allow-same-origin allow-forms');
  globalThis.descendantHost=await frame(sourceHost.contentDocument,'descendant');
  globalThis.siblingHost=await frame(document,'sibling');
  globalThis.originalDocuments=[document,sourceHost.contentDocument,descendantHost.contentDocument,siblingHost.contentDocument];
  globalThis.formMarker=0;
  for(const host of [sourceHost,descendantHost,siblingHost])host.contentWindow.formMarker=0;
  globalThis.submitForm=(target,action)=>{
    const form=sourceHost.contentDocument.createElement('form');
    form.target=target;form.action=action;sourceHost.contentDocument.body.append(form);
    try{form.submit();}finally{form.remove();}
  };
  return originalDocuments.length;
})()"#;

#[tokio::test(flavor = "current_thread")]
async fn sandboxed_form_targets_preserve_siblings_and_ancestors_after_owned_work_settles() {
    // Chrome156 probe 8ebf9c73 independently records the same per-target side effects.
    // The native contract uses public eval's work drain, not a timeout as a denial oracle.
    for (target, expected_markers) in [
        ("_self", [0, 1, 0, 0]),
        ("descendant", [0, 0, 1, 0]),
        ("sibling", [0, 0, 0, 0]),
        ("_parent", [0, 0, 0, 0]),
    ] {
        let server = fixture::Server::start().await;
        let mut page = PageSession::open(
            "<!doctype html><title>Root retained</title><body>root retained",
            &server.url("/root"),
            "",
            "",
            5000,
        )
        .await
        .unwrap();
        assert_eq!(page.eval_json_expression(TREE).await.unwrap(), "4");
        page.eval(&format!(
            "submitForm({},'javascript:void(globalThis.formMarker++)');globalThis.submissionReturned=true",
            serde_json::to_string(target).unwrap(),
        )).await.unwrap();
        assert_eq!(
            page.eval_json_expression("new Promise(resolve=>{sourceHost.contentWindow.positiveCompletion=()=>resolve(true);submitForm('_self','javascript:void(globalThis.positiveCompletion())')})")
                .await.unwrap(),
            "true",
            "{target}: subsequent allowed action must actually complete"
        );
        page.eval("void 0").await.unwrap();
        let observed: Value = serde_json::from_str(
            &page.eval_json_expression(r#"({
              returned:submissionReturned,
              markers:[formMarker,sourceHost.contentWindow.formMarker,descendantHost.contentWindow.formMarker,siblingHost.contentWindow.formMarker],
              retained:originalDocuments.map((doc,index)=>doc===[document,sourceHost.contentDocument,descendantHost.contentDocument,siblingHost.contentDocument][index]),
              hrefs:[location.href,sourceHost.contentWindow.location.href,descendantHost.contentWindow.location.href,siblingHost.contentWindow.location.href],
              rootTitle:document.title,sourceText:sourceHost.contentDocument.body.firstChild.textContent,siblingText:siblingHost.contentDocument.body.textContent
            })"#).await.unwrap(),
        ).unwrap();
        assert_eq!(
            observed,
            json!({
                "returned":true,"markers":expected_markers,"retained":[true,true,true,true],
                "hrefs":[server.url("/root"),"about:srcdoc","about:srcdoc","about:srcdoc"],
                "rootTitle":"Root retained","sourceText":"retained","siblingText":"retained"
            }),
            "{target}"
        );
        assert!(
            !page.has_pending_auxiliary_work(),
            "{target}: no HTTP navigation admitted"
        );
        assert!(
            server.requests().is_empty(),
            "{target}: unexpected wire request"
        );
        page.close();
    }
}
