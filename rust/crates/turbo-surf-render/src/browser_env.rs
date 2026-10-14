//! Generated browser binding adapter; source: scripts/browser-env-adapter.rs.in.
//! Canonical DOM + CSS grammar + JS: turbo-test @ 9f5a1f1.
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
