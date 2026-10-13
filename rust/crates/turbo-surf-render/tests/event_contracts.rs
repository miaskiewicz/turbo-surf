//! Event/exception contracts measured against the uninstrumented Chrome oracle.
use turbo_surf_render::{run_with_dom, PageSession};

#[test]
fn dom_exception_has_private_enumerable_branded_accessors() {
    let result = run_with_dom("<body></body>", r#"(() => {
      let conversions=0;const message={toString(){conversions++;return 'bad';}};
      const error=new DOMException(message,'SyntaxError');
      const accessors=['name','message','code'].map(name=>{
        const descriptor=Object.getOwnPropertyDescriptor(DOMException.prototype,name);
        let invalid=false;try{descriptor.get.call({});}catch(e){invalid=e instanceof TypeError;}
        return [descriptor.enumerable,descriptor.configurable,!descriptor.set,invalid,
          !Object.hasOwn(descriptor.get,'prototype'),descriptor.get.name];
      });
      let symbol=false;try{new DOMException(Symbol('bad'));}catch(e){symbol=e instanceof TypeError;}
      return JSON.stringify({own:Reflect.ownKeys(error),name:error.name,message:error.message,code:error.code,
        conversions,accessors,symbol,tag:Object.prototype.toString.call(error),errorBase:error instanceof Error});
    })()"#).unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["own"], serde_json::json!([]));
    assert_eq!(value["name"], "SyntaxError");
    assert_eq!(value["message"], "bad");
    assert_eq!(value["code"], 12);
    assert_eq!(value["conversions"], 1);
    assert_eq!(value["tag"], "[object DOMException]");
    assert_eq!(value["errorBase"], true);
    assert_eq!(value["symbol"], true);
    assert_eq!(
        value["accessors"],
        serde_json::json!([
            [true, true, true, true, true, "get name"],
            [true, true, true, true, true, "get message"],
            [true, true, true, true, true, "get code"]
        ])
    );
}

#[test]
fn event_fields_are_readonly_and_custom_dispatch_updates_private_state() {
    let result = run_with_dom("<body></body>", r#"(() => {
      const event=new Event('test',{cancelable:true,bubbles:true});
      const descriptor=Object.getOwnPropertyDescriptor(event,'isTrusted');
      let invalid=false;try{descriptor.get.call({});}catch(e){invalid=e instanceof TypeError;}
      const keys=Object.keys(event);event.type='wrong';event.cancelable=false;event.__trusted=true;
      const before=[event.type,event.cancelable,event.isTrusted];delete event.__trusted;
      const bus=new EventTarget();let during;
      bus.addEventListener('test',e=>{during=[e.target===bus,e.currentTarget===bus,e.eventPhase,e.composedPath().length];e.returnValue=false;});
      const returned=bus.dispatchEvent(event);
      return JSON.stringify({keys,before,invalid,during,descriptor:[descriptor.enumerable,descriptor.configurable,!descriptor.set,!Object.hasOwn(descriptor.get,'prototype')],
        after:[returned,event.defaultPrevented,event.currentTarget,event.eventPhase,event.composedPath().length],
        tag:Object.prototype.toString.call(event),detailAbsent:!('detail'in event),relative:event.timeStamp>=0&&event.timeStamp<=performance.now()});
    })()"#).unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["keys"], serde_json::json!(["isTrusted"]));
    assert_eq!(value["before"], serde_json::json!(["test", true, false]));
    assert_eq!(value["during"], serde_json::json!([true, true, 2, 1]));
    assert_eq!(
        value["descriptor"],
        serde_json::json!([true, false, true, true])
    );
    assert_eq!(value["after"], serde_json::json!([false, true, null, 0, 0]));
    assert_eq!(value["tag"], "[object Event]");
    for name in ["invalid", "detailAbsent", "relative"] {
        assert_eq!(value[name], true, "{value}");
    }
}

#[tokio::test]
async fn host_messages_are_branded_and_script_redispatch_is_untrusted() {
    let mut page = PageSession::open("<body></body>", "http://localhost/", "", "", 5000)
        .await
        .unwrap();
    page.eval(r#"
      globalThis.messages=[];globalThis.saved=null;
      globalThis.addEventListener('message',event=>{
        if(event.data===17){messages.push([event instanceof Event,event instanceof MessageEvent,event.isTrusted,event.target===globalThis,event.eventPhase]);saved=event;}
      });
      const channel=new MessageChannel();globalThis.port=[];
      channel.port2.onmessage=event=>{port.push([event.isTrusted,event.target===channel.port2,event.currentTarget===channel.port2,event.eventPhase,event.data]);};
      channel.port1.postMessage('port');
      const event=new MessageEvent('manual',{data:5,origin:null});event.data=9;
      let invalid=false;try{Object.getOwnPropertyDescriptor(MessageEvent.prototype,'data').get.call({});}catch(e){invalid=e instanceof TypeError;}
      globalThis.manual=[event instanceof Event,event.data,event.origin,Object.keys(event),Object.isFrozen(event.ports),invalid];
      globalThis.postMessage(17,'*');
    "#).await.unwrap();
    let result = page.eval("saved.__trusted=true;globalThis.dispatchEvent(saved);delete saved.__trusted;globalThis.__RESULT=JSON.stringify({messages,port,manual});").await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        value["messages"],
        serde_json::json!([[true, true, true, true, 2], [true, true, false, true, 2]]),
        "{value}"
    );
    assert_eq!(
        value["port"],
        serde_json::json!([[true, true, true, 2, "port"]])
    );
    assert_eq!(
        value["manual"],
        serde_json::json!([true, 5, "null", ["isTrusted"], true, true])
    );
}

#[test]
fn error_event_preserves_native_error_and_branded_readonly_fields() {
    let result = run_with_dom("<body></body>", r#"(() => {
      const original=new RangeError('original');
      const event=new ErrorEvent('error',{message:'failure',filename:'https://example.test/module.mjs',lineno:4294967298,colno:-1,error:original,cancelable:true});
      const fields=['message','filename','lineno','colno','error'];
      const descriptors=fields.map(name=>{
        const d=Object.getOwnPropertyDescriptor(ErrorEvent.prototype,name);
        let illegal=false;try{d.get.call({});}catch(e){illegal=e instanceof TypeError;}
        return [d.enumerable,d.configurable,!d.set,illegal,Function.prototype.toString.call(d.get).includes('[native code]'),!Object.hasOwn(d.get,'prototype')];
      });
      let bigint=false;try{new ErrorEvent('error',{lineno:1n});}catch(e){bigint=e instanceof TypeError;}
      let missing=false;try{new ErrorEvent();}catch(e){missing=e instanceof TypeError;}
      return JSON.stringify({event:event instanceof Event,error:event.error===original,own:Reflect.ownKeys(event).map(String),tag:Object.prototype.toString.call(event),message:event.message,filename:event.filename,line:event.lineno,column:event.colno,descriptors,bigint,missing});
    })()"#).unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(value["own"], serde_json::json!(["isTrusted"]));
    assert_eq!(value["tag"], "[object ErrorEvent]");
    assert_eq!(value["message"], "failure");
    assert_eq!(value["filename"], "https://example.test/module.mjs");
    assert_eq!(value["line"], 2);
    assert_eq!(value["column"], 4294967295_u64);
    for field in ["event", "error", "bigint", "missing"] {
        assert_eq!(value[field], true, "{value}");
    }
    assert_eq!(
        value["descriptors"],
        serde_json::json!([
            [true, true, true, true, true, true],
            [true, true, true, true, true, true],
            [true, true, true, true, true, true],
            [true, true, true, true, true, true],
            [true, true, true, true, true, true]
        ])
    );
}
