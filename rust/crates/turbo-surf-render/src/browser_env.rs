//! Generated browser binding adapter; source: scripts/browser-env-adapter.rs.in.
//! Canonical DOM + CSS grammar + JS: turbo-test @ e178d4b.
//! Rebuild only through scripts/vendor-browser-env.sh from committed upstream HEAD.
#![allow(unused)]
#![allow(clippy::all)]
include!("browser_env_upstream.rs");

/// Seed a fetched document using the canonical native binding.
pub fn install_html(scope: &mut v8::PinScope, html: &str) {
    install_with_html(scope, html);
}

/// Serialize the live tree back to HTML — the render result. Empty if no DOM is
/// installed. (turbo-surf addition; upstream is a test env that never serializes.)
pub fn document_html() -> String {
    DOM.with(|d| {
        d.borrow().as_ref().map(|s| {
            let root = s.tree.root();
            turbo_dom_parser::rtdom::serialize::serialize_inner(&s.tree, root)
        })
    })
    .unwrap_or_default()
}

#[path = "live_layout_dom.rs"]
mod live_layout_dom;
pub(crate) use live_layout_dom::capture as capture_native_layout;
pub(crate) use live_layout_dom::retire_context as retire_native_document;
pub(crate) use live_layout_dom::{
    capture_document as capture_native_document_layout,
    valid_receiver as valid_native_geometry_receiver, wrap_elements as wrap_native_layout_elements,
};

#[path = "compression_dictionary_dom.rs"]
mod compression_dictionary_dom;
pub(crate) use compression_dictionary_dom::links as native_compression_dictionary_links;
