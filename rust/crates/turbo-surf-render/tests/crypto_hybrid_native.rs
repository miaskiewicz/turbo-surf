//! Isolated cppgc runtime contracts; never share canonical standalone V8 init.
#[path = "../src/crypto_hybrid.rs"]
mod crypto_hybrid;
use deno_core::v8;
use serde_json::json;

#[test]
fn native_clone_keeps_private_material_and_permissions_without_extracting_bytes() {
    let mut runtime = deno_core::JsRuntime::new(deno_core::RuntimeOptions {
        extensions: vec![crypto_hybrid::extension()],
        ..Default::default()
    });
    let value = runtime.execute_script("authored-clone", "Deno.core.ops.NativeHybridKey.generate(false,['encapsulateBits','decapsulateBits']).privateKey").unwrap();
    deno_core::scope!(scope, runtime);
    let value = v8::Local::new(scope, value);
    assert!(crypto_hybrid::is_hybrid_key(scope, value));
    let snapshot = crypto_hybrid::HybridKeySnapshot::capture(scope, value).unwrap();
    assert_eq!(snapshot.size_bytes(), 1216 + 32);
    let cloned = snapshot.restore(scope);
    assert!(crypto_hybrid::is_hybrid_key(scope, cloned.into()));
    assert_ne!(cloned, value.to_object(scope).unwrap());
    let name = v8::String::new(scope, "extractable").unwrap();
    assert!(!cloned.get(scope, name.into()).unwrap().boolean_value(scope));
    let name = v8::String::new(scope, "type").unwrap();
    assert_eq!(
        cloned
            .get(scope, name.into())
            .unwrap()
            .to_rust_string_lossy(scope),
        "private"
    );
}

#[test]
fn cppgc_factories_preserve_native_brands_and_permissions() {
    let mut runtime = deno_core::JsRuntime::new(deno_core::RuntimeOptions {
        extensions: vec![crypto_hybrid::extension()],
        ..Default::default()
    });
    let result=runtime.execute_script("<hybrid-native-contract>",r#"(()=>{
      const core=Deno.core,Native=core.ops.NativeHybridKey;
      for(const name of ['DataError','InvalidAccessError','NotSupportedError','OperationError','SyntaxError'])core.registerErrorBuilder('DOMException'+name,message=>{const error=new Error(message);error.name=name;return error;});
      const pair=Native.generate(false,['encapsulateBits','decapsulateBits']);
      const bits=Native.encapsulateBits(pair.publicKey,false),recovered=Native.decapsulateBits(pair.privateKey,new Uint8Array(bits.ciphertext),false);
      const equal=(a,b)=>Array.from(new Uint8Array(a)).every((value,index)=>value===new Uint8Array(b)[index]);
      const failures=[];for(const callback of [()=>Native.exportBytes('raw-seed',pair.privateKey),()=>Native.encapsulateBits(pair.publicKey,true),()=>Native.decapsulateBits(pair.publicKey,new Uint8Array(bits.ciphertext),false),()=>Native.getPublicKey({},[])])try{callback();failures.push(null)}catch(error){failures.push(error.name)}
      const exported=Native.exportJwk(pair.publicKey),imported=Native.importJwk(exported,true,['encapsulateBits']);
      return JSON.stringify({roundtrip:equal(bits.sharedKey,recovered),brand:Native.isKey(pair.privateKey),forged:Native.isKey(Object.create(Native.prototype)),own:Reflect.ownKeys(pair.privateKey),privateType:pair.privateKey.type,privateExtractable:pair.privateKey.extractable,importType:imported.type,publicBytes:Native.exportBytes('raw-public',imported).byteLength,failures});
    })()"#).unwrap();
    deno_core::scope!(scope, runtime);
    let result = v8::Local::new(scope, result).to_rust_string_lossy(scope);
    let result: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        result,
        json!({"roundtrip":true,"brand":true,"forged":false,"own":[],"privateType":"private","privateExtractable":false,"importType":"public","publicBytes":1216,"failures":["InvalidAccessError","InvalidAccessError","InvalidAccessError","TypeError"]})
    );
}
