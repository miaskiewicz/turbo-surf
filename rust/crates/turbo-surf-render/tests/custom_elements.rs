//! Exercise the committed native registry through the production PageSession.
use sha2::{Digest, Sha256};
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

macro_rules! contract {
    ($name:ident, $probe:literal, $fixture:literal) => {
        #[tokio::test(flavor = "current_thread")]
        async fn $name() {
            check(include_str!($probe), include_str!($fixture)).await;
        }
    };
}
contract!(
    registry_construction_and_lifecycle,
    "../src/custom_elements_probe.js",
    "../src/custom_elements_chrome155.json"
);
contract!(
    extensions_failures_and_scoped_registries,
    "../src/custom_elements_extended_probe.js",
    "../src/custom_elements_extended_chrome155.json"
);
contract!(
    reactions_and_clone_import_lifecycle,
    "../src/custom_elements_reactions_probe.js",
    "../src/custom_elements_reactions_chrome155.json"
);
contract!(
    idl_conversion_and_failure_atomicity,
    "../src/custom_elements_idl_probe.js",
    "../src/custom_elements_idl_chrome155.json"
);
contract!(
    move_before_reaction_order_and_callbacks,
    "../src/custom_elements_moves_probe.js",
    "../src/custom_elements_moves_chrome155.json"
);

#[tokio::test(flavor = "current_thread")]
async fn cross_realm_adoption_and_borrowed_registry_methods() {
    if !turbo_surf_render::stable_window_proxy() {
        return;
    }
    check(
        include_str!("../src/custom_elements_realms_probe.js"),
        include_str!("../src/custom_elements_realms_chrome155.json"),
    )
    .await;
}

contract!(
    callback_realms_errors_and_reentrant_adoption,
    "../src/custom_elements_callbacks_probe.js",
    "../src/custom_elements_callbacks_chrome155.json"
);

contract!(
    contextual_parser_and_observer_lifecycle,
    "../src/dom_parser_observer_lifecycle_probe.js",
    "../src/dom_parser_observer_lifecycle_chrome155.json"
);

contract!(
    cross_document_shadow_stylesheet_adoption_lifecycle,
    "../src/css_adoption_lifecycle_probe.js",
    "../src/css_adoption_lifecycle_chrome155.json"
);

contract!(
    retired_frame_preserves_registry_but_suppresses_automatic_custom_element_lifecycle,
    "../src/custom_elements_retired_probe.js",
    "../src/custom_elements_retired_chrome155.json"
);
