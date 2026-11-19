//! Public asynchronous ownership and cancellation contracts over actual realms and wire I/O.
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn cross_realm_writable_abort_waits_for_inflight_write_and_rejects_queued_work() {
    let mut page = PageSession::open("<body>", "https://async-stream.test/", "", "", 5000)
        .await
        .unwrap();
    let value=page.eval_json_expression(r#"(async()=>{
      const frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;
      let started;const start=new Promise(resolve=>started=resolve);
      const reason={owner:'root-abort'},effects={events:[],started,reason,child};child.__effects=effects;
      const stream=new child.WritableStream({
        write:new child.Function('value',`const e=__effects;e.events.push('write:'+value);return new Promise(resolve=>{e.release=()=>{e.events.push('finish:'+value);resolve()};e.started()})`),
        abort:new child.Function('reason',`const e=__effects;e.events.push('abort');e.reasonSame=reason===e.reason;e.realm=globalThis===e.child`)
      });
      const writer=stream.getWriter(),first=writer.write('first');await start;
      const second=writer.write('second').then(()=>false,error=>error===reason);
      const closed=writer.closed.then(()=>false,error=>error===reason);
      let aborted=false;
      const abort=WritableStreamDefaultWriter.prototype.abort.call(writer,reason).then(()=>{aborted=true});
      await Promise.resolve();const waited=!aborted&&effects.events.join(',')==='write:first';
      effects.release();await first;await abort;const rejected=await second,closedSame=await closed;
      writer.releaseLock();frame.remove();
      return {waited,rejected,closedSame,events:effects.events,sourceReason:effects.reasonSame,sourceRealm:effects.realm,unlocked:!stream.locked};
    })()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&value).unwrap(),
        json!({"waited":true,"rejected":true,"closedSame":true,"events":["write:first","finish:first","abort"],"sourceReason":true,"sourceRealm":true,"unlocked":true})
    );
}

async fn request_path(socket: &mut tokio::net::TcpStream) -> String {
    let mut request = Vec::new();
    while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
        let mut chunk = [0; 1024];
        let count = socket.read(&mut chunk).await.unwrap();
        assert!(count > 0, "Request closed before headers");
        request.extend_from_slice(&chunk[..count]);
        assert!(request.len() < 16 * 1024, "Fixture request header bound");
    }
    String::from_utf8(request)
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .into()
}

#[tokio::test(flavor = "current_thread")]
async fn transferred_fetch_body_abort_clones_reason_and_closes_only_its_transport() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut held, _) = listener.accept().await.unwrap();
        assert_eq!(request_path(&mut held).await, "/hold");
        held.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n\x00\xffA").await.unwrap();
        let mut byte = [0; 1];
        let closed = tokio::time::timeout(Duration::from_secs(5), held.read(&mut byte))
            .await
            .expect("Aborted owner must close the held native transport");
        match closed {
            Ok(0) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::BrokenPipe
                ) => {}
            other => panic!("Held transport did not close: {other:?}"),
        }
        let (mut healthy, _) = listener.accept().await.unwrap();
        assert_eq!(request_path(&mut healthy).await, "/healthy");
        healthy
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nhealthy")
            .await
            .unwrap();
    });
    let mut page = PageSession::open("<body>", &base, "", "", 5000)
        .await
        .unwrap();
    let value=page.eval_json_expression(r#"(async()=>{
      const frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;
      const controller=new child.AbortController(),reason={kind:'stop-transferred-body'};
      const response=await child.fetch(location.origin+'/hold',{signal:controller.signal});
      const body=structuredClone(response.body,{transfer:[response.body]}),reader=body.getReader(),prefix=[];
      let brands=true;while(prefix.length<3){const part=await reader.read();if(part.done)throw new Error('Body ended before prefix');brands=brands&&part.value instanceof Uint8Array;prefix.push(...part.value);}
      const pending=reader.read();controller.abort(reason);let rejected=false,cloned=false,ownRealm=false;
      try{await pending;}catch(error){rejected=error.kind===reason.kind;cloned=error!==reason;ownRealm=Object.getPrototypeOf(error)===Object.prototype;}
      const originalReason=controller.signal.reason===reason;reader.releaseLock();frame.remove();
      const healthy=await (await fetch('/healthy')).text();
      return {prefix,brands,rejected,cloned,ownRealm,originalReason,healthy};
    })()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&value).unwrap(),
        json!({"prefix":[0,255,65],"brands":true,"rejected":true,"cloned":true,"ownRealm":true,"originalReason":true,"healthy":"healthy"})
    );
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn nested_crypto_in_an_algorithm_getter_keeps_independent_owners_and_authored_error() {
    let mut page = PageSession::open("<body>", "https://async-crypto.test/", "", "", 5000)
        .await
        .unwrap();
    let value=page.eval_json_expression(r#"(async()=>{
      const frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;
      const sentinel=new Error('authored normalization failure');let reads=0,sync=false,call,nested;
      try{call=child.crypto.subtle.digest({get name(){reads++;nested=crypto.subtle.digest('SHA-256',new Uint8Array([97,98,99]));throw sentinel}},new Uint8Array([1]));}catch(error){sync=true;}
      let same=false;try{await call;}catch(error){same=error===sentinel;}
      const hex=value=>Array.from(new Uint8Array(value),byte=>byte.toString(16).padStart(2,'0')).join('');
      const nestedRoot=nested instanceof Promise&&!(nested instanceof child.Promise),nestedHex=hex(await nested);
      const childHex=hex(await child.crypto.subtle.digest('SHA-256',new Uint8Array()));
      const childPromise=call instanceof child.Promise;frame.remove();
      return {reads,sync,promise:childPromise,same,nestedRoot,nestedHex,childHex};
    })()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&value).unwrap(),
        json!({"reads":1,"sync":false,"promise":true,"same":true,"nestedRoot":true,"nestedHex":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad","childHex":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn cross_realm_pipe_abort_with_prevention_keeps_both_streams_usable() {
    let mut page = PageSession::open("<body>", "https://async-pipe.test/", "", "", 5000)
        .await
        .unwrap();
    let result = page.eval_json_expression(r#"(async()=>{
      const frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;
      let controller;const events=[];
      const source=new child.ReadableStream({start(value){controller=value;},cancel(){events.push('cancel');}});
      const sink=new child.WritableStream({write(value){events.push('write:'+value);},abort(){events.push('abort');},close(){events.push('close');}});
      const aborter=new AbortController(),reason={owner:'root-pipe-abort'};
      const pending=ReadableStream.prototype.pipeTo.call(source,sink,{signal:aborter.signal,preventAbort:true,preventCancel:true});
      const brand=pending instanceof child.Promise&&!(pending instanceof Promise),rootBrand=pending instanceof Promise&&!(pending instanceof child.Promise),locked=source.locked&&sink.locked;
      aborter.abort(reason);let same=false;try{await pending;}catch(error){same=error===reason;}
      const unlocked=!source.locked&&!sink.locked,reader=source.getReader();controller.enqueue(23);
      const item=await reader.read();reader.releaseLock();
      const writer=sink.getWriter();await writer.write(29);await writer.close();writer.releaseLock();
      const result={brand,rootBrand,locked,same,unlocked,item:[item.value,item.done],events,finalUnlocked:!source.locked&&!sink.locked};frame.remove();return result;
    })()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&result).unwrap(),
        json!({"brand":true,"rootBrand":false,"locked":true,"same":true,"unlocked":true,"item":[23,false],"events":["write:29","close"],"finalUnlocked":true})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn stream_iterator_early_return_preserves_prevent_cancel_and_reason_identity() {
    let mut page = PageSession::open("<body>", "https://async-iterator.test/", "", "", 5000)
        .await
        .unwrap();
    let result = page.eval_json_expression(r#"(async()=>{
      const frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;
      const reason={owner:'root-iterator-return'},rows=[];
      for(const preventCancel of [true,false]){
        let controller,calls=0,same=false;
        const source=new child.ReadableStream({start(value){controller=value;value.enqueue(11);},cancel(value){calls++;same=value===reason;}});
        const iterator=ReadableStream.prototype.values.call(source,{preventCancel});
        const pending=iterator.next(),brand=pending instanceof child.Promise&&!(pending instanceof Promise),rootBrand=pending instanceof Promise&&!(pending instanceof child.Promise);
        const first=await pending,returned=await iterator.return(reason),unlocked=!source.locked;
        let reusable=false;
        if(preventCancel){controller.enqueue(17);const reader=source.getReader();const next=await reader.read();reusable=next.value===17&&!next.done;reader.releaseLock();}
        rows.push({preventCancel,brand,rootBrand,first:[first.value,first.done],returned:returned.done&&returned.value===reason,calls,same,unlocked,reusable});
      }
      frame.remove();return rows;
    })()"#).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&result).unwrap(),
        json!([
          {"preventCancel":true,"brand":true,"rootBrand":false,"first":[11,false],"returned":true,"calls":0,"same":false,"unlocked":true,"reusable":true},
          {"preventCancel":false,"brand":true,"rootBrand":false,"first":[11,false],"returned":true,"calls":1,"same":true,"unlocked":true,"reusable":false}
        ])
    );
}
