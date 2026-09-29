//! Fingerprint **debug/probe mode**: run a page's JS in the render isolate with
//! `navigator` / `screen` / `window.chrome` / canvas wrapped in logging proxies,
//! then report every property a script *touched* and which ones came back
//! `undefined` — i.e. exactly what an anti-bot check read and what we still need
//! to shim to satisfy it.
//!
//! This is the reconnaissance step for getting past consistency-only gates (and
//! the groundwork for any in-house solver): point it at a WAF's collector script
//! and it tells you the surface to fill in. It does NOT execute the network — feed
//! it the page HTML + the script you want to observe.

use serde::Serialize;
use std::collections::BTreeMap;

/// What a script touched on the instrumented globals.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProbeAccess {
    /// Instrumented object: `navigator`, `screen`, `chrome`, `canvas`, `document`.
    pub target: String,
    /// Property / method name.
    pub prop: String,
    /// `"get"` (read) or `"call"` (invoked as a function).
    pub kind: String,
    /// Whether the value was defined (a `get` returning `undefined` is a shim gap).
    pub defined: bool,
    /// How many times it was touched.
    pub count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProbeReport {
    /// Distinct accesses, sorted by `target.prop`.
    pub accesses: Vec<ProbeAccess>,
    /// `target.prop` reads that returned `undefined` — the shim to-do list.
    pub shim_needed: Vec<String>,
}

// Wraps the fingerprint surfaces in logging Proxies. Runs in the page-script slot
// AFTER ENV_BOOTSTRAP, so it re-wraps the real (already-installed) globals — no
// ENV_BOOTSTRAP edit or op plumbing needed. Records into `globalThis.__probe`.
//
// Coverage is deliberately broad enough to inventory a heavy VM fingerprinter
// (BotGuard/reCAPTCHA-class), not just a passive consistency probe: the whole
// `window` surface via recording accessors (so undefined reads surface as shim
// gaps), `document` beyond `createElement`, canvas 2D + WebGL contexts down to
// `getParameter`/`getExtension`/`getSupportedExtensions` with their args,
// `performance`, `Date`, `Intl`, `MessageChannel`, and `Function.prototype.toString`
// anti-tamper reads. Each section is guarded so a failure can't abort recon.
const PROBE_INSTALL: &str = r#"(() => {
  const log = (globalThis.__probe = []);
  const rec = (target, prop, kind, defined) => {
    try { log.push({ target, prop: String(prop), kind, defined }); } catch (e) {}
  };
  // Shallow logging proxy: records own-prop get/call; wraps returned functions so
  // their invocation is recorded too. Object results are left raw unless a caller
  // re-wraps them (canvas/context/navigator sub-objects do).
  const wrap = (name, obj) => {
    if (!obj || (typeof obj !== "object" && typeof obj !== "function")) return obj;
    return new Proxy(obj, {
      get(o, p) {
        let v; try { v = Reflect.get(o, p); } catch (e) { v = undefined; }
        rec(name, p, "get", v !== undefined);
        if (typeof v === "function") {
          return function (...a) { rec(name, p, "call", true); return v.apply(this === undefined ? o : this, a); };
        }
        return v;
      },
    });
  };
  // Preserve constructor-ness for function-valued globals (XMLHttpRequest, Worker,
  // RTCPeerConnection, the observers…): a plain closure would break `new`.
  const wrapFn = (name, fn) => new Proxy(fn, {
    apply(t, thiz, a) { rec(name, "()", "call", true); return Reflect.apply(t, thiz === undefined ? globalThis : thiz, a); },
    construct(t, a, nt) { rec(name, "new", "construct", true); return Reflect.construct(t, a, nt); },
  });
  // WebGL / 2D context: record getParameter/getExtension/getSupportedExtensions/readPixels
  // (the GPU-fingerprint surface) with their arguments, and whether the context existed.
  const ctxWrap = (kind, ctx) => new Proxy(ctx, {
    get(o, p) {
      let v; try { v = Reflect.get(o, p); } catch (e) { v = undefined; }
      rec("ctx:" + kind, p, "get", v !== undefined);
      if (typeof v === "function") {
        return function (...a) {
          rec("ctx:" + kind, String(p) + (a.length ? "(" + a.map(String).join(",") + ")" : ""), "call", true);
          return v.apply(o, a);
        };
      }
      return v;
    },
  });
  const canvasWrap = (el) => new Proxy(el, {
    get(o, p) {
      let v; try { v = Reflect.get(o, p); } catch (e) { v = undefined; }
      rec("canvas", p, "get", v !== undefined);
      if (p === "getContext") {
        return function (kind, ...rest) {
          rec("canvas", "getContext(" + String(kind) + ")", "call", true);
          let ctx = null; try { ctx = v.call(o, kind, ...rest); } catch (e) { ctx = null; }
          // A null context (e.g. WebGL) is the shim gap; record its presence explicitly.
          rec("canvas", "getContext(" + String(kind) + ")=>" + (ctx ? "ctx" : "null"), "get", ctx != null);
          return ctx ? ctxWrap(String(kind), ctx) : ctx;
        };
      }
      if (typeof v === "function") {
        return function (...a) { rec("canvas", p, "call", true); return v.apply(o, a); };
      }
      return v;
    },
  });
  const wrapDoc = (doc) => new Proxy(doc, {
    get(o, p) {
      let v; try { v = Reflect.get(o, p); } catch (e) { v = undefined; }
      rec("document", p, "get", v !== undefined);
      if (p === "createElement") {
        return function (tag) {
          const el = v.call(o, tag);
          if (String(tag).toLowerCase() === "canvas") { rec("document", "createElement(canvas)", "call", true); return canvasWrap(el); }
          return el;
        };
      }
      if (typeof v === "function") {
        return function (...a) { rec("document", p, "call", true); return v.apply(o, a); };
      }
      return v;
    },
  });

  // Dedicated wraps for the compound fingerprint objects (substituted into the
  // curated window loop below so bare `navigator`/`document`/… resolve to them).
  const dedicated = Object.create(null);
  if (globalThis.navigator) dedicated.navigator = wrap("navigator", globalThis.navigator);
  dedicated.screen = wrap("screen", globalThis.screen || {});
  if (globalThis.chrome) dedicated.chrome = wrap("chrome", globalThis.chrome);
  if (globalThis.document) dedicated.document = wrapDoc(globalThis.document);
  if (globalThis.performance) dedicated.performance = wrap("performance", globalThis.performance);
  try {
    const D = globalThis.Date;
    dedicated.Date = new Proxy(D, {
      construct(T, a) { rec("Date", "new", "construct", true); return Reflect.construct(T, a); },
      apply(T, thiz, a) { rec("Date", "()", "call", true); return Reflect.apply(T, thiz, a); },
      get(o, p) { let v = Reflect.get(o, p); rec("Date", p, "get", v !== undefined); if (typeof v === "function") return function (...a) { rec("Date", p, "call", true); return v.apply(o, a); }; return v; },
    });
  } catch (e) {}
  if (globalThis.Intl) dedicated.Intl = wrap("Intl", globalThis.Intl);
  try {
    if (globalThis.MessageChannel) {
      dedicated.MessageChannel = new Proxy(globalThis.MessageChannel, {
        construct(T, a) { rec("MessageChannel", "new", "construct", true); return Reflect.construct(T, a); },
      });
    }
  } catch (e) {}

  // Function.prototype.toString anti-tamper reads — BotGuard calls this heavily to
  // detect polyfilled/non-native functions. Count them, preserving the underlying
  // native-branding trap ENV_BOOTSTRAP installed.
  try {
    const ft = Function.prototype.toString;
    const rts = function toString() { rec("Function.prototype", "toString", "call", true); return ft.call(this); };
    try { Object.defineProperty(rts, "name", { value: "toString", configurable: true }); } catch (e) {}
    Function.prototype.toString = rts;
  } catch (e) {}

  // Curated window-surface: a recording accessor per name so window-level reads register
  // AND undefined reads surface as shim gaps (the backfill to-do list). Object values get
  // a logging proxy; function values keep constructor-ness via wrapFn; the compound
  // objects above are substituted from `dedicated`. `window`/`self` are pinned to the
  // realm so recon never nulls out the global the VM needs to even start.
  const WRAP_OBJ = new Set(["history", "location", "crypto", "localStorage", "sessionStorage", "visualViewport", "external", "speechSynthesis"]);
  const WIN_NAMES = ("top parent frames self window name length opener closed origin isSecureContext crossOriginIsolated " +
    "document navigator screen chrome performance Date Intl MessageChannel MessagePort postMessage " +
    "Notification RTCPeerConnection webkitRTCPeerConnection indexedDB caches " +
    "WebGLRenderingContext WebGL2RenderingContext OffscreenCanvas createImageBitmap requestIdleCallback cancelIdleCallback " +
    "matchMedia getComputedStyle IntersectionObserver ResizeObserver PerformanceObserver MutationObserver " +
    "Worker SharedWorker WebAssembly Reflect Proxy BigInt queueMicrotask structuredClone reportError scheduler trustedTypes " +
    "devicePixelRatio innerWidth innerHeight outerWidth outerHeight screenX screenY scrollX scrollY pageXOffset pageYOffset " +
    "history location crypto localStorage sessionStorage visualViewport external speechSynthesis " +
    "XMLHttpRequest fetch addEventListener removeEventListener dispatchEvent requestAnimationFrame cancelAnimationFrame " +
    "onerror onmessage ononline onoffline Permissions").split(/\s+/);
  const seen = new Set();
  for (const name of WIN_NAMES) {
    if (!name || seen.has(name)) continue; seen.add(name);
    try {
      let cur = globalThis[name];
      let exposed;
      if (name === "window" || name === "self") { cur = globalThis; exposed = globalThis; }
      else if (dedicated[name] !== undefined) exposed = dedicated[name];
      else if (cur === null || cur === undefined) exposed = cur;
      else if (typeof cur === "function") exposed = wrapFn(name, cur);
      else if (typeof cur === "object" && WRAP_OBJ.has(name)) exposed = wrap(name, cur);
      else exposed = cur;
      const defined = cur !== undefined;
      Object.defineProperty(globalThis, name, {
        configurable: true,
        get() { rec("window", name, "get", defined); return exposed; },
        set(v) { rec("window", name, "set", true); exposed = v; },
      });
    } catch (e) {}
  }
})();"#;

#[derive(serde::Deserialize)]
struct RawAccess {
    target: String,
    prop: String,
    kind: String,
    defined: bool,
}

/// Run `script` against `html` with the fingerprint globals instrumented, and
/// report what it touched. Aggregates duplicate touches and surfaces the reads
/// that returned `undefined` as the shim to-do list.
pub fn probe_globals(html: &str, script: &str) -> Result<ProbeReport, String> {
    // PROBE_INSTALL, then the (guarded) script, then serialise the log.
    let wrapped = format!(
        "{PROBE_INSTALL}\ntry {{\n{script}\n}} catch (e) {{}}\n;JSON.stringify(globalThis.__probe || [])"
    );
    let json = crate::runtime::run_with_dom(html, &wrapped)?;
    let raw: Vec<RawAccess> =
        serde_json::from_str(&json).map_err(|e| format!("probe log parse: {e}"))?;
    Ok(aggregate(raw))
}

// Aggregate raw accesses by (target, prop, kind) into a report; a prop is a shim gap
// if every `get` of it came back undefined. Shared by the sync + async probes.
fn aggregate(raw: Vec<RawAccess>) -> ProbeReport {
    let mut agg: BTreeMap<(String, String, String), (bool, u32)> = BTreeMap::new();
    for a in raw {
        let entry = agg.entry((a.target, a.prop, a.kind)).or_insert((false, 0));
        entry.0 |= a.defined;
        entry.1 += 1;
    }

    let mut accesses: Vec<ProbeAccess> = agg
        .into_iter()
        .map(|((target, prop, kind), (defined, count))| ProbeAccess {
            target,
            prop,
            kind,
            defined,
            count,
        })
        .collect();
    accesses.sort_by(|a, b| (&a.target, &a.prop).cmp(&(&b.target, &b.prop)));

    let mut shim_needed: Vec<String> = accesses
        .iter()
        .filter(|a| a.kind == "get" && !a.defined)
        .map(|a| format!("{}.{}", a.target, a.prop))
        .collect();
    shim_needed.sort();
    shim_needed.dedup();

    ProbeReport {
        accesses,
        shim_needed,
    }
}

// A hidden node the async probe writes its `__probe` log JSON into: an async run
// returns the serialized document, not a trailing expression, so we read the log
// back out of the DOM (same trick as the reCAPTCHA token sink).
const PROBE_SINK: &str = "__ts_probe_log";

/// Execution-complete anti-bot recon: run `script` (a page's own scripts, external
/// bodies already inlined) to completion in a full **async** isolate — dynamic
/// `<script>` injection + `op_fetch` + timers finish, so a BotGuard/reCAPTCHA-class
/// VM that bootstraps its heavy VM at runtime actually runs — under the instrumented
/// fingerprint globals, and report BOTH what env surface it read (`shim_needed` =
/// the reads we still return `undefined`) AND the cookies the isolate earned
/// (`storage_state` JSON). `cookies` seeds the jar (storage_state or ""); `ua` the
/// navigator UA; `budget_ms` bounds the VM. This is the native-mint probe: point it
/// at google's homepage and see what BotGuard demands + whether it set a session
/// cookie (e.g. a trusted `__Secure-ENID`) with no browser.
pub async fn probe_page_async(
    html: &str,
    base: &str,
    ua: &str,
    cookies: &str,
    script: &str,
    budget_ms: u64,
) -> Result<(ProbeReport, String), String> {
    // Assemble three programs joined by the render tier's SCRIPT_BOUNDARY so each runs as its own
    // top-level script (like a browser): (1) PROBE_INSTALL wraps the globals first; (2) the page's
    // own scripts — themselves boundary-delimited, each tolerated by `exec_page_scripts`; (3) a
    // writer that snapshots the accumulated log into the sink node, RE-writing (last wins) on both
    // an early timer AND the window `load` event. The window-`load` write is critical: `run_async`
    // fires DOMContentLoaded/load AFTER the first `__runTimers` pass, and a BotGuard/reCAPTCHA-class
    // collector commonly does its heavy env reads in a `load` handler — capturing only at
    // setTimeout(0) would miss them and under-report `shim_needed`. NB: no `try {}` may span a
    // SCRIPT_BOUNDARY — the wrapper would fragment across the split.
    let sink_writer = format!(
        "(function(){{ var __w = function(){{ try {{\n\
           var __el = document.getElementById({sink:?}) || document.createElement('div');\n\
           __el.id = {sink:?};\n\
           __el.textContent = JSON.stringify(globalThis.__probe || []);\n\
           if (!__el.parentNode) (document.body || document.documentElement).appendChild(__el);\n\
         }} catch (e) {{}} }};\n\
         try {{ globalThis.addEventListener('load', __w); }} catch (e) {{}}\n\
         setTimeout(__w, 0);\n\
       }})();",
        sink = PROBE_SINK,
    );
    let boundary = crate::runtime::SCRIPT_BOUNDARY;
    let wrapped = format!("{PROBE_INSTALL}{boundary}{script}{boundary}{sink_writer}");
    let (doc, storage) =
        crate::runtime::render_capture_cookies(html, base, ua, cookies, &wrapped, budget_ms)
            .await?;
    let raw: Vec<RawAccess> =
        serde_json::from_str(&extract_sink(&doc, PROBE_SINK)).unwrap_or_default();
    Ok((aggregate(raw), storage))
}

// Read the text content of the sink node out of the serialized document (the id is unique + ASCII,
// so a plain substring scan is enough). The JSON was written as textContent, so serialization
// HTML-escapes `<`/`>`/`&`/`"` — un-escape them before the caller parses, or a recorded prop/arg
// containing any of those would make the parse fail and silently drop the whole recon log.
fn html_unescape(s: &str) -> String {
    // Order matters: resolve `&amp;` LAST so `&amp;lt;` doesn't become `<`.
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&amp;", "&")
}
fn extract_sink(html: &str, sink_id: &str) -> String {
    let needle = format!("id=\"{sink_id}\"");
    let Some(i) = html.find(&needle) else {
        return "[]".to_string();
    };
    let Some(gt) = html[i..].find('>') else {
        return "[]".to_string();
    };
    let start = i + gt + 1;
    match html[start..].find('<') {
        Some(lt) => html_unescape(html[start..start + lt].trim()),
        None => "[]".to_string(),
    }
}

// Tests for `probe_globals` live in `tests/probe.rs`, a separate test process:
// they boot a deno_core runtime (which initializes the V8 platform), and the
// lib unit-test binary must stay deno_core-free so it never collides with
// `browser_env`'s standalone-V8 smoke test (see browser_env.rs header).
