//! Official native WebCrypto classes; no replacement of the rtdom Web globals.
use deno_core::Extension;

pub(crate) const BOOTSTRAP: &str = include_str!("crypto_backend.js");
const ALGORITHMS: &str = include_str!("crypto_algorithms.js");
const HYBRID: &str = include_str!("crypto_hybrid.js");

pub(crate) fn install<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    core: v8::Local<'s, v8::Object>,
    secure: bool,
) -> Result<(), String> {
    let global = scope.get_current_context().global(scope);
    let name = v8::String::new(scope, "ts").unwrap();
    let key = v8::Symbol::for_key(scope, name);
    let namespace = global
        .get(scope, key.into())
        .and_then(|value| value.to_object(scope))
        .ok_or("native DOM function factory namespace missing")?;
    let key = v8::String::new(scope, "makeNativeFunction").unwrap();
    let factory = namespace
        .get(scope, key.into())
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
        .ok_or("native DOM function factory missing")?;
    v8::tc_scope!(let scope, scope);
    let algorithm_source = v8::String::new(scope, ALGORITHMS).unwrap();
    let algorithm_factory = v8::Script::compile(scope, algorithm_source, None)
        .and_then(|script| script.run(scope))
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
        .ok_or("native WebCrypto algorithm profile failed")?;
    let hybrid_source = v8::String::new(scope, HYBRID).unwrap();
    let hybrid_factory = v8::Script::compile(scope, hybrid_source, None)
        .and_then(|script| script.run(scope))
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
        .ok_or("native WebCrypto hybrid adapter failed")?;
    let source = v8::String::new(scope, BOOTSTRAP).unwrap();
    let result = v8::Script::compile(scope, source, None)
        .and_then(|script| script.run(scope))
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok());
    if let Some(function) = result {
        let receiver = v8::undefined(scope);
        let secure = v8::Boolean::new(scope, secure);
        if function
            .call(
                scope,
                receiver.into(),
                &[
                    core.into(),
                    factory.into(),
                    secure.into(),
                    algorithm_factory.into(),
                    hybrid_factory.into(),
                ],
            )
            .is_some()
        {
            return Ok(());
        }
    }
    Err(scope
        .exception()
        .map(|error| error.to_rust_string_lossy(scope))
        .unwrap_or_else(|| "native WebCrypto bootstrap failed".to_owned()))
}

pub(crate) fn extensions() -> Vec<Extension> {
    vec![
        deno_webidl::deno_webidl::init(),
        deno_web::deno_web::init(
            deno_web::BlobStore::default_arc(),
            None,
            false,
            deno_web::InMemoryBroadcastChannel::default(),
        ),
        deno_crypto::deno_crypto::init(None),
        crate::crypto_hybrid::extension(),
    ]
}
