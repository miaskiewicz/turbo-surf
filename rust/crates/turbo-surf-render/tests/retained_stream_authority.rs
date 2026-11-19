use serde_json::{json, Value};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn borrowed_stream_controller_methods_preserve_retained_receiver_realm() {
    let mut page = PageSession::open(
        "<body><iframe id=f></iframe>",
        "https://retained-stream.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let result = page.eval_json_expression(r#"(()=>{
      const frame=document.getElementById('f'),child=frame.contentWindow;
      const source=new child.ReadableStream({start(controller){child.controller=controller;}});
      const controller=child.controller;
      frame.remove();
      ReadableStreamDefaultController.prototype.enqueue.call(controller,7);
      const size=Object.getOwnPropertyDescriptor(ReadableStreamDefaultController.prototype,'desiredSize').get.call(controller);
      ReadableStreamDefaultController.prototype.close.call(controller);
      let error=null;
      try{ReadableStreamDefaultController.prototype.enqueue.call(controller,8);}
      catch(value){error={name:value.name,rootTypeError:value instanceof TypeError,childTypeError:value instanceof child.TypeError,rootPrototype:Object.getPrototypeOf(value)===TypeError.prototype,childPrototype:Object.getPrototypeOf(value)===child.TypeError.prototype};}
      return {size,error,locked:source.locked};
    })()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&result).unwrap(),
        json!({"size":0,"error":{"name":"TypeError","rootTypeError":true,"childTypeError":false,"rootPrototype":true,"childPrototype":false},"locked":false})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn retained_stream_dependency_does_not_readmit_retired_document_tasks() {
    use tokio::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}/", listener.local_addr().unwrap());
    let mut page = PageSession::open("<body><iframe id=f></iframe>", &origin, "", "", 5000)
        .await
        .unwrap();
    let result = page
        .eval_json_expression(
            r#"(async()=>{
      const frame=document.getElementById('f'),child=frame.contentWindow;
      let markStarted;
      const started=new Promise(resolve=>{markStarted=resolve;});
      const effects={values:[],timer:false,idb:false,started:markStarted};child.__effects=effects;
      const source=new child.WritableStream({write:new child.Function('value',`
        __effects.values.push(value);
        setTimeout(()=>{__effects.timer=true;},0);
        try{indexedDB.open('retired-stream').onsuccess=()=>{__effects.idb=true;};}
        catch(error){__effects.idbError=error.name;}
        try{location.assign('/forbidden-retired-navigation');}catch(error){}
        __effects.fetch=fetch('/forbidden-retired-fetch').then(()=> 'unexpected',error=>error.name);
        __effects.started();
      `)});
      const target=structuredClone(source,{transfer:[source]}),writer=target.getWriter();
      frame.remove();
      await writer.write(43);
      await started;
      const fetchError=await effects.fetch;
      await new Promise(resolve=>setTimeout(resolve,20));
      return {values:effects.values,timer:effects.timer,idb:effects.idb,fetchError};
    })()"#,
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&result).unwrap(),
        json!({"values":[43],"timer":false,"idb":false,"fetchError":"TypeError"})
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), listener.accept())
            .await
            .is_err(),
        "Retired stream callback admitted an HTTP request"
    );
    assert!(!page.drain_network_log().contains("forbidden-retired"));
}

#[tokio::test(flavor = "current_thread")]
async fn completed_retained_stream_operations_release_the_retired_frontend() {
    let mut page = PageSession::open(
        "<body><iframe id=f></iframe>",
        "https://retained-stream-gc.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let prepared = page.eval_json_expression(r#"(async()=>{
      const frame=document.getElementById('f'),child=frame.contentWindow;
      globalThis.retainedCloseSeen=false;
      const sourceClosed=new Promise(resolve=>{globalThis.retainedCloseResolve=resolve;});
      const source=new child.WritableStream({write(){},close(){globalThis.retainedCloseSeen=true;globalThis.retainedCloseResolve();}});
      const target=structuredClone(source,{transfer:[source]}),writer=target.getWriter();
      globalThis.retainedStreamRefs=[new WeakRef(child),new WeakRef(source),new WeakRef(target),new WeakRef(writer)];
      frame.remove();await writer.write(7);await writer.close();
      // Transport acknowledgement precedes the actual source sink's close.
      await sourceClosed;delete globalThis.retainedCloseResolve;
      return {prepared:'prepared',sourceClosed:retainedCloseSeen};
    })()"#).await.unwrap();
    let prepared: Value = serde_json::from_str(&prepared).unwrap();
    eprintln!("retained stream close phase BEFORE original 3 GC: {prepared}");
    assert_eq!(prepared, json!({"prepared":"prepared","sourceClosed":true}));
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    eprintln!(
        "retained stream actual realm roles after original 3 GC: {}",
        page.context_registry_diagnostics_for_testing()
    );
    assert_eq!(
        page.eval_expression(
            "JSON.stringify(retainedStreamRefs.map(ref=>ref.deref()===undefined))"
        )
        .await
        .unwrap(),
        "[true,true,true,true]"
    );
}
