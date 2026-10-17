//! Actual endpoint ownership, native graph aliases, and asynchronous delivery.
use serde_json::{json, Value};

async fn evaluate(source: &str) -> Value {
    let raw = turbo_surf_render::eval_async(
        "<!doctype html><body></body>",
        "https://ports.test/",
        &format!("(async()=>{{{source}}})()"),
    )
    .await
    .unwrap();
    serde_json::from_str(&raw).unwrap_or_else(|_| panic!("invalid port result: {raw}"))
}

#[tokio::test(flavor = "current_thread")]
async fn port_buffer_transfer_detaches_and_preserves_native_aliases() {
    let value = evaluate(r#"
      const channel=new MessageChannel(), buffer=new ArrayBuffer(4);
      new Uint8Array(buffer).set([1,2,3,4]);
      let synchronous=true;
      const received=new Promise(resolve=>channel.port2.onmessage=event=>resolve({
        bytes:Array.from(new Uint8Array(event.data.buffer)),alias:event.data.buffer===event.data.alias,
        async:!synchronous,trusted:event.isTrusted,phase:event.eventPhase,
        target:event.target===channel.port2&&event.currentTarget===channel.port2,
        origin:event.origin,source:event.source,ports:event.ports.length,frozen:Object.isFrozen(event.ports)
      }));
      channel.port1.postMessage({buffer,alias:buffer},[buffer]);
      const detached=buffer.byteLength;synchronous=false;const receivedValue=await received;
      channel.port1.close();channel.port2.close();
      globalThis.__RESULT=JSON.stringify({detached,received:receivedValue,own:Reflect.ownKeys(channel.port1),
        brand:channel.port1 instanceof MessagePort&&channel.port1 instanceof EventTarget});
    "#).await;
    assert_eq!(value["detached"], 0);
    assert_eq!(value["own"], json!([]));
    assert_eq!(value["brand"], true);
    assert_eq!(value["received"]["bytes"], json!([1, 2, 3, 4]));
    assert_eq!(value["received"]["phase"], 2);
    assert_eq!(value["received"]["origin"], "");
    assert_eq!(value["received"]["source"], Value::Null);
    assert_eq!(value["received"]["ports"], 0);
    for flag in ["alias", "async", "trusted", "target", "frozen"] {
        assert_eq!(value["received"][flag], true, "{flag}: {value}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn endpoint_transfer_preserves_queued_messages_and_neuters_the_source() {
    let value = evaluate(r#"
      const channel=new MessageChannel();channel.port1.postMessage('one');channel.port1.postMessage('two');
      const moved=structuredClone(channel.port2,{transfer:[channel.port2]});
      const deliveries=[];let synchronous=true;
      const received=new Promise(resolve=>moved.onmessage=event=>{
        deliveries.push([event.data,!synchronous,event.target===moved,event.isTrusted]);
        if(deliveries.length===2)resolve();
      });
      const before=deliveries.length;synchronous=false;
      let detached,missing;try{structuredClone(channel.port2,{transfer:[channel.port2]})}catch(e){detached=[e.name,e.code]}
      try{structuredClone(moved)}catch(e){missing=[e.name,e.code]}
      const silent=channel.port2.postMessage(()=>{})===undefined;
      await received;const port=moved instanceof MessagePort&&moved!==channel.port2;
      channel.port1.close();moved.close();
      const closed=new MessageChannel();closed.port1.close();
      const closedClone=structuredClone(closed.port1,{transfer:[closed.port1]});
      globalThis.__RESULT=JSON.stringify({deliveries,before,detached,missing,silent,port,
        closed:closedClone instanceof MessagePort&&closedClone.postMessage(()=>{})===undefined});
    "#).await;
    assert_eq!(value["before"], 0);
    assert_eq!(
        value["deliveries"],
        json!([["one", true, true, true], ["two", true, true, true]])
    );
    assert_eq!(value["detached"], json!(["DataCloneError", 25]));
    assert_eq!(value["missing"], json!(["DataCloneError", 25]));
    for flag in ["silent", "port", "closed"] {
        assert_eq!(value[flag], true, "{value}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn failed_graph_or_transfer_validation_changes_no_source_owner() {
    let value = evaluate(r#"
      const channel=new MessageChannel(), moving=new MessageChannel(), buffer=new ArrayBuffer(3);
      const failure=run=>{try{run();return null}catch(e){return [e.name,e.code]}};
      const duplicate=failure(()=>channel.port1.postMessage({buffer},[buffer,buffer]));
      const duplicatePort=failure(()=>channel.port1.postMessage('bad',[moving.port2,moving.port2]));
      const self=failure(()=>channel.port1.postMessage('bad',[channel.port1]));
      let calls=0;const sentinel=new RangeError('authored');let original=false;
      try{channel.port1.postMessage({get value(){calls++;throw sentinel}},[moving.port2,buffer])}catch(e){original=e===sentinel}
      const length=buffer.byteLength;
      const received=new Promise(resolve=>moving.port2.onmessage=e=>resolve(e.data));
      moving.port1.postMessage('still-owned');const usable=await received;
      channel.port1.close();channel.port2.close();moving.port1.close();moving.port2.close();
      globalThis.__RESULT=JSON.stringify({duplicate,duplicatePort,self,calls,original,length,usable});
    "#).await;
    for name in ["duplicate", "duplicatePort", "self"] {
        assert_eq!(value[name], json!(["DataCloneError", 25]), "{value}");
    }
    assert_eq!(value["calls"], 1);
    assert_eq!(value["original"], true);
    assert_eq!(value["length"], 3);
    assert_eq!(value["usable"], "still-owned");
}

#[tokio::test(flavor = "current_thread")]
async fn window_transfer_serializes_before_origin_filter_and_delivers_port_identity() {
    let value = evaluate(r#"
      const channel=new MessageChannel(), buffer=new ArrayBuffer(2);new Uint8Array(buffer).set([7,8]);
      const received=new Promise(resolve=>addEventListener('message',event=>{
        if(event.data.kind!=='transfer')return;
        const port=event.ports[0];port.onmessage=message=>resolve({
          identity:event.data.port===port,realm:Object.getPrototypeOf(port)===MessagePort.prototype,
          bytes:Array.from(new Uint8Array(event.data.buffer)),frozen:Object.isFrozen(event.ports),
          source:event.source===window,origin:event.origin===location.origin,trusted:event.isTrusted,reply:message.data
        });channel.port1.postMessage('peer');
      },{once:true}));
      window.postMessage({kind:'transfer',port:channel.port2,buffer},location.origin,[channel.port2,buffer]);
      const detached=buffer.byteLength;
      const result=await received;
      const mismatch=new ArrayBuffer(5);let getter=0;
      window.postMessage({get value(){getter++;return 1}},'https://mismatch.invalid',[mismatch]);
      channel.port1.close();
      globalThis.__RESULT=JSON.stringify({result,detached,mismatch:[mismatch.byteLength,getter]});
    "#).await;
    assert_eq!(value["detached"], 0);
    assert_eq!(value["mismatch"], json!([0, 1]));
    assert_eq!(value["result"]["bytes"], json!([7, 8]));
    assert_eq!(value["result"]["reply"], "peer");
    for flag in ["identity", "realm", "frozen", "source", "origin", "trusted"] {
        assert_eq!(value["result"][flag], true, "{flag}: {value}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn foreign_window_and_transferred_port_use_receiver_realm_and_strong_brands() {
    let value = evaluate(r#"
      const frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;
      const channel=new MessageChannel();
      const received=new Promise(resolve=>child.addEventListener('message',event=>{
        const port=event.ports[0];port.onmessage=message=>resolve({
          port:event.data.port===port,realm:port instanceof child.MessagePort&&!(port instanceof MessagePort),
          event:message instanceof child.MessageEvent,source:event.source===window,
          reply:message.data,own:Reflect.ownKeys(port)
        });channel.port1.postMessage('foreign');
      },{once:true}));
      child.postMessage({port:channel.port2},'*',[channel.port2]);const result=await received;
      let forged=false;try{MessagePort.prototype.postMessage.call(Object.create(MessagePort.prototype),'bad')}catch(e){forged=e instanceof TypeError}
      const fake=structuredClone(Object.create(MessagePort.prototype));
      const plain=Object.getPrototypeOf(fake)===Object.prototype;
      channel.port1.close();frame.remove();
      globalThis.__RESULT=JSON.stringify({result,forged,plain});
    "#).await;
    for name in ["port", "realm", "event", "source"] {
        assert_eq!(value["result"][name], true, "{name}: {value}");
    }
    assert_eq!(value["result"]["own"], json!([]));
    assert_eq!(value["result"]["reply"], "foreign");
    assert_eq!(value["forged"], true);
    assert_eq!(value["plain"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn port_handler_registration_order_signal_once_and_cleanup_are_native() {
    let value = evaluate(r#"
      const channel=new MessageChannel(), port=channel.port2, abort=new AbortController(), rows=[];
      port.onmessage=e=>rows.push('handler:'+e.data);
      port.addEventListener('message',e=>rows.push('capture:'+e.data),true);
      port.addEventListener('message',e=>rows.push('once:'+e.data),{once:true});
      port.addEventListener('message',()=>rows.push('aborted'),{signal:abort.signal});abort.abort();
      let saved;
      const received=new Promise(resolve=>port.addEventListener('message',e=>{rows.push('last:'+e.data);saved=e;if(e.data===2)resolve()}));
      channel.port1.postMessage(1);channel.port1.postMessage(2);await received;
      const clean=[saved.target===port,saved.currentTarget,saved.eventPhase,saved.composedPath().length];
      channel.port1.close();port.close();
      globalThis.__RESULT=JSON.stringify({rows,clean,own:Reflect.ownKeys(port),inherited:!Object.hasOwn(MessagePort.prototype,'addEventListener')});
    "#).await;
    assert_eq!(
        value["rows"],
        json!([
            "handler:1",
            "capture:1",
            "once:1",
            "last:1",
            "handler:2",
            "capture:2",
            "last:2"
        ])
    );
    assert_eq!(value["clean"], json!([true, null, 0, 0]));
    assert_eq!(value["own"], json!([]));
    assert_eq!(value["inherited"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn guest_prototype_hooks_never_observe_native_port_state_or_transfer_plans() {
    let value = evaluate(r#"
      const channel=new MessageChannel(), moving=new MessageChannel();let hooks=0;
      Object.defineProperty(Array.prototype,'0',{configurable:true,set(){hooks++}});
      Object.defineProperty(Object.prototype,'listeners',{configurable:true,set(){hooks++}});
      const oldIterator=Array.prototype[Symbol.iterator];Array.prototype[Symbol.iterator]=function(){hooks++;throw new Error('private iterator')};
      let received,detached;
      try{
        const transfer={*[Symbol.iterator](){yield moving.port2}};
        const moved=structuredClone(moving.port2,{transfer});
        const promise=new Promise(resolve=>moved.onmessage=e=>resolve(e.data));
        moving.port1.postMessage(17);received=await promise;
        detached=Reflect.ownKeys(moved).length===0;
      }finally{delete Array.prototype[0];delete Object.prototype.listeners;Array.prototype[Symbol.iterator]=oldIterator;}
      channel.port1.close();channel.port2.close();moving.port1.close();
      globalThis.__RESULT=JSON.stringify({hooks,received,detached});
    "#).await;
    assert_eq!(
        value["hooks"], 0,
        "private state reached guest hooks: {value}"
    );
    assert_eq!(value["received"], 17);
    assert_eq!(value["detached"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn transferring_the_receiving_endpoint_discards_delivery_and_retains_sender_validation() {
    let value = evaluate(r#"
      const channel=new MessageChannel(),buffer=new ArrayBuffer(3);let calls=0,getter=0;
      channel.port2.onmessage=()=>calls++;
      channel.port1.postMessage({get value(){getter++;return 1},port:channel.port2,buffer},[channel.port2,buffer]);
      let detached,after;
      try{structuredClone(channel.port2,{transfer:[channel.port2]})}catch(error){detached=error.name}
      try{channel.port1.postMessage(()=>{})}catch(error){after=error.name}
      await new Promise(resolve=>setTimeout(resolve,0));
      let closed=0,invalid;
      const sequence={ [Symbol.iterator](){return {next(){return {value:1,done:false}},return(){closed++;return {done:true}}}}};
      try{channel.port1.postMessage('invalid',sequence)}catch(error){invalid=error.name}
      channel.port1.close();channel.port2.close();
      globalThis.__RESULT=JSON.stringify({calls,getter,buffer:buffer.byteLength,detached,after,closed,invalid});
    "#).await;
    assert_eq!(
        value,
        json!({"calls":0,"getter":1,"buffer":0,"detached":"DataCloneError","after":"DataCloneError","closed":0,"invalid":"TypeError"})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn port_overloads_and_window_sequence_conversion_match_webidl_order() {
    let value=evaluate(r#"
      const channel=new MessageChannel(),capture=fn=>{try{fn();return 'ok'}catch(error){return error.name}};
      const values=[undefined,null,false,1,'x',{},[],{transfer:null},{transfer:[]},{transfer:[1]},
        {[Symbol.iterator](){const result=function(){};result.done=true;return {next(){return result}}}}];
      const port=values.map(value=>capture(()=>channel.port1.postMessage('x',value)));
      const windowResults=values.map(value=>capture(()=>window.postMessage('x','https://mismatch.invalid',value)));
      let getter=0;
      const invalidOrigin=capture(()=>window.postMessage('x','bad-origin',{[Symbol.iterator](){getter++;return [][Symbol.iterator]()}}));
      channel.port1.close();channel.port2.close();
      globalThis.__RESULT=JSON.stringify({port,window:windowResults,invalidOrigin,getter});
    "#).await;
    assert_eq!(
        value["port"],
        json!([
            "ok",
            "ok",
            "TypeError",
            "TypeError",
            "TypeError",
            "ok",
            "ok",
            "TypeError",
            "ok",
            "TypeError",
            "ok"
        ])
    );
    assert_eq!(
        value["window"],
        json!([
            "ok",
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            "ok",
            "TypeError",
            "TypeError",
            "TypeError",
            "ok"
        ])
    );
    assert_eq!(value["invalidOrigin"], "SyntaxError");
    assert_eq!(value["getter"], 1);
}

#[tokio::test(flavor = "current_thread")]
async fn timer_callback_receivers_never_expose_private_scheduling_records() {
    let value=evaluate(r#"
      let receiver;
      await new Promise(resolve=>setTimeout(function(){'use strict';receiver=this;resolve()},0));
      globalThis.__RESULT=JSON.stringify({window:receiver===window,privateFields:Object.hasOwn(receiver,'due')||Object.hasOwn(receiver,'fn')||Object.hasOwn(receiver,'args')});
    "#).await;
    assert_eq!(value, json!({"window":true,"privateFields":false}));
}

#[tokio::test(flavor = "current_thread")]
async fn opaque_frame_receives_true_port_ownership_without_window_authority() {
    if !turbo_surf_render::stable_window_proxy() {
        return;
    }
    let value=evaluate(r#"
      const frame=document.createElement('iframe');frame.setAttribute('sandbox','allow-scripts');
      frame.srcdoc='<script>onmessage=event=>{const port=event.ports[0];let blocked=false;try{parent.document}catch(error){blocked=error.name===\"SecurityError\"}port.postMessage({realm:port instanceof MessagePort,identity:port===event.data.port,source:event.source===parent,origin:event.origin,blocked,bytes:Array.from(new Uint8Array(event.data.buffer)),trusted:event.isTrusted});port.close()};parent.postMessage(\"ready\",\"*\");<'+ '/script>';
      const channel=new MessageChannel(),buffer=new ArrayBuffer(2);new Uint8Array(buffer).set([5,9]);
      const result=new Promise(resolve=>{channel.port1.onmessage=event=>resolve(event.data);addEventListener('message',event=>{
        if(event.data==='ready'&&event.source===frame.contentWindow)frame.contentWindow.postMessage({port:channel.port2,buffer},{targetOrigin:'*',transfer:[channel.port2,buffer]});
      })});
      document.body.append(frame);const received=await result;
      channel.port1.close();frame.remove();
      globalThis.__RESULT=JSON.stringify({received,detached:buffer.byteLength});
    "#).await;
    assert_eq!(value["detached"], 0);
    assert_eq!(value["received"]["bytes"], json!([5, 9]));
    assert_eq!(value["received"]["origin"], "https://ports.test");
    for flag in ["realm", "identity", "source", "blocked", "trusted"] {
        assert_eq!(value["received"][flag], true, "{flag}: {value}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn iterable_overload_queries_iterator_twice_and_null_handler_enables_delivery() {
    let value=evaluate(r#"
      let getters=0,methods=0;const channel=new MessageChannel();
      const iterable={get [Symbol.iterator](){getters++;return function(){methods++;return [][Symbol.iterator]()}}};
      channel.port1.postMessage('iterable',iterable);
      const nullStart=new MessageChannel();nullStart.port2.onmessage=null;
      const received=new Promise(resolve=>nullStart.port2.addEventListener('message',event=>resolve(event.data)));
      nullStart.port1.postMessage('queued');const data=await received;
      channel.port1.close();channel.port2.close();nullStart.port1.close();nullStart.port2.close();
      globalThis.__RESULT=JSON.stringify({getters,methods,data});
    "#).await;
    assert_eq!(value, json!({"getters":2,"methods":1,"data":"queued"}));
}

#[tokio::test(flavor = "current_thread")]
async fn queue_memory_rejection_preserves_buffer_and_port_owners_and_releases_reservations() {
    let mut page =
        turbo_surf_render::PageSession::open("<body></body>", "https://budget.test/", "", "", 5000)
            .await
            .unwrap();
    page.eval("globalThis.channel=new MessageChannel();globalThis.moving=new MessageChannel();globalThis.buffer=new ArrayBuffer(128);").await.unwrap();
    page.set_message_port_limits_for_testing(8, 64);
    let raw=page.eval(r#"
      let error;try{channel.port1.postMessage({port:moving.port2,buffer},[moving.port2,buffer])}catch(caught){error=caught.name}
      globalThis.moved=structuredClone(moving.port2,{transfer:[moving.port2]});
      globalThis.__RESULT=JSON.stringify({error,bytes:buffer.byteLength,port:moved instanceof MessagePort});
    "#).await.unwrap();
    let value: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(value, json!({"error":"RangeError","bytes":128,"port":true}));
    let state = page.message_port_diagnostics_for_testing();
    for field in [
        "reservations",
        "reservedBytes",
        "queuedBytes",
        "queuedMessages",
    ] {
        assert_eq!(state[field], 0, "{field}: {state}");
    }
    page.set_message_port_limits_for_testing(8, 4096);
    page.eval(r#"
      moved.onmessage=event=>globalThis.delivered=event.data;moving.port1.postMessage('still-owned');
    "#).await.unwrap();
    let raw = page
        .eval("globalThis.__RESULT=JSON.stringify({delivered});")
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        json!({"delivered":"still-owned"})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn reentrant_getters_cannot_overrun_reserved_queue_count() {
    let mut page =
        turbo_surf_render::PageSession::open("<body></body>", "https://budget.test/", "", "", 5000)
            .await
            .unwrap();
    page.eval(
        "globalThis.channel=new MessageChannel();globalThis.nested=null;globalThis.events=[];",
    )
    .await
    .unwrap();
    page.set_message_port_limits_for_testing(1, 4096);
    page.eval(r#"
      channel.port1.postMessage({get value(){try{channel.port1.postMessage('nested')}catch(error){nested=error.name}return 'outer'}});
    "#).await.unwrap();
    let state = page.message_port_diagnostics_for_testing();
    assert_eq!(state["queuedMessages"], 1, "{state}");
    assert_eq!(state["reservations"], 0);
    assert_eq!(state["reservedBytes"], 0);
    page.eval("channel.port2.onmessage=event=>events.push(event.data.value); ")
        .await
        .unwrap();
    let raw = page
        .eval("globalThis.__RESULT=JSON.stringify({nested,events});")
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        json!({"nested":"RangeError","events":["outer"]})
    );
    let state = page.message_port_diagnostics_for_testing();
    for field in [
        "queuedMessages",
        "queuedBytes",
        "reservations",
        "reservedBytes",
    ] {
        assert_eq!(state[field], 0, "{state}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn getter_exceptions_release_queue_admission_without_detaching_sources() {
    let mut page =
        turbo_surf_render::PageSession::open("<body></body>", "https://budget.test/", "", "", 5000)
            .await
            .unwrap();
    page.eval("globalThis.channel=new MessageChannel();globalThis.buffer=new ArrayBuffer(32);")
        .await
        .unwrap();
    page.set_message_port_limits_for_testing(1, 4096);
    let raw=page.eval(r#"
      const sentinel=new RangeError('authored');let original=false;
      try{channel.port1.postMessage({get value(){throw sentinel}},[buffer])}catch(error){original=error===sentinel}
      channel.port1.postMessage('after');globalThis.__RESULT=JSON.stringify({original,bytes:buffer.byteLength});
    "#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        json!({"original":true,"bytes":32})
    );
    let state = page.message_port_diagnostics_for_testing();
    assert_eq!(state["queuedMessages"], 1, "{state}");
    assert_eq!(state["reservations"], 0);
    assert_eq!(state["reservedBytes"], 0);
}

#[tokio::test(flavor = "current_thread")]
async fn a_doomed_receiver_and_origin_mismatch_do_not_require_delivery_admission() {
    let mut page =
        turbo_surf_render::PageSession::open("<body></body>", "https://budget.test/", "", "", 5000)
            .await
            .unwrap();
    page.set_message_port_limits_for_testing(0, 0);
    let raw = page
        .eval(
            r#"
      const channel=new MessageChannel(),buffer=new ArrayBuffer(32),wrong=new ArrayBuffer(16);
      channel.port1.postMessage({port:channel.port2,buffer},[channel.port2,buffer]);
      window.postMessage({wrong},'https://mismatch.invalid',[wrong]);
      globalThis.__RESULT=JSON.stringify({buffer:buffer.byteLength,wrong:wrong.byteLength});
    "#,
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        json!({"buffer":0,"wrong":0})
    );
    let state = page.message_port_diagnostics_for_testing();
    for field in [
        "reservations",
        "reservedBytes",
        "queuedMessages",
        "queuedBytes",
    ] {
        assert_eq!(state[field], 0, "{state}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn private_endpoint_graphs_follow_chrome_reachability_and_reclaim_queued_cycles() {
    let mut page = turbo_surf_render::PageSession::open(
        "<body></body>",
        "https://lifetime.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval(include_str!(
        "../../../../harness/detection/message-port-lifetime-probe.js"
    ))
    .await
    .unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    let raw = page
        .eval("globalThis.__RESULT=JSON.stringify(__readPortLifetime());")
        .await
        .unwrap();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/message-port-lifetime-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, control["first"]);
    let state = page.message_port_diagnostics_for_testing();
    assert_eq!(state["endpoints"], 6, "{state}");
    page.eval("__portLifetimeState.retained=null;")
        .await
        .unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    let raw = page
        .eval("globalThis.__RESULT=JSON.stringify(__readPortLifetime());")
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        control["afterRelease"]
    );
    let state = page.message_port_diagnostics_for_testing();
    for field in [
        "endpoints",
        "queuedMessages",
        "queuedBytes",
        "reservations",
        "reservedBytes",
    ] {
        assert_eq!(state[field], 0, "{field}: {state}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn closed_live_wrappers_remain_transferable_and_reclaimed_detached_wrappers_are_inert() {
    let mut page = turbo_surf_render::PageSession::open(
        "<body></body>",
        "https://lifetime.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval("{const channel=new MessageChannel();channel.port1.close();channel.port2.close();globalThis.retainedClosedPort=channel.port2;}").await.unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(page.message_port_diagnostics_for_testing()["endpoints"], 1);
    let raw=page.eval("globalThis.moved=structuredClone(retainedClosedPort,{transfer:[retainedClosedPort]});moved.postMessage(()=>{});globalThis.__RESULT=JSON.stringify({closedTransfer:moved instanceof MessagePort});").await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        json!({"closedTransfer":true})
    );
    page.eval("moved=null;").await.unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(page.message_port_diagnostics_for_testing()["endpoints"], 0);
    let raw=page.eval("let error;retainedClosedPort.start();retainedClosedPort.close();retainedClosedPort.postMessage(()=>{});try{structuredClone(retainedClosedPort,{transfer:[retainedClosedPort]})}catch(caught){error=caught.name}globalThis.__RESULT=JSON.stringify({error});").await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        json!({"error":"DataCloneError"})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn payload_getter_lifecycle_changes_recheck_sender_and_preserve_outer_detachment() {
    let value=evaluate(r#"
      async function measure(mode){
        const channel=new MessageChannel(),buffer=new ArrayBuffer(16),events=[];let moved=null,getterCalls=0;
        channel.port2.onmessage=event=>events.push(event.data.value);
        channel.port1.postMessage({buffer,get value(){
          getterCalls++;if(mode==='close-sender')channel.port1.close();
          if(mode==='transfer-sender')moved=structuredClone(channel.port1,{transfer:[channel.port1]});
          if(mode==='close-receiver')channel.port2.close();
          if(mode==='transfer-receiver')moved=structuredClone(channel.port2,{transfer:[channel.port2]});
          return mode;
        }},[buffer]);
        if(mode==='transfer-receiver')moved.onmessage=event=>events.push(event.data.value);
        await new Promise(resolve=>setTimeout(resolve,0));
        const result={mode,getterCalls,detached:buffer.byteLength,events};channel.port1.close();channel.port2.close();moved?.close();return result;
      }
      const rows=[];for(const mode of ['unchanged','close-sender','close-receiver','transfer-receiver'])rows.push(await measure(mode));
      const boundedTransferSender=await measure('transfer-sender');globalThis.__RESULT=JSON.stringify({rows,boundedTransferSender});
    "#).await;
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/message-port-getter-lifecycle-chrome155.json"
    ))
    .unwrap();
    for (actual, expected) in value["rows"]
        .as_array()
        .unwrap()
        .iter()
        .zip(control["result"].as_array().unwrap())
    {
        for field in ["mode", "getterCalls", "detached", "events"] {
            assert_eq!(actual[field], expected[field], "{field}: {actual}");
        }
    }
    // Chrome155.0.8059.79 hangs synchronously on this reentrant sender transfer.
    // Require bounded engine behavior separately; this is not a successful Chrome match.
    assert_eq!(
        value["boundedTransferSender"],
        json!({"mode":"transfer-sender","getterCalls":1,"detached":0,"events":[]})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn pending_delivery_pins_its_receiver_then_releases_after_the_task() {
    let mut page = turbo_surf_render::PageSession::open(
        "<body></body>",
        "https://lifetime.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval_expression(r#"(()=>{
      const channel=new MessageChannel();globalThis.pendingDelivered=0;globalThis.pendingReceiver=new WeakRef(channel.port2);
      channel.port2.onmessage=()=>pendingDelivered++;channel.port1.postMessage('queued');return 'prepared';
    })()"#).await.unwrap();
    assert_eq!(
        page.message_port_diagnostics_for_testing()["queuedMessages"],
        1
    );
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    let raw=page.eval_expression("JSON.stringify({alive:pendingReceiver.deref()!==undefined,delivered:pendingDelivered})").await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        json!({"alive":true,"delivered":0})
    );
    page.eval("globalThis.__RESULT='';").await.unwrap();
    let raw = page
        .eval_expression("String(pendingDelivered)")
        .await
        .unwrap();
    assert_eq!(raw, "1");
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    let raw = page
        .eval_expression("String(pendingReceiver.deref()===undefined)")
        .await
        .unwrap();
    assert_eq!(raw, "true");
    let state = page.message_port_diagnostics_for_testing();
    for field in ["endpoints", "queuedMessages", "queuedBytes"] {
        assert_eq!(state[field], 0, "{state}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn queued_native_keys_do_not_retain_source_expandos_and_unreachable_port_graphs() {
    let mut page = turbo_surf_render::PageSession::open(
        "<body></body>",
        "https://lifetime.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval_expression(include_str!(
        "../../../../harness/detection/message-port-key-lifetime-probe.js"
    ))
    .await
    .unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    let raw = page
        .eval_expression("JSON.stringify(__readPortLifetime())")
        .await
        .unwrap();
    let actual: Value = serde_json::from_str(&raw).unwrap();
    let control: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/message-port-key-lifetime-chrome155.json"
    ))
    .unwrap();
    assert_eq!(actual, control["first"]);
    let state = page.message_port_diagnostics_for_testing();
    for field in ["endpoints", "queuedMessages", "queuedBytes"] {
        assert_eq!(state[field], 0, "{state}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn queued_key_delivery_survives_source_collection_and_keeps_real_permissions() {
    let mut page = turbo_surf_render::PageSession::open(
        "<body></body>",
        "https://lifetime.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    page.eval_expression(r#"(async()=>{
      const key=await crypto.subtle.importKey('raw',new Uint8Array(16),'AES-GCM',false,['encrypt']);
      const channel=new MessageChannel();globalThis.retainedKeyReceiver=channel.port2;globalThis.sourceKey=new WeakRef(key);
      key.receiver=channel.port2;channel.port1.postMessage({key,again:key});return 'prepared';
    })()"#).await.unwrap();
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(
        page.eval_expression("String(sourceKey.deref()===undefined)")
            .await
            .unwrap(),
        "true"
    );
    let raw=page.eval(r#"retainedKeyReceiver.onmessage=async event=>{
      const key=event.data.key,iv=new Uint8Array(12),data=new Uint8Array([7]);
      const ciphertext=await crypto.subtle.encrypt({name:'AES-GCM',iv},key,data);
      let exportError,decryptError;try{await crypto.subtle.exportKey('raw',key)}catch(error){exportError=error.name;}
      try{await crypto.subtle.decrypt({name:'AES-GCM',iv},key,ciphertext)}catch(error){decryptError=error.name;}
      globalThis.__RESULT=JSON.stringify({alias:key===event.data.again,brand:key instanceof CryptoKey,ownKeys:Reflect.ownKeys(key),algorithm:key.algorithm,usages:key.usages,extractable:key.extractable,bytes:ciphertext.byteLength,exportError,decryptError});
      retainedKeyReceiver.close();retainedKeyReceiver=null;
    };"#).await.unwrap();
    let value: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        value,
        json!({"alias":true,"brand":true,"ownKeys":[],"algorithm":{"name":"AES-GCM","length":128},"usages":["encrypt"],"extractable":false,"bytes":17,"exportError":"InvalidAccessError","decryptError":"InvalidAccessError"})
    );
    for _ in 0..3 {
        page.collect_message_port_garbage_for_testing();
    }
    assert_eq!(page.message_port_diagnostics_for_testing()["endpoints"], 0);
}

#[tokio::test(flavor = "current_thread")]
async fn native_key_material_counts_against_queue_admission_before_buffer_transfer() {
    let mut page =
        turbo_surf_render::PageSession::open("<body></body>", "https://budget.test/", "", "", 5000)
            .await
            .unwrap();
    page.set_message_port_limits_for_testing(100, 128);
    let raw=page.eval(r#"(async()=>{
      const key=await crypto.subtle.importKey('raw',new Uint8Array(512),{name:'HMAC',hash:'SHA-256'},false,['sign']);
      const channel=new MessageChannel(),buffer=new ArrayBuffer(4);let error;
      try{channel.port1.postMessage({key,buffer},[buffer])}catch(caught){error=caught.name;}
      globalThis.__RESULT=JSON.stringify({error,bytes:buffer.byteLength});channel.port1.close();channel.port2.close();
    })();"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&raw).unwrap(),
        json!({"error":"RangeError","bytes":4})
    );
    let state = page.message_port_diagnostics_for_testing();
    for field in ["queuedMessages", "queuedBytes", "reservations"] {
        assert_eq!(state[field], 0, "{state}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn cloned_native_key_snapshots_restore_typed_algorithm_metadata_and_operations() {
    let mut page = turbo_surf_render::PageSession::open(
        "<body></body>",
        "https://crypto.test/",
        "",
        "",
        20000,
    )
    .await
    .unwrap();
    let raw=page.eval_json_expression(r#"(async()=>{
      const data=new Uint8Array([7,8]),rows=[];
      const rsa=await crypto.subtle.generateKey({name:'RSA-OAEP',modulusLength:1024,publicExponent:new Uint8Array([1,0,1]),hash:'SHA-256'},false,['encrypt','decrypt']);
      const rsac=structuredClone(rsa),cipher=await crypto.subtle.encrypt('RSA-OAEP',rsac.publicKey,data),plain=await crypto.subtle.decrypt('RSA-OAEP',rsac.privateKey,cipher);
      const ec=await crypto.subtle.generateKey({name:'ECDSA',namedCurve:'P-256'},false,['sign','verify']),ecc=structuredClone(ec);
      const signature=await crypto.subtle.sign({name:'ECDSA',hash:'SHA-256'},ecc.privateKey,data),verified=await crypto.subtle.verify({name:'ECDSA',hash:'SHA-256'},ecc.publicKey,signature,data);
      const hmac=await crypto.subtle.importKey('raw',new Uint8Array(32),{name:'HMAC',hash:'SHA-256'},false,['sign','verify']),hmacc=structuredClone(hmac);
      const mac=await crypto.subtle.sign('HMAC',hmacc,data),macVerified=await crypto.subtle.verify('HMAC',hmacc,mac,data);
      return {rsa:rsac.privateKey.algorithm,exponent:Array.from(rsac.privateKey.algorithm.publicExponent),rsaBytes:Array.from(new Uint8Array(plain)),ec:ecc.privateKey.algorithm,verified,hmac:hmacc.algorithm,macVerified,privatePermissions:[rsac.privateKey,ecc.privateKey,hmacc].map(key=>key.extractable)};
    })()"#).await.unwrap();
    let value: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(value["rsa"]["name"], "RSA-OAEP");
    assert_eq!(value["rsa"]["modulusLength"], 1024);
    assert_eq!(value["rsa"]["hash"], json!({"name":"SHA-256"}));
    assert_eq!(value["exponent"], json!([1, 0, 1]));
    assert_eq!(value["rsaBytes"], json!([7, 8]));
    assert_eq!(value["ec"], json!({"name":"ECDSA","namedCurve":"P-256"}));
    assert_eq!(value["verified"], true);
    assert_eq!(
        value["hmac"],
        json!({"name":"HMAC","length":256,"hash":{"name":"SHA-256"}})
    );
    assert_eq!(value["macVerified"], true);
    assert_eq!(value["privatePermissions"], json!([false, false, false]));
}
