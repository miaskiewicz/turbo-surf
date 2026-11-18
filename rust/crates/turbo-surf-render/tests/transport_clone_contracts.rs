//! Public browser contracts for view rehoming, storage cloning and retired authority.
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::{AuxiliaryHostServices, AuxiliaryResourcePolicy, PageSession};

#[tokio::test(flavor = "current_thread")]
async fn byob_reads_preserve_all_view_brands_offsets_and_bytes() {
    let mut page = PageSession::open("<body>", "https://byob-contract.test/", "", "", 5000)
        .await
        .unwrap();
    let result = page.eval_json_expression(r#"(async()=>{
      const types=[Uint8Array,Uint8ClampedArray,Int8Array,Uint16Array,Int16Array,Uint32Array,Int32Array,Float16Array,Float32Array,Float64Array,BigUint64Array,BigInt64Array,DataView];
      const rows=[];
      for(const Type of types){
        const original=new ArrayBuffer(32),width=Type.BYTES_PER_ELEMENT||1;
        const input=new Type(original,8,16/width);
        const payload=Uint8Array.from({length:16},(_,index)=>index+1);
        const stream=new ReadableStream({type:'bytes',pull(controller){controller.enqueue(payload);controller.close();}});
        const reader=stream.getReader({mode:'byob'}),result=await reader.read(input),value=result.value;
        rows.push([Type.name,Object.getPrototypeOf(value)===Type.prototype,value.buffer instanceof ArrayBuffer,value.byteOffset,value.byteLength,result.done,original.byteLength,[...new Uint8Array(value.buffer,value.byteOffset,value.byteLength)]]);
        reader.releaseLock();
      }
      return rows;
    })()"#).await.unwrap();
    let actual: Value = serde_json::from_str(&result).unwrap();
    let names = [
        "Uint8Array",
        "Uint8ClampedArray",
        "Int8Array",
        "Uint16Array",
        "Int16Array",
        "Uint32Array",
        "Int32Array",
        "Float16Array",
        "Float32Array",
        "Float64Array",
        "BigUint64Array",
        "BigInt64Array",
        "DataView",
    ];
    assert_eq!(actual.as_array().unwrap().len(), names.len());
    for (row, name) in actual.as_array().unwrap().iter().zip(names) {
        assert_eq!(
            *row,
            json!([
                name,
                true,
                true,
                8,
                16,
                false,
                0,
                (1..=16).collect::<Vec<_>>()
            ]),
            "{name}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn history_storage_clone_failures_leave_the_current_entry_unchanged() {
    let headers = vec![
        ("Cross-Origin-Opener-Policy".into(), "same-origin".into()),
        ("Cross-Origin-Embedder-Policy".into(), "require-corp".into()),
    ];
    let services = AuxiliaryHostServices {
        policy: AuxiliaryResourcePolicy {
            max_history_state_bytes: 256,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut page = PageSession::open_with_document_response_and_auxiliary_services(
        "<body>",
        "https://history-clone.test/start",
        "",
        "",
        5000,
        "",
        Default::default(),
        &headers,
        Default::default(),
        Default::default(),
        None,
        Default::default(),
        "",
        None,
        services,
    )
    .await
    .unwrap();
    let result=page.eval_json_expression(r#"(()=>{
      if(!crossOriginIsolated)throw new Error('Shared-memory fixture must be isolated');
      history.replaceState({accepted:1},'', '/kept');
      const before=JSON.stringify([location.href,history.length,history.state]);
      const shared=new SharedArrayBuffer(8),module=new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0]));
      const cloneable=[structuredClone(shared).byteLength===8,structuredClone(module) instanceof WebAssembly.Module];
      let reads=0;const large={get payload(){reads++;return 'x'.repeat(1024)}};
      const rows=[];
      for(const [value,fragment] of [[shared,'Shared memory'],[module,'WebAssembly modules'],[large,'storage budget exceeded']]){
        try{history.pushState(value,'','/rejected');rows.push(['accepted']);}
        catch(error){rows.push([error.name,error.message.includes(fragment),JSON.stringify([location.href,history.length,history.state])===before]);}
      }
      history.pushState({accepted:2},'', '/after');
      return {cloneable,rows,reads,after:history.state.accepted,pathname:location.pathname};
    })()"#).await.unwrap();
    let actual: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        actual,
        json!({"cloneable":[true,true],"rows":[["DataCloneError",true,true],["DataCloneError",true,true],["DataCloneError",true,true]],"reads":1,"after":2,"pathname":"/after"})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn locking_a_transfer_listed_stream_in_a_getter_preserves_its_reader() {
    let mut page = PageSession::open("<body>", "https://stream-transfer.test/", "", "", 5000)
        .await
        .unwrap();
    let result=page.eval_json_expression(r#"(async()=>{
      const stream=new ReadableStream({start(controller){controller.enqueue(7);controller.close();}});
      let reader,reads=0,failure;
      try{structuredClone({get payload(){reads++;reader=stream.getReader();return 0;}},{transfer:[stream]});failure='accepted';}
      catch(error){failure=error.name;}
      const locked=stream.locked,first=await reader.read(),end=await reader.read();reader.releaseLock();
      return {failure,reads,locked,first,end,unlocked:!stream.locked};
    })()"#).await.unwrap();
    let actual: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        actual,
        json!({"failure":"TypeError","reads":1,"locked":true,"first":{"value":7,"done":false},"end":{"done":true},"unlocked":true})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn retained_crypto_methods_cannot_start_work_for_a_retired_frame() {
    let mut page = PageSession::open("<body>", "https://retired-crypto.test/", "", "", 5000)
        .await
        .unwrap();
    let result=page.eval_json_expression(r#"(async()=>{
      const frame=document.createElement('iframe');document.body.append(frame);
      const child=frame.contentWindow,subtle=child.crypto.subtle,digest=subtle.digest;
      const activeHidden=child.Function('return typeof Deno')()==='undefined';frame.remove();
      let rejected=false,ownRealm=false,hidden=false,codegenDenied=false;
      try{await digest.call(subtle,'SHA-256',new Uint8Array([1]));}
      catch(error){rejected=true;ownRealm=error instanceof child.TypeError&&!(error instanceof TypeError);hidden=activeHidden&&error.constructor.constructor===child.Function;try{error.constructor.constructor('return typeof Deno')();}catch(generation){codegenDenied=generation instanceof child.EvalError&&generation.message.includes('active guest document');}}
      const healthy=await crypto.subtle.digest('SHA-256',new Uint8Array());
      return {rejected,ownRealm,hidden,codegenDenied,healthy:healthy.byteLength};
    })()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&result).unwrap(),
        json!({"rejected":true,"ownRealm":true,"hidden":true,"codegenDenied":true,"healthy":32})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_eval_delivers_response_storage_clears_to_existing_realms() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let mut bytes = [0; 1024];
            let count = socket.read(&mut bytes).await.unwrap();
            assert!(count > 0);
            request.extend_from_slice(&bytes[..count]);
        }
        assert!(String::from_utf8_lossy(&request).starts_with("GET /clear "));
        socket.write_all(b"HTTP/1.1 200 OK\r\nClear-Site-Data: \"storage\"\r\nCache-Control: no-store\r\nContent-Length: 7\r\nConnection: close\r\n\r\ncleared").await.unwrap();
    });
    let result=turbo_surf_render::eval_async("<body>",&base,r#"(async()=>{
      localStorage.setItem('seed','local');sessionStorage.setItem('seed','session');
      const frame=document.createElement('iframe'),loaded=new Promise(resolve=>frame.onload=resolve);document.body.append(frame);await loaded;
      const child=frame.contentWindow,rows=[];let finish;
      const cleared=new Promise(resolve=>finish=resolve);
      const capture=(owner,event)=>{rows.push([owner,event.key,event.oldValue,event.newValue,event.storageArea===((owner==='root'?window:child).localStorage)?'local':'session']);if(rows.length===4)finish();};
      addEventListener('storage',event=>capture('root',event));child.addEventListener('storage',event=>capture('child',event));
      const body=await (await fetch('/clear')).text();await cleared;
      rows.sort((a,b)=>JSON.stringify(a).localeCompare(JSON.stringify(b)));
      globalThis.__RESULT=JSON.stringify({body,rows,empty:[localStorage.length,sessionStorage.length,child.localStorage.length,child.sessionStorage.length]});
    })()"#).await.unwrap();
    let actual: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        actual,
        json!({"body":"cleared","rows":[["child",null,null,null,"local"],["child",null,null,null,"session"],["root",null,null,null,"local"],["root",null,null,null,"session"]],"empty":[0,0,0,0]})
    );
    server.await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn hybrid_key_clones_keep_native_material_permissions_and_guest_brands() {
    let fixture =
        include_str!("../../../../harness/detection/fixtures/webcrypto-xwing-chrome155.json");
    let script = r#"
      const name='MLKEM768-X25519',seed=Uint8Array.from(fixture.seedHex.match(/../g),pair=>parseInt(pair,16));
      const original=await crypto.subtle.importKey('raw-seed',seed,name,false,['decapsulateBits']);
      const privateCopy=structuredClone(original),publicKey=await crypto.subtle.getPublicKey(privateCopy,['encapsulateBits']),publicCopy=structuredClone(publicKey);
      const sealed=await crypto.subtle.encapsulateBits(name,publicCopy),opened=await crypto.subtle.decapsulateBits(name,privateCopy,sealed.ciphertext);
      const expected=new Uint8Array(sealed.sharedKey),actual=new Uint8Array(opened);
      let exportName;try{await crypto.subtle.exportKey('raw-seed',privateCopy);exportName='exported';}catch(error){exportName=error.name;}
      return {distinct:privateCopy!==original&&publicCopy!==publicKey,brands:[privateCopy,publicCopy].every(key=>key instanceof CryptoKey&&key.constructor===CryptoKey),metadata:[privateCopy.type,privateCopy.extractable,privateCopy.algorithm.name,privateCopy.usages,publicCopy.type,publicCopy.algorithm.name],secret:actual.length===32&&actual.length===expected.length&&actual.every((byte,index)=>byte===expected[index]),exportName,hidden:privateCopy.constructor.constructor('return typeof Deno')()==='undefined'};
    "#;
    let source = format!("(async()=>{{const fixture={fixture};{script}}})()");
    let mut page = PageSession::open("<body>", "https://hybrid-clone.test/", "", "", 5000)
        .await
        .unwrap();
    let result = page.eval_json_expression(&source).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&result).unwrap(),
        json!({"distinct":true,"brands":true,"metadata":["private",false,"MLKEM768-X25519",["decapsulateBits"],"public","MLKEM768-X25519"],"secret":true,"exportName":"InvalidAccessError","hidden":true})
    );
}
