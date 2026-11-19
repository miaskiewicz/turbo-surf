//! A fired per-document resource consumer must not root its removed native realm.
use serde_json::Value;
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn fired_resource_consumer_does_not_retain_removed_frame_document_or_window() {
    let mut page = PageSession::open(
        "<body><p id='healthy'>root</p>",
        "https://ownership.test/",
        "",
        "",
        60_000,
    )
    .await
    .unwrap();
    let prepared = page.eval_json_expression(r#"(async()=>{
      const frame=document.createElement('iframe');
      const ready=new Promise(resolve=>frame.addEventListener('load',resolve,{once:true}));
      frame.srcdoc='<body>';document.body.append(frame);await ready;
      const child=frame.contentWindow,doc=child.document;
      const image=doc.createElement('img');
      const loaded=new Promise((resolve,reject)=>{
        image.addEventListener('load',()=>resolve(true),{once:true});
        image.addEventListener('error',()=>reject(Error('resource fixture failed')),{once:true});
      });
      doc.body.append(image);
      image.setAttribute('src','data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAMAAAACCAYAAACddGYaAAAAEUlEQVR4nGP4z8DwH4YZkDkAm34L9XKwuTwAAAAASUVORK5CYII=');
      await loaded;
      const actual={complete:image.complete,width:image.naturalWidth,height:image.naturalHeight};
      globalThis.resourceConsumerWeakRefs=[new WeakRef(child),new WeakRef(doc),new WeakRef(image)];
      frame.remove();
      return actual;
    })()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&prepared).unwrap(),
        serde_json::json!({"complete":true,"width":3,"height":2})
    );
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    let result=page.eval_json_expression("({collected:resourceConsumerWeakRefs.map(ref=>ref.deref()===undefined),root:document.querySelector('#healthy').textContent,guestGc:typeof gc})").await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&result).unwrap(),
        serde_json::json!({"collected":[true,true,true],"root":"root","guestGc":"undefined"})
    );
}
