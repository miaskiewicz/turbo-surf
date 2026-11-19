//! SOURCE-ONLY candidate. Public PageSession and real backend operations only.
use serde_json::{json, Value};
use turbo_surf_render::PageSession;

async fn evaluate(source: &str) -> Value {
    let mut page = PageSession::open(
        "<!doctype html><body>owner error controls</body>",
        "https://guest.test/owner-errors",
        "",
        "",
        5000,
    )
    .await
    .unwrap();
    let result = page.eval_json_expression(source).await.unwrap();
    page.close();
    serde_json::from_str(&result).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn borrowed_crypto_preserves_owning_backend_errors_and_authored_error_identity() {
    let result = evaluate(r#"(async () => {
      const a=document.createElement('iframe'), b=document.createElement('iframe');
      document.body.append(a,b);
      const A=a.contentWindow, B=b.contentWindow;
      const privateRealmHidden = (error, realm) =>
        error.constructor.constructor('return globalThis')() === realm &&
        error.constructor.constructor('return typeof Deno')() === 'undefined';
      const key=await A.crypto.subtle.generateKey({name:'AES-GCM',length:128},false,['encrypt']);
      let native=false, nativeName=false, nativePrivate=false, nativeActualName=null;
      try {
        await B.SubtleCrypto.prototype.encrypt.call(A.crypto.subtle,
          {name:'AES-GCM',iv:new A.Uint8Array(0)},key,new A.Uint8Array(1));
      } catch(error) {
        native=error instanceof A.DOMException && !(error instanceof B.DOMException);
        nativeName=error.name==='OperationError';nativeActualName=error.name;
        nativePrivate=privateRealmHidden(error,A);
      }
      let arity=false, arityPrivate=false;
      try { await B.SubtleCrypto.prototype.digest.call(A.crypto.subtle); }
      catch(error) {
        arity=error instanceof B.TypeError && !(error instanceof A.TypeError);
        arityPrivate=privateRealmHidden(error,B);
      }
      const sentinel=new B.TypeError('authored algorithm getter');
      let authored=false, getterCalls=0;
      const algorithm={get name(){getterCalls++;throw sentinel;}};
      try { await B.SubtleCrypto.prototype.digest.call(A.crypto.subtle,algorithm,new A.Uint8Array()); }
      catch(error) { authored=error===sentinel; }
      const values={native,nativeName,nativeActualName,nativePrivate,arity,arityPrivate,authored,getterCalls};
      a.remove();b.remove();return values;
    })()"#).await;
    assert_eq!(
        result,
        json!({
            "native": true, "nativeName": true, "nativeActualName": "OperationError", "nativePrivate": true,
            "arity": true, "arityPrivate": true, "authored": true, "getterCalls": 1
        })
    );
}

#[tokio::test(flavor = "current_thread")]
async fn borrowed_stream_rejection_keeps_receiver_realm_and_authored_foreign_values() {
    let result = evaluate(r#"(async () => {
      const a=document.createElement('iframe'), b=document.createElement('iframe');
      document.body.append(a,b);
      const A=a.contentWindow, B=b.contentWindow;
      const stream=new A.ReadableStream(), reader=stream.getReader();
      reader.releaseLock();
      const pending=B.ReadableStreamDefaultReader.prototype.read.call(reader);
      let native=false, nativePrivate=false;
      try { await pending; } catch(error) {
        native=error instanceof A.TypeError && !(error instanceof B.TypeError);
        nativePrivate=error.constructor.constructor('return globalThis')()===A &&
          error.constructor.constructor('return typeof Deno')()==='undefined';
      }
      const getterSentinel=new B.TypeError('authored source getter');
      let getterIdentity=false, getterCalls=0;
      try { new A.ReadableStream({get pull(){getterCalls++;throw getterSentinel;}}); }
      catch(error) { getterIdentity=error===getterSentinel; }
      const sourceSentinel=new B.Error('authored source rejection');
      const failed=new A.ReadableStream({start(controller){controller.error(sourceSentinel);}});
      let sourceIdentity=false;
      try { await failed.getReader().read(); } catch(error) { sourceIdentity=error===sourceSentinel; }
      const values={promise:pending instanceof A.Promise && !(pending instanceof B.Promise),
        native,nativePrivate,getterIdentity,getterCalls,sourceIdentity};
      a.remove();b.remove();return values;
    })()"#).await;
    assert_eq!(
        result,
        json!({
            "promise": true, "native": true, "nativePrivate": true,
            "getterIdentity": true, "getterCalls": 1, "sourceIdentity": true
        })
    );
}

#[tokio::test(flavor = "current_thread")]
async fn borrowed_decrypt_zero_iv_is_a_native_owning_realm_operation_error() {
    let result = evaluate(r#"(async()=>{
      const a=document.createElement('iframe'),b=document.createElement('iframe');
      document.body.append(a,b);const A=a.contentWindow,B=b.contentWindow;
      const key=await A.crypto.subtle.generateKey({name:'AES-GCM',length:128},false,['encrypt','decrypt']);
      let value;
      try { await B.SubtleCrypto.prototype.decrypt.call(A.crypto.subtle,
        {name:'AES-GCM',iv:new A.Uint8Array(0)},key,new A.Uint8Array(16));value={resolved:true}; }
      catch(error) { value={name:error.name,own:error instanceof A.DOMException && !(error instanceof B.DOMException),
        private:error.constructor.constructor('return globalThis')()===A && error.constructor.constructor('return typeof Deno')()==='undefined'}; }
      a.remove();b.remove();return value;
    })()"#).await;
    assert_eq!(
        result,
        json!({"name":"OperationError","own":true,"private":true})
    );
}
