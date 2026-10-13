//! Native WebCrypto behavior, using public test vectors and offline key roundtrips.
use serde_json::Value;

async fn evaluate(source: &str) -> Value {
    let source = format!(
        r#"(async()=>{{const hex=value=>Array.from(new Uint8Array(value),byte=>byte.toString(16).padStart(2,'0')).join('');const fromHex=value=>Uint8Array.from(value.match(/../g),pair=>parseInt(pair,16));const error=async callback=>{{try{{await callback();return null;}}catch(error){{return {{name:error.name,dom:error instanceof DOMException}};}}}};{source}}})()"#
    );
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
async fn native_digests_preserve_buffer_ranges_and_known_answers() {
    let value = evaluate(r#"
      const data=new Uint8Array([0,97,98,99,0]),digests={};
      for(const name of ['SHA-1','SHA-256','SHA-384','SHA-512'])digests[name]=hex(await crypto.subtle.digest(name,new DataView(data.buffer,1,3)));
      globalThis.__RESULT=JSON.stringify({digests,unsupported:await error(()=>crypto.subtle.digest('unknown',data)),missing:await error(()=>crypto.subtle.digest())});
    "#).await;
    assert_eq!(
        value["digests"]["SHA-1"],
        "a9993e364706816aba3e25717850c26c9cd0d89d"
    );
    assert_eq!(
        value["digests"]["SHA-256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(value["digests"]["SHA-384"], "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7");
    assert_eq!(value["digests"]["SHA-512"], "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f");
    assert_eq!(value["unsupported"]["name"], "NotSupportedError");
    assert_eq!(value["unsupported"]["dom"], true);
    assert_eq!(value["missing"]["name"], "TypeError");
}

#[tokio::test(flavor = "current_thread")]
async fn aes_gcm_import_encrypt_decrypt_and_extractability_are_real() {
    let value = evaluate(r#"
      const raw=new Uint8Array(16),iv=new Uint8Array(12),data=new Uint8Array(16);
      const imported=crypto.subtle.importKey('raw',raw,'AES-GCM',true,['encrypt','decrypt']);const key=await imported;
      const ciphertext=await crypto.subtle.encrypt({name:'AES-GCM',iv},key,data);
      const recovered=await crypto.subtle.decrypt({name:'AES-GCM',iv},key,ciphertext);
      const privateKey=await crypto.subtle.importKey('raw',raw,'AES-GCM',false,['encrypt']);
      const changed=new Uint8Array(ciphertext.slice(0));changed[0]^=1;
      globalThis.__RESULT=JSON.stringify({importPromise:imported instanceof Promise,ciphertext:hex(ciphertext),recovered:hex(recovered),exported:hex(await crypto.subtle.exportKey('raw',key)),key:{type:key.type,extractable:key.extractable,algorithm:key.algorithm,usages:key.usages},privateExport:await error(()=>crypto.subtle.exportKey('raw',privateKey)),changed:await error(()=>crypto.subtle.decrypt({name:'AES-GCM',iv},key,changed))});
    "#).await;
    assert_eq!(value["importPromise"], true);
    assert_eq!(
        value["ciphertext"],
        "0388dace60b6a392f328c2b971b2fe78ab6e47d42cec13bdf53a67b21257bddf"
    );
    assert_eq!(value["recovered"], "00000000000000000000000000000000");
    assert_eq!(value["exported"], "00000000000000000000000000000000");
    assert_eq!(value["key"]["type"], "secret");
    assert_eq!(value["key"]["algorithm"]["length"], 128);
    assert_eq!(value["privateExport"]["name"], "InvalidAccessError");
    assert_eq!(value["changed"]["name"], "OperationError");
}

#[tokio::test(flavor = "current_thread")]
async fn hmac_and_ed25519_verify_public_vectors_and_reject_modified_messages() {
    let value = evaluate(r#"
      const hmac=await crypto.subtle.importKey('raw',new Uint8Array(20).fill(0x0b),{name:'HMAC',hash:'SHA-256'},false,['sign','verify']);
      const data=new TextEncoder().encode('Hi There'),signature=await crypto.subtle.sign('HMAC',hmac,data);
      const publicBytes=fromHex('d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a');
      const edSignature=fromHex('e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b');
      const publicKey=await crypto.subtle.importKey('raw',publicBytes,'Ed25519',true,['verify']);
      globalThis.__RESULT=JSON.stringify({hmac:hex(signature),hmacVerify:await crypto.subtle.verify('HMAC',hmac,signature,data),edVerify:await crypto.subtle.verify('Ed25519',publicKey,edSignature,new Uint8Array()),modified:await crypto.subtle.verify('Ed25519',publicKey,edSignature,new Uint8Array([1]))});
    "#).await;
    assert_eq!(
        value["hmac"],
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
    assert_eq!(value["hmacVerify"], true);
    assert_eq!(value["edVerify"], true);
    assert_eq!(value["modified"], false);
}

#[tokio::test(flavor = "current_thread")]
async fn generated_private_keys_derive_real_public_keys_without_export_permission() {
    let value = evaluate(r#"
      const pair=await crypto.subtle.generateKey('Ed25519',false,['sign','verify']);
      const derived=crypto.subtle.getPublicKey(pair.privateKey,['verify']),publicKey=await derived;
      const data=new TextEncoder().encode('native key derivation'),signature=await crypto.subtle.sign('Ed25519',pair.privateKey,data);
      globalThis.__RESULT=JSON.stringify({promise:derived instanceof Promise,same:hex(await crypto.subtle.exportKey('raw',publicKey))===hex(await crypto.subtle.exportKey('raw',pair.publicKey)),verify:await crypto.subtle.verify('Ed25519',publicKey,signature,data),privateExport:await error(()=>crypto.subtle.exportKey('pkcs8',pair.privateKey)),publicArgument:await error(()=>crypto.subtle.getPublicKey(pair.publicKey,['verify'])),key:{type:publicKey.type,extractable:publicKey.extractable,algorithm:publicKey.algorithm,usages:publicKey.usages},own:Reflect.ownKeys(pair.privateKey).map(String)});
    "#).await;
    for name in ["promise", "same", "verify"] {
        assert_eq!(value[name], true, "{value}");
    }
    assert_eq!(value["privateExport"]["name"], "InvalidAccessError");
    assert_eq!(value["publicArgument"]["name"], "InvalidAccessError");
    assert_eq!(value["key"]["type"], "public");
    assert_eq!(value["key"]["extractable"], true);
    assert_eq!(value["own"], serde_json::json!([]));
}

#[tokio::test(flavor = "current_thread")]
async fn ml_kem_bits_and_aes_keys_encapsulate_and_decapsulate_real_shared_material() {
    let value = evaluate(r#"
      const cases=[];
      for(const name of ['ML-KEM-768','ML-KEM-1024']){
        const pair=await crypto.subtle.generateKey(name,false,['encapsulateBits','decapsulateBits','encapsulateKey','decapsulateKey']);
        const bits=await crypto.subtle.encapsulateBits(name,pair.publicKey);
        const recovered=await crypto.subtle.decapsulateBits(name,pair.privateKey,bits.ciphertext);
        const keyCall=crypto.subtle.encapsulateKey(name,pair.publicKey,{name:'AES-GCM',length:256},true,['encrypt','decrypt']),wrapped=await keyCall;
        const recoveredCall=crypto.subtle.decapsulateKey(name,pair.privateKey,wrapped.ciphertext,{name:'AES-GCM',length:256},true,['encrypt','decrypt']),key=await recoveredCall;
        cases.push({name,bytes:bits.ciphertext.byteLength,secretBytes:bits.sharedKey.byteLength,bits:hex(bits.sharedKey)===hex(recovered),key:hex(await crypto.subtle.exportKey('raw',wrapped.sharedKey))===hex(await crypto.subtle.exportKey('raw',key)),keyPromise:keyCall instanceof Promise,decapsulatePromise:recoveredCall instanceof Promise,algorithm:key.algorithm,type:key.type,privateExtractable:pair.privateKey.extractable});
      }globalThis.__RESULT=JSON.stringify(cases);
    "#).await;
    for (index, row) in value.as_array().unwrap().iter().enumerate() {
        assert_eq!(row["bytes"], [1088, 1568][index]);
        assert_eq!(row["secretBytes"], 32);
        for name in ["bits", "key", "keyPromise", "decapsulatePromise"] {
            assert_eq!(row[name], true, "{row}");
        }
        assert_eq!(row["algorithm"]["name"], "AES-GCM");
        assert_eq!(row["algorithm"]["length"], 256);
        assert_eq!(row["type"], "secret");
        assert_eq!(row["privateExtractable"], false);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn native_support_queries_and_key_brands_reject_forged_inputs() {
    let value = evaluate(r#"
      const rows={};for(const [operation,algorithm] of [['digest','SHA-256'],['generateKey','Ed25519'],['encapsulateBits','ML-KEM-768'],['digest','AES-GCM'],['digest','unknown']])rows[operation+':'+algorithm]=SubtleCrypto.supports(operation,algorithm);
      const key=await crypto.subtle.generateKey({name:'AES-GCM',length:128},false,['encrypt']);
      const forged=Object.create(CryptoKey.prototype);const errors=[];
      for(const call of [()=>crypto.subtle.getPublicKey(forged,[]),()=>SubtleCrypto.prototype.getPublicKey.call({},key,[]),()=>crypto.subtle.importKey(),()=>crypto.subtle.encapsulateBits(),()=>crypto.subtle.decapsulateKey()])errors.push((await error(call))?.name);
      let constructor=false,constructorError=null;try{new CryptoKey()}catch(error){constructor=error instanceof TypeError;constructorError={name:error.name,message:error.message,tag:Object.prototype.toString.call(error)};}
      globalThis.__RESULT=JSON.stringify({rows,errors,constructor,constructorError,hostTypes:[typeof Deno,typeof __bootstrap,typeof process,typeof require],host:typeof Deno==='undefined'&&typeof __bootstrap==='undefined'&&typeof process==='undefined'&&typeof require==='undefined',arity:[crypto.subtle.importKey.length,crypto.subtle.getPublicKey.length,crypto.subtle.encapsulateKey.length,crypto.subtle.decapsulateKey.length]});
    "#).await;
    assert_eq!(value["rows"]["digest:SHA-256"], true);
    assert_eq!(value["rows"]["generateKey:Ed25519"], true);
    assert_eq!(value["rows"]["encapsulateBits:ML-KEM-768"], true);
    assert_eq!(value["rows"]["digest:AES-GCM"], false);
    assert_eq!(value["rows"]["digest:unknown"], false);
    assert_eq!(
        value["errors"],
        serde_json::json!([
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError"
        ])
    );
    assert_eq!(value["constructor"], true, "{value}");
    assert_eq!(value["host"], true, "{value}");
    assert_eq!(value["arity"], serde_json::json!([5, 2, 5, 6]));
}

#[tokio::test(flavor = "current_thread")]
async fn secure_crypto_exposure_uses_native_origin_policy_and_cannot_be_spoofed_by_page_globals() {
    for (url, expected) in [
        ("https://example.test/", true),
        ("http://127.0.0.1/", true),
        ("http://[::1]/", true),
        ("http://localhost/", true),
        ("http://example.test/", false),
        ("http://localhost.example.test/", false),
        ("about:blank", false),
    ] {
        let mut page =
            turbo_surf_render::PageSession::open("<!doctype html><body></body>", url, "", "", 5000)
                .await
                .unwrap();
        let state: Value = serde_json::from_str(&page.eval("globalThis.__RESULT=JSON.stringify([isSecureContext,typeof crypto.subtle==='object',typeof SubtleCrypto==='function',typeof CryptoKey==='function',typeof crypto.randomUUID==='function']);").await.unwrap()).unwrap();
        assert_eq!(
            state,
            serde_json::json!([expected, expected, expected, expected, expected]),
            "{url}"
        );
        if !expected {
            assert_eq!(page.eval("globalThis.isSecureContext=true;globalThis.__RESULT=String(crypto.subtle===undefined)").await.unwrap(), "true");
        }
        page.close();
    }
}

#[tokio::test(flavor = "current_thread")]
async fn crypto_key_metadata_is_copied_without_mutating_native_permissions() {
    let value = evaluate(r#"
      const pair=await crypto.subtle.generateKey('Ed25519',false,['sign','verify']),key=pair.privateKey;
      const algorithm=key.algorithm,usages=key.usages;
      const shapes={algorithmCopy:algorithm!==key.algorithm,usagesCopy:usages!==key.usages,algorithmMutable:!Object.isFrozen(algorithm),usagesMutable:!Object.isFrozen(usages),objectRealm:Object.getPrototypeOf(algorithm)===Object.prototype,arrayRealm:Object.getPrototypeOf(usages)===Array.prototype};
      algorithm.name='AES-GCM';usages.push('decrypt');Object.defineProperty(key,'extractable',{value:true});
      const bytes=new Uint8Array([1]),signature=await crypto.subtle.sign('Ed25519',key,bytes);
      globalThis.__RESULT=JSON.stringify({shapes,nativeAlgorithm:key.algorithm.name,nativeUsages:key.usages,verify:await crypto.subtle.verify('Ed25519',pair.publicKey,signature,bytes),cannotExport:(await error(()=>crypto.subtle.exportKey('pkcs8',key)))?.name,privateFactories:['Crypto','SubtleCrypto','CryptoKey'].every(name=>!Object.hasOwn(globalThis[name],'create')&&!Object.hasOwn(globalThis[name],'registerSymbols')&&!Object.hasOwn(globalThis[name],'importSync'))});
    "#).await;
    for (name, flag) in value["shapes"].as_object().unwrap() {
        assert_eq!(*flag, true, "{name}: {value}");
    }
    assert_eq!(value["nativeAlgorithm"], "Ed25519");
    assert_eq!(value["nativeUsages"], serde_json::json!(["sign"]));
    assert_eq!(value["verify"], true);
    assert_eq!(value["cannotExport"], "InvalidAccessError");
    assert_eq!(value["privateFactories"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn iframe_crypto_has_independent_prototypes_and_does_not_rebrand_foreign_inputs() {
    let value = evaluate(r#"
      const parentPair=await crypto.subtle.generateKey('Ed25519',false,['sign','verify']);
      const parentPrototype=Object.getPrototypeOf(parentPair.privateKey);
      const frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;
      const childPair=await child.crypto.subtle.generateKey('Ed25519',false,['sign','verify']);
      const message=new Uint8Array([4]),signature=await child.crypto.subtle.sign('Ed25519',parentPair.privateKey,message);
      const childOwn=Object.getPrototypeOf(childPair.privateKey)===child.CryptoKey.prototype&&childPair.privateKey.constructor===child.CryptoKey;
      const metadata=childPair.privateKey.algorithm,usages=childPair.privateKey.usages;
      const errors={};for(const [name,call] of Object.entries({brand:()=>child.SubtleCrypto.prototype.getPublicKey.call({},parentPair.privateKey,[]),permission:()=>child.crypto.subtle.exportKey('pkcs8',childPair.privateKey)}))try{await call();errors[name]=false;}catch(error){errors[name]=name==='brand'?error instanceof child.TypeError:error instanceof child.DOMException;}
      globalThis.__RESULT=JSON.stringify({functionRealm:Object.getPrototypeOf(child.Crypto)===child.Function.prototype&&Object.getPrototypeOf(child.SubtleCrypto.prototype.digest)===child.Function.prototype,constructors:child.Crypto!==Crypto&&child.SubtleCrypto!==SubtleCrypto&&child.CryptoKey!==CryptoKey,parentStable:Object.getPrototypeOf(parentPair.privateKey)===parentPrototype&&parentPair.privateKey.constructor===CryptoKey,childOwn,cryptoOwner:Object.getPrototypeOf(child.crypto)===child.Crypto.prototype&&Object.getPrototypeOf(crypto)===Crypto.prototype,metadataOwner:Object.getPrototypeOf(metadata)===child.Object.prototype&&Object.getPrototypeOf(usages)===child.Array.prototype,foreignVerify:await crypto.subtle.verify('Ed25519',parentPair.publicKey,signature,message),foreignUnchanged:Object.getPrototypeOf(parentPair.privateKey)===parentPrototype,errors});frame.remove();
    "#).await;
    for name in [
        "functionRealm",
        "constructors",
        "parentStable",
        "childOwn",
        "cryptoOwner",
        "metadataOwner",
        "foreignVerify",
        "foreignUnchanged",
    ] {
        assert_eq!(value[name], true, "{name}: {value}");
    }
    assert_eq!(value["errors"]["brand"], true);
    assert_eq!(value["errors"]["permission"], true);
}

#[tokio::test(flavor = "current_thread")]
async fn borrowed_crypto_methods_return_receiver_realm_objects_and_reject_false_windows() {
    let value = evaluate(r#"
      const frame=document.createElement('iframe');document.body.append(frame);const child=frame.contentWindow;
      const parentKey=await crypto.subtle.generateKey('Ed25519',false,['sign','verify']);
      const call=crypto.subtle.getPublicKey.call(child.crypto.subtle,parentKey.privateKey,['verify']),key=await call;
      const digest=await child.crypto.subtle.digest('SHA-256',new Uint8Array());
      const kem=await child.crypto.subtle.generateKey('ML-KEM-768',false,['encapsulateBits','decapsulateBits']);
      const bits=await child.crypto.subtle.encapsulateBits('ML-KEM-768',kem.publicKey);
      const getter=Object.getOwnPropertyDescriptor(globalThis,'crypto').get;let getterBrand=false,supportBrand=false;
      try{getter.call({})}catch(error){getterBrand=error instanceof TypeError;}
      try{child.SubtleCrypto.supports()}catch(error){supportBrand=error instanceof child.TypeError;}
      const jwk=await child.crypto.subtle.exportKey('jwk',key);
      globalThis.__RESULT=JSON.stringify({promise:call instanceof child.Promise&&!(call instanceof Promise),key:Object.getPrototypeOf(key)===child.CryptoKey.prototype,digest:Object.getPrototypeOf(digest)===child.ArrayBuffer.prototype,bits:Object.getPrototypeOf(bits)===child.Object.prototype&&Object.getPrototypeOf(bits.ciphertext)===child.ArrayBuffer.prototype&&Object.getPrototypeOf(bits.sharedKey)===child.ArrayBuffer.prototype,jwk:Object.getPrototypeOf(jwk)===child.Object.prototype&&Object.getPrototypeOf(jwk.key_ops)===child.Array.prototype,getterBrand,supportBrand,nullWindow:getter.call(null)===crypto,foreignKey:Object.getPrototypeOf(parentKey.privateKey)===CryptoKey.prototype});frame.remove();
    "#).await;
    for (name, flag) in value.as_object().unwrap() {
        assert_eq!(*flag, true, "{name}: {value}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn reusable_crypto_collector_reports_real_algorithms_and_roundtrips() {
    let probe = include_str!("../../../../harness/detection/webcrypto-probe.js");
    let source = format!("globalThis.__RESULT=await {}", probe);
    let value = evaluate(&source).await;
    if let Ok(path) = std::env::var("TURBO_CRYPTO_SNAPSHOT") {
        std::fs::write(path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    }
    assert_eq!(value["supports"]["digest"]["SHA-256"]["value"], true);
    assert_eq!(value["ed"]["verify"], true);
    assert_eq!(value["kem"]["ML-KEM-512"]["error"], "NotSupportedError");
    for name in ["ML-KEM-768", "ML-KEM-1024"] {
        assert_eq!(value["kem"][name]["bitsMatch"], true, "{name}: {value}");
        assert_eq!(value["kem"][name]["keyMatch"], true, "{name}: {value}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn crypto_profile_normalizes_required_dictionaries_without_enabling_backend_only_algorithms()
{
    let value = evaluate(r#"
      let reads=0;const algorithm={get name(){reads++;return 'AES-GCM';},length:128};
      const key=await crypto.subtle.generateKey(algorithm,false,['decrypt','encrypt']);
      const support={valid:SubtleCrypto.supports('generateKey',{name:'AES-GCM',length:128}),missing:SubtleCrypto.supports('generateKey','AES-GCM'),invalid:SubtleCrypto.supports('generateKey',{name:'AES-GCM',length:7}),bigint:SubtleCrypto.supports('generateKey',{name:'AES-GCM',length:128n}),hash:SubtleCrypto.supports('generateKey',{name:'HMAC',hash:'SHA-256'}),noHash:SubtleCrypto.supports('generateKey','HMAC'),backendOnly:SubtleCrypto.supports('digest','SHA3-256'),kem512:SubtleCrypto.supports('generateKey','ML-KEM-512')};
      Object.prototype.generateKey='';let rejected;try{await crypto.subtle.generateKey('SHA-256',false,[])}catch(e){rejected=e.name;}delete Object.prototype.generateKey;
      globalThis.__RESULT=JSON.stringify({support,reads,usages:key.usages,rejected,unsupported:(await error(()=>crypto.subtle.generateKey('ML-KEM-512',false,['encapsulateBits','decapsulateBits'])))?.name,nested:(await error(()=>crypto.subtle.generateKey({name:'HMAC',hash:'SHA3-256'},false,['sign'])))?.name});
    "#).await;
    assert_eq!(value["reads"], 1);
    assert_eq!(value["usages"], serde_json::json!(["encrypt", "decrypt"]));
    assert_eq!(value["rejected"], "NotSupportedError");
    assert_eq!(value["unsupported"], "NotSupportedError");
    assert_eq!(value["nested"], "NotSupportedError");
    assert_eq!(
        value["support"],
        serde_json::json!({"valid":true,"missing":false,"invalid":false,"bigint":false,"hash":true,"noHash":false,"backendOnly":false,"kem512":false})
    );
}
