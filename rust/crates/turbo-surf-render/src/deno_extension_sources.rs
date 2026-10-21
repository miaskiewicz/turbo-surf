//! Compile-time official extension sources, independent of the build machine.
use deno_core::{Extension, ExtensionFileSource, FastStaticString};
use std::borrow::Cow;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Js,
    Esm,
    LazyJs,
    LazyEsm,
}

struct EmbeddedSource {
    extension: &'static str,
    kind: SourceKind,
    specifier: &'static str,
    code: FastStaticString,
}

include!("deno_extension_source_table.rs");

fn embed_list(
    name: &'static str,
    kind: SourceKind,
    files: &mut Cow<'static, [ExtensionFileSource]>,
) -> Result<(), String> {
    let expected: Vec<_> = EMBEDDED
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
    let name = extension.name;
    if !EMBEDDED.iter().any(|source| source.extension == name) {
        return Err(format!("Unknown official Deno source extension {name}"));
    }
    embed_list(name, SourceKind::Js, &mut extension.js_files)?;
    embed_list(name, SourceKind::Esm, &mut extension.esm_files)?;
    embed_list(
        name,
        SourceKind::LazyJs,
        &mut extension.lazy_loaded_js_files,
    )?;
    embed_list(
        name,
        SourceKind::LazyEsm,
        &mut extension.lazy_loaded_esm_files,
    )?;
    Ok(extension)
}

#[cfg(test)]
#[path = "deno_extension_sources_tests.rs"]
mod tests;
