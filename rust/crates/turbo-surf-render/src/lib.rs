//! JS-execution tier (tier 3): a `deno_core` V8 isolate with a real rtdom↔V8 DOM
//! binding ([`browser_env`], the `turbo-dom-browser-env` crate) so page scripts hydrate
//! against a genuine `document`. [`runtime`] is the browser-environment runtime — it
//! grafts the binding onto deno_core's context, installs the non-DOM `window` globals
//! a real page needs (timers/fetch/URL/crypto/streams/…), and drives the event
//! loop + hydration pump + execution budget.

// The binding is a dependency (not a module): surf code names its items as
// `crate::browser_env::*`; surf's native services over its documents
// (live_layout_dom, compression_dictionary_dom) use its `host_internals`.
use turbo_dom_browser_env::browser_env;

mod audio_backend;
mod audio_buffer;
mod audio_dsp;
mod audio_effects;
mod audio_fft;
mod audio_timeline;
mod compression_dictionary_dom;
mod context_ownership;
mod crypto_backend;
mod crypto_clone;
mod crypto_hybrid;
mod csp;
mod csp_reporting;
mod document_facts;
mod document_response;
mod fetch_cors;
mod geometry_backend;
mod integrity;
mod layout_profile;
mod live_layout;
mod live_layout_backend;
#[cfg(test)]
mod live_layout_budget_tests;
mod live_layout_composed;
mod live_layout_dom;
mod live_layout_media;
mod live_layout_neutral;
mod live_layout_resources;
mod live_layout_scoped;
mod live_layout_supports;
mod live_layout_zoom;
mod live_stylesheets;
mod message_ports;
mod permissions_policy;
mod probe;
mod referrer_meta;
mod resource_policy;
mod runtime;
mod script_fetch;
mod script_inventory;
mod storage_gc;
mod storage_inbox;
pub use storage_inbox::StorageEventSignal;
mod stream_transfers;
mod web_streams_backend;
mod window_named_properties;

pub use document_response::DocumentResponseMetadata;
pub use probe::{probe_globals, probe_page_async, ProbeAccess, ProbeReport};
pub use runtime::{
    ensure_platform, eval_async, render_capture_cookies_with_browsing_state,
    render_capture_cookies_with_document_response, render_capture_cookies_with_request_state,
    render_capture_cookies_with_response_headers, render_html, render_html_async, render_hydrate,
    render_hydrate_with_budget, render_page, render_page_pooled, render_page_with_budget,
    run_with_dom, set_fingerprint, set_measure_fn, set_raster_fn, set_raster_rgba_fn, set_webgl_fn,
    set_webgl_metadata_fn, BrowserStorage, HydrationDiagnostics, HydrationPhase, HydrationProgress,
    NavigationActivationSnapshot, PageSession, PageSessionOptions, WindowTimerMode,
    DEFAULT_RENDER_BUDGET_MS, HUMAN_INPUT_JS, SCRIPT_BOUNDARY,
};

/// Actual native engine version, independently of navigator's browser identity.
pub fn v8_version() -> &'static str {
    v8::V8::get_version()
}

/// Whether genuine native access checks and stable navigation proxies are compiled.
pub const fn stable_window_proxy() -> bool {
    cfg!(turbo_surf_native_proxy)
}

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

/// A [`turbo_surf_core::challenge::RecaptchaEngine`] backed by the V8 render tier — it
/// runs the page's OWN reCAPTCHA integration in the isolate. The page's `api.js` main
/// VM executes, we drive `grecaptcha.execute(sitekey, {action})`, and the bframe
/// iframe + parent↔bframe postMessage handshake (network over `op_fetch`) resolves to
/// a client `g-recaptcha-response` token — which we read back out of a sink node.
///
/// This mints the token *client-side only*; server acceptance is Google-scored (see
/// [`turbo_surf_core::recaptcha`] for the honest v3/invisible-vs-v2-image boundary).
pub struct V8RecaptchaEngine;

// A hidden node the driver writes the token into — read back after serialization.
const RECAPTCHA_TOKEN_SINK: &str = "__ts_recaptcha_token";

impl V8RecaptchaEngine {
    // The driver appended to the page: waits for `grecaptcha`, runs `execute`, and
    // writes the resolved token (or the widget's response field, for invisible v2)
    // into the sink node. `sitekey`/`action` are inlined as JSON string literals.
    fn driver_markup(sitekey: &str, action: &str) -> String {
        let sk = serde_json::to_string(sitekey).unwrap_or_else(|_| "\"\"".to_string());
        let ac = serde_json::to_string(action).unwrap_or_else(|_| "\"\"".to_string());
        format!(
            r#"<div id="{sink}" hidden></div><script>
(function(){{
  var SK={sk}, AC={ac}, tries=0;
  function sink(t){{ try{{ var el=document.getElementById("{sink}"); if(el) el.textContent=t||""; }}catch(e){{}} }}
  function resp(){{ try{{ var el=document.getElementById("g-recaptcha-response")||document.querySelector('textarea[name="g-recaptcha-response"]'); return (el&&el.value)?el.value:""; }}catch(e){{ return ""; }} }}
  function drive(){{
    var g=window.grecaptcha&&(window.grecaptcha.enterprise||window.grecaptcha);
    if(!g||typeof g.execute!=="function"){{ if(tries++<50) setTimeout(drive,20); return; }}
    var ready=(typeof g.ready==="function")?new Promise(function(r){{ g.ready(r); }}):Promise.resolve();
    ready.then(function(){{
      var out; try{{ out=g.execute(SK, AC?{{action:AC}}:undefined); }}catch(e){{ out=null; }}
      if(out&&typeof out.then==="function"){{ out.then(function(t){{ sink(t||resp()); }}).catch(function(){{ sink(resp()); }}); }}
      else {{ var p=0; (function wait(){{ var t=resp(); if(t) sink(t); else if(p++<50) setTimeout(wait,20); else sink(""); }})(); }}
    }});
  }}
  drive();
}})();
</script>"#,
            sink = RECAPTCHA_TOKEN_SINK,
            sk = sk,
            ac = ac,
        )
    }

    // Read the token the driver stored in the sink node out of the serialized DOM.
    // A page that removed the sink yields an empty token.
    fn extract_token(html: &str) -> String {
        let token = || {
            let sink = html.find(RECAPTCHA_TOKEN_SINK)?;
            let start = sink + html[sink..].find('>')? + 1;
            let end = start + html[start..].find('<')?;
            Some(html[start..end].trim().to_string())
        };
        token().unwrap_or_default()
    }
}

#[async_trait::async_trait]
impl turbo_surf_core::challenge::RecaptchaEngine for V8RecaptchaEngine {
    async fn execute(
        &self,
        page_html: &str,
        page_url: &str,
        sitekey: &str,
        action: &str,
    ) -> Result<String, String> {
        // Append the sink + driver just before </body> (or at the end) so it runs after
        // the page's own reCAPTCHA scripts have defined `grecaptcha`.
        let inject = Self::driver_markup(sitekey, action);
        let doc = match page_html.rfind("</body>") {
            Some(pos) => format!("{}{}{}", &page_html[..pos], inject, &page_html[pos..]),
            None => format!("{page_html}{inject}"),
        };
        let url = page_url.to_string();
        // The deno_core `JsRuntime` future is `!Send`, but `RecaptchaEngine::execute`
        // (and the `ChallengeSolver` above it) must stay `Send`. So drive the render on
        // a dedicated thread with its own current-thread runtime and await the result
        // over a `Send` channel — the platform is parented on the stable keeper thread
        // (via `ensure_platform`) so this ephemeral thread only builds an isolate.
        let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
        std::thread::spawn(move || {
            ensure_platform();
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send(Err(format!("recaptcha render runtime: {e}")));
                    return;
                }
            };
            let out = rt.block_on(render_hydrate_with_budget(
                &doc,
                &url,
                "",
                "",
                DEFAULT_RENDER_BUDGET_MS,
            ));
            let _ = tx.send(out);
        });
        let out = rx
            .await
            .map_err(|_| "recaptcha render thread panicked".to_string())??;
        Ok(Self::extract_token(&out))
    }
}

#[cfg(test)]
mod recaptcha_token_tests {
    use super::{V8RecaptchaEngine, RECAPTCHA_TOKEN_SINK};

    #[test]
    fn the_token_is_the_trimmed_text_of_the_sink_node() {
        let html =
            format!("<p>x</p><div id=\"{RECAPTCHA_TOKEN_SINK}\" hidden> tok-1 </div></body>");
        assert_eq!(V8RecaptchaEngine::extract_token(&html), "tok-1");
        for html in [
            "<body>no sink</body>".to_string(),
            format!("<div id={RECAPTCHA_TOKEN_SINK}"),
            format!("<div id={RECAPTCHA_TOKEN_SINK}>dangling"),
        ] {
            assert_eq!(V8RecaptchaEngine::extract_token(&html), "", "{html}");
        }
    }
}
