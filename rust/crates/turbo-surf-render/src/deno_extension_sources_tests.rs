use super::*;
use deno_core::ExtensionFileSourceCode;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Reviewed SHA-256 and byte length of every official source, in declaration
/// order (deno_web 0.290, deno_webidl 0.259, deno_crypto 0.273 JavaScript, carried
/// unchanged by the 0.x.1 distributions). Formerly `src/vendor/deno_extensions/
/// pins.json`; a distribution update that changes any byte fails here.
#[rustfmt::skip]
const SOURCE_PINS: &[(&str, &str, usize)] = &[
    ("ext:deno_web/locks.js", "4a497cc252ee9bcd4243f60c9dafe68e3b97e4a971b19dd9cb307ffbc65692dd", 9266),
    ("ext:deno_web/webtransport.js", "7c7597ddaae73649f195eb264642f21dc684fd4e290e9032331a23c421d453b6", 32004),
    ("ext:deno_web/00_infra.js", "9e7af28dda103610318e055cedc27a4025e60b0f5b07b9485e1adcaa403a60e4", 10156),
    ("ext:deno_web/00_url.js", "287bfc3bd3c69735db823802feaf845f928d651a4bc2334a7d5911b3e2e54d55", 35673),
    ("ext:deno_web/01_broadcast_channel.js", "6beeaaaa0e0b3f8445bb233cda7fd669d7d8d38a5ad17f48b9f632069dbea8f7", 6690),
    ("ext:deno_web/01_console.js", "338b4977b1366a2e93dadac91b9910c638b4c4e42417af76ec51767a973be0f3", 18989),
    ("ext:deno_web/01_dom_exception.js", "11a4aaa1dfba2faa9b869d0d59afd80dfd73afe409bcedd137099eca77fd0932", 9755),
    ("ext:deno_web/01_mimesniff.js", "fb19ee5d4c9b29273984d0b3ae2065a1feab5736e51944a5f9a8353cc5f2dafc", 10743),
    ("ext:deno_web/01_urlpattern.js", "390f5d05553c87c1332dbe6b3c0fdda71331231ebc870bc8f47f23abb976965a", 14370),
    ("ext:deno_web/02_event.js", "d2213f8eb208a3bf9068ff872544b6e26e2954cd10ea3a3d92c487f19850baf0", 43921),
    ("ext:deno_web/02_structured_clone.js", "603508bbe0bfd3d6b2e92df0eb5583d560f9b9920f5e126475e79ef32228720d", 3849),
    ("ext:deno_web/02_timers.js", "430df6ab2f6f2038cda5b48f554b6c011b277aef3c25090eea17d56da1e534a9", 4652),
    ("ext:deno_web/03_abort_signal.js", "24bae9830d93344f9cb363f27ef8d1e21ee2d680ed5c91aea6ba9af499461983", 13214),
    ("ext:deno_web/04_global_interfaces.js", "ceb5866a82585493d9ecf15d6791390f9c76adc36122a709eaf1606cc635161a", 1839),
    ("ext:deno_web/05_base64.js", "4497e5d9067c9402b54d55330fc12fc9a48db7e2ac9e65c33b67980f562858d8", 1678),
    ("ext:deno_web/06_streams.js", "b35c039ee8cb6932b91f71d43e47d6f2fbd9d68e13cdedeb03ba60cca3369787", 253786),
    ("ext:deno_web/08_text_encoding.js", "8c6e65f4cf52a4f1c8a60d80a4f65475927f6233f40989b0a1dcabcc996ac24c", 19146),
    ("ext:deno_web/09_file.js", "53bf526755ea1d7d32564cb8a7fdfc3838a697f3e5593b96b88708fa6fa45e05", 24452),
    ("ext:deno_web/10_filereader.js", "459af415091e50d08a4dbc680466ea25abbd12bb61ca58fad926c8f2366deb27", 17475),
    ("ext:deno_web/12_location.js", "237c142f610cfe8bc662c30887354528bad4954916736b101adc98c113b92d8a", 10477),
    ("ext:deno_web/13_message_port.js", "c6dc65a01d41e0f5afd9da8f4e806844a09b69d5e0f82abaefda0fc90411c4f4", 40092),
    ("ext:deno_web/14_compression.js", "1ffe8ad28324b5f24fe09407093a2376c23b74d3ac35fc2a04fc7ee20032191d", 5014),
    ("ext:deno_web/15_performance.js", "30aa41363804c47eb26140a993a2f4c8d3e8d395fdfd82bed8911b132d9579cd", 22839),
    ("ext:deno_web/16_image_data.js", "873fdfb4c5d422bdec5e76d946bf172f3e3f321df21131930f485b6fd32734d0", 1143),
    ("ext:deno_web/17_geometry.js", "14211860a08106764c8df3079d770383feab61d48df9a943dc13f0addecef0ef", 5040),
    ("ext:deno_web/18_css_stylesheet.js", "b952842b310b124ea94b34178d065129f8d73c145b2c176a1569141b9a939863", 1637),
    ("ext:deno_webidl/00_webidl.js", "9b94137c7e48d21f2c635ea10537b0059106fcb6c77953c25db7682c61559372", 46603),
    ("ext:deno_crypto/00_crypto.js", "c0400a2298b3cce374917bb363a941d0e6be4acbce4ea8585419b415d16b1575", 12897),
];

/// SHA-256 of the Deno MIT license each distribution embeds as `LICENSE`.
const LICENSE_SHA256: &str = "f62497fffecc0852960c8d3e6934b9db86d16396e9b604072e923892cae3a588";

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

fn source_hashes() -> BTreeMap<&'static str, &'static str> {
    SOURCE_PINS
        .iter()
        .map(|(specifier, sha256, _)| (*specifier, *sha256))
        .collect()
}

#[test]
fn distribution_tables_match_the_reviewed_pins() {
    let embedded = embedded();
    assert_eq!(embedded.len(), SOURCE_PINS.len());
    for (source, (specifier, sha256, bytes)) in embedded.iter().zip(SOURCE_PINS) {
        assert_eq!(source.specifier, *specifier);
        let code = source.code.as_str();
        assert!(code.is_ascii(), "{specifier}");
        // The distribution's own pins describe its bytes...
        assert_eq!(code.len(), source.bytes, "{specifier}");
        assert_eq!(
            format!("{:x}", Sha256::digest(code)),
            source.sha256,
            "{specifier}"
        );
        // ...and equal the reviewed pins.
        assert_eq!(
            (source.sha256, source.bytes),
            (*sha256, *bytes),
            "{specifier}"
        );
    }
    for (extension, license, pinned) in [
        (
            deno_web::embedded_sources::EXTENSION,
            deno_web::embedded_sources::LICENSE,
            deno_web::embedded_sources::LICENSE_SHA256,
        ),
        (
            deno_webidl::embedded_sources::EXTENSION,
            deno_webidl::embedded_sources::LICENSE,
            deno_webidl::embedded_sources::LICENSE_SHA256,
        ),
        (
            deno_crypto::embedded_sources::EXTENSION,
            deno_crypto::embedded_sources::LICENSE,
            deno_crypto::embedded_sources::LICENSE_SHA256,
        ),
    ] {
        assert_eq!(
            format!("{:x}", Sha256::digest(license)),
            pinned,
            "{extension}"
        );
        assert_eq!(pinned, LICENSE_SHA256, "{extension}");
        assert!(embedded.iter().any(|source| source.extension == extension));
    }
}

#[test]
fn all_official_sources_embed_exact_bytes_without_build_files() {
    assert_eq!(embedded().len(), 28);
    assert_eq!(SOURCE_PINS.len(), 28);
    let expected = source_hashes();
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
    let expected = source_hashes();
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
