//! Harden only private dense event arrays, preserving the official Deno algorithms.
use deno_core::{Extension, ExtensionFileSource};
use sha2::{Digest, Sha256};

const HELPERS: &str = include_str!("web_event_arrays.js");
const MODULES: &[(&str, &str, &[&str])] = &[
    (
        "ext:deno_web/02_event.js",
        "d2213f8eb208a3bf9068ff872544b6e26e2954cd10ea3a3d92c487f19850baf0",
        &[
            "ArrayPrototypePush",
            "ArrayPrototypeSlice",
            "ArrayPrototypeMap",
            "ArrayPrototypeSplice",
            "ArrayPrototypeUnshift",
        ],
    ),
    (
        "ext:deno_web/03_abort_signal.js",
        "24bae9830d93344f9cb363f27ef8d1e21ee2d680ed5c91aea6ba9af499461983",
        &["ArrayPrototypePush"],
    ),
];

fn adapt(source: &str, expected: &str, names: &[&str]) -> Result<String, String> {
    if format!("{:x}", Sha256::digest(source.as_bytes())) != expected {
        return Err(
            "Official Deno event source changed; review the private dense-array adapter".into(),
        );
    }
    let marker = "} = primordials;";
    if source.matches(marker).count() != 1 {
        return Err("Official Deno primordial declaration is ambiguous".into());
    }
    let mut source = source.to_owned();
    for name in names {
        let import = format!("  {name},\n");
        if source.matches(&import).count() != 1 {
            return Err(format!("Official Deno {name} import changed"));
        }
        source = source.replacen(&import, "", 1);
    }
    let helper_source = HELPERS.trim().trim_end_matches(';');
    let helpers = format!(
        "{marker}\nconst {{ {} }} = ({helper_source})(primordials);",
        names.join(", ")
    );
    Ok(source.replacen(marker, &helpers, 1))
}

const WEBIDL: &[(&str, &str, &[&str])] = &[(
    "ext:deno_webidl/00_webidl.js",
    "9b94137c7e48d21f2c635ea10537b0059106fcb6c77953c25db7682c61559372",
    &["ArrayPrototypePush"],
)];

pub(crate) fn harden(extension: Extension) -> Result<Extension, String> {
    harden_modules(extension, MODULES)
}

pub(crate) fn harden_webidl(extension: Extension) -> Result<Extension, String> {
    harden_modules(extension, WEBIDL)
}

fn harden_modules(
    mut extension: Extension,
    modules: &[(&'static str, &str, &[&str])],
) -> Result<Extension, String> {
    let sources = extension.lazy_loaded_js_files.to_mut();
    for (specifier, expected, names) in modules {
        let matches: Vec<_> = sources
            .iter()
            .enumerate()
            .filter(|(_, source)| source.specifier == *specifier)
            .map(|(index, _)| index)
            .collect();
        let [index] = matches.as_slice() else {
            return Err(format!(
                "Official Deno module {specifier} missing or duplicated"
            ));
        };
        let source = sources[*index].load().map_err(|error| error.to_string())?;
        let source = adapt(source.as_ref(), expected, names)?;
        sources[*index] = ExtensionFileSource::new_computed(specifier, source.into());
    }
    Ok(extension)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn webidl_extension_is_pinned_and_hardened() {
        let extension = harden_webidl(deno_webidl::deno_webidl::init()).unwrap();
        let source = extension.lazy_loaded_js_files[0].load().unwrap();
        assert!(source.contains("ArrayPrototypePush: push"));
        assert!(!source.contains("  ArrayPrototypePush,\n"));
        assert!(adapt("changed", WEBIDL[0].1, WEBIDL[0].2).is_err());
        assert!(harden_webidl(deno_web::deno_web::init(
            deno_web::BlobStore::default_arc(),
            None,
            false,
            deno_web::InMemoryBroadcastChannel::default(),
        ))
        .is_err());
    }

    #[test]
    fn upstream_drift_is_rejected_before_execution() {
        assert!(adapt("changed", MODULES[0].1, MODULES[0].2).is_err());
    }
}
