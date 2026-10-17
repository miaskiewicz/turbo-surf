//! Public X-Wing contracts measured against Chrome 155 on authored local fixtures.
use serde_json::{json, Value};

async fn evaluate(source: &str) -> Value {
    let fixture =
        include_str!("../../../../harness/detection/fixtures/webcrypto-xwing-chrome155.json");
    let source = format!(
        r#"(async()=>{{
      const fixture={fixture},name='MLKEM768-X25519';
      const hex=value=>Array.from(new Uint8Array(value),byte=>byte.toString(16).padStart(2,'0')).join('');
      const fromHex=value=>Uint8Array.from(value.match(/../g),pair=>parseInt(pair,16));
      const digest=async value=>hex(await crypto.subtle.digest('SHA-256',value));
      const error=async callback=>{{try{{await callback();return null;}}catch(error){{return {{name:error.name,dom:error instanceof DOMException}};}}}};
      {source}
    }})()"#
    );
    let result = turbo_surf_render::eval_async(
        "<!doctype html><body></body>",
        "https://hybrid.test/",
        &source,
    )
    .await
    .unwrap();
    serde_json::from_str(&result).unwrap_or_else(|_| panic!("invalid hybrid result: {result}"))
}

#[tokio::test(flavor = "current_thread")]
async fn seeded_hybrid_keys_match_chrome_and_enforce_native_permissions() {
    let value=evaluate(r#"
      const seed=fromHex(fixture.seedHex),importCall=crypto.subtle.importKey('raw-seed',seed,name,false,['decapsulateKey','decapsulateBits']),privateKey=await importCall;
      const deriveCall=crypto.subtle.getPublicKey(privateKey,['encapsulateBits','encapsulateKey']),publicKey=await deriveCall;
      const exported=await crypto.subtle.exportKey('raw-public',publicKey);
      const signature=await error(()=>crypto.subtle.sign('Ed25519',privateKey,new Uint8Array()));
      Object.defineProperty(privateKey,'extractable',{value:true});
      globalThis.__RESULT=JSON.stringify({importPromise:importCall instanceof Promise,publicPromise:deriveCall instanceof Promise,hash:await digest(exported),bytes:exported.byteLength,algorithm:publicKey.algorithm,usages:publicKey.usages,privateUsages:privateKey.usages,privateExport:await error(()=>crypto.subtle.exportKey('raw-seed',privateKey)),wrongAlgorithm:signature,rawFormat:await error(()=>crypto.subtle.exportKey('raw',publicKey)),factoryHidden:typeof NativeHybridKey==='undefined'&&typeof Deno==='undefined'&&!Object.hasOwn(CryptoKey,'isKey'),brand:Object.getPrototypeOf(privateKey)===CryptoKey.prototype&&Object.prototype.toString.call(privateKey)==='[object CryptoKey]'});
    "#).await;
    for name in ["importPromise", "publicPromise", "factoryHidden", "brand"] {
        assert_eq!(value[name], true, "{value}");
    }
    assert_eq!(
        value["hash"],
        "c9a3565ffde4f72b51661be391ee13e46378d7f06dd5c8bf5af9d2cfb5b8336b"
    );
    assert_eq!(value["bytes"], 1216);
    assert_eq!(value["algorithm"], json!({"name":"MLKEM768-X25519"}));
    assert_eq!(
        value["usages"],
        json!(["encapsulateKey", "encapsulateBits"])
    );
    assert_eq!(
        value["privateUsages"],
        json!(["decapsulateKey", "decapsulateBits"])
    );
    for name in ["privateExport", "wrongAlgorithm"] {
        assert_eq!(value[name], json!({"name":"InvalidAccessError","dom":true}));
    }
    assert_eq!(value["rawFormat"]["name"], "NotSupportedError");
}

#[tokio::test(flavor = "current_thread")]
async fn hybrid_bits_and_keys_interoperate_with_chrome_ciphertext() {
    let value=evaluate(r#"
      const privateKey=await crypto.subtle.importKey('raw-seed',fromHex(fixture.seedHex),name,false,['decapsulateBits','decapsulateKey']);
      const publicKey=await crypto.subtle.getPublicKey(privateKey,['encapsulateBits','encapsulateKey']);
      const fromChrome=await crypto.subtle.decapsulateBits(name,privateKey,fromHex(fixture.ciphertextHex));
      const bitsCall=crypto.subtle.encapsulateBits(name,publicKey),bits=await bitsCall,secret=await crypto.subtle.decapsulateBits(name,privateKey,bits.ciphertext);
      const keyCall=crypto.subtle.encapsulateKey(name,publicKey,{name:'AES-GCM',length:256},true,['encrypt']),wrapped=await keyCall;
      const key=await crypto.subtle.decapsulateKey(name,privateKey,wrapped.ciphertext,{name:'AES-GCM',length:256},true,['encrypt']);
      const forged=Object.create(CryptoKey.prototype);
      globalThis.__RESULT=JSON.stringify({chromeHash:await digest(fromChrome),expected:fixture.sharedSha256,bitsPromise:bitsCall instanceof Promise,keyPromise:keyCall instanceof Promise,bits:hex(bits.sharedKey)===hex(secret),key:hex(await crypto.subtle.exportKey('raw',wrapped.sharedKey))===hex(await crypto.subtle.exportKey('raw',key)),cipherBytes:bits.ciphertext.byteLength,secretBytes:bits.sharedKey.byteLength,metadata:key.algorithm,forged:await error(()=>crypto.subtle.encapsulateBits(name,forged)),malformed:await error(()=>crypto.subtle.decapsulateBits(name,privateKey,new Uint8Array(1119)))});
    "#).await;
    assert_eq!(value["chromeHash"], value["expected"]);
    for name in ["bitsPromise", "keyPromise", "bits", "key"] {
        assert_eq!(value[name], true, "{value}");
    }
    assert_eq!(value["cipherBytes"], 1120);
    assert_eq!(value["secretBytes"], 32);
    assert_eq!(value["metadata"], json!({"name":"AES-GCM","length":256}));
    assert_eq!(value["forged"]["name"], "TypeError");
    assert_eq!(value["malformed"]["name"], "OperationError");
}

#[tokio::test(flavor = "current_thread")]
async fn hybrid_wrapping_preserves_wrap_only_and_unwrap_only_key_permissions() {
    let value=evaluate(r#"
      const privateKey=await crypto.subtle.importKey('raw-seed',fromHex(fixture.seedHex),name,true,['decapsulateBits']);
      const bytes=new Uint8Array(32),wrapping=await crypto.subtle.importKey('raw',bytes,'AES-KW',false,['wrapKey']),unwrapping=await crypto.subtle.importKey('raw',bytes,'AES-KW',false,['unwrapKey']);
      const ciphertext=await crypto.subtle.wrapKey('raw-seed',privateKey,wrapping,'AES-KW');
      const restored=await crypto.subtle.unwrapKey('raw-seed',ciphertext,unwrapping,'AES-KW',name,false,['decapsulateBits']);
      const publicKey=await crypto.subtle.getPublicKey(restored,['encapsulateBits']);
      const gcmWrapping=await crypto.subtle.importKey('raw',bytes,'AES-GCM',false,['wrapKey']),gcmUnwrapping=await crypto.subtle.importKey('raw',bytes,'AES-GCM',false,['unwrapKey']);
      const algorithm={name:'AES-GCM',iv:new Uint8Array(12)};let hooks=0;Object.prototype.toJSON=function(){hooks++;throw new Error('guest JSON hook');};
      let restoredJwk;try{const wrapped=await crypto.subtle.wrapKey('jwk',privateKey,gcmWrapping,algorithm);restoredJwk=await crypto.subtle.unwrapKey('jwk',wrapped,gcmUnwrapping,algorithm,name,false,['decapsulateBits']);}finally{delete Object.prototype.toJSON;}
      const jwkPublic=await crypto.subtle.getPublicKey(restoredJwk,['encapsulateBits']);
      globalThis.__RESULT=JSON.stringify({cipherBytes:ciphertext.byteLength,hash:await digest(await crypto.subtle.exportKey('raw-public',publicKey)),jwkHash:await digest(await crypto.subtle.exportKey('raw-public',jwkPublic)),expected:fixture.publicSha256,hooks,privateExport:await error(()=>crypto.subtle.exportKey('raw-seed',restored)),wrongWrap:await error(()=>crypto.subtle.wrapKey('raw-seed',privateKey,unwrapping,'AES-KW'))});
    "#).await;
    assert_eq!(value["cipherBytes"], 40);
    assert_eq!(value["hooks"], 0);
    assert_eq!(value["hash"], value["expected"]);
    assert_eq!(value["jwkHash"], value["expected"]);
    for name in ["privateExport", "wrongWrap"] {
        assert_eq!(value[name]["name"], "InvalidAccessError", "{value}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn hybrid_foreign_keys_keep_their_realm_and_new_results_follow_the_receiver() {
    let value=evaluate(r#"
      const pair=await crypto.subtle.generateKey(name,false,['encapsulateBits','decapsulateBits']);
      const prototype=Object.getPrototypeOf(pair.privateKey),frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;
      const call=crypto.subtle.getPublicKey.call(child.crypto.subtle,pair.privateKey,['encapsulateBits']),publicKey=await call;
      const bits=await child.crypto.subtle.encapsulateBits(name,publicKey),secret=await crypto.subtle.decapsulateBits(name,pair.privateKey,bits.ciphertext);
      let metadataBrand=false;try{Object.getOwnPropertyDescriptor(child.CryptoKey.prototype,'algorithm').get.call({})}catch(error){metadataBrand=error instanceof child.TypeError;}let brand=false;try{await child.crypto.subtle.exportKey('raw-seed',pair.privateKey)}catch(error){brand=error instanceof child.DOMException&&error.name==='InvalidAccessError';}
      globalThis.__RESULT=JSON.stringify({promise:call instanceof child.Promise,key:Object.getPrototypeOf(publicKey)===child.CryptoKey.prototype,foreign:Object.getPrototypeOf(pair.privateKey)===prototype&&pair.privateKey.constructor===CryptoKey,buffers:Object.getPrototypeOf(bits.ciphertext)===child.ArrayBuffer.prototype&&Object.getPrototypeOf(bits.sharedKey)===child.ArrayBuffer.prototype,roundtrip:hex(secret)===hex(bits.sharedKey),brand,metadataBrand});frame.remove();
    "#).await;
    for (name, flag) in value.as_object().unwrap() {
        assert_eq!(*flag, true, "{name}: {value}");
    }
}
