//! Exercise the committed native registry through the production PageSession.
use sha2::{Digest, Sha256};
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

macro_rules! contract {
    ($name:ident, $probe:ident, $fixture:ident) => {
        #[tokio::test(flavor = "current_thread")]
        async fn $name() {
            check(test_support::$probe, test_support::$fixture).await;
        }
    };
}
contract!(
    registry_construction_and_lifecycle,
    CUSTOM_ELEMENTS_PROBE_JS,
    CUSTOM_ELEMENTS_CHROME155_JSON
);
contract!(
    extensions_failures_and_scoped_registries,
    CUSTOM_ELEMENTS_EXTENDED_PROBE_JS,
    CUSTOM_ELEMENTS_EXTENDED_CHROME155_JSON
);
contract!(
    reactions_and_clone_import_lifecycle,
    CUSTOM_ELEMENTS_REACTIONS_PROBE_JS,
    CUSTOM_ELEMENTS_REACTIONS_CHROME155_JSON
);
contract!(
    idl_conversion_and_failure_atomicity,
    CUSTOM_ELEMENTS_IDL_PROBE_JS,
    CUSTOM_ELEMENTS_IDL_CHROME155_JSON
);
contract!(
    move_before_reaction_order_and_callbacks,
    CUSTOM_ELEMENTS_MOVES_PROBE_JS,
    CUSTOM_ELEMENTS_MOVES_CHROME155_JSON
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
    CUSTOM_ELEMENTS_CALLBACKS_PROBE_JS,
    CUSTOM_ELEMENTS_CALLBACKS_CHROME155_JSON
);

contract!(
    contextual_parser_and_observer_lifecycle,
    DOM_PARSER_OBSERVER_LIFECYCLE_PROBE_JS,
    DOM_PARSER_OBSERVER_LIFECYCLE_CHROME155_JSON
);

contract!(
    cross_document_shadow_stylesheet_adoption_lifecycle,
    CSS_ADOPTION_LIFECYCLE_PROBE_JS,
    CSS_ADOPTION_LIFECYCLE_CHROME155_JSON
);

contract!(
    retired_frame_preserves_registry_but_suppresses_automatic_custom_element_lifecycle,
    CUSTOM_ELEMENTS_RETIRED_PROBE_JS,
    CUSTOM_ELEMENTS_RETIRED_CHROME155_JSON
);
