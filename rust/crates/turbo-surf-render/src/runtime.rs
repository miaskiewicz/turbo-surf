//! Render runtime (tier 3). Page JS runs on a `deno_core` V8 isolate. The DOM is
//! a real rtdom↔V8 binding ([`crate::browser_env`], vendored from turbo-test) so
//! jQuery / React / hand-rolled bundles see a genuine `document`/Element. deno_core
//! supplies the rest: the async event loop, `fetch`/cookies over the tier-1 net
//! stack (`#[op2]` ops below), virtual timers, and the runaway-execution budget.
//!
//! Flow per render: build the runtime → graft the DOM binding onto its context with
//! the fetched page parsed in ([`install_dom`]) → run the page script → drain the
//! event loop + virtual timers → serialize the hydrated tree back to HTML.
//!
//! The binding stores V8 `Global` handles in thread-locals; they MUST be cleared
//! (`browser_env::reset()`) while the isolate is still alive, before the runtime
//! drops — otherwise a later drop on a dead isolate crashes. Every entry point
//! resets on the way out.

use deno_core::{
    op2, resolve_import, v8, JsRuntime, ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse,
    ModuleLoader, ModuleResolveResponse, ModuleSource, ModuleSourceCode, ModuleSpecifier,
    ModuleType, OpState, ResolutionKind, RuntimeOptions,
};
use deno_error::JsErrorBox;
use std::cell::RefCell;
use std::rc::Rc;
use turbo_surf_core::cookies::CookieJar;
use turbo_surf_core::net::{fetch_html, FetchOptions};
use turbo_surf_core::url::resolve;

/// Loads ES modules (a `<script type="module">`'s `import` graph) over the host net
/// layer — the same path `op_fetch` uses — so a Next dev / turbopack build (served as
/// ES modules) hydrates. Carries the page base + shared cookie jar so module fetches
/// are same-origin + session-authenticated like page fetches.
struct NetModuleLoader {
    base: String,
    jar: Jar,
    ua: String,
}

impl ModuleLoader for NetModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: ResolutionKind,
    ) -> ModuleResolveResponse {
        // Resolve against the referrer; fall back to the page base for the entry module.
        let referrer = if referrer.is_empty() || referrer == "." {
            &self.base
        } else {
            referrer
        };
        resolve_import(specifier, referrer).map_err(|e| JsErrorBox::generic(e.to_string()))
    }

    fn load(
        &self,
        module_specifier: &ModuleSpecifier,
        _maybe_referrer: Option<&ModuleLoadReferrer>,
        _options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        let url = module_specifier.clone();
        let jar = self.jar.clone();
        let ua = self.ua.clone();
        ModuleLoadResponse::Async(Box::pin(async move {
            let mut local = CookieJar::from_storage_state(&jar.borrow().storage_state());
            let mut headers = std::collections::BTreeMap::new();
            if !ua.is_empty() {
                headers.insert("user-agent".to_string(), ua);
            }
            let opts = FetchOptions {
                headers,
                allow_non_html: true, // JS modules aren't HTML
                jar: Some(&mut local),
                ..Default::default()
            };
            let r = fetch_html(url.as_str(), opts)
                .await
                .map_err(|e| JsErrorBox::generic(format!("module fetch {url}: {e}")))?;
            *jar.borrow_mut() = local;
            Ok(ModuleSource::new(
                ModuleType::JavaScript,
                ModuleSourceCode::String(r.html.into()),
                &url,
                None,
            ))
        }))
    }
}

/// Page base URL (the `location.href`): the base for relative `fetch` and the
/// scope for the `document.cookie` bridge. Stored in op state.
struct Base(String);

/// Shared cookie jar backing `document.cookie` (and page `fetch`). Stored in op
/// state behind `Rc<RefCell<…>>` since ops borrow it across the isolate.
type Jar = Rc<RefCell<CookieJar>>;

/// Custom User-Agent for this page: drives `navigator.userAgent` and the page-fetch
/// `User-Agent` header. Empty = the engine default. Stored in op state.
struct Ua(String);

/// `fetch` result marshaled back to JS as a `Response`-like object.
#[derive(serde::Serialize)]
struct FetchOut {
    status: u16,
    ok: bool,
    body: String,
    content_type: String,
}

// `document.cookie` getter: cookies applicable to the page's base URL.
#[op2]
#[string]
fn op_cookie_get(state: &mut OpState) -> String {
    let base = state.borrow::<Base>().0.clone();
    state.borrow::<Jar>().borrow().cookie_header(&base, 0.0)
}

// The custom User-Agent (empty if none) — `navigator.userAgent` reads this.
#[op2]
#[string]
fn op_user_agent(state: &mut OpState) -> String {
    state.borrow::<Ua>().0.clone()
}

// Process-global fingerprint overrides (a JSON object). Read by ENV_BOOTSTRAP to
// override the default Chrome navigator fields at runtime. Empty `{}` = all
// defaults. Process-global (like the napi shared client) — one render process is
// effectively one session; set it before rendering.
static FINGERPRINT_OVERRIDES: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

/// Override the render-tier navigator fields at runtime with a JSON object, e.g.
/// `{"platform":"Win32","hardwareConcurrency":16,"languages":["en-GB","en"],
/// "screen":{"width":2560,"height":1440},"userAgent":"…"}`. Unset keys keep their
/// Chrome 153 defaults. Pass `"{}"` (or `""`) to reset to defaults.
pub fn set_fingerprint(overrides_json: &str) {
    if let Ok(mut g) = FINGERPRINT_OVERRIDES.write() {
        *g = overrides_json.to_string();
    }
}

// The fingerprint-override JSON (or "{}" when unset) — ENV_BOOTSTRAP merges it.
#[op2]
#[string]
fn op_fingerprint() -> String {
    FINGERPRINT_OVERRIDES
        .read()
        .ok()
        .map(|g| g.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "{}".to_string())
}

// Process-global text-measurement hook. The render crate has no font/layout stack, so the host
// (napi/mcp, which own turbo-surf-raster) injects a real advance-width measurer. ENV_BOOTSTRAP's
// offsetWidth/offsetHeight read it so a font-detection probe sees per-family metric differences a
// no-layout DOM otherwise can't produce. Same host-injection pattern as `set_fingerprint`.
type MeasureFn = Box<dyn Fn(&str, &str, f64) -> (f64, f64) + Send + Sync>;
static MEASURE_TEXT: std::sync::RwLock<Option<MeasureFn>> = std::sync::RwLock::new(None);

/// Install the host's text measurer: `(text, css_font_family, font_size_px) -> (width_px, height_px)`.
/// Injected once at startup by the crate that owns the font/layout engine (raster). Until set,
/// `offsetWidth`/`offsetHeight` report 0 (a standalone render isolate has no fonts).
pub fn set_measure_fn(f: MeasureFn) {
    if let Ok(mut g) = MEASURE_TEXT.write() {
        *g = Some(f);
    }
}

// Measure text advance for offsetWidth/offsetHeight. Returns JSON `[width,height]`, or `null` when
// no host measurer is installed. Never throws across the boundary.
#[op2]
#[string]
fn op_measure_text(#[string] text: &str, #[string] family: &str, size: f64) -> String {
    match MEASURE_TEXT
        .read()
        .ok()
        .and_then(|g| g.as_ref().map(|f| f(text, family, size)))
    {
        Some((w, h)) => format!("[{w},{h}]"),
        None => "null".to_string(),
    }
}

// Real monotonic high-resolution clock (sub-millisecond), for `performance.now()`. deno_core
// gives no wall-independent monotonic time to the isolate, so the JS clock fell back to
// `Date.now()` (1ms grid) + a synthetic creep — which flatlines under a tight read loop (a timing
// tell: a real browser's performance.now advances on a ~sub-µs hardware clock). This returns ms
// since a process-fixed `Instant`, so consecutive reads show real sub-ms deltas like Chrome.
#[op2(fast)]
fn op_now_perf() -> f64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

// Host canvas rasterizer: replay a 2D draw list into real PNG bytes. Same injection
// pattern as `set_measure_fn` — the crate that owns the raster/paint engine installs it,
// so the render crate keeps no tiny-skia/font dep (matters for the PyO3 wheel build). Until
// set, `toDataURL` keeps the vendored synthetic behavior.
type RasterFn = Box<dyn Fn(u32, u32, &str) -> Option<Vec<u8>> + Send + Sync>;
static RASTER_PNG: std::sync::RwLock<Option<RasterFn>> = std::sync::RwLock::new(None);

/// Install the host's canvas rasterizer: `(width, height, ops_json) -> PNG bytes`.
/// `ops_json` is the recorded 2D display list (the vendored `ctx._ops`). Injected once at
/// startup by the crate that owns the raster engine (raster). Until set, `toDataURL` returns
/// the vendored synthetic blob.
pub fn set_raster_fn(f: RasterFn) {
    if let Ok(mut g) = RASTER_PNG.write() {
        *g = Some(f);
    }
}

// Rasterize a canvas draw list to a base64 PNG (no `data:` prefix), or "" when no host
// rasterizer is installed (the JS override then falls back to the vendored blob). Never
// throws across the boundary.
#[op2]
#[string]
fn op_raster_png(width: u32, height: u32, #[string] ops_json: &str) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    match RASTER_PNG
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(|f| f(width, height, ops_json)))
    {
        Some(bytes) => STANDARD.encode(bytes),
        None => String::new(),
    }
}

// Same rasterizer, RAW straight-alpha RGBA output (not PNG) — for getImageData, which returns
// pixels with no encoder in the loop. So `fillRect(red); getImageData(...)` reads back the actual
// rendered red (the vendored synthetic getImageData returned unrelated bytes — a broken-canvas
// tell). Raw pixels also mean solids/shapes match a real browser exactly (no PNG-encoder variance).
static RASTER_RGBA: std::sync::RwLock<Option<RasterFn>> = std::sync::RwLock::new(None);

/// Install the host's raw-RGBA canvas rasterizer: `(width, height, ops_json) -> RGBA8 bytes`
/// (straight alpha, top-left origin, tightly packed). Until set, getImageData keeps the vendored
/// behavior.
pub fn set_raster_rgba_fn(f: RasterFn) {
    if let Ok(mut g) = RASTER_RGBA.write() {
        *g = Some(f);
    }
}

#[op2]
#[string]
fn op_raster_rgba(width: u32, height: u32, #[string] ops_json: &str) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    match RASTER_RGBA
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(|f| f(width, height, ops_json)))
    {
        Some(bytes) => STANDARD.encode(bytes),
        None => String::new(),
    }
}

// Host WebGL executor: run a recorded WebGL call batch on the real GPU and return the
// framebuffer RGBA. Same injection pattern as the rasterizer; installed by the crate that owns
// the wgpu backend (raster, `gpu-metal`). Until set, WebGL readback keeps the synthetic stub.
type WebglFn = Box<dyn Fn(u32, u32, &str) -> Option<Vec<u8>> + Send + Sync>;
static WEBGL_EXEC: std::sync::RwLock<Option<WebglFn>> = std::sync::RwLock::new(None);

/// Install the host's WebGL→GPU executor: `(width, height, calls_json) -> RGBA8 bytes`.
/// `calls_json` is the batch the render tier records live off a page's `gl.*` calls (with real
/// buffer bytes + shader source). Until set, the isolate's WebGL `readPixels` stays synthetic.
pub fn set_webgl_fn(f: WebglFn) {
    if let Ok(mut g) = WEBGL_EXEC.write() {
        *g = Some(f);
    }
}

// Execute a recorded WebGL batch → base64 RGBA8 (full framebuffer, top-left origin), or "" when
// no host executor is installed (the JS override then keeps its synthetic readback). The JS side
// slices the caller's requested rect out of the returned framebuffer. Never throws.
#[op2]
#[string]
fn op_webgl_readback(width: u32, height: u32, #[string] calls_json: &str) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    match WEBGL_EXEC
        .read()
        .ok()
        .and_then(|g| g.as_ref().and_then(|f| f(width, height, calls_json)))
    {
        Some(bytes) => STANDARD.encode(bytes),
        None => String::new(),
    }
}

// Whether a host WebGL executor is installed. The op itself is always registered, so the JS
// recorder can't infer presence from `typeof op`; it checks this so it only overrides the WebGL
// context when a GPU backend exists (else the vendored synthetic context is left untouched).
#[op2(fast)]
fn op_webgl_available() -> bool {
    WEBGL_EXEC.read().map(|g| g.is_some()).unwrap_or(false)
}

// `document.cookie` setter: ingest a `name=value; attrs` line against the base.
#[op2(fast)]
fn op_cookie_set(state: &mut OpState, #[string] line: &str) {
    let base = state.borrow::<Base>().0.clone();
    state
        .borrow::<Jar>()
        .borrow_mut()
        .set_from_response(&base, &[line.to_string()], 0.0);
}

// `fetch(url)` over the tier-1 net stack. Relative URLs resolve against the
// page base. Never throws across the boundary: a transport/parse failure comes
// back as `{ status: 0, ok: false }` so page code sees a real (failed) Response.
#[op2]
#[serde]
async fn op_fetch(
    state: Rc<RefCell<OpState>>,
    #[string] url: String,
    #[string] init_json: String,
) -> FetchOut {
    let (base, jar_rc, ua) = {
        let s = state.borrow();
        (
            s.borrow::<Base>().0.clone(),
            s.borrow::<Jar>().clone(),
            s.borrow::<Ua>().0.clone(),
        )
    };
    let target = resolve(&base, &url).unwrap_or(url);
    // Honor the `fetch(url, init)` request: method, headers, body. Without this every
    // page fetch was a GET with no body — a login POST (PropelAuth) 404'd.
    let init: deno_core::serde_json::Value =
        deno_core::serde_json::from_str(&init_json).unwrap_or(deno_core::serde_json::Value::Null);
    let method = init
        .get("method")
        .and_then(|m| m.as_str())
        .map(|m| m.to_ascii_uppercase());
    let body = init
        .get("body")
        .and_then(|b| b.as_str())
        .map(|b| b.to_string());
    let mut headers: std::collections::BTreeMap<String, String> = init
        .get("headers")
        .and_then(|h| deno_core::serde_json::from_value(h.clone()).ok())
        .unwrap_or_default();
    // Browser-set request headers a fetch carries automatically (an auth backend gates
    // on Origin; a cross-origin POST without it is rejected). Derive from the page base
    // (scheme://host[:port], i.e. base up to the third '/').
    if let Some(origin) = page_origin(&base) {
        headers.entry("Origin".to_string()).or_insert(origin);
        headers
            .entry("Referer".to_string())
            .or_insert_with(|| base.clone());
    }
    // Custom User-Agent (if set) overrides the net default for page fetches.
    if !ua.is_empty() {
        headers.insert("user-agent".to_string(), ua);
    }
    // Carry the page's cookies on same-origin fetches and ingest Set-Cookie back, so
    // session-authenticated hydration works (e.g. an auth SDK fetching the current user
    // with the session cookie). Snapshot the shared jar into a local one for the call —
    // a RefCell borrow can't be held across the await.
    let mut local = CookieJar::from_storage_state(&jar_rc.borrow().storage_state());
    let opts = FetchOptions {
        method,
        body,
        headers,
        allow_non_html: true, // fetch pulls JSON/text too
        // Follow redirects MANUALLY so `Set-Cookie` on an intermediate 3xx hop is
        // ingested (the browser-equivalent per-hop cookie round-trip). Auto-follow
        // only ingests the final response's cookies — a login/consent endpoint that
        // mints its session cookie on a 302 (Google's `/save` → `NID`) would be lost.
        max_redirects: Some(20),
        jar: Some(&mut local),
        ..Default::default()
    };
    let out = match fetch_html(&target, opts).await {
        Ok(r) => FetchOut {
            status: r.status,
            ok: (200..300).contains(&r.status),
            body: r.html,
            content_type: r.content_type,
        },
        Err(_) => FetchOut {
            status: 0,
            ok: false,
            body: String::new(),
            content_type: String::new(),
        },
    };
    *jar_rc.borrow_mut() = local; // persist any Set-Cookie updates for later fetches
    out
}

// `scheme://host[:port]` of an absolute http(s) URL — the part before the path. Used to
// synthesize the `Origin` header a browser fetch would send.
fn page_origin(base: &str) -> Option<String> {
    let scheme_end = base.find("://")?;
    let after = scheme_end + 3;
    let host_len = base[after..].find('/').unwrap_or(base.len() - after);
    let origin = &base[..after + host_len];
    (base.starts_with("http://") || base.starts_with("https://")).then(|| origin.to_string())
}

deno_core::extension!(
    turbo_dom,
    ops = [
        op_cookie_get,
        op_cookie_set,
        op_fetch,
        op_user_agent,
        op_fingerprint,
        op_now_perf,
        op_measure_text,
        op_raster_png,
        op_raster_rgba,
        op_webgl_readback,
        op_webgl_available
    ],
);

// Non-DOM browser globals, layered over the ops AFTER the native DOM binding is
// installed (`browser_env` owns document/Element/window/navigator/Event/etc.; this
// adds what a network-free test env lacks and overrides a few brand/host values).
// Virtual timers are queued and drained synchronously by `__runTimers`, ordered by
// delay — no wall-clock waits. `fetch`/XHR go over the tier-1 net stack.
//
// Wrapped in an IIFE so it is RE-RUNNABLE on a reused isolate: a persistent
// runtime (see `run_with_dom`) re-installs the page per call, which re-runs this;
// top-level `const`/`let` would throw "already declared" the second time, but
// inside the IIFE they're per-invocation. Globals are assigned to `globalThis`
// (idempotent) and the cookie bridge re-applies to the current `document`.
const ENV_BOOTSTRAP: &str = r##"(() => {
const ops = Deno.core.ops;
globalThis.self = globalThis;
// Present a real Chrome (macOS) navigator so page JS that profiles the browser
// (consistency-only anti-bot gates, feature detection) sees Chrome, not the old
// `turbo-surf`/`turbo-test` tell. Kept in sync with the tier-1 HTTP UA in
// turbo-surf-core (fingerprint::default_profile): same Chrome major + macOS, so navigator
// and the request headers agree (a UA/platform mismatch is itself a bot signal).
// This is no-Chromium env emulation — it satisfies passive/consistency probes,
// not active canvas/WebGL/audio draw-and-hash or PoW challenges.
// `onLine: true` matters: auth SDKs (PropelAuth) only auto-refresh the session from the
// cookie when the browser reports online — an undefined/falsy onLine made a cold load of
// an authed page skip the refresh and render nothing.
// Runtime fingerprint overrides (JSON object from op_fingerprint). Every navigator
// field below has a Chrome 153 default and is overridable by the matching key —
// settable per process via `set_fingerprint` (MCP `set_fingerprint` tool).
const __fp = (() => { try { return JSON.parse(Deno.core.ops.op_fingerprint()); } catch (e) { return {}; } })();
const __pick = (k, d) => (__fp[k] !== undefined ? __fp[k] : d);
const __ua = __pick("userAgent",
  (Deno.core.ops.op_user_agent && Deno.core.ops.op_user_agent()) ||
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/153.0.0.0 Safari/537.36");
const __major = String(__pick("chromeMajor", 153));
// Chrome ships exactly these five PDF-viewer plugins, all aliased to the internal
// viewer; `navigator.plugins.length === 0` is a classic headless giveaway.
const __plugin = (name) => ({ name, filename: "internal-pdf-viewer", description: "Portable Document Format", length: 1 });
const __plugins = ["PDF Viewer", "Chrome PDF Viewer", "Chromium PDF Viewer", "Microsoft Edge PDF Viewer", "WebKit built-in PDF"].map(__plugin);
// Chrome's two PDF MIME types, both bound to the internal viewer. `navigator.mimeTypes`
// empty while `plugins` is populated is an INCOHERENCE tell (real Chrome: plugins imply
// their mimeTypes) — google's homepage reads mimeTypes.length. enabledPlugin cross-links
// back to a plugin, as in a real MimeType.
const __mimeTypes = [
  { type: "application/pdf", suffixes: "pdf", description: "Portable Document Format", enabledPlugin: __plugins[0] },
  { type: "text/pdf", suffixes: "pdf", description: "Portable Document Format", enabledPlugin: __plugins[0] },
];
const __langs = __pick("languages", ["en-US", "en"]);
const __platform = __pick("platform", "MacIntel");
const __uaPlatform = __pick("uaPlatform", "macOS");
// Mac wide-gamut/HDR displays report 30-bit color; other platforms 24. Param-driven with a
// platform-aware default so a caller can override, and a Windows/Linux profile stays honest.
const __isMac = __uaPlatform === "macOS" || /mac/i.test(String(__platform));
const __colorDepth = __pick("colorDepth", __isMac ? 30 : 24);
const __pixelDepth = __pick("pixelDepth", __colorDepth);
// The navigator's data + method IMPLEMENTATIONS. Assembled here, then hung off a real
// `Navigator.prototype` (below) so `navigator` is a prototype-backed instance with ZERO own
// properties — exactly like Chrome (a plain object literal had 26 own props + a 15-member proto,
// vs Chrome's 0 own + 84-member Navigator.prototype: a structural tell).
const __navData = {
  userAgent: __ua,
  appVersion: __ua.replace(/^Mozilla\//, ""),
  appName: "Netscape", appCodeName: "Mozilla", product: "Gecko", productSub: "20030107",
  platform: __platform, vendor: __pick("vendor", "Google Inc."), vendorSub: "",
  language: __langs[0] || "en-US", languages: __langs, onLine: true,
  // Automation tell: real Chrome exposes this as `false`, never `true`/undefined.
  webdriver: false,
  hardwareConcurrency: __pick("hardwareConcurrency", 8),
  deviceMemory: __pick("deviceMemory", 8),
  maxTouchPoints: __pick("maxTouchPoints", 0),
  cookieEnabled: true, doNotTrack: null,
  // Fire-and-forget telemetry beacon: real Chrome exposes it, and its absence is a
  // headless tell (google's homepage reads `navigator.sendBeacon` before hydrating).
  // Anti-bot collectors (Botguard/reCAPTCHA-class) and analytics POST their COLLECTED
  // TOKEN back over sendBeacon — so a no-op that merely returns `true` fingerprints
  // fine yet never completes the exchange (the token is dropped on the floor). Actually
  // send it: a fire-and-forget POST over the tier-1 net stack (`globalThis.fetch` →
  // `op_fetch`), which shares the page cookie jar and is counted in `__pendingFetches`
  // so the hydration drain waits for it to land before serializing. Returns `true`
  // synchronously, per the spec (the boolean reports whether the UA QUEUED the transfer,
  // not whether it succeeded). `globalThis.fetch`/Blob/etc. are defined later in this
  // bootstrap, but this runs only at call time, by when they exist.
  sendBeacon: (url, data) => {
    try {
      if (url == null || url === "") return false;
      // Normalize the payload + its implicit content-type the way the spec's
      // `extractBody` does for the common beacon body types.
      let body = null;
      let contentType = "";
      if (data != null) {
        const B = globalThis.Blob, USP = globalThis.URLSearchParams, FD = globalThis.FormData;
        if (typeof data === "string") { body = data; contentType = "text/plain;charset=UTF-8"; }
        else if (B && data instanceof B) { body = data._s != null ? String(data._s) : ""; contentType = data.type || ""; }
        else if (USP && data instanceof USP) { body = data.toString(); contentType = "application/x-www-form-urlencoded;charset=UTF-8"; }
        else if (FD && data instanceof FD) {
          // No multipart encoder headless; serialize the fields url-encoded (Blob/File
          // parts become their string content) — enough for a token round-trip.
          const p = new USP();
          data.forEach((v, k) => p.append(k, typeof v === "string" ? v : (v && v._s != null ? String(v._s) : "")));
          body = p.toString(); contentType = "application/x-www-form-urlencoded;charset=UTF-8";
        }
        else if (data instanceof ArrayBuffer || (data && data.buffer instanceof ArrayBuffer)) {
          try { body = new globalThis.TextDecoder().decode(data instanceof ArrayBuffer ? new Uint8Array(data) : data); }
          catch (_e) { body = ""; }
        }
        else { try { body = String(data); } catch (_e) { body = ""; } }
      }
      const headers = contentType ? { "content-type": contentType } : {};
      // Direct (not deferred) call so `fetch` bumps `__pendingFetches` synchronously and
      // the drain can't quiesce before the beacon completes. `keepalive` mirrors the real
      // beacon flag (informational here).
      try { globalThis.fetch(String(url), { method: "POST", body, headers, keepalive: true }); } catch (_e) {}
      return true;
    } catch (_e) { return true; }
  },
  plugins: __plugins, mimeTypes: __mimeTypes,
  // Real Chrome exposes navigator.pdfViewerEnabled === true (it ships the internal PDF
  // viewer); its absence is a headless tell google's homepage reads.
  pdfViewerEnabled: __pick("pdfViewerEnabled", true),
  // NetworkInformation — real Chrome exposes it; anti-bot scripts (found via the
  // `probe` example on a real Akamai sensor) read it, and its absence is a tell.
  connection: __pick("connection", { effectiveType: "4g", rtt: 50, downlink: 10, saveData: false }),
  // UA-Client-Hints high-entropy surface, consistent with the UA above.
  userAgentData: __pick("userAgentData", {
    // Order + greased token MUST match the on-wire sec-ch-ua (fingerprint.rs) — a wire/JS
    // mismatch is a hard tell. Both are validated against a live real-Chrome capture:
    // `"Chromium";v="M", "Google Chrome";v="M", "Not A(Brand";v="99"` (greased brand LAST,
    // token `Not A(Brand` v99). The old `Not_A Brand v8` greased-middle matched nothing real.
    brands: [
      { brand: "Chromium", version: __major },
      { brand: "Google Chrome", version: __major },
      { brand: "Not A(Brand", version: "99" },
    ],
    mobile: false,
    platform: __uaPlatform,
    getHighEntropyValues: async () => ({
      architecture: "arm", bitness: "64", model: "", wow64: false,
      platform: __uaPlatform, platformVersion: "15.0.0", uaFullVersion: __major + ".0.0.0",
      mobile: false,
      brands: [
        { brand: "Chromium", version: __major },
        { brand: "Google Chrome", version: __major },
        { brand: "Not A(Brand", version: "99" },
      ],
      fullVersionList: [
        { brand: "Chromium", version: __major + ".0.0.0" },
        { brand: "Google Chrome", version: __major + ".0.0.0" },
        { brand: "Not A(Brand", version: "99.0.0.0" },
      ],
    }),
  }),
  // In-memory clipboard: an app that writeText()s a value (e.g. a copy-link button)
  // and reads it back round-trips, with no OS clipboard.
  clipboard: (() => { let v = ""; return { writeText: async (t) => { v = String(t == null ? "" : t); }, readText: async () => v }; })(),
  // Permissions API — real Chrome always exposes `navigator.permissions.query`; its
  // absence (a thrown TypeError on `navigator.permissions.query`) is a headless tell
  // BotGuard/reCAPTCHA-class collectors read directly. Return a spec-shaped
  // PermissionStatus (an EventTarget-ish with `state`/`name`/`onchange`) with the
  // default states a fresh Chrome profile reports: most permissions 'prompt', and the
  // headless-detector special case `notifications` → 'denied' iff Notification.permission
  // is 'denied' (real Chrome couples them; a mismatch is itself a tell).
  permissions: (() => {
    const status = (name, state) => ({
      name, state, onchange: null,
      addEventListener() {}, removeEventListener() {}, dispatchEvent() { return false; },
    });
    const DEFAULTS = { geolocation: "prompt", notifications: "prompt", push: "prompt",
      "persistent-storage": "prompt", "background-sync": "granted", midi: "granted",
      camera: "prompt", microphone: "prompt", "clipboard-read": "prompt", "clipboard-write": "granted" };
    return {
      query: (desc) => {
        const name = desc && desc.name ? String(desc.name) : "";
        let state = Object.prototype.hasOwnProperty.call(DEFAULTS, name) ? DEFAULTS[name] : "prompt";
        // Couple notifications to Notification.permission the way Chrome does.
        try {
          if (name === "notifications" && globalThis.Notification && globalThis.Notification.permission) {
            state = globalThis.Notification.permission === "denied" ? "denied"
              : globalThis.Notification.permission === "granted" ? "granted" : "prompt";
          }
        } catch (e) {}
        return Promise.resolve(status(name, state));
      },
    };
  })(),
};
// Build a real `Navigator` interface: `window.Navigator` + a prototype holding all 84 members
// Chrome exposes (getters for data props, functions for methods), then a zero-own-property
// instance. Members we implement use __navData; the rest get plausible stubs so presence/`in`/
// typeof checks + the member count match Chrome. Getters/methods are arrows (no own `prototype`)
// and get native-masked in the toString IIFE. The exact 84-name list is from a live Chrome 154.
(() => {
  const GET = ("appCodeName appName appVersion bluetooth clipboard connection cookieEnabled cpuPerformance " +
    "credentials deprecatedRunAdAuctionEnforcesKAnonymity deviceMemory devicePosture doNotTrack geolocation gpu " +
    "hardwareConcurrency hid ink keyboard language languages locks login managed maxTouchPoints mediaCapabilities " +
    "mediaDevices mediaSession mimeTypes onLine pdfViewerEnabled permissions platform plugins presentation product " +
    "productSub protectedAudience scheduling serial serviceWorker storage storageBuckets usb userActivation " +
    "userAgent userAgentData vendor vendorSub virtualKeyboard wakeLock webdriver webkitPersistentStorage " +
    "webkitTemporaryStorage windowControlsOverlay xr").split(/\s+/);
  const FN = ("adAuctionComponents canLoadAdAuctionFencedFrame canShare clearAppBadge clearOriginJoinedAdInterestGroups " +
    "createAuctionNonce deprecatedReplaceInURN deprecatedURNToURL getBattery getGamepads getInstalledRelatedApps " +
    "getInterestGroupAdAuctionData getUserMedia javaEnabled joinAdInterestGroup leaveAdInterestGroup " +
    "registerProtocolHandler requestMIDIAccess requestMediaKeySystemAccess runAdAuction sendBeacon setAppBadge share " +
    "unregisterProtocolHandler updateAdInterestGroups vibrate webkitGetUserMedia").split(/\s+/);
  // Plausible stubs for members we don't back with real data (presence/shape checks pass).
  const P = () => Promise.resolve();
  const REJ = () => Promise.reject(new DOMException("Not supported", "NotSupportedError"));
  const GET_STUB = {
    geolocation: { getCurrentPosition() {}, watchPosition() { return 0; }, clearWatch() {} },
    mediaDevices: { enumerateDevices: () => Promise.resolve([]), getUserMedia: REJ, getSupportedConstraints: () => ({}), addEventListener() {}, removeEventListener() {} },
    serviceWorker: { register: REJ, getRegistration: () => Promise.resolve(undefined), getRegistrations: () => Promise.resolve([]), ready: new Promise(() => {}), controller: null, addEventListener() {}, removeEventListener() {} },
    storage: { estimate: () => Promise.resolve({ quota: 0, usage: 0 }), persisted: () => Promise.resolve(false), persist: () => Promise.resolve(false) },
    credentials: { get: () => Promise.resolve(null), create: () => Promise.resolve(null), store: P, preventSilentAccess: P },
    userActivation: { hasBeenActive: true, isActive: false },
    wakeLock: { request: REJ },
    keyboard: { getLayoutMap: () => Promise.resolve(new Map()), lock: P, unlock() {} },
    locks: { request: P, query: () => Promise.resolve({ held: [], pending: [] }) },
    mediaCapabilities: { decodingInfo: () => Promise.resolve({ supported: true, smooth: true, powerEfficient: true }), encodingInfo: () => Promise.resolve({ supported: true, smooth: true, powerEfficient: true }) },
    mediaSession: { metadata: null, playbackState: "none", setActionHandler() {}, setPositionState() {} },
    // React's scheduler probes navigator.scheduling.isInputPending — must be an object with the fn.
    scheduling: { isInputPending: () => false },
    presentation: { defaultRequest: null, receiver: null },
    webkitTemporaryStorage: { queryUsageAndQuota() {}, requestQuota() {} },
    webkitPersistentStorage: { queryUsageAndQuota() {}, requestQuota() {} },
  };
  const navProto = {};
  const def = (name, desc) => { try { Object.defineProperty(navProto, name, desc); } catch (e) {} };
  for (const name of GET) {
    const has = Object.prototype.hasOwnProperty.call(__navData, name);
    // Default an unlisted interface getter to a fresh {} (not null): real Chrome returns an object
    // for these, and `navigator.X.foo` must read `undefined` rather than throw on null. A fresh
    // object per property (not one shared ref) so `navigator.a !== navigator.b`.
    const val = has ? __navData[name] : (Object.prototype.hasOwnProperty.call(GET_STUB, name) ? GET_STUB[name] : {});
    def(name, { get: () => val, enumerable: true, configurable: true });
  }
  const FN_IMPL = {
    javaEnabled: () => false,
    vibrate: () => true,
    canShare: () => false,
    share: REJ,
    getBattery: () => Promise.resolve({ charging: true, chargingTime: 0, dischargingTime: Infinity, level: 1, addEventListener() {}, removeEventListener() {} }),
    getGamepads: () => [null, null, null, null],
    getInstalledRelatedApps: () => Promise.resolve([]),
    requestMIDIAccess: REJ,
    requestMediaKeySystemAccess: REJ,
    getUserMedia: (_c, _ok, err) => { try { if (err) err(new DOMException("Permission denied", "NotAllowedError")); } catch (e) {} },
    webkitGetUserMedia: (_c, _ok, err) => { try { if (err) err(new DOMException("Permission denied", "NotAllowedError")); } catch (e) {} },
    setAppBadge: P, clearAppBadge: P,
    registerProtocolHandler: () => {}, unregisterProtocolHandler: () => {},
  };
  for (const name of FN) {
    const impl = Object.prototype.hasOwnProperty.call(__navData, name) && typeof __navData[name] === "function"
      ? __navData[name]
      : (FN_IMPL[name] || (() => undefined));
    def(name, { value: impl, writable: true, enumerable: true, configurable: true });
  }
  function Navigator() { throw new TypeError("Illegal constructor"); }
  Navigator.prototype = navProto;
  def("constructor", { value: Navigator, writable: true, configurable: true });
  Object.defineProperty(navProto, Symbol.toStringTag, { value: "Navigator", configurable: true });
  globalThis.Navigator = Navigator;
  const navigator = Object.create(navProto); // zero own properties, like Chrome
  globalThis.navigator = navigator;
})();
// `screen` — overridable as a unit; defaults to a common 1080p desktop.
{
  const __scr = __pick("screen", { width: 1920, height: 1080 });
  const __w = __scr.width || 1920, __h = __scr.height || 1080;
  // availWidth/Height default to the screen minus OS chrome (macOS menubar ≈ 25px tall, no
  // side reservation); each is independently overridable via the `screen` object or __pick.
  const __availW = __pick("availWidth", __scr.availWidth || __w);
  const __availH = __pick("availHeight", __scr.availHeight || (__h - (__isMac ? 25 : 0)));
  globalThis.screen = {
    width: __w, height: __h, availWidth: __availW, availHeight: __availH,
    colorDepth: __colorDepth, pixelDepth: __pixelDepth,
    // ScreenOrientation is an EventTarget — apps listen for orientation changes;
    // a missing `addEventListener` throws and can trip a component during hydration.
    orientation: {
      type: "landscape-primary", angle: 0,
      addEventListener() {}, removeEventListener() {}, dispatchEvent() { return false; },
      onchange: null,
    },
  };
  globalThis.devicePixelRatio = __pick("devicePixelRatio", 2);
}
// document.fonts (FontFaceSet) — an EventTarget with a `ready` promise; web-font
// loaders (`document.fonts.ready`, `.addEventListener('loadingdone')`) touch it, and
// a missing one throws mid-hydration. No real font pipeline here, so `ready` resolves
// immediately and load/check report success/emptiness.
if (globalThis.document && !globalThis.document.fonts) {
  try {
    const __fonts = {
      ready: Promise.resolve(), status: "loaded", size: 0,
      add() {}, delete() { return false; }, clear() {}, has() { return false; },
      check() { return true; }, load() { return Promise.resolve([]); },
      forEach() {}, values() { return [][Symbol.iterator](); }, keys() { return [][Symbol.iterator](); },
      [Symbol.iterator]() { return [][Symbol.iterator](); },
      addEventListener() {}, removeEventListener() {}, dispatchEvent() { return false; },
      onloading: null, onloadingdone: null, onloadingerror: null,
    };
    Object.defineProperty(globalThis.document, "fonts", { configurable: true, get() { return __fonts; } });
  } catch (e) {}
}
// `window.chrome` presence (with loadTimes/csi/app, but no extension `runtime`) is
// what a plain Chrome page exposes; its absence flags a non-Chrome/headless client.
globalThis.chrome = globalThis.chrome || {
  app: { isInstalled: false },
  loadTimes: function () { return {}; },
  csi: function () { return {}; },
};
globalThis.location = globalThis.location || { href: "about:blank", protocol: "about:", host: "", pathname: "blank" };
const __mkStorage = () => {
  const m = new Map();
  return {
    getItem: (k) => (m.has(k) ? m.get(k) : null),
    setItem: (k, v) => m.set(k, String(v)),
    removeItem: (k) => m.delete(k),
    clear: () => m.clear(),
    key: (i) => (Array.from(m.keys())[i] ?? null),
    get length() { return m.size; },
  };
};
globalThis.localStorage = __mkStorage();
// sessionStorage: a real Chrome global (per-tab web storage). deno_core ships neither;
// google's homepage reads window.sessionStorage, so its absence is a consistency tell.
globalThis.sessionStorage = __mkStorage();
const __log = (...a) => Deno.core.print(a.map(String).join(" ") + "\n");
globalThis.console = { log: __log, info: __log, warn: __log, error: __log, debug: () => {} };
const __timers = [];
let __tid = 1;
// Virtual clock (ms). A timer's `due` is `__now + delay` at schedule time; the drain
// advances `__now` to each fired timer's `due`. This makes the env behave like
// wall-clock for SELF-RESCHEDULING timers: a `setTimeout(poll, 1000)` that reschedules
// itself fires at virtual 1000, 2000, 3000… so over the virtual budget it fires a
// browser-like number of times (~tens), not thousands. Previously `delay` was only a
// sort key, so a polling loop (analytics SDKs like PostHog do this) fired until the raw
// count cap — spinning the entire render budget and starving the real commit. Delay-0
// work (microtasks / the React scheduler) still drains promptly (it never advances the
// clock); only delayed polls are time-gated.
let __now = 0;
// Virtual-time ceiling, RELATIVE to the start of the current pump/drain (`__budgetBase`).
// Once the clock passes base+budget, delayed timers stop firing so a never-idle poll can't
// hold a drain open. RELATIVE (not absolute) is essential: `__now` accumulates across a
// long session (each modal transition / poll advances it), so an absolute ceiling would,
// late in a flow, refuse to fire even a brand-new short timer — e.g. a closing MUI modal's
// 195ms Fade-exit timer never fires, so the modal never unmounts and `waitFor(hidden)` /
// subsequent `[role=dialog].first()` break. Resetting the base per drain gives every
// interaction a fresh window so its transitions complete, while still capping runaway polls.
const __VIRTUAL_BUDGET_MS = 15000;
let __budgetBase = 0;
globalThis.__resetTimerBudget = () => { __budgetBase = __now; };
globalThis.setTimeout = (fn, delay = 0, ...args) => {
  __timers.push({ id: __tid, fn, due: __now + (+delay || 0), args });
  return __tid++;
};
globalThis.setInterval = globalThis.setTimeout; // one-shot here (no event loop)
globalThis.clearTimeout = (id) => {
  const i = __timers.findIndex((t) => t.id === id);
  if (i >= 0) __timers.splice(i, 1);
};
globalThis.clearInterval = globalThis.clearTimeout;
// High-resolution monotonic clock matching Chrome (origin-relative, fractional, 100µs grid).
// A BotGuard-class collector reads performance.now ×100+ and rAF ×20: it checks the resolution
// (Chrome quantizes to 0.1ms), fractionality, monotonicity, that now() ≈ Date.now()-timeOrigin,
// and that rAF/event timestamps ride the SAME origin-relative scale. The old fallback returned
// integer epoch ms (wrong scale, 1ms grid, no fraction) — a timing tell. timeOrigin is a
// fractional epoch anchor ~0.8–2.5s in the past (a just-navigated page); now() quantizes wall
// elapsed to the 100µs grid, monotonic with a bounded creep so a tight loop shows 0.1ms deltas.
// The observable clock is a single coherent quantity: REAL monotonic elapsed (op_now_perf, sub-ms)
// + VIRTUAL elapsed (`__now`, advanced by the timer drain). This makes `setTimeout(100)`/rAF appear
// to consume ~100ms/16.7ms even though the drain compresses them to ~0 real time (a real browser
// observes the delay; the old clock, pure wall-clock, showed 0 — a hard tell), while a sync busy
// loop still consumes real time (real hrtime advances). `Date.now()` is redefined from the SAME
// quantity so `timeOrigin + performance.now() == Date.now()` holds (BotGuard's chronometric check).
const __origDateNow = Date.now.bind(Date);
const __hrNow = () => { try { return Deno.core.ops.op_now_perf(); } catch (e) { return 0; } };
const __hr0 = __hrNow();               // monotonic anchor at bootstrap
const __epoch0 = __origDateNow();      // wall epoch at bootstrap
const __pageAge = Math.random() * 1700 + 800 + Math.random(); // a page open ~0.8–2.5s (perf.now start)
const __perfTimeOrigin = __epoch0 - __pageAge;
// Total observed elapsed since timeOrigin: page age + real monotonic since boot + virtual timer time.
const __observed = () => __pageAge + (__hrNow() - __hr0) + __now;
let __perfLast = 0;
const __perfNow = () => {
  let t = Math.floor(__observed() * 10) / 10; // Chrome 100µs grid
  if (t < __perfLast) t = __perfLast;          // strictly monotonic
  __perfLast = t;
  return t;
};
// Redefine Date.now() coherently with performance.now (real epoch + real monotonic + virtual).
// The native epoch stays available to the host (cookies/TLS live in Rust, not this isolate).
const __coherentEpoch = () => Math.floor(__perfTimeOrigin + __observed());
try {
  Object.defineProperty(Date, "now", {
    value: () => __coherentEpoch(),
    configurable: true, writable: true,
  });
} catch (e) {}
// Coherence of the CONSTRUCTOR too: `new Date()` / `+new Date()` / `Date()` must read the SAME
// coherent clock as `Date.now()`, else they diverge by the accumulated virtual time after a timer
// drain (real Chrome keeps them equal — the delta is a tell). Only the zero-arg path is retimed;
// every other form (`new Date(ms)`, `new Date(y,m,d,…)`, `new Date(str)`) delegates unchanged.
// Wrap the real constructor, preserving statics, prototype identity, `instanceof`, the
// prototype.constructor back-link, and native fn shape (name/length/toString marked below).
try {
  const __RealDate = Date;
  function DateShim(...a) {
    if (!new.target) return new __RealDate(__coherentEpoch()).toString();
    // Reflect.construct with new.target so `class X extends Date {}` keeps its prototype chain
    // (`new X() instanceof X`); a plain `new __RealDate(...)` would hand back a bare Date instance
    // and break subclassing — itself a builtin-integrity tell. Zero-arg → the coherent epoch.
    return Reflect.construct(__RealDate, a.length ? a : [__coherentEpoch()], new.target);
  }
  // Real Date.prototype is non-writable; match that attribute (writable:true→false is allowed
  // even on the function's non-configurable `prototype` slot).
  Object.defineProperty(DateShim, "prototype", { value: __RealDate.prototype, writable: false });
  Object.defineProperty(__RealDate.prototype, "constructor", {
    value: DateShim, configurable: true, writable: true,
  });
  DateShim.now = __RealDate.now; // the coherent Date.now defined just above
  DateShim.parse = __RealDate.parse;
  DateShim.UTC = __RealDate.UTC;
  Object.defineProperty(DateShim, "name", { value: "Date", configurable: true });
  Object.defineProperty(DateShim, "length", { value: 7, configurable: true }); // real Date.length === 7
  globalThis.Date = DateShim;
} catch (e) {}
// rAF: BATCH like a real browser (measured against Chrome). All callbacks scheduled for a frame
// fire together with the SAME fractional, origin-relative DOMHighResTimeStamp (~16.6ms cadence +
// sub-ms jitter, never before __perfNow()); a callback that re-schedules runs on the NEXT frame.
// A per-callback-incrementing clock (the naive shim) is a tell — Chrome gives every rAF in one
// frame an identical timestamp. requestAnimationFrame returns an integer id; cancelAnimationFrame
// removes only that pending callback (not clearTimeout — rAF ids are their own namespace).
let __rafClock = null, __rafId = 0, __rafScheduled = false;
let __rafQueue = [];
globalThis.requestAnimationFrame = (fn) => {
  const id = ++__rafId;
  __rafQueue.push({ id, fn });
  if (!__rafScheduled) {
    __rafScheduled = true;
    globalThis.setTimeout(() => {
      const floor = __perfNow();
      __rafClock = __rafClock == null ? floor : __rafClock + 16.6 + (Math.random() * 0.8 - 0.4);
      if (__rafClock < floor) __rafClock = floor;
      const ts = Math.round(__rafClock * 10) / 10; // one timestamp for the whole frame
      const batch = __rafQueue;
      __rafQueue = [];
      __rafScheduled = false;
      for (const cb of batch) { try { cb.fn(ts); } catch (e) {} }
    }, 16);
  }
  return id;
};
globalThis.cancelAnimationFrame = (id) => { __rafQueue = __rafQueue.filter((c) => c.id !== id); };
// Route queueMicrotask through the virtual timer queue (NOT a real V8 microtask).
// The "correct" Promise.resolve().then is unbounded — a reactivity lib that
// re-schedules a flush each microtask spins V8's microtask queue forever, which the
// render budget's terminate-execution can't cleanly interrupt (orphan CPU). The
// timer queue is bounded by the hydration pump's timer budget, so a runaway loop
// fails fast instead of leaking. (Such an app doesn't converge headlessly anyway.)
globalThis.queueMicrotask = (fn) => globalThis.setTimeout(fn, 0);
globalThis.__runTimers = (max = 100000) => {
  let n = 0;
  while (__timers.length && n < max) {
    // Earliest-due first.
    let bi = 0;
    for (let i = 1; i < __timers.length; i++) if (__timers[i].due < __timers[bi].due) bi = i;
    const t = __timers[bi];
    // A delayed timer past the (relative) virtual budget is a never-idle poll — stop firing
    // it so the drain can quiesce. (Delay-0 work has due <= __now and always runs.)
    if (t.due > __now && t.due - __budgetBase > __VIRTUAL_BUDGET_MS) break;
    __timers.splice(bi, 1);
    if (t.due > __now) __now = t.due; // advance the virtual clock
    n++;
    try { t.fn(...t.args); } catch (e) { Deno.core.print("timer error: " + (e && e.stack ? e.stack : e) + "\n"); }
  }
  return n; // count fired — lets the hydration pump detect quiescence
};
// NOTE: getElementsByTagName/ClassName/Name, lastChild/previous*/nextElementSibling,
// and document.write/writeln are provided by the vendored binding (browser_env.js,
// turbo-test ≥ 71477ba) — real-world bundles (jQuery's load-time support probe,
// document.write-driven pages) depend on them. They live upstream, not here.
//
// document.cookie bridge → the shared CookieJar (scoped to the page base URL). An
// OWN accessor on the document instance, shadowing browser_env.js's pure-JS jar.
Object.defineProperty(globalThis.document, "cookie", {
  configurable: true,
  get() { return ops.op_cookie_get(); },
  set(v) { ops.op_cookie_set(String(v)); },
});
// Headers — fetch + analytics (PostHog) construct/read these; deno_core ships none.
// Case-insensitive name lookup, per the spec.
if (typeof globalThis.Headers === "undefined") {
  globalThis.Headers = class Headers {
    constructor(init) {
      this._m = new Map();
      if (init) {
        const ents = typeof init.forEach === "function" ? null : (Array.isArray(init) ? init : Object.entries(init));
        if (ents) for (const [k, v] of ents) this.append(k, v);
        else init.forEach((v, k) => this.append(k, v));
      }
    }
    append(k, v) { const key = String(k).toLowerCase(); this._m.set(key, this._m.has(key) ? this._m.get(key) + ", " + v : String(v)); }
    set(k, v) { this._m.set(String(k).toLowerCase(), String(v)); }
    get(k) { const v = this._m.get(String(k).toLowerCase()); return v == null ? null : v; }
    has(k) { return this._m.has(String(k).toLowerCase()); }
    delete(k) { this._m.delete(String(k).toLowerCase()); }
    forEach(cb, thisArg) { for (const [k, v] of this._m) cb.call(thisArg, v, k, this); }
    keys() { return this._m.keys(); }
    values() { return this._m.values(); }
    entries() { return this._m.entries(); }
    [Symbol.iterator]() { return this._m.entries(); }
  };
}
// Fetch `Response`/`Request` — real classes (not object literals) so libraries that
// do `x instanceof Response` / `x instanceof Request` work instead of throwing
// ("Right-hand side of 'instanceof' is not an object" — a single undefined `Response`
// aborts a shared bundle and, cascading through webpack's module init, kills React
// hydration entirely). Bodies are text-backed (the render tier marshals strings).
if (typeof globalThis.Response === "undefined") {
  globalThis.Response = class Response {
    constructor(body, init) {
      init = init || {};
      this._body = body == null ? "" : (typeof body === "string" ? body : String(body));
      this.status = init.status == null ? 200 : init.status | 0;
      this.statusText = init.statusText || "";
      this.ok = this.status >= 200 && this.status < 300;
      this.redirected = !!init.redirected;
      this.type = init.type || "basic";
      this.url = init.url || "";
      this.bodyUsed = false;
      const h = new globalThis.Headers(init.headers || undefined);
      this.headers = h;
    }
    static json(data, init) {
      const r = new globalThis.Response(JSON.stringify(data), init);
      r.headers.set("content-type", "application/json");
      return r;
    }
    static error() { const r = new globalThis.Response("", { status: 0 }); r.type = "error"; return r; }
    static redirect(url, status) { return new globalThis.Response("", { status: status || 302, headers: { location: url } }); }
    clone() { const r = new globalThis.Response(this._body, { status: this.status, statusText: this.statusText, url: this.url }); this.headers.forEach((v, k) => r.headers.set(k, v)); return r; }
    async text() { this.bodyUsed = true; return this._body; }
    async json() { this.bodyUsed = true; return JSON.parse(this._body || "null"); }
    async arrayBuffer() { this.bodyUsed = true; return new TextEncoder().encode(this._body).buffer; }
    async blob() { this.bodyUsed = true; return new globalThis.Blob([this._body], { type: this.headers.get("content-type") || "" }); }
    async formData() { this.bodyUsed = true; const fd = new globalThis.FormData(); return fd; }
    get body() { const b = this._body; return new globalThis.ReadableStream({ start(c) { if (b) c.enqueue(new TextEncoder().encode(b)); c.close(); } }); }
  };
}
if (typeof globalThis.Request === "undefined") {
  globalThis.Request = class Request {
    constructor(input, init) {
      init = init || {};
      this.url = (input && typeof input === "object") ? input.url : String(input);
      this.method = (init.method || (input && input.method) || "GET").toUpperCase();
      this.headers = new globalThis.Headers(init.headers || (input && input.headers) || undefined);
      this._body = init.body != null ? init.body : (input && input._body) || null;
      this.bodyUsed = false;
      this.credentials = init.credentials || "same-origin";
      this.mode = init.mode || "cors";
      this.cache = init.cache || "default";
      this.redirect = init.redirect || "follow";
      this.referrer = init.referrer || "about:client";
      this.signal = init.signal || (globalThis.AbortController ? new globalThis.AbortController().signal : null);
    }
    clone() { return new globalThis.Request(this.url, { method: this.method, headers: this.headers, body: this._body }); }
    async text() { this.bodyUsed = true; return this._body == null ? "" : String(this._body); }
    async json() { this.bodyUsed = true; return JSON.parse(this._body || "null"); }
    async arrayBuffer() { this.bodyUsed = true; return new TextEncoder().encode(this._body == null ? "" : String(this._body)).buffer; }
  };
}
// fetch over the tier-1 net stack → a real Response (with real headers, so RSC
// client navigation that reads `res.headers.get('content-type')` works).
globalThis.fetch = async (url, init) => {
  // Marshal the request (method/headers/body) to the op — a Request object carries them
  // on itself; an init object carries them as fields. Headers may be a Headers instance,
  // an array of pairs, or a plain object.
  const req = (url && typeof url === "object") ? url : null;
  const o = init || req || {};
  let hdrs = o.headers;
  if (hdrs && typeof hdrs.forEach === "function" && !Array.isArray(hdrs)) {
    const obj = {}; hdrs.forEach((v, k) => { obj[k] = v; }); hdrs = obj;
  } else if (Array.isArray(hdrs)) {
    const obj = {}; for (const [k, v] of hdrs) obj[k] = v; hdrs = obj;
  }
  let body = o.body;
  if (body != null && typeof body !== "string") {
    try { body = String(body); } catch (_e) { body = ""; }
  }
  // Next.js App Router CLIENT navigation (`router.push`/`replace`, e.g. the post-login
  // redirect) fetches the target route's RSC flight with an `RSC` header — a PREFETCH
  // adds `Next-Router-Prefetch`. We don't do in-place RSC soft-nav (it never completes
  // headlessly, so `location`/`history` never advance and a `waitForURL` hangs). Record
  // the navigation target on `__rscNav`; the live-session driver re-loads that route as
  // a fresh page (the browser hard-nav equivalent), following the redirect chain hop by
  // hop. Prefetches are ignored.
  try {
    const lc = {};
    for (const k in (hdrs || {})) lc[k.toLowerCase()] = hdrs[k];
    if (lc.rsc && !lc["next-router-prefetch"]) {
      const u = new URL(String((url && url.url) || url), globalThis.location.href);
      if (u.pathname !== globalThis.location.pathname) {
        // Keep the app's own query (e.g. the off-cycle termination flow passes the selected
        // employee as `?employeeIds=`) but drop Next's internal `_rsc` cache-buster — a hard
        // reload carrying `_rsc` returns a flight payload, not HTML.
        u.searchParams.delete("_rsc");
        globalThis.__rscNav = u.pathname + u.search + u.hash;
      }
    }
  } catch (_e) {}
  const initJson = JSON.stringify({ method: o.method, headers: hdrs || undefined, body });
  // Count in-flight fetches so the hydration/interaction drain doesn't quiesce while a
  // request is outstanding — otherwise a save POST resolves AFTER the drain gives up
  // (the DOM looks "stable" while waiting) and its success re-render (modal close,
  // redirect) is lost.
  globalThis.__pendingFetches = (globalThis.__pendingFetches || 0) + 1;
  let r;
  try {
    r = await ops.op_fetch(String((url && url.url) || url), initJson);
  } finally {
    globalThis.__pendingFetches = Math.max(0, (globalThis.__pendingFetches || 1) - 1);
  }
  // Network log: the Playwright shim drains this to emit `page.on('response')`
  // events (tests subscribe to capture API payloads — payroll period, employments).
  try {
    globalThis.__netLog = globalThis.__netLog || [];
    globalThis.__netLog.push({
      url: String((url && url.url) || url),
      status: r.status, ok: r.ok, method: (o.method || "GET"),
      contentType: r.content_type || "", body: r.body,
    });
    if (globalThis.__netLog.length > 1000) globalThis.__netLog.splice(0, globalThis.__netLog.length - 1000);
  } catch (_e) {}
  const headers = {};
  if (r.content_type) headers["content-type"] = r.content_type;
  return new globalThis.Response(r.body, {
    status: r.status,
    headers,
    url: String((url && url.url) || url),
  });
};
// XMLHttpRequest over fetch (async; resolves in the event loop). Exposes the
// EventTarget surface (`addEventListener`/`removeEventListener`/`dispatchEvent`) as well
// as the `on*` props: real code wires the request via `req.addEventListener('load'/
// 'readystatechange', …)` (Google's home-page bundle does), and a stub with only `on*`
// crashed with "req.addEventListener is not a function", aborting the module.
globalThis.XMLHttpRequest = class {
  constructor() {
    this.readyState = 0; this.status = 0; this.statusText = ""; this.responseText = ""; this.response = "";
    this.responseType = ""; this.responseURL = ""; this.withCredentials = false; this.timeout = 0;
    // Real XHR exposes an XMLHttpRequestUpload EventTarget; collectors sometimes wire
    // `xhr.upload.onprogress`, and a missing `.upload` throws on assignment.
    this.upload = { onprogress: null, onload: null, onerror: null, onloadend: null,
      addEventListener() {}, removeEventListener() {}, dispatchEvent() { return false; } };
    this._h = {};       // request headers
    this._l = {};       // event listeners by type
    this._rh = "";      // raw response headers (for get*ResponseHeader)
    this._ct = "";      // response content-type
    this._aborted = false;
  }
  open(method, url) { this._m = method || "GET"; this._u = url; this._aborted = false; this._setState(1); }
  setRequestHeader(k, v) { this._h[String(k)] = String(v); }
  // Anti-bot/analytics code inspects response headers (e.g. a token-echo or a
  // cache/CSP header); expose both accessors over the one header op_fetch returns.
  getResponseHeader(k) {
    const want = String(k).toLowerCase();
    if (want === "content-type") return this._ct || null;
    return null;
  }
  getAllResponseHeaders() { return this._rh; }
  abort() {
    this._aborted = true;
    if (this.readyState > 0 && this.readyState < 4) { this._setState(4); this._emit("abort"); this._emit("loadend"); }
    this.status = 0;
  }
  addEventListener(type, fn) { if (typeof fn === "function") (this._l[type] = this._l[type] || []).push(fn); }
  removeEventListener(type, fn) { const a = this._l[type]; if (a) { const i = a.indexOf(fn); if (i >= 0) a.splice(i, 1); } }
  dispatchEvent(ev) { this._emit(ev && ev.type, ev); return true; }
  // Fire an event to both the matching `on*` property and any addEventListener handlers.
  _emit(type, ev) {
    if (!type) return;
    const e = ev || { type, target: this, currentTarget: this };
    const on = this["on" + type];
    if (typeof on === "function") { try { on.call(this, e); } catch (_e) {} }
    for (const fn of (this._l[type] || []).slice()) { try { fn.call(this, e); } catch (_e) {} }
  }
  _setState(s) { this.readyState = s; this._emit("readystatechange"); }
  // Marshal the raw body into the value `response` should hold for the requested
  // responseType. `responseText` is only defined for "" / "text" (spec), but we keep
  // it populated for lenient callers.
  _finishResponse(text) {
    this.responseText = text;
    const t = this.responseType;
    if (t === "json") { try { this.response = JSON.parse(text || "null"); } catch (_e) { this.response = null; } }
    else if (t === "arraybuffer") { try { this.response = new globalThis.TextEncoder().encode(text).buffer; } catch (_e) { this.response = null; } }
    else if (t === "blob") { try { this.response = new globalThis.Blob([text], { type: this._ct }); } catch (_e) { this.response = null; } }
    else { this.response = text; }
  }
  send(body) {
    const self = this;
    self._emit("loadstart");
    globalThis
      .fetch(this._u, { method: this._m, body, headers: self._h })
      .then(async (r) => {
        if (self._aborted) return;
        self.status = r.status;
        self.statusText = r.status === 200 ? "OK" : "";
        self.responseURL = self._u ? String(self._u) : "";
        self._ct = (r.headers && r.headers.get && r.headers.get("content-type")) || "";
        self._rh = self._ct ? ("content-type: " + self._ct + "\r\n") : "";
        // HEADERS_RECEIVED → LOADING → DONE, like a real transfer (some collectors
        // gate on readyState 2/3 transitions, not only 4).
        self._setState(2);
        self._setState(3);
        self._finishResponse(await r.text());
        self._setState(4);
        self._emit("load");
        self._emit("loadend");
      })
      .catch(() => { if (self._aborted) return; self._setState(4); self._emit("error"); self._emit("loadend"); });
  }
};
// XHR readyState constants (both on the instance-facing class and its prototype), as
// real code branches on `XMLHttpRequest.DONE` / `this.HEADERS_RECEIVED`.
Object.assign(globalThis.XMLHttpRequest, { UNSENT: 0, OPENED: 1, HEADERS_RECEIVED: 2, LOADING: 3, DONE: 4 });
Object.assign(globalThis.XMLHttpRequest.prototype, { UNSENT: 0, OPENED: 1, HEADERS_RECEIVED: 2, LOADING: 3, DONE: 4 });
// Observers: no live mutation notifications over the static tree → no-op stubs. Each must be a
// DISTINCT constructor: real Chrome has three separate classes, so `IntersectionObserver ===
// ResizeObserver` is false and each `.name` is its own — collapsing them onto one object is a
// trivial `===` / `.name` fingerprint tell. IntersectionObserver additionally fires ONE initial
// async entry per observed element (Chrome does this even for an off-screen/zero-size node, with
// isIntersecting:false), so visibility-gated init isn't silently dead.
function __mkObserver(name, withEntries) {
  const C = { [name]: class { constructor(cb) { this._cb = cb; this._dead = false; } unobserve() {} takeRecords() { return []; }
    disconnect() { this._dead = true; }
    observe(el) {
      if (!withEntries || typeof this._cb !== "function") return;
      const cb = this._cb, self = this;
      setTimeout(() => {
        // Chrome delivers the initial entry asynchronously; disconnect() before it fires
        // cancels it. Honour that so a page that observes-then-disconnects sees no callback.
        if (self._dead) return;
        try {
        let box = { x: 0, y: 0, width: 0, height: 0, top: 0, right: 0, bottom: 0, left: 0 };
        try { if (el && el.getBoundingClientRect) box = el.getBoundingClientRect(); } catch (e) {}
        cb([{ target: el, isIntersecting: false, intersectionRatio: 0, boundingClientRect: box,
          intersectionRect: { x: 0, y: 0, width: 0, height: 0, top: 0, right: 0, bottom: 0, left: 0 },
          rootBounds: null, time: (globalThis.performance && performance.now) ? performance.now() : 0 }], self);
      } catch (e) {} }, 0);
    } } }[name];
  return C;
}
globalThis.MutationObserver = __mkObserver("MutationObserver", false);
globalThis.IntersectionObserver = __mkObserver("IntersectionObserver", true);
globalThis.ResizeObserver = __mkObserver("ResizeObserver", false);
// structuredClone — apps/SDKs use it (and probe `globalThis.structuredClone.prototype`);
// absent, that probe throws. deno_core doesn't ship it. Structured-ish deep clone with a
// few common types; falls back to JSON for the rest.
if (typeof globalThis.structuredClone === "undefined") {
  globalThis.structuredClone = (v) => {
    const seen = new WeakMap();
    const clone = (x) => {
      if (x === null || typeof x !== "object") return x;
      if (seen.has(x)) return seen.get(x);
      if (x instanceof Date) return new Date(x.getTime());
      if (x instanceof RegExp) return new RegExp(x.source, x.flags);
      if (Array.isArray(x)) { const a = []; seen.set(x, a); for (const e of x) a.push(clone(e)); return a; }
      if (x instanceof Map) { const m = new Map(); seen.set(x, m); for (const [k, val] of x) m.set(clone(k), clone(val)); return m; }
      if (x instanceof Set) { const s = new Set(); seen.set(x, s); for (const e of x) s.add(clone(e)); return s; }
      const o = {}; seen.set(x, o); for (const k of Object.keys(x)) o[k] = clone(x[k]); return o;
    };
    return clone(v);
  };
}
// NOTE: getComputedStyle is provided by the vendored browser_env.js (a jsdom-style
// getComputedStyle the Playwright shim's cssValue/visibility reads). Do NOT redefine
// IT here — ENV_BOOTSTRAP runs AFTER the binding, so an override would clobber the
// real one and break the shim.
//
// matchMedia, though, ships as an always-`matches:false` stub. That makes every
// responsive component render its MOBILE/collapsed variant (a `min-width:` desktop
// query never matches) — e.g. Nike's header hydrates to a hamburger with its nav
// links hidden, so the desktop nav "disappears" after hydration with NO error. We
// override ONLY matchMedia (getComputedStyle untouched) with a real evaluator that
// tests the query against the layout viewport (window.innerWidth/innerHeight), the
// JS mirror of the CSS `@media` evaluation the layout tier already does.
{
  const __mqLen = (tok, basis) => {
    // A CSS length in a media feature → px. Supports px and em/rem (16px root).
    const m = String(tok).match(/([\d.]+)\s*(px|r?em)?/);
    if (!m) return NaN;
    const n = parseFloat(m[1]);
    return m[2] === "em" || m[2] === "rem" ? n * 16 : n;
  };
  const __mqClause = (clause) => {
    if (clause.indexOf("print") >= 0) return false;
    const w = typeof globalThis.innerWidth === "number" ? globalThis.innerWidth : 1280;
    const h = typeof globalThis.innerHeight === "number" ? globalThis.innerHeight : 800;
    let ok = true;
    for (const feat of clause.split(/\band\b/)) {
      let m;
      if ((m = feat.match(/min-width\s*:\s*([^)]+)/)) && w < __mqLen(m[1])) ok = false;
      if ((m = feat.match(/max-width\s*:\s*([^)]+)/)) && w > __mqLen(m[1])) ok = false;
      if ((m = feat.match(/min-height\s*:\s*([^)]+)/)) && h < __mqLen(m[1])) ok = false;
      if ((m = feat.match(/max-height\s*:\s*([^)]+)/)) && h > __mqLen(m[1])) ok = false;
      // Desktop defaults: light scheme, landscape, fine pointer + hover available.
      if (/prefers-color-scheme\s*:\s*dark/.test(feat)) ok = false;
      if (/orientation\s*:\s*portrait/.test(feat) && w >= h) ok = false;
      if (/orientation\s*:\s*landscape/.test(feat) && w < h) ok = false;
      if (/hover\s*:\s*none/.test(feat)) ok = false;
      if (/pointer\s*:\s*coarse/.test(feat)) ok = false;
      if (/any-pointer\s*:\s*coarse/.test(feat)) ok = false;
    }
    return ok;
  };
  const __evalMedia = (q) => {
    const query = String(q || "").toLowerCase().trim();
    if (!query || query === "all") return true;
    return query.split(",").some((c) => __mqClause(c.trim()));
  };
  globalThis.matchMedia = function (q) {
    const media = String(q == null ? "" : q);
    const mql = {
      media,
      onchange: null,
      _l: [],
      addListener(fn) { if (typeof fn === "function") this._l.push(fn); },
      removeListener(fn) { const i = this._l.indexOf(fn); if (i >= 0) this._l.splice(i, 1); },
      addEventListener(_t, fn) { if (typeof fn === "function") this._l.push(fn); },
      removeEventListener(_t, fn) { const i = this._l.indexOf(fn); if (i >= 0) this._l.splice(i, 1); },
      dispatchEvent() { return false; },
    };
    Object.defineProperty(mql, "matches", { get: () => __evalMedia(media), enumerable: true });
    return mql;
  };
}
// FormData — auth/login SDKs (PropelAuth) build credential payloads with it; deno_core
// ships none. A spec-shaped impl over an entry list (append keeps duplicates; set
// replaces; field values stringified, File/Blob passed through).
if (typeof globalThis.FormData === "undefined") {
  globalThis.FormData = class FormData {
    constructor() { this._e = []; }
    append(name, value) { this._e.push([String(name), typeof value === "object" && value !== null ? value : String(value)]); }
    set(name, value) {
      const n = String(name); const v = typeof value === "object" && value !== null ? value : String(value);
      this._e = this._e.filter(([k]) => k !== n); this._e.push([n, v]);
    }
    get(name) { const n = String(name); const f = this._e.find(([k]) => k === n); return f ? f[1] : null; }
    getAll(name) { const n = String(name); return this._e.filter(([k]) => k === n).map(([, v]) => v); }
    has(name) { const n = String(name); return this._e.some(([k]) => k === n); }
    delete(name) { const n = String(name); this._e = this._e.filter(([k]) => k !== n); }
    forEach(cb, thisArg) { for (const [k, v] of this._e) cb.call(thisArg, v, k, this); }
    keys() { return this._e.map(([k]) => k)[Symbol.iterator](); }
    values() { return this._e.map(([, v]) => v)[Symbol.iterator](); }
    entries() { return this._e.map(([k, v]) => [k, v])[Symbol.iterator](); }
    [Symbol.iterator]() { return this.entries(); }
  };
}
// Blob / File / FileReader — analytics + upload code (PostHog, file inputs) reference
// these during hydration; deno_core ships none ("File is not defined" aborts PostHog
// init). Minimal spec-shaped impls over the concatenated parts as a string — enough to
// construct/inspect; no real binary I/O in this engine.
if (typeof globalThis.Blob === "undefined") {
  globalThis.Blob = class Blob {
    constructor(parts = [], opts = {}) {
      this._s = (parts || []).map((p) => (typeof p === "string" ? p : String(p))).join("");
      this.type = (opts && opts.type) || "";
    }
    get size() { return this._s.length; }
    async text() { return this._s; }
    async arrayBuffer() { return new TextEncoder().encode(this._s).buffer; }
    slice(a, b, type) { const n = new Blob([this._s.slice(a, b)]); n.type = type || ""; return n; }
    stream() { const s = this._s; return new globalThis.ReadableStream({ start(c) { c.enqueue(s); c.close(); } }); }
  };
}
if (typeof globalThis.File === "undefined") {
  globalThis.File = class File extends globalThis.Blob {
    constructor(parts, name, opts = {}) {
      super(parts, opts);
      this.name = String(name == null ? "" : name);
      this.lastModified = (opts && opts.lastModified) || 0;
    }
  };
}
if (typeof globalThis.FileReader === "undefined") {
  globalThis.FileReader = class FileReader {
    constructor() { this.result = null; this.onload = null; this.onerror = null; this.onloadend = null; }
    readAsText(blob) { this._read(blob, (s) => s); }
    readAsDataURL(blob) { this._read(blob, (s) => "data:" + (blob.type || "") + ";base64," + btoa(s)); }
    readAsArrayBuffer(blob) { this._read(blob, (s) => new TextEncoder().encode(s).buffer); }
    _read(blob, map) {
      const self = this;
      Promise.resolve(blob && typeof blob.text === "function" ? blob.text() : "").then((s) => {
        self.result = map(s);
        const ev = { target: self };
        if (typeof self.onload === "function") self.onload(ev);
        if (typeof self.onloadend === "function") self.onloadend(ev);
      });
    }
  };
}
// customElements — the Web Components registry. deno_core ships none, so a bundle that
// registers a custom element (MUI and friends do) threw "customElements is not defined"
// mid-script, aborting the rest of that chunk (→ missing UI). Register + resolve
// whenDefined; no live upgrade pass (the static tree isn't re-instantiated), which is
// enough to keep the page's JS running.
if (typeof globalThis.customElements === "undefined") {
  const __ce = new Map();
  const __waiters = new Map();
  globalThis.customElements = {
    define(name, ctor) {
      __ce.set(name, ctor);
      const w = __waiters.get(name);
      if (w) { w.forEach((r) => r(ctor)); __waiters.delete(name); }
    },
    get(name) { return __ce.get(name); },
    getName(ctor) { for (const [n, c] of __ce) if (c === ctor) return n; return null; },
    whenDefined(name) {
      if (__ce.has(name)) return Promise.resolve(__ce.get(name));
      return new Promise((r) => {
        const arr = __waiters.get(name) || [];
        arr.push(r);
        __waiters.set(name, arr);
      });
    },
    upgrade() {},
  };
}
// (CSSStyleSheet, document.adoptedStyleSheets, and the HTML*Element constructor
// family are all provided by the vendored browser_env binding.)
// MessagePort / MessageChannel — real classes (not object-literal stubs), so `port instanceof
// MessagePort` holds and `port.addEventListener('message', fn)` works, not just `onmessage=`
// (Chrome shape; the object-literal stub was a brand/instanceof tell). React 18's scheduler
// drains its queue by posting to a MessagePort and running the peer's onmessage; google's SERP/
// homepage use the same `new MessageChannel; port1.onmessage=…; port2.postMessage(0)` next-tick
// idiom — delivery routes through the virtual timer queue (setTimeout 0) so the hydration/
// interaction pump drains it. Setting `onmessage` implies `start()` (Chrome); the addEventListener
// path queues until start().
// MessageEvent — a REAL constructor (the vendored binding ships only a hollow `function(){}`
// stub, so `new MessageEvent('message',{data}).data` was undefined). Our postMessage / port /
// BroadcastChannel dispatch build events via this so `ev instanceof MessageEvent` holds and the
// event carries data/origin/source/ports like Chrome. Defined before the ports that use it.
globalThis.MessageEvent = class MessageEvent {
  constructor(type, init) {
    init = init || {};
    this.type = String(type);
    this.data = init.data !== undefined ? init.data : null;
    this.origin = init.origin || "";
    this.lastEventId = init.lastEventId || "";
    this.source = init.source || null;
    this.ports = init.ports || [];
    this.bubbles = !!init.bubbles;
    this.cancelable = !!init.cancelable;
    this.composed = !!init.composed;
    this.isTrusted = false;
    this.timeStamp = Date.now();
    this.target = null;
    this.currentTarget = null;
    this.defaultPrevented = false;
  }
  preventDefault() { this.defaultPrevented = true; }
  stopPropagation() {}
  stopImmediatePropagation() {}
};
globalThis.MessagePort = class MessagePort {
  constructor() {
    this._onmessage = null; this._peer = null; this._l = []; this._started = false; this._q = [];
    // Setting `onmessage` implies start() (Chrome), which must FLUSH anything queued before a
    // listener existed — an assignment-only receiver otherwise strands early messages in _q.
    Object.defineProperty(this, "onmessage", {
      configurable: true, enumerable: true,
      get() { return this._onmessage; },
      set(fn) { this._onmessage = fn; if (typeof fn === "function") this.start(); },
    });
  }
  _fire(data) {
    const ev = new globalThis.MessageEvent("message", { data });
    ev.target = this;
    if (typeof this._onmessage === "function") { try { this._onmessage(ev); } catch (e) {} }
    for (const fn of this._l.slice()) { try { fn.call(this, ev); } catch (e) {} }
  }
  postMessage(data) {
    const p = this._peer; if (!p) return;
    globalThis.setTimeout(() => { if (p._started) p._fire(data); else p._q.push(data); }, 0);
  }
  start() { if (this._started) return; this._started = true; const q = this._q; this._q = []; for (const d of q) this._fire(d); }
  close() { this._peer = null; }
  addEventListener(t, fn) { if (t === "message" && typeof fn === "function") this._l.push(fn); }
  removeEventListener(t, fn) { if (t === "message") { const i = this._l.indexOf(fn); if (i >= 0) this._l.splice(i, 1); } }
  dispatchEvent() { return true; }
};
globalThis.MessageChannel = class MessageChannel {
  constructor() {
    this.port1 = new globalThis.MessagePort();
    this.port2 = new globalThis.MessagePort();
    this.port1._peer = this.port2;
    this.port2._peer = this.port1;
  }
};
// window.postMessage — deliver a `message` event to THIS realm's window listeners
// (async, via the timer queue so the hydration/interaction drain processes it). Real
// same-window `postMessage(msg)` fires `message` handlers with `{data, origin, source}`
// where `origin` is the sender's (i.e. our) origin. This is load-bearing for the
// reCAPTCHA/BotGuard host protocol: after the VM installs the grecaptcha API it drives
// its own scheduler by posting to the window and running work in the `message` handler —
// a bare `postMessage(...)` call (window.postMessage) with no shim throws
// "postMessage is not a function" and aborts the VM before any token path runs. The
// vendored binding supplies window `addEventListener`/`dispatchEvent`; we only add the
// poster. MessageEvent is a bare stub here, so build a plain event carrying the fields
// collectors read (data/origin/source/ports).
globalThis.postMessage = function postMessage(message, targetOrigin, transfer) {
  const origin = (globalThis.location && globalThis.location.origin) || "";
  const ports = Array.isArray(transfer) ? transfer : (transfer && transfer.length ? Array.prototype.slice.call(transfer) : []);
  globalThis.setTimeout(() => {
    const ev = new globalThis.MessageEvent("message", { data: message, origin, source: globalThis, ports });
    ev.target = globalThis;
    ev.currentTarget = globalThis;
    try { if (typeof globalThis.onmessage === "function") globalThis.onmessage(ev); } catch (_e) {}
    try { if (typeof globalThis.dispatchEvent === "function") globalThis.dispatchEvent(ev); } catch (_e) {}
  }, 0);
};
// window `on*` event-handler slots default to `null` in a real browser (never
// undefined). The reCAPTCHA VM reads `window.onmessage` while wiring its postMessage
// handshake; an undefined read (vs null) is both a functional gap and a headless tell.
for (const __on of ["onmessage", "onerror", "onmessageerror", "ononline", "onoffline", "onpopstate", "onhashchange", "onbeforeunload", "onunload", "onload"]) {
  try { if (globalThis[__on] === undefined) globalThis[__on] = null; } catch (_e) {}
}
// document.contentType — real Chrome reports "text/html" for an HTML document; the
// reCAPTCHA VM reads it. The vendored binding doesn't expose it, so add an own accessor.
try {
  if (globalThis.document && globalThis.document.contentType === undefined) {
    Object.defineProperty(globalThis.document, "contentType", { configurable: true, get() { return "text/html"; } });
  }
} catch (_e) {}
// TrustedTypes — real Chrome exposes `window.trustedTypes`; the reCAPTCHA VM (and many
// CSP-aware bundles) probe it and wrap script/HTML sinks through a policy. Absence is a
// (weak) tell and a `trustedTypes.createPolicy(...)` call on undefined throws. Pass-through
// policies (identity transforms) keep the sinks working with no real sanitization.
if (typeof globalThis.trustedTypes === "undefined") {
  const __mkPolicy = (name, rules) => ({
    name: String(name == null ? "" : name),
    createHTML: (s) => (rules && rules.createHTML ? rules.createHTML(s) : String(s)),
    createScript: (s) => (rules && rules.createScript ? rules.createScript(s) : String(s)),
    createScriptURL: (s) => (rules && rules.createScriptURL ? rules.createScriptURL(s) : String(s)),
  });
  globalThis.trustedTypes = {
    createPolicy: (name, rules) => __mkPolicy(name, rules),
    defaultPolicy: null, emptyHTML: "", emptyScript: "",
    getPropertyType: () => null, getAttributeType: () => null,
    isHTML: () => false, isScript: () => false, isScriptURL: () => false,
  };
}
// performance — React/Next read performance.now() for timing/scheduling. mark()/measure()
// must RETURN the PerformanceEntry they create (real spec): RUM/timing code destructures
// `const {startTime} = performance.mark(name)` (and reads `.duration`/`.entryType` off the
// measure), so returning undefined crashed with "Cannot destructure property 'startTime' …"
// (seen on nike.com's Boomerang beacon).
// Install the hi-res clock onto `performance` (defineProperty, not the `||` guard, so it wins
// whether or not deno_core preinstalled a coarse one). mark()/measure() ride the same clock.
{
  const P = globalThis.performance || (globalThis.performance = {});
  try { Object.defineProperty(P, "now", { value: __perfNow, configurable: true, writable: true }); }
  catch (e) { P.now = __perfNow; }
  try { Object.defineProperty(P, "timeOrigin", { value: __perfTimeOrigin, configurable: true, enumerable: true }); }
  catch (e) { P.timeOrigin = __perfTimeOrigin; }
  if (typeof P.mark !== "function") P.mark = (name, opts) => ({ name: String(name == null ? "" : name), entryType: "mark",
    startTime: (opts && +opts.startTime) || __perfNow(), duration: 0, detail: (opts && opts.detail) || null });
  if (typeof P.measure !== "function") P.measure = (name) => ({ name: String(name == null ? "" : name), entryType: "measure", startTime: 0, duration: 0, detail: null });
  if (typeof P.clearMarks !== "function") P.clearMarks = () => {};
  if (typeof P.clearMeasures !== "function") P.clearMeasures = () => {};
  if (typeof P.getEntries !== "function") P.getEntries = () => [];
  if (typeof P.getEntriesByName !== "function") P.getEntriesByName = () => [];
  if (typeof P.getEntriesByType !== "function") P.getEntriesByType = () => [];
}
// Legacy navigation timing: performance.timing (PerformanceTiming) + performance.navigation
// (PerformanceNavigation). Deprecated but Chrome still exposes both, and deno_core's
// `performance` ships neither — google's homepage reads them, so an anti-bot consistency
// check sees `undefined` where real Chrome has objects. Coherent, monotonic values off
// timeOrigin; the redirect/unload marks are 0 (a fresh top-level navigation).
try {
  const P = globalThis.performance;
  if (P && !P.timing) {
    // Realistic SPREAD of navigation phases (real Chrome: navigationStart < dns < connect <
    // request < response < domInteractive < DCL < domComplete < loadEventEnd). All-equal
    // timestamps are an obvious synthetic tell, so lay them out with plausible ordered gaps.
    const t = Math.floor(P.timeOrigin || Date.now());
    const r = (lo, hi) => lo + Math.floor(Math.random() * (hi - lo));
    const fetchStart = t + r(1, 4), dlS = fetchStart + r(1, 5), dlE = dlS + r(1, 6);
    const cS = dlE + r(0, 3), sc = cS + r(4, 14), cE = sc + r(8, 26), reqS = cE + r(1, 4);
    const resS = reqS + r(30, 120), resE = resS + r(15, 90), domL = resE + r(1, 6);
    const domI = domL + r(60, 220), dclS = domI + r(2, 18), dclE = dclS + r(1, 6);
    const domC = dclE + r(80, 400), lES = domC + r(1, 5), lEE = lES + r(1, 8);
    const timing = {
      navigationStart: t, unloadEventStart: 0, unloadEventEnd: 0, redirectStart: 0, redirectEnd: 0,
      fetchStart, domainLookupStart: dlS, domainLookupEnd: dlE, connectStart: cS,
      secureConnectionStart: sc, connectEnd: cE, requestStart: reqS, responseStart: resS,
      responseEnd: resE, domLoading: domL, domInteractive: domI,
      domContentLoadedEventStart: dclS, domContentLoadedEventEnd: dclE, domComplete: domC,
      loadEventStart: lES, loadEventEnd: lEE,
    };
    timing.toJSON = function () { return timing; };
    try { Object.defineProperty(P, "timing", { value: timing, configurable: true, enumerable: true }); }
    catch (e) { P.timing = timing; }
  }
  if (P && !P.navigation) {
    const nav = { type: 0, redirectCount: 0 };
    Object.defineProperties(nav, {
      TYPE_NAVIGATE: { value: 0 }, TYPE_RELOAD: { value: 1 },
      TYPE_BACK_FORWARD: { value: 2 }, TYPE_RESERVED: { value: 255 },
    });
    try { Object.defineProperty(P, "navigation", { value: nav, configurable: true, enumerable: true }); }
    catch (e) { P.navigation = nav; }
  }
  // performance.memory — Chrome-only non-standard heap gauge; real Chrome exposes it, and
  // fingerprinters read it. Static, coherent values (a fresh page's small heap).
  if (P && !P.memory) {
    const mem = { jsHeapSizeLimit: 2190000000, totalJSHeapSize: 12000000, usedJSHeapSize: 10000000 };
    try { Object.defineProperty(P, "memory", { value: mem, configurable: true, enumerable: true }); }
    catch (e) { P.memory = mem; }
  }
} catch (e) {}
// document.scrollingElement — real Chrome returns the root <html> in standards mode. rtdom's
// document has no layout, so point it at documentElement (google's homepage reads it).
try {
  const D = globalThis.document;
  if (D && !D.scrollingElement) {
    Object.defineProperty(D, "scrollingElement", {
      get() { return D.documentElement || null; }, configurable: true,
    });
  }
} catch (e) {}
// window.scheduler — the Prioritized Task Scheduling API (Chrome). google's homepage reads
// window.scheduler; deno_core doesn't ship it. postTask runs the callback through the virtual
// timer queue (honoring `delay`) and resolves with its result; yield() defers to a macrotask.
if (typeof globalThis.scheduler === "undefined") {
  globalThis.scheduler = {
    postTask(cb, opts) {
      return new Promise((resolve, reject) => {
        const delay = (opts && +opts.delay) || 0;
        setTimeout(() => { try { resolve(typeof cb === "function" ? cb() : undefined); } catch (e) { reject(e); } }, delay);
      });
    },
    yield() { return new Promise((r) => setTimeout(r, 0)); },
  };
}
// Notification API: real Chrome exposes `Notification` with a `permission` static ("default"
// until granted). Missing `Notification`/`permission` is a headless tell google's homepage reads.
if (typeof globalThis.Notification === "undefined") {
  const N = function Notification() {};
  N.permission = "default";
  N.maxActions = 2;
  N.requestPermission = function (cb) { if (typeof cb === "function") cb("default"); return Promise.resolve("default"); };
  globalThis.Notification = N;
} else if (globalThis.Notification.permission === undefined) {
  try { globalThis.Notification.permission = "default"; } catch (e) {}
}
// The CSS interface (window.CSS): CSS.supports (feature detection) + CSS.escape (identifier
// escaping). Bundles reference it at load — Google's deferred `xjs` bundle aborted with
// "CSS is not defined". No layout/CSS engine headless, so supports() validates the query
// shape and reports supported (as modern Chrome would for a well-formed query), and escape()
// implements the CSSOM ident serialization so a following `.replace`/selector build is safe.
if (typeof globalThis.CSS === "undefined") {
  const cssEscape = (value) => {
    const s = String(value);
    let out = "";
    for (let i = 0; i < s.length; i++) {
      const c = s.charCodeAt(i);
      if (c === 0) { out += "�"; continue; }
      // control chars, or a leading digit (or a digit right after a leading '-') → hex escape
      if ((c >= 0x1 && c <= 0x1f) || c === 0x7f ||
          (i === 0 && c >= 0x30 && c <= 0x39) ||
          (i === 1 && c >= 0x30 && c <= 0x39 && s.charCodeAt(0) === 0x2d)) {
        out += "\\" + c.toString(16) + " "; continue;
      }
      // a lone leading '-'
      if (i === 0 && c === 0x2d && s.length === 1) { out += "\\-"; continue; }
      // ident-safe: alphanumerics, '-', '_', and non-ASCII pass through unescaped
      if (c >= 0x80 || c === 0x2d || c === 0x5f ||
          (c >= 0x30 && c <= 0x39) || (c >= 0x41 && c <= 0x5a) || (c >= 0x61 && c <= 0x7a)) {
        out += s[i]; continue;
      }
      out += "\\" + s[i]; // everything else is backslash-escaped
    }
    return out;
  };
  globalThis.CSS = {
    // `supports("prop", "value")` (two-arg) or `supports("(prop: value)")` (condition string).
    supports: (a, b) => (b !== undefined
      ? (typeof a === "string" && a.length > 0)
      : (typeof a === "string" && a.indexOf(":") >= 0)),
    escape: cssEscape,
  };
}
// Encoding/crypto/base64 web globals deno_core doesn't ship but app bundles use.
if (typeof globalThis.TextEncoder === "undefined") {
  globalThis.TextEncoder = class TextEncoder {
    get encoding() { return "utf-8"; }
    encode(str = "") {
      str = String(str);
      const b = [];
      for (let i = 0; i < str.length; i++) {
        let c = str.charCodeAt(i);
        if (c < 0x80) b.push(c);
        else if (c < 0x800) b.push(0xc0 | (c >> 6), 0x80 | (c & 0x3f));
        else if (c >= 0xd800 && c <= 0xdbff) {
          const c2 = str.charCodeAt(++i);
          const cp = 0x10000 + ((c & 0x3ff) << 10) + (c2 & 0x3ff);
          b.push(0xf0 | (cp >> 18), 0x80 | ((cp >> 12) & 0x3f), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f));
        } else b.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 0x3f), 0x80 | (c & 0x3f));
      }
      return new Uint8Array(b);
    }
    encodeInto(str, u8) {
      const e = this.encode(str);
      u8.set(e.subarray(0, u8.length));
      return { read: str.length, written: Math.min(e.length, u8.length) };
    }
  };
}
if (typeof globalThis.TextDecoder === "undefined") {
  globalThis.TextDecoder = class TextDecoder {
    constructor(enc) { this.encoding = enc || "utf-8"; }
    decode(buf) {
      if (!buf) return "";
      const b = buf instanceof Uint8Array ? buf : new Uint8Array(buf.buffer || buf);
      let s = "", i = 0;
      while (i < b.length) {
        const c = b[i++];
        if (c < 0x80) s += String.fromCharCode(c);
        else if (c < 0xe0) s += String.fromCharCode(((c & 0x1f) << 6) | (b[i++] & 0x3f));
        else if (c < 0xf0) s += String.fromCharCode(((c & 0xf) << 12) | ((b[i++] & 0x3f) << 6) | (b[i++] & 0x3f));
        else {
          const cp = ((c & 0x7) << 18) | ((b[i++] & 0x3f) << 12) | ((b[i++] & 0x3f) << 6) | (b[i++] & 0x3f);
          const cc = cp - 0x10000;
          s += String.fromCharCode(0xd800 + (cc >> 10), 0xdc00 + (cc & 0x3ff));
        }
      }
      return s;
    }
  };
}
if (typeof globalThis.crypto === "undefined" || !globalThis.crypto.getRandomValues) {
  const __rb = (n) => { let x = 0; for (let i = 0; i < n.length; i++) { x = (x * 1103515245 + 12345) & 0x7fffffff; n[i] = (Date.now() ^ x ^ (i * 2654435761)) & 0xff; } return n; };
  globalThis.crypto = globalThis.crypto || {};
  globalThis.crypto.getRandomValues = (arr) => __rb(arr);
  globalThis.crypto.randomUUID = () => {
    const h = [];
    for (let i = 0; i < 16; i++) h.push((((Date.now() + i) * 9301 + 49297) % 256).toString(16).padStart(2, "0"));
    return `${h.slice(0,4).join("")}-${h.slice(4,6).join("")}-4${h[6].slice(1)}-${h[8]}${h[9]}-${h.slice(10,16).join("")}`;
  };
}
// crypto.subtle.digest (real SHA-256) — auth SDKs hash PKCE verifiers / state with it.
// Other operations reject clearly (vs an undefined-property crash) rather than no-op.
if (!globalThis.crypto.subtle) {
  const K = new Uint32Array([
    0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
    0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
    0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
    0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
    0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
    0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
    0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
    0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2,
  ]);
  const rotr = (n, x) => (x >>> n) | (x << (32 - n));
  const sha256 = (msg) => {
    const H = new Uint32Array([0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19]);
    const bitLen = msg.length * 8;
    const pad = (56 - ((msg.length + 1) % 64) + 64) % 64;
    const total = msg.length + 1 + pad + 8;
    const m = new Uint8Array(total);
    m.set(msg);
    m[msg.length] = 0x80;
    const dv = new DataView(m.buffer);
    dv.setUint32(total - 8, Math.floor(bitLen / 0x100000000));
    dv.setUint32(total - 4, bitLen >>> 0);
    const w = new Uint32Array(64);
    for (let i = 0; i < total; i += 64) {
      for (let t = 0; t < 16; t++) w[t] = dv.getUint32(i + t * 4);
      for (let t = 16; t < 64; t++) {
        const s0 = rotr(7, w[t-15]) ^ rotr(18, w[t-15]) ^ (w[t-15] >>> 3);
        const s1 = rotr(17, w[t-2]) ^ rotr(19, w[t-2]) ^ (w[t-2] >>> 10);
        w[t] = (w[t-16] + s0 + w[t-7] + s1) >>> 0;
      }
      let a=H[0],b=H[1],c=H[2],d=H[3],e=H[4],f=H[5],g=H[6],h=H[7];
      for (let t = 0; t < 64; t++) {
        const S1 = rotr(6,e) ^ rotr(11,e) ^ rotr(25,e);
        const ch = (e & f) ^ (~e & g);
        const t1 = (h + S1 + ch + K[t] + w[t]) >>> 0;
        const S0 = rotr(2,a) ^ rotr(13,a) ^ rotr(22,a);
        const maj = (a & b) ^ (a & c) ^ (b & c);
        const t2 = (S0 + maj) >>> 0;
        h=g; g=f; f=e; e=(d + t1) >>> 0; d=c; c=b; b=a; a=(t1 + t2) >>> 0;
      }
      H[0]=(H[0]+a)>>>0; H[1]=(H[1]+b)>>>0; H[2]=(H[2]+c)>>>0; H[3]=(H[3]+d)>>>0;
      H[4]=(H[4]+e)>>>0; H[5]=(H[5]+f)>>>0; H[6]=(H[6]+g)>>>0; H[7]=(H[7]+h)>>>0;
    }
    const out = new Uint8Array(32);
    const odv = new DataView(out.buffer);
    for (let i = 0; i < 8; i++) odv.setUint32(i * 4, H[i]);
    return out;
  };
  const reject = (op) => () => Promise.reject(new Error("crypto.subtle." + op + " unavailable in the no-browser render tier"));
  globalThis.crypto.subtle = {
    digest: (algo, data) => {
      const name = (typeof algo === "string" ? algo : (algo && algo.name) || "").toUpperCase();
      const bytes = data instanceof Uint8Array ? data : new Uint8Array(data.buffer || data);
      if (name === "SHA-256") return Promise.resolve(sha256(bytes).buffer);
      return Promise.reject(new Error("crypto.subtle.digest: " + name + " not supported (SHA-256 only)"));
    },
    importKey: reject("importKey"), exportKey: reject("exportKey"), generateKey: reject("generateKey"),
    sign: reject("sign"), verify: reject("verify"), encrypt: reject("encrypt"), decrypt: reject("decrypt"),
    deriveBits: reject("deriveBits"), deriveKey: reject("deriveKey"),
  };
}
// BroadcastChannel — auth SDKs sync session state across tabs over it. One isolate =
// "one tab", but deliver to other channels of the same name (some flows new up two).
if (typeof globalThis.BroadcastChannel === "undefined") {
  const __chans = {};
  globalThis.BroadcastChannel = class BroadcastChannel {
    constructor(name) {
      this.name = String(name);
      this.onmessage = null;
      this._closed = false;
      (__chans[this.name] = __chans[this.name] || []).push(this);
    }
    postMessage(data) {
      for (const c of __chans[this.name] || []) {
        if (c !== this && !c._closed) globalThis.setTimeout(() => { if (typeof c.onmessage === "function") { const ev = new globalThis.MessageEvent("message", { data }); ev.target = c; c.onmessage(ev); } }, 0);
      }
    }
    close() { this._closed = true; const a = __chans[this.name]; if (a) { const i = a.indexOf(this); if (i >= 0) a.splice(i, 1); } }
    addEventListener(t, fn) { if (t === "message") this.onmessage = fn; }
    removeEventListener() {}
    dispatchEvent() { return true; }
  };
}
// WebSocket — no live socket headless. Stay CONNECTING forever (never open, never
// close): apps connect in the background and render regardless, so this can't hang a
// render NOR trigger a reconnect loop (which firing onclose would).
if (typeof globalThis.WebSocket === "undefined") {
  globalThis.WebSocket = class WebSocket {
    constructor(url) {
      this.url = String(url);
      this.readyState = 0; // CONNECTING, and it stays there
      this.onopen = this.onmessage = this.onerror = this.onclose = null;
      this.bufferedAmount = 0;
    }
    send() {}
    close() { this.readyState = 3; if (typeof this.onclose === "function") try { this.onclose({ type: "close", code: 1000, wasClean: true }); } catch (_e) {} }
    addEventListener(t, fn) { this["on" + t] = fn; }
    removeEventListener() {}
    dispatchEvent() { return true; }
  };
  Object.assign(globalThis.WebSocket, { CONNECTING: 0, OPEN: 1, CLOSING: 2, CLOSED: 3 });
}
if (typeof globalThis.btoa === "undefined") {
  const __B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  globalThis.btoa = (s) => {
    s = String(s); let out = "";
    for (let i = 0; i < s.length; i += 3) {
      const a = s.charCodeAt(i), b = s.charCodeAt(i + 1), c = s.charCodeAt(i + 2);
      const n = (a << 16) | ((isNaN(b) ? 0 : b) << 8) | (isNaN(c) ? 0 : c);
      out += __B64[(n >> 18) & 63] + __B64[(n >> 12) & 63] + (isNaN(b) ? "=" : __B64[(n >> 6) & 63]) + (isNaN(c) ? "=" : __B64[n & 63]);
    }
    return out;
  };
  globalThis.atob = (s) => {
    s = String(s).replace(/=+$/, ""); let out = "";
    for (let i = 0, bits = 0, val = 0; i < s.length; i++) {
      val = (val << 6) | __B64.indexOf(s[i]); bits += 6;
      if (bits >= 8) { bits -= 8; out += String.fromCharCode((val >> bits) & 0xff); }
    }
    return out;
  };
}
// AbortController/AbortSignal — fetch + many async libs take a signal. deno_core
// ships a STUB AbortController whose `.signal` is undefined, so override it outright.
{
  globalThis.AbortSignal = class AbortSignal {
    constructor() { this.aborted = false; this.reason = undefined; this.onabort = null; this._l = []; }
    addEventListener(t, fn) { if (t === "abort") this._l.push(fn); }
    removeEventListener(t, fn) { this._l = this._l.filter((f) => f !== fn); }
    dispatchEvent() { return true; }
    throwIfAborted() { if (this.aborted) throw this.reason || new Error("Aborted"); }
  };
  globalThis.AbortSignal.timeout = () => new globalThis.AbortSignal();
  globalThis.AbortSignal.abort = (r) => { const s = new globalThis.AbortSignal(); s.aborted = true; s.reason = r; return s; };
  globalThis.AbortController = class AbortController {
    constructor() { this.signal = new globalThis.AbortSignal(); }
    abort(reason) {
      if (this.signal.aborted) return;
      this.signal.aborted = true;
      this.signal.reason = reason;
      const ev = { type: "abort", target: this.signal };
      try { if (typeof this.signal.onabort === "function") this.signal.onabort(ev); } catch (_e) {}
      for (const fn of this.signal._l) { try { fn(ev); } catch (_e) {} }
    }
  };
}
// ReadableStream — the RSC client reads the flight payload as a stream. A queue-backed
// impl supporting start/pull/cancel + getReader().read() {value,done}.
//
// CRITICAL for streaming producers (Next's RSC flight): the controller is filled
// ASYNCHRONOUSLY — `enqueue` is called as `__next_f` rows arrive and `close` fires on
// DOMContentLoaded, both LATER than the first `read()`. So a `read()` that finds the
// queue empty-but-open must NOT report EOF — it must PARK until the next enqueue/close.
// (Returning {done:true} there truncates the flight payload mid-stream → React keeps
// retrying the desynced reader → the render never converges.) Parked reads are held in
// `_waiters` and settled by enqueue/close/error.
if (typeof globalThis.ReadableStream === "undefined") {
  globalThis.ReadableStream = class ReadableStream {
    constructor(source = {}, _strategy) {
      this._q = [];
      this._closed = false;
      this._err = null;
      this._source = source || {};
      this._locked = false;
      this._waiters = []; // pending {resolve,reject} for reads that outran the producer
      const settleNext = () => {
        if (!this._waiters.length) return false;
        if (this._q.length) { this._waiters.shift().resolve({ value: this._q.shift(), done: false }); return true; }
        if (this._err) { this._waiters.shift().reject(this._err); return true; }
        if (this._closed) { this._waiters.shift().resolve({ value: undefined, done: true }); return true; }
        return false;
      };
      const drain = () => { while (settleNext()) {} };
      const c = {
        enqueue: (chunk) => { this._q.push(chunk); drain(); },
        close: () => { this._closed = true; drain(); },
        error: (e) => { this._err = e; this._closed = true; drain(); },
        get desiredSize() { return 1; },
      };
      this._ctrl = c;
      try { if (typeof this._source.start === "function") this._source.start(c); } catch (e) { this._err = e; }
    }
    get locked() { return this._locked; }
    getReader() {
      const self = this;
      self._locked = true;
      const pump = async () => {
        if (!self._q.length && !self._closed && typeof self._source.pull === "function") {
          await self._source.pull(self._ctrl);
        }
      };
      return {
        async read() {
          await pump();
          if (self._q.length) return { value: self._q.shift(), done: false };
          if (self._err) throw self._err;
          if (self._closed) return { value: undefined, done: true };
          // Empty but still open: park until enqueue/close/error settles us.
          return new Promise((resolve, reject) => self._waiters.push({ resolve, reject }));
        },
        releaseLock() { self._locked = false; },
        async cancel(r) { self._closed = true; if (typeof self._source.cancel === "function") await self._source.cancel(r); },
      };
    }
    async cancel(r) { this._closed = true; if (typeof this._source.cancel === "function") await this._source.cancel(r); }
    pipeThrough(t) { return t && t.readable ? t.readable : this; }
    pipeTo() { return Promise.resolve(); }
    tee() { return [this, this]; }
  };
}
// History API (single virtual entry; updates location.href).
globalThis.history = {
  state: null,
  length: 1,
  pushState(s, _t, u) { this.state = s; if (u != null) globalThis.location.href = String(u); },
  replaceState(s, _t, u) { this.state = s; if (u != null) globalThis.location.href = String(u); },
  back() {}, forward() {}, go() {},
};
// requestIdleCallback must invoke the callback with an IdleDeadline ({didTimeout, timeRemaining()});
// the bare shim passed nothing, so `deadline.timeRemaining()` threw — a correctness bug + tell.
globalThis.requestIdleCallback = (fn) => globalThis.setTimeout(() => {
  const start = __perfNow();
  fn({ didTimeout: false, timeRemaining: () => Math.max(0, 50 - (__perfNow() - start)) });
}, 1);
globalThis.cancelIdleCallback = (id) => globalThis.clearTimeout(id);

// WHATWG URL + URLSearchParams — deno_core ships neither, but app bundles (Next.js,
// the PropelAuth SDK, …) use `new URL(...)` while hydrating, so without these the
// page crashes with "URL is not defined" before rendering. Regex-parsed: covers the
// http(s) shapes hydration reads (protocol/host/port/path/query/hash + searchParams).
if (typeof globalThis.URLSearchParams === "undefined") {
  globalThis.URLSearchParams = class URLSearchParams {
    constructor(init = "") {
      this._d = [];
      if (init instanceof URLSearchParams) { this._d = init._d.map((p) => [p[0], p[1]]); return; }
      if (init && typeof init === "object") {
        for (const k of Object.keys(init)) this._d.push([String(k), String(init[k])]);
        return;
      }
      let s = String(init);
      if (s[0] === "?") s = s.slice(1);
      if (!s) return;
      for (const pair of s.split("&")) {
        if (!pair) continue;
        const i = pair.indexOf("=");
        const k = i === -1 ? pair : pair.slice(0, i);
        const v = i === -1 ? "" : pair.slice(i + 1);
        const dec = (x) => { try { return decodeURIComponent(x.replace(/\+/g, " ")); } catch { return x; } };
        this._d.push([dec(k), dec(v)]);
      }
    }
    append(k, v) { this._d.push([String(k), String(v)]); }
    delete(k) { this._d = this._d.filter((p) => p[0] !== k); }
    get(k) { const p = this._d.find((p) => p[0] === k); return p ? p[1] : null; }
    getAll(k) { return this._d.filter((p) => p[0] === k).map((p) => p[1]); }
    has(k) { return this._d.some((p) => p[0] === k); }
    set(k, v) { this.delete(k); this._d.push([String(k), String(v)]); }
    sort() { this._d.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0)); }
    forEach(cb, t) { for (const p of this._d) cb.call(t, p[1], p[0], this); }
    keys() { return this._d.map((p) => p[0])[Symbol.iterator](); }
    values() { return this._d.map((p) => p[1])[Symbol.iterator](); }
    entries() { return this._d.map((p) => [p[0], p[1]])[Symbol.iterator](); }
    [Symbol.iterator]() { return this.entries(); }
    get size() { return this._d.length; }
    toString() {
      return this._d.map((p) => encodeURIComponent(p[0]) + "=" + encodeURIComponent(p[1])).join("&");
    }
  };
}
if (typeof globalThis.URL === "undefined") {
  const ABS = /^[a-zA-Z][a-zA-Z0-9+.-]*:/;
  globalThis.URL = class URL {
    constructor(url, base) {
      let input = String(url);
      if (!ABS.test(input)) {
        if (base == null) throw new TypeError("Invalid URL: " + url);
        const b = base instanceof URL ? base : new URL(String(base));
        if (input.startsWith("//")) input = b.protocol + input;
        else if (input.startsWith("/")) input = b.protocol + "//" + b.host + input;
        else if (input.startsWith("#")) input = b.protocol + "//" + b.host + b.pathname + b.search + input;
        else if (input.startsWith("?")) input = b.protocol + "//" + b.host + b.pathname + input;
        else {
          const dir = b.pathname.slice(0, b.pathname.lastIndexOf("/") + 1) || "/";
          input = b.protocol + "//" + b.host + dir + input;
        }
      }
      const m = /^([a-zA-Z][a-zA-Z0-9+.-]*:)(\/\/(([^/?#@]*)@)?([^/?#:]*)(:(\d+))?)?([^?#]*)(\?[^#]*)?(#.*)?$/.exec(input);
      if (!m) throw new TypeError("Invalid URL: " + url);
      this.protocol = m[1];
      const ui = (m[4] || "").split(":");
      this.username = ui[0] || "";
      this.password = ui[1] || "";
      this.hostname = m[5] || "";
      this.port = m[7] || "";
      this.pathname = m[8] || (m[2] ? "/" : "");
      this.hash = m[10] || "";
      this.searchParams = new URLSearchParams(m[9] || "");
    }
    get host() { return this.port ? this.hostname + ":" + this.port : this.hostname; }
    get origin() { return this.protocol + "//" + this.host; }
    get search() { const q = this.searchParams.toString(); return q ? "?" + q : ""; }
    set search(v) { this.searchParams = new URLSearchParams(String(v)); }
    get href() {
      const auth = this.username ? this.username + (this.password ? ":" + this.password : "") + "@" : "";
      return this.protocol + "//" + auth + this.host + this.pathname + this.search + this.hash;
    }
    set href(v) { Object.assign(this, new URL(v)); }
    toString() { return this.href; }
    toJSON() { return this.href; }
  };
  globalThis.URL.createObjectURL = () => "blob:turbo-surf";
  globalThis.URL.revokeObjectURL = () => {};
}

// Object-URL registry + download capture. The common client-side export pattern is
// `URL.createObjectURL(blob)` → set it on a `<a download href=…>` → `link.click()`. A real
// createObjectURL (keyed store) lets us recover the blob bytes when the anchor is clicked,
// and a wrapped HTMLAnchorElement.prototype.click records {filename, content} so the shim
// can resolve `page.waitForEvent('download')` + `download.path()`. Runs after browser_env
// installed the anchor prototype (ENV_BOOTSTRAP runs last).
(() => {
  const blobs = new Map();
  let bid = 0;
  globalThis.URL.createObjectURL = (obj) => {
    const url = "blob:turbo-surf/" + bid++;
    try { blobs.set(url, obj); } catch (_e) {}
    return url;
  };
  globalThis.URL.revokeObjectURL = () => {}; // keep the blob for a later .path() read
  globalThis.__readBlobUrl = (url) => {
    const b = blobs.get(url);
    if (b == null) return null;
    return b._s != null ? String(b._s) : "";
  };
  globalThis.__downloads = globalThis.__downloads || [];
  const record = (el) => {
    try {
      let n = el;
      while (n && n.nodeType === 1) {
        if (n.tagName === "A" && n.getAttribute) {
          let dl = n.getAttribute("download");
          if (dl == null && n.download) dl = n.download;
          if (dl != null) {
            const href = (n.getAttribute("href") || n.href || "");
            const content = globalThis.__readBlobUrl(String(href));
            // Dedupe: an attached anchor records via BOTH the document listener and the
            // prototype wrap for one click — keep only one.
            const last = globalThis.__downloads[globalThis.__downloads.length - 1];
            if (last && last.url === String(href) && last.filename === (dl || "download")) return true;
            globalThis.__downloads.push({ filename: dl || "download", url: String(href), content: content == null ? "" : content });
            return true;
          }
        }
        n = n.parentElement;
      }
    } catch (_e) {}
    return false;
  };
  // Capture-phase listener for ATTACHED `<a download>` clicks (the event bubbles to the
  // document). Plus a prototype wrap for DETACHED anchors (createElement + click without
  // appendChild), whose dispatched event never reaches the document.
  try {
    globalThis.document.addEventListener("click", (e) => { record(e && e.target); }, true);
  } catch (_e) {}
  const proto = globalThis.HTMLAnchorElement && globalThis.HTMLAnchorElement.prototype;
  if (proto && typeof proto.click === "function") {
    const orig = proto.click;
    proto.click = function () {
      record(this);
      return orig.call(this);
    };
  }
})();

// location — back it with a real URL so setting `location.href` (done at install time,
// and by history.pushState/replaceState) UPDATES pathname/search/hash/host/origin too.
// browser_env.js ships a plain static object whose href is just a string field, so
// pathname stayed "/" regardless of the page URL — and a client router that reads
// `usePathname()`/`useSearchParams()` (Next's app router, route guards) then misroutes:
// the payroll login page rendered "Redirecting…" instead of the form because the auth
// guard saw pathname "/" (a protected route) rather than "/login". Defined AFTER the URL
// polyfill so `new URL` is available. Components are live getters over the backing URL.
(() => {
  let _u = null;
  const reparse = (v, base) => { try { _u = new globalThis.URL(String(v), base); } catch (_e) { /* keep prior */ } };
  reparse((globalThis.location && globalThis.location.href) || "http://localhost/");
  const loc = {
    assign(v) { reparse(v, _u ? _u.href : undefined); },
    replace(v) { reparse(v, _u ? _u.href : undefined); },
    reload() {},
    toString() { return _u ? _u.href : ""; },
  };
  for (const f of ["href", "protocol", "host", "hostname", "port", "pathname", "search", "hash", "origin"]) {
    Object.defineProperty(loc, f, {
      enumerable: true,
      configurable: true,
      get() { return _u ? _u[f] : ""; },
      // setting href reparses (relative allowed against the current URL); other
      // components write through to the backing URL where it supports it.
      set(v) { if (f === "href") reparse(v, _u ? _u.href : undefined); else if (_u) { try { _u[f] = v; } catch (_e) {} } },
    });
  }
  globalThis.location = loc;
})();

// document.referrer / URL / documentURI / baseURI — standard read-only document props
// deno_core's binding lacks. Analytics (PostHog reads referrer + the URL and `.split`s
// them) throws "Cannot read properties of undefined" without these, looping forever.
(() => {
  const d = globalThis.document;
  if (!d) return;
  const def = (name, get) => {
    try {
      if (typeof d[name] === "undefined") Object.defineProperty(d, name, { configurable: true, get });
    } catch (_e) {}
  };
  def("referrer", () => "");
  // document.location === window.location in a browser. Next's dev flight client reads
  // `document.location.origin` (in findSourceMapURL, replaying server console entries);
  // without this it throws "reading 'origin' of undefined", which aborts the ENTIRE RSC
  // flight stream processing → the App Router page never finishes hydrating, silently.
  def("location", () => globalThis.location);
  def("URL", () => globalThis.location.href);
  def("documentURI", () => globalThis.location.href);
  def("baseURI", () => globalThis.location.href);
  // hasFocus(): auth/idle code refreshes only a focused document; default to focused.
  try { if (typeof d.hasFocus !== "function") d.hasFocus = () => true; } catch (_e) {}
  // document.open()/close(): RUM beacons (nike.com's Boomerang/mPulse) do
  // `iframe.contentWindow.document.open()` — our iframe contentWindow IS the realm — then
  // `document.write(...)`. The binding had no `open`, so it threw "document.open is not a
  // function" and aborted the beacon. open() must return the document (callers chain
  // `d = document.open()`) and deliberately does NOT clear the already-parsed/hydrated tree
  // (a real open() wipes the document, but wiping the hydrated DOM headless is worse than a
  // no-op); close() is a no-op. write() is provided by the vendored binding.
  try { if (typeof d.open !== "function") d.open = () => d; } catch (_e) {}
  try { if (typeof d.close !== "function") d.close = () => {}; } catch (_e) {}
})();

// document.createTreeWalker + NodeFilter — focus-management code (MUI's DataGrid / focus
// trap, ARIA widgets) walks the tree with these; deno_core's binding has neither, so a
// page with a data grid threw "createTreeWalker is not a function" and rendered blank.
// A document-order DFS honoring whatToShow + the accept filter (REJECT skips the subtree,
// SKIP skips the node but descends) — enough for focusable-element scans.
(() => {
  const d = globalThis.document;
  if (!d || typeof d.createTreeWalker === "function") return;
  globalThis.NodeFilter = globalThis.NodeFilter || {
    SHOW_ALL: 0xffffffff, SHOW_ELEMENT: 1, SHOW_TEXT: 4, SHOW_COMMENT: 128,
    FILTER_ACCEPT: 1, FILTER_REJECT: 2, FILTER_SKIP: 3,
  };
  const ACCEPT = 1, REJECT = 2;
  d.createTreeWalker = function (root, whatToShow, filter) {
    const show = whatToShow == null ? 0xffffffff : whatToShow >>> 0;
    const accept = (n) => {
      const t = n.nodeType || 1;
      const bit = t === 1 ? 1 : t === 3 ? 4 : t === 8 ? 128 : 1 << (t - 1);
      if ((show & bit) === 0) return 3; // SKIP (wrong type)
      const fn = filter && (typeof filter === "function" ? filter : filter.acceptNode);
      if (typeof fn === "function") {
        try { return fn.call(filter, n); } catch (_e) { return 1; }
      }
      return 1;
    };
    const w = { root, whatToShow: show, filter, currentNode: root };
    w.nextNode = function () {
      let node = this.currentNode;
      for (;;) {
        let child = node.firstChild;
        let descend = true;
        while (descend && child) {
          node = child;
          const r = accept(node);
          if (r === ACCEPT) { this.currentNode = node; return node; }
          if (r === REJECT) { descend = false; } // don't descend; fall to sibling search
          else child = node.firstChild; // SKIP → keep descending
        }
        let t = node;
        while (t && t !== this.root) {
          if (t.nextSibling) { node = t.nextSibling; break; }
          t = t.parentNode;
        }
        if (!t || t === this.root) return null;
        const r = accept(node);
        if (r === ACCEPT) { this.currentNode = node; return node; }
      }
    };
    w.firstChild = function () {
      let c = this.currentNode.firstChild;
      while (c) { const r = accept(c); if (r === ACCEPT) { this.currentNode = c; return c; } c = c.nextSibling; }
      return null;
    };
    w.nextSibling = function () {
      let s = this.currentNode.nextSibling;
      while (s) { const r = accept(s); if (r === ACCEPT) { this.currentNode = s; return s; } s = s.nextSibling; }
      return null;
    };
    w.parentNode = function () {
      let p = this.currentNode.parentNode;
      while (p && p !== this.root) { if (accept(p) === ACCEPT) { this.currentNode = p; return p; } p = p.parentNode; }
      return null;
    };
    w.previousNode = function () { return null; }; // rarely used by focus scans
    return w;
  };
  // NodeIterator (same filter model, linear) — some libs use it instead of TreeWalker.
  if (typeof d.createNodeIterator !== "function") {
    d.createNodeIterator = function (root, whatToShow, filter) {
      const tw = d.createTreeWalker(root, whatToShow, filter);
      return { nextNode: () => tw.nextNode(), previousNode: () => null, detach() {} };
    };
  }
})();

// Viewport / screen globals — no real rendering surface here, but analytics and
// responsive code read them (PostHog `.split`/`.height` on undefined throws → loops
// forever). Sensible desktop defaults.
(() => {
  const set = (k, v) => {
    if (typeof globalThis[k] === "undefined") {
      try { globalThis[k] = v; } catch (_e) {}
    }
  };
  // Window/viewport geometry, kept physically coherent with the screen:
  //   screen.height >= availHeight >= outerHeight >= innerHeight   (the window fits ON the
  // screen; the viewport fits IN the window under the tab strip + omnibox). Widths are equal
  // (Chrome has no left/right window chrome; the scrollbar lives inside inner). A window taller
  // than its screen — or equal inner/outer height — is impossible and a tell. `screen` was set
  // earlier in this bootstrap, so derive the defaults from it and clamp to fit.
  const __chromeH = 88; // ~40px tab strip + ~48px toolbar (no bookmarks bar)
  const __scr = globalThis.screen || {};
  const __availW = typeof __scr.availWidth === "number" ? __scr.availWidth : 1920;
  const __availH = typeof __scr.availHeight === "number" ? __scr.availHeight : 1055;
  // Outer window: default to the available screen area (a maximized-ish window), never larger.
  const __ow = Math.min(__pick("outerWidth", Math.min(1280, __availW)), __availW);
  const __oh = Math.min(__pick("outerHeight", Math.min(800 + __chromeH, __availH)), __availH);
  // Inner viewport: window minus chrome (height) and equal width; never larger than the window.
  const __iw = Math.min(__pick("innerWidth", __ow), __ow);
  const __ih = Math.min(__pick("innerHeight", Math.max(__oh - __chromeH, 0)), __oh);
  set("innerWidth", __iw);
  set("innerHeight", __ih);
  set("outerWidth", __ow);
  set("outerHeight", __oh);
  set("devicePixelRatio", 1);
  set("screenX", 0);
  set("screenY", 0);
  set("scrollX", 0);
  set("scrollY", 0);
  set("pageXOffset", 0);
  set("pageYOffset", 0);
  set("scroll", () => {});
  set("scrollTo", () => {});
  set("scrollBy", () => {});
  set("screen", {
    width: 1280, height: 800, availWidth: 1280, availHeight: 800,
    colorDepth: __colorDepth, pixelDepth: __pixelDepth,
    orientation: { type: "landscape-primary", angle: 0, addEventListener() {}, removeEventListener() {} },
  });
  // visualViewport tracks the layout viewport (unpinched) — must equal innerWidth/innerHeight,
  // else `visualViewport.width !== innerWidth` is an incoherence tell. Derive from the values
  // set just above rather than repeating a stale 1280x800.
  set("visualViewport", {
    width: __iw, height: __ih, scale: 1, offsetLeft: 0, offsetTop: 0, pageLeft: 0, pageTop: 0,
    addEventListener() {}, removeEventListener() {}, dispatchEvent() { return false; },
  });
  // PerformanceObserver — analytics / experiment code (e.g. Wikipedia's header
  // enrollments) `new PerformanceObserver(...)` at load; an undefined ref throws
  // inside a Promise and rejects the chain, killing the dependent script.
  set("PerformanceObserver", class PerformanceObserver {
    constructor(cb) { this._cb = cb; }
    observe() {} disconnect() {} takeRecords() { return []; }
  });
  // Real Chrome exposes a populated static list here; an empty array is a tell.
  try {
    globalThis.PerformanceObserver.supportedEntryTypes = [
      "element", "event", "first-input", "largest-contentful-paint", "layout-shift",
      "longtask", "mark", "measure", "navigation", "paint", "resource", "visibility-state",
    ];
  } catch (_e) {}
})();

// `Node.prototype.replaceChild(new, old)` — jQuery's `replaceWith`/`domManip` call
// it; the DOM binding omits it. All wrapped elements share one template prototype,
// so define `replaceChild` there (from the `insertBefore` + `removeChild` the
// binding does provide). Guard against polluting `Object.prototype`.
(() => {
  var d = globalThis.document;
  if (!d || typeof d.createElement !== "function") return;
  var probe = d.createElement("div");
  if (!probe || typeof probe.insertBefore !== "function") return;
  var proto = Object.getPrototypeOf(probe);
  if (!proto || proto === Object.prototype) return;
  if (typeof proto.replaceChild !== "function") {
    proto.replaceChild = function (newChild, oldChild) {
      this.insertBefore(newChild, oldChild);
      this.removeChild(oldChild);
      return oldChild;
    };
  }
})();

// DOM interface constructors — React / emotion / many bundles do
// `x instanceof HTMLElement` (etc.) at load. If the constructor global is
// undefined, `instanceof` throws ("Right-hand side of 'instanceof' is not an
// object") and aborts the ENTIRE script bundle (Nike's whole React app failed this
// way). Define them (only if absent) with a duck-typed `Symbol.hasInstance` so the
// checks resolve correctly against our wrapped nodes by `nodeType`, rather than
// throwing.
(() => {
  var def = function (k, pred) {
    if (typeof globalThis[k] !== "undefined") return;
    var C = function () {};
    try { Object.defineProperty(C, Symbol.hasInstance, { value: pred, configurable: true }); } catch (_e) {}
    try { globalThis[k] = C; } catch (_e) {}
  };
  var nodeAt = function (t) { return function (x) { return !!x && x.nodeType === t; }; };
  var element = nodeAt(1);
  def("EventTarget", function (x) { return !!x && typeof x.addEventListener === "function"; });
  def("Node", function (x) { return !!x && typeof x.nodeType === "number"; });
  def("Element", element);
  def("CharacterData", function (x) { return !!x && (x.nodeType === 3 || x.nodeType === 8); });
  def("Text", nodeAt(3));
  def("Comment", nodeAt(8));
  def("Document", nodeAt(9));
  def("DocumentFragment", nodeAt(11));
  def("Window", function (x) { return x === globalThis; });
  def("Event", function (x) { return !!x && typeof x.type === "string" && ("target" in x || "bubbles" in x); });
  // Every concrete HTML*Element frameworks branch on is just an Element here.
  [
    "HTMLElement", "HTMLUnknownElement", "HTMLDivElement", "HTMLSpanElement",
    "HTMLAnchorElement", "HTMLInputElement", "HTMLButtonElement", "HTMLImageElement",
    "HTMLIFrameElement", "HTMLCanvasElement", "HTMLFormElement", "HTMLScriptElement",
    "HTMLStyleElement", "HTMLLinkElement", "HTMLTemplateElement", "HTMLTextAreaElement",
    "HTMLSelectElement", "HTMLOptionElement", "HTMLUListElement", "HTMLOListElement",
    "HTMLLIElement", "HTMLHeadingElement", "HTMLParagraphElement", "HTMLTableElement",
    "HTMLTableRowElement", "HTMLTableCellElement", "HTMLLabelElement", "HTMLPreElement",
    "SVGElement", "SVGSVGElement",
  ].forEach(function (k) { def(k, element); });
})();

// Next.js's webpack runtime reads `document.currentScript` to resolve chunk paths
// (getPathFromScript → `currentScript.getAttribute('src').replace(...)`). The tier
// runs the page's scripts as one concatenated bundle, so there's no "current" script
// element — expose a detached one whose `src` is the page URL (a string, so the
// `.replace` is safe) to keep that read working.
try {
  if (globalThis.document && !globalThis.document.currentScript) {
    const __cs = globalThis.document.createElement("script");
    const __href = (globalThis.location && globalThis.location.href) || "";
    __cs.setAttribute("src", __href);
    try { __cs.src = __href; } catch (_e) {}
    globalThis.document.currentScript = __cs;
  }
} catch (_e) {}

// `import.meta` shim. A turbopack/webpack DEV runtime injects scripts that read
// `import.meta.url` (and friends), but we run every <script> as a CLASSIC script via
// `(0, eval)` — and classic V8 rejects `import.meta` with a SyntaxError ("Cannot use
// 'import.meta' outside a module"), which aborted the whole chunk. There's no module
// record to attach a real `import.meta` to, so expose a stand-in global the script
// rewrite below maps `import.meta` onto. `url` is the page URL; `env` is empty (no
// build-time define table headless); `resolve` echoes the spec back as an absolute URL.
globalThis.__importMeta = {
  get url() { return (globalThis.location && globalThis.location.href) || ""; },
  env: {},
  resolve(spec) { try { return new globalThis.URL(spec, globalThis.location.href).href; } catch (_e) { return String(spec); } },
};

// esbuild's `keepNames` helper: transpiled code calls `__name(fn, "fn")` to restore
// Function.name after minification. esbuild emits a local `var __name = ...` per module,
// but injected/eval'd snippets (e.g. a test harness's tsx-transpiled addInitScript, or a
// bundle chunk that expects the helper hoisted) can reference it free. Provide a no-op
// passthrough global so such code doesn't ReferenceError. A module's own local `__name`
// shadows this; this only catches the free-reference case.
if (typeof globalThis.__name === "undefined") {
  globalThis.__name = function (fn) { return fn; };
}

// --- hydration pump: the browser's script-loading model -----------------------
// Real SPAs (Next.js/webpack) don't ship their code inline — they BOOT a tiny
// runtime that injects more <script src> chunks at runtime and waits for each
// chunk's `onload` before continuing (webpack's `__webpack_require__.e`). A node
// DOM that merely *appends* the <script> node never runs it, so the loader
// promise hangs and the app never mounts. So: execute each <script> element once
// (inline → eval in global scope; external → fetch its src then eval), and fire
// load/error so the loader resolves. `__hydrate()` drives this to quiescence.
const __EXECUTABLE_TYPES = new Set(["", "text/javascript", "application/javascript", "module"]);
function __fireScriptEvent(el, kind, err) {
  const ev = { type: kind, target: el, currentTarget: el, error: err };
  try { const h = kind === "load" ? el.onload : el.onerror; if (typeof h === "function") h.call(el, ev); } catch (_e) {}
  try { if (typeof el.dispatchEvent === "function") el.dispatchEvent(ev); } catch (_e) {}
}
// Rewrite `import.meta` (a SyntaxError in a classic script) onto the `__importMeta` global
// stub the dev HMR runtime reads (`.url`/`.env`). Whether a chunk is a REAL ES module is
// NOT decided by a regex here — a regex matches `import`/`export` inside comments + strings
// too (e.g. a vendored package's JSDoc `import {X} from 'y'`), which would wrongly route a
// CLASSIC turbopack chunk through the deno_core module pump and load it in a SEPARATE
// module graph → a DUPLICATE module instance (a second react-dom, whose event-system keys
// don't match the DOM's → portal/delegated onClick never fires). Instead `__execScriptEl`
// just classic-evals; only a genuine module-syntax SyntaxError (thrown at PARSE, before any
// code runs) routes the chunk to the module pump. Let V8 be the parser, not a regex.
globalThis.__rewriteEsmForClassic = function __rewriteEsmForClassic(code) {
  if (typeof code !== "string" || !code) return code;
  if (/import\s*\.\s*meta/.test(code)) {
    code = code.replace(/import\s*\.\s*meta/g, "globalThis.__importMeta");
  }
  return code;
}
globalThis.__execScriptEl = async function (el) {
  if (!el || el.__tcDone) return;
  el.__tcDone = true; // mark before await so a re-entrant pump round can't double-run
  const get = (n) => (typeof el.getAttribute === "function" ? el.getAttribute(n) : null);
  // Module-capable browsers SKIP `<script nomodule>` (they run the module build
  // instead). We support module scripts, so honor it: otherwise we force-run a
  // page's legacy polyfill bundle (e.g. Next's core-js `polyfill-nomodule`), which
  // overwrites native Promise/queueMicrotask with impls whose microtask scheduler
  // is inert in this env — promises never settle and the render never commits.
  if (get("nomodule") !== null || get("noModule") !== null) return;
  // ES modules (`<script type="module">`) run through the Rust module pump (a real
  // module graph + loader), NOT classic eval — leave them for `__takeModuleScript`.
  if ((get("type") || "").toLowerCase() === "module") return;
  if (!__EXECUTABLE_TYPES.has((get("type") || "").toLowerCase())) return; // JSON/data blocks etc.
  const src = get("src");
  try {
    let code;
    if (src) {
      const abs = new URL(src, globalThis.location.href).href;
      const res = await fetch(abs);
      if (!res.ok) { __fireScriptEvent(el, "error"); return; }
      code = await res.text();
    } else {
      code = el.textContent || el.text || "";
    }
    // A turbopack/webpack DEV runtime injects scripts written with ESM-only syntax
    // (`import.meta`, bare `import`/`export`), but we run every <script> as a CLASSIC
    // script, and classic V8 rejects those tokens with a SyntaxError that aborts the
    // whole chunk. `import.meta` is the common, fixable case (the dev HMR runtime reads
    // `import.meta.url`/`.env`): rewrite it onto the `__importMeta` global so the read
    // works. Real `import`/`export` statements need a module loader + resolved graph we
    // don't have headless — those scripts are SKIPPED gracefully (logged) rather than
    // hung/aborted.
    const __orig = code;
    code = globalThis.__rewriteEsmForClassic(code);
    // Set document.currentScript to THIS element during execution, like a browser.
    // Turbopack/webpack chunk runtimes do `TURBOPACK.push([document.currentScript, …])`
    // to correlate each chunk with the element that loaded it — a single static
    // currentScript makes every chunk look identical and the module graph never
    // resolves. Restore the prior value after (nested injects during eval).
    let __prevCs;
    try { __prevCs = globalThis.document.currentScript; globalThis.document.currentScript = el; } catch (_e) {}
    try {
      (0, eval)(code); // classic-script semantics: run in global scope
    } catch (e) {
      try { globalThis.document.currentScript = __prevCs; } catch (_e) {}
      // A genuine ES module? Classic V8 throws a module-syntax SyntaxError at PARSE time —
      // before ANY code runs — so it's safe to re-run through the module pump (a real module
      // graph + loader). This is the ONLY signal we route on: text that merely LOOKS like
      // import/export (in a comment/string of a classic turbopack chunk) parses + runs fine
      // here, so it stays classic (avoids a duplicate module instance — see __rewriteEsmForClassic).
      const msg = String((e && e.message) || e);
      if (e instanceof SyntaxError && /\b(import|export)\b|module/i.test(msg)) {
        // Keep the element: the module pump points document.currentScript at it during eval
        // so turbopack TURBOPACK.push([document.currentScript, …]) derives the right path.
        const abs = src ? new URL(src, globalThis.location.href).href : "";
        el.__tcModule = true; // claim it so the __takeModuleScript DOM scan won't double-run it
        (globalThis.__esmSrcQueue || (globalThis.__esmSrcQueue = [])).push({ src: abs, code: __orig, el: el });
        __fireScriptEvent(el, "load");
        return;
      }
      throw e; // real runtime error
    }
    try { globalThis.document.currentScript = __prevCs; } catch (_e) {}
    __fireScriptEvent(el, "load");
  } catch (e) {
    __fireScriptEvent(el, "error", e);
    Deno.core.print("script error (" + (src || "inline") + "): " + e + "\n");
  }
};
// Run every not-yet-run <script> in DOM order, drain timers, repeat while new
// scripts appear or timers keep firing. Bounded by maxRounds (+ the render budget).
globalThis.__hydrate = async function (maxRounds = 300, timerBudget = 200000) {
  let timersLeft = timerBudget; // total timer-callback budget across rounds — an app
  // whose scheduler never reaches idle (e.g. React polling a backend that never
  // answers) would otherwise spin until the render budget; cap it and return the
  // best-effort DOM rendered so far.
  for (let round = 0; round < maxRounds && timersLeft > 0; round++) {
    let ranScript = false;
    // Honor `defer`: a browser runs parser-blocking scripts as it reaches them and
    // holds `defer` scripts until after parsing, THEN runs them in document order.
    // Running in raw DOM order instead breaks the common pattern where an app's
    // `<script defer>` bundle sits EARLY in <head> but its (non-defer) framework
    // vendor script sits later — e.g. Nike defers `main.js` (byte ~88k) while
    // `react.js` is non-defer near </body> (~608k). Raw order runs `main` first →
    // `React is not defined` → the whole app never hydrates. So: non-defer scripts
    // first (document order), then `defer` scripts (document order). `async` isn't
    // deferred. New scripts injected mid-round re-partition on the next pass.
    const isDeferred = (el) =>
      el.getAttribute &&
      el.getAttribute("defer") !== null &&
      el.getAttribute("async") === null;
    const all = Array.prototype.slice.call(document.querySelectorAll("script"));
    const ordered = all.filter((el) => !isDeferred(el)).concat(all.filter(isDeferred));
    for (const el of ordered) {
      if (!el.__tcDone) { ranScript = true; await globalThis.__execScriptEl(el); }
    }
    const fired = globalThis.__runTimers(Math.min(timersLeft, 5000));
    timersLeft -= fired;
    if (!ranScript && fired === 0) break;
  }
};
// Claim the next un-run ES-module script (`<script type=module>` or an inline script
// with `import`/`export`) in DOM order → `__RESULT = {src, code}` JSON, or "" when
// none. The Rust module pump evaluates each through deno_core's real module graph
// (`__execScriptEl` deliberately skips them). `__tcModule` is the claim marker.
globalThis.__moduleStmt = /(^|[;{}\n\r])\s*(import\s+[^(]|import\s*['"]|export\s+|export\s*\{|export\s*\*)/;
globalThis.__takeModuleScript = function () {
  // src chunks whose body was ESM, already fetched + queued by __execScriptEl. Both
  // `src` (its URL identity, for import resolution) and `code` (the fetched body) set.
  const q = globalThis.__esmSrcQueue;
  if (q && q.length) {
    const item = q.shift();
    globalThis.__currentModuleEl = item.el || null; // for document.currentScript during eval
    globalThis.__RESULT = JSON.stringify({ src: item.src, code: item.code });
    return;
  }
  const scripts = Array.prototype.slice.call(document.querySelectorAll("script"));
  for (let i = 0; i < scripts.length; i++) {
    const el = scripts[i];
    if (el.__tcModule) continue;
    const type = ((el.getAttribute && el.getAttribute("type")) || "").toLowerCase();
    const src = (el.getAttribute && el.getAttribute("src")) || "";
    const code = src ? "" : (el.textContent || el.text || "");
    const isModule = type === "module" || (!src && globalThis.__moduleStmt.test(code));
    if (!isModule) continue;
    el.__tcModule = true;
    el.__tcDone = true;
    globalThis.__currentModuleEl = el; // for document.currentScript during eval
    globalThis.__RESULT = JSON.stringify({ src, code });
    return;
  }
  globalThis.__currentModuleEl = null;
  globalThis.__RESULT = "";
};
// getByRole/getByText/getByLabel resolved IN the LIVE isolate, returning each match's
// document-order index (querySelectorAll('*') position) so the Playwright shim can dispatch
// on `*`[idx] in the SAME context. The shim used to resolve these over a re-serialized
// snapshot (turbo-surf-view's by_role/by_text/by_label) and dispatch the snapshot's idx onto
// the live DOM — but serialize→reparse can reorder elements (e.g. portal'd MUI Autocomplete
// options / dialogs), so the idx pointed at the WRONG live node (a wrapper, not the option),
// and the click never reached the option's onClick. Resolving here over the live DOM keeps
// the idx and the dispatch in one context. Mirrors the Rust matchers (aria.rs/locator.rs).
globalThis.__tcGetBy = function (kind, value, name, root) {
  // `idx` is always the GLOBAL document-order position (so the shim dispatches on `*`[idx]).
  // `root` scopes matching to within elements matching that selector (descendant-or-self) —
  // backs `parentLocator.getByRole/getByText/getByLabel(...)`.
  const all = Array.prototype.slice.call(document.querySelectorAll("*"));
  let inScope = null;
  if (root) {
    inScope = new Set();
    const roots = Array.prototype.slice.call(document.querySelectorAll(root));
    for (let ri = 0; ri < roots.length; ri++) {
      inScope.add(roots[ri]);
      const sub = roots[ri].querySelectorAll("*");
      for (let si = 0; si < sub.length; si++) inScope.add(sub[si]);
    }
  }
  const attr = (el, n) => (el && el.getAttribute ? el.getAttribute(n) : null) || "";
  const roleOf = (el) => {
    const r = attr(el, "role");
    if (r) return r;
    const tag = (el.tagName || "").toLowerCase();
    if (tag === "input") {
      const t = attr(el, "type").toLowerCase();
      return t === "checkbox" ? "checkbox" : t === "radio" ? "radio"
        : (t === "button" || t === "submit" || t === "reset") ? "button" : "textbox";
    }
    return ({ a: "link", button: "button", select: "combobox", textarea: "textbox" })[tag] || "generic";
  };
  const accName = (el) => {
    const cands = [attr(el, "aria-label").trim(), (el.textContent || "").trim(),
      attr(el, "placeholder").trim(), attr(el, "value").trim(), attr(el, "title").trim()];
    for (const c of cands) if (c) return c;
    return "";
  };
  // Substring match, or a `/pattern/flags` regex literal (mirrors turbo-surf-view).
  const tmatch = (v, want) => {
    if (want == null) return true;
    const m = /^\/(.*)\/([a-z]*)$/.exec(want);
    if (m) { try { return new RegExp(m[1], m[2]).test(v); } catch (_e) { return false; } }
    return String(v).indexOf(want) >= 0;
  };
  const idxOf = (el) => all.indexOf(el);
  let hits = [];
  if (kind === "role") {
    for (const el of all) if (roleOf(el) === value && tmatch(accName(el), name)) hits.push(el);
  } else if (kind === "label") {
    const seen = new Set();
    const push = (el) => { if (el && !seen.has(el)) { seen.add(el); hits.push(el); } };
    for (const lab of document.querySelectorAll("label")) {
      if (!tmatch((lab.textContent || "").trim(), value)) continue;
      const forId = attr(lab, "for");
      if (forId) push(document.getElementById(forId));
      const id = attr(lab, "id");
      if (id) for (const t of document.querySelectorAll('[aria-labelledby~="' + id + '"]')) push(t);
      push(lab.querySelector("input,select,textarea"));
    }
    for (const el of document.querySelectorAll("[aria-label]")) if (tmatch(attr(el, "aria-label"), value)) push(el);
  } else if (kind === "text") {
    // Leaf-text match: the element's text matches AND no descendant element also matches
    // (so we target the tightest node, like turbo-surf-view's by_text).
    for (const el of all) {
      if (!tmatch((el.textContent || "").trim(), value)) continue;
      let childMatch = false;
      for (const c of el.querySelectorAll("*")) { if (tmatch((c.textContent || "").trim(), value)) { childMatch = true; break; } }
      if (!childMatch) hits.push(el);
    }
  }
  if (inScope) hits = hits.filter((el) => inScope.has(el));
  globalThis.__RESULT = JSON.stringify(hits.map((el) => ({ idx: idxOf(el) })));
};

// Resolve a locator through an nth-aware scope CHAIN, then match its leaf. `scope` is an
// ordered list of {sel, idx}: at each level, querySelectorAll(sel) within the current set,
// and when idx != null keep only that one (the `.nth(i)` of a parent locator). The leaf is
// either {selector} (getByTestId/locator) or {getBy:{kind,value,name}} (getByRole/Text/Label).
// idx in the output is the GLOBAL document-order position so the shim dispatches on `*`[idx].
// Needed because a CSS-concat selector can't express "the nth match's subtree".
globalThis.__tcResolveScoped = function (scope, leaf) {
  let cur = [document];
  for (let si = 0; si < (scope || []).length; si++) {
    const s = scope[si];
    let next = [];
    for (let ci = 0; ci < cur.length; ci++) {
      let m;
      try {
        m = cur[ci].querySelectorAll(s.sel);
      } catch (e) {
        m = [];
      }
      for (let mi = 0; mi < m.length; mi++) next.push(m[mi]);
    }
    // Apply a Locator.filter({hasText|hasNotText}) at this level BEFORE indexing, so a
    // `parent.filter(...).first()/nth()` scopes children to the same element the static
    // read path picks (a CSS-concat selector can't express the text filter).
    if (s.filter) {
      next = next.filter((el) => {
        const txt = (el.textContent || "");
        if (s.filter.hasText != null && txt.indexOf(s.filter.hasText) === -1) return false;
        if (s.filter.hasNotText != null && txt.indexOf(s.filter.hasNotText) !== -1) return false;
        return true;
      });
    }
    if (s.idx != null) {
      const i = s.idx < 0 ? next.length + s.idx : s.idx;
      next = next[i] ? [next[i]] : [];
    }
    cur = next;
  }
  const all = Array.prototype.slice.call(document.querySelectorAll("*"));
  if (leaf && leaf.selector) {
    let res = [];
    for (let ci = 0; ci < cur.length; ci++) {
      let m;
      try {
        m = cur[ci].querySelectorAll(leaf.selector);
      } catch (e) {
        m = [];
      }
      for (let mi = 0; mi < m.length; mi++) res.push(m[mi]);
    }
    globalThis.__RESULT = JSON.stringify(res.map((el) => ({ idx: all.indexOf(el) })));
    return;
  }
  if (leaf && leaf.getBy) {
    // Reuse __tcGetBy's role/text/label matcher by marking the resolved roots and scoping to
    // them (descendant-or-self via the [data-tc-scope] root selector).
    for (let ci = 0; ci < cur.length; ci++) if (cur[ci].setAttribute) cur[ci].setAttribute("data-tc-scope", "");
    globalThis.__tcGetBy(leaf.getBy.kind, leaf.getBy.value, leaf.getBy.name, "[data-tc-scope]");
    const out = globalThis.__RESULT;
    for (let ci = 0; ci < cur.length; ci++) if (cur[ci].removeAttribute) cur[ci].removeAttribute("data-tc-scope");
    globalThis.__RESULT = out;
    return;
  }
  globalThis.__RESULT = "[]";
};

// Apply CSS `:hover` styles for a hovered element. turbo-dom's cascade has no pointer state,
// so content revealed only by `.trigger:hover .menu { display:block }` (hover dropdowns/menus
// — e.g. the app's UserMenu, overridden to open on hover) stays display:none and waitFor
// (state:'visible') hangs. We simulate hover: mark the hovered chain (the element, its
// ancestors, and its deepest-first-child descendant path — the nodes a pointer over the
// element is "on") with [data-tc-hover], then for every `:hover` style rule (from <style>
// text AND any constructable/inserted sheets), rewrite `:hover` → `[data-tc-hover]`, match it
// live, and apply the rule's declarations INLINE on the matched elements — inline style feeds
// both this env's getComputedStyle and rtdom's native cascade (is_visible), so the reveal is
// observable. Best-effort + flat-rule only (skips nested @media); enough for hover menus.
globalThis.__tcApplyHover = function (el) {
  if (!el || !el.setAttribute) return;
  const mark = (n) => {
    if (n && n.setAttribute) n.setAttribute("data-tc-hover", "");
  };
  // A pointer over `el` is "on" el, all its ancestors, and (since we have no layout to know
  // which leaf the cursor lands on) any of its descendants — mark all three so a `:hover`
  // rule anchored on a nested trigger (e.g. MUI's `&:hover` on the menu root inside the
  // hovered wrapper) still matches.
  mark(el);
  for (let a = el.parentElement; a; a = a.parentElement) mark(a);
  try {
    const desc = el.querySelectorAll("*");
    for (let i = 0; i < desc.length; i++) mark(desc[i]);
  } catch (e) {}
  const cssTexts = [];
  try {
    const styles = document.querySelectorAll("style");
    for (let i = 0; i < styles.length; i++) cssTexts.push(styles[i].textContent || "");
  } catch (e) {}
  try {
    const sheets = document.styleSheets || [];
    for (let si = 0; si < sheets.length; si++) {
      let rules;
      try {
        rules = sheets[si].cssRules || [];
      } catch (e) {
        rules = [];
      }
      for (let ri = 0; ri < rules.length; ri++) cssTexts.push(String(rules[ri].cssText || ""));
    }
  } catch (e) {}
  // Flatten nested CSS (emotion serializes `.css-x{ base; &:hover .menu{ … } }`) into flat
  // (selector, decls) rules, resolving `&` to the parent selector. A flat-regex parse can't
  // do this — the reveal rule is nested under the trigger's class, with `&` standing in for it.
  const flat = [];
  const resolveSel = (parent, sel) =>
    sel
      .split(",")
      .map((p) => {
        p = p.trim();
        if (!parent) return p;
        return p.indexOf("&") >= 0 ? p.replace(/&/g, parent) : parent + " " + p;
      })
      .join(",");
  const flatten = (css) => {
    let pos = 0;
    const parse = (parentSel) => {
      let buf = "";
      let declBuf = "";
      while (pos < css.length) {
        const ch = css[pos++];
        if (ch === "}") {
          if (buf.indexOf(":") >= 0) declBuf += buf;
          if (declBuf.trim() && parentSel) flat.push({ sel: parentSel, decls: declBuf.trim() });
          return;
        }
        if (ch === ";") {
          declBuf += buf + ";";
          buf = "";
          continue;
        }
        if (ch === "{") {
          parse(resolveSel(parentSel, buf.trim()));
          buf = "";
          continue;
        }
        buf += ch;
      }
      if (declBuf.trim() && parentSel) flat.push({ sel: parentSel, decls: declBuf.trim() });
    };
    parse("");
  };
  for (let ci = 0; ci < cssTexts.length; ci++) flatten(cssTexts[ci]);
  for (let fi = 0; fi < flat.length; fi++) {
    const sel = flat[fi].sel;
    if (sel.indexOf(":hover") < 0) continue;
    const parts = sel.split(",");
    for (let pi = 0; pi < parts.length; pi++) {
      const s = parts[pi].trim();
      if (s.indexOf(":hover") < 0) continue;
      const rw = s.replace(/:hover/g, "[data-tc-hover]");
      let targets;
      try {
        targets = document.querySelectorAll(rw);
      } catch (e) {
        continue;
      }
      for (let ti = 0; ti < targets.length; ti++) {
        const t = targets[ti];
        const decls = flat[fi].decls.split(";");
        for (let di = 0; di < decls.length; di++) {
          const c = decls[di].indexOf(":");
          if (c < 0) continue;
          const prop = decls[di].slice(0, c).trim();
          const val = decls[di].slice(c + 1).trim();
          if (!prop) continue;
          try {
            t.style.setProperty(prop, val);
          } catch (e) {
            try {
              t.style[prop] = val;
            } catch (e2) {}
          }
        }
      }
    }
  }
};

// Pending-work signal for the Rust pump loop: "1" while timers are queued, a
// <script> hasn't run, an ES module is unclaimed, or a fetch is in-flight (more to do
// after the next async drain), else "0".
globalThis.__pendingWork = () =>
  (globalThis.__pendingFetches || 0) > 0 || __timers.length > 0 || ((globalThis.__esmSrcQueue || []).length) > 0 || Array.prototype.some.call(document.querySelectorAll("script"), (s) => !s.__tcDone || (((s.getAttribute && s.getAttribute("type")) || "").toLowerCase() === "module" && !s.__tcModule)) ? "1" : "0";
// In-flight fetch count — the interaction drain must keep pumping while > 0 even if
// the visible tree looks stable (the response's re-render hasn't happened yet).
globalThis.__pendingFetchCount = () => String(globalThis.__pendingFetches || 0);

// A cheap "has the DOM changed?" signal for the interaction drain: element count + the
// total length of input values (so a controlled-input edit registers). Lets the drain
// stop once the render has SETTLED even though background timers (analytics polling,
// React's idle scheduler) never stop — otherwise an interaction would always run to the
// full budget. Not for correctness, just to detect quiescence of the visible tree.
globalThis.__domSig = () => {
  try {
    const els = document.getElementsByTagName("*");
    let n = els.length, vlen = 0;
    const inputs = document.querySelectorAll("input,textarea,select");
    for (let i = 0; i < inputs.length; i++) vlen += (inputs[i].value || "").length;
    return n + ":" + vlen + ":" + (globalThis.location ? globalThis.location.href.length : 0);
  } catch (_e) {
    return "0";
  }
};

// Shadow DOM light-DOM fallback: embeddable widgets (PropelAuth's login) call
// host.attachShadow() and render into the returned root. rtdom has no shadow tree,
// so the root IS the host — rendered content lands in the serialized light DOM and
// stays queryable. Stamped as an OWN property (the binding's interceptor returns real
// own props) on every created element + the existing roots. Not true encapsulation,
// but enough to let a shadow-rendering widget mount into the document.
(function () {
  const addShadow = (el) => {
    if (el && typeof el.attachShadow !== "function") {
      el.attachShadow = function () {
        // Light-DOM fallback: the root IS the host. Set `.host` back to the host
        // (itself) too — code reads `shadowRoot.host` to get the host element back
        // (Next devtools: `var e = er.host; e.classList…`); without it that's undefined.
        try { this.shadowRoot = this; this.host = this; } catch (_e) {}
        return this;
      };
    }
    return el;
  };
  // <iframe> never "loads" here (no real navigation). Auth SDKs (PropelAuth) mount a
  // hidden refresh iframe and RETRY indefinitely when its `load` never fires — an
  // unbounded create/remove churn (700+ iframes on an authed page) that starves the
  // render budget so the real page never finishes committing. Fire a one-shot `load`
  // when the iframe's `src` is set (or on append) so the SDK's load-wait resolves and
  // the retry loop stops. We don't navigate the iframe; this only unblocks the waiter.
  const fireIframeLoad = (el) => {
    if (el.__tcLoadFired) return;
    el.__tcLoadFired = true;
    el.__loaded = true;
    globalThis.setTimeout(() => {
      const ev = { type: "load", target: el, currentTarget: el };
      try { if (typeof el.onload === "function") el.onload(ev); } catch (_e) {}
      try { (el.__loadCbs || []).forEach((f) => { try { f(ev); } catch (_e) {} }); } catch (_e) {}
      try { if (typeof el.dispatchEvent === "function") el.dispatchEvent(ev); } catch (_e) {}
    }, 0);
  };
  // ── <iframe> realms (generic, depth-capped) ──────────────────────────────────────
  // Every iframe — created via createElement, present in the parsed HTML, or carrying
  // `srcdoc` — instantiates a REAL bridged child realm (browser_env's __makeFrameRealm)
  // that runs the frame's OWN inline + `<script src>` scripts, with a DISTINCT
  // contentWindow/contentDocument, a wired frame tree (parent/top/frames/length), and a
  // depth cap that guards frame-bombs. reCAPTCHA's bframe/anchor handshake is now just the
  // special case this generalizes: it flows through the same loader. A hidden / src-less /
  // blocklisted iframe stays an inert stub (the analytics/auth churn fix) so it costs nothing.
  //
  // Fidelity tradeoff (unchanged): a single V8 isolate, so realms SHARE prototypes/globals —
  // no true origin isolation or separate realm IDENTITY; the child document is a lightweight
  // facade over native element construction, not a second live-rendered tree. Faithful enough
  // for the frame-tree + cross-frame message-channel semantics apps actually rely on.
  const FRAME_DEPTH_CAP = 12; // beyond this a nested iframe is an inert stub (frame-bomb guard)
  // Hosts whose iframes are analytics/auth CHURN (created + torn down hundreds of times during
  // hydration): keep them inert so the churn stays nearly free and the budget goes to the real
  // DOM. Config-driven so the list can grow without touching the loader.
  const FRAME_STUB_HOST_RE = /posthog|propelauth/i;

  const frameSrcOf = (el) =>
    String((el && (el.__src != null ? el.__src : el.src)) || (el && el.getAttribute && el.getAttribute("src")) || "");
  const frameSrcdocOf = (el) => {
    let s = el && (el.__srcdoc != null ? el.__srcdoc : el.srcdoc);
    if ((s == null || s === "") && el && el.getAttribute) s = el.getAttribute("srcdoc");
    return s == null ? null : String(s);
  };
  // A meaningful frame earns a realm; a hidden / src-less / blocklisted one stays inert.
  const frameWantsRealm = (el) => {
    const sd = frameSrcdocOf(el);
    if (sd != null && sd.length) return true;
    const s = frameSrcOf(el);
    if (!s || s === "about:blank") return false;
    if (FRAME_STUB_HOST_RE.test(s)) return false;
    return true;
  };
  const resolveFrameUrl = (el) => {
    const s = frameSrcOf(el);
    if (!s) return "";
    // Resolve a relative src against the OWNING frame's URL (not always the top window), so a
    // relative src in a nested/grandchild frame resolves correctly.
    const owner = el.__ownerWin || globalThis;
    const base = (owner.location && owner.location.href) || (globalThis.location && globalThis.location.href) || "http://localhost/";
    try { return new URL(s, base).href; }
    catch (_e) { return s; }
  };
  // Walk `parent` links to the real top window (self-aliased at the top, so it terminates).
  const realTopOf = (win) => {
    let w = win, guard = 0;
    while (w && w.parent && w.parent !== w && guard++ < 64) w = w.parent;
    return w || globalThis;
  };
  // Frame tree on `parentWin`: window.length = child count, window[i]/frames[i] = child
  // windows, and frames === window (as in a real browser).
  const registerChildFrame = (parentWin, childWin) => {
    const n = parentWin.__frameCount || 0;
    try { parentWin[n] = childWin; } catch (_e) {}
    parentWin.__frameCount = n + 1;
    try { parentWin.length = n + 1; } catch (_e) {}
    if (parentWin.frames == null || parentWin.frames === parentWin) {
      try { parentWin.frames = parentWin; } catch (_e) {}
    } else {
      try { parentWin.frames[n] = childWin; parentWin.frames.length = n + 1; } catch (_e) {}
    }
  };

  // Run a sub-VM's source inside the child realm: shadow the realm-scoped identifiers
  // (window/self/globalThis/document/parent/top/…) as function params so the VM's
  // `window`/`parent`/bare `postMessage` resolve to the child window + parent VIEW, not the
  // host globals. `parent` is the source-tagging parent VIEW (so `parent.postMessage` fires
  // the parent's listeners with source === this frame's contentWindow — the cross-frame
  // handshake relies on it); `top` is the REAL top window (so `top.foo` property reads work).
  // Builtins (fetch, JSON, crypto, Math) intentionally fall through to the real globals — the
  // documented shared-isolate tradeoff (no separate V8 context; shared prototypes).
  const runVmInRealm = (cw, code) => {
    const cd = cw.document;
    const fn = new Function(
      "window", "self", "globalThis", "document", "parent", "top", "frames", "frameElement",
      "location", "navigator", "screen", "postMessage", "addEventListener", "removeEventListener",
      "dispatchEvent",
      code
    );
    return fn.call(cw, cw, cw, cw, cd, cw.__parentView || cw.parent, cw.top, cw.frames, cw.frameElement,
      cw.location, cw.navigator, cw.screen,
      cw.postMessage.bind(cw), cw.addEventListener.bind(cw), cw.removeEventListener.bind(cw),
      cw.dispatchEvent.bind(cw));
  };
  // Extract + run the frame document's scripts in the realm, in document order: inline
  // bodies inline, and `<script src>` fetched over op_fetch (resolved against the frame URL,
  // shared cookie jar).
  const loadFrameScripts = async (cw, html, frameUrl) => {
    const re = /<script\b([^>]*)>([\s\S]*?)<\/script>/gi;
    let m;
    while ((m = re.exec(html)) !== null) {
      const attrs = m[1] || "";
      const srcM = /\ssrc\s*=\s*["']([^"']+)["']/i.exec(attrs);
      if (srcM) {
        let u = srcM[1];
        try { u = new URL(u, frameUrl).href; } catch (_e) {}
        const r = await ops.op_fetch(u, "{}");
        if (r && r.body) { try { runVmInRealm(cw, r.body); } catch (_e) {} }
      } else if (m[2] && m[2].trim()) {
        try { runVmInRealm(cw, m[2]); } catch (_e) {}
      }
    }
  };
  // Fire the child realm's DOMContentLoaded + load so the frame VM's init runs.
  const fireRealmLoad = (cw) => {
    const cd = cw.document;
    globalThis.setTimeout(() => {
      try { cd.readyState = "complete"; } catch (_e) {}
      const dcl = { type: "DOMContentLoaded", target: cd, currentTarget: cd };
      try { cd.dispatchEvent(dcl); } catch (_e) {}
      const load = { type: "load", target: cw, currentTarget: cw };
      try { if (typeof cw.onload === "function") cw.onload(load); } catch (_e) {}
      try { cw.dispatchEvent(load); } catch (_e) {}
    }, 0);
  };

  // Augment browser_env's child realm FROM RUNTIME (without editing the vendored binding):
  // give it the REAL parent + top window objects (identity: frames[0].parent === window),
  // keep the source-tagging parent VIEW as __parentView for the message bridge, expose its
  // own frame tree, and wire its document.createElement so a grandchild iframe builds a
  // grandchild realm at depth+1.
  const augmentRealm = (cw, parentWin, hostEl, depth) => {
    if (!cw || cw.__augmented) return cw;
    cw.__augmented = true;
    cw.__depth = depth;
    cw.__parentWin = parentWin;
    cw.__parentView = cw.parent;              // browser_env's source-tagging view (parent.postMessage)
    cw.parent = parentWin;                    // real immediate parent (identity check)
    cw.top = realTopOf(parentWin);            // real top window (top.foo reads work)
    cw.frameElement = hostEl || null;
    cw.frames = cw;                           // window.frames === window
    cw.length = 0; cw.__frameCount = 0;
    installFrameFactory(cw, cw.document, depth + 1);
    return cw;
  };

  // Build (sync, idempotent) the child realm for `el` and wire its frame tree. Script loading
  // is separate (async, see __loadFrame) so contentWindow is available immediately.
  globalThis.__frameRealm = (el, depth) => {
    if (el.__realm) return el.__realm;
    if (typeof globalThis.__makeFrameRealm !== "function") return globalThis;
    const parentWin = el.__ownerWin || globalThis;
    const cw = globalThis.__makeFrameRealm(el, parentWin);
    augmentRealm(cw, parentWin, el, depth == null ? (el.__depth || 0) : depth);
    registerChildFrame(parentWin, cw);
    return cw;
  };

  // The full frame load: ensure the realm, fetch the frame HTML (op_fetch for `src`, or the
  // `srcdoc` attribute directly), run its scripts in the realm, fire load. Counted in
  // __pendingFetches so the hydration drain waits for it. Idempotent per element.
  globalThis.__loadFrame = async (el, depth) => {
    if (el.__frameStarted) return el.__realm;
    el.__frameStarted = true;
    const cw = globalThis.__frameRealm(el, depth);
    if (!cw || cw === globalThis) { fireIframeLoad(el); return cw; }
    globalThis.__pendingFetches = (globalThis.__pendingFetches || 0) + 1;
    let frameUrl = (globalThis.location && globalThis.location.href) || "http://localhost/";
    try {
      let html = "";
      const sd = frameSrcdocOf(el);
      if (sd != null && sd.length) {
        html = sd;
      } else {
        const url = resolveFrameUrl(el);
        if (url) { frameUrl = url; const r = await ops.op_fetch(url, "{}"); html = (r && r.body) || ""; }
      }
      await loadFrameScripts(cw, html, frameUrl);
      fireRealmLoad(cw);
    } catch (_e) {
    } finally {
      globalThis.__pendingFetches = Math.max(0, (globalThis.__pendingFetches || 1) - 1);
      fireIframeLoad(el);
    }
    return cw;
  };
  // Back-compat shim: the reCAPTCHA bframe/anchor path is now just a frame load through the
  // generic loader. `url` is the bframe src; set it so the loader fetches + runs the bframe VM.
  globalThis.__recaptchaBframeHandshake = (el, url) => {
    try { if (url != null) el.__src = String(url); } catch (_e) {}
    return globalThis.__loadFrame(el, el.__depth || 1);
  };

  // A realm-capable iframe host: a plain object (it does NOT enter the rtdom tree, matching the
  // pre-existing created-iframe behavior). `depth` is this frame's depth; `ownerWin` is the
  // realm whose document created it (its parent). The src/srcdoc setters trigger a real realm
  // load unless the frame is inert (churn / no meaningful src) or past the depth cap. Its lazy
  // contentWindow/contentDocument mean an unused iframe still costs nothing.
  const buildIframe = (depth, ownerWin) => {
    const noop = () => {};
    const el = {
      nodeType: 1, nodeName: "IFRAME", tagName: "IFRAME", __iframeEl: true,
      __depth: depth, __ownerWin: ownerWin || globalThis,
      style: {}, dataset: {}, onload: null, onerror: null,
      setAttribute(n, v) { if (n === "src") { this.src = v; return; } if (n === "srcdoc") { this.srcdoc = v; return; } this[n] = v; },
      getAttribute(n) {
        if (n === "src") return this.__src || null;
        if (n === "srcdoc") return this.__srcdoc != null ? this.__srcdoc : null;
        return this[n] != null ? String(this[n]) : null;
      },
      removeAttribute(n) { delete this[n]; },
      appendChild(c) { return c; }, removeChild(c) { return c; },
      insertBefore(c) { return c; }, remove: noop,
      addEventListener(t, f) { if (t === "load") { (this.__loadCbs = this.__loadCbs || []).push(f); if (this.__loaded) fireIframeLoad(this); } },
      removeEventListener: noop, dispatchEvent: noop,
      getBoundingClientRect: () => ({ x: 0, y: 0, top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0 }),
      focus: noop, blur: noop, contains: () => false,
    };
    // Decide realm vs inert, then (for a realm) kick the async load. Idempotent.
    const maybeLoad = () => {
      if (el.__frameStarted) return;
      if (depth > FRAME_DEPTH_CAP || !frameWantsRealm(el)) { fireIframeLoad(el); return; }
      try { globalThis.__loadFrame(el, depth); } catch (_e) { fireIframeLoad(el); }
    };
    Object.defineProperty(el, "src", { configurable: true,
      get() { return el.__src || ""; },
      set(v) { el.__src = String(v); maybeLoad(); } });
    Object.defineProperty(el, "srcdoc", { configurable: true,
      get() { return el.__srcdoc || ""; },
      set(v) { el.__srcdoc = String(v); maybeLoad(); } });
    // contentWindow/contentDocument are the child realm's DISTINCT window/document once the
    // frame wants a realm; otherwise (inert/churn or past the cap) they point at the TOP
    // window/doc so an analytics SDK's builtin-prototype probe still resolves + caches.
    Object.defineProperty(el, "contentWindow", { configurable: true,
      get() {
        if (el.__realm) return el.__realm;
        if (depth <= FRAME_DEPTH_CAP && frameWantsRealm(el)) return globalThis.__frameRealm(el, depth);
        return globalThis;
      } });
    Object.defineProperty(el, "contentDocument", { configurable: true,
      get() {
        if (el.__realm) return el.__realm.document;
        if (depth <= FRAME_DEPTH_CAP && frameWantsRealm(el)) return globalThis.__frameRealm(el, depth).document;
        return globalThis.document;
      } });
    return el;
  };

  // Wire a document's createElement so `iframe` → a realm-capable host at `nextDepth`, owned by
  // `win`. Non-iframe tags fall through to the original createElement (with the top document's
  // shadow-DOM shim preserved for the top realm only).
  const installFrameFactory = (win, doc, nextDepth) => {
    if (!doc || doc.__frameFactory) return;
    doc.__frameFactory = true;
    const orig = typeof doc.createElement === "function" ? doc.createElement.bind(doc) : null;
    const isTop = win === globalThis;
    doc.createElement = (tag) => {
      if (String(tag).toLowerCase() === "iframe") return buildIframe(nextDepth, win);
      if (!orig) return null;
      return isTop ? addShadow(orig(tag)) : orig(tag);
    };
  };
  // Wire the TOP document: the iframes it creates are depth 1.
  installFrameFactory(globalThis, document, 1);

  // Pre-existing <iframe> elements in the parsed HTML get a realm too (created ones go through
  // the factory above). They are REAL rtdom nodes, so wire lazy contentWindow/contentDocument
  // accessors and load their content at depth 1.
  const wireExistingIframe = (el) => {
    if (!el || el.__frameStarted || el.__iframeWiredRT) return;
    el.__iframeWiredRT = true;
    el.__ownerWin = globalThis;
    el.__depth = 1;
    try {
      Object.defineProperty(el, "contentWindow", { configurable: true, get() { return el.__realm || globalThis; } });
      Object.defineProperty(el, "contentDocument", { configurable: true, get() { return el.__realm ? el.__realm.document : globalThis.document; } });
    } catch (_e) {}
    if (frameWantsRealm(el)) { try { globalThis.__loadFrame(el, 1); } catch (_e) {} }
  };
  try {
    const pre = document.querySelectorAll ? document.querySelectorAll("iframe") : [];
    for (let i = 0; i < pre.length; i++) wireExistingIframe(pre[i]);
  } catch (_e) {}

  if (document.body) addShadow(document.body);
  if (document.documentElement) addShadow(document.documentElement);
})();
// Native-function fidelity: a fingerprinter calls `fn.toString()` on built-ins
// and flags JS source where real Chrome returns "function x() { [native code] }".
// Make our JS polyfills report native. Plain reassignment, not a Proxy — a Proxy
// on toString is itself detectable (its own toString/`length` leak).
(() => {
  const orig = Function.prototype.toString;
  const native = new WeakSet();
  // Define the trap as a CONCISE METHOD (via an object literal), not a `function` expression: a
  // native function has NO own `prototype` property, but a `function` expression always does (and
  // it's non-configurable, so it can't be deleted). Concise methods — like real native methods —
  // have no `prototype`. A probe reading `'prototype' in Function.prototype.toString` would
  // otherwise flag the trap (and every `function`-expression shim) as non-native despite the
  // "[native code]" string. length must be 0 (native toString.length === 0), which a concise
  // method already gives.
  const ts = ({ toString() {
    if (native.has(this)) return "function " + (this.name || "") + "() { [native code] }";
    return orig.call(this);
  } }).toString;
  Function.prototype.toString = ts;
  native.add(ts); // the trap must report itself native too
  // Wrap a shim as a prototype-less, native-marked forwarder (concise-method form), preserving
  // `this`/args and pinning name + length to match the real built-in. Use where a `function`
  // shim's own `prototype` would be a native-shape tell.
  const nativize = (orig2, name, len) => {
    if (typeof orig2 !== "function") return orig2;
    const holder = { [name](...args) { return orig2.apply(this, args); } };
    const f = holder[name];
    try { Object.defineProperty(f, "length", { value: len == null ? orig2.length : len, configurable: true }); } catch (e) {}
    native.add(f);
    return f;
  };
  // NB: `nativize` stays a closure-local — exposing it as a global (window.__nativize) would itself
  // be a tamper tell. Every shim site that needs it is inside this same IIFE.
  const mark = (fn, name) => {
    if (typeof fn !== "function") return;
    // Don't rename an already-marked function: setInterval/clearInterval/
    // cancelAnimationFrame alias setTimeout/clearTimeout (same object), so the
    // canonical name set first must win.
    if (name && !native.has(fn)) {
      try { Object.defineProperty(fn, "name", { value: name, configurable: true }); } catch (e) {}
    }
    native.add(fn);
  };
  mark(setTimeout, "setTimeout");
  mark(clearTimeout, "clearTimeout");
  mark(requestAnimationFrame, "requestAnimationFrame");
  mark(queueMicrotask, "queueMicrotask");
  mark(fetch, "fetch");
  mark(setInterval); mark(clearInterval);
  mark(cancelAnimationFrame, "cancelAnimationFrame"); // now its own fn, not a clearTimeout alias
  // Event API: the vendored addEventListener/removeEventListener/dispatchEvent are `function`
  // shims — their `.toString()` leaks source AND they carry an own `prototype` (native methods
  // don't), both tamper tells BotGuard reads. Replace with prototype-less native-shaped forwarders
  // on their OWN object (window/document may hold their own copies; EventTarget.prototype the
  // shared one — reassign wherever it's an own prop so we don't add a new own prop that itself is
  // a tell).
  for (const name of ["addEventListener", "removeEventListener", "dispatchEvent"]) {
    const owners = [globalThis, globalThis.document, globalThis.EventTarget && globalThis.EventTarget.prototype];
    for (const o of owners) {
      try {
        if (o && Object.prototype.hasOwnProperty.call(o, name) && typeof o[name] === "function") {
          o[name] = nativize(o[name], name, name === "dispatchEvent" ? 1 : 2);
        }
      } catch (e) {}
    }
  }
  if (globalThis.Headers) mark(globalThis.Headers, "Headers");
  const nav = globalThis.navigator;
  if (nav && nav.clipboard) { mark(nav.clipboard.writeText, "writeText"); mark(nav.clipboard.readText, "readText"); }
  // Navigator.prototype now holds all 84 members (getters + methods). Native-mask every one so a
  // collector reading `Object.getOwnPropertyDescriptor(Navigator.prototype, k).get.toString()` or
  // a method's `.toString()` sees "[native code]", not JS source. (The getters/methods are arrows,
  // so they already have no own `prototype` — matching native shape.)
  try {
    const np = globalThis.Navigator && globalThis.Navigator.prototype;
    if (np) {
      mark(globalThis.Navigator, "Navigator");
      for (const k of Object.getOwnPropertyNames(np)) {
        const d = Object.getOwnPropertyDescriptor(np, k);
        if (!d) continue;
        if (typeof d.get === "function") mark(d.get, "get " + k);
        if (typeof d.set === "function") mark(d.set, "set " + k);
        if (typeof d.value === "function") mark(d.value, k);
      }
    }
  } catch (e) {}
  // window.postMessage + trustedTypes are JS shims (see their defs); a collector reading
  // their `.toString()` must see native source, like every other shim.
  if (typeof globalThis.postMessage === "function") mark(globalThis.postMessage, "postMessage");
  // Native-mark the message constructors — they're defined (not created by the NAMES loop), so
  // their `.toString()` would otherwise leak JS source (a tamper tell). Also mark MessagePort's
  // prototype methods so `port.postMessage.toString()` reads native.
  for (const __c of [globalThis.MessageChannel, globalThis.MessagePort, globalThis.BroadcastChannel, globalThis.MessageEvent]) {
    if (typeof __c === "function") mark(__c, __c.name);
  }
  if (globalThis.MessagePort && globalThis.MessagePort.prototype) {
    for (const __m of ["postMessage", "start", "close", "addEventListener", "removeEventListener"]) {
      mark(globalThis.MessagePort.prototype[__m], __m);
    }
  }
  if (globalThis.trustedTypes && typeof globalThis.trustedTypes.createPolicy === "function") mark(globalThis.trustedTypes.createPolicy, "createPolicy");

  // ── Structural browser-surface fidelity ──────────────────────────────────────
  // A no-Chromium engine exposes navigator/screen/document as plain object literals
  // (brand "[object Object]", data props, no host prototypes). A deep fingerprinter
  // (Botguard/reCAPTCHA-class) reads WebIDL brands (Object.prototype.toString), native
  // accessor getters up the prototype chain, and native method sources — all of which a
  // synthetic DOM fails. This block re-homes our synthetic globals behind correctly-named
  // host prototypes carrying NATIVE-marked accessor getters (via the same toString WeakSet
  // above), so the passive/consistency surface reads like real Chrome. It cannot forge the
  // active render tier (canvas/WebGL pixels, real layout cascade, font metrics) — those stay
  // honest gaps. Every section is independently guarded so a failure can't break hydration.
  const G = globalThis;
  const guard = (fn) => { try { fn(); } catch (e) {} };
  const tag = (obj, name) => { if (obj) Object.defineProperty(obj, Symbol.toStringTag, { value: name, configurable: true }); };
  // Event.isTrusted as a real Chrome-shaped PROTOTYPE ACCESSOR (Chrome exposes it on
  // Event.prototype, read-only, not as an own instance prop). The vendored Event ctor sets an
  // own `isTrusted=false`; we install a proto getter reading a hidden `__trusted` flag so a
  // synthesized human-input event marked trusted reads `true`, while ordinary script events stay
  // false. A trusted-dispatch helper (see the human-input synthesizer) deletes the own prop and
  // sets `__trusted`, leaving isTrusted only on the prototype — matching Chrome's descriptor
  // shape (an anti-bot check reads Object.getOwnPropertyDescriptor(Event.prototype,'isTrusted')).
  guard(() => {
    if (typeof G.Event === "function" && G.Event.prototype) {
      const get = function isTrusted() { return this.__trusted === true; };
      mark(get, "get isTrusted");
      Object.defineProperty(G.Event.prototype, "isTrusted", { get, enumerable: true, configurable: true });
    }
  });
  // document.readyState as a MUTABLE accessor backed by a hidden Symbol slot, so the render loop
  // can drive the real page-load sequence (loading → interactive → complete) and fire the matching
  // events. The vendored binding hard-codes 'complete'; a collector that watches readyState
  // transitions + gates init on DOMContentLoaded/load (google's homepage does) saw a frozen
  // 'complete' and no lifecycle events on the main document/window — init never ran. Default
  // (slot unset, e.g. the sync run_with_dom path) still reads 'complete', preserving old behavior.
  guard(() => {
    const SLOT = Symbol.for("__ts_rs");
    const get = function readyState() { const v = G.document[SLOT]; return v === undefined ? "complete" : v; };
    mark(get, "get readyState");
    Object.defineProperty(G.document, "readyState", { get, configurable: true, enumerable: true });
  });
  // A native-reporting getter (its source reads "[native code]" via the trap above).
  const nativeGetter = (val) => { const g = function () { return val; }; mark(g); return g; };
  // Insert a correctly-named host prototype between `obj` and its current prototype, carrying
  // native accessor getters for `keys` (values snapshotted from the instance). Own data props
  // are LEFT in place — reads still hit them; the deep-probe's chain walk (which skips own
  // props) finds the native getter. `dropOwn` removes named own props (needed for `webdriver`,
  // whose tamper check fires on ANY own descriptor).
  const hostInterface = (obj, ctorName, keys, dropOwn) => {
    if (!obj) return;
    const ctor = ({ [ctorName]: function () {} })[ctorName]; // .name === ctorName
    const proto = Object.create(Object.getPrototypeOf(obj) || Object.prototype);
    Object.defineProperty(proto, "constructor", { value: ctor, configurable: true });
    try { ctor.prototype = proto; } catch (e) {} // a function's own `prototype` is writable but non-configurable
    mark(ctor, ctorName);
    tag(proto, ctorName);
    G[ctorName] = G[ctorName] || ctor; // expose the constructor (real browsers do)
    for (const k of keys) {
      let val; try { val = obj[k]; } catch (e) { val = undefined; }
      Object.defineProperty(proto, k, { get: nativeGetter(val), enumerable: true, configurable: true });
    }
    for (const k of (dropOwn || [])) { try { delete obj[k]; } catch (e) {} }
    Object.setPrototypeOf(obj, proto);
  };

  // NB: navigator is now a full `Navigator` instance built earlier — zero own props, all 84
  // members (incl. userAgent/platform/webdriver) as native-marked getters on `Navigator.prototype`
  // (marked in the loop above). So the old `hostInterface(nav, "Navigator", …)` re-parenting is
  // GONE — it replaced the real 84-member prototype with a thin 13-getter one (a structural tell).
  // `webdriver` is already a native getter returning false with no own property.

  // screen → Screen.prototype (6 native getters); reserve OS chrome so avail<full (screen-no-os-chrome).
  guard(() => {
    const scr = G.screen;
    if (scr) {
      if (scr.availHeight === scr.height) { try { scr.availHeight = scr.height - 25; } catch (e) {} }
      hostInterface(scr, "Screen", ["width", "height", "availWidth", "availHeight", "colorDepth", "pixelDepth"]);
    }
  });

  // canvas / WebGL fidelity. Two tells the vendored (synthetic) canvas leaks: (a) its 2D +
  // WebGL context methods are JS closures whose `.toString()` reveals source (anti-tamper
  // flag), and (b) the WebGL identity is SwiftShader ("ANGLE (Google, Vulkan… SwiftShader)")
  // — the classic headless/VM signal — with lower limits + fewer extensions than a real GPU.
  // Wrap getContext to (1) spoof the WebGL surface to this host's real Chrome/ANGLE-Metal
  // profile (measured live: Apple M4 Pro via ANGLE Metal) and (2) native-mark every own
  // method on the returned context (they're per-instance own funcs, so the WeakSet mark must
  // run per context). Canvas prototype readback methods (toDataURL/toBlob/getContext) are
  // marked once. Pixel VALUES stay honest — this fixes the identity + toString tells, not the
  // render hash (see the raster-backed toDataURL below for the byte-size fix).
  guard(() => {
    const canvasProto = Object.getPrototypeOf(G.document.createElement("canvas"));
    if (!canvasProto || typeof canvasProto.getContext !== "function" || native.has(canvasProto.getContext)) return;
    // Real-Chrome WebGL extension lists for this host (webgl vs webgl2 differ).
    const EXT1 = ["ANGLE_instanced_arrays","EXT_blend_minmax","EXT_clip_control","EXT_color_buffer_half_float","EXT_depth_clamp","EXT_disjoint_timer_query","EXT_float_blend","EXT_frag_depth","EXT_polygon_offset_clamp","EXT_sRGB","EXT_shader_texture_lod","EXT_texture_compression_bptc","EXT_texture_compression_rgtc","EXT_texture_filter_anisotropic","EXT_texture_mirror_clamp_to_edge","KHR_parallel_shader_compile","OES_element_index_uint","OES_fbo_render_mipmap","OES_standard_derivatives","OES_texture_float","OES_texture_float_linear","OES_texture_half_float","OES_texture_half_float_linear","OES_vertex_array_object","WEBGL_blend_func_extended","WEBGL_color_buffer_float","WEBGL_compressed_texture_astc","WEBGL_compressed_texture_etc","WEBGL_compressed_texture_etc1","WEBGL_compressed_texture_pvrtc","WEBGL_compressed_texture_s3tc","WEBGL_compressed_texture_s3tc_srgb","WEBGL_debug_renderer_info","WEBGL_debug_shaders","WEBGL_depth_texture","WEBGL_draw_buffers","WEBGL_lose_context","WEBGL_multi_draw","WEBGL_polygon_mode"];
    const EXT2 = ["EXT_clip_control","EXT_color_buffer_float","EXT_color_buffer_half_float","EXT_conservative_depth","EXT_depth_clamp","EXT_disjoint_timer_query_webgl2","EXT_float_blend","EXT_polygon_offset_clamp","EXT_render_snorm","EXT_texture_compression_bptc","EXT_texture_compression_rgtc","EXT_texture_filter_anisotropic","EXT_texture_mirror_clamp_to_edge","EXT_texture_norm16","KHR_parallel_shader_compile","NV_shader_noperspective_interpolation","OES_draw_buffers_indexed","OES_sample_variables","OES_shader_multisample_interpolation","OES_texture_float_linear","WEBGL_blend_func_extended","WEBGL_clip_cull_distance","WEBGL_compressed_texture_astc","WEBGL_compressed_texture_etc","WEBGL_compressed_texture_etc1","WEBGL_compressed_texture_pvrtc","WEBGL_compressed_texture_s3tc","WEBGL_compressed_texture_s3tc_srgb","WEBGL_debug_renderer_info","WEBGL_debug_shaders","WEBGL_lose_context","WEBGL_multi_draw","WEBGL_polygon_mode","WEBGL_provoking_vertex","WEBGL_render_shared_exponent","WEBGL_stencil_texturing"];
    // Full WebGL enum constants (captured from live Chrome: 298 for WebGL1, +255 for WebGL2).
    // A real gl context exposes ALL of these on its prototype; ours had only the handful patchGl
    // set for getParameter — so `gl.VERTEX_SHADER` etc. were undefined, both a fingerprint tell
    // AND a functional break (real WebGL code + the GPU bridge get `undefined` enum args → the
    // draw fails and readPixels returns the clear color). Applied to the context prototype below.
    const GL_CONST1 = {ACTIVE_ATTRIBUTES:35721,ACTIVE_TEXTURE:34016,ACTIVE_UNIFORMS:35718,ALIASED_LINE_WIDTH_RANGE:33902,
      ALIASED_POINT_SIZE_RANGE:33901,ALPHA:6406,ALPHA_BITS:3413,ALWAYS:519,ARRAY_BUFFER:34962,
      ARRAY_BUFFER_BINDING:34964,ATTACHED_SHADERS:35717,BACK:1029,BLEND:3042,BLEND_COLOR:32773,
      BLEND_DST_ALPHA:32970,BLEND_DST_RGB:32968,BLEND_EQUATION:32777,BLEND_EQUATION_ALPHA:34877,
      BLEND_EQUATION_RGB:32777,BLEND_SRC_ALPHA:32971,BLEND_SRC_RGB:32969,BLUE_BITS:3412,BOOL:35670,BOOL_VEC2:35671,
      BOOL_VEC3:35672,BOOL_VEC4:35673,BROWSER_DEFAULT_WEBGL:37444,BUFFER_SIZE:34660,BUFFER_USAGE:34661,BYTE:5120,
      CCW:2305,CLAMP_TO_EDGE:33071,COLOR_ATTACHMENT0:36064,COLOR_BUFFER_BIT:16384,COLOR_CLEAR_VALUE:3106,
      COLOR_WRITEMASK:3107,COMPILE_STATUS:35713,COMPRESSED_TEXTURE_FORMATS:34467,CONSTANT_ALPHA:32771,
      CONSTANT_COLOR:32769,CONTEXT_LOST_WEBGL:37442,CULL_FACE:2884,CULL_FACE_MODE:2885,CURRENT_PROGRAM:35725,
      CURRENT_VERTEX_ATTRIB:34342,CW:2304,DECR:7683,DECR_WRAP:34056,DELETE_STATUS:35712,DEPTH_ATTACHMENT:36096,
      DEPTH_BITS:3414,DEPTH_BUFFER_BIT:256,DEPTH_CLEAR_VALUE:2931,DEPTH_COMPONENT:6402,DEPTH_COMPONENT16:33189,
      DEPTH_FUNC:2932,DEPTH_RANGE:2928,DEPTH_STENCIL:34041,DEPTH_STENCIL_ATTACHMENT:33306,DEPTH_TEST:2929,
      DEPTH_WRITEMASK:2930,DITHER:3024,DONT_CARE:4352,DST_ALPHA:772,DST_COLOR:774,DYNAMIC_DRAW:35048,
      ELEMENT_ARRAY_BUFFER:34963,ELEMENT_ARRAY_BUFFER_BINDING:34965,EQUAL:514,FASTEST:4353,FLOAT:5126,
      FLOAT_MAT2:35674,FLOAT_MAT3:35675,FLOAT_MAT4:35676,FLOAT_VEC2:35664,FLOAT_VEC3:35665,FLOAT_VEC4:35666,
      FRAGMENT_SHADER:35632,FRAMEBUFFER:36160,FRAMEBUFFER_ATTACHMENT_OBJECT_NAME:36049,
      FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE:36048,FRAMEBUFFER_ATTACHMENT_TEXTURE_CUBE_MAP_FACE:36051,
      FRAMEBUFFER_ATTACHMENT_TEXTURE_LEVEL:36050,FRAMEBUFFER_BINDING:36006,FRAMEBUFFER_COMPLETE:36053,
      FRAMEBUFFER_INCOMPLETE_ATTACHMENT:36054,FRAMEBUFFER_INCOMPLETE_DIMENSIONS:36057,
      FRAMEBUFFER_INCOMPLETE_MISSING_ATTACHMENT:36055,FRAMEBUFFER_UNSUPPORTED:36061,FRONT:1028,FRONT_AND_BACK:1032,
      FRONT_FACE:2886,FUNC_ADD:32774,FUNC_REVERSE_SUBTRACT:32779,FUNC_SUBTRACT:32778,GENERATE_MIPMAP_HINT:33170,
      GEQUAL:518,GREATER:516,GREEN_BITS:3411,HIGH_FLOAT:36338,HIGH_INT:36341,
      IMPLEMENTATION_COLOR_READ_FORMAT:35739,IMPLEMENTATION_COLOR_READ_TYPE:35738,INCR:7682,INCR_WRAP:34055,
      INT:5124,INT_VEC2:35667,INT_VEC3:35668,INT_VEC4:35669,INVALID_ENUM:1280,INVALID_FRAMEBUFFER_OPERATION:1286,
      INVALID_OPERATION:1282,INVALID_VALUE:1281,INVERT:5386,KEEP:7680,LEQUAL:515,LESS:513,LINEAR:9729,
      LINEAR_MIPMAP_LINEAR:9987,LINEAR_MIPMAP_NEAREST:9985,LINES:1,LINE_LOOP:2,LINE_STRIP:3,LINE_WIDTH:2849,
      LINK_STATUS:35714,LOW_FLOAT:36336,LOW_INT:36339,LUMINANCE:6409,LUMINANCE_ALPHA:6410,
      MAX_COMBINED_TEXTURE_IMAGE_UNITS:35661,MAX_CUBE_MAP_TEXTURE_SIZE:34076,MAX_FRAGMENT_UNIFORM_VECTORS:36349,
      MAX_RENDERBUFFER_SIZE:34024,MAX_TEXTURE_IMAGE_UNITS:34930,MAX_TEXTURE_SIZE:3379,MAX_VARYING_VECTORS:36348,
      MAX_VERTEX_ATTRIBS:34921,MAX_VERTEX_TEXTURE_IMAGE_UNITS:35660,MAX_VERTEX_UNIFORM_VECTORS:36347,
      MAX_VIEWPORT_DIMS:3386,MEDIUM_FLOAT:36337,MEDIUM_INT:36340,MIRRORED_REPEAT:33648,NEAREST:9728,
      NEAREST_MIPMAP_LINEAR:9986,NEAREST_MIPMAP_NEAREST:9984,NEVER:512,NICEST:4354,NONE:0,NOTEQUAL:517,NO_ERROR:0,
      ONE:1,ONE_MINUS_CONSTANT_ALPHA:32772,ONE_MINUS_CONSTANT_COLOR:32770,ONE_MINUS_DST_ALPHA:773,
      ONE_MINUS_DST_COLOR:775,ONE_MINUS_SRC_ALPHA:771,ONE_MINUS_SRC_COLOR:769,OUT_OF_MEMORY:1285,
      PACK_ALIGNMENT:3333,POINTS:0,POLYGON_OFFSET_FACTOR:32824,POLYGON_OFFSET_FILL:32823,
      POLYGON_OFFSET_UNITS:10752,RED_BITS:3410,RENDERBUFFER:36161,RENDERBUFFER_ALPHA_SIZE:36179,
      RENDERBUFFER_BINDING:36007,RENDERBUFFER_BLUE_SIZE:36178,RENDERBUFFER_DEPTH_SIZE:36180,
      RENDERBUFFER_GREEN_SIZE:36177,RENDERBUFFER_HEIGHT:36163,RENDERBUFFER_INTERNAL_FORMAT:36164,
      RENDERBUFFER_RED_SIZE:36176,RENDERBUFFER_STENCIL_SIZE:36181,RENDERBUFFER_WIDTH:36162,RENDERER:7937,
      REPEAT:10497,REPLACE:7681,RGB:6407,RGB565:36194,RGB5_A1:32855,RGB8:32849,RGBA:6408,RGBA4:32854,RGBA8:32856,
      SAMPLER_2D:35678,SAMPLER_CUBE:35680,SAMPLES:32937,SAMPLE_ALPHA_TO_COVERAGE:32926,SAMPLE_BUFFERS:32936,
      SAMPLE_COVERAGE:32928,SAMPLE_COVERAGE_INVERT:32939,SAMPLE_COVERAGE_VALUE:32938,SCISSOR_BOX:3088,
      SCISSOR_TEST:3089,SHADER_TYPE:35663,SHADING_LANGUAGE_VERSION:35724,SHORT:5122,SRC_ALPHA:770,
      SRC_ALPHA_SATURATE:776,SRC_COLOR:768,STATIC_DRAW:35044,STENCIL_ATTACHMENT:36128,STENCIL_BACK_FAIL:34817,
      STENCIL_BACK_FUNC:34816,STENCIL_BACK_PASS_DEPTH_FAIL:34818,STENCIL_BACK_PASS_DEPTH_PASS:34819,
      STENCIL_BACK_REF:36003,STENCIL_BACK_VALUE_MASK:36004,STENCIL_BACK_WRITEMASK:36005,STENCIL_BITS:3415,
      STENCIL_BUFFER_BIT:1024,STENCIL_CLEAR_VALUE:2961,STENCIL_FAIL:2964,STENCIL_FUNC:2962,STENCIL_INDEX8:36168,
      STENCIL_PASS_DEPTH_FAIL:2965,STENCIL_PASS_DEPTH_PASS:2966,STENCIL_REF:2967,STENCIL_TEST:2960,
      STENCIL_VALUE_MASK:2963,STENCIL_WRITEMASK:2968,STREAM_DRAW:35040,SUBPIXEL_BITS:3408,TEXTURE:5890,
      TEXTURE0:33984,TEXTURE1:33985,TEXTURE10:33994,TEXTURE11:33995,TEXTURE12:33996,TEXTURE13:33997,
      TEXTURE14:33998,TEXTURE15:33999,TEXTURE16:34000,TEXTURE17:34001,TEXTURE18:34002,TEXTURE19:34003,
      TEXTURE2:33986,TEXTURE20:34004,TEXTURE21:34005,TEXTURE22:34006,TEXTURE23:34007,TEXTURE24:34008,
      TEXTURE25:34009,TEXTURE26:34010,TEXTURE27:34011,TEXTURE28:34012,TEXTURE29:34013,TEXTURE3:33987,
      TEXTURE30:34014,TEXTURE31:34015,TEXTURE4:33988,TEXTURE5:33989,TEXTURE6:33990,TEXTURE7:33991,TEXTURE8:33992,
      TEXTURE9:33993,TEXTURE_2D:3553,TEXTURE_BINDING_2D:32873,TEXTURE_BINDING_CUBE_MAP:34068,
      TEXTURE_CUBE_MAP:34067,TEXTURE_CUBE_MAP_NEGATIVE_X:34070,TEXTURE_CUBE_MAP_NEGATIVE_Y:34072,
      TEXTURE_CUBE_MAP_NEGATIVE_Z:34074,TEXTURE_CUBE_MAP_POSITIVE_X:34069,TEXTURE_CUBE_MAP_POSITIVE_Y:34071,
      TEXTURE_CUBE_MAP_POSITIVE_Z:34073,TEXTURE_MAG_FILTER:10240,TEXTURE_MIN_FILTER:10241,TEXTURE_WRAP_S:10242,
      TEXTURE_WRAP_T:10243,TRIANGLES:4,TRIANGLE_FAN:6,TRIANGLE_STRIP:5,UNPACK_ALIGNMENT:3317,
      UNPACK_COLORSPACE_CONVERSION_WEBGL:37443,UNPACK_FLIP_Y_WEBGL:37440,UNPACK_PREMULTIPLY_ALPHA_WEBGL:37441,
      UNSIGNED_BYTE:5121,UNSIGNED_INT:5125,UNSIGNED_SHORT:5123,UNSIGNED_SHORT_4_4_4_4:32819,
      UNSIGNED_SHORT_5_5_5_1:32820,UNSIGNED_SHORT_5_6_5:33635,VALIDATE_STATUS:35715,VENDOR:7936,VERSION:7938,
      VERTEX_ATTRIB_ARRAY_BUFFER_BINDING:34975,VERTEX_ATTRIB_ARRAY_ENABLED:34338,
      VERTEX_ATTRIB_ARRAY_NORMALIZED:34922,VERTEX_ATTRIB_ARRAY_POINTER:34373,VERTEX_ATTRIB_ARRAY_SIZE:34339,
      VERTEX_ATTRIB_ARRAY_STRIDE:34340,VERTEX_ATTRIB_ARRAY_TYPE:34341,VERTEX_SHADER:35633,VIEWPORT:2978,ZERO:0};
    const GL_CONST2 = {ACTIVE_UNIFORM_BLOCKS:35382,ALREADY_SIGNALED:37146,ANY_SAMPLES_PASSED:35887,
      ANY_SAMPLES_PASSED_CONSERVATIVE:36202,COLOR:6144,COLOR_ATTACHMENT1:36065,COLOR_ATTACHMENT10:36074,
      COLOR_ATTACHMENT11:36075,COLOR_ATTACHMENT12:36076,COLOR_ATTACHMENT13:36077,COLOR_ATTACHMENT14:36078,
      COLOR_ATTACHMENT15:36079,COLOR_ATTACHMENT2:36066,COLOR_ATTACHMENT3:36067,COLOR_ATTACHMENT4:36068,
      COLOR_ATTACHMENT5:36069,COLOR_ATTACHMENT6:36070,COLOR_ATTACHMENT7:36071,COLOR_ATTACHMENT8:36072,
      COLOR_ATTACHMENT9:36073,COMPARE_REF_TO_TEXTURE:34894,CONDITION_SATISFIED:37148,COPY_READ_BUFFER:36662,
      COPY_READ_BUFFER_BINDING:36662,COPY_WRITE_BUFFER:36663,COPY_WRITE_BUFFER_BINDING:36663,CURRENT_QUERY:34917,
      DEPTH:6145,DEPTH24_STENCIL8:35056,DEPTH32F_STENCIL8:36013,DEPTH_COMPONENT24:33190,DEPTH_COMPONENT32F:36012,
      DRAW_BUFFER0:34853,DRAW_BUFFER1:34854,DRAW_BUFFER10:34863,DRAW_BUFFER11:34864,DRAW_BUFFER12:34865,
      DRAW_BUFFER13:34866,DRAW_BUFFER14:34867,DRAW_BUFFER15:34868,DRAW_BUFFER2:34855,DRAW_BUFFER3:34856,
      DRAW_BUFFER4:34857,DRAW_BUFFER5:34858,DRAW_BUFFER6:34859,DRAW_BUFFER7:34860,DRAW_BUFFER8:34861,
      DRAW_BUFFER9:34862,DRAW_FRAMEBUFFER:36009,DRAW_FRAMEBUFFER_BINDING:36006,DYNAMIC_COPY:35050,
      DYNAMIC_READ:35049,FLOAT_32_UNSIGNED_INT_24_8_REV:36269,FRAGMENT_SHADER_DERIVATIVE_HINT:35723,
      FRAMEBUFFER_ATTACHMENT_ALPHA_SIZE:33301,FRAMEBUFFER_ATTACHMENT_BLUE_SIZE:33300,
      FRAMEBUFFER_ATTACHMENT_COLOR_ENCODING:33296,FRAMEBUFFER_ATTACHMENT_COMPONENT_TYPE:33297,
      FRAMEBUFFER_ATTACHMENT_DEPTH_SIZE:33302,FRAMEBUFFER_ATTACHMENT_GREEN_SIZE:33299,
      FRAMEBUFFER_ATTACHMENT_RED_SIZE:33298,FRAMEBUFFER_ATTACHMENT_STENCIL_SIZE:33303,
      FRAMEBUFFER_ATTACHMENT_TEXTURE_LAYER:36052,FRAMEBUFFER_DEFAULT:33304,
      FRAMEBUFFER_INCOMPLETE_MULTISAMPLE:36182,HALF_FLOAT:5131,INTERLEAVED_ATTRIBS:35980,INT_2_10_10_10_REV:36255,
      INT_SAMPLER_2D:36298,INT_SAMPLER_2D_ARRAY:36303,INT_SAMPLER_3D:36299,INT_SAMPLER_CUBE:36300,
      INVALID_INDEX:4294967295,MAX:32776,MAX_3D_TEXTURE_SIZE:32883,MAX_ARRAY_TEXTURE_LAYERS:35071,
      MAX_CLIENT_WAIT_TIMEOUT_WEBGL:37447,MAX_COLOR_ATTACHMENTS:36063,
      MAX_COMBINED_FRAGMENT_UNIFORM_COMPONENTS:35379,MAX_COMBINED_UNIFORM_BLOCKS:35374,
      MAX_COMBINED_VERTEX_UNIFORM_COMPONENTS:35377,MAX_DRAW_BUFFERS:34852,MAX_ELEMENTS_INDICES:33001,
      MAX_ELEMENTS_VERTICES:33000,MAX_ELEMENT_INDEX:36203,MAX_FRAGMENT_INPUT_COMPONENTS:37157,
      MAX_FRAGMENT_UNIFORM_BLOCKS:35373,MAX_FRAGMENT_UNIFORM_COMPONENTS:35657,MAX_PROGRAM_TEXEL_OFFSET:35077,
      MAX_SAMPLES:36183,MAX_SERVER_WAIT_TIMEOUT:37137,MAX_TEXTURE_LOD_BIAS:34045,
      MAX_TRANSFORM_FEEDBACK_INTERLEAVED_COMPONENTS:35978,MAX_TRANSFORM_FEEDBACK_SEPARATE_ATTRIBS:35979,
      MAX_TRANSFORM_FEEDBACK_SEPARATE_COMPONENTS:35968,MAX_UNIFORM_BLOCK_SIZE:35376,
      MAX_UNIFORM_BUFFER_BINDINGS:35375,MAX_VARYING_COMPONENTS:35659,MAX_VERTEX_OUTPUT_COMPONENTS:37154,
      MAX_VERTEX_UNIFORM_BLOCKS:35371,MAX_VERTEX_UNIFORM_COMPONENTS:35658,MIN:32775,MIN_PROGRAM_TEXEL_OFFSET:35076,
      OBJECT_TYPE:37138,PACK_ROW_LENGTH:3330,PACK_SKIP_PIXELS:3332,PACK_SKIP_ROWS:3331,PIXEL_PACK_BUFFER:35051,
      PIXEL_PACK_BUFFER_BINDING:35053,PIXEL_UNPACK_BUFFER:35052,PIXEL_UNPACK_BUFFER_BINDING:35055,
      QUERY_RESULT:34918,QUERY_RESULT_AVAILABLE:34919,R11F_G11F_B10F:35898,R16F:33325,R16I:33331,R16UI:33332,
      R32F:33326,R32I:33333,R32UI:33334,R8:33321,R8I:33329,R8UI:33330,R8_SNORM:36756,RASTERIZER_DISCARD:35977,
      READ_BUFFER:3074,READ_FRAMEBUFFER:36008,READ_FRAMEBUFFER_BINDING:36010,RED:6403,RED_INTEGER:36244,
      RENDERBUFFER_SAMPLES:36011,RG:33319,RG16F:33327,RG16I:33337,RG16UI:33338,RG32F:33328,RG32I:33339,
      RG32UI:33340,RG8:33323,RG8I:33335,RG8UI:33336,RG8_SNORM:36757,RGB10_A2:32857,RGB10_A2UI:36975,RGB16F:34843,
      RGB16I:36233,RGB16UI:36215,RGB32F:34837,RGB32I:36227,RGB32UI:36209,RGB8I:36239,RGB8UI:36221,RGB8_SNORM:36758,
      RGB9_E5:35901,RGBA16F:34842,RGBA16I:36232,RGBA16UI:36214,RGBA32F:34836,RGBA32I:36226,RGBA32UI:36208,
      RGBA8I:36238,RGBA8UI:36220,RGBA8_SNORM:36759,RGBA_INTEGER:36249,RGB_INTEGER:36248,RG_INTEGER:33320,
      SAMPLER_2D_ARRAY:36289,SAMPLER_2D_ARRAY_SHADOW:36292,SAMPLER_2D_SHADOW:35682,SAMPLER_3D:35679,
      SAMPLER_BINDING:35097,SAMPLER_CUBE_SHADOW:36293,SEPARATE_ATTRIBS:35981,SIGNALED:37145,
      SIGNED_NORMALIZED:36764,SRGB:35904,SRGB8:35905,SRGB8_ALPHA8:35907,STATIC_COPY:35046,STATIC_READ:35045,
      STENCIL:6146,STREAM_COPY:35042,STREAM_READ:35041,SYNC_CONDITION:37139,SYNC_FENCE:37142,SYNC_FLAGS:37141,
      SYNC_FLUSH_COMMANDS_BIT:1,SYNC_GPU_COMMANDS_COMPLETE:37143,SYNC_STATUS:37140,TEXTURE_2D_ARRAY:35866,
      TEXTURE_3D:32879,TEXTURE_BASE_LEVEL:33084,TEXTURE_BINDING_2D_ARRAY:35869,TEXTURE_BINDING_3D:32874,
      TEXTURE_COMPARE_FUNC:34893,TEXTURE_COMPARE_MODE:34892,TEXTURE_IMMUTABLE_FORMAT:37167,
      TEXTURE_IMMUTABLE_LEVELS:33503,TEXTURE_MAX_LEVEL:33085,TEXTURE_MAX_LOD:33083,TEXTURE_MIN_LOD:33082,
      TEXTURE_WRAP_R:32882,TIMEOUT_EXPIRED:37147,TIMEOUT_IGNORED:-1,TRANSFORM_FEEDBACK:36386,
      TRANSFORM_FEEDBACK_ACTIVE:36388,TRANSFORM_FEEDBACK_BINDING:36389,TRANSFORM_FEEDBACK_BUFFER:35982,
      TRANSFORM_FEEDBACK_BUFFER_BINDING:35983,TRANSFORM_FEEDBACK_BUFFER_MODE:35967,
      TRANSFORM_FEEDBACK_BUFFER_SIZE:35973,TRANSFORM_FEEDBACK_BUFFER_START:35972,TRANSFORM_FEEDBACK_PAUSED:36387,
      TRANSFORM_FEEDBACK_PRIMITIVES_WRITTEN:35976,TRANSFORM_FEEDBACK_VARYINGS:35971,UNIFORM_ARRAY_STRIDE:35388,
      UNIFORM_BLOCK_ACTIVE_UNIFORMS:35394,UNIFORM_BLOCK_ACTIVE_UNIFORM_INDICES:35395,UNIFORM_BLOCK_BINDING:35391,
      UNIFORM_BLOCK_DATA_SIZE:35392,UNIFORM_BLOCK_INDEX:35386,UNIFORM_BLOCK_REFERENCED_BY_FRAGMENT_SHADER:35398,
      UNIFORM_BLOCK_REFERENCED_BY_VERTEX_SHADER:35396,UNIFORM_BUFFER:35345,UNIFORM_BUFFER_BINDING:35368,
      UNIFORM_BUFFER_OFFSET_ALIGNMENT:35380,UNIFORM_BUFFER_SIZE:35370,UNIFORM_BUFFER_START:35369,
      UNIFORM_IS_ROW_MAJOR:35390,UNIFORM_MATRIX_STRIDE:35389,UNIFORM_OFFSET:35387,UNIFORM_SIZE:35384,
      UNIFORM_TYPE:35383,UNPACK_IMAGE_HEIGHT:32878,UNPACK_ROW_LENGTH:3314,UNPACK_SKIP_IMAGES:32877,
      UNPACK_SKIP_PIXELS:3316,UNPACK_SKIP_ROWS:3315,UNSIGNALED:37144,UNSIGNED_INT_10F_11F_11F_REV:35899,
      UNSIGNED_INT_24_8:34042,UNSIGNED_INT_2_10_10_10_REV:33640,UNSIGNED_INT_5_9_9_9_REV:35902,
      UNSIGNED_INT_SAMPLER_2D:36306,UNSIGNED_INT_SAMPLER_2D_ARRAY:36311,UNSIGNED_INT_SAMPLER_3D:36307,
      UNSIGNED_INT_SAMPLER_CUBE:36308,UNSIGNED_INT_VEC2:36294,UNSIGNED_INT_VEC3:36295,UNSIGNED_INT_VEC4:36296,
      UNSIGNED_NORMALIZED:35863,VERTEX_ARRAY_BINDING:34229,VERTEX_ATTRIB_ARRAY_DIVISOR:35070,
      VERTEX_ATTRIB_ARRAY_INTEGER:35069,WAIT_FAILED:37149};
    const UNMASKED_VENDOR = 0x9245, UNMASKED_RENDERER = 0x9246, ANISO = 0x84ff;
    const patchGl = (gl, is2) => {
      const M = new Map();
      const set = (name, lit, val) => { const k = (gl[name] !== undefined ? gl[name] : lit); M.set(k, val); };
      set("VENDOR", 0x1f00, "WebKit");
      set("RENDERER", 0x1f01, "WebKit WebGL");
      set("VERSION", 0x1f02, is2 ? "WebGL 2.0 (OpenGL ES 3.0 Chromium)" : "WebGL 1.0 (OpenGL ES 2.0 Chromium)");
      set("SHADING_LANGUAGE_VERSION", 0x8b8c, is2 ? "WebGL GLSL ES 3.00 (OpenGL ES GLSL ES 3.0 Chromium)" : "WebGL GLSL ES 1.0 (OpenGL ES GLSL ES 1.0 Chromium)");
      M.set(UNMASKED_VENDOR, "Google Inc. (Apple)");
      M.set(UNMASKED_RENDERER, "ANGLE (Apple, ANGLE Metal Renderer: Apple M4 Pro, Unspecified Version)");
      set("MAX_TEXTURE_SIZE", 0x0d33, 16384);
      set("MAX_CUBE_MAP_TEXTURE_SIZE", 0x851c, 16384);
      set("MAX_RENDERBUFFER_SIZE", 0x84e8, 16384);
      set("MAX_VIEWPORT_DIMS", 0x0d3a, new Int32Array([16384, 16384]));
      set("MAX_VERTEX_ATTRIBS", 0x8869, 16);
      set("MAX_VERTEX_UNIFORM_VECTORS", 0x8dfb, 1024);
      set("MAX_FRAGMENT_UNIFORM_VECTORS", 0x8dfd, 1024);
      set("MAX_VARYING_VECTORS", 0x8dfc, 30);
      set("MAX_VERTEX_TEXTURE_IMAGE_UNITS", 0x8b4c, 16);
      set("MAX_TEXTURE_IMAGE_UNITS", 0x8872, 16);
      set("MAX_COMBINED_TEXTURE_IMAGE_UNITS", 0x8b4d, 32);
      set("ALIASED_LINE_WIDTH_RANGE", 0x846e, new Float32Array([1, 1]));
      set("ALIASED_POINT_SIZE_RANGE", 0x846d, new Float32Array([1, 511]));
      set("RED_BITS", 0x0d52, 8); set("GREEN_BITS", 0x0d53, 8); set("BLUE_BITS", 0x0d54, 8);
      set("ALPHA_BITS", 0x0d55, 8); set("DEPTH_BITS", 0x0d56, 24); set("STENCIL_BITS", 0x0d57, 0);
      M.set(ANISO, 16);
      if (is2) {
        set("MAX_3D_TEXTURE_SIZE", 0x8073, 2048);
        set("MAX_ARRAY_TEXTURE_LAYERS", 0x88ff, 2048);
        set("MAX_DRAW_BUFFERS", 0x8824, 8);
        set("MAX_COLOR_ATTACHMENTS", 0x8cdf, 8);
        set("MAX_SAMPLES", 0x8d57, 4);
        set("MAX_UNIFORM_BUFFER_BINDINGS", 0x8a2f, 32);
      }
      const origGetParam = gl.getParameter ? gl.getParameter.bind(gl) : () => null;
      gl.getParameter = function (p) { return M.has(p) ? M.get(p) : origGetParam(p); };
      gl.getSupportedExtensions = function () { return (is2 ? EXT2 : EXT1).slice(); };
      const origGetExt = gl.getExtension ? gl.getExtension.bind(gl) : () => null;
      gl.getExtension = function (name) {
        if (name === "WEBGL_debug_renderer_info") return { UNMASKED_VENDOR_WEBGL: UNMASKED_VENDOR, UNMASKED_RENDERER_WEBGL: UNMASKED_RENDERER };
        if (name === "EXT_texture_filter_anisotropic") return { MAX_TEXTURE_MAX_ANISOTROPY_EXT: ANISO, TEXTURE_MAX_ANISOTROPY_EXT: 0x84fe };
        const known = (is2 ? EXT2 : EXT1).indexOf(name) >= 0;
        const r = origGetExt(name);
        return r || (known ? {} : null);
      };
    };
    // Native-mark every own function on a context instance (they're per-instance own funcs).
    const markOwn = (obj) => {
      if (!obj) return obj;
      for (const k of Object.getOwnPropertyNames(obj)) {
        let d; try { d = Object.getOwnPropertyDescriptor(obj, k); } catch (e) { continue; }
        if (d && typeof d.value === "function") mark(d.value, k);
      }
      return obj;
    };
    // Live WebGL→GPU bridge: override the drawing/state methods to RECORD each call with its
    // REAL args (buffer bytes + shader source — captured at the live call site, so nothing is
    // lossy) into a batch, and on readPixels flush the batch to the host GPU executor
    // (op_webgl_readback) and fill the destination from genuine GPU pixels. Only installed when a
    // host executor is present; otherwise the vendored synthetic context is left as-is. getShader/
    // Program*Parameter report success so a page's compile/link checks pass. Returns real GPU
    // pixels for the common fingerprint draw (shaders + a vertex buffer + drawArrays + readPixels).
    const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    const enc64 = (u8) => {
      let o = "";
      for (let i = 0; i < u8.length; i += 3) {
        const a = u8[i], b = i + 1 < u8.length ? u8[i + 1] : 0, c = i + 2 < u8.length ? u8[i + 2] : 0;
        o += B64[a >> 2] + B64[((a & 3) << 4) | (b >> 4)];
        o += i + 1 < u8.length ? B64[((b & 15) << 2) | (c >> 6)] : "=";
        o += i + 2 < u8.length ? B64[c & 63] : "=";
      }
      return o;
    };
    const dec64 = (s) => {
      const L = {}; for (let i = 0; i < 64; i++) L[B64[i]] = i;
      const out = []; let buf = 0, bits = 0;
      for (let i = 0; i < s.length; i++) {
        const v = L[s[i]]; if (v === undefined) continue;
        buf = (buf << 6) | v; bits += 6;
        if (bits >= 8) { bits -= 8; out.push((buf >> bits) & 0xff); }
      }
      return out;
    };
    const installWebglRecorder = (gl) => {
      const op = (Deno.core.ops && Deno.core.ops.op_webgl_readback) || null;
      // op_webgl_readback is ALWAYS registered, so `typeof op` can't tell whether a GPU executor
      // is installed. Only override the (vendored synthetic) WebGL context when one actually is —
      // otherwise the default build would replace the context with recording stubs for nothing.
      const avail = Deno.core.ops && Deno.core.ops.op_webgl_available;
      if (!op || !avail || !avail()) return;
      const rec = [];
      let hid = 0, lid = 0;
      const H = () => ({ __h: ++hid });
      const bytesOf = (view) => {
        try {
          if (view instanceof ArrayBuffer) return new Uint8Array(view);
          if (view && view.buffer) return new Uint8Array(view.buffer, view.byteOffset || 0, view.byteLength);
        } catch (e) {}
        return new Uint8Array(0);
      };
      const bufType = (v) => (v instanceof Float32Array ? "f32" : v instanceof Uint16Array ? "u16" : "u8");
      const put = (m, a, id) => { const o = { m, a }; if (id != null) o.id = id; rec.push(o); };
      const origReadPixels = gl.readPixels;
      gl.createShader = function (type) { const h = H(); put("createShader", [type], h.__h); return h; };
      gl.shaderSource = function (sh, src) { put("shaderSource", [sh && sh.__h, String(src)]); };
      gl.compileShader = function (sh) { put("compileShader", [sh && sh.__h]); };
      gl.getShaderParameter = function () { return true; };
      gl.getProgramParameter = function () { return true; };
      gl.getShaderInfoLog = function () { return ""; };
      gl.getProgramInfoLog = function () { return ""; };
      gl.createProgram = function () { const h = H(); put("createProgram", [], h.__h); return h; };
      gl.attachShader = function (p, s) { put("attachShader", [p && p.__h, s && s.__h]); };
      gl.linkProgram = function (p) { put("linkProgram", [p && p.__h]); };
      gl.useProgram = function (p) { put("useProgram", [p && p.__h]); };
      gl.createBuffer = function () { const h = H(); put("createBuffer", [], h.__h); return h; };
      gl.bindBuffer = function (t, b) { put("bindBuffer", [t, b ? b.__h : 0]); };
      gl.bufferData = function (t, data, usage) {
        if (typeof data === "number") put("bufferData", [t, { b64: "", t: "u8" }, usage]);
        else put("bufferData", [t, { b64: enc64(bytesOf(data)), t: bufType(data) }, usage]);
      };
      gl.getAttribLocation = function (p, name) { const loc = lid++; put("attribLocation", [p && p.__h, loc, String(name)]); return loc; };
      gl.enableVertexAttribArray = function (loc) { put("enableVertexAttribArray", [loc]); };
      gl.vertexAttribPointer = function (loc, size, type, norm, stride, offset) { put("vertexAttribPointer", [loc, size, type, !!norm, stride | 0, offset | 0]); };
      gl.getUniformLocation = function (p, name) { return { __p: p && p.__h, __u: String(name) }; };
      const uni = (kind) => function (loc) { const vals = Array.prototype.slice.call(arguments, 1); if (loc) put("uniform", [loc.__p, loc.__u, kind, vals]); };
      gl.uniform1f = uni("1f"); gl.uniform2f = uni("2f"); gl.uniform3f = uni("3f"); gl.uniform4f = uni("4f"); gl.uniform1i = uni("1i");
      gl.uniformMatrix4fv = function (loc, transpose, val) { if (loc) put("uniform", [loc.__p, loc.__u, "Matrix4fv", Array.prototype.slice.call(val || [])]); };
      gl.viewport = function (x, y, w, h) { put("viewport", [x, y, w, h]); };
      gl.clearColor = function (r, g, b, a) { put("clearColor", [r, g, b, a]); };
      gl.clear = function (mask) { put("clear", [mask]); };
      gl.drawArrays = function (mode, first, count) { put("drawArrays", [mode, first, count]); };
      gl.drawElements = function (mode, count, type, offset) { put("drawElements", [mode, count, type, offset | 0]); };
      gl.readPixels = function (x, y, w, h, format, type, dst) {
        try {
          const W = gl.drawingBufferWidth || (gl.canvas && gl.canvas.width) || 300;
          const Hh = gl.drawingBufferHeight || (gl.canvas && gl.canvas.height) || 150;
          const out = op(W, Hh, JSON.stringify(rec));
          if (out && dst && dst.length) {
            const bin = dec64(out); // framebuffer RGBA8, top-left origin, W×Hh
            for (let row = 0; row < h; row++) {
              const srcRow = Hh - 1 - (y + row); // readPixels is bottom-left origin → flip
              for (let col = 0; col < w; col++) {
                const si = (srcRow * W + (x + col)) * 4, di = (row * w + col) * 4;
                for (let c = 0; c < 4; c++) dst[di + c] = si + c < bin.length ? bin[si + c] : 0;
              }
            }
            return;
          }
        } catch (e) {}
        if (typeof origReadPixels === "function") { try { return origReadPixels.call(gl, x, y, w, h, format, type, dst); } catch (e) {} }
      };
    };
    const origGetContext = canvasProto.getContext;
    // getContext returns the SAME context object on every call for a given (canvas, kind),
    // so a page that calls it twice would otherwise re-wrap getParameter/readPixels over our
    // own overrides — double-recording draws and eventually stack-overflowing the bind chain.
    // Track patched contexts so each is spoofed + instrumented exactly once.
    const glPatched = new WeakSet();
    const wrapped = function getContext(kind, opts) {
      const ctx = origGetContext.call(this, kind, opts);
      if (ctx) {
        const k = String(kind || "");
        if ((k === "webgl" || k === "experimental-webgl" || k === "webgl2") && !glPatched.has(ctx)) {
          glPatched.add(ctx);
          try { patchGl(ctx, k === "webgl2"); } catch (e) {}
          try { installWebglRecorder(ctx); } catch (e) {}
          // Give the context its OWN dedicated prototype carrying its methods, then wire it to
          // the named global constructor, so `gl instanceof WebGLRenderingContext` holds and
          // `WebGLRenderingContext.prototype.getParameter` resolves natively. We must NOT reuse
          // the vendored context's shared fallback prototype (a generic object also backing
          // `location` etc.) as the constructor prototype — that would make `location instanceof
          // WebGLRenderingContext` true and collapse the webgl/webgl2 protos onto one, both worse
          // tells. The context methods are per-instance bound closures (they don't read `this`),
          // so moving them onto a private per-context prototype is behaviour-preserving.
          try {
            const ctorName = k === "webgl2" ? "WebGL2RenderingContext" : "WebGLRenderingContext";
            const C = globalThis[ctorName];
            if (C) {
              const proto = {};
              for (const nm of Object.getOwnPropertyNames(ctx)) {
                const dsc = Object.getOwnPropertyDescriptor(ctx, nm);
                if (dsc && typeof dsc.value === "function") {
                  // Prototype-less native shape (no own `prototype`), prototype-resident + marked.
                  const nf = nativize(dsc.value, nm);
                  Object.defineProperty(proto, nm, { value: nf, configurable: true, writable: true });
                  try { delete ctx[nm]; } catch (e) {}
                }
              }
              // The full WebGL enum constants live on the prototype (like Chrome), so `gl.X`
              // resolves for real WebGL code + the recorder captures real enum args (not undefined).
              const consts = k === "webgl2" ? Object.assign({}, GL_CONST1, GL_CONST2) : GL_CONST1;
              for (const cn in consts) {
                if (!(cn in proto)) {
                  Object.defineProperty(proto, cn, { value: consts[cn], enumerable: false, configurable: true, writable: false });
                }
              }
              Object.setPrototypeOf(proto, Object.getPrototypeOf(ctx));
              Object.defineProperty(proto, Symbol.toStringTag, { value: ctorName, configurable: true });
              Object.defineProperty(proto, "constructor", { value: C, configurable: true, writable: true });
              C.prototype = proto;
              Object.setPrototypeOf(ctx, proto);
            }
          } catch (e) {}
        }
        // 2D context: replace the vendored getImageData (an OWN instance method returning synthetic
        // bytes — `fillRect(red); getImageData` read back NON-red, a broken-canvas tell) with one
        // that reads the ACTUAL rendered pixels back through the raw-RGBA rasterizer. Raw pixels
        // have no PNG encoder in the loop, so shapes/solids read back exactly like a real browser.
        if (k === "2d") {
          // Class tag: real Chrome is "[object CanvasRenderingContext2D]"; the vendored ctx tagged
          // as "[object DOMImplementation]" (its shared fallback proto) — a tell. Brand the instance
          // (not the shared proto — that would mistag every other object backed by it, incl.
          // `location`, like the WebGL-proto lesson).
          try {
            Object.defineProperty(ctx, Symbol.toStringTag, { value: "CanvasRenderingContext2D", configurable: true });
          } catch (e) {}
          // getContextAttributes: real Chrome exposes it; its absence is a tell. Return the default
          // 2D context attributes (native-masked), matching a fresh getContext('2d').
          if (typeof ctx.getContextAttributes !== "function") {
            ctx.getContextAttributes = nativize(({ getContextAttributes() {
              return { alpha: true, colorSpace: "srgb", colorType: "unorm8", desynchronized: false, toneMapping: { mode: "standard" }, willReadFrequently: false };
            } }).getContextAttributes, "getContextAttributes", 0);
          }
          // measureText: the vendored width was a synthetic approximation (a font-metrics tell —
          // font-detection fingerprints compare measureText width across families). Route the WIDTH
          // through the host's real system-font measurer (op_measure_text), which measures the same
          // CoreText face Chrome does → width matches Chrome exactly. The bounding-box fields are
          // derived from the size with Arial-typical ratios (the measurer returns width+height only;
          // real per-glyph boxes would need a richer measurer — width is the load-bearing metric).
          const measureOp = Deno.core.ops && Deno.core.ops.op_measure_text;
          if (typeof ctx.measureText === "function" && measureOp) {
            const origMT = ctx.measureText;
            ctx.measureText = nativize(({ measureText(text) {
              try {
                const font = String(this.font || "10px sans-serif");
                const m = /(\d+(?:\.\d+)?)px\s+(.+)$/.exec(font);
                const size = m ? parseFloat(m[1]) : 10;
                const fam = m ? m[2].replace(/^['"]|['"]$/g, "").split(",")[0].trim() : "sans-serif";
                const r = JSON.parse(measureOp(String(text == null ? "" : text), fam, size));
                if (r && r.length === 2) {
                  const w = r[0];
                  const round2 = (n) => Math.round(n * 100) / 100;
                  const asc = round2(size * 0.875), desc = round2(size * 0.212);
                  return {
                    width: w,
                    actualBoundingBoxLeft: 0, actualBoundingBoxRight: w,
                    actualBoundingBoxAscent: round2(size * 0.72), actualBoundingBoxDescent: round2(size * 0.16),
                    fontBoundingBoxAscent: asc, fontBoundingBoxDescent: desc,
                    emHeightAscent: asc, emHeightDescent: desc,
                    hangingBaseline: round2(asc * 0.8), alphabeticBaseline: 0, ideographicBaseline: -desc,
                  };
                }
              } catch (e) {}
              return origMT.call(this, text);
            } }).measureText, "measureText", 1);
          }
        }
        if (k === "2d" && Object.prototype.hasOwnProperty.call(ctx, "getImageData")) {
          const origGID = ctx.getImageData;
          const rgbaOp2 = Deno.core.ops && Deno.core.ops.op_raster_rgba;
          const b64ToBytes = (b64) => { try { const s = atob(b64); const u = new Uint8Array(s.length); for (let i = 0; i < s.length; i++) u[i] = s.charCodeAt(i); return u; } catch (e) { return null; } };
          ctx.getImageData = nativize(({ getImageData(sx, sy, sw, sh) {
            try {
              const cv = this.canvas, ops = this._ops;
              const W = (cv && cv.width) || 300, H = (cv && cv.height) || 150;
              if (rgbaOp2 && cv && ops && sw > 0 && sh > 0) {
                const full = b64ToBytes(rgbaOp2(W, H, JSON.stringify(ops)));
                if (full && full.length >= W * H * 4) {
                  sx = sx | 0; sy = sy | 0; sw = sw | 0; sh = sh | 0;
                  const out = new Uint8ClampedArray(sw * sh * 4);
                  for (let row = 0; row < sh; row++) {
                    for (let col = 0; col < sw; col++) {
                      const yy = sy + row, xx = sx + col, di = (row * sw + col) * 4;
                      if (yy >= 0 && yy < H && xx >= 0 && xx < W) {
                        const si = (yy * W + xx) * 4;
                        out[di] = full[si]; out[di + 1] = full[si + 1]; out[di + 2] = full[si + 2]; out[di + 3] = full[si + 3];
                      }
                    }
                  }
                  // The stub `ImageData` ctor ignores its args (no `.data`), so build the object
                  // directly and brand it so `Object.prototype.toString.call(img)` reads ImageData.
                  const img = { data: out, width: sw, height: sh, colorSpace: "srgb" };
                  try { Object.defineProperty(img, Symbol.toStringTag, { value: "ImageData", configurable: true }); } catch (e) {}
                  return img;
                }
              }
            } catch (e) {}
            return origGID ? origGID.call(this, sx, sy, sw, sh) : { data: new Uint8ClampedArray(Math.max(0, sw | 0) * Math.max(0, sh | 0) * 4), width: sw | 0, height: sh | 0 };
          } }).getImageData, "getImageData", 4);
        }
        markOwn(ctx);
      }
      return ctx;
    };
    // Prototype-less native shape (native methods have no own `prototype`); getContext.length === 1.
    canvasProto.getContext = nativize(wrapped, "getContext", 1);
    // Raster-backed toDataURL: replay the vendored 2D display list (ctx._ops) through the
    // host rasterizer (op_raster_png) into a REAL PNG. The vendored stub returns a ~94-byte
    // synthetic blob — an impossible size for rendered content, a canvas-fingerprint tell.
    // Falls back to the vendored blob when no rasterizer is installed or on any error.
    const rasterOp = (Deno.core.ops && Deno.core.ops.op_raster_png) || null;
    const origToDataURL = canvasProto.toDataURL;
    if (typeof origToDataURL === "function") {
      // Concise-method form (no own `prototype`, native shape); toDataURL.length === 0.
      canvasProto.toDataURL = nativize(({ toDataURL(type) {
        // The raster op only encodes PNG. For a non-PNG request (e.g. the webp-support probe
        // toDataURL('image/webp') or an explicit image/jpeg) defer to the vendored path, which
        // labels the data URL with the requested MIME — returning PNG for a webp request would be
        // both wrong bytes and a tell.
        const mime = typeof type === "string" && type ? type.toLowerCase() : "image/png";
        try {
          const ctx = this.__ctx2d;
          if (mime === "image/png" && rasterOp && ctx && ctx._ops) {
            const b64 = rasterOp(this.width || 300, this.height || 150, JSON.stringify(ctx._ops));
            if (b64) return "data:image/png;base64," + b64;
          }
        } catch (e) {}
        return origToDataURL.call(this, type);
      } }).toDataURL, "toDataURL", 0);
    }
    if (typeof canvasProto.toBlob === "function") canvasProto.toBlob = nativize(canvasProto.toBlob, "toBlob", 1);
    if (typeof canvasProto.getBoundingClientRect === "function") canvasProto.getBoundingClientRect = nativize(canvasProto.getBoundingClientRect, "getBoundingClientRect", 0);

  });

  // document → HTMLDocument brand + 5 native getters (best-effort: the native DOM object may
  // reject a prototype swap; the brand tag still lands on the instance).
  guard(() => { if (G.document) tag(G.document, "HTMLDocument"); });
  guard(() => hostInterface(G.document, "HTMLDocument", ["cookie", "title", "referrer", "readyState", "URL"]));

  // window / history / console brands (WINDOW_BRANDS: note console's brand is lowercase).
  guard(() => tag(G, "Window"));
  guard(() => {
    G.history = G.history || { length: 1, state: null, scrollRestoration: "auto",
      back() {}, forward() {}, go() {}, pushState() {}, replaceState() {} };
    tag(G.history, "History");
  });
  guard(() => { if (G.console) { tag(G.console, "console"); ["log", "info", "warn", "error", "debug"].forEach((m) => mark(G.console[m], m)); } });
  guard(() => { if (G.performance && G.performance.now) mark(G.performance.now, "now"); });
  // The coherent-clock override replaced Date.now with a plain arrow and wrapped the Date
  // constructor with a JS shim — both leak JS source via toString (an anti-hook tell) unless
  // native-masked. Mark the constructor + its static.
  guard(() => { if (G.Date) mark(G.Date, "Date"); });
  guard(() => { if (G.Date && G.Date.now) mark(G.Date.now, "now"); });

  // createElement was re-wrapped as a JS closure above; re-mark it native (create-element-not-native).
  guard(() => { if (G.document && G.document.createElement) mark(G.document.createElement, "createElement"); });

  // Kill the Gecko/WebKit engine false-positives: CSS.supports must reject vendor-prefixed
  // Gecko props (a permissive syntax-only stub answered true → engineGecko → ua-chrome-wrong-engine).
  guard(() => {
    if (G.CSS && typeof G.CSS.supports === "function") {
      const origSupports = G.CSS.supports.bind(G.CSS);
      G.CSS.supports = function supports(prop, val) {
        const p = String(prop == null ? "" : prop).toLowerCase();
        if (p.indexOf("-moz-") === 0 || p.indexOf("-webkit-") === 0 || p.indexOf("-ms-") === 0) return false;
        try { return origSupports(prop, val); } catch (e) { return false; }
      };
      mark(G.CSS.supports, "supports");
    }
  });

  // Constructed-object host interfaces we own in JS (brand + native method). The 9 rtdom-native
  // element/Range/Text brands need vendored-binding work and stay as residual struct fails.
  const defClass = (name, method, ctorArgsOk) => guard(() => {
    let C = G[name];
    if (typeof C !== "function") { C = function () {}; Object.defineProperty(C, "name", { value: name, configurable: true }); G[name] = C; }
    mark(C, name);
    C.prototype = C.prototype || {};
    tag(C.prototype, name);
    if (method && typeof C.prototype[method] !== "function") { C.prototype[method] = function () {}; }
    if (method) { try { Object.defineProperty(C.prototype[method], "name", { value: method, configurable: true }); } catch (e) {} mark(C.prototype[method]); }
  });
  defClass("Blob", "slice");
  defClass("Headers", null);
  defClass("XMLHttpRequest", "open");
  // defClass native-marks only the one probed method (`open`); mark the rest of the XHR
  // surface too, so `.toString()` on any of them reads native.
  guard(() => {
    const xp = G.XMLHttpRequest && G.XMLHttpRequest.prototype;
    if (xp) ["send", "setRequestHeader", "getResponseHeader", "getAllResponseHeaders",
      "abort", "addEventListener", "removeEventListener", "dispatchEvent"].forEach((m) => {
      if (typeof xp[m] === "function") mark(xp[m], m);
    });
  });
  defClass("URL", null);
  defClass("Event", null);
  defClass("DOMParser", "parseFromString");
  guard(() => { if (G.crypto) tag(G.crypto, "Crypto"); });

  // rtdom nodes/elements expose DISTINCT per-tag prototypes reachable from JS, so brand each
  // (Symbol.toStringTag on the tag's own prototype) and native-mark its checked method IN PLACE
  // (mark the existing function, never shadow it) — clears struct-brand-table + struct-method-not-native
  // without touching the vendored DOM binding.
  guard(() => {
    const brandNode = (obj, name, method) => {
      if (!obj) return;
      let proto = Object.getPrototypeOf(obj);
      // NEVER brand a shared root proto: some rtdom singletons (e.g. document.implementation) have
      // Object.prototype as their direct proto, so tagging the proto would set Symbol.toStringTag on
      // Object.prototype — making Object.prototype.toString.call({}) / [] / new Date() all read the
      // wrong brand (a trivial, high-severity bot tell). Brand the INSTANCE itself in that case.
      if (proto === Object.prototype || proto === null) {
        tag(obj, name);
        // Brand the instance's OWN constructor too (a singleton like document.implementation has
        // Object.prototype as its proto, so it would otherwise report `.constructor === Object`).
        const ctor = ({ [name]: function () {} })[name];
        try { Object.defineProperty(obj, "constructor", { value: ctor, configurable: true }); mark(ctor, name); } catch (e) {}
        proto = null;
      }
      if (proto) {
        tag(proto, name);
        const ctor = ({ [name]: function () {} })[name];
        try { Object.defineProperty(proto, "constructor", { value: ctor, configurable: true }); mark(ctor, name); } catch (e) {}
      }
      if (method) {
        let fn; try { fn = obj[method]; } catch (e) {}
        if (typeof fn !== "function" && proto) { proto[method] = function () {}; fn = proto[method]; try { Object.defineProperty(fn, "name", { value: method, configurable: true }); } catch (e) {} }
        if (typeof fn === "function") mark(fn);
      }
    };
    const d = G.document;
    if (d) {
      brandNode(d.createElement("a"), "HTMLAnchorElement", "click");
      brandNode(d.createElement("canvas"), "HTMLCanvasElement", "getContext");
      brandNode(d.createElement("video"), "HTMLVideoElement", "canPlayType");
      brandNode(d.createElement("input"), "HTMLInputElement", null);
      brandNode(d.implementation, "DOMImplementation", null);
      brandNode(d.createRange && d.createRange(), "Range", "cloneRange");
      brandNode(d.createTextNode && d.createTextNode("x"), "Text", null);
      brandNode(d.createComment && d.createComment("x"), "Comment", null);
      brandNode(d.createDocumentFragment && d.createDocumentFragment(), "DocumentFragment", null);
    }
  });
  // video / text / comment / documentFragment share ONE generic rtdom prototype, so a static brand
  // collides (last write wins). Install a COMPUTED Symbol.toStringTag accessor deriving the WebIDL
  // brand from the node's own type — one proto, per-instance-correct brands. Distinct-proto nodes
  // (a/canvas/input/range) keep their closer static brand.
  guard(() => {
    const d = G.document;
    if (!d) return;
    const EL = { A: "HTMLAnchorElement", CANVAS: "HTMLCanvasElement", VIDEO: "HTMLVideoElement",
      INPUT: "HTMLInputElement", AUDIO: "HTMLAudioElement", IMG: "HTMLImageElement", DIV: "HTMLDivElement",
      SPAN: "HTMLSpanElement", P: "HTMLParagraphElement", BUTTON: "HTMLButtonElement" };
    const brandOf = function () {
      try {
        const nt = this.nodeType;
        const nn = String(this.nodeName == null ? "" : this.nodeName);
        if (nt === 3 || nn === "#text") return "Text";
        if (nt === 8 || nn === "#comment") return "Comment";
        if (nt === 11 || nn.toUpperCase() === "#DOCUMENT-FRAGMENT") return "DocumentFragment";
        if (nt === 1) return EL[nn] || "HTMLElement";
      } catch (e) {}
      return "Object";
    };
    for (const o of [d.createTextNode("x"), d.createComment("x"), d.createDocumentFragment(), d.createElement("video")]) {
      const p = o && Object.getPrototypeOf(o);
      if (p) { try { Object.defineProperty(p, Symbol.toStringTag, { configurable: true, get: brandOf }); } catch (e) {} }
    }
  });

  // Presence of universal host constructors (no-webgl-context / no-audio-context / surface-missing).
  const ensureCtor = (name) => guard(() => { if (typeof G[name] !== "function") { const c = function () {}; Object.defineProperty(c, "name", { value: name, configurable: true }); G[name] = c; } mark(G[name], name); });
  ["WebGLRenderingContext", "WebGL2RenderingContext", "AudioContext", "webkitAudioContext",
   "IntersectionObserver", "ResizeObserver", "AbortController", "URL", "PerformanceObserver",
   "MutationObserver", "Worker"].forEach(ensureCtor);
  guard(() => { ["fetch", "requestAnimationFrame", "queueMicrotask", "matchMedia"].forEach((n) => { if (typeof G[n] === "function") mark(G[n], n); }); });

  // Document encoding accessors — a real Chrome document reports characterSet/charset/
  // inputEncoding = "UTF-8"; the vendored no-layout document leaves them undefined, a tell
  // an anti-bot collector reads (`document.characterSet`). Define on the document's own
  // prototype so all documents (incl. child realms) inherit; skip if already present.
  guard(() => {
    const d = G.document; if (!d) return;
    // The vendored document already has an OWN `characterSet` that returns undefined, so an
    // `in`/existence check would skip it — force-define the encoding accessors to "UTF-8".
    for (const k of ["characterSet", "charset", "inputEncoding"]) {
      try { Object.defineProperty(d, k, { configurable: true, get() { return "UTF-8"; } }); } catch (e) {}
    }
    // Document event-handler slots (real Chrome exposes them as writable, default null). Framework
    // code reads/sets document.onreadystatechange etc.; absence reads as undefined (a minor tell).
    for (const k of ["onreadystatechange", "onvisibilitychange", "onfullscreenchange", "onfullscreenerror", "onpointerlockchange", "onpointerlockerror", "onsecuritypolicyviolation", "onbeforecopy", "onbeforecut", "onbeforepaste", "onfreeze", "onresume", "onsearch"]) {
      // Force-define (the vendored document may already have it as an undefined-returning slot, so
      // an `in` check would skip it — same as characterSet). A settable null handler, like Chrome.
      try {
        let cur = null;
        Object.defineProperty(d, k, { enumerable: true, configurable: true, get() { return cur; }, set(v) { cur = v; } });
      } catch (e) {}
    }
  });

  // Native-mask the Permissions API method: real `navigator.permissions.query.toString()`
  // reports "[native code]"; our JS closure would leak source (an anti-tamper tell).
  guard(() => {
    const q = G.navigator && G.navigator.permissions && G.navigator.permissions.query;
    if (typeof q === "function") mark(q, "query");
  });

  // window.location must class-tag as "[object Location]". The vendored location is a plain
  // object (Object.prototype.toString → the wrong tag, e.g. "[object DOMImplementation]" or
  // "[object Object]"), a trivial `Object.prototype.toString.call(location)` bot tell. Tag it
  // (and its prototype, so a page that reads the tag off the proto also sees Location).
  guard(() => {
    const loc = G.location; if (!loc || typeof loc !== "object") return;
    const tagLocation = (o) => { try { Object.defineProperty(o, Symbol.toStringTag, { value: "Location", configurable: true }); } catch (e) {} };
    tagLocation(loc);
    const p = Object.getPrototypeOf(loc);
    if (p && p !== Object.prototype) tagLocation(p);
  });

  // OfflineAudioContext with a non-silent DSP buffer (audio-silent needs energy > 0). A deterministic
  // synthetic waveform — device-invariant, indistinguishable from a real offline render to a hash+energy
  // probe, and honest (we ARE computing an audio buffer on CPU, not faking a specific device's DSP).
  guard(() => {
    const AudioParam = () => ({ value: 0, setValueAtTime() {}, linearRampToValueAtTime() {}, setTargetAtTime() {} });
    const node = () => ({ connect() { return node(); }, disconnect() {}, start() {}, stop() {},
      frequency: AudioParam(), threshold: AudioParam(), knee: AudioParam(), ratio: AudioParam(),
      attack: AudioParam(), release: AudioParam(), gain: AudioParam(), type: "triangle" });
    function OfflineAudioContext(_ch, length, _rate) {
      this.length = length || 44100; this.sampleRate = _rate || 44100; this.destination = node();
      this.createOscillator = node; this.createDynamicsCompressor = node; this.createGain = node;
      this.createBiquadFilter = node; this.currentTime = 0;
      this.startRendering = () => Promise.resolve({
        length: this.length, numberOfChannels: 1, sampleRate: this.sampleRate,
        getChannelData: () => { const a = new Float32Array(this.length); for (let i = 0; i < a.length; i++) a[i] = Math.sin(i * 0.017) * 0.25 + 0.05; return a; },
      });
    }
    G.OfflineAudioContext = OfflineAudioContext; mark(G.OfflineAudioContext, "OfflineAudioContext");
  });

  // offsetWidth / offsetHeight via the host's real font measurer (op_measure_text, system fonts).
  // A no-layout DOM reports neither, so a font-detection probe (span metrics per font-family) sees
  // fontCount 0 (no-system-fonts). Defined on the base element/node prototype so all elements inherit;
  // measures the node's own text under its computed font. No host measurer installed => 0 (honest).
  guard(() => {
    const measure = (elm) => {
      try {
        const t = elm && elm.textContent; if (!t) return null;
        const op = Deno.core.ops.op_measure_text; if (!op) return null;
        const cs = G.getComputedStyle(elm);
        const fam = (cs && cs.fontFamily) || "sans-serif";
        const size = parseFloat((cs && cs.fontSize) || "16") || 16;
        const r = JSON.parse(op(String(t), String(fam), size));
        return r && r.length === 2 ? r : null;
      } catch (e) { return null; }
    };
    let p = Object.getPrototypeOf(document.createElement("span"));
    while (p && Object.getPrototypeOf(p) && Object.getPrototypeOf(p) !== Object.prototype) p = Object.getPrototypeOf(p);
    if (p) {
      if (!Object.getOwnPropertyDescriptor(p, "offsetWidth")) {
        Object.defineProperty(p, "offsetWidth", { configurable: true, get() { const m = measure(this); return m ? Math.round(m[0]) : 0; } });
      }
      if (!Object.getOwnPropertyDescriptor(p, "offsetHeight")) {
        Object.defineProperty(p, "offsetHeight", { configurable: true, get() { const m = measure(this); return m ? Math.round(m[1]) : 0; } });
      }
    }
  });

  // Chrome-desktop feature markers (ua-version-spoof): each must be PRESENT for a claimed major of
  // 139..148; existence-only stubs (probed via `in`), never invoked. NB: deliberately NOT adding any
  // Gecko/WebKit marker (e.g. GestureEvent) that would re-trip the engine check.
  guard(() => {
    const ns = (path) => { const segs = path.split("."); let o = G;
      for (let i = 0; i < segs.length - 1; i++) { const s = segs[i]; if (o[s] == null) o[s] = (s === "prototype") ? {} : function () {}; o = o[s]; }
      const last = segs[segs.length - 1]; if (!(last in o)) o[last] = function () {}; };
    ["Object.groupBy", "Promise.withResolvers", "Array.fromAsync", "Uint8Array.fromBase64",
     "IDBObjectStore.prototype.getAllRecords", "Document.prototype.activeViewTransition",
     "PerformanceResourceTiming.prototype.contentEncoding", "Map.prototype.getOrInsert",
     "HTMLMediaElement.prototype.loading"].forEach(ns);
    if (!("Temporal" in G)) G.Temporal = {};
    if (!("Sanitizer" in G)) G.Sanitizer = function Sanitizer() {};
    if (typeof Math.sumPrecise !== "function") Math.sumPrecise = function sumPrecise() { return 0; };
  });

  // Pad window's own-property count above the fake-DOM floor (window-prop-count-low, <600). These are
  // genuine Chrome global interface names; define any absent as a stub constructor (also improves the
  // window-surface realism). Not load-bearing — a weak, corroborating tell.
  guard(() => {
    const NAMES = ("HTMLElement HTMLDivElement HTMLSpanElement HTMLBodyElement HTMLHeadElement HTMLHtmlElement " +
      "HTMLParagraphElement HTMLImageElement HTMLButtonElement HTMLSelectElement HTMLOptionElement HTMLTextAreaElement " +
      "HTMLLabelElement HTMLFormElement HTMLTableElement HTMLTableRowElement HTMLTableCellElement HTMLUListElement " +
      "HTMLOListElement HTMLLIElement HTMLHeadingElement HTMLScriptElement HTMLStyleElement HTMLLinkElement HTMLMetaElement " +
      "HTMLIFrameElement HTMLCanvasElement HTMLVideoElement HTMLAudioElement HTMLMediaElement HTMLSourceElement " +
      "HTMLTrackElement HTMLPictureElement HTMLTemplateElement HTMLSlotElement HTMLDetailsElement HTMLDialogElement " +
      "SVGElement SVGSVGElement SVGRectElement SVGCircleElement SVGPathElement SVGGElement SVGTextElement SVGUseElement " +
      "CSSStyleSheet CSSStyleRule CSSMediaRule CSSKeyframesRule CSSKeyframeRule CSSSupportsRule CSSFontFaceRule " +
      "CSSStyleDeclaration StyleSheet MediaQueryList DOMRect DOMRectReadOnly DOMPoint DOMMatrix DOMTokenList NamedNodeMap " +
      "NodeList HTMLCollection Attr CharacterData ProcessingInstruction CDATASection ShadowRoot CustomEvent MouseEvent " +
      "KeyboardEvent PointerEvent TouchEvent WheelEvent FocusEvent InputEvent UIEvent DragEvent ClipboardEvent " +
      "AnimationEvent TransitionEvent ProgressEvent MessageEvent CloseEvent ErrorEvent PopStateEvent HashChangeEvent " +
      "StorageEvent PageTransitionEvent BeforeUnloadEvent GamepadEvent SubmitEvent FormDataEvent " +
      "AbortSignal EventTarget FileReader FileList File FormData ReadableStream WritableStream TransformStream " +
      "TextEncoder TextDecoder TextEncoderStream TextDecoderStream CompressionStream DecompressionStream " +
      "BroadcastChannel MessageChannel MessagePort WebSocket XMLHttpRequestUpload XMLSerializer XPathEvaluator " +
      "XPathResult NodeIterator TreeWalker MutationRecord PerformanceEntry PerformanceMark PerformanceMeasure " +
      "PerformanceNavigationTiming PerformanceResourceTiming PerformanceObserverEntryList PerformancePaintTiming " +
      "IntersectionObserverEntry ResizeObserverEntry ReportingObserver Crypto CryptoKey SubtleCrypto CacheStorage Cache " +
      "ServiceWorker ServiceWorkerContainer ServiceWorkerRegistration Notification PushManager Permissions PermissionStatus " +
      "Geolocation MediaStream MediaStreamTrack RTCPeerConnection RTCDataChannel AudioBuffer AudioNode GainNode OscillatorNode " +
      "AnalyserNode BiquadFilterNode DynamicsCompressorNode Image Audio Option Path2D ImageData ImageBitmap OffscreenCanvas " +
      "IDBDatabase IDBTransaction IDBObjectStore IDBIndex IDBCursor IDBKeyRange IDBRequest IDBFactory " +
      "VisualViewport Screen History Location Navigator BarProp CSSFontFeatureValuesRule " +
      "SVGLineElement SVGPolygonElement SVGPolylineElement SVGEllipseElement SVGImageElement SVGDefsElement " +
      "SVGClipPathElement SVGLinearGradientElement SVGRadialGradientElement SVGStopElement SVGSymbolElement " +
      "SVGMarkerElement SVGPatternElement SVGMaskElement SVGFilterElement SVGTitleElement SVGDescElement " +
      "SVGAnimateElement SVGForeignObjectElement SVGTextPathElement SVGTSpanElement SVGViewElement SVGSwitchElement " +
      "HTMLAreaElement HTMLBaseElement HTMLBRElement HTMLDataElement HTMLDataListElement HTMLDListElement " +
      "HTMLEmbedElement HTMLFieldSetElement HTMLHRElement HTMLLegendElement HTMLMapElement HTMLMenuElement " +
      "HTMLMeterElement HTMLModElement HTMLObjectElement HTMLOptGroupElement HTMLOutputElement HTMLParamElement " +
      "HTMLPreElement HTMLProgressElement HTMLQuoteElement HTMLTableCaptionElement HTMLTableColElement " +
      "HTMLTableSectionElement HTMLTimeElement HTMLTitleElement HTMLUnknownElement HTMLMarqueeElement HTMLFontElement " +
      "CSSConditionRule CSSGroupingRule CSSImportRule CSSNamespaceRule CSSPageRule CSSCounterStyleRule CSSLayerBlockRule " +
      "CSSLayerStatementRule CSSPropertyRule CSSNestedDeclarations CSSPositionTryRule CSSTransition CSSAnimation " +
      "CSSNumericValue CSSUnitValue CSSKeywordValue CSSMathSum CSSTransformValue CSSUnparsedValue CSSVariableReferenceValue " +
      "DOMRectList DOMQuad DOMStringList DOMStringMap DOMException DOMImplementation DOMParser Range StaticRange " +
      "AbstractRange Selection Comment Text CDATASection DocumentType DocumentFragment ShadowRoot Element " +
      "AnimationEffect KeyframeEffect Animation AnimationTimeline DocumentTimeline AnimationPlaybackEvent " +
      "IntersectionObserver ResizeObserver ReportingObserver PerformanceObserver MutationObserver " +
      "Worklet PaintWorkletGlobalScope AudioWorklet AudioWorkletNode Blob FileSystemHandle FileSystemFileHandle " +
      "FileSystemDirectoryHandle FileSystemWritableFileStream StorageManager NavigatorUAData Sanitizer TrustedHTML " +
      "TrustedScript TrustedScriptURL TrustedTypePolicy TrustedTypePolicyFactory Highlight HighlightRegistry " +
      "EyeDropper FragmentDirective NavigateEvent Navigation NavigationHistoryEntry NavigationTransition " +
      "CookieStore CookieChangeEvent PressureObserver ScreenDetails ScreenDetailed WakeLock WakeLockSentinel " +
      "MediaQueryListEvent PictureInPictureEvent PictureInPictureWindow RemotePlayback TextTrack TextTrackCue " +
      "VTTCue TextTrackList TimeRanges MediaError MediaEncryptedEvent SourceBuffer MediaSource " +
      "AudioData VideoFrame EncodedAudioChunk EncodedVideoChunk ImageTrack ImageDecoder GPUDevice GPUAdapter " +
      "GPUBuffer GPUTexture GPUCanvasContext WebGLBuffer WebGLProgram WebGLShader WebGLTexture WebGLFramebuffer " +
      "WebGLRenderbuffer WebGLUniformLocation WebGLActiveInfo WebGLContextEvent WebGLVertexArrayObject " +
      "CanvasGradient CanvasPattern CanvasRenderingContext2D OffscreenCanvasRenderingContext2D Path2D TextMetrics").split(/\s+/);
    for (const n of NAMES) { if (n && typeof G[n] === "undefined") { const c = function () {}; try { Object.defineProperty(c, "name", { value: n, configurable: true }); } catch (e) {} G[n] = c; mark(c, n); } }
  });

  // Extend the window surface to real Chrome's full breadth (measured live: 1235 own props vs our
  // ~527) so `X in window` / `typeof window.X` presence checks pass for the ~740 globals we lacked.
  // Three groups from a Chrome-vs-turbo-surf diff: interface constructors (stub fns), on* event-
  // handler slots (null, like Chrome until assigned), and misc methods/props/bar objects. Weak
  // corroborating surface (BotGuard reads presence), not load-bearing.
  guard(() => {
    const IFACES = ("AbsoluteOrientationSensor Accelerometer AnimationTrigger AudioBufferSourceNode AudioDecoder " +
      "AudioDestinationNode AudioEncoder AudioListener AudioParam AudioParamMap AudioPlaybackStats " +
      "AudioProcessingEvent AudioScheduledSourceNode AudioSinkInfo AuthenticatorAssertionResponse " +
      "AuthenticatorAttestationResponse AuthenticatorResponse BackgroundFetchManager BackgroundFetchRecord " +
      "BackgroundFetchRegistration BarcodeDetector BaseAudioContext BatteryManager BeforeInstallPromptEvent " +
      "BlobEvent Bluetooth BluetoothCharacteristicProperties BluetoothDevice " +
      "BluetoothRemoteGATTCharacteristic BluetoothRemoteGATTDescriptor BluetoothRemoteGATTServer " +
      "BluetoothRemoteGATTService BluetoothUUID BrowserCaptureMediaStreamTrack ByteLengthQueuingStrategy " +
      "CSPViolationReportBody CSSContainerRule CSSFontPaletteValuesRule CSSFunctionDeclarations " +
      "CSSFunctionDescriptors CSSFunctionRule CSSImageValue CSSMarginRule CSSMathClamp CSSMathInvert " +
      "CSSMathMax CSSMathMin CSSMathNegate CSSMathProduct CSSMathValue CSSMatrixComponent CSSNumericArray " +
      "CSSPerspective CSSPositionTryDescriptors CSSPositionValue CSSPseudoElement CSSRotate CSSRuleList " +
      "CSSScale CSSScopeRule CSSSkew CSSSkewX CSSSkewY CSSStartingStyleRule CSSStyleValue " +
      "CSSTransformComponent CSSTranslate CSSViewTransitionRule CanvasCaptureMediaStreamTrack " +
      "CaptureController CaretPosition ChannelMergerNode ChannelSplitterNode ChapterInformation " +
      "CharacterBoundsUpdateEvent Clipboard ClipboardChangeEvent ClipboardItem CloseWatcher CommandEvent " +
      "ConstantSourceNode ContentVisibilityAutoStateChangeEvent ConvolverNode CookieStoreManager " +
      "CountQueuingStrategy CrashReportContext CreateMonitor Credential CredentialsContainer CropTarget " +
      "CustomElementRegistry CustomStateSet DOMError DOMMatrixReadOnly DOMPointReadOnly DataTransfer " +
      "DataTransferItem DataTransferItemList DelayNode DelegatedInkTrailPresenter DeviceMotionEvent " +
      "DeviceMotionEventAcceleration DeviceMotionEventRotationRate DeviceOrientationEvent DevicePosture " +
      "DigitalCredential DocumentPictureInPicture DocumentPictureInPictureEvent EditContext " +
      "ElementInternals EventCounts EventSource External FeaturePolicy FederatedCredential Fence " +
      "FencedFrameConfig FetchLaterResult FileSystemObserver FontData FontFace FontFaceSet " +
      "FontFaceSetLoadEvent GPU GPUAdapterInfo GPUBindGroup GPUBindGroupLayout GPUBufferUsage GPUColorWrite " +
      "GPUCommandBuffer GPUCommandEncoder GPUCompilationInfo GPUCompilationMessage GPUComputePassEncoder " +
      "GPUComputePipeline GPUDeviceLostInfo GPUError GPUExternalTexture GPUInternalError GPUMapMode " +
      "GPUOutOfMemoryError GPUPipelineError GPUPipelineLayout GPUQuerySet GPUQueue GPURenderBundle " +
      "GPURenderBundleEncoder GPURenderPassEncoder GPURenderPipeline GPUSampler GPUShaderModule " +
      "GPUShaderStage GPUSupportedFeatures GPUSupportedLimits GPUTextureUsage GPUTextureView " +
      "GPUUncapturedErrorEvent GPUValidationError Gamepad GamepadButton GamepadHapticActuator " +
      "GeolocationCoordinates GeolocationPosition GeolocationPositionError GravitySensor Gyroscope HID " +
      "HIDConnectionEvent HIDDevice HIDInputReportEvent HTMLAllCollection HTMLCameraElement " +
      "HTMLDirectoryElement HTMLFencedFrameElement HTMLFormControlsCollection HTMLFrameElement " +
      "HTMLFrameSetElement HTMLGeolocationElement HTMLMicrophoneElement HTMLOptionsCollection " +
      "HTMLSelectedContentElement HTMLUserMediaElement IDBCursorWithValue IDBOpenDBRequest IDBRecord " +
      "IDBVersionChangeEvent IIRFilterNode IdentityCredential IdentityCredentialError IdentityProvider " +
      "IdleDeadline IdleDetector ImageBitmapRenderingContext ImageCapture ImageTrackList Ink " +
      "InputDeviceCapabilities InputDeviceInfo IntegrityViolationReportBody InteractionContentfulPaint " +
      "InterestEvent Keyboard KeyboardLayoutMap LanguageDetector LanguageModel LargestContentfulPaint " +
      "LaunchParams LaunchQueue LayoutShift LayoutShiftAttribution LinearAccelerationSensor Lock " +
      "LockManager MIDIAccess MIDIConnectionEvent MIDIInput MIDIInputMap MIDIMessageEvent MIDIOutput " +
      "MIDIOutputMap MIDIPort MathMLElement MediaCapabilities MediaDeviceInfo MediaDevices " +
      "MediaElementAudioSourceNode MediaKeyMessageEvent MediaKeySession MediaKeyStatusMap " +
      "MediaKeySystemAccess MediaKeys MediaMetadata MediaRecorder MediaSession MediaSourceHandle " +
      "MediaStreamAudioDestinationNode MediaStreamAudioSourceNode MediaStreamEvent " +
      "MediaStreamTrackAudioStats MediaStreamTrackEvent MediaStreamTrackGenerator MediaStreamTrackProcessor " +
      "MediaStreamTrackVideoStats MimeType MimeTypeArray NavigationActivation " +
      "NavigationCurrentEntryChangeEvent NavigationDestination NavigationPrecommitController " +
      "NavigationPreloadManager NavigatorLogin NavigatorManagedData NetworkInformation NodeRange " +
      "NotRestoredReasonDetails NotRestoredReasons OTPCredential Observable OfflineAudioCompletionEvent " +
      "OpaqueRange OrientationSensor Origin OverconstrainedError PageRevealEvent PageSwapEvent PannerNode " +
      "PasswordCredential PaymentAddress PaymentManager PaymentMethodChangeEvent PaymentRequest " +
      "PaymentRequestUpdateEvent PaymentResponse Performance PerformanceElementTiming " +
      "PerformanceEventTiming PerformanceLongAnimationFrameTiming PerformanceLongTaskTiming " +
      "PerformanceNavigation PerformanceScriptTiming PerformanceServerTiming PerformanceSoftNavigation " +
      "PerformanceTiming PerformanceTimingConfidence PeriodicSyncManager PeriodicWave PermissionsPolicy " +
      "Plugin PluginArray Presentation PresentationAvailability PresentationConnection " +
      "PresentationConnectionAvailableEvent PresentationConnectionCloseEvent PresentationConnectionList " +
      "PresentationReceiver PresentationRequest PressureRecord Profiler PromiseRejectionEvent " +
      "ProtectedAudience PublicKeyCredential PushSubscription PushSubscriptionOptions QuotaExceededError " +
      "RTCCertificate RTCDTMFSender RTCDTMFToneChangeEvent RTCDataChannelEvent RTCDtlsTransport " +
      "RTCEncodedAudioFrame RTCEncodedVideoFrame RTCError RTCErrorEvent RTCIceCandidate RTCIceTransport " +
      "RTCPeerConnectionIceErrorEvent RTCPeerConnectionIceEvent RTCRtpReceiver RTCRtpScriptTransform " +
      "RTCRtpSender RTCRtpTransceiver RTCSctpTransport RTCSessionDescription RTCStatsReport RTCTrackEvent " +
      "RadioNodeList ReadableByteStreamController ReadableStreamBYOBReader ReadableStreamBYOBRequest " +
      "ReadableStreamDefaultController ReadableStreamDefaultReader RelativeOrientationSensor ReportBody " +
      "ResizeObserverSize RestrictionTarget SVGAElement SVGAngle SVGAnimateMotionElement " +
      "SVGAnimateTransformElement SVGAnimatedAngle SVGAnimatedBoolean SVGAnimatedEnumeration " +
      "SVGAnimatedInteger SVGAnimatedLength SVGAnimatedLengthList SVGAnimatedNumber SVGAnimatedNumberList " +
      "SVGAnimatedPreserveAspectRatio SVGAnimatedRect SVGAnimatedString SVGAnimatedTransformList " +
      "SVGAnimationElement SVGComponentTransferFunctionElement SVGFEBlendElement SVGFEColorMatrixElement " +
      "SVGFEComponentTransferElement SVGFECompositeElement SVGFEConvolveMatrixElement " +
      "SVGFEDiffuseLightingElement SVGFEDisplacementMapElement SVGFEDistantLightElement " +
      "SVGFEDropShadowElement SVGFEFloodElement SVGFEFuncAElement SVGFEFuncBElement SVGFEFuncGElement " +
      "SVGFEFuncRElement SVGFEGaussianBlurElement SVGFEImageElement SVGFEMergeElement SVGFEMergeNodeElement " +
      "SVGFEMorphologyElement SVGFEOffsetElement SVGFEPointLightElement SVGFESpecularLightingElement " +
      "SVGFESpotLightElement SVGFETileElement SVGFETurbulenceElement SVGGeometryElement SVGGradientElement " +
      "SVGGraphicsElement SVGLength SVGLengthList SVGMPathElement SVGMatrix SVGMetadataElement SVGNumber " +
      "SVGNumberList SVGPoint SVGPointList SVGPreserveAspectRatio SVGRect SVGScriptElement SVGSetElement " +
      "SVGStringList SVGStyleElement SVGTextContentElement SVGTextPositioningElement SVGTransform " +
      "SVGTransformList SVGUnitTypes Scheduler Scheduling ScreenOrientation ScriptProcessorNode " +
      "ScrollTimeline SecurityPolicyViolationEvent Sensor SensorErrorEvent Serial SerialPort SnapEvent " +
      "SourceBufferList SpeechGrammar SpeechGrammarList SpeechRecognition SpeechRecognitionErrorEvent " +
      "SpeechRecognitionEvent SpeechRecognitionPhrase SpeechSynthesis SpeechSynthesisErrorEvent " +
      "SpeechSynthesisEvent SpeechSynthesisUtterance SpeechSynthesisVoice StereoPannerNode Storage " +
      "StorageBucket StorageBucketManager StylePropertyMap StylePropertyMapReadOnly StyleSheetList " +
      "Subscriber Summarizer SyncManager TaskAttributionTiming TaskController TaskPriorityChangeEvent " +
      "TaskSignal TextEvent TextFormat TextFormatUpdateEvent TextTrackCueList TextUpdateEvent " +
      "TimelineTrigger TimelineTriggerRange TimelineTriggerRangeList ToggleEvent Touch TouchList TrackEvent " +
      "TransformStreamDefaultController Translator URLPattern USB USBAlternateInterface USBConfiguration " +
      "USBConnectionEvent USBDevice USBEndpoint USBInTransferResult USBInterface " +
      "USBIsochronousInTransferPacket USBIsochronousInTransferResult USBIsochronousOutTransferPacket " +
      "USBIsochronousOutTransferResult USBOutTransferResult UserActivation ValidityState VideoColorSpace " +
      "VideoDecoder VideoEncoder VideoPlaybackQuality ViewTimeline ViewTransition ViewTransitionTypeSet " +
      "Viewport VirtualKeyboard VirtualKeyboardGeometryChangeEvent VisibilityStateEntry " +
      "WGSLLanguageFeatures WaveShaperNode WebGLObject WebGLQuery WebGLSampler WebGLShaderPrecisionFormat " +
      "WebGLSync WebGLTransformFeedback WebKitCSSMatrix WebKitMutationObserver WebSocketError " +
      "WebSocketStream WebTransport WebTransportBidirectionalStream WebTransportDatagramDuplexStream " +
      "WebTransportError WindowControlsOverlay WindowControlsOverlayGeometryChangeEvent " +
      "WritableStreamDefaultController WritableStreamDefaultWriter XMLDocument XMLHttpRequestEventTarget " +
      "XPathExpression XRAnchor XRAnchorSet XRBoundedReferenceSpace XRCPUDepthInformation XRCamera " +
      "XRCompositionLayer XRCubeLayer XRCylinderLayer XRDOMOverlayState XRDepthInformation XREquirectLayer " +
      "XRFrame XRHand XRHitTestResult XRHitTestSource XRInputSource XRInputSourceArray XRInputSourceEvent " +
      "XRInputSourcesChangeEvent XRJointPose XRJointSpace XRLayer XRLayerEvent XRLightEstimate XRLightProbe " +
      "XRPlane XRPlaneSet XRPose XRProjectionLayer XRQuadLayer XRRay XRReferenceSpace XRReferenceSpaceEvent " +
      "XRRenderState XRRigidTransform XRSession XRSessionEvent XRSpace XRSubImage XRSystem " +
      "XRTransientInputHitTestResult XRTransientInputHitTestSource XRView XRViewerPose XRViewport " +
      "XRVisibilityMaskChangeEvent XRWebGLBinding XRWebGLDepthInformation XRWebGLLayer XRWebGLSubImage " +
      "XSLTProcessor").split(/\s+/);
    for (const n of IFACES) { if (n && typeof G[n] === "undefined") { const c = function () {}; try { Object.defineProperty(c, "name", { value: n, configurable: true }); } catch (e) {} G[n] = c; mark(c, n); } }
    const ON = ("onabort onafterprint onanimationcancel onanimationend onanimationiteration onanimationstart " +
      "onappinstalled onauxclick onbeforeinput onbeforeinstallprompt onbeforematch onbeforeprint " +
      "onbeforetoggle onbeforexrselect onblur oncancel oncanplay oncanplaythrough onchange onclick onclose " +
      "oncommand oncontentvisibilityautostatechange oncontextlost oncontextmenu oncontextrestored " +
      "oncuechange ondblclick ondevicemotion ondeviceorientation ondeviceorientationabsolute ondrag " +
      "ondragend ondragenter ondragleave ondragover ondragstart ondrop ondurationchange onemptied onended " +
      "onfocus onformdata ongamepadconnected ongamepaddisconnected ongotpointercapture oninput oninvalid " +
      "onkeydown onkeypress onkeyup onlanguagechange onloadeddata onloadedmetadata onloadstart " +
      "onlostpointercapture onmousedown onmouseenter onmouseleave onmousemove onmouseout onmouseover " +
      "onmouseup onmousewheel onpagehide onpagereveal onpageshow onpageswap onpause onplay onplaying " +
      "onpointercancel onpointerdown onpointerenter onpointerleave onpointermove onpointerout onpointerover " +
      "onpointerrawupdate onpointerup onprogress onratechange onrejectionhandled onreset onresize onscroll " +
      "onscrollend onscrollsnapchange onscrollsnapchanging onsearch onsecuritypolicyviolation onseeked " +
      "onseeking onselect onselectionchange onselectstart onslotchange onstalled onstorage onsubmit " +
      "onsuspend ontimeupdate ontoggle ontransitioncancel ontransitionend ontransitionrun ontransitionstart " +
      "onunhandledrejection onvolumechange onwaiting onwebkitanimationend onwebkitanimationiteration " +
      "onwebkitanimationstart onwebkittransitionend onwheel").split(/\s+/);
    for (const n of ON) { if (n && !(n in G)) { try { Object.defineProperty(G, n, { value: null, writable: true, enumerable: true, configurable: true }); } catch (e) {} } }
    const bar = () => ({ visible: true });
    const misc = {
      closed: false, length: 0, name: "", opener: null, status: "", crossOriginIsolated: false,
      isSecureContext: true, originAgentCluster: true, offscreenBuffering: true, credentialless: false,
      origin: (G.location && G.location.origin) || "https://www.google.com", clientInformation: G.navigator,
      locationbar: bar(), menubar: bar(), personalbar: bar(), scrollbars: bar(), statusbar: bar(), toolbar: bar(),
      // These real Web APIs are FEATURE-DETECTED by page code (`if (window.X)`) which then calls
      // their methods — a bare {} is truthy but methodless, so e.g. `navigation.entries()` throws
      // (before, an undefined `navigation` was skipped). Give each its commonly-called methods.
      caches: { open: () => Promise.resolve({ match: () => Promise.resolve(undefined), put: () => Promise.resolve(), keys: () => Promise.resolve([]) }), has: () => Promise.resolve(false), keys: () => Promise.resolve([]), match: () => Promise.resolve(undefined), delete: () => Promise.resolve(false) },
      cookieStore: { get: () => Promise.resolve(null), getAll: () => Promise.resolve([]), set: () => Promise.resolve(), delete: () => Promise.resolve(), addEventListener() {}, removeEventListener() {} },
      navigation: { entries: () => [], currentEntry: null, canGoBack: false, canGoForward: false, navigate() {}, reload() {}, traverseTo() {}, back() {}, forward() {}, updateCurrentEntry() {}, addEventListener() {}, removeEventListener() {} },
      speechSynthesis: { getVoices: () => [], speak() {}, cancel() {}, pause() {}, resume() {}, pending: false, speaking: false, paused: false, addEventListener() {}, removeEventListener() {} },
      viewport: {}, launchQueue: { setConsumer() {} },
      external: {}, fence: null, crashReport: {}, documentPictureInPicture: {}, event: undefined,
      styleMedia: { type: "screen", matchMedium: nativize(() => false, "matchMedium") }, screenLeft: 0, screenTop: 0,
      webkitURL: G.URL, webkitMediaStream: G.MediaStream, webkitRTCPeerConnection: G.RTCPeerConnection,
    };
    for (const k in misc) { if (!(k in G)) { try { G[k] = misc[k]; } catch (e) {} } }
    const METHODS = ("alert blur captureEvents close confirm createImageBitmap fetchLater find focus " +
      "getScreenDetails moveBy moveTo open print prompt queryLocalFonts releaseEvents reportError requestResize " +
      "resizeBy resizeTo showDirectoryPicker showOpenFilePicker showSaveFilePicker stop webkitCancelAnimationFrame " +
      "webkitRequestAnimationFrame webkitRequestFileSystem webkitResolveLocalFileSystemURL " +
      "webkitSpeechGrammar webkitSpeechGrammarList webkitSpeechRecognition").split(/\s+/);
    for (const n of METHODS) { if (typeof G[n] !== "function") { try { G[n] = nativize(() => undefined, n); } catch (e) {} } }
  });

  // Final safety net: in a real browser EVERY window-global function reports "[native code]".
  // Native-mark any DATA-property function on window still unmarked (a shim/vendored fn that
  // slipped through) so its toString can't leak JS source. Descriptor-based (never triggers a
  // getter → no side effects); functions behind getters are covered where they're defined.
  guard(() => {
    for (const k of Object.getOwnPropertyNames(G)) {
      try {
        const d = Object.getOwnPropertyDescriptor(G, k);
        if (d && typeof d.value === "function" && !native.has(d.value)) mark(d.value, k);
      } catch (e) {}
    }
  });
})();
})();"##;

/// Initialize the V8 platform ONCE, on a dedicated thread that lives for the whole
/// process. Since V8 11.6 every `JsRuntime` must share the thread that first initialized
/// the platform; deno_core does this lazily on whichever thread creates the FIRST runtime
/// (jsruntime.rs: "all runtimes must have a common parent thread that initialized the V8
/// platform"). This engine creates isolates on many threads — the `evaluate` pool,
/// `render`'s per-call thread, the pooled render worker, each live session's thread — and
/// `render` SPAWNS-then-JOINS its thread, so if that transient thread parents the platform
/// and then exits, a runtime later created on another thread faults (SIGBUS on Linux;
/// macOS tolerates it).
///
/// The parent must be both STABLE (outlives every runtime) and NOT the napi addon's Node
/// main thread (which already runs Node's own V8 — initializing deno_core's V8 platform
/// there interferes). So spawn a dedicated "v8-platform" keeper thread that calls
/// `init_platform` and then parks forever; the platform is parented on a thread that
/// never dies and never touches Node's V8. Blocks until init is done so the first runtime
/// can't race ahead of it.
pub fn ensure_platform() {
    use std::sync::mpsc::channel;
    use std::sync::OnceLock;
    static KEEPER: OnceLock<()> = OnceLock::new();
    KEEPER.get_or_init(|| {
        pin_timezone();
        let (ready_tx, ready_rx) = channel::<()>();
        std::thread::Builder::new()
            .name("v8-platform".into())
            .spawn(move || {
                JsRuntime::init_platform(None);
                let _ = ready_tx.send(());
                // Park forever: the platform's parent thread must outlive every runtime.
                loop {
                    std::thread::park();
                }
            })
            .expect("spawn v8 platform keeper");
        let _ = ready_rx.recv(); // platform initialized before any runtime is built
    });
}

// Pin a coherent timezone for the synthetic browser — OPT-IN via `TURBO_SURF_TZ`. An isolate that
// reports the host machine's zone (leaked via `Intl.DateTimeFormat().resolvedOptions().timeZone` +
// `Date.getTimezoneOffset`, incoherent with the en-US identity) is a fingerprint tell, but ICU
// reads the timezone from the `TZ` env, and mutating a process-global env var is a data race with
// other threads + would silently change the HOST process's timezone when this crate is embedded in
// the napi addon / PyO3 wheel. So we set `TZ` ONLY when the operator explicitly opts in with
// `TURBO_SURF_TZ` (e.g. `America/New_York`), once, before the platform initializes; we never touch
// it by default. The per-isolate `date_time_configuration_change_notification(Redetect)` (see
// make_runtime) then re-reads it. Without the opt-in, the isolate uses the host zone.
fn pin_timezone() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        if let Ok(tz) = std::env::var("TURBO_SURF_TZ") {
            if !tz.is_empty() {
                std::env::set_var("TZ", tz);
            }
        }
    });
}

fn make_runtime(base: &str, cookies: &str, ua: &str) -> JsRuntime {
    pin_timezone();
    // Build the shared cookie jar first so it backs BOTH page `fetch` (op_state) and the
    // ES-module loader (`<script type=module>` import graphs) — same session, one jar.
    let jar: Jar = Rc::new(RefCell::new(if cookies.is_empty() {
        CookieJar::new()
    } else {
        CookieJar::from_storage_state(cookies)
    }));
    let mut rt = JsRuntime::new(RuntimeOptions {
        extensions: vec![turbo_dom::init()],
        module_loader: Some(Rc::new(NetModuleLoader {
            base: base.to_string(),
            jar: jar.clone(),
            ua: ua.to_string(),
        })),
        ..Default::default()
    });
    // Force ICU to re-detect the timezone from the (now-pinned) TZ env for THIS isolate, so
    // Date/Intl report the coherent zone rather than a cached host default.
    rt.v8_isolate()
        .date_time_configuration_change_notification(v8::TimeZoneDetection::Redetect);
    let state = rt.op_state();
    let mut state = state.borrow_mut();
    state.put::<Base>(Base(base.to_string()));
    state.put::<Jar>(jar);
    state.put::<Ua>(Ua(ua.to_string()));
    drop(state);
    rt
}

/// Graft the native DOM binding onto the runtime's context (parsing `html` into the
/// tree), then layer the non-DOM env globals over the ops. After this the page
/// script runs against a real `document`.
fn install_dom(rt: &mut JsRuntime, html: &str, base: &str) -> Result<(), String> {
    let context = rt.main_context();
    {
        let scope = v8::HandleScope::new(rt.v8_isolate());
        let scope = std::pin::pin!(scope);
        let mut scope = scope.init();
        let context = v8::Local::new(&scope, context);
        let mut scope = v8::ContextScope::new(&mut scope, context);
        crate::browser_env::install_html(&mut scope, html);
    }
    rt.execute_script("<env>", ENV_BOOTSTRAP)
        .map_err(|e| e.to_string())?;
    rt.execute_script("<location>", format!("location.href = {base:?}"))
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn read_string(rt: &mut JsRuntime, global: v8::Global<v8::Value>) -> Result<String, String> {
    let context = rt.main_context();
    let scope = v8::HandleScope::new(rt.v8_isolate());
    let scope = std::pin::pin!(scope);
    let mut scope = scope.init();
    let context = v8::Local::new(&scope, context);
    let scope = v8::ContextScope::new(&mut scope, context);
    let local = v8::Local::new(&scope, global);
    Ok(local.to_rust_string_lossy(&scope))
}

thread_local! {
    /// Persistent evaluate runtime, reused across `run_with_dom` calls (i.e. across
    /// pages) so the ~20ms V8-isolate boot is paid ONCE per thread, not per call —
    /// the dominant per-page cost for a no-JS crawler whose link/field extraction
    /// goes through `page.evaluate`. Safe to reuse across pages: each call reinstalls
    /// a fresh DOM from the page's HTML, and the binding's V8 globals are cleared
    /// (`browser_env::reset`) after every call, so the thread-local DOM is empty at
    /// thread exit (no dangling handles when the isolate finally drops). Page-JS
    /// isolation across pages is intentionally relaxed here — a crawl doesn't need it.
    static EVAL_RT: RefCell<Option<(JsRuntime, String)>> = const { RefCell::new(None) };
}

/// Evaluate `script` against `html`'s DOM, returning its result as a string
/// (Playwright `page.evaluate`-ish; synchronous, no event loop). Reuses a
/// thread-persistent isolate AND the installed DOM across calls on the SAME page
/// (see [`EVAL_RT`]): the page is parsed + installed once, then repeated
/// `page.evaluate`s on it just run script (~0.5 ms) instead of re-parsing the
/// document (~5 ms). The DOM is re-installed only when the HTML changes (a new
/// page). Same-page evaluates share the page's globals/DOM, which matches
/// Playwright's page-scoped `evaluate` semantics.
pub fn run_with_dom(html: &str, script: &str) -> Result<String, String> {
    EVAL_RT.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some((make_runtime("about:blank", "", ""), String::new()));
        }
        let (rt, installed) = slot.as_mut().expect("eval runtime present");
        if installed != html {
            crate::browser_env::reset(); // drop the previous page's binding (isolate still alive)
            install_dom(rt, html, "about:blank")?;
            installed.clear();
            installed.push_str(html);
        }
        let global = rt
            .execute_script("<page>", script.to_string())
            .map_err(|e| e.to_string())?;
        read_string(rt, global)
    })
}

/// Run page `script` against `html`, drain virtual timers, and return the hydrated
/// document HTML. The Lane B render contract: JS-gated page in, HTML after the
/// page's own scripts ran out. (Sync; no event loop — see [`render_page`].)
pub fn render_html(html: &str, script: &str) -> Result<String, String> {
    let mut rt = make_runtime("about:blank", "", "");
    let out = run_sync(&mut rt, html, script);
    crate::browser_env::reset();
    out
}

fn run_sync(rt: &mut JsRuntime, html: &str, script: &str) -> Result<String, String> {
    install_dom(rt, html, "about:blank")?;
    rt.execute_script("<page>", script.to_string())
        .map_err(|e| e.to_string())?;
    rt.execute_script("<timers>", "__runTimers()")
        .map_err(|e| e.to_string())?;
    Ok(crate::browser_env::document_html())
}

async fn drain_event_loop(rt: &mut JsRuntime) -> Result<(), String> {
    match rt
        .run_event_loop(deno_core::PollEventLoopOptions::default())
        .await
    {
        Ok(()) => Ok(()),
        Err(e) => {
            // Browser-tolerant: a page's UNHANDLED promise rejection logs in a real
            // browser, it doesn't abort the page. deno_core surfaces it as a fatal event-
            // loop error ("Uncaught (in promise) …"); swallow it so hydration keeps going
            // (the pump re-polls). Real execution errors (terminated budget, op failures)
            // still propagate.
            let s = e.to_string();
            if s.contains("Uncaught (in promise)") || s.contains("Unhandled") {
                Ok(())
            } else {
                Err(s)
            }
        }
    }
}

/// Like [`render_html`] but drives deno_core's event loop, so a page script that
/// hydrates asynchronously (`Promise`/`async`-`await`/microtasks, and timer
/// callbacks that themselves await) resolves before serialization. This is the
/// fidelity step real SPA frameworks need.
pub async fn render_html_async(html: &str, script: &str) -> Result<String, String> {
    render_page(html, "about:blank", script).await
}

/// Default render execution budget (eval-guard). A page script that loops forever
/// (sync) or never settles (async) is terminated past this.
pub const DEFAULT_RENDER_BUDGET_MS: u64 = 10_000;

/// Async render with a page base URL — relative `fetch` resolves against it and
/// the `document.cookie` bridge is scoped to it. Drives the event loop so
/// `fetch`-driven and promise-based hydration completes before serialization.
/// Bounded by [`DEFAULT_RENDER_BUDGET_MS`].
pub async fn render_page(html: &str, base: &str, script: &str) -> Result<String, String> {
    render_page_with_budget(html, base, script, DEFAULT_RENDER_BUDGET_MS).await
}

/// `render_page` with an explicit execution budget (ms). The V8 isolate is a true
/// isolate (host heap unreachable from guest); this adds a runaway-execution guard:
/// a watchdog thread terminates the isolate if the script exceeds `budget_ms`.
pub async fn render_page_with_budget(
    html: &str,
    base: &str,
    script: &str,
    budget_ms: u64,
) -> Result<String, String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let mut rt = make_runtime(base, "", "");
    let handle = rt.v8_isolate().thread_safe_handle();
    let done = Arc::new(AtomicBool::new(false));
    let watch = done.clone();
    let watchdog = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        while !watch.load(Ordering::Relaxed) {
            if start.elapsed() >= std::time::Duration::from_millis(budget_ms) {
                handle.terminate_execution();
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    });

    let result = run_async(&mut rt, html, base, script).await;
    done.store(true, Ordering::Relaxed);
    let _ = watchdog.join();
    let out = result.map_err(|e| budget_msg(&e, budget_ms));
    crate::browser_env::reset();
    out
}

/// Full async render that ALSO returns the isolate's earned cookies (the shared jar
/// as a storage_state JSON string). Seeds the jar from `cookies` (storage_state or
/// "") and the navigator UA from `ua`, runs the page's own scripts to completion
/// (dynamic `<script>` injection + `op_fetch` + timers), then reads the jar back —
/// the in-isolate anti-bot recon path (did the page's integrity JS set a session
/// cookie, e.g. google's `__Secure-ENID`, with no browser?). Cookie read-back
/// happens even on a budget kill so a partial run's cookies aren't lost.
pub async fn render_capture_cookies(
    html: &str,
    base: &str,
    ua: &str,
    cookies: &str,
    script: &str,
    budget_ms: u64,
) -> Result<(String, String), String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let mut rt = make_runtime(base, cookies, ua);
    let handle = rt.v8_isolate().thread_safe_handle();
    let done = Arc::new(AtomicBool::new(false));
    let watch = done.clone();
    let watchdog = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        while !watch.load(Ordering::Relaxed) {
            if start.elapsed() >= std::time::Duration::from_millis(budget_ms) {
                handle.terminate_execution();
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    });

    let result = run_async(&mut rt, html, base, script).await;
    done.store(true, Ordering::Relaxed);
    let _ = watchdog.join();
    // Read the earned cookies out of the shared jar regardless of run outcome.
    let storage = {
        let state = rt.op_state();
        let st = state.borrow();
        let jar = st.borrow::<Jar>();
        let out = jar.borrow().storage_state();
        out
    };
    let doc = result.map_err(|e| budget_msg(&e, budget_ms));
    crate::browser_env::reset();
    Ok((doc?, storage))
}

thread_local! {
    /// Persistent render runtime, reused across `render_page_pooled` calls on a thread
    /// so the V8-isolate boot + extension wiring is paid ONCE per worker thread instead
    /// of per page — the dominant per-page cost on a JS-mode crawl (each page builds a
    /// fresh isolate otherwise). Reuse is SAFE for the render contract the same way
    /// [`EVAL_RT`] is: every call reinstalls a fresh DOM + re-runs the (idempotent)
    /// `ENV_BOOTSTRAP` (which re-seeds the timer queue + env globals), and the binding's
    /// V8 globals are cleared (`browser_env::reset`) after every call. Page-JS isolation
    /// ACROSS pages is intentionally relaxed (a crawl doesn't need it); WITHIN a page the
    /// isolate is still a true isolate. A poisoned runtime (budget-terminated, or any
    /// error) is dropped instead of returned to the slot, so the next page starts clean.
    static RENDER_RT: RefCell<Option<JsRuntime>> = const { RefCell::new(None) };
}

/// Repoint a reused runtime's per-page session (base URL / cookie jar / UA) in op
/// state. The page `fetch`/`document.cookie` ops read these, so they must reflect the
/// CURRENT page, not the one the runtime was first built for.
fn reset_session(rt: &JsRuntime, base: &str, cookies: &str, ua: &str) {
    let jar: Jar = Rc::new(RefCell::new(if cookies.is_empty() {
        CookieJar::new()
    } else {
        CookieJar::from_storage_state(cookies)
    }));
    let state = rt.op_state();
    let mut state = state.borrow_mut();
    state.put::<Base>(Base(base.to_string()));
    state.put::<Jar>(jar);
    state.put::<Ua>(Ua(ua.to_string()));
}

/// Like [`render_page_with_budget`] but reuses a thread-local isolate across calls (see
/// [`RENDER_RT`]) — the JS-crawl fast path. Only the classic-script render is pooled
/// (module `import` graphs need a per-page module-loader base, which a reused runtime
/// can't repoint), so this drives `render_page`, not the hydrate tier.
pub async fn render_page_pooled(
    html: &str,
    base: &str,
    script: &str,
    budget_ms: u64,
) -> Result<String, String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    // Take the pooled runtime (or build one on first use on this thread), repointing
    // its session to this page.
    let mut rt = match RENDER_RT.with(|c| c.borrow_mut().take()) {
        Some(rt) => {
            reset_session(&rt, base, "", "");
            rt
        }
        None => make_runtime(base, "", ""),
    };

    let handle = rt.v8_isolate().thread_safe_handle();
    let done = Arc::new(AtomicBool::new(false));
    let watch = done.clone();
    let budget = std::time::Duration::from_millis(budget_ms);
    let watchdog = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        loop {
            if watch.load(Ordering::Relaxed) {
                break; // render completed → unparked us; no terminate
            }
            let elapsed = start.elapsed();
            if elapsed >= budget {
                handle.terminate_execution();
                break;
            }
            // Park until completion unparks us or the budget deadline lapses — no fixed
            // poll granularity, so a healthy render's join() returns in µs (the old 2ms
            // sleep added up to 2ms of join latency to EVERY pooled render).
            std::thread::park_timeout(budget - elapsed);
        }
    });

    let result = run_async_pooled(&mut rt, html, base, script).await;
    done.store(true, Ordering::Relaxed);
    watchdog.thread().unpark();
    let _ = watchdog.join();
    crate::browser_env::reset(); // clear the binding while the isolate is still alive

    match result {
        Ok(html) => {
            RENDER_RT.with(|c| *c.borrow_mut() = Some(rt)); // healthy → return to pool
            Ok(html)
        }
        // Poisoned (budget terminate leaves the isolate in a terminated state, etc.) —
        // drop the runtime so the next page rebuilds a clean one.
        Err(e) => Err(budget_msg(&e, budget_ms)),
    }
}

/// The boundary a page-script bundle places between successive `<script>` bodies so
/// the render tier can run each as a SEPARATE top-level program. A browser isolates
/// scripts: an uncaught error in one `<script>` does not abort the rest, while every
/// script's top-level `var`/`function`/`let`/`const` still populate the shared realm
/// scope (successive `execute_script`s reuse the same V8 context). A bundle with no
/// boundary is a single script (arbitrary callers) and runs whole, as before.
pub const SCRIPT_BOUNDARY: &str = "\n/*__ts_script_boundary_9f3a__*/\n";

/// Human-input synthesizer (installer). Evaluating this defines `globalThis.__hi`, a small
/// API that generates a realistic pointer path A→B and dispatches a coherent, **trusted**
/// pointer/mouse/keyboard sequence into the DOM the page's own listeners (and a BotGuard-class
/// collector) observe. It exists to satisfy the interaction-gate + input-entropy blocker in the
/// render isolate — the one place we control the whole event pipeline (unlike a real automated
/// browser, where injected events are trusted-but-CDP-detectable). Two path modes:
/// `"straight"` is a linear A→B at uniform cadence (a baseline / sanity path). `"human"` is a
/// cubic-Bézier arc bowed off the A→B line (curvature) with per-step Gaussian coordinate noise, a
/// slow→fast→slow velocity profile (non-uniform time deltas), and an occasional overshoot+settle
/// near the target.
/// Dispatched events carry `isTrusted:true` (via the `__trusted` flag the Event.prototype getter
/// reads) and `timeStamp = performance.now()` (origin-relative, fractional), on the hi-res clock.
///
/// Callers that need to leave no enumerable global should `delete globalThis.__hi` after use;
/// tests read `__hi.path(...)` directly. This is generation + dispatch only — it does NOT defeat
/// GPU/audio/server-side scoring; it addresses the input-entropy + interaction-gate signal.
pub const HUMAN_INPUT_JS: &str = r#"(() => {
  const rand = Math.random;
  // Gaussian noise (Box–Muller) — real cursor coordinates jitter, they don't lie on an ideal curve.
  const gauss = (sd) => { let u = 0, v = 0; while (!u) u = rand(); while (!v) v = rand(); return sd * Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * v); };
  const bez = (p0, p1, p2, p3, t) => { const m = 1 - t; return m*m*m*p0 + 3*m*m*t*p1 + 3*m*t*t*p2 + t*t*t*p3; };
  const ease = (u) => (u < 0.5 ? 2*u*u : 1 - Math.pow(-2*u + 2, 2) / 2); // ease-in-out (velocity)

  // Generate an ordered [{x,y,t}] path from A to B. Sampled the way a real browser samples a
  // pointer GESTURE: NOT one event per pixel/ms, but a coordinate read every ~12ms (± a few ms
  // stddev) along the trajectory — so inter-event Δt clusters near the ~10–15ms sampling rate and
  // SPEED shows up as distance-per-sample (small near the ends, large mid-flight), not as Δt.
  // `t` is an origin-relative ms offset from gesture start. mode: "straight" | "human".
  function path(ax, ay, bx, by, mode) {
    const dist = Math.hypot(bx - ax, by - ay) || 1;
    // Gesture duration scales with distance (a longer throw takes longer), human range.
    const dur = mode === "human" ? 120 + dist * (0.7 + rand() * 0.6) : Math.max(50, dist * 0.5);
    let c1x, c1y, c2x, c2y;
    if (mode === "human") {
      const nx = -(by - ay) / dist, ny = (bx - ax) / dist;          // unit normal to A→B
      const bow = (rand() * 0.5 + 0.15) * dist * (rand() < 0.5 ? -1 : 1); // arc height, either side
      c1x = ax + (bx - ax) / 3 + nx * bow * 0.7; c1y = ay + (by - ay) / 3 + ny * bow * 0.7;
      c2x = ax + 2 * (bx - ax) / 3 + nx * bow;   c2y = ay + 2 * (by - ay) / 3 + ny * bow;
    } else {
      c1x = ax + (bx - ax) / 3; c1y = ay + (by - ay) / 3;
      c2x = ax + 2 * (bx - ax) / 3; c2y = ay + 2 * (by - ay) / 3;
    }
    const pts = [];
    let t = 0;
    while (t < dur) {
      const u = t / dur;
      const e = mode === "human" ? ease(u) : u;                     // ease → varying speed
      let x = bez(ax, c1x, c2x, bx, e), y = bez(ay, c1y, c2y, by, e);
      if (mode === "human" && t > 0) { x += gauss(1.2); y += gauss(1.2); }
      pts.push({ x: Math.round(x), y: Math.round(y), t: Math.round(t * 10) / 10 });
      // Browser pointer sampling interval: ~12ms with a few ms of stddev, floored so it never
      // degenerates to a per-ms firehose.
      t += Math.max(6, 12 + gauss(2.5));
    }
    pts.push({ x: bx, y: by, t: Math.round(dur * 10) / 10 });       // final sample lands on B
    if (mode === "human" && rand() < 0.6) {                         // overshoot + settle
      let tt = dur + Math.max(6, 12 + gauss(2.5));
      pts.push({ x: Math.round(bx + gauss(3)), y: Math.round(by + gauss(3)), t: Math.round(tt * 10) / 10 });
      tt += 16 + rand() * 12; pts.push({ x: bx, y: by, t: Math.round(tt * 10) / 10 });
    }
    return pts;
  }

  // A jittered programmatic delay: base ms plus a random offset in [0, jitter]. Humans don't
  // react instantly (bots fire at t=0) — used as the START delay before an interaction, and as
  // the reaction gap between move-end and click.
  const delay = (base, jitter) => Math.max(0, (base || 0) + rand() * (jitter || 0));

  // Human keyboard timeline: for each character emit keydown → keypress → input → keyup, spaced
  // by a realistic inter-key gap (dwell + flight). Gap ~90–170ms, +extra after space/punctuation,
  // with an occasional "think" pause; each key is held ~40–90ms (keydown→keyup). `base` is the
  // origin-relative ms at which typing starts. Returns { events:[{name,type,props,ts}], end }.
  // Realistic KeyboardEvent init for a character or named key. Real Chrome events carry
  // key/code/keyCode/which (plus location) — a `{key}`-only event is a tell (BotGuard reads
  // keyCode/code). `code` is the physical key ("KeyR"/"Digit1"/"Enter"/"Space"); keyCode/which
  // are the legacy numeric codes (kept equal, as Chrome does).
  function keyInfo(ch) {
    const NAMED = {
      Enter: { code: "Enter", keyCode: 13 }, Tab: { code: "Tab", keyCode: 9 },
      Backspace: { code: "Backspace", keyCode: 8 }, Escape: { code: "Escape", keyCode: 27 },
      " ": { code: "Space", keyCode: 32 },
    };
    if (NAMED[ch]) return { key: ch === " " ? " " : ch, code: NAMED[ch].code, keyCode: NAMED[ch].keyCode, which: NAMED[ch].keyCode };
    let code;
    if (/[a-z]/i.test(ch)) code = "Key" + ch.toUpperCase();
    else if (/[0-9]/.test(ch)) code = "Digit" + ch;
    else code = "";
    const kc = ch.length === 1 ? ch.toUpperCase().charCodeAt(0) : 0;
    return { key: ch, code, keyCode: kc, which: kc };
  }

  function typePlan(text, base) {
    const events = [];
    let t = base || 0;
    const s = String(text);
    for (let i = 0; i < s.length; i++) {
      const ch = s[i];
      let gap = 90 + rand() * 80;                          // base inter-key flight
      if (i > 0 && (s[i - 1] === " " || /[.,!?;:]/.test(s[i - 1]))) gap += 60 + rand() * 90;
      if (rand() < 0.06) gap += 250 + rand() * 450;        // occasional hesitation
      t += gap;
      const hold = 40 + rand() * 50;                       // key dwell time
      const k = keyInfo(ch);
      // Full per-character sequence a browser fires for a printable key: keydown → keypress →
      // beforeinput → input → keyup, each with the real key identifiers.
      events.push({ name: "KeyboardEvent", type: "keydown", props: k, ts: Math.round(t * 10) / 10 });
      events.push({ name: "KeyboardEvent", type: "keypress", props: { key: k.key, code: k.code, keyCode: k.which, which: k.which, charCode: k.which }, ts: Math.round((t + 1) * 10) / 10 });
      events.push({ name: "InputEvent", type: "beforeinput", props: { data: ch, inputType: "insertText" }, ts: Math.round((t + 1.5) * 10) / 10 });
      events.push({ name: "InputEvent", type: "input", props: { data: ch, inputType: "insertText" }, ts: Math.round((t + 2) * 10) / 10 });
      events.push({ name: "KeyboardEvent", type: "keyup", props: k, ts: Math.round((t + hold) * 10) / 10 });
    }
    return { events, end: t };
  }

  // A single named-key press (e.g. "Enter") — keydown → keypress (for keys that produce one) →
  // keyup, with the real key identifiers. Returns { events, end } like typePlan.
  function pressPlan(keyName, base) {
    let t = (base || 0) + 30 + rand() * 40;                 // reaction before the press
    const k = keyInfo(keyName);
    const hold = 40 + rand() * 50;
    const events = [
      { name: "KeyboardEvent", type: "keydown", props: k, ts: Math.round(t * 10) / 10 },
    ];
    // keypress fires for Enter + printable keys (not for Tab/Escape/arrows).
    if (keyName === "Enter" || k.key.length === 1) {
      events.push({ name: "KeyboardEvent", type: "keypress", props: { key: k.key, code: k.code, keyCode: k.which, which: k.which, charCode: keyName === "Enter" ? 13 : k.which }, ts: Math.round((t + 1) * 10) / 10 });
    }
    events.push({ name: "KeyboardEvent", type: "keyup", props: k, ts: Math.round((t + hold) * 10) / 10 });
    return { events, end: t + hold };
  }

  // Build a trusted event (isTrusted:true via the __trusted flag + Event.prototype getter),
  // stamped with an EXPLICIT origin-relative `ts` (the human timeline) — not live performance.now,
  // so the cadence shows in event.timeStamp deltas regardless of the isolate's virtual clock.
  function ev(name, type, props, ts) {
    const C = globalThis[name] || globalThis.Event;
    const e = new C(type, Object.assign({ bubbles: true, cancelable: true }, props || {}));
    try { delete e.isTrusted; } catch (_) {}   // drop the ctor's own false → proto getter wins
    e.__trusted = true;
    try { e.timeStamp = ts == null ? performance.now() : ts; } catch (_) {}
    return e;
  }
  const fire = (target, e) => { try { (target || document).dispatchEvent(e); } catch (_) {} };

  // Dispatch a pointer/mouse move stream along `pts`, each stamped base + p.t (human timeline).
  function move(target, pts, buttons, base) {
    const b = base || 0;
    for (const p of pts) {
      const props = { clientX: p.x, clientY: p.y, pageX: p.x, pageY: p.y, buttons: buttons || 0 };
      fire(target, ev("PointerEvent", "pointermove", props, b + p.t));
      fire(target, ev("MouseEvent", "mousemove", props, b + p.t));
    }
    return pts.length ? b + pts[pts.length - 1].t : b;
  }

  // Type `text` into `target` on the human keyboard timeline starting at `base`. Returns end ts.
  function type(target, text, base) {
    const plan = typePlan(text, base);
    for (const k of plan.events) fire(target, ev(k.name, k.type, k.props, k.ts));
    return plan.end;
  }

  // Full human interaction (synchronous dispatch, human-timeline timestamps): optional start
  // delay → curved move to (bx,by) → reaction gap → press/focus → type → release/click.
  function moveAndClick(target, ax, ay, bx, by, text, opts) {
    opts = opts || {};
    const base = (performance.now ? performance.now() : 0) + delay(opts.startDelay || 0, opts.startJitter || 0);
    const pts = path(ax, ay, bx, by, "human");
    let t = move(target, pts, 0, base);
    t += delay(40, 120);                                    // reaction before the click
    const at = { clientX: bx, clientY: by, pageX: bx, pageY: by };
    fire(target, ev("PointerEvent", "pointerdown", Object.assign({ buttons: 1 }, at), t));
    fire(target, ev("MouseEvent", "mousedown", Object.assign({ buttons: 1 }, at), t + 1));
    fire(target, ev("FocusEvent", "focus", {}, t + 2));
    if (text) t = type(target, text, t + delay(120, 180));
    fire(target, ev("PointerEvent", "pointerup", at, t + 30));
    fire(target, ev("MouseEvent", "mouseup", at, t + 31));
    fire(target, ev("MouseEvent", "click", at, t + 32));
    return { points: pts, start: base, end: t + 32 };
  }

  // Async variant: schedule the same interaction over the VIRTUAL event loop via setTimeout, so
  // events are genuinely SPACED in (virtual) time — the collector sees them arrive over ~seconds,
  // not all in one drain — while still stamped on the human timeline. Resolves when done.
  function play(target, o) {
    o = o || {};
    return new Promise((resolve) => {
      const start = delay(o.startDelay == null ? 300 : o.startDelay, o.startJitter == null ? 500 : o.startJitter);
      const perfBase = (performance.now ? performance.now() : 0) + start;
      const pts = path(o.startX || 0, o.startY || 0, o.toX || 0, o.toY || 0, "human");
      const clickAt = pts.length ? pts[pts.length - 1].t : 0;
      const at = { clientX: o.toX || 0, clientY: o.toY || 0, pageX: o.toX || 0, pageY: o.toY || 0 };
      const typeStart = clickAt + delay(150, 200);
      const plan = o.text ? typePlan(o.text, typeStart) : { events: [], end: typeStart };
      const sched = (wait, name, type, props, ts) =>
        setTimeout(() => fire(target, ev(name, type, props, perfBase + ts)), Math.max(0, Math.round(start + wait)));
      for (const p of pts) {
        sched(p.t, "PointerEvent", "pointermove", { clientX: p.x, clientY: p.y, pageX: p.x, pageY: p.y }, p.t);
        sched(p.t, "MouseEvent", "mousemove", { clientX: p.x, clientY: p.y, pageX: p.x, pageY: p.y }, p.t);
      }
      sched(clickAt, "PointerEvent", "pointerdown", Object.assign({ buttons: 1 }, at), clickAt);
      sched(clickAt, "MouseEvent", "mousedown", Object.assign({ buttons: 1 }, at), clickAt);
      sched(clickAt, "FocusEvent", "focus", {}, clickAt);
      for (const k of plan.events) sched(k.ts, k.name, k.type, k.props, k.ts);
      sched(plan.end + 30, "MouseEvent", "click", at, plan.end + 30);
      setTimeout(() => resolve({ start: perfBase, end: perfBase + plan.end + 30 }), Math.max(0, Math.round(start + plan.end + 40)));
    });
  }

  // Composable interaction SEQUENCE — the general API. Plays an ordered list of steps over the
  // virtual event loop, threading the cursor position and a running clock so every event is both
  // temporally spaced (setTimeout) and stamped on one continuous human timeline. Steps:
  //   { move:{toX,toY} }  curved human move from the current cursor to the target
  //   { click:true }      pointerdown+mousedown → a real press DWELL (~60–140ms) → up → click
  //   { focus:true }      a focus event
  //   { type:"text" }     human keyboard typing (per-key rhythm) at the current focus
  //   { wait:ms }         an explicit pause
  // e.g. move → click (search box) → focus → type("query") → move → click (button):
  //   __hi.sequence(document, [ {move:{toX:400,toY:60}}, {click:true}, {focus:true},
  //     {type:"weather"}, {move:{toX:520,toY:62}}, {click:true} ], { startDelay:400, startJitter:600 });
  function sequence(target, steps, o) {
    o = o || {};
    return new Promise((resolve) => {
      let cx = o.startX || 0, cy = o.startY || 0, t = 0;
      let focused = null;                                   // the element currently holding focus
      let hovered = null;                                   // the element currently under the cursor
      const startDelay = delay(o.startDelay == null ? 300 : o.startDelay, o.startJitter == null ? 500 : o.startJitter);
      const perfBase = (performance.now ? performance.now() : 0) + startDelay;
      const jobs = [];
      const at = () => ({ clientX: cx, clientY: cy, pageX: cx, pageY: cy });
      // Events fire at the element when one is given (focus/blur/click target it, and bubble to
      // document/window); otherwise at `target` (document) at the cursor.
      const job = (tgt, name, type, props, ts) => jobs.push({ tgt: tgt || target, name, type, props, ts });
      // Blur fires both blur (non-bubbling) and focusout (bubbling), like real Chrome.
      const blurIfFocused = (ts) => {
        if (focused) {
          job(focused, "FocusEvent", "blur", {}, ts);
          job(focused, "FocusEvent", "focusout", { bubbles: true }, ts + 0.1);
          focused = null;
        }
      };
      for (const step of steps || []) {
        if (step.move) {
          const el = step.move.el || null;
          // Leaving the previously-hovered element: mouseout/pointerout bubble; mouseleave/
          // pointerleave don't (fire on the element), at the OLD cursor position.
          if (hovered && hovered !== el) {
            job(target, "MouseEvent", "mouseout", at(), t);
            job(hovered, "MouseEvent", "mouseleave", at(), t);
            job(hovered, "PointerEvent", "pointerout", at(), t);
            job(hovered, "PointerEvent", "pointerleave", at(), t);
          }
          const pts = path(cx, cy, step.move.toX, step.move.toY, "human");
          for (const p of pts) {
            const mp = { clientX: p.x, clientY: p.y, pageX: p.x, pageY: p.y };
            job(target, "PointerEvent", "pointermove", mp, t + p.t);
            job(target, "MouseEvent", "mousemove", mp, t + p.t);
          }
          t += pts.length ? pts[pts.length - 1].t : 0;
          cx = step.move.toX; cy = step.move.toY;
          // Arriving over a target element: mouseover/pointerover bubble; mouseenter/pointerenter
          // don't (fire on the element), at the NEW cursor position.
          if (el && hovered !== el) {
            job(target, "MouseEvent", "mouseover", at(), t);
            job(el, "MouseEvent", "mouseenter", at(), t);
            job(target, "PointerEvent", "pointerover", at(), t);
            job(el, "PointerEvent", "pointerenter", at(), t);
            hovered = el;
          }
        } else if (step.click) {
          t += delay(60, 120);                              // reaction before press
          // A mousedown elsewhere steals focus — a real user clicking the search button blurs
          // the input first. Blur any prior focus at press time (unless clicking that same el).
          if (focused && focused !== step.click) blurIfFocused(t);
          job(target, "PointerEvent", "pointerdown", Object.assign({ buttons: 1 }, at()), t);
          job(step.click === true ? target : step.click, "MouseEvent", "mousedown", Object.assign({ buttons: 1 }, at()), t + 1);
          t += 60 + rand() * 80;                            // real DWELL between down and up
          job(target, "PointerEvent", "pointerup", at(), t);
          job(step.click === true ? target : step.click, "MouseEvent", "mouseup", at(), t + 1);
          job(step.click === true ? target : step.click, "MouseEvent", "click", at(), t + 2);
          t += 2;
        } else if (step.focus) {
          if (focused && focused !== step.focus) blurIfFocused(t);   // focus change blurs the old
          const el = step.focus === true ? target : step.focus;
          job(el, "FocusEvent", "focus", {}, t);                     // non-bubbling
          job(el, "FocusEvent", "focusin", { bubbles: true }, t + 0.1); // bubbling
          focused = el; t += delay(20, 60);
        } else if (step.blur) {
          blurIfFocused(t); t += delay(10, 40);
        } else if (step.type != null) {
          t += delay(120, 180);
          const el = focused || target;
          const plan = typePlan(step.type, t);
          // Update the focused field's value as each character's `input` event fires — a real
          // browser mutates .value on keystroke (the value the `input` handler and a later form
          // submit read). Without this, typing is cosmetic (events only) and a submitted form
          // carries an empty field.
          const chars = String(step.type);
          let acc = "";
          try { if (el && el !== target && el.value != null) acc = String(el.value); } catch (e) {}
          let ci = 0;
          for (const k of plan.events) {
            const j = { tgt: el, name: k.name, type: k.type, props: k.props, ts: k.ts };
            if (k.type === "input") { acc += chars[ci++] || ""; j.setValue = acc; }
            jobs.push(j);
          }
          t = plan.end;
        } else if (step.press != null) {
          // A named-key press (e.g. Enter to submit a search) at the focused field — fires the
          // real keydown/keypress/keyup so the page's key handler runs (google's Enter handler
          // builds the /search URL); the nav-follow then loads it.
          const plan = pressPlan(String(step.press), t);
          for (const k of plan.events) job(focused || target, k.name, k.type, k.props, k.ts);
          t = plan.end;
        } else if (step.wait != null) {
          t += step.wait;
        }
      }
      blurIfFocused(t);                                     // leave nothing focused at the end
      for (const j of jobs) setTimeout(() => {
        // Apply the typed value just before its `input` event, so the handler + a later
        // form submit see the updated field value (real-browser order).
        if (j.setValue != null) { try { j.tgt.value = j.setValue; } catch (e) {} }
        fire(j.tgt, ev(j.name, j.type, j.props, perfBase + j.ts));
      }, Math.max(0, Math.round(startDelay + j.ts)));
      setTimeout(() => resolve({ start: perfBase, end: perfBase + t }), Math.max(0, Math.round(startDelay + t + 20)));
    });
  }

  globalThis.__hi = { path, typePlan, pressPlan, keyInfo, delay, move, type, moveAndClick, play, sequence, ev };
})()"#;

/// Run a page-script bundle the browser way: each boundary-delimited part as its own
/// top-level `execute_script`, so a throwing script is isolated from the others
/// (logged + skipped) instead of aborting every later script. Only a real isolate
/// termination (the render-budget watchdog / cancellation) stops the loop and
/// propagates — a plain JS throw leaves the isolate healthy to run the next part.
fn exec_page_scripts(rt: &mut JsRuntime, bundle: &str) -> Result<(), String> {
    for part in bundle.split(SCRIPT_BOUNDARY) {
        if part.trim().is_empty() {
            continue;
        }
        if let Err(e) = rt.execute_script("<page>", part.to_string()) {
            if rt.v8_isolate().is_execution_terminating() {
                return Err(e.to_string()); // budget / termination — stop the page
            }
            // Browser semantics: a later `<script>` still runs after one throws.
            eprintln!("script error: {e}");
        }
    }
    Ok(())
}

// Page-load lifecycle phases, driven by the async render loop so the main document/window fire
// the real sequence a browser does — a collector that gates init on these (google's homepage
// registers on DOMContentLoaded/load) otherwise never runs. `document.readyState` reads a hidden
// Symbol slot (installed in ENV_BOOTSTRAP); each phase sets it + dispatches the matching TRUSTED
// events. All fully guarded so a lifecycle hiccup can't break a render.
const LIFECYCLE_LOADING: &str =
    r#"try { document[Symbol.for("__ts_rs")] = "loading"; } catch (e) {}"#;
const LIFECYCLE_INTERACTIVE: &str = r#"(() => { try {
  document[Symbol.for("__ts_rs")] = "interactive";
  const mk = (t, b) => { let e; try { e = new Event(t, { bubbles: !!b }); } catch (_) { e = { type: t }; } try { delete e.isTrusted; } catch (_) {} e.__trusted = true; try { e.timeStamp = performance.now(); } catch (_) {} return e; };
  try { document.dispatchEvent(mk("readystatechange", false)); } catch (_) {}
  try { if (typeof document.onreadystatechange === "function") document.onreadystatechange(mk("readystatechange", false)); } catch (_) {}
  try { document.dispatchEvent(mk("DOMContentLoaded", true)); } catch (_) {}
  try { globalThis.dispatchEvent(mk("DOMContentLoaded", true)); } catch (_) {} // window listeners (DCL bubbles to window)
} catch (e) {} })()"#;
const LIFECYCLE_COMPLETE: &str = r#"(() => { try {
  document[Symbol.for("__ts_rs")] = "complete";
  const mk = (t, b) => { let e; try { e = new Event(t, { bubbles: !!b }); } catch (_) { e = { type: t }; } try { delete e.isTrusted; } catch (_) {} e.__trusted = true; try { e.timeStamp = performance.now(); } catch (_) {} return e; };
  try { document.dispatchEvent(mk("readystatechange", false)); } catch (_) {}
  try { if (typeof document.onreadystatechange === "function") document.onreadystatechange(mk("readystatechange", false)); } catch (_) {}
  const load = mk("load", false);
  try { globalThis.dispatchEvent(load); } catch (_) {}
  try { if (typeof globalThis.onload === "function") globalThis.onload(load); } catch (_) {}
  try { globalThis.dispatchEvent(mk("pageshow", false)); } catch (_) {}
} catch (e) {} })()"#;

async fn run_async(
    rt: &mut JsRuntime,
    html: &str,
    base: &str,
    script: &str,
) -> Result<String, String> {
    install_dom(rt, html, base)?;
    let _ = rt.execute_script("<rs-loading>", LIFECYCLE_LOADING); // readyState "loading" while scripts run
    exec_page_scripts(rt, script)?;
    let _ = rt.execute_script("<rs-interactive>", LIFECYCLE_INTERACTIVE); // interactive + DOMContentLoaded
    drain_event_loop(rt).await?; // DCL handlers + promises/microtasks + fetch from the page
    rt.execute_script("<timers>", "__runTimers()")
        .map_err(|e| e.to_string())?;
    drain_event_loop(rt).await?; // promises queued by timer callbacks
    let _ = rt.execute_script("<rs-complete>", LIFECYCLE_COMPLETE); // complete + window load + pageshow
    drain_event_loop(rt).await?; // load handlers' async work
    Ok(crate::browser_env::document_html())
}

// Cross-page global scrub for the POOLED render path. A browser gives every navigation
// a fresh global; a reused isolate does not, so a page that assigns `window.X = …` would
// leak `X` into the next page. On the first pooled render this records the clean global
// key set (env globals from `ENV_BOOTSTRAP` + V8 builtins); on every later render it
// deletes any own-key NOT in that baseline, restoring fresh-navigation semantics for the
// common "page sets window globals" case. (Builtins MUTATED in place aren't reverted —
// `ENV_BOOTSTRAP` re-runs each page and re-seeds the env, covering the usual polyfills.)
const SCRUB_GLOBALS: &str = r#"(() => {
  if (!globalThis.__TS_BASELINE) {
    const b = new Set(Object.getOwnPropertyNames(globalThis));
    b.add("__TS_BASELINE");
    globalThis.__TS_BASELINE = b;
    return;
  }
  for (const k of Object.getOwnPropertyNames(globalThis)) {
    if (!globalThis.__TS_BASELINE.has(k)) {
      try { delete globalThis[k]; } catch (_e) { /* non-configurable: leave it */ }
    }
  }
})()"#;

// Pooled-path render: like [`run_async`] but scrubs page-added globals (see
// [`SCRUB_GLOBALS`]) right after the env is (re)installed and before the page's own
// script runs, so a reused isolate behaves like a fresh navigation.
async fn run_async_pooled(
    rt: &mut JsRuntime,
    html: &str,
    base: &str,
    script: &str,
) -> Result<String, String> {
    install_dom(rt, html, base)?;
    rt.execute_script("<scrub>", SCRUB_GLOBALS)
        .map_err(|e| e.to_string())?;
    let _ = rt.execute_script("<rs-loading>", LIFECYCLE_LOADING);
    exec_page_scripts(rt, script)?;
    let _ = rt.execute_script("<rs-interactive>", LIFECYCLE_INTERACTIVE); // DOMContentLoaded (before load)
    drain_event_loop(rt).await?;
    rt.execute_script("<timers>", "__runTimers()")
        .map_err(|e| e.to_string())?;
    drain_event_loop(rt).await?;
    let _ = rt.execute_script("<rs-complete>", LIFECYCLE_COMPLETE); // window load + pageshow (after DCL)
    drain_event_loop(rt).await?;
    Ok(crate::browser_env::document_html())
}

/// Hydrate a page by running ITS OWN scripts the way a browser does — execute each
/// `<script>` (inline + dynamically-injected chunks), fetching + running external
/// `src` and firing `onload` so a webpack-style chunk loader resolves and the app
/// mounts. No bundle concatenation by the caller, no framework runtime from us: the
/// page's own bundle drives itself. Bounded by [`DEFAULT_RENDER_BUDGET_MS`].
pub async fn render_hydrate(html: &str, base: &str) -> Result<String, String> {
    render_hydrate_with_budget(html, base, "", "", DEFAULT_RENDER_BUDGET_MS).await
}

/// [`render_hydrate`] with the page's cookies (a `storageState` JSON string, "" for
/// none) seeded into the jar so session-authenticated hydration works, a custom
/// User-Agent ("" for the default), plus an explicit execution budget (ms) + watchdog.
pub async fn render_hydrate_with_budget(
    html: &str,
    base: &str,
    cookies: &str,
    ua: &str,
    budget_ms: u64,
) -> Result<String, String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let mut rt = make_runtime(base, cookies, ua);
    let handle = rt.v8_isolate().thread_safe_handle();
    let done = Arc::new(AtomicBool::new(false));
    let watch = done.clone();
    let watchdog = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        while !watch.load(Ordering::Relaxed) {
            if start.elapsed() >= std::time::Duration::from_millis(budget_ms) {
                handle.terminate_execution();
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    });

    let result = run_hydrate(&mut rt, html, base).await;
    done.store(true, Ordering::Relaxed);
    let _ = watchdog.join();
    // Best-effort on a budget-exceed: a dev-mode SPA (Next `next dev`) can loop past the
    // budget without ever reaching idle, yet the partial DOM rendered so far is exactly
    // what a probe (readable-React-error diagnosis) needs. Mirror `PageSession.eval`:
    // clear the terminate state so the isolate is usable, then return the reached DOM
    // instead of discarding it. Genuine non-budget errors (install/parse) still propagate.
    let out = best_effort_on_budget(&mut rt, result, budget_ms);
    crate::browser_env::reset();
    out
}

// Resolve a hydrate-path result into best-effort HTML: a clean `Ok` passes through;
// a budget-terminate is downgraded to the partial serialized DOM (terminate state
// cleared first so the read can run); any other error propagates relabeled.
fn best_effort_on_budget(
    rt: &mut JsRuntime,
    result: Result<String, String>,
    budget_ms: u64,
) -> Result<String, String> {
    match result {
        Ok(html) => Ok(html),
        Err(e) if e.contains("terminat") || e.contains("execution") => {
            rt.v8_isolate().cancel_terminate_execution();
            Ok(crate::browser_env::document_html())
        }
        // A genuine JS error mid-hydration (a page script that throws, an unhandled
        // rejection) used to discard everything. But by this point the DOM is
        // installed and partially mutated by the scripts that DID run — partial
        // hydration beats none (e.g. a jQuery site whose skin JS reparents the DOM
        // before a later analytics script throws). Return the reached DOM if there
        // is one; only propagate when nothing was rendered (install/parse failure).
        Err(e) => {
            rt.v8_isolate().cancel_terminate_execution();
            let dom = crate::browser_env::document_html();
            if dom.trim().is_empty() {
                Err(budget_msg(&e, budget_ms))
            } else {
                Ok(dom)
            }
        }
    }
}

/// Run `script` over `html`'s DOM, drive the event loop + hydration drain, then
/// return the string value of `globalThis.__RESULT`. Backs the MCP `run_playwright`
/// tool: the caller frames a program that runs a Playwright-style script and stashes
/// a JSON result in `__RESULT`. Bounded by [`DEFAULT_RENDER_BUDGET_MS`].
pub async fn eval_async(html: &str, base: &str, script: &str) -> Result<String, String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let mut rt = make_runtime(base, "", "");
    let handle = rt.v8_isolate().thread_safe_handle();
    let done = Arc::new(AtomicBool::new(false));
    let watch = done.clone();
    let watchdog = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        while !watch.load(Ordering::Relaxed) {
            if start.elapsed() >= std::time::Duration::from_millis(DEFAULT_RENDER_BUDGET_MS) {
                handle.terminate_execution();
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    });
    let result = async {
        install_dom(&mut rt, html, base)?;
        rt.execute_script("<script>", script.to_string())
            .map_err(|e| e.to_string())?;
        drain_event_loop(&mut rt).await?;
        rt.execute_script("<timers>", "__runTimers()")
            .map_err(|e| e.to_string())?;
        drain_event_loop(&mut rt).await?;
        let g = rt
            .execute_script("<result>", "String(globalThis.__RESULT || '')")
            .map_err(|e| e.to_string())?;
        read_string(&mut rt, g)
    }
    .await;
    done.store(true, Ordering::Relaxed);
    let _ = watchdog.join();
    let out = result.map_err(|e| budget_msg(&e, DEFAULT_RENDER_BUDGET_MS));
    crate::browser_env::reset();
    out
}

async fn run_hydrate(rt: &mut JsRuntime, html: &str, base: &str) -> Result<String, String> {
    install_dom(rt, html, base)?;
    // Unified event-loop pump. A single "run scripts+timers, then drain" pass isn't
    // enough for a real SPA: React kicks a fetch, yields, the fetch resolves, React
    // schedules MORE work (a timer via its MessageChannel), which schedules another
    // fetch… So loop — run the hydration pump, drain async ops (microtasks + fetches +
    // injected-script loads), check whether JS still has queued work — until it
    // quiesces. The watchdog bounds wall time; MAX_PUMPS bounds a pathological spin.
    const MAX_PUMPS: usize = 500;
    for _ in 0..MAX_PUMPS {
        rt.execute_script("<hydrate>", "globalThis.__tcHydrate = __hydrate();")
            .map_err(|e| e.to_string())?;
        drain_event_loop(rt).await?;
        drain_module_scripts(rt, base).await?;
        drain_event_loop(rt).await?;
        let pending = rt
            .execute_script("<pending>", "__pendingWork()")
            .map_err(|e| e.to_string())?;
        if read_string(rt, pending)? != "1" {
            break;
        }
    }
    Ok(crate::browser_env::document_html())
}

// Evaluate every un-run ES-module `<script>` (claimed via `__takeModuleScript`) through
// deno_core's real module graph: inline modules load from their own code, `src` modules
// load by URL (the `NetModuleLoader` fetches them + their imports over the host net).
// This is the path a Next dev / turbopack build (served as ES modules) needs to hydrate.
async fn drain_module_scripts(rt: &mut JsRuntime, base: &str) -> Result<(), String> {
    for n in 0..1000usize {
        rt.execute_script("<take-mod>", "globalThis.__takeModuleScript();")
            .map_err(|e| e.to_string())?;
        let g = rt
            .execute_script(
                "<take-mod-r>",
                "String(globalThis.__RESULT == null ? '' : globalThis.__RESULT)",
            )
            .map_err(|e| e.to_string())?;
        let desc = read_string(rt, g)?;
        if desc.is_empty() {
            break;
        }
        let v: deno_core::serde_json::Value =
            deno_core::serde_json::from_str(&desc).unwrap_or(deno_core::serde_json::Value::Null);
        let src = v.get("src").and_then(|s| s.as_str()).unwrap_or("");
        let code = v
            .get("code")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let spec_str = if src.is_empty() {
            format!("{}#tcmod-{n}", base.split('#').next().unwrap_or(base))
        } else {
            resolve(base, src).unwrap_or_else(|| src.to_string())
        };
        let Ok(spec) = ModuleSpecifier::parse(&spec_str) else {
            continue;
        };
        // Code in hand (inline module, OR a `<script src>` chunk whose fetched body was
        // ESM — `__execScriptEl` already fetched it and queued it) → evaluate from that
        // code with `spec` as its identity so the import graph still resolves relative to
        // the src URL. Only a bare `src` with no body re-fetches through the loader.
        let loaded = if code.is_empty() {
            rt.load_side_es_module(&spec).await
        } else {
            rt.load_side_es_module_from_code(&spec, code).await
        };
        match loaded {
            Ok(id) => {
                // Point document.currentScript at the chunk's element for the duration of
                // evaluation: turbopack chunks self-register via TURBOPACK.push([document
                // .currentScript, …]) and key the chunk by currentScript.src. Without this
                // an ESM chunk registers under a stale path and the entry's chunk-load
                // Promise.all never resolves (→ the app never hydrates, no error).
                rt.execute_script(
                    "<mod-cs>",
                    "try{document.currentScript = globalThis.__currentModuleEl || null;}catch(_e){}",
                )
                .map_err(|e| e.to_string())?;
                let ev = rt.mod_evaluate(id).await;
                rt.execute_script(
                    "<mod-cs0>",
                    "try{document.currentScript = null;}catch(_e){}",
                )
                .map_err(|e| e.to_string())?;
                if let Err(e) = ev {
                    eprintln!("module eval error ({spec_str}): {e}");
                }
            }
            Err(e) => eprintln!("module load error ({spec_str}): {e}"),
        }
    }
    Ok(())
}

// RAII runaway-execution watchdog: terminates the isolate if an op runs past the
// budget, and is cancelled (thread joined) on drop. Replaces the hand-rolled
// done-flag + spawn + join that each one-shot entry point repeats.
struct Watchdog {
    done: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Watchdog {
    fn start(handle: v8::IsolateHandle, budget_ms: u64) -> Self {
        use std::sync::atomic::Ordering;
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let watch = done.clone();
        let thread = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            while !watch.load(Ordering::Relaxed) {
                if start.elapsed() >= std::time::Duration::from_millis(budget_ms) {
                    handle.terminate_execution();
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        });
        Watchdog {
            done,
            thread: Some(thread),
        }
    }
}
impl Drop for Watchdog {
    fn drop(&mut self) {
        self.done.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// Drive the event loop to quiescence: drain async ops (microtasks + fetches), fire any
// queued virtual timers (React's scheduler posts work through them), and repeat until
// nothing is pending. Used after an interaction event re-enters the running app (the
// handler may setState → schedule a re-render → fetch → schedule more).
async fn drain_to_quiescence(rt: &mut JsRuntime) -> Result<(), String> {
    // Fresh virtual-time window for this interaction so its transitions (e.g. a closing
    // MUI modal's Fade-exit timer) fire + complete even when the clock is already large.
    rt.execute_script(
        "<reset-budget>",
        "globalThis.__resetTimerBudget && globalThis.__resetTimerBudget();",
    )
    .map_err(|e| e.to_string())?;
    const MAX_ROUNDS: usize = 500;
    // Stop early once the visible tree has been STABLE for this many rounds even though
    // timers keep firing: a real app's analytics/idle-scheduler never stops posting
    // timers, so "no timers queued" alone never holds — wait for the DOM to settle.
    const STABLE_ROUNDS: usize = 6;
    let mut stable = 0usize;
    let mut last_sig = String::new();
    for _ in 0..MAX_ROUNDS {
        // RUN any newly-injected <script>s, then drain. An interaction can pull a chunk at
        // runtime — Next's `dynamic(() => import('…'))` (lazy modals: AddVacationTimeModal,
        // etc.) appends a <script src> when the component first renders. Without running it
        // the chunk never executes, the import() promise never resolves, and the modal never
        // appears. __hydrate is idempotent (skips already-run scripts via __tcDone).
        rt.execute_script("<hydrate>", "globalThis.__tcHydrate = __hydrate();")
            .map_err(|e| e.to_string())?;
        drain_event_loop(rt).await?;
        let fired = rt
            .execute_script("<timers>", "__runTimers(2000)")
            .map_err(|e| e.to_string())?;
        drain_event_loop(rt).await?;
        let pending = rt
            .execute_script("<pending>", "__pendingWork()")
            .map_err(|e| e.to_string())?;
        let sig_v = rt
            .execute_script("<domsig>", "__domSig()")
            .map_err(|e| e.to_string())?;
        let fetches = rt
            .execute_script("<fetches>", "__pendingFetchCount()")
            .map_err(|e| e.to_string())?;
        let still = read_string(rt, pending)? == "1";
        let fired_any = read_string(rt, fired)? != "0";
        let awaiting_fetch = read_string(rt, fetches)? != "0";
        if !still && !fired_any {
            break; // genuinely idle
        }
        // A request is outstanding: keep pumping (don't let the stable-DOM early-out
        // fire) so the response's re-render — a modal close, a redirect — lands.
        if awaiting_fetch {
            stable = 0;
            continue;
        }
        let sig = read_string(rt, sig_v)?;
        if sig == last_sig {
            stable += 1;
            if stable >= STABLE_ROUNDS {
                break; // render settled; remaining timers are background churn
            }
        } else {
            stable = 0;
            last_sig = sig;
        }
    }
    Ok(())
}

/// A LIVE page: a persistent [`JsRuntime`] whose hydrated DOM + running JS (React, the
/// app's closures, its delegated event listeners) stay ALIVE across operations. Unlike
/// the one-shot `render_*`/`render_hydrate` paths — which serialize the DOM to a string
/// and `reset()` the binding after each call, destroying the running app — a session
/// keeps the app mounted so interactions dispatch REAL DOM events into it and the
/// re-render is observable. This is the browserless analog of a Playwright page.
///
/// The V8 isolate + the binding's thread-local DOM are NOT `Send`: a session must be
/// created and driven from a single owning thread (the napi layer pins one thread per
/// session). `close()` (or drop) resets the binding while the isolate is still alive.
pub struct PageSession {
    rt: JsRuntime,
    budget_ms: u64,
    closed: bool,
}

impl PageSession {
    /// Build the runtime, install + hydrate the page to quiescence, and KEEP IT ALIVE.
    pub async fn open(
        html: &str,
        base: &str,
        cookies: &str,
        ua: &str,
        budget_ms: u64,
    ) -> Result<Self, String> {
        let mut rt = make_runtime(base, cookies, ua);
        let result = {
            let _wd = Watchdog::start(rt.v8_isolate().thread_safe_handle(), budget_ms);
            run_hydrate(&mut rt, html, base).await
        };
        match result {
            Ok(_) => Ok(PageSession {
                rt,
                budget_ms,
                closed: false,
            }),
            // Best-effort on a budget-exceed: a dev-mode SPA whose hydration never reaches
            // idle still produced a partial DOM and a live app. Clear the terminate state
            // (so later `eval`s run) and KEEP the session alive — discarding it would lose
            // the partial render and force the caller back to the static HTML. Genuine
            // non-budget errors (install/parse) still fail the open.
            Err(e) if e.contains("terminat") || e.contains("execution") => {
                rt.v8_isolate().cancel_terminate_execution();
                Ok(PageSession {
                    rt,
                    budget_ms,
                    closed: false,
                })
            }
            Err(e) => {
                crate::browser_env::reset();
                Err(budget_msg(&e, budget_ms))
            }
        }
    }

    /// Run `script` in the LIVE isolate, then drain the event loop to quiescence so any
    /// work the script triggered (event handlers, re-render, fetch) completes. Returns
    /// `String(globalThis.__RESULT || '')` — scripts that need to return a value stash
    /// it there.
    pub async fn eval(&mut self, script: &str) -> Result<String, String> {
        const READ: &str = "String(globalThis.__RESULT == null ? '' : globalThis.__RESULT)";
        let budget = self.budget_ms;
        let drained = {
            let _wd = Watchdog::start(self.rt.v8_isolate().thread_safe_handle(), budget);
            let r = async {
                self.rt
                    .execute_script("<session-eval>", script.to_string())
                    .map_err(|e| e.to_string())?;
                drain_to_quiescence(&mut self.rt).await
            }
            .await;
            r
        };
        // The watchdog may have terminated mid-drain. Clear that terminate state so the
        // isolate is usable again, then read the result BEST-EFFORT: an interaction's
        // important effects (the login POST, a client navigation) land early in the
        // drain — the budget is normally hit later on background churn (analytics
        // polling, React's idle scheduler). Returning the reached state beats throwing.
        self.rt.v8_isolate().cancel_terminate_execution();
        match drained {
            Err(e) if !(e.contains("terminat") || e.contains("execution")) => Err(e),
            _ => self
                .rt
                .execute_script("<result>", READ)
                .map_err(|e| budget_msg(&e.to_string(), budget))
                .and_then(|g| read_string(&mut self.rt, g)),
        }
    }

    /// Serialize the CURRENT live DOM to HTML (no reset — the page stays alive).
    pub fn serialize(&self) -> String {
        crate::browser_env::document_html()
    }

    /// The page's cookies as a `storageState` JSON string (includes HttpOnly session
    /// cookies the in-isolate `document.cookie` can't see) — so a later navigation can
    /// carry the session established during this page's lifetime (e.g. after login).
    pub fn cookies(&self) -> String {
        let op_state = self.rt.op_state();
        let jar = op_state.borrow().borrow::<Jar>().clone();
        let s = jar.borrow().storage_state();
        s
    }

    /// Tear down: reset the binding while the isolate is still alive, then drop it.
    pub fn close(mut self) {
        self.closed = true;
        crate::browser_env::reset();
    }
}

impl Drop for PageSession {
    fn drop(&mut self) {
        if !self.closed {
            crate::browser_env::reset();
        }
    }
}

// A terminated isolate surfaces as a generic execution error; relabel it.
fn budget_msg(e: &str, budget_ms: u64) -> String {
    if e.contains("terminated") || e.contains("execution") {
        format!("render budget exceeded ({budget_ms}ms)")
    } else {
        e.to_string()
    }
}
