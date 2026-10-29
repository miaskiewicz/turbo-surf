// The distributions' compile-time source tables must match what each extension
// declares, so an embedder can substitute them without reading build paths.
macro_rules! check_extension {
    ($krate:ident, $extension:expr, $expected:expr) => {{
        use $krate::embedded_sources::{EXTENSION, LICENSE, SOURCES, SourceKind};
        let extension: deno_core::Extension = $extension;
        assert_eq!(EXTENSION, extension.name);
        assert!(LICENSE.starts_with("MIT License"));
        assert_eq!(SOURCES.len(), $expected);
        let lists = [
            (SourceKind::Js, &extension.js_files),
            (SourceKind::Esm, &extension.esm_files),
            (SourceKind::LazyJs, &extension.lazy_loaded_js_files),
            (SourceKind::LazyEsm, &extension.lazy_loaded_esm_files),
        ];
        for (kind, declared) in lists {
            let embedded: Vec<_> = SOURCES.iter().filter(|s| s.kind == kind).collect();
            assert_eq!(embedded.len(), declared.len(), "{} {:?}", EXTENSION, kind);
            for (source, declared) in embedded.iter().zip(declared.iter()) {
                assert_eq!(source.extension, EXTENSION);
                assert_eq!(source.specifier, declared.specifier);
                assert_eq!(source.code.as_str().len(), source.bytes);
                assert!(source.code.as_str().is_ascii());
                assert_eq!(source.file_source().specifier, source.specifier);
            }
        }
    }};
}

#[test]
fn embedded_sources_match_extension_declarations() {
    check_extension!(deno_webidl, deno_webidl::deno_webidl::init(), 1);
    check_extension!(deno_crypto, deno_crypto::deno_crypto::init(None), 1);
    check_extension!(
        deno_web,
        deno_web::deno_web::init(
            deno_web::BlobStore::default_arc(),
            None,
            false,
            deno_web::InMemoryBroadcastChannel::default(),
        ),
        26
    );
}
