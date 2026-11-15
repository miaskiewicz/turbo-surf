#[path = "support/popup_contract.rs"]
pub mod fixture;
use serde_json::{json, Value};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn live_base_and_empty_fragments_preserve_the_document_without_http() {
    let server = fixture::Server::start().await;
    let mut page = PageSession::open(
        "<!doctype html><p id=marker>retained</p>",
        &server.url("/start"),
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let actual: Value = serde_json::from_str(
        &page
            .eval_json_expression(
                r#"(async()=>{
      const original=document,marker=document.getElementById('marker'),namespace={};
      globalThis.fragmentNamespace=namespace;
      let reads=0;Object.defineProperty(document,'baseURI',{get(){reads++;throw Error('public base getter');}});
      history.replaceState({kept:true},'', '/folder/');
      const events=[];addEventListener('hashchange',event=>events.push([event.oldURL,event.newURL]));
      const turn=()=>new Promise(resolve=>setTimeout(resolve,0));
      location.hash='';await turn();const initial=events.length;
      const change=operation=>new Promise(resolve=>{addEventListener('hashchange',resolve,{once:true});operation();});
      await change(()=>location.assign('#x'));
      location.hash='#x';await turn();const repeated=events.length;
      await change(()=>{location.hash='';});
      return {events,initial,repeated,url:location.href,same:document===original,
        marker:document.getElementById('marker')===marker,namespace:fragmentNamespace===namespace,reads};
    })()"#,
            )
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        actual,
        json!({
            "events":[
                [server.url("/folder/"),server.url("/folder/#x")],
                [server.url("/folder/#x"),server.url("/folder/#")]
            ],
            "initial":0,"repeated":1,"url":server.url("/folder/#"),
            "same":true,"marker":true,"namespace":true,"reads":0
        })
    );
    assert!(server.requests().is_empty());
    assert!(!page.has_pending_auxiliary_work());
}
