//! Exercise the committed native registry through the production PageSession.
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use turbo_dom_browser_env::test_support;
use turbo_surf_render::PageSession;

async fn check(probe: &str, fixture: &str) {
    let fixture_json = serde_json::to_string(fixture).unwrap();
    let probe_sha = format!("{:x}", Sha256::digest(probe.as_bytes()));
    let mut page = PageSession::open(
        "<!doctype html><body>",
        "https://custom-elements.test/document.html",
        "",
        "",
        10000,
    )
    .await
    .unwrap();
    // Compare in V8: DOM strings and valid JSON may contain lone UTF-16 units
    // which serde_json::Value cannot represent as a Rust String.
    let value = page
        .eval_expression(&format!(
            r#"(async()=>{{
                const stringify=JSON.stringify.bind(JSON),keys=Object.keys;
                const control=JSON.parse({fixture_json});
                if(!control.browser.startsWith("Chrome/155.")||control.v8!=="15.5.35.23"||control.errors.length||control.probeSha256!=="{probe_sha}")throw Error("Stale custom-element control");
                const actual=await ({probe});
                return stringify(keys(control.result).filter(key=>stringify(actual[key])!==stringify(control.result[key])).map(key=>({{key,actual:actual[key],expected:control.result[key]}})));
            }})()"#,
            probe = probe.trim_end().trim_end_matches(';')
        ))
        .await
        .unwrap();
    assert_eq!(value, "[]");
}

/// A binding probe/control file that `test_support` does not export as a constant
/// yet, read from the turbo-dom-browser-env package (`src/`) this build resolved.
fn binding_file(name: &str) -> String {
    static SRC: OnceLock<PathBuf> = OnceLock::new();
    let src = SRC.get_or_init(|| {
        let output = Command::new(env!("CARGO"))
            .args(["metadata", "--format-version", "1", "--manifest-path"])
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
            .output()
            .unwrap();
        assert!(output.status.success(), "cargo metadata failed");
        let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let manifest = metadata["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|package| package["name"] == "turbo-dom-browser-env")
            .and_then(|package| package["manifest_path"].as_str())
            .unwrap();
        Path::new(manifest).parent().unwrap().join("src")
    });
    std::fs::read_to_string(src.join(name)).unwrap()
}

macro_rules! contract {
    ($name:ident, $probe:literal, $fixture:literal) => {
        #[tokio::test(flavor = "current_thread")]
        async fn $name() {
            check(&binding_file($probe), &binding_file($fixture)).await;
        }
    };
}
contract!(
    registry_construction_and_lifecycle,
    "custom_elements_probe.js",
    "custom_elements_chrome155.json"
);
contract!(
    extensions_failures_and_scoped_registries,
    "custom_elements_extended_probe.js",
    "custom_elements_extended_chrome155.json"
);
contract!(
    reactions_and_clone_import_lifecycle,
    "custom_elements_reactions_probe.js",
    "custom_elements_reactions_chrome155.json"
);
contract!(
    idl_conversion_and_failure_atomicity,
    "custom_elements_idl_probe.js",
    "custom_elements_idl_chrome155.json"
);
contract!(
    move_before_reaction_order_and_callbacks,
    "custom_elements_moves_probe.js",
    "custom_elements_moves_chrome155.json"
);

#[tokio::test(flavor = "current_thread")]
async fn cross_realm_adoption_and_borrowed_registry_methods() {
    if !turbo_surf_render::stable_window_proxy() {
        return;
    }
    check(
        test_support::CUSTOM_ELEMENTS_REALMS_PROBE_JS,
        test_support::CUSTOM_ELEMENTS_REALMS_CHROME155_JSON,
    )
    .await;
}

contract!(
    callback_realms_errors_and_reentrant_adoption,
    "custom_elements_callbacks_probe.js",
    "custom_elements_callbacks_chrome155.json"
);

contract!(
    contextual_parser_and_observer_lifecycle,
    "dom_parser_observer_lifecycle_probe.js",
    "dom_parser_observer_lifecycle_chrome155.json"
);

contract!(
    cross_document_shadow_stylesheet_adoption_lifecycle,
    "css_adoption_lifecycle_probe.js",
    "css_adoption_lifecycle_chrome155.json"
);

contract!(
    retired_frame_preserves_registry_but_suppresses_automatic_custom_element_lifecycle,
    "custom_elements_retired_probe.js",
    "custom_elements_retired_chrome155.json"
);
