//! Native-engine regression controls; assert the selected pinned engine first.
#![cfg(turbo_surf_native_proxy)]
use turbo_surf_render::PageSession;
fn assert_engine() {
    assert_eq!(
        turbo_surf_render::v8_version(),
        env!("TURBO_SURF_EXPECTED_NATIVE_V8")
    );
}
#[tokio::test(flavor = "current_thread")]
async fn cross_origin_cached_functions_preserve_identity_without_rooting_removed_targets() {
    assert_engine();
    let mut page = PageSession::open("<body>parent", "https://ownership.test/", "", "", 10000)
        .await
        .unwrap();
    let value=page.eval_expression(r#"(async()=>{
        const frame=document.createElement('iframe');frame.setAttribute('sandbox','allow-scripts');
        const ready=new Promise(resolve=>frame.onload=resolve);frame.srcdoc='<body>child';document.body.append(frame);await ready;frame.onload=null;
        const child=frame.contentWindow,location=child.location;
        const first=child.postMessage,second=child.postMessage;
        const getter=Object.getOwnPropertyDescriptor(child,'location').get;
        const repeated=Object.getOwnPropertyDescriptor(child,'location').get;
        let denied;try{child.document}catch(error){denied=error.name}
        globalThis.ownershipRefs=[new WeakRef(child),new WeakRef(first),new WeakRef(location)];
        frame.remove();
        return JSON.stringify([first===second,getter===repeated,getter.call(child)===location,denied]);
    })()"#).await.unwrap();
    assert_eq!(value, r#"[true,true,true,"SecurityError"]"#);
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.eval_expression("JSON.stringify(ownershipRefs.map(reference=>!!reference.deref()))")
            .await
            .unwrap(),
        "[false,false,false]"
    );
}
#[tokio::test(flavor = "current_thread")]
async fn retained_cross_origin_method_keeps_target_until_last_method_reference_is_released() {
    assert_engine();
    let mut page = PageSession::open("<body>", "https://ownership.test/", "", "", 10000)
        .await
        .unwrap();
    page.eval_expression(r#"(async()=>{
        const frame=document.createElement('iframe');frame.setAttribute('sandbox','allow-scripts');
        const ready=new Promise(resolve=>frame.onload=resolve);frame.srcdoc='<body>child';document.body.append(frame);await ready;frame.onload=null;
        const child=frame.contentWindow;globalThis.ownershipRef=new WeakRef(child);globalThis.retainedMethod=child.postMessage;
        frame.remove();return 'ready';
    })()"#).await.unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.eval_expression("String(!!ownershipRef.deref())")
            .await
            .unwrap(),
        "true"
    );
    page.eval_expression("retainedMethod=null;'released'")
        .await
        .unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.eval_expression("String(!!ownershipRef.deref())")
            .await
            .unwrap(),
        "false"
    );
}
#[tokio::test(flavor = "current_thread")]
async fn native_document_getter_routes_receiver_and_current_document_across_navigation() {
    assert_engine();
    let mut page = PageSession::open("<body>parent", "https://ownership.test/", "", "", 10000)
        .await
        .unwrap();
    let value=page.eval_expression(r#"(async()=>{
        const frame=document.createElement('iframe');let ready=new Promise(resolve=>frame.onload=resolve);
        frame.srcdoc='<body>first';document.body.append(frame);await ready;frame.onload=null;
        const child=frame.contentWindow,first=frame.contentDocument;
        const get=Object.getOwnPropertyDescriptor(child,'document').get;
        let forged;try{get.call({})}catch(error){forged=error.name}
        const before=[first!==document,first===child.document,first.body.textContent,get.call(window)===document,forged];
        ready=new Promise(resolve=>frame.onload=resolve);frame.srcdoc='<body>second';await ready;frame.onload=null;
        return JSON.stringify([before,child===frame.contentWindow,first!==frame.contentDocument,first.body.textContent,frame.contentDocument.body.textContent,get.call(child)===frame.contentDocument]);
    })()"#).await.unwrap();
    assert_eq!(
        value,
        r#"[[true,true,"first",true,"TypeError"],true,true,"first","second",true]"#
    );
}
#[tokio::test(flavor = "current_thread")]
async fn cached_cross_origin_methods_are_per_caller_and_do_not_keep_retired_callers_alive() {
    assert_engine();
    let mut page = PageSession::open("<body>", "https://ownership.test/", "", "", 10000)
        .await
        .unwrap();
    let value=page.eval_expression(r#"(async()=>{
      const load=async(opaque)=>{const frame=document.createElement('iframe');if(opaque)frame.setAttribute('sandbox','allow-scripts');const ready=new Promise(resolve=>frame.onload=resolve);frame.srcdoc='<body>child';document.body.append(frame);await ready;frame.onload=null;return frame;};
      const caller=await load(false),target=await load(true),realm=caller.contentWindow,window=target.contentWindow;
      realm.peer=window;
      const main=window.postMessage,read=realm.Function('return peer.postMessage');
      const foreign=read();
      globalThis.retiredCallerArray=new WeakRef(realm.Array);
      const result=[main!==foreign,foreign===read(),Object.getPrototypeOf(foreign)===realm.Function.prototype];
      caller.remove();
      globalThis.liveTarget=target;
      return JSON.stringify(result);
    })()"#).await.unwrap();
    assert_eq!(value, "[true,true,true]");
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.eval_expression("String(!!retiredCallerArray.deref())")
            .await
            .unwrap(),
        "false"
    );
}
