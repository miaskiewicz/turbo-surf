//! Compile-time official extension sources, independent of the build machine.
//!
//! The exact published Deno JS comes from the named distributions' own
//! `embedded_sources` tables (`turbo-surf-deno-web` / `-webidl` / `-crypto`
//! 0.x.1, generated and checksum-verified by `rust/release-deps/deno/prepare.py`).
//! Nothing is vendored here; the reviewed SHA-256 / length pins are re-checked by
//! this module's tests.
use deno_core::{Extension, ExtensionFileSource, FastStaticString};
use std::borrow::Cow;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Js,
    Esm,
    LazyJs,
    LazyEsm,
}

#[derive(Clone, Copy)]
struct EmbeddedSource {
    extension: &'static str,
    kind: SourceKind,
    specifier: &'static str,
    code: FastStaticString,
    /// The distribution's own pins (prepare.py), re-checked by the tests.
    #[cfg_attr(not(test), allow(dead_code))]
    sha256: &'static str,
    #[cfg_attr(not(test), allow(dead_code))]
    bytes: usize,
}

/// One distribution's `embedded_sources::SOURCES`, in declaration order. Each
/// crate defines its own (identical) `SourceKind`/`EmbeddedSource` types.
macro_rules! distribution {
    ($krate:ident) => {
        $krate::embedded_sources::SOURCES.iter().map(|source| {
            EmbeddedSource {
                extension: source.extension,
                // The distributions declare their fieldless SourceKind in this same
                // order (prepare.py); the pinned sources use only the lazy kinds, and
                // embed_list's inventory check (every pinned extension embeds in the
                // tests) fails on any reordering.
                kind: [
                    SourceKind::Js,
                    SourceKind::Esm,
                    SourceKind::LazyJs,
                    SourceKind::LazyEsm,
                ][source.kind as usize],
                specifier: source.specifier,
                code: source.code,
                sha256: source.sha256,
                bytes: source.bytes,
            }
        })
    };
}

fn embedded() -> Vec<EmbeddedSource> {
    distribution!(deno_web)
        .chain(distribution!(deno_webidl))
        .chain(distribution!(deno_crypto))
        .collect()
}

fn embed_list(
    embedded: &[EmbeddedSource],
    name: &'static str,
    kind: SourceKind,
    files: &mut Cow<'static, [ExtensionFileSource]>,
) -> Result<(), String> {
    let expected: Vec<_> = embedded
        .iter()
        .filter(|source| source.extension == name && source.kind == kind)
        .collect();
    if files.len() != expected.len()
        || files
            .iter()
            .zip(&expected)
            .any(|(actual, pinned)| actual.specifier != pinned.specifier)
    {
        return Err(format!("Official Deno source inventory changed for {name}"));
    }
    // No load() here: upstream paths belong to the build container, not the
    // installed application. Only the reviewed compile-time bytes are executed.
    *files = Cow::Owned(
        expected
            .into_iter()
            .map(|source| ExtensionFileSource::new(source.specifier, source.code))
            .collect(),
    );
    Ok(())
}

pub(super) fn embed(mut extension: Extension) -> Result<Extension, String> {
    let embedded = embedded();
    let name = extension.name;
    if !embedded.iter().any(|source| source.extension == name) {
        return Err(format!("Unknown official Deno source extension {name}"));
    }
    embed_list(&embedded, name, SourceKind::Js, &mut extension.js_files)?;
    embed_list(&embedded, name, SourceKind::Esm, &mut extension.esm_files)?;
    embed_list(
        &embedded,
        name,
        SourceKind::LazyJs,
        &mut extension.lazy_loaded_js_files,
    )?;
    embed_list(
        &embedded,
        name,
        SourceKind::LazyEsm,
        &mut extension.lazy_loaded_esm_files,
    )?;
    Ok(extension)
}

#[cfg(test)]
#[path = "deno_extension_sources_tests.rs"]
mod tests;
