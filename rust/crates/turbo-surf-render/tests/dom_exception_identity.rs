//! turbo-surf#42: a page sees one `DOMException` interface per realm. Exceptions it
//! authors, that surf's natives raise and that turbo-dom-browser-env's modules raise
//! (`HTMLImageElement.prototype.decode()`'s `EncodingError`) are all `instanceof` it,
//! `structuredClone` serializes them, and the IDL getters brand-check another same-origin
//! realm's exceptions — checked against a Chrome 156 control
//! (`fixture-gen/dom-exception-identity-chrome.mjs`).
#[path = "support/binding_control.rs"]
mod binding_control;

#[tokio::test(flavor = "current_thread")]
async fn every_dom_exception_is_the_realm_interface_like_chrome156() {
    // Known gap (turbo-surf#42): the binding's modules raise the DOMException they captured
    // at install, and surf installs its own DOMException afterwards. Their exceptions are
    // instances of neither realm's interface, and as plain objects to surf's serializer
    // they clone to `{}`.
    let gaps = [
        (
            "decode",
            serde_json::json!([false, false, false, "EncodingError", 0]),
        ),
        ("decodeClone", serde_json::json!([false, null, null])),
        ("childDecode", serde_json::json!([false, false])),
    ];
    binding_control::matches_control_with_gaps(
        include_str!("fixtures/dom_exception_identity_probe.js"),
        include_bytes!("fixtures/dom_exception_identity_cases.json"),
        include_str!("fixtures/dom_exception_identity_chrome156.json"),
        |case| case["url"].as_str().unwrap(),
        &gaps,
    )
    .await;
}
