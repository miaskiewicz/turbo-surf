//! Generated browser binding adapter; source: scripts/browser-env-adapter.rs.in.
//! Canonical DOM + CSS grammar + JS: turbo-test @ d3f44f0.
//! Rebuild only through scripts/vendor-browser-env.sh from committed upstream HEAD.
#![allow(unused)]
#![allow(clippy::all)]
include!("browser_env_upstream.rs");

/// Seed a fetched document using the canonical binding, then install surf additions.
pub fn install_html(scope: &mut v8::PinScope, html: &str) {
    install_with_html(scope, html);
    run_js(scope, TURBOSURF_SHIM);
}

/// turbo-surf DOM shims layered on top of the vendored `BOOTSTRAP`, for browser
/// APIs real page scripts need that the test-oriented binding omits. Kept here
/// (not in the vendored `browser_env.js`) so re-vendoring never clobbers them.
///
/// - `document.implementation.createHTMLDocument`: jQuery calls this at load to
///   feature-detect + to parse HTML in an isolated context. Without it jQuery
///   throws on init, which aborts every jQuery-dependent page script (e.g.
///   Wikipedia's Vector skin, which reparents the TOC + sets layout state).
const TURBOSURF_SHIM: &str = r#"
(function () {
  var d = globalThis.document;
  if (!d || d.implementation) return;
  d.implementation = {
    createHTMLDocument: function (title) {
      var html = d.createElement('html');
      var head = d.createElement('head');
      var body = d.createElement('body');
      html.appendChild(head);
      html.appendChild(body);
      if (title != null) {
        var t = d.createElement('title');
        t.appendChild(d.createTextNode(String(title)));
        head.appendChild(t);
      }
      return {
        documentElement: html,
        head: head,
        body: body,
        location: globalThis.location || { href: '' },
        implementation: d.implementation,
        createElement: function (n) { return d.createElement(n); },
        createElementNS: function (ns, n) { return d.createElementNS ? d.createElementNS(ns, n) : d.createElement(n); },
        createTextNode: function (v) { return d.createTextNode(v); },
        createDocumentFragment: function () { return d.createDocumentFragment(); },
        getElementById: function (id) { return html.querySelector ? html.querySelector('#' + id) : null; },
        querySelector: function (s) { return html.querySelector ? html.querySelector(s) : null; },
        querySelectorAll: function (s) { return html.querySelectorAll ? html.querySelectorAll(s) : []; }
      };
    }
  };
})();
"#;

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
