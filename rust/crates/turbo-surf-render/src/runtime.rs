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
/// Chrome 149 defaults. Pass `"{}"` (or `""`) to reset to defaults.
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
    match MEASURE_TEXT.read().ok().and_then(|g| g.as_ref().map(|f| f(text, family, size))) {
        Some((w, h)) => format!("[{w},{h}]"),
        None => "null".to_string(),
    }
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
        op_measure_text
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
// field below has a Chrome 149 default and is overridable by the matching key —
// settable per process via `set_fingerprint` (MCP `set_fingerprint` tool).
const __fp = (() => { try { return JSON.parse(Deno.core.ops.op_fingerprint()); } catch (e) { return {}; } })();
const __pick = (k, d) => (__fp[k] !== undefined ? __fp[k] : d);
const __ua = __pick("userAgent",
  (Deno.core.ops.op_user_agent && Deno.core.ops.op_user_agent()) ||
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/149.0.0.0 Safari/537.36");
const __major = String(__pick("chromeMajor", 149));
// Chrome ships exactly these five PDF-viewer plugins, all aliased to the internal
// viewer; `navigator.plugins.length === 0` is a classic headless giveaway.
const __plugin = (name) => ({ name, filename: "internal-pdf-viewer", description: "Portable Document Format", length: 1 });
const __plugins = ["PDF Viewer", "Chrome PDF Viewer", "Chromium PDF Viewer", "Microsoft Edge PDF Viewer", "WebKit built-in PDF"].map(__plugin);
const __langs = __pick("languages", ["en-US", "en"]);
const __platform = __pick("platform", "MacIntel");
const __uaPlatform = __pick("uaPlatform", "macOS");
globalThis.navigator = {
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
  // No network here — accept + report success, matching the spec's boolean return.
  sendBeacon: (_url, _data) => true,
  plugins: __plugins, mimeTypes: [],
  // NetworkInformation — real Chrome exposes it; anti-bot scripts (found via the
  // `probe` example on a real Akamai sensor) read it, and its absence is a tell.
  connection: __pick("connection", { effectiveType: "4g", rtt: 50, downlink: 10, saveData: false }),
  // UA-Client-Hints high-entropy surface, consistent with the UA above.
  userAgentData: __pick("userAgentData", {
    brands: [
      { brand: "Google Chrome", version: __major },
      { brand: "Chromium", version: __major },
      { brand: "Not)A;Brand", version: "24" },
    ],
    mobile: false,
    platform: __uaPlatform,
    getHighEntropyValues: async () => ({
      architecture: "arm", bitness: "64", model: "",
      platform: __uaPlatform, platformVersion: "15.0.0", uaFullVersion: __major + ".0.0.0",
    }),
  }),
  // In-memory clipboard: an app that writeText()s a value (e.g. a copy-link button)
  // and reads it back round-trips, with no OS clipboard.
  clipboard: (() => { let v = ""; return { writeText: async (t) => { v = String(t == null ? "" : t); }, readText: async () => v }; })(),
};
// `screen` — overridable as a unit; defaults to a common 1080p desktop.
{
  const __scr = __pick("screen", { width: 1920, height: 1080 });
  const __w = __scr.width || 1920, __h = __scr.height || 1080;
  globalThis.screen = {
    width: __w, height: __h, availWidth: __w, availHeight: __h,
    colorDepth: 24, pixelDepth: 24,
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
globalThis.localStorage = (() => {
  const m = new Map();
  return {
    getItem: (k) => (m.has(k) ? m.get(k) : null),
    setItem: (k, v) => m.set(k, String(v)),
    removeItem: (k) => m.delete(k),
    clear: () => m.clear(),
  };
})();
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
globalThis.requestAnimationFrame = (fn) => globalThis.setTimeout(fn, 16);
globalThis.cancelAnimationFrame = globalThis.clearTimeout;
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
    this.readyState = 0; this.status = 0; this.responseText = ""; this.response = "";
    this._h = {}; // request headers
    this._l = {}; // event listeners by type
  }
  open(method, url) { this._m = method || "GET"; this._u = url; this._setState(1); }
  setRequestHeader(k, v) { this._h[String(k)] = String(v); }
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
  send(body) {
    const self = this;
    globalThis
      .fetch(this._u, { method: this._m, body, headers: self._h })
      .then(async (r) => {
        self.status = r.status;
        self.responseText = await r.text();
        self.response = self.responseText;
        self._setState(4);
        self._emit("load");
        self._emit("loadend");
      })
      .catch(() => { self._setState(4); self._emit("error"); self._emit("loadend"); });
  }
};
// Observers: no live mutation notifications over the static tree → no-op stubs.
class __NoopObserver {
  constructor(cb) { this._cb = cb; }
  observe() {}
  unobserve() {}
  disconnect() {}
  takeRecords() { return []; }
}
globalThis.MutationObserver = __NoopObserver;
globalThis.IntersectionObserver = __NoopObserver;
globalThis.ResizeObserver = __NoopObserver;
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
// MessageChannel — React 18's scheduler drains its work queue by posting to a
// MessagePort and running the handler on the other port's onmessage. Route the
// message through the timer queue (setTimeout 0) so the hydration pump drains it;
// without this, React's scheduled mount/hydration never runs.
globalThis.MessageChannel = class MessageChannel {
  constructor() {
    const p1 = { onmessage: null, close() {}, start() {}, addEventListener() {}, removeEventListener() {} };
    const p2 = { onmessage: null, close() {}, start() {}, addEventListener() {}, removeEventListener() {} };
    p1.postMessage = (data) => globalThis.setTimeout(() => { if (typeof p2.onmessage === "function") p2.onmessage({ data, target: p2 }); }, 0);
    p2.postMessage = (data) => globalThis.setTimeout(() => { if (typeof p1.onmessage === "function") p1.onmessage({ data, target: p1 }); }, 0);
    this.port1 = p1;
    this.port2 = p2;
  }
};
globalThis.MessagePort = function MessagePort() {};
// performance — React/Next read performance.now() for timing/scheduling. mark()/measure()
// must RETURN the PerformanceEntry they create (real spec): RUM/timing code destructures
// `const {startTime} = performance.mark(name)` (and reads `.duration`/`.entryType` off the
// measure), so returning undefined crashed with "Cannot destructure property 'startTime' …"
// (seen on nike.com's Boomerang beacon).
globalThis.performance = globalThis.performance || {
  now: () => Date.now(),
  timeOrigin: 0,
  mark(name, opts) {
    return { name: String(name == null ? "" : name), entryType: "mark",
      startTime: (opts && +opts.startTime) || Date.now(), duration: 0, detail: (opts && opts.detail) || null };
  },
  measure(name) {
    return { name: String(name == null ? "" : name), entryType: "measure", startTime: 0, duration: 0, detail: null };
  },
  clearMarks() {}, clearMeasures() {},
  getEntries: () => [], getEntriesByName: () => [], getEntriesByType: () => [],
};
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
        if (c !== this && !c._closed) globalThis.setTimeout(() => { if (typeof c.onmessage === "function") c.onmessage({ data, target: c }); }, 0);
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
globalThis.requestIdleCallback = (fn) => globalThis.setTimeout(fn, 0);
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
  set("innerWidth", 1280);
  set("innerHeight", 800);
  set("outerWidth", 1280);
  set("outerHeight", 800);
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
    colorDepth: 24, pixelDepth: 24,
    orientation: { type: "landscape-primary", angle: 0, addEventListener() {}, removeEventListener() {} },
  });
  set("visualViewport", {
    width: 1280, height: 800, scale: 1, offsetLeft: 0, offsetTop: 0, pageLeft: 0, pageTop: 0,
    addEventListener() {}, removeEventListener() {}, dispatchEvent() { return false; },
  });
  // PerformanceObserver — analytics / experiment code (e.g. Wikipedia's header
  // enrollments) `new PerformanceObserver(...)` at load; an undefined ref throws
  // inside a Promise and rejects the chain, killing the dependent script.
  set("PerformanceObserver", class PerformanceObserver {
    constructor(cb) { this._cb = cb; }
    observe() {} disconnect() {} takeRecords() { return []; }
  });
  try { globalThis.PerformanceObserver.supportedEntryTypes = []; } catch (_e) {}
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
    globalThis.setTimeout(() => {
      const ev = { type: "load", target: el, currentTarget: el };
      try { if (typeof el.onload === "function") el.onload(ev); } catch (_e) {}
      try { if (typeof el.dispatchEvent === "function") el.dispatchEvent(ev); } catch (_e) {}
    }, 0);
  };
  // Analytics SDKs (PostHog) churn HUNDREDS of hidden <iframe>s during hydration. With
  // no real navigation each one never loads, so the SDK keeps recreating them — and the
  // tree append/remove + serialize cost of that churn starves the render budget so the
  // real page never finishes. Hand these a LIGHTWEIGHT detached stub (never enters the
  // rtdom tree; append/remove/measure are no-ops; load fires once; contentWindow/Document
  // are present) so the churn is nearly free and the budget goes to the real DOM.
  const makeIframeStub = () => {
    const noop = () => {};
    // contentWindow IS globalThis: analytics SDKs (PostHog) create a throwaway iframe
    // purely to read the *native* prototype of a builtin off `iframe.contentWindow[name]`
    // (to defeat page monkey-patching). If contentWindow is missing they bail WITHOUT
    // caching and recreate an iframe on EVERY call → 700+ iframe churn that starves the
    // render budget. Pointing contentWindow at our realm makes `contentWindow[name].prototype`
    // resolve, so the SDK caches and the loop stops after one lookup per builtin.
    const win = globalThis;
    const stub = {
      nodeType: 1, nodeName: "IFRAME", tagName: "IFRAME", __iframeStub: true,
      style: {}, dataset: {}, contentWindow: win, contentDocument: globalThis.document,
      onload: null, onerror: null,
      setAttribute(n, v) { if (n === "src" && v) fireIframeLoad(this); this[n] = v; },
      getAttribute(n) { return this[n] != null ? String(this[n]) : null; },
      removeAttribute(n) { delete this[n]; },
      appendChild(c) { return c; }, removeChild(c) { return c; },
      insertBefore(c) { return c; }, remove: noop,
      addEventListener(t, f) { if (t === "load") { this.onload = f; fireIframeLoad(this); } },
      removeEventListener: noop, dispatchEvent: noop,
      getBoundingClientRect: () => ({ x: 0, y: 0, top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0 }),
      focus: noop, blur: noop, contains: () => false,
    };
    Object.defineProperty(stub, "src", {
      configurable: true,
      get() { return stub.__src || ""; },
      set(v) { stub.__src = String(v); if (stub.__src) fireIframeLoad(stub); },
    });
    return stub;
  };
  const origCreate = document.createElement.bind(document);
  document.createElement = (tag) => {
    if (String(tag).toLowerCase() === "iframe") return makeIframeStub();
    return addShadow(origCreate(tag));
  };
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
  const ts = function toString() {
    if (native.has(this)) return "function " + (this.name || "") + "() { [native code] }";
    return orig.call(this);
  };
  Object.defineProperty(ts, "name", { value: "toString", configurable: true });
  Function.prototype.toString = ts;
  native.add(ts); // the trap must report itself native too
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
  mark(setInterval); mark(clearInterval); mark(cancelAnimationFrame);
  if (globalThis.Headers) mark(globalThis.Headers, "Headers");
  const nav = globalThis.navigator;
  if (nav && nav.clipboard) { mark(nav.clipboard.writeText, "writeText"); mark(nav.clipboard.readText, "readText"); }

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

  // navigator → Navigator.prototype (13 native getters). webdriver moves to a native getter
  // returning false with NO own property (defeats webdriver-getter-tampered + the alt/contradiction
  // cross-reads, which call the prototype getter).
  guard(() => hostInterface(nav, "Navigator",
    ["userAgent", "platform", "language", "languages", "hardwareConcurrency", "vendor",
     "onLine", "appVersion", "appName", "product", "cookieEnabled", "appCodeName",
     "maxTouchPoints", "webdriver"], ["webdriver"]));
  guard(() => { Object.defineProperty(Object.getPrototypeOf(nav), "webdriver", { get: nativeGetter(false), enumerable: true, configurable: true }); });
  guard(() => { if (nav && typeof nav.javaEnabled !== "function") { nav.javaEnabled = function javaEnabled() { return false; }; mark(nav.javaEnabled, "javaEnabled"); } });

  // screen → Screen.prototype (6 native getters); reserve OS chrome so avail<full (screen-no-os-chrome).
  guard(() => {
    const scr = G.screen;
    if (scr) {
      if (scr.availHeight === scr.height) { try { scr.availHeight = scr.height - 25; } catch (e) {} }
      hostInterface(scr, "Screen", ["width", "height", "availWidth", "availHeight", "colorDepth", "pixelDepth"]);
    }
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
      const proto = Object.getPrototypeOf(obj);
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

fn make_runtime(base: &str, cookies: &str, ua: &str) -> JsRuntime {
    // Build the shared cookie jar first so it backs BOTH page `fetch` (op_state) and the
    // ES-module loader (`<script type=module>` import graphs) — same session, one jar.
    let jar: Jar = Rc::new(RefCell::new(if cookies.is_empty() {
        CookieJar::new()
    } else {
        CookieJar::from_storage_state(cookies)
    }));
    let rt = JsRuntime::new(RuntimeOptions {
        extensions: vec![turbo_dom::init()],
        module_loader: Some(Rc::new(NetModuleLoader {
            base: base.to_string(),
            jar: jar.clone(),
            ua: ua.to_string(),
        })),
        ..Default::default()
    });
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

async fn run_async(
    rt: &mut JsRuntime,
    html: &str,
    base: &str,
    script: &str,
) -> Result<String, String> {
    install_dom(rt, html, base)?;
    exec_page_scripts(rt, script)?;
    drain_event_loop(rt).await?; // promises/microtasks + fetch from the page
    rt.execute_script("<timers>", "__runTimers()")
        .map_err(|e| e.to_string())?;
    drain_event_loop(rt).await?; // promises queued by timer callbacks
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
    exec_page_scripts(rt, script)?;
    drain_event_loop(rt).await?;
    rt.execute_script("<timers>", "__runTimers()")
        .map_err(|e| e.to_string())?;
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
