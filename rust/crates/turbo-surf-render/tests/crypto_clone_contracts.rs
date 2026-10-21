//! Private native key cloning must preserve permissions and keep material private.
use serde_json::Value;

async fn evaluate(source: &str) -> Value {
    let source = format!("(async()=>{{{source}}})()");
    let raw = turbo_surf_render::eval_async(
        "<!doctype html><body></body>",
        "https://crypto.test/",
        &source,
    )
    .await
    .unwrap();
    serde_json::from_str(&raw).unwrap_or_else(|_| panic!("invalid crypto result: {raw}"))
}

#[tokio::test(flavor = "current_thread")]
async fn nonextractable_key_creation_never_calls_inherited_material_setters() {
    let value = evaluate(r#"
      let calls=0,observed;
      const fields=['keyData','data','seed','privateKey','publicKey','cppgc'];
      for(const field of fields)Object.defineProperty(Object.prototype,field,{
        configurable:true,set(value){calls++;observed=value;}
      });
      let key,error,privateKeys=[];
      try{
        key=await crypto.subtle.importKey('raw',new Uint8Array(16),'AES-GCM',false,['encrypt']);
        for(const name of ['Ed25519','ML-KEM-768','MLKEM768-X25519']){
          const usages=name==='Ed25519'?['sign','verify']:['encapsulateBits','decapsulateBits'];
          const pair=await crypto.subtle.generateKey(name,false,usages);
          privateKeys.push(!pair.privateKey.extractable&&Reflect.ownKeys(pair.privateKey).length===0);
        }
      }
      catch(caught){error=caught.name;}
      finally{for(const field of fields)delete Object.prototype[field];}
      const bytes=observed instanceof Uint8Array?observed:observed?.data;
      globalThis.__RESULT=JSON.stringify({calls,observedBytes:bytes?.byteLength??0,
        error:error??null,extractable:key?.extractable,ownKeys:key?Reflect.ownKeys(key).length:null,privateKeys});
    "#).await;
    assert_eq!(
        value["calls"], 0,
        "private key material reached a page setter: {value}"
    );
    assert_eq!(value["observedBytes"], 0);
    assert_eq!(value["error"], Value::Null);
    assert_eq!(value["extractable"], false);
    assert_eq!(value["ownKeys"], 0);
    assert_eq!(value["privateKeys"], serde_json::json!([true, true, true]));
}

#[tokio::test(flavor = "current_thread")]
async fn native_metadata_arrays_bypass_inherited_index_setters() {
    let value = evaluate(
        r#"
      let calls=0;
      Object.defineProperty(Array.prototype,'0',{configurable:true,set(){calls++;}});
      let key,error;
      try{key=await crypto.subtle.importKey('raw',new Uint8Array(16),'AES-GCM',false,['encrypt']);}
      catch(caught){error=caught.name;}
      finally{delete Array.prototype[0];}
      globalThis.__RESULT=JSON.stringify({calls,error:error??null,usages:key?.usages});
    "#,
    )
    .await;
    assert_eq!(value["calls"], 0, "{value}");
    assert_eq!(value["error"], Value::Null);
    assert_eq!(value["usages"], serde_json::json!(["encrypt"]));
}

#[tokio::test(flavor = "current_thread")]
async fn graph_clone_preserves_native_keys_permissions_aliases_and_builtins() {
    let value = evaluate(r#"
      const key=await crypto.subtle.importKey('raw',new Uint8Array(16),'AES-GCM',false,['encrypt']);
      const graph={key,again:key,date:new Date(1234),bytes:new Uint8Array([1,2,3])};
      graph.self=graph;graph.map=new Map([[key,graph]]);graph.set=new Set([key]);
      const clone=structuredClone(graph),iv=new Uint8Array(12),data=new Uint8Array(16);
      const ciphertext=await crypto.subtle.encrypt({name:'AES-GCM',iv},clone.key,data);
      let exportError;try{await crypto.subtle.exportKey('raw',clone.key)}catch(error){exportError=error.name;}
      let decryptError;try{await crypto.subtle.decrypt({name:'AES-GCM',iv},clone.key,ciphertext)}catch(error){decryptError=error.name;}
      const fake=structuredClone(Object.create(CryptoKey.prototype));
      let functionError;try{structuredClone(()=>{})}catch(error){functionError=error.name;}
      globalThis.__RESULT=JSON.stringify({distinct:clone.key!==key,alias:clone.key===clone.again,
        cycle:clone.self===clone,map:clone.map.get(clone.key)===clone,set:clone.set.has(clone.key),
        date:clone.date.getTime(),bytes:Array.from(clone.bytes),key:clone.key instanceof CryptoKey,
        extractable:clone.key.extractable,usages:clone.key.usages,algorithm:clone.key.algorithm.name,
        ciphertext:Array.from(new Uint8Array(ciphertext)),exportError,decryptError,functionError,
        fakePlain:Object.getPrototypeOf(fake)===Object.prototype&&!(fake instanceof CryptoKey),
        ownKeys:Reflect.ownKeys(clone.key).length});
    "#).await;
    for field in [
        "distinct",
        "alias",
        "cycle",
        "map",
        "set",
        "key",
        "fakePlain",
    ] {
        assert_eq!(value[field], true, "{field}: {value}");
    }
    assert_eq!(value["date"], 1234);
    assert_eq!(value["bytes"], serde_json::json!([1, 2, 3]));
    assert_eq!(value["extractable"], false);
    assert_eq!(value["usages"], serde_json::json!(["encrypt"]));
    assert_eq!(value["algorithm"], "AES-GCM");
    assert_eq!(value["ciphertext"].as_array().unwrap().len(), 32);
    assert_eq!(value["exportError"], "InvalidAccessError");
    assert_eq!(value["decryptError"], "InvalidAccessError");
    assert_eq!(value["functionError"], "DataCloneError");
    assert_eq!(value["ownKeys"], 0);
}

#[tokio::test(flavor = "current_thread")]
async fn native_key_clone_ignores_forged_properties_and_page_clone_hooks() {
    let value = evaluate(r#"
      const key=await crypto.subtle.importKey('raw',new Uint8Array(16),'AES-GCM',false,['encrypt']);
      let calls=0;
      const poison=()=>{calls++;throw new Error('guest key hook');};
      for(const name of ['extractable','usages','algorithm','type'])
        Object.defineProperty(key,name,{get:poison,configurable:true});
      Object.defineProperty(key,Symbol.for('Deno.core.hostObject'),{get:poison,configurable:true});
      Object.defineProperty(Object.prototype,'keyData',{set:poison,configurable:true});
      const clone=structuredClone(key);delete Object.prototype.keyData;
      let exported;try{await crypto.subtle.exportKey('raw',clone)}catch(error){exported=error.name;}
      globalThis.__RESULT=JSON.stringify({calls,extractable:clone.extractable,usages:clone.usages,
        algorithm:clone.algorithm.name,type:clone.type,ownKeys:Reflect.ownKeys(clone).length,exported});
    "#).await;
    assert_eq!(value["calls"], 0, "{value}");
    assert_eq!(value["extractable"], false);
    assert_eq!(value["usages"], serde_json::json!(["encrypt"]));
    assert_eq!(value["algorithm"], "AES-GCM");
    assert_eq!(value["type"], "secret");
    assert_eq!(value["ownKeys"], 0);
    assert_eq!(value["exported"], "InvalidAccessError");
}

#[tokio::test(flavor = "current_thread")]
async fn kem_and_signature_clones_keep_real_material_without_export_permission() {
    let value = evaluate(r#"
      const rows={};
      for(const name of ['ML-KEM-768','ML-KEM-1024','MLKEM768-X25519']){
        const pair=await crypto.subtle.generateKey(name,false,['encapsulateBits','decapsulateBits']);
        const clone=structuredClone(pair),bits=await crypto.subtle.encapsulateBits(name,clone.publicKey);
        const secret=await crypto.subtle.decapsulateBits(name,clone.privateKey,bits.ciphertext);
        rows[name]={equal:Array.from(new Uint8Array(secret)).join()===Array.from(new Uint8Array(bits.sharedKey)).join(),
          private:!clone.privateKey.extractable,public:clone.publicKey.extractable,
          usages:clone.privateKey.usages,distinct:clone.privateKey!==pair.privateKey,
          ownKeys:Reflect.ownKeys(clone.privateKey).length};
      }
      const pair=await crypto.subtle.generateKey('Ed25519',false,['sign','verify']),clone=structuredClone(pair);
      const message=new Uint8Array([7,8,9]),signature=await crypto.subtle.sign('Ed25519',clone.privateKey,message);
      const verified=await crypto.subtle.verify('Ed25519',pair.publicKey,signature,message);
      globalThis.__RESULT=JSON.stringify({rows,verified,private:!clone.privateKey.extractable});
    "#).await;
    for (name, row) in value["rows"].as_object().unwrap() {
        for field in ["equal", "private", "public", "distinct"] {
            assert_eq!(row[field], true, "{name}: {value}");
        }
        assert_eq!(row["usages"], serde_json::json!(["decapsulateBits"]));
        assert_eq!(row["ownKeys"], 0);
    }
    assert_eq!(value["verified"], true);
    assert_eq!(value["private"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn cloning_into_child_realms_adapts_only_new_keys_and_message_data() {
    let value = evaluate(r#"
      const key=await crypto.subtle.importKey('raw',new Uint8Array(16),'AES-GCM',false,['encrypt']);
      const frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;
      const graph=child.structuredClone({key,date:new Date(42)}),borrowed=structuredClone.call(child,key);
      const channel=new MessageChannel();
      const delivered=new Promise(resolve=>channel.port1.onmessage=event=>resolve(event.data));
      channel.port2.postMessage({key});const message=await delivered;
      globalThis.__RESULT=JSON.stringify({child:graph.key instanceof child.CryptoKey&&!(graph.key instanceof CryptoKey),
        childObject:Object.getPrototypeOf(graph)===child.Object.prototype,
        childDate:graph.date instanceof child.Date,borrowed:borrowed instanceof child.CryptoKey&&!(borrowed instanceof CryptoKey),
        original:Object.getPrototypeOf(key)===CryptoKey.prototype,permission:!graph.key.extractable,
        message:message.key instanceof CryptoKey&&message.key!==key&&!message.key.extractable});
      channel.port1.close();channel.port2.close();frame.remove();
    "#).await;
    for (field, flag) in value.as_object().unwrap() {
        assert_eq!(*flag, true, "{field}: {value}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn reusable_clone_collector_matches_normal_chrome155() {
    let expected: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/crypto-clone-chrome155.json"
    ))
    .unwrap();
    let source = format!(
        "globalThis.__RESULT=await ({})",
        include_str!("../../../../harness/detection/crypto-clone-probe.js")
            .trim()
            .trim_end_matches(';')
    );
    let actual = evaluate(&source).await;
    assert_eq!(actual, expected["result"]);
}

#[tokio::test(flavor = "current_thread")]
async fn clone_propagates_getter_errors_and_rejects_real_events_without_reading_page_brands() {
    let value = evaluate(r#"
      const sentinel=new TypeError('authored getter sentinel'),object={get value(){throw sentinel;}};
      let getter=false;try{structuredClone(object)}catch(error){getter=error===sentinel;}
      const event=new Event('authored');
      Object.defineProperty(event,'constructor',{get(){throw sentinel;}});
      Object.defineProperty(Event,Symbol.hasInstance,{value(){throw sentinel;}});
      let real;try{structuredClone(event)}catch(error){real=error instanceof DOMException&&error.name==='DataCloneError';}
      const fake=structuredClone(Object.create(Event.prototype));
      globalThis.__RESULT=JSON.stringify({getter,real,fake:Object.getPrototypeOf(fake)===Object.prototype});
    "#).await;
    for (name, flag) in value.as_object().unwrap() {
        assert_eq!(*flag, true, "{name}: {value}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn private_event_targets_and_abort_objects_reject_cloning_across_realms() {
    let value = evaluate(r#"
      const frame=document.createElement('iframe');document.body.append(frame);
      const child=frame.contentWindow;
      class DerivedTarget extends EventTarget {}
      class DerivedController extends AbortController {}
      const controller=new AbortController(),foreign=new child.AbortController();
      const objects=[new EventTarget(),new DerivedTarget(),controller,controller.signal,
        new DerivedController(),new child.EventTarget(),foreign,foreign.signal];
      let hooks=0;const poison=()=>{hooks++;throw new Error('page brand hook');};
      for(const object of objects)Object.defineProperty(object,'constructor',{get:poison});
      for(const ctor of [EventTarget,AbortController,AbortSignal])
        Object.defineProperty(ctor,Symbol.hasInstance,{value:poison});
      const rejected=objects.map(object=>{
        try{structuredClone(object);return false;}
        catch(error){return error instanceof DOMException&&error.name==='DataCloneError';}
      });
      const forged=[EventTarget,AbortController,AbortSignal,child.EventTarget,child.AbortSignal]
        .map(ctor=>structuredClone(Object.create(ctor.prototype)))
        .every(object=>Object.getPrototypeOf(object)===Object.prototype&&Reflect.ownKeys(object).length===0);
      globalThis.__RESULT=JSON.stringify({rejected,hooks,forged,
        private:objects.every(object=>Reflect.ownKeys(object).join()==='constructor')});
      frame.remove();
    "#).await;
    assert_eq!(value["rejected"], serde_json::json!(vec![true; 8]));
    assert_eq!(value["hooks"], 0);
    assert_eq!(value["forged"], true);
    assert_eq!(value["private"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn window_and_foreign_port_messages_clone_native_keys_into_the_receiver_realm() {
    let value = evaluate(r#"
      const key=await crypto.subtle.importKey('raw',new Uint8Array(16),'AES-GCM',false,['encrypt']);
      const frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;
      let blocked=0;
      const received=new Promise(resolve=>child.addEventListener('message',event=>{
        if(event.data.label==='blocked'){blocked++;return;}
        const graph=event.data;
        resolve({realm:graph.key instanceof child.CryptoKey&&!(graph.key instanceof CryptoKey),
          alias:graph.key===graph.again,cycle:graph.self===graph,distinct:graph.key!==key,
          permissions:!graph.key.extractable&&graph.key.usages.join()==='encrypt',
          source:event.source===window,origin:event.origin===location.origin});
      }));
      const graph={label:'allowed',key,again:key};graph.self=graph;
      const sentinel=new TypeError('authored message getter');let getter=false;
      try{child.postMessage({get value(){throw sentinel;}},'https://different.test')}catch(error){getter=error===sentinel;}
      let invalid;try{child.postMessage(key,'invalid-target-origin')}catch(error){invalid=error instanceof child.DOMException&&error.name==='SyntaxError';}
      child.postMessage({label:'blocked',key},'https://different.test');
      child.postMessage(graph,location.origin+'/authored/path?query=fixture');const message=await received;
      const channel=new child.MessageChannel();
      const delivered=new Promise(resolve=>channel.port1.onmessage=event=>resolve({
        realm:event.data.key instanceof child.CryptoKey&&!(event.data.key instanceof CryptoKey),
        distinct:event.data.key!==key,permission:!event.data.key.extractable,
        event:event instanceof child.MessageEvent
      }));
      channel.port2.postMessage({key});const port=await delivered;
      channel.port1.close();channel.port2.close();frame.remove();
      globalThis.__RESULT=JSON.stringify({message,port,blocked,getter,invalid,original:Object.getPrototypeOf(key)===CryptoKey.prototype});
    "#).await;
    for group in ["message", "port"] {
        for (name, flag) in value[group].as_object().unwrap() {
            assert_eq!(*flag, true, "{group}.{name}: {value}");
        }
    }
    assert_eq!(value["blocked"], 0);
    assert_eq!(value["getter"], true);
    assert_eq!(value["invalid"], true);
    assert_eq!(value["original"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn genuine_xpath_objects_reject_cloning_while_forged_prototypes_are_plain() {
    let value = evaluate(r#"
      const objects=[new XPathEvaluator(),document.createExpression('count(//*)'),
        document.evaluate('count(//*)',document,null,XPathResult.NUMBER_TYPE)];
      const failures=[];
      for(const object of objects){
        Object.defineProperty(object,'constructor',{get(){throw new Error('guest constructor');}});
        try{structuredClone(object);failures.push(false);}
        catch(error){failures.push(error instanceof DOMException&&error.name==='DataCloneError');}
      }
      const fake=structuredClone(Object.create(XPathResult.prototype));
      globalThis.__RESULT=JSON.stringify({failures,fake:Object.getPrototypeOf(fake)===Object.prototype});
    "#).await;
    assert_eq!(value["failures"], serde_json::json!([true, true, true]));
    assert_eq!(value["fake"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn native_dom_exception_clone_preserves_private_state_and_receiver_realm() {
    let value = evaluate(r#"
      const Exception=DOMException,source=new Exception('authored','InvalidStateError');let hooks=0;
      for(const name of ['name','message','code'])Object.defineProperty(source,name,{
        get(){hooks++;throw new Error('guest exception getter');},configurable:true});
      const frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;
      globalThis.DOMException=function(){throw new Error('guest constructor');};
      const clone=structuredClone({source,again:source}),foreign=child.structuredClone(source);
      globalThis.DOMException=Exception;
      globalThis.__RESULT=JSON.stringify({hooks,brand:clone.source instanceof Exception,
        distinct:clone.source!==source,alias:clone.source===clone.again,
        name:clone.source.name,message:clone.source.message,code:clone.source.code,
        child:foreign instanceof child.DOMException&&!(foreign instanceof Exception),
        foreignName:foreign.name,foreignMessage:foreign.message,foreignCode:foreign.code});
      frame.remove();
    "#).await;
    assert_eq!(value["hooks"], 0);
    for name in ["brand", "distinct", "alias", "child"] {
        assert_eq!(value[name], true, "{name}: {value}");
    }
    for name in ["name", "foreignName"] {
        assert_eq!(value[name], "InvalidStateError");
    }
    for name in ["message", "foreignMessage"] {
        assert_eq!(value[name], "authored");
    }
    for name in ["code", "foreignCode"] {
        assert_eq!(value[name], 11);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn private_uncloneable_platform_brands_reject_without_page_property_inspection() {
    let value = evaluate(r#"
      const objects=[document.body.classList,document.querySelectorAll('authored-missing'),
        new URL('https://fixture.test/'),new URLSearchParams('a=1')],failures=[];
      for(const object of objects){
        Object.defineProperty(object,'constructor',{get(){throw new Error('guest constructor');}});
        try{structuredClone(object);failures.push(false);}
        catch(error){failures.push(error instanceof DOMException&&error.name==='DataCloneError');}
      }
      const fake=structuredClone(Object.create(URL.prototype));
      globalThis.__RESULT=JSON.stringify({failures,fake:Object.getPrototypeOf(fake)===Object.prototype});
    "#).await;
    assert_eq!(
        value["failures"],
        serde_json::json!([true, true, true, true])
    );
    assert_eq!(value["fake"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn user_timing_detail_uses_native_key_and_exception_cloning_and_preserves_getter_errors() {
    let value = evaluate(r#"
      const key=await crypto.subtle.importKey('raw',new Uint8Array(16),'AES-GCM',false,['encrypt']);
      const exception=new DOMException('authored','InvalidStateError'),detail={key,again:key,exception};detail.self=detail;
      const mark=performance.mark('authored-private-detail',{detail});
      const measure=performance.measure('authored-private-measure',{start:0,end:1,detail});
      const sentinel=new TypeError('authored detail getter');let getter=false;
      try{performance.mark('authored-failure',{detail:{get value(){throw sentinel;}}})}catch(error){getter=error===sentinel;}
      globalThis.__RESULT=JSON.stringify({key:mark.detail.key instanceof CryptoKey,
        distinct:mark.detail.key!==key,alias:mark.detail.key===mark.detail.again,
        cycle:mark.detail.self===mark.detail,permission:!mark.detail.key.extractable,
        exception:mark.detail.exception instanceof DOMException&&mark.detail.exception.code===11,
        same:mark.detail===mark.detail,measure:measure.detail.key instanceof CryptoKey&&measure.detail.key!==mark.detail.key,
        getter});
    "#).await;
    for (name, flag) in value.as_object().unwrap() {
        assert_eq!(*flag, true, "{name}: {value}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn array_buffer_transfers_detach_all_views_preserve_aliases_and_validate_before_mutating() {
    let value = evaluate(r#"
      const buffer=new ArrayBuffer(8),bytes=new Uint8Array(buffer),view=new DataView(buffer);
      bytes[0]=7;
      const copy=structuredClone({buffer,bytes,view,again:bytes},{transfer:[buffer]});
      const unused=new ArrayBuffer(2);structuredClone('authored',{transfer:[unused]});
      const failed=new ArrayBuffer(4),other=new ArrayBuffer(2),errors=[];
      const reject=callback=>{try{callback();errors.push(false)}catch(e){errors.push(e instanceof DOMException&&e.name==='DataCloneError')}};
      reject(()=>structuredClone(null,{transfer:[failed,failed]}));
      reject(()=>structuredClone(null,{transfer:[failed,{}]}));
      reject(()=>structuredClone({fn(){}},{transfer:[failed]}));
      reject(()=>structuredClone(null,{transfer:[failed,buffer]}));
      const sentinel=new Error('authored getter'),object={get value(){throw sentinel;}};let getter=false;
      try{structuredClone(object,{transfer:[failed]})}catch(e){getter=e===sentinel;}
      const detachedDuring=new ArrayBuffer(2),interrupted={get value(){structuredClone(detachedDuring,{transfer:[detachedDuring]});return 1;}};
      reject(()=>structuredClone(interrupted,{transfer:[other,detachedDuring]}));
      const resizable=new ArrayBuffer(2,{maxByteLength:8});new Uint8Array(resizable)[0]=9;
      const result=structuredClone(resizable,{transfer:[resizable]});result.resize(4);
      const frame=document.createElement('iframe');document.body.append(frame);
      const foreignSource=new Uint8Array([3]),foreign=structuredClone.call(frame.contentWindow,foreignSource,{transfer:[foreignSource.buffer]});
      globalThis.__RESULT=JSON.stringify({bytes:bytes.length,buffer:buffer.byteLength,
        viewError:(()=>{try{return view.byteLength}catch(e){return e.name}})(),
        value:copy.bytes[0],aliases:copy.bytes.buffer===copy.buffer&&copy.view.buffer===copy.buffer&&copy.bytes===copy.again,
        unused:unused.byteLength,errors,failed:failed.byteLength,other:other.byteLength,getter,
        resizable:result.resizable,maximum:result.maxByteLength,resized:result.byteLength,first:new Uint8Array(result)[0],
        foreign:foreign instanceof frame.contentWindow.Uint8Array&&foreign.buffer instanceof frame.contentWindow.ArrayBuffer,
        foreignValue:foreign[0],foreignSource:foreignSource.length});frame.remove();
    "#).await;
    assert_eq!(value["bytes"], 0);
    assert_eq!(value["buffer"], 0);
    assert_eq!(value["viewError"], "TypeError");
    assert_eq!(value["value"], 7);
    assert_eq!(value["aliases"], true);
    assert_eq!(value["unused"], 0);
    assert_eq!(
        value["errors"],
        serde_json::json!([true, true, true, true, true])
    );
    assert_eq!(value["failed"], 4);
    assert_eq!(value["other"], 2);
    assert_eq!(value["getter"], true);
    assert_eq!(value["resizable"], true);
    assert_eq!(value["maximum"], 8);
    assert_eq!(value["resized"], 4);
    assert_eq!(value["first"], 9);
    assert_eq!(value["foreign"], true);
    assert_eq!(value["foreignValue"], 3);
    assert_eq!(value["foreignSource"], 0);
}

#[tokio::test(flavor = "current_thread")]
async fn transfer_dictionary_and_sequence_conversion_preserves_webidl_getter_and_iterator_contracts(
) {
    let value = evaluate(
        r#"
      const error=callback=>{try{callback();return null}catch(e){return e.name}};
      const dictionaries=[undefined,null,()=>{},0,'',true,Symbol('authored')];
      const options=dictionaries.map(options=>error(()=>structuredClone(1,options)));
      const sequences=[null,0,'',{},[],[1],[{}]];
      const transfer=sequences.map(transfer=>error(()=>structuredClone(1,{transfer})));
      let getter=0,iterator=0,next=0,closed=0;
      const sequence={get [Symbol.iterator](){iterator++;return function(){return {
        next(){next++;return {done:false,value:1}},return(){closed++;return {done:true}}
      }}}};
      const converted=error(()=>structuredClone(null,{get transfer(){getter++;return sequence;}}));
      const buffer=new ArrayBuffer(1),valid={*[Symbol.iterator](){yield buffer}};
      const cloned=structuredClone(buffer,{transfer:valid});
      globalThis.__RESULT=JSON.stringify({options,transfer,getter,iterator,next,closed,converted,
        detached:buffer.byteLength===0,valid:cloned.byteLength===1});
    "#,
    )
    .await;
    assert_eq!(
        value["options"],
        serde_json::json!([
            null,
            null,
            null,
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError"
        ])
    );
    assert_eq!(
        value["transfer"],
        serde_json::json!([
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            null,
            "TypeError",
            "DataCloneError"
        ])
    );
    for name in ["getter", "iterator", "next"] {
        assert_eq!(value[name], 1, "{name}: {value}");
    }
    assert_eq!(value["closed"], 0);
    assert_eq!(value["converted"], "TypeError");
    assert_eq!(value["detached"], true);
    assert_eq!(value["valid"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn transfer_conversion_ignores_guest_array_helpers_and_inherited_index_setters() {
    let value = evaluate(r#"
      const bytes=new Uint8Array([7]),transfer=[bytes.buffer];let hooks=0,copy,error;
      const from=Array.from,push=Array.prototype.push,includes=Array.prototype.includes;
      const intercepted=()=>{hooks++;throw new Error('guest array helper');};
      Object.defineProperty(Array.prototype,'0',{configurable:true,set(){hooks++;}});
      Array.from=intercepted;Array.prototype.push=intercepted;Array.prototype.includes=intercepted;
      try{copy=structuredClone(bytes,{transfer});}catch(caught){error=caught.name;}
      finally{Array.from=from;Array.prototype.push=push;Array.prototype.includes=includes;delete Array.prototype[0];}
      globalThis.__RESULT=JSON.stringify({hooks,error:error??null,source:bytes.length,result:copy?.[0]});
    "#).await;
    assert_eq!(value["hooks"], 0, "{value}");
    assert_eq!(value["error"], Value::Null);
    assert_eq!(value["source"], 0);
    assert_eq!(value["result"], 7);
}
