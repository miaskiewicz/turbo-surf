//! Offline Web-IDL contracts measured by the shared Chrome/native collector.
use turbo_surf_render::run_with_dom;
fn evaluate(script: &str) -> serde_json::Value {
    serde_json::from_str(&run_with_dom("<body></body>", script).unwrap()).unwrap()
}
#[test]
fn required_arguments_and_optional_arities() {
    let value = evaluate(
        r#"(() => {
      const throws=fn=>{try{fn();return false;}catch(e){return e instanceof TypeError;}};
      return JSON.stringify({lengths:[Headers.length,Response.length,Request.length,URL.length,MessageEvent.length,TextDecoder.length,fetch.length,navigator.sendBeacon.length,Response.json.length,Response.redirect.length,Headers.prototype.forEach.length,postMessage.length],
      required:[throws(()=>new Request()),throws(()=>new URL()),throws(()=>new MessageEvent()),throws(()=>postMessage())],method:!Object.hasOwn(postMessage,'prototype'),headers:new Headers([['X-Test','value']]).get('x-test')});
    })()"#,
    );
    assert_eq!(
        value["lengths"],
        serde_json::json!([0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1])
    );
    assert_eq!(
        value["required"],
        serde_json::json!([true, true, true, true])
    );
    assert_eq!(value["method"], true);
    assert_eq!(value["headers"], "value");
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_interface_brands_accept_genuine_same_origin_foreign_realm_objects() {
    let mut page = turbo_surf_render::PageSession::open(
        "<body><iframe srcdoc='<body></body>'></iframe></body>",
        "https://brands.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let result=page.eval(r#"globalThis.__RESULT=(()=>{
      const child=document.querySelector('iframe').contentWindow;
      const headers=new child.Headers({'X-Test':'foreign'}),request=new child.Request('https://brands.test/path'),response=new child.Response('body');
      const get=(Constructor,name,receiver)=>Object.getOwnPropertyDescriptor(Constructor.prototype,name).get.call(receiver);
      const event=new child.MessageEvent('message',{data:'foreign'}),error=new child.DOMException('message','SecurityError');
      let forged=false;try{Headers.prototype.get.call(Object.create(child.Headers.prototype),'x-test')}catch(error){forged=error instanceof TypeError}
      return JSON.stringify({header:Headers.prototype.get.call(headers,'x-test'),request:get(Request,'url',request),response:get(Response,'status',response),message:get(MessageEvent,'data',event),error:get(DOMException,'code',error),forged});
    })()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result).unwrap(),
        serde_json::json!({"header":"foreign","request":"https://brands.test/path","response":200,"message":"foreign","error":18,"forged":true})
    );
}
#[tokio::test(flavor = "current_thread")]
async fn storage_is_branded_inherited_and_coerces_named_keys() {
    let mut page = turbo_surf_render::PageSession::open(
        "<body></body>",
        "https://storage.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let value:serde_json::Value=serde_json::from_str(&page.eval(r#"globalThis.__RESULT=(() => {
      const storage=localStorage,empty=Reflect.ownKeys(storage).length;storage.setItem(7,42);storage.named='property';
      let invalid=false;try{Storage.prototype.getItem.call({},'x');}catch(e){invalid=e instanceof TypeError;}
      let invalidLength=false;try{Object.getOwnPropertyDescriptor(Storage.prototype,'length').get.call({});}catch(e){invalidLength=e instanceof TypeError;}
      const result={empty,tag:Object.prototype.toString.call(storage),ctor:storage.constructor.name,ownMethod:Object.hasOwn(storage,'getItem'),values:[storage.getItem('7'),storage.named,storage.key(0),storage.length],invalid,invalidLength,nonconstructible:!Object.hasOwn(storage.getItem,'prototype'),independent:sessionStorage.length===0};delete storage.named;result.deleted=storage.getItem('named')===null;return JSON.stringify(result);
    })()"#).await.unwrap()).unwrap();
    assert_eq!(value["empty"], 0);
    assert_eq!(value["tag"], "[object Storage]");
    assert_eq!(value["ctor"], "Storage");
    assert_eq!(value["ownMethod"], false);
    assert_eq!(
        value["values"],
        serde_json::json!(["42", "property", "7", 2])
    );
    for key in [
        "invalid",
        "invalidLength",
        "nonconstructible",
        "independent",
        "deleted",
    ] {
        assert_eq!(value[key], true, "{value}");
    }
}
#[test]
fn native_urls_normalize_and_keep_live_search_params() {
    let value = evaluate(
        r#"(() => {
      const url=new URL('../a/./b?x=one#hash','https://EXAMPLE.test:443/base/page'),initial=url.href,params=url.searchParams;
      url.search='?second=2';params.append('third','3');url.pathname='/space path';url.port='443';
      let invalid=false;try{Object.getOwnPropertyDescriptor(URL.prototype,'hostname').get.call({});}catch(e){invalid=e instanceof TypeError;}
      return JSON.stringify({initial,href:url.href,identity:params===url.searchParams,unicode:new URL('https://mañana.example/').hostname,invalid,unsupported:URL.parse('https://[invalid')===null,opaque:new URL('data:text/plain,hello').origin});
    })()"#,
    );
    assert_eq!(value["initial"], "https://example.test/a/b?x=one#hash");
    assert_eq!(
        value["href"],
        "https://example.test/space%20path?second=2&third=3#hash"
    );
    assert_eq!(value["unicode"], "xn--maana-pta.example");
    assert_eq!(value["opaque"], "null");
    for key in ["identity", "invalid", "unsupported"] {
        assert_eq!(value[key], true, "{value}");
    }
}
#[test]
fn navigator_getters_require_actual_receiver() {
    let value = evaluate(
        r#"(() => {const descriptor=Object.getOwnPropertyDescriptor(Navigator.prototype,'languages');let invalid=false;try{descriptor.get.call({});}catch(e){invalid=e instanceof TypeError;}return JSON.stringify({invalid,name:descriptor.get.name,method:!Object.hasOwn(descriptor.get,'prototype'),same:descriptor.get.call(navigator)===navigator.languages});})()"#,
    );
    assert_eq!(
        value,
        serde_json::json!({"invalid":true,"name":"get languages","method":true,"same":true})
    );
}
#[tokio::test(flavor = "current_thread")]
async fn crypto_uses_private_branded_interfaces_and_preserves_real_digest() {
    let value: serde_json::Value = serde_json::from_str(&turbo_surf_render::eval_async("<body></body>", "https://example.test/", r#"(async () => {
      const subtle=crypto.subtle;let badCrypto=false,badSubtle=false,badConstructor=false;
      try{Crypto.prototype.getRandomValues.call({},new Uint8Array(1));}catch(error){badCrypto=error instanceof TypeError;}
      try{await SubtleCrypto.prototype.digest.call({},'SHA-256',new Uint8Array());}catch(error){badSubtle=error instanceof TypeError;}
      try{new Crypto();}catch(error){badConstructor=error instanceof TypeError;}
      const bytes=new Uint8Array(await subtle.digest('SHA-256',new TextEncoder().encode('abc')));
      globalThis.__RESULT=JSON.stringify({badCrypto,badSubtle,badConstructor,own:Reflect.ownKeys(crypto),subtleOwn:Reflect.ownKeys(subtle),owners:[Object.getPrototypeOf(crypto)===Crypto.prototype,Object.getPrototypeOf(subtle)===SubtleCrypto.prototype],tags:[Object.prototype.toString.call(crypto),Object.prototype.toString.call(subtle)],digest:Array.from(bytes,byte=>byte.toString(16).padStart(2,'0')).join('')});
    })()"#).await.unwrap()).unwrap();
    for key in ["badCrypto", "badSubtle", "badConstructor"] {
        assert_eq!(value[key], true, "{value}");
    }
    assert_eq!(value["own"], serde_json::json!([]));
    assert_eq!(value["subtleOwn"], serde_json::json!([]));
    assert_eq!(value["owners"], serde_json::json!([true, true]));
    assert_eq!(
        value["tags"],
        serde_json::json!(["[object Crypto]", "[object SubtleCrypto]"])
    );
    assert_eq!(
        value["digest"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
#[test]
fn performance_and_permissions_methods_are_owned_by_branded_prototypes() {
    let value = evaluate(
        r#"(() => {
      let invalid=false;try{performance.now.call({});}catch(e){invalid=e instanceof TypeError;}
      return JSON.stringify({invalid,performanceTag:Object.prototype.toString.call(performance),performanceOwn:Reflect.ownKeys(performance),permissionsTag:Object.prototype.toString.call(navigator.permissions),permissionsOwn:Reflect.ownKeys(navigator.permissions),clock:performance.now()>=0,owners:[Object.getPrototypeOf(performance)===Performance.prototype,Object.getPrototypeOf(navigator.permissions)===Permissions.prototype]});
    })()"#,
    );
    assert_eq!(value["performanceTag"], "[object Performance]");
    assert_eq!(value["permissionsTag"], "[object Permissions]");
    assert_eq!(value["performanceOwn"], serde_json::json!([]));
    assert_eq!(value["permissionsOwn"], serde_json::json!([]));
    assert_eq!(value["owners"], serde_json::json!([true, true]));
    assert_eq!(value["invalid"], true);
    assert_eq!(value["clock"], true);
}
#[test]
fn header_sequences_validation_and_private_branding() {
    let value = evaluate(
        r#"(() => {
      const headers=new Headers([['Z-Test',' last '],['a-test','one'],['A-Test','two']]);
      const errors=[];for(const call of [()=>headers.set('bad name','x'),()=>headers.set('x','line\nbreak'),()=>Headers.prototype.get.call({},'x'),()=>new Headers([['incomplete']])]){try{call();errors.push(false);}catch(e){errors.push(e instanceof TypeError);}}
      return JSON.stringify({values:Array.from(headers),own:Reflect.ownKeys(headers),errors});
    })()"#,
    );
    assert_eq!(
        value["values"],
        serde_json::json!([["a-test", "one, two"], ["z-test", "last"]])
    );
    assert_eq!(value["own"], serde_json::json!([]));
    assert_eq!(value["errors"], serde_json::json!([true, true, true, true]));
}
#[test]
fn canvas_dimensions_are_reflected_live_accessors_and_methods_reject_foreign_receivers() {
    let value = evaluate(
        r#"(() => {
      const canvas=document.createElement('canvas'),initial=[canvas.width,canvas.height];
      canvas.width=12;canvas.setAttribute('height','9');
      const width=Object.getOwnPropertyDescriptor(HTMLCanvasElement.prototype,'width');
      const invalid=[];for(const call of [()=>width.get.call({}),()=>canvas.getContext.call({},'2d'),()=>canvas.toDataURL.call({}),()=>canvas.getContext.call(document.body,'2d')]){try{call();invalid.push(false);}catch(e){invalid.push(e instanceof TypeError);}}
      const reflected=[canvas.width,canvas.height,canvas.getAttribute('width')];canvas.width=0;
      return JSON.stringify({initial,reflected,invalid,empty:canvas.toDataURL(),tag:Object.prototype.toString.call(HTMLElement.prototype)});
    })()"#,
    );
    assert_eq!(value["initial"], serde_json::json!([300, 150]));
    assert_eq!(value["reflected"], serde_json::json!([12, 9, "12"]));
    assert_eq!(
        value["invalid"],
        serde_json::json!([true, true, true, true])
    );
    assert_eq!(value["empty"], "data:,");
    assert_eq!(value["tag"], "[object HTMLElement]");
}
#[test]
fn search_params_preserve_raw_url_until_mutation_and_follow_form_encoding() {
    let value = evaluate(
        r#"(() => {
      const url=new URL('https://example.test/?a=%7e&b=two%20words');const before=url.href;
      const params=url.searchParams;params.sort();const after=url.href;
      const sequence=new URLSearchParams([['a','first'],['b','between'],['a','last']]);sequence.set('a','new');
      const decoded=new URLSearchParams('x=%FF&y=hello+world');
      url.host='EXAMPLE.test:8080';url.hostname='new.test';
      return JSON.stringify({before,after,sequence:Array.from(sequence),decoded:[decoded.get('x'),decoded.get('y')],own:Reflect.ownKeys(params),host:url.host,identity:params===url.searchParams});
    })()"#,
    );
    assert_eq!(value["before"], "https://example.test/?a=%7e&b=two%20words");
    assert_eq!(value["after"], "https://example.test/?a=%7E&b=two+words");
    assert_eq!(
        value["sequence"],
        serde_json::json!([["a", "new"], ["b", "between"]])
    );
    assert_eq!(value["decoded"], serde_json::json!(["�", "hello world"]));
    assert_eq!(value["own"], serde_json::json!([]));
    assert_eq!(value["host"], "new.test:8080");
    assert_eq!(value["identity"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn response_and_request_use_private_branded_state_and_consume_bodies_once() {
    let raw = turbo_surf_render::eval_async("<body></body>", "https://example.test/base/", r#"(async () => {
      const response=new Response('é'), clone=response.clone();
      const stream=response.body, stable=stream===response.body;
      const result={own:Reflect.ownKeys(response),stable,type:response.type,contentType:response.headers.get('content-type'),bytes:Array.from(await clone.bytes()),text:await response.text(),used:response.bodyUsed};
      try{await response.text();result.again=false;}catch(error){result.again=error instanceof TypeError;}
      try{response.clone();result.cloneUsed=false;}catch(error){result.cloneUsed=error instanceof TypeError;}
      try{Response.prototype.clone.call({});result.invalid=false;}catch(error){result.invalid=error instanceof TypeError;}
      const request=new Request('../submit',{method:'post',body:new URLSearchParams([['a','two words']])});
      result.request={own:Reflect.ownKeys(request),url:request.url,method:request.method,type:request.headers.get('content-type'),text:await request.text(),duplex:request.duplex};
      result.nullBody=new Response().body===null;result.error=[Response.error().status,Response.error().type];
      globalThis.__RESULT=JSON.stringify(result);
    })()"#).await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(value["own"], serde_json::json!([]));
    assert_eq!(value["bytes"], serde_json::json!([195, 169]));
    assert_eq!(value["text"], "é");
    assert_eq!(value["type"], "default");
    assert_eq!(value["contentType"], "text/plain;charset=UTF-8");
    assert_eq!(
        value["request"],
        serde_json::json!({"own":[],"url":"https://example.test/submit","method":"POST","type":"application/x-www-form-urlencoded;charset=UTF-8","text":"a=two+words","duplex":"half"})
    );
    for key in [
        "stable",
        "used",
        "again",
        "cloneUsed",
        "invalid",
        "nullBody",
    ] {
        assert_eq!(value[key], true, "{value}");
    }
    assert_eq!(value["error"], serde_json::json!([0, "error"]));
}
#[tokio::test(flavor = "current_thread")]
async fn cloned_stream_bodies_are_independent_and_blob_parts_preserve_binary_bytes() {
    let raw=turbo_surf_render::eval_async("<body></body>","https://example.test/",r#"(async()=>{
      const response=new Response(new ReadableStream({start(c){c.enqueue(new Uint8Array([65,66]));c.close();}}));
      const stream=response.body,clone=response.clone();const text=await Promise.all([response.text(),clone.text()]);
      const blob=new Blob([new Uint8Array([0,255]),'é'],{type:'APPLICATION/OCTET-STREAM'});
      globalThis.__RESULT=JSON.stringify({text,bytes:Array.from(await blob.bytes()),size:blob.size,type:blob.type,own:Reflect.ownKeys(blob),slice:Array.from(await blob.slice(-2).bytes())});
    })()"#).await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"text":["AB","AB"],"bytes":[0,255,195,169],"size":4,"type":"application/octet-stream","own":[],"slice":[195,169]})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn body_text_stream_decodes_split_utf8_and_marks_disturbed_immediately() {
    let raw=turbo_surf_render::eval_async("<body></body>","https://example.test/",r#"(async()=>{const response=new Response(new ReadableStream({start(c){c.enqueue(new Uint8Array([195]));c.enqueue(new Uint8Array([169]));c.close();}}));const stream=response.textStream(),used=response.bodyUsed,reader=stream.getReader();const result=[];while(true){const part=await reader.read();if(part.done)break;result.push(part.value);}globalThis.__RESULT=JSON.stringify({used,result,address:new Request('https://example.test/').targetAddressSpace});})()"#).await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"used":true,"result":["é"],"address":"unknown"})
    );
}
#[test]
fn window_properties_separates_window_and_event_target_prototypes() {
    let value = evaluate(
        r#"(()=>{const windowProto=Object.getPrototypeOf(globalThis),properties=Object.getPrototypeOf(windowProto);return JSON.stringify({window:windowProto===Window.prototype,tag:Object.prototype.toString.call(properties),target:Object.getPrototypeOf(properties)===EventTarget.prototype,ownEvent:Object.hasOwn(properties,'addEventListener'),globalTag:Object.hasOwn(globalThis,Symbol.toStringTag)});})()"#,
    );
    assert_eq!(
        value,
        serde_json::json!({"window":true,"tag":"[object WindowProperties]","target":true,"ownEvent":false,"globalTag":false})
    );
}

#[test]
fn native_decoding_handles_legacy_labels_bom_fatal_and_encoder_boundaries() {
    let value = evaluate(
        r#"(()=>{const decoder=new TextDecoder(),bom=new Uint8Array([239,187,191,65]),encoder=new TextEncoder(),small=new Uint8Array(3);const result={label:new TextDecoder('latin1').encoding,legacy:new TextDecoder('latin1').decode(new Uint8Array([128])),bom:[decoder.decode(bom),new TextDecoder('utf-8',{ignoreBOM:true}).decode(bom)],bad:decoder.decode(new Uint8Array([255])),boundary:encoder.encodeInto('é😀',small),bytes:Array.from(small),own:[Reflect.ownKeys(decoder),Reflect.ownKeys(encoder)]};try{new TextDecoder('utf-8',{fatal:true}).decode(new Uint8Array([255]));result.fatal=false;}catch(error){result.fatal=error instanceof TypeError;}return JSON.stringify(result);})()"#,
    );
    assert_eq!(
        value,
        serde_json::json!({"label":"windows-1252","legacy":"€","bom":["A","\u{feff}A"],"bad":"�","boundary":{"read":1,"written":2},"bytes":[195,169,0],"own":[[],[]],"fatal":true})
    );
}
#[tokio::test(flavor = "current_thread")]
async fn cryptographic_random_values_use_typed_bytes_and_standard_uuid_bits() {
    let raw = turbo_surf_render::eval_async("<body></body>","https://crypto.test/",
        r#"(()=>{const values=new Uint32Array(8),returned=crypto.getRandomValues(values),uuid=crypto.randomUUID();const errors=[];for(const call of [()=>crypto.getRandomValues(new Float32Array(1)),()=>crypto.getRandomValues(new Uint8Array(65537))]){try{call();errors.push('none');}catch(error){errors.push(error.name);}}globalThis.__RESULT=JSON.stringify({same:values===returned,nonzero:values.some(value=>value!==0),uuid:/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(uuid),errors});})()"#,
    ).await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"same":true,"nonzero":true,"uuid":true,"errors":["TypeMismatchError","QuotaExceededError"]})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn network_response_preserves_raw_bytes_visible_headers_and_cookie_privacy() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        loop {
            let mut bytes = [0; 1024];
            let count = socket.read(&mut bytes).await.unwrap();
            assert!(count > 0);
            request.extend_from_slice(&bytes[..count]);
            if request.windows(4).any(|part| part == b"\r\n\r\n") {
                break;
            }
        }
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nX-Test: retained\r\nSet-Cookie: secret=private; HttpOnly; Path=/\r\nContent-Length: 2\r\nConnection: close\r\n\r\n\x00\xff").await.unwrap();
    });
    let raw=turbo_surf_render::eval_async("<body></body>",&base,r#"(async()=>{const response=await fetch('/bytes');globalThis.__RESULT=JSON.stringify({bytes:Array.from(await response.bytes()),header:response.headers.get('x-test'),cookie:response.headers.get('set-cookie'),documentCookie:document.cookie,type:response.type,url:response.url,own:Reflect.ownKeys(response)});})()"#).await.unwrap();
    server.await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(value["bytes"], serde_json::json!([0, 255]));
    assert_eq!(value["header"], "retained");
    assert_eq!(value["cookie"], serde_json::Value::Null);
    assert_eq!(value["documentCookie"], "");
    assert_eq!(value["type"], "basic");
    assert_eq!(value["url"], format!("{base}bytes"));
    assert_eq!(value["own"], serde_json::json!([]));
}

#[test]
fn headers_and_search_parameter_iterators_follow_mutations_and_preserve_aliases() {
    let value = evaluate(
        r#"(() => {
      const headers=new Headers([['a','one'],['set-cookie','first=1'],['set-cookie','second=2']]);
      const hi=headers.entries(), first=hi.next().value;headers.set('b','two');
      const params=new URLSearchParams('a=one&b=two'),pi=params.entries();pi.next();params.set('b','changed');params.append('c','last');
      let bad=false;try{Headers.prototype.entries.call({});}catch(error){bad=error instanceof TypeError;}
      return JSON.stringify({first,remaining:Array.from(hi),cookies:headers.getSetCookie(),combined:headers.get('set-cookie'),params:Array.from(pi),aliases:[Headers.prototype.entries===Headers.prototype[Symbol.iterator],URLSearchParams.prototype.entries===URLSearchParams.prototype[Symbol.iterator]],bad});
    })()"#,
    );
    assert_eq!(value["first"], serde_json::json!(["a", "one"]));
    assert_eq!(
        value["remaining"],
        serde_json::json!([
            ["b", "two"],
            ["set-cookie", "first=1"],
            ["set-cookie", "second=2"]
        ])
    );
    assert_eq!(value["cookies"], serde_json::json!(["first=1", "second=2"]));
    assert_eq!(value["combined"], "first=1, second=2");
    assert_eq!(
        value["params"],
        serde_json::json!([["b", "changed"], ["c", "last"]])
    );
    assert_eq!(value["aliases"], serde_json::json!([true, true]));
    assert_eq!(value["bad"], true);
}

#[test]
fn geometry_matrices_compute_transforms_inverses_and_private_receiver_state() {
    let value = evaluate(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../harness/detection/geometry-probe.js"
    )));
    assert_eq!(value["alias"], true);
    assert_eq!(value["private"], serde_json::json!([]));
    assert_eq!(
        value["point"],
        serde_json::json!({"x":18,"y":35,"z":0,"w":1})
    );
    for (key, expected) in [("x", 4.0), ("y", 5.0), ("z", 0.0), ("w", 1.0)] {
        assert!(
            (value["roundTrip"][key].as_f64().unwrap() - expected).abs() < 1e-6,
            "{value}"
        );
    }
    assert_eq!(value["product"], serde_json::json!([2, 3, 24, 44]));
    assert_eq!(value["rotation"], serde_json::json!([0, 1]));
    assert_eq!(value["readonly"], serde_json::json!([1, 2, 3, 4, 5, 6]));
    assert_eq!(value["singular"], serde_json::json!([true, false]));
    assert_eq!(
        value["errors"],
        serde_json::json!(["TypeError", "SyntaxError", "TypeError", "SyntaxError"])
    );
    assert_eq!(value["illegal"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn subtle_crypto_validates_buffer_ranges_and_rejects_unsupported_algorithms_and_forged_keys()
{
    let raw = turbo_surf_render::eval_async("<body></body>", "https://example.test/", r#"(async()=>{
      const bytes=new Uint8Array([0,97,98,99,0]);
      const hash=new Uint8Array(await crypto.subtle.digest('SHA-256',new DataView(bytes.buffer,1,3)));
      const errors=[];
      for(const call of [()=>crypto.subtle.digest(),()=>crypto.subtle.digest('SHA-256','abc'),()=>crypto.subtle.digest('unknown',bytes),()=>crypto.subtle.wrapKey(),()=>crypto.subtle.wrapKey('raw',{}, {}, {})]) {
        try{await call();errors.push(null);}catch(error){errors.push(error.name);}
      }
      globalThis.__RESULT=JSON.stringify({digest:Array.from(hash,b=>b.toString(16).padStart(2,'0')).join(''),errors,arities:[crypto.subtle.wrapKey.length,crypto.subtle.unwrapKey.length,crypto.subtle.importKey.length]});
    })()"#).await.unwrap();
    let result: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        result["digest"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        result["errors"],
        serde_json::json!([
            "TypeError",
            "TypeError",
            "NotSupportedError",
            "TypeError",
            "TypeError"
        ])
    );
    assert_eq!(result["arities"], serde_json::json!([4, 7, 5]));
}

#[test]
fn browser_iterators_have_private_native_metadata_and_preserve_live_mutations() {
    let probe = include_str!("../../../../harness/detection/iterator-probe.js");
    let result = evaluate(probe);
    for (name, tag) in [
        ("headers", "[object Headers Iterator]"),
        ("params", "[object URLSearchParams Iterator]"),
    ] {
        let value = &result[name];
        assert_eq!(value["tag"], tag);
        assert_eq!(value["own"], serde_json::json!([]));
        assert_eq!(
            value["prototypes"][1]["keys"],
            serde_json::json!(["Symbol(Symbol.toStringTag)", "next"])
        );
        assert_eq!(value["next"]["source"], "function next() { [native code] }");
        assert_eq!(value["next"]["own"], serde_json::json!(["length", "name"]));
        assert_eq!(value["invalid"], "TypeError");
        assert_eq!(value["iterable"], true);
        assert_eq!(
            value["live"],
            serde_json::json!([{"value":["a","one"],"done":false},{"value":["b","two"],"done":false},{"done":true}])
        );
    }
}

#[test]
fn html_offset_metrics_have_the_native_interface_owner_and_receiver_guard() {
    let value = evaluate(
        r#"(()=>{
      const get=Object.getOwnPropertyDescriptor(HTMLElement.prototype,'offsetWidth').get;
      const errors=[];
      for(const target of [{},document,document.createElementNS('http://www.w3.org/2000/svg','svg')]){
        try{get.call(target);errors.push(false);}catch(error){errors.push(error instanceof TypeError);}
      }
      return JSON.stringify({errors,base:!Object.hasOwn(Node.prototype,'offsetWidth')&&!Object.hasOwn(EventTarget.prototype,'offsetWidth'),source:Function.prototype.toString.call(get),detached:document.createElement('span').offsetWidth});
    })()"#,
    );
    assert_eq!(value["errors"], serde_json::json!([true, true, true]));
    assert_eq!(value["base"], true);
    assert_eq!(
        value["source"],
        "function get offsetWidth() { [native code] }"
    );
    assert_eq!(value["detached"], 0);
}

#[tokio::test(flavor = "current_thread")]
async fn broadcast_channel_dispatches_native_targets_and_cloned_messages() {
    let mut page = turbo_surf_render::PageSession::open(
        "<!doctype html><body></body>",
        "https://channels.test/",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let result = page.eval(r#"
      const sender=new BroadcastChannel('records'),receiver=new BroadcastChannel('records');
      const messages=[];
      receiver.addEventListener('message',event=>messages.push({value:event.data.value,target:event.target===receiver,current:event.currentTarget===receiver,event:event instanceof MessageEvent}));
      const data={value:'before'};sender.postMessage(data);data.value='after';
      const closed=new BroadcastChannel('records');closed.close();
      let closedError='',brandError='';
      try{closed.postMessage('blocked')}catch(error){closedError=error.name}
      try{BroadcastChannel.prototype.postMessage.call(Object.create(BroadcastChannel.prototype),'forged')}catch(error){brandError=error.name}
      setTimeout(()=>{globalThis.__RESULT=JSON.stringify({messages,closedError,brandError,keys:Object.keys(sender)})},20);
    "#).await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        value["messages"],
        serde_json::json!([{"value":"before","target":true,"current":true,"event":true}])
    );
    assert_eq!(value["closedError"], "InvalidStateError");
    assert_eq!(value["brandError"], "TypeError");
    assert_eq!(value["keys"], serde_json::json!([]));
}

#[test]
fn structured_clone_preserves_native_types_and_rejects_uncloneable_values() {
    let value = evaluate(
        r#"(()=>{
      const buffer=new Uint8Array([1,2,3]),date=new Date(123),map=new Map([[1,new Set(['value'])]]);
      const source={buffer,date,map,integer:42n};source.self=source;
      const copy=structuredClone(source);
      const error=fn=>{try{fn();return '';}catch(e){return e instanceof DOMException?e.name:'wrong-brand';}};
      return JSON.stringify({cycle:copy.self===copy,buffer:copy.buffer instanceof Uint8Array && copy.buffer!==buffer && copy.buffer[1]===2,date:copy.date instanceof Date && +copy.date===123,map:copy.map instanceof Map && copy.map.get(1) instanceof Set && copy.map.get(1).has('value'),integer:copy.integer===42n,function:error(()=>structuredClone({fn(){}})),proxy:error(()=>structuredClone(new Proxy({},{}))),node:error(()=>structuredClone(document.body)),transfer:error(()=>structuredClone(buffer,{transfer:[buffer.buffer]})),attached:buffer.byteLength===3});
    })()"#,
    );
    assert_eq!(
        value,
        serde_json::json!({"cycle":true,"buffer":true,"date":true,"map":true,"integer":true,"function":"DataCloneError","proxy":"DataCloneError","node":"DataCloneError","transfer":"NotSupportedError","attached":true})
    );
}
