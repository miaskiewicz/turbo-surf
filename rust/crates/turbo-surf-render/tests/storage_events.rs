use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn storage_events_match_source_pinned_chrome() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "http://127.0.0.1:8920/",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let probe = include_str!("../../../../harness/detection/storage-event-probe.js")
        .trim()
        .trim_end_matches(';');
    let result = page.eval(&format!("(async()=>{{globalThis.__RESULT=JSON.stringify(await({probe}));}})().catch(e=>{{globalThis.__RESULT=JSON.stringify({{error:String(e),stack:e.stack}});}})")).await.unwrap();
    let actual: serde_json::Value = serde_json::from_str(&result).unwrap();
    let control: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/storage-event-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, control["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn storage_tasks_preserve_microtask_and_receiver_lifecycle() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "http://127.0.0.1:8920/",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let result = page.eval(r#"(async()=>{
       const frame=document.createElement('iframe');const loaded=new Promise(resolve=>frame.onload=resolve);document.body.append(frame);await loaded;
       const child=frame.contentWindow;globalThis.received=[];
       child.addEventListener('storage',event=>{received.push(event.key);queueMicrotask(()=>received.push('micro:'+event.key));});
       localStorage.setItem('first','one');localStorage.setItem('second','two');
       await new Promise(resolve=>setTimeout(resolve,30));
       const first=received.slice();
       localStorage.setItem('retired','three');frame.remove();
       await new Promise(resolve=>setTimeout(resolve,30));
       globalThis.__RESULT=JSON.stringify({first,last:received});
    })()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result).unwrap(),
        serde_json::json!({"first":["first","micro:first","second","micro:second"],"last":["first","micro:first","second","micro:second"]})
    );
}

#[cfg(turbo_surf_native_proxy)]
#[tokio::test(flavor = "current_thread")]
async fn queued_storage_tasks_do_not_root_retired_receivers() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://storage-lifetime.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval_expression(r#"(()=>{const frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;globalThis.storageEventRealm=new WeakRef(child);localStorage.setItem('queued','value');frame.remove();return 'queued';})()"#).await.unwrap();
    assert!(page.pending_storage_events_for_testing() > 0);
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.eval_expression("storageEventRealm.deref()===undefined")
            .await
            .unwrap(),
        "true"
    );
    assert!(page.pending_storage_events_for_testing() > 0);
    page.eval("globalThis.__RESULT='drained'").await.unwrap();
    assert_eq!(page.pending_storage_events_for_testing(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn storage_delivery_uses_private_intrinsics_and_receiving_area() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "http://127.0.0.1:8920/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let result=page.eval(r#"(async()=>{
      const frame=document.createElement('iframe');const loaded=new Promise(resolve=>frame.onload=resolve);document.body.append(frame);await loaded;
      const child=frame.contentWindow,area=child.localStorage,OriginalEvent=child.StorageEvent;let received;
      child.addEventListener('storage',event=>{received=[event.storageArea===area,event instanceof OriginalEvent,event.key,event.isTrusted]});
      Object.defineProperty(child,'localStorage',{get(){throw new Error('public getter')}});
      child.StorageEvent=function(){throw new Error('public constructor')};child.String.prototype.charCodeAt=function(){throw new Error('public intrinsic')};
      localStorage.setItem('private','value');await new Promise(resolve=>setTimeout(resolve,30));
      frame.remove();globalThis.__RESULT=JSON.stringify(received);
    })()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result).unwrap(),
        serde_json::json!([true, true, "private", true])
    );
}

#[tokio::test(flavor = "current_thread")]
async fn native_storage_task_budget_failure_is_atomic_and_recovers_after_delivery() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "http://127.0.0.1:8920/",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    let result=page.eval(r#"(async()=>{
      const frames=[];
      for(let i=0;i<2;i++){const frame=document.createElement('iframe');const loaded=new Promise(resolve=>frame.onload=resolve);document.body.append(frame);await loaded;frames.push(frame);}
      const a='a'.repeat(1024*1024),b='b'.repeat(1024*1024);let last=null,error='none';
      for(let index=0;index<40;index++){const next=index%2?a:b;try{localStorage.setItem('budget',next);last=next;}catch(e){error=e.name;break;}}
      const atomic=localStorage.getItem('budget')===last;
      await new Promise(resolve=>setTimeout(resolve,30));
      localStorage.setItem('budget','recovered');const recovered=localStorage.getItem('budget');
      for(const frame of frames)frame.remove();
      globalThis.__RESULT=JSON.stringify({error,atomic,recovered});
    })()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result).unwrap(),
        serde_json::json!({"error":"RangeError","atomic":true,"recovered":"recovered"})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn trusted_storage_delivery_does_not_read_polluted_object_prototypes() {
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "http://127.0.0.1:8920/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let result=page.eval(r#"(async()=>{
      const frame=document.createElement('iframe');const loaded=new Promise(resolve=>frame.onload=resolve);document.body.append(frame);await loaded;
      const child=frame.contentWindow;let calls=0,hooks=0;
      child.addEventListener('storage',event=>{if(event.key==='polluted'&&!event.bubbles&&!event.cancelable&&event.isTrusted)calls++;});
      for(const name of ['bubbles','cancelable','composed','localStorage','sessionStorage'])Object.defineProperty(child.Object.prototype,name,{configurable:true,get(){hooks++;throw new Error('private inherited getter '+name)}});
      localStorage.setItem('polluted','value');await new Promise(resolve=>setTimeout(resolve,30));
      for(const name of ['bubbles','cancelable','composed','localStorage','sessionStorage'])delete child.Object.prototype[name];
      frame.remove();globalThis.__RESULT=JSON.stringify({calls,hooks});
    })()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result).unwrap(),
        serde_json::json!({"calls":1,"hooks":0})
    );
}
