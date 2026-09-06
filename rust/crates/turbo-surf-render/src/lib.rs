//! JS-execution tier (tier 3): a `deno_core` V8 isolate with a real rtdom↔V8 DOM
//! binding ([`browser_env`], vendored from turbo-test) so page scripts hydrate
//! against a genuine `document`. [`runtime`] is the browser-environment runtime — it
//! grafts the binding onto deno_core's context, installs the non-DOM `window` globals
//! a real page needs (timers/fetch/URL/crypto/streams/…), and drives the event
//! loop + hydration pump + execution budget.

mod browser_env;
mod probe;
mod runtime;

pub use probe::{probe_globals, ProbeAccess, ProbeReport};
pub use runtime::{
    ensure_platform, eval_async, render_html, render_html_async, render_hydrate,
    render_hydrate_with_budget, render_page, render_page_pooled, render_page_with_budget,
    run_with_dom, set_fingerprint, set_measure_fn, PageSession, DEFAULT_RENDER_BUDGET_MS, SCRIPT_BOUNDARY,
};

/// A [`turbo_surf_core::challenge::PowEngine`] backed by the V8 render tier — runs a
/// challenge's own JS (against a real `document` + the controllable Chrome
/// navigator) and returns the answer it computes. This is what makes the
/// Cloudflare solver *proper*: execute the challenge instead of reversing its math.
pub struct V8PowEngine;

impl turbo_surf_core::challenge::PowEngine for V8PowEngine {
    fn compute(&self, script: &str) -> Result<String, String> {
        // The challenge JS runs against an empty document; it computes against the
        // navigator/window we expose. `run_with_dom` returns the trailing
        // expression — the wrapper script ends by reading the answer sink.
        run_with_dom("<html><body></body></html>", script)
    }
}
