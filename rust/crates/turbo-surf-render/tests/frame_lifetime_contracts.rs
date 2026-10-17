use serde_json::{json, Value};

#[tokio::test(flavor = "current_thread")]
async fn removed_window_still_serializes_transfers_before_discarding_delivery() {
    let mut page = turbo_surf_render::PageSession::open(
        "<iframe id=f></iframe>",
        "https://lifetime.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let raw=page.eval_expression(r#"(()=>{const frame=document.getElementById('f'),child=frame.contentWindow,buffer=new ArrayBuffer(4);frame.remove();let error=null;try{child.postMessage({buffer},'*',[buffer])}catch(caught){error=caught.name;}return JSON.stringify({bytes:buffer.byteLength,error});})()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        json!({"bytes":0,"error":null})
    );
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["queuedWindowMessages"],
        0
    );
}

#[tokio::test(flavor = "current_thread")]
async fn removal_releases_native_context_roots_and_pending_window_transfer_snapshots_immediately() {
    let mut page = turbo_surf_render::PageSession::open(
        "<iframe id=f></iframe>",
        "https://lifetime.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval_expression(r#"(()=>{const frame=document.getElementById('f'),child=frame.contentWindow,channel=new MessageChannel(),buffer=new ArrayBuffer(4096);globalThis.survivingPeer=channel.port1;child.postMessage({buffer,port:channel.port2},'*',[buffer,channel.port2]);frame.remove();return String(buffer.byteLength);})()"#).await.unwrap();
    let state = page.frame_lifecycle_diagnostics_for_testing();
    assert_eq!(
        state,
        json!({"hostContexts":0,"retiredHostContexts":0,"queuedWindowMessages":0,"queuedWindowBytes":0})
    );
    let state = page.message_port_diagnostics_for_testing();
    assert_eq!(state["queuedMessages"], 0);
    assert_eq!(state["queuedBytes"], 0);
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(page.message_port_diagnostics_for_testing()["endpoints"], 1);
}

#[tokio::test(flavor = "current_thread")]
async fn parent_window_messages_survive_child_retirement_and_detached_hosts_reconnect() {
    let mut page = turbo_surf_render::PageSession::open(
        "<iframe id=f></iframe>",
        "https://lifetime.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval_expression(r#"(()=>{const frame=document.getElementById('f'),child=frame.contentWindow;globalThis.lifetimeEvents=[];addEventListener('message',event=>lifetimeEvents.push({data:event.data,source:event.source!==null}));child.eval("parent.postMessage('before-removal','*')");frame.remove();globalThis.detachedFrame=frame;return 'prepared';})()"#).await.unwrap();
    page.eval("globalThis.__RESULT='pump';").await.unwrap();
    let raw = page
        .eval_expression("JSON.stringify(lifetimeEvents)")
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        json!([{"data":"before-removal","source":true}])
    );
    page.eval("document.body.appendChild(detachedFrame);globalThis.__RESULT=String(detachedFrame.contentWindow!==null);").await.unwrap();
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["hostContexts"],
        1
    );
    page.eval("detachedFrame.remove();globalThis.__RESULT='done';")
        .await
        .unwrap();
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["hostContexts"],
        0
    );
}

#[tokio::test(flavor = "current_thread")]
async fn retirement_during_payload_getter_commits_buffers_and_discards_its_snapshot() {
    let mut page = turbo_surf_render::PageSession::open(
        "<iframe id=f></iframe>",
        "https://lifetime.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let raw=page.eval_expression(r#"(()=>{const frame=document.getElementById('f'),child=frame.contentWindow,buffer=new ArrayBuffer(4);let calls=0;child.postMessage({get remove(){calls++;frame.remove();return 1},buffer},'*',[buffer]);return JSON.stringify({calls,bytes:buffer.byteLength});})()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        json!({"calls":1,"bytes":0})
    );
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["queuedWindowMessages"],
        0
    );
}

#[tokio::test(flavor = "current_thread")]
async fn retired_frame_cancels_timer_payload_roots_while_retained_document_stays_usable() {
    let mut page = turbo_surf_render::PageSession::open(
        "<iframe id=f></iframe>",
        "https://lifetime.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval_expression(r#"(()=>{
      const frame=document.getElementById('f'),child=frame.contentWindow;globalThis.oldDocument=frame.contentDocument;
      child.eval("(()=>{const marker={};parent.timerPayload=new parent.WeakRef(marker);setTimeout(()=>marker,1000);})()");
      frame.remove();return 'prepared';
    })()"#).await.unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    let raw=page.eval_expression("JSON.stringify({collected:timerPayload.deref()===undefined,usable:oldDocument.createElement('p').ownerDocument===oldDocument})").await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        json!({"collected":true,"usable":true})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn repeated_navigation_keeps_only_active_host_contexts_and_retires_nested_ports() {
    let mut page = turbo_surf_render::PageSession::open(
        "<iframe id=f></iframe>",
        "https://lifetime.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    for index in 0..4 {
        let source = format!(
            r#"(()=>{{const frame=document.getElementById('f');frame.srcdoc='<body>navigation-{index}<iframe id=n></iframe>';globalThis.__RESULT='navigation';}})();"#
        );
        page.eval(&source).await.unwrap();
        let state = page.frame_lifecycle_diagnostics_for_testing();
        assert_eq!(state["hostContexts"], 2, "{state}");
        assert_eq!(state["retiredHostContexts"], 0, "{state}");
    }
    page.eval_expression(r#"(()=>{
      const frame=document.getElementById('f'),nested=frame.contentDocument.getElementById('n'),child=nested.contentWindow;
      const channel=new child.MessageChannel();globalThis.nestedEvents=[];globalThis.nestedPeer=structuredClone(channel.port1,{transfer:[channel.port1]});
      nestedPeer.onmessage=event=>nestedEvents.push(event.data);globalThis.nestedSender=channel.port2;
      child.eval("setTimeout(()=>parent.parent.nestedEvents.push('old-timer'),1000)");
      frame.remove();nestedSender.postMessage('after-removal');return 'prepared';
    })()"#).await.unwrap();
    page.eval("globalThis.__RESULT='pump';").await.unwrap();
    assert_eq!(
        page.eval_expression("JSON.stringify(nestedEvents)")
            .await
            .unwrap(),
        "[]"
    );
    assert_eq!(
        page.frame_lifecycle_diagnostics_for_testing()["hostContexts"],
        0
    );
    page.eval("nestedSender=null;nestedPeer=null;globalThis.__RESULT='released';")
        .await
        .unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(page.message_port_diagnostics_for_testing()["endpoints"], 0);
}

#[tokio::test(flavor = "current_thread")]
async fn navigation_during_clone_discards_only_the_old_window_transfer_snapshot() {
    let mut page = turbo_surf_render::PageSession::open(
        "<iframe id=f></iframe>",
        "https://lifetime.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let actual = page.eval_json_expression(r#"(()=>{
      const frame=document.getElementById('f'),child=frame.contentWindow;
      const channel=new MessageChannel(),buffer=new ArrayBuffer(4096);
      globalThis.navigationPeer=channel.port1;globalThis.oldArray=new WeakRef(child.Array);
      let calls=0;
      child.postMessage({get navigation(){calls++;frame.srcdoc='<body>replacement';return true;},buffer,port:channel.port2},'*',[buffer,channel.port2]);
      return {calls,bytes:buffer.byteLength};
    })()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&actual).unwrap(),
        json!({"calls":1,"bytes":0})
    );
    let state = page.frame_lifecycle_diagnostics_for_testing();
    assert_eq!(state["retiredHostContexts"], 0, "{state}");
    assert_eq!(state["queuedWindowMessages"], 0, "{state}");
    assert_eq!(state["queuedWindowBytes"], 0, "{state}");
    page.eval("globalThis.__RESULT='pump';").await.unwrap();
    assert_eq!(
        page.eval_expression("document.getElementById('f').contentDocument.body.textContent")
            .await
            .unwrap(),
        "replacement"
    );
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.eval_expression("String(oldArray.deref()===undefined)")
            .await
            .unwrap(),
        "true"
    );
    let ports = page.message_port_diagnostics_for_testing();
    assert_eq!(ports["endpoints"], 1, "{ports}");
    assert_eq!(ports["queuedMessages"], 0, "{ports}");
    assert_eq!(ports["queuedBytes"], 0, "{ports}");
    page.eval_expression("navigationPeer=null;document.getElementById('f').remove();'released'")
        .await
        .unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(page.message_port_diagnostics_for_testing()["endpoints"], 0);
}
