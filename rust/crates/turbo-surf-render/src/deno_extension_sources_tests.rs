use super::*;
use deno_core::ExtensionFileSourceCode;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const ABSENT_SOURCE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/__turbo_surf_absent_extension_build_source__"
);

fn official_extensions() -> Vec<Extension> {
    vec![
        deno_webidl::deno_webidl::init(),
        deno_web::deno_web::init(
            deno_web::BlobStore::default_arc(),
            None,
            false,
            deno_web::InMemoryBroadcastChannel::default(),
        ),
        deno_crypto::deno_crypto::init(None),
    ]
}

fn lists(extension: &Extension) -> [&[ExtensionFileSource]; 4] {
    [
        &extension.js_files,
        &extension.esm_files,
        &extension.lazy_loaded_js_files,
        &extension.lazy_loaded_esm_files,
    ]
}

fn inventory(extension: &Extension) -> Vec<(usize, &'static str)> {
    lists(extension)
        .into_iter()
        .enumerate()
        .flat_map(|(kind, files)| files.iter().map(move |file| (kind, file.specifier)))
        .collect()
}

fn poison_build_sources(mut extension: Extension) -> Extension {
    assert!(!std::path::Path::new(ABSENT_SOURCE).exists());
    for files in [
        &mut extension.js_files,
        &mut extension.esm_files,
        &mut extension.lazy_loaded_js_files,
        &mut extension.lazy_loaded_esm_files,
    ] {
        for file in files.to_mut() {
            file.code = ExtensionFileSourceCode::LoadedFromFsDuringSnapshot(ABSENT_SOURCE);
            assert!(file.load().is_err(), "poisoned source unexpectedly loaded");
        }
    }
    extension
}

fn harden(extension: Extension) -> Extension {
    match extension.name {
        "deno_webidl" => super::super::web_event_arrays::harden_webidl(extension).unwrap(),
        "deno_web" => super::super::web_event_arrays::harden(extension).unwrap(),
        "deno_crypto" => extension,
        _ => panic!("unexpected official extension"),
    }
}

fn prepared_poisoned_extensions() -> Vec<Extension> {
    official_extensions()
        .into_iter()
        .map(|extension| harden(embed(poison_build_sources(extension)).unwrap()))
        .collect()
}

#[test]
fn all_official_sources_embed_exact_bytes_without_build_files() {
    assert_eq!(EMBEDDED.len(), 28);
    assert_eq!(SOURCE_HASHES.len(), 28);
    let expected: BTreeMap<_, _> = SOURCE_HASHES.iter().copied().collect();
    assert_eq!(expected.len(), 28);
    let mut seen = BTreeMap::new();
    for extension in official_extensions() {
        let original = inventory(&extension);
        let embedded = embed(poison_build_sources(extension)).unwrap();
        assert_eq!(inventory(&embedded), original, "kind/order/specifier drift");
        for files in lists(&embedded) {
            for source in files {
                assert!(source.is_runtime_loadable());
                let bytes = source.load().unwrap();
                let actual = format!("{:x}", Sha256::digest(bytes.as_bytes()));
                assert_eq!(
                    Some(actual.as_str()),
                    expected.get(source.specifier).copied()
                );
                assert!(seen.insert(source.specifier, actual).is_none());
            }
        }
    }
    assert_eq!(seen.len(), 28);
}

#[test]
fn embedding_preserves_existing_three_pinned_hardening_adaptations() {
    let expected: BTreeMap<_, _> = SOURCE_HASHES.iter().copied().collect();
    let adapted = [
        "ext:deno_webidl/00_webidl.js",
        "ext:deno_web/02_event.js",
        "ext:deno_web/03_abort_signal.js",
    ];
    let originals = official_extensions();
    let prepared = prepared_poisoned_extensions();
    let mut changed = 0;
    for (original, extension) in originals.iter().zip(&prepared) {
        assert_eq!(inventory(original), inventory(extension));
        for files in lists(extension) {
            for source in files {
                assert!(source.is_runtime_loadable());
                let code = source.load().unwrap();
                let actual = format!("{:x}", Sha256::digest(code.as_bytes()));
                if adapted.contains(&source.specifier) {
                    assert_ne!(actual.as_str(), expected[source.specifier]);
                    assert!(code.contains("ArrayPrototypePush: push"));
                    assert!(!code.contains("  ArrayPrototypePush,\n"));
                    changed += 1;
                } else {
                    assert_eq!(actual.as_str(), expected[source.specifier]);
                }
            }
        }
    }
    assert_eq!(changed, 3);
}

#[test]
fn official_source_inventory_drift_fails_closed() {
    let web = || official_extensions().remove(1);
    let mut unknown_extension = web();
    unknown_extension.name = "unreviewed_extension";
    assert!(embed(unknown_extension).is_err());
    let mut missing = web();
    missing.lazy_loaded_js_files.to_mut().remove(0);
    assert!(embed(missing).is_err());
    let mut duplicate = web();
    let first = duplicate.lazy_loaded_js_files[0].clone();
    duplicate.lazy_loaded_js_files.to_mut().push(first);
    assert!(embed(duplicate).is_err());
    let mut unknown_source = web();
    unknown_source.lazy_loaded_js_files.to_mut()[0] = ExtensionFileSource::new_computed(
        "ext:deno_web/unreviewed.js",
        "globalThis.unreviewed = true;".into(),
    );
    assert!(embed(unknown_source).is_err());
    let mut reordered = web();
    reordered.lazy_loaded_js_files.to_mut().swap(0, 1);
    assert!(embed(reordered).is_err());
    let mut wrong_kind = web();
    let first = wrong_kind.lazy_loaded_js_files.to_mut().remove(0);
    wrong_kind.js_files.to_mut().push(first);
    assert!(embed(wrong_kind).is_err());
}

#[test]
fn poisoned_prepared_extensions_run_native_crypto_streams_and_timers() {
    const NAME: &str = "crypto_backend::deno_extension_sources::tests::poisoned_prepared_extensions_run_native_crypto_streams_and_timers";
    // The binding's unit suite also owns standalone V8 initializers. Exec this
    // exact test alone so this actual JsRuntime has a fresh process/platform.
    if !std::env::args().any(|argument| argument == NAME) {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", NAME, "--nocapture", "--test-threads=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let output = child.wait_with_output().unwrap();
                panic!(
                    "portable extension child exceeded 30s: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    crate::runtime::ensure_platform();
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    executor.block_on(async {
        let mut runtime = deno_core::JsRuntime::new(deno_core::RuntimeOptions {
            extensions: prepared_poisoned_extensions(),
            ..Default::default()
        });
        runtime.execute_script("<portable-official-extensions>", r#"
          (async () => {
            const load = Deno.core.loadExtScript;
            const webidl = load('ext:deno_webidl/00_webidl.js');
            const {crypto} = load('ext:deno_crypto/00_crypto.js');
            const {ReadableStream} = load('ext:deno_web/06_streams.js');
            const {setTimeout} = load('ext:deno_web/02_timers.js');
            const bytes = new Uint8Array(4);
            const identity = crypto.getRandomValues(bytes) === bytes;
            const digest = await crypto.subtle.digest('SHA-256', new Uint8Array([97,98,99]));
            const hex = Array.from(new Uint8Array(digest), b => b.toString(16).padStart(2,'0')).join('');
            const reader = new ReadableStream({start(c) { c.enqueue('stream'); c.close(); }}).getReader();
            const first = await reader.read(), last = await reader.read();
            const timer = await new Promise(resolve => setTimeout(() => resolve('timer'), 0));
            globalThis.__portableResult = {identity, hex, first, last, timer, idl: typeof webidl.converters.DOMString};
          })().catch(error => { globalThis.__portableError = String(error && error.stack || error); });
        "#).unwrap();
        runtime.run_event_loop(Default::default()).await.unwrap();
        let value = runtime.execute_script("<portable-result>", "JSON.stringify(globalThis.__portableResult || {error: globalThis.__portableError})").unwrap();
        deno_core::scope!(scope, &mut runtime);
        let text = v8::Local::new(scope, value).to_rust_string_lossy(scope);
        let actual: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(actual, serde_json::json!({
            "identity": true,
            "hex": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "first": {"value": "stream", "done": false},
            "last": {"done": true},
            "timer": "timer",
            "idl": "function"
        }));
    });
}
