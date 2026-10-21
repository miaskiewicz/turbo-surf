#[test]
fn coherent_runtime_crypto_and_type_graph() {
    let mut runtime: deno_core::JsRuntime = deno_bench_util::create_js_runtime(|| vec![
        deno_webidl::deno_webidl::init(),
        deno_web::deno_web::init(
            deno_web::BlobStore::default_arc(), None, false,
            deno_web::InMemoryBroadcastChannel::default(),
        ),
        deno_crypto::deno_crypto::init(None),
    ]);
    let answer = runtime.execute_script("arithmetic.js", "6 * 7").unwrap();
    {
        deno_core::scope!(scope, runtime);
        let local: v8::Local<v8::Value> = v8::Local::new(scope, answer);
        let value: i32 = serde_v8::from_v8(scope, local).unwrap();
        assert_eq!(value, 42);
    }
    let key = runtime.execute_script("key.js", r#"
        globalThis.setterCalls = 0;
        Object.defineProperty(Object.prototype, 'keyData', {
            set() { globalThis.setterCalls++; }, configurable: true
        });
        const provider = Deno.core.loadExtScript('ext:deno_crypto/00_crypto.js');
        globalThis.crypto = provider.crypto;
        provider.importCryptoKeySync('raw', new Uint8Array(16),
            {name: 'AES-GCM'}, false, ['encrypt']);
    "#).unwrap();
    {
        deno_core::scope!(scope, runtime);
        let key: v8::Local<v8::Value> = v8::Local::new(scope, key);
        assert!(deno_crypto::is_crypto_key(scope, key));
        let snapshot = deno_crypto::NativeKeySnapshot::capture(scope, key).unwrap();
        assert!(snapshot.size_bytes() >= 16);
        let restored = snapshot.restore(scope);
        assert!(deno_crypto::is_crypto_key(scope, restored.into()));
        let global = scope.get_current_context().global(scope);
        let name = v8::String::new(scope, "restoredKey").unwrap();
        assert!(global.set(scope, name.into(), restored.into()).unwrap());
    }
    runtime.execute_script("check.js", r#"
        if (setterCalls !== 0) throw Error('private key bytes reached inherited setter');
        if (restoredKey.extractable !== false || restoredKey.type !== 'secret' ||
            restoredKey.algorithm.name !== 'AES-GCM' || restoredKey.usages.join() !== 'encrypt')
            throw Error('restored native key policy changed');
        delete Object.prototype.keyData;
    "#).unwrap();
}
