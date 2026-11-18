//! Window.location conversion failures are atomic before native navigation admission.
#[path = "support/popup_contract.rs"]
pub mod fixture;
use serde_json::{json, Value};

#[tokio::test(flavor = "current_thread")]
async fn window_location_conversion_errors_preserve_the_target_and_allow_later_navigation() {
    let server = fixture::Server::start().await;
    let mut page = fixture::open(&server.origin, Default::default()).await;
    fixture::child(&mut page).await;
    fixture::settle(&mut page).await;
    let actual: Value = serde_json::from_str(
        &page.eval_json_expression(r#"(() => {
          const originalDocument=child.document,originalLocation=child.location;
          child.retainedMarker=71;globalThis.rootMarker=93;
          const before=[child.location.href,child.history.length,child.history.state];
          const token=new Error('authored conversion failed'),trace=[];
          const values=[
            {[Symbol.toPrimitive](hint){trace.push('primitive:'+hint);throw token},
              toString(){throw new Error('must not fall back')}},
            {get [Symbol.toPrimitive](){trace.push('getter');throw token}},
            {toString(){trace.push('string');throw token},
              valueOf(){throw new Error('must not reach valueOf')}}
          ];
          const failures=values.map(value=>{
            try{child.location=value;return false}catch(error){return error===token}
          });
          return {failures,trace,
            document:child.document===originalDocument,location:child.location===originalLocation,
            history:JSON.stringify(before)===JSON.stringify([child.location.href,child.history.length,child.history.state]),
            markers:[child.retainedMarker,rootMarker],title:child.document.title};
        })()"#).await.unwrap(),
    ).unwrap();
    assert_eq!(
        actual,
        json!({
            "failures":[true,true,true],"trace":["primitive:string","getter","string"],
            "document":true,"location":true,"history":true,"markers":[71,93],"title":"original"
        })
    );
    assert!(!page.has_pending_auxiliary_work());
    assert!(server.requests().is_empty());
    let accepted: Value = serde_json::from_str(
        &page.eval_json_expression(r#"(() => {
          globalThis.validConversions=[];
          child.location={ [Symbol.toPrimitive](hint){validConversions.push(hint);return '/converted-target'} };
          return validConversions;
        })()"#).await.unwrap(),
    ).unwrap();
    assert_eq!(accepted, json!(["string"]));
    fixture::settle(&mut page).await;
    let recovered: Value = serde_json::from_str(
        &page.eval_json_expression(r#"({url:child.location.href,title:child.document.title,
          oldMarker:typeof child.retainedMarker,rootMarker,body:child.document.getElementById('content').textContent})"#)
            .await.unwrap(),
    ).unwrap();
    assert_eq!(
        recovered,
        json!({"url":server.url("/converted-target"),"title":"converted-target",
        "oldMarker":"undefined","rootMarker":93,"body":"converted-target"})
    );
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "GET");
    assert_eq!(requests[0].target, "/converted-target");
    page.close();
}
