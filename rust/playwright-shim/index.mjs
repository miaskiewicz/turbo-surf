/* eslint-disable no-underscore-dangle, eqeqeq, prefer-const, no-await-in-loop, unicorn/consistent-function-scoping, no-unused-vars, no-array-reverse */
// A Playwright-shaped façade backed by the turbo-surf native (Rust) addon.
// No browser, no Chromium: `goto` fetches over Rust+reqwest, the read/locator/
// expect surface runs over the cached HTML through the napi addon (turbo-dom +
// the view modules), the JS-render tier (`render`/`evaluate`) runs page scripts
// in a deno_core V8 isolate over rtdom — never a browser.
//
// This is the drop-in seam: a suite imports `@playwright/test`; the muscle is
// Rust. Most of the surface added here is PURE JS and never crosses the napi
// boundary (locator composition, generic-value asserts, waiters, events,
// viewport/header/timeout state). Only genuine DOM/render semantics cross — and
// the addon caches the last parse per thread, so a crossing on an unchanged page
// is a string-marshal + op, not a re-parse. What a no-browser engine physically
// cannot do (pixels, real input devices, network interception) throws honestly
// or no-ops — see LIMITATIONS.md.

import { handleEvaluation, hasJSHandles, ensureHandleSession } from "./js-handles.mjs";
import { consoleMessage } from "./console-message.mjs";
import { AsyncLocalStorage } from "node:async_hooks";
import { randomUUID } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { basename } from "node:path";
import { closeScreenshotScene, paintScreenshotScene } from "./screenshot-scene.mjs";
import { historyUrl, syncHistory, traverseLive } from "./history-navigation.mjs";
import { validateArtifactOptions } from "./artifact-options.mjs";
import { encodeStorageOrigins, decodeStorageOrigins } from "./storage-state.mjs";
import { equalValues } from "./equality.mjs";
import { queryVisibleSelector } from "./visible-selector.mjs";
import {
  decodeEvaluationError,
  decodeEvaluationValue,
  encodeEvaluationError,
  encodeEvaluationValue,
  evaluationArgument,
} from "./evaluation-codec.mjs";
import { makeKeyboard } from "./keyboard.mjs";
import { captureAssets } from "./screenshot-resources.mjs";
import { invokeHostRoute, routeResponse } from "./host-routes.mjs";

const require = createRequire(import.meta.url);
const native = require("../crates/turbo-surf-napi/index.js");

// Playwright's methods return promises, so unsupported ones REJECT (not sync
// throw) — `await page.screenshot()` then surfaces the reason cleanly.
const UNSUPPORTED = (api, why) => async () => {
  throw new Error(`turbo-surf: ${api} unavailable — ${why}`);
};
const PIXEL = "no-browser engine (no rendering surface)";
const INPUT = "no synthetic input devices (static DOM, no pointer/keyboard hardware)";
const NETIO = "no in-process network interception";

function requireOfflineSupport(offline) {
  if (offline)
    throw new Error(
      "turbo-surf: offline emulation is unavailable in the native context network layer",
    );
}

function cookieMatchesUrl(cookie, url) {
  const domain = String(cookie.domain ?? "")
    .toLowerCase()
    .replace(/^\./, "");
  const hostOnly = cookie.host_only ?? !String(cookie.domain).startsWith(".");
  const host = url.hostname.toLowerCase();
  if (host !== domain && (hostOnly || !host.endsWith(`.${domain}`))) return false;
  if (cookie.secure && url.protocol !== "https:") return false;
  const path = cookie.path || "/";
  return (
    url.pathname === path ||
    (url.pathname.startsWith(path) && (path.endsWith("/") || url.pathname[path.length] === "/"))
  );
}

function isXPathSelector(selector) {
  return (
    typeof selector === "string" && /^(?:xpath=|\/|\.\.?\/|\.\.?$|\()/.test(selector.trimStart())
  );
}

function hasSelectorEngine(selector) {
  return isXPathSelector(selector) || (selector?.trimStart() ?? "").startsWith("css=");
}

function cookieMatchesFilter(cookie, filters) {
  return ["name", "domain", "path"].every((field) => {
    const expected = filters[field];
    if (expected === undefined) return true;
    if (expected instanceof RegExp) {
      expected.lastIndex = 0;
      return expected.test(cookie[field]);
    }
    return cookie[field] === expected;
  });
}

function nativeCookieRecord(cookie) {
  let domain = cookie.domain;
  let path = cookie.path;
  let secure = cookie.secure ?? false;
  if (cookie.url !== undefined) {
    if (domain !== undefined || path !== undefined)
      throw new Error("Cookie must use either url or domain/path");
    const url = new URL(cookie.url);
    if (!["http:", "https:"].includes(url.protocol))
      throw new Error("Cookie URL must use HTTP or HTTPS");
    domain = url.hostname;
    path = url.pathname.slice(0, url.pathname.lastIndexOf("/") + 1);
    secure = url.protocol === "https:";
  }
  if (!domain || !path) throw new Error("Cookie requires a url or domain/path pair");
  return {
    name: cookie.name,
    value: cookie.value,
    domain: domain.toLowerCase().replace(/^\./, ""),
    path,
    host_only: cookie.host_only ?? !domain.startsWith("."),
    secure,
    ...nativeCookieAttributes(cookie),
  };
}

function nativeCookieAttributes(cookie) {
  return {
    http_only: cookie.http_only ?? cookie.httpOnly ?? false,
    same_site: String(cookie.same_site ?? cookie.sameSite ?? "lax").toLowerCase(),
    expires_at:
      cookie.expires_at ??
      (cookie.expires == null || cookie.expires === -1 ? null : cookie.expires * 1000),
  };
}

class DownloadNavigationError extends Error {
  constructor() {
    super("page.goto: net::ERR_ABORTED: Download is starting");
  }
}

function attachmentRecord(response) {
  const disposition = (response.headers ?? []).find(
    ([name]) => name.toLowerCase() === "content-disposition",
  )?.[1];
  if (!disposition || !/^attachment(?:;|$)/i.test(disposition.trim())) return null;
  return {
    filename: attachmentFilename(disposition, response.finalUrl),
    url: response.finalUrl,
    bodyBytes: response.bodyBytes ?? Buffer.from(response.html ?? ""),
  };
}

function attachmentFilename(disposition, url) {
  const encoded = /(?:^|;)\s*filename\*\s*=\s*UTF-8'[^']*'([^;]*)/i.exec(disposition);
  const plain = /(?:^|;)\s*filename\s*=\s*(?:"((?:\\.|[^"])*)"|([^;]*))/i.exec(disposition);
  let filename = plain ? (plain[1] ?? plain[2]).trim().replace(/\\(.)/g, "$1") : "";
  if (encoded) {
    try {
      filename = decodeURIComponent(encoded[1].trim());
    } catch {
      /* use the plain filename */
    }
  }
  return filename || new URL(url).pathname.split("/").at(-1) || "download";
}

// Build an executable source string from page.evaluate's (fn|string, arg) form.
function evalSource(script, arg) {
  if (typeof script === "function") return `(${script.toString()})(${evaluationArgument(arg)})`;
  return String(script);
}

// Run `fn` over `items` with at most `limit` in flight, preserving input order
// in the results. Backs screenshot asset preparation, whose fetches are
// independent but must not stampede the network.
async function mapBounded(items, limit, fn) {
  const results = Array.from({ length: items.length });
  let next = 0;
  await Promise.all(
    Array.from({ length: Math.min(limit, items.length) }, async () => {
      while (next < items.length) {
        const i = next++;
        results[i] = await fn(items[i], i);
      }
    }),
  );
  return results;
}

// Quote a value for a CSS attribute selector (backs getByPlaceholder/AltText/Title).
function cssAttrValue(text) {
  return `"${String(text).replace(/"/g, '\\"')}"`;
}

function validateResourceTagOptions(kind, opts) {
  const supplied = [opts.content != null, opts.url != null, opts.path != null].filter(
    Boolean,
  ).length;
  if (supplied !== 1)
    throw new Error(
      `add${kind === "style" ? "Style" : "Script"}Tag requires exactly one of content, url or path`,
    );
}

// --- page.route helpers ---------------------------------------------------
// Normalize a route pattern + fulfill payload into a JSON-serializable record.
// `patternKey` preserves predicate identity, or the string/RegExp source for unroute.
// Convert one HAR entry (Playwright / Chrome DevTools format) into the same
// route record normalizeRoute produces. Matches the entry's URL exactly: the
// init-script's globToRegex anchors non-wildcard patterns, so the request URL
// must equal the recorded one (modulo the substring fallback). Entries missing
// a URL or response body are skipped by the caller.
function harEntryToRoute(entry) {
  const url = entry?.request?.url;
  if (!url) return null;
  const resp = entry.response ?? {};
  const headers = {};
  for (const h of resp.headers || []) {
    if (h && h.name) headers[String(h.name).toLowerCase()] = h.value == null ? "" : String(h.value);
  }
  const content = resp.content ?? {};
  const payload = {
    status: resp.status ?? 200,
    headers,
    contentType: content.mimeType ?? null,
    body:
      content.encoding === "base64"
        ? Buffer.from(content.text ?? "", "base64")
        : content.text == null
          ? ""
          : String(content.text),
  };
  // HAR replay uses the same host dispatch and response encoding as normal routes.
  return {
    id: randomUUID(),
    kind: "glob",
    pattern: url,
    flags: "",
    patternKey: url,
    handler: (route) => route.fulfill(payload),
  };
}

function normalizeRoute(pattern, handler) {
  const isRegex = pattern instanceof RegExp;
  const isPredicate = typeof pattern === "function";
  if (!isRegex && !isPredicate && typeof pattern !== "string")
    throw new TypeError("Route pattern must be a string, RegExp, or URL predicate");
  if (typeof handler !== "function") throw new TypeError("Route handler must be a function");
  return {
    kind: isPredicate ? "predicate" : isRegex ? "regex" : "glob",
    pattern: isPredicate ? null : isRegex ? pattern.source : String(pattern),
    flags: isRegex ? pattern.flags : "",
    patternKey: isPredicate ? pattern : isRegex ? pattern.source : String(pattern),
    id: randomUUID(),
    handler,
  };
}

function matchesHostRoutePredicate({ kind, patternKey }, url) {
  return kind !== "predicate" || patternKey(new URL(url));
}

// Build an init script that monkey-patches `fetch` (and XMLHttpRequest for the
// common open+send path) inside the live isolate. Routes are matched in the
// reverse registration order; a RegExp source uses `new RegExp(...)`, a glob
// supports `**` (any chars) and `*` (any non-slash chars), with substring as a
// final fallback.
function buildRouteInitScript(routes, bindingName) {
  const table = JSON.stringify(
    (routes ?? []).map((r) => ({
      kind: r.kind,
      pattern: r.pattern,
      flags: r.flags,
      id: r.id,
    })),
  );
  return `(() => {
    const invokeHost = __tsNs.shimBindings?.functions[${JSON.stringify(bindingName)}];
    const previous = __tsNs.shimRouteState;
    const state = previous || {routes: [], fetch: globalThis.fetch, xhr: globalThis.XMLHttpRequest};
    __tsNs.shimRouteState = state;
    const routes = state.routes;
    state.requests ??= new Map();
    const next = ${table};
    routes.splice(0, routes.length, ...next);
    if (!routes.length) {
      if (previous) { globalThis.fetch = state.fetch; globalThis.XMLHttpRequest = state.xhr; state.active=false; }
      return;
    }
    if (state.active) return;
    state.active=true;
    const globToRegex = (p) => {
      // Escape * and ? too, so the two translation passes below can locate
      // every star — a bare * in the source would otherwise skip the escape
      // step, leave ** unchanged, and produce an invalid regex (turbo-surf#240).
      const esc = p.replace(/[.+^\${}()|\\[\\]\\\\*?]/g, "\\\\$&");
      const re = "^" + esc.replace(/\\\\\\*\\\\\\*/g, ".*").replace(/\\\\\\*/g, "[^/]*") + "$";
      return new RegExp(re);
    };
    const candidates = (url) => {
      // Match against the URL as-given AND its absolute/path forms, so a HAR
      // entry's absolute URL matches a page fetch issued with a relative URL
      // (and vice versa). Falls back to the raw input on any URL-parse error.
      const out = [url];
      try {
        const base = typeof location !== "undefined" && location.href ? location.href : "about:blank";
        const u = new URL(url, base);
        if (out.indexOf(u.href) === -1) out.push(u.href);
        const pq = u.pathname + u.search;
        if (out.indexOf(pq) === -1) out.push(pq);
      } catch (_e) { /* non-URL input — raw-only matching */ }
      return out;
    };
    const find = (url, start = routes.length - 1, table = routes) => {
      const urls = candidates(url);
      // Playwright runs the LATEST handler first (reverse registration order) so
      // a per-test override can shadow a globally registered mock. Iterate the
      // route table from the end.
      for (let i = start; i >= 0; i--) {
        const r = table[i];
        // Predicate callbacks remain in the host; this is only a candidate.
        if (r.kind === "predicate") return r;
        for (const u of urls) {
          try {
            if (r.kind === "regex") {
              if (new RegExp(r.pattern, r.flags).test(u)) return r;
            } else {
              if (globToRegex(r.pattern).test(u)) return r;
              if (u.indexOf(r.pattern) !== -1) return r;
            }
          } catch (_e) { /* skip malformed pattern */ }
        }
      }
      return null;
    };
    const synth = (opts) => {
      const o = opts || {};
      const h = Object.assign({}, o.headers || {});
      if (o.contentType && !Object.keys(h).some((k) => k.toLowerCase() === "content-type"))
        h["content-type"] = o.contentType;
      return new Response(o.status === 204 || o.status === 205 || o.status === 304 ? null : new Uint8Array(o.bodyBytes || []), { status: o.status == null ? 200 : o.status, headers: h });
    };
    // Only request records and decisions cross the isolate boundary; callbacks stay in Node.
    const requestRecord = async (input, init, resourceType) => {
      const request = new Request(input, init);
      const headers = {}; request.headers.forEach((v,k)=>{headers[k]=v;});
      const bodyBytes=request.body === null ? null : Array.from(new Uint8Array(await request.arrayBuffer()));
      const requestId = state.nextRequest = (state.nextRequest || 0) + 1;
      state.requests.set(requestId, request);
      const record = {requestId, url:request.url, method:request.method, headers,
        bodyBytes,
        credentials:request.credentials, mode:request.mode, cache:request.cache,
        redirect:request.redirect, referrer:request.referrer, referrerPolicy:request.referrerPolicy,
        resourceType};
      Object.defineProperty(record, 'signal', {value:request.signal});
      return record;
    };
    const routeDecision = async (pending, signal, requestId) => {
      let abort;
      const canceled = new Promise((_,reject)=>{
        abort=()=>{invokeHost({cancel:requestId}).catch(()=>{});reject(signal.reason);};
        if(signal.aborted) abort(); else signal.addEventListener('abort',abort,{once:true});
      });
      try { return await Promise.race([pending,canceled]); }
      finally {signal.removeEventListener('abort',abort);}
    };
    const runRouteChain = async (first, request, chain) => {
      let route = first;
      const signal = request.signal;
      const originalUrl = request.url;
      while (route) {
        const decision = await routeDecision(invokeHost({id:route.id, request, matchUrl:originalUrl}), signal, request.requestId);
        if (decision.request) request = decision.request;
        if (decision.action !== "fallback") return {...decision, request};
        route = find(originalUrl, chain.indexOf(route) - 1, chain);
      }
      return {fulfilled: null, action: "continue", request};
    };
    const finishRequest = request => {
      state.requests.delete(request.requestId);
      invokeHost({complete:request.requestId}).catch(()=>{});
    };
    const original = (request, signal) => state.fetch(request.url, {
      ...request, signal, body:request.bodyBytes === null ? undefined : new Uint8Array(request.bodyBytes),
    });
    if (typeof globalThis.fetch === "function") {
      const origFetch = globalThis.fetch;
      globalThis.fetch = async function (input, init) {
        const url = typeof input === "string" ? input : (input && input.url) || "";
        const chain = routes.slice();
        const r = find(url, chain.length - 1, chain);
        if (!r) return origFetch(input, init);
        const request = await requestRecord(input, init, "fetch");
        try {
          const decision = await runRouteChain(r, request, chain);
          if (decision.fulfilled) return synth(decision.fulfilled);
          if (decision.action === "abort") throw new TypeError("net::ERR_" + (decision.errorCode || "ABORTED").toUpperCase());
          return await original(decision.request, request.signal);
        } finally {finishRequest(request);}
      };
    }
    if (typeof globalThis.XMLHttpRequest === "function") {
      const OrigXHR = globalThis.XMLHttpRequest;
      const bodyBrands = [
        Object.getOwnPropertyDescriptor(ArrayBuffer.prototype,"byteLength").get,
        Object.getOwnPropertyDescriptor(Blob.prototype,"size").get,
        FormData.prototype.entries,
        Object.getOwnPropertyDescriptor(URLSearchParams.prototype,"size")?.get || URLSearchParams.prototype.toString,
        Object.getOwnPropertyDescriptor(Document.prototype,"URL").get,
      ];
      const xhrBody = value => {
        if(value==null || typeof value==="string" || ArrayBuffer.isView(value)) return value;
        for(const brand of bodyBrands) {
          try { Reflect.apply(brand,value,[]); return value; } catch {}
        }
        if(typeof value==="symbol") throw new TypeError("Cannot convert a Symbol value to a string");
        return String(value);
      };
      function MockXHR() {
        this._mocked = null;
        this._real = null;
        this._url = "";
        this._method = "GET";
        this._reqHeaders = {};
      }
      MockXHR.prototype.open = function (method, url) {
        this._method = method;
        this._url = url;
        this._mocked = find(url);
        if (!this._mocked) {
          this._real = new OrigXHR();
          this._real.open(method, url);
        }
      };
      MockXHR.prototype.setRequestHeader = function (name, value) {
        this._reqHeaders[name] = value;
        if (this._real) this._real.setRequestHeader(name, value);
      };
      MockXHR.prototype._emit = function (name, event) {
        const callback = this["on" + name];
        if (typeof callback === "function") callback.call(this, event || new Event(name));
      };
      MockXHR.prototype._sendReal = function (body) {
        if (!this._real) {
          this._real = new OrigXHR();
          this._real.open(this._method, this._url);
          for (const name of Object.keys(this._reqHeaders)) this._real.setRequestHeader(name, this._reqHeaders[name]);
        }
        const self = this;
        for (const name of ["readystatechange", "load", "error", "loadend", "timeout", "abort"]) {
          this._real.addEventListener(name, (event) => {
            self.readyState = self._real.readyState;
            self.status = self._real.status;
            self.responseText = self._real.responseText;
            self.response = self._real.response;
            self._emit(name, event);
          });
        }
        this._real.send(body);
      };
      MockXHR.prototype.send = function (body) {
        const chain = routes.slice();
        this._mocked = find(this._url, chain.length - 1, chain);
        if (this._mocked) {
          const self = this;
          // XHR converts its BodyInit before discarding GET/HEAD payload bytes.
          const converted = xhrBody(body), method = String(this._method), normalized = method.toUpperCase();
          const payload = normalized === "GET" || normalized === "HEAD" ? null : converted;
          requestRecord(new URL(this._url, location.href).href, {method,body:payload,headers:this._reqHeaders}, "xhr").then(request => runRouteChain(this._mocked, request, chain).finally(()=>finishRequest(request))).then(({ fulfilled, action, request }) => {
            if (action === "abort") {
              self.readyState = 4;
              self.status = 0;
              self.responseText = self.response = "";
              self._emit("readystatechange");
              self._emit("error");
              self._emit("loadend");
              return;
            }
            if (!fulfilled) { self._url=request.url; self._method=request.method; self._reqHeaders=request.headers; self._real=null; self._sendReal(request.bodyBytes === null ? null : new Uint8Array(request.bodyBytes)); return; }
            const o = fulfilled;
            self.readyState = 4;
            self.status = o.status == null ? 200 : o.status;
            const b = new TextDecoder().decode(new Uint8Array(o.bodyBytes || []));
            self.responseText = b;
            self.response = b;
            self._emit("readystatechange");
            self._emit("load");
            self._emit("loadend");
          });
        } else this._sendReal(body);
      };
      MockXHR.prototype.addEventListener = function (name, fn) {
        this["on" + name] = fn;
      };
      globalThis.XMLHttpRequest = MockXHR;
    }
  })();`;
}

// Persistent exposed functions queue serialized calls; Node settles the matching
// guest promise after invoking the host handler between isolate evaluations.
function buildExposeInitScript(names, internalName) {
  if (!names || !names.length) return "";
  const list = JSON.stringify(names);
  return `"use strict"; (() => {
    const bag = __tsNs.shimBindings ||= {__proto__:null,q:{__proto__:null,length:0},n:0,nonce:__tsNs.shimBindingId(),error:__tsNs.shimBindingError,pending:{__proto__:null},evaluations:{__proto__:null}};
    bag.functions ||= {__proto__:null};
    const define = (name) => {
      if (bag.functions[name]) return;
      if (typeof globalThis[name] === "function" && globalThis[name].__ts) return;
      const fn = function (...args) {
        const id = ++bag.n;
        __tsNs.shimBindingPending(true);
        return __tsNs.shimBindingPromise((resolve, reject) => {
          let settled=false;
          const finish=()=>{if(!settled){settled=true;__tsNs.shimBindingPending(false);}};
          bag.pending[id] = {__proto__:null, resolve:value=>{finish();resolve(value);}, reject:error=>{finish();reject(error);} };
          bag.q[bag.q.length++] = {__proto__:null, id, name, args };
        });
      };
      bag.functions[name] = fn;
      if (name === ${JSON.stringify(internalName)}) return;
      try { Object.defineProperty(fn, "__ts", { value: true }); } catch (_e) {}
      try {
        Object.defineProperty(globalThis, name, { value: fn, configurable: true, writable: true });
      } catch (_e) {
        try { globalThis[name] = fn; } catch (_e2) {}
      }
    };
    for (const n of ${list}) define(n);
  })();`;
}

// ---------------------------------------------------------------------------
// Locator — a lazy query over the page's current HTML. `_resolve(html)` returns
// the match array (each `{ node, text, html }`) in ONE napi call; composition
// (first/last/nth/filter) and counts stay in JS over that array.
// ---------------------------------------------------------------------------
function responseBudget(value) {
  if (value === undefined) return undefined;
  if (!Number.isInteger(value) || value < 1 || value > 0xffffffff) {
    throw new Error("maxResponseBytes must be a positive 32-bit byte count");
  }
  return value;
}

function renderedVisibility(element) {
  for (let ancestor = element; ancestor; ancestor = ancestor.parentElement) {
    if (ancestor.hasAttribute("hidden") || getComputedStyle(ancestor).display === "none")
      return false;
  }
  const style = getComputedStyle(element);
  if (style.visibility === "hidden" || style.visibility === "collapse") return false;
  if (style.display === "contents") {
    for (const child of element.childNodes) {
      if (child.nodeType === 1 && renderedVisibility(child)) return true;
      if (child.nodeType === 3) {
        const range = document.createRange();
        range.selectNode(child);
        const rect = range.getBoundingClientRect();
        if (rect.width > 0 && rect.height > 0) return true;
      }
    }
    return false;
  }
  const rect = element.getBoundingClientRect();
  return rect.width > 0 && rect.height > 0;
}

class Locator {
  constructor(page, resolve, opts = {}) {
    this._page = page;
    this._resolve = resolve;
    this._count = opts.count ?? null;
    this._index = opts.index ?? null;
    this._filter = opts.filter ?? null; // (match) => boolean, applied in JS
    // getByRole/getByText/getByLabel carry {kind,value,name} so a LIVE dispatch can
    // re-resolve the match IN the running isolate (live `*` indices) instead of trusting
    // a re-serialized snapshot's index, which diverges for portal'd content (MUI options).
    this._getBy = opts.getBy ?? null;
    this._testId = opts.testId ?? null;
    this._parent = opts.parent ?? null;
    this._composite = opts.composite ?? null;
    // Each wrapper preserves the preceding selection stage (including nth) before filtering.
    this._querySource = opts.querySource ?? null;
    this._queryFilter = opts.queryFilter ?? null;
    // nth-aware scope chain ([{sel, idx, filter}, …]) for nested locators where a parent had
    // `.nth(i)` or `.filter(...)` — a CSS-concat selector can't express "the i-th (or
    // text-filtered) match's subtree", so the live drive walks this chain via
    // __tcResolveScoped. Empty for the common (non-indexed, non-filtered) case.
    this._scope = opts.scope ?? [];
    // The base selector this locator's match set comes from. Unlike `_selector` (the
    // own-dispatch selector, dropped by `.filter()` since a filtered set isn't one CSS
    // selector) this SURVIVES `.filter()` so children of a filtered parent can still scope
    // to it via the chain. Set wherever `_selector` is (and carried through `_derive`).
    this._scopeSel = opts.scopeSel ?? null;
    // Serializable form of an applied `.filter()` ({hasText|hasNotText} strings only) so the
    // scope chain can re-apply it in the live isolate. null for regex/has/hasNot filters
    // (those keep the JS-predicate-only behaviour — no live child scoping).
    this._filterSpec = opts.filterSpec ?? null;
  }

  page() {
    return this._page instanceof Frame ? this._page.page() : this._page;
  }

  // The scope chain a CHILD of this locator inherits: our chain plus this locator's own
  // base selector, index, and (serializable) filter.
  _childScope() {
    return [
      ...this._scope,
      {
        sel: this._scopeSel ?? this._selector,
        getBy: this._getBy,
        idx: this._index ?? null,
        filter: this._filterSpec,
      },
    ];
  }

  _needsQuery() {
    return (
      !!this._querySource ||
      !!this._selector?.includes(":visible") ||
      !!this._parent?._needsQuery() ||
      (!!this._composite &&
        (this._composite.left._needsQuery() || this._composite.right._needsQuery()))
    );
  }
  async _prepareQuery() {
    if (this._needsQuery()) {
      await this._page._ensureLive();
      if (!this._page._live) throw new Error("Extended locator queries require a live document");
    }
  }
  _queryRecords() {
    const marker = `data-turbo-surf-query-${randomUUID()}`;
    const result = this._page._readEngine(`
      const matches = ${this._liveElementsExpression()};
      const all = Array.from(document.querySelectorAll('*'));
      const positions = new Map(all.map((element, index) => [element, index]));
      // Mark a detached copy: reparsing may repair invalid live HTML and reorder nodes.
      const copy = document.documentElement.cloneNode(true);
      const copies = [copy, ...copy.querySelectorAll('*')];
      const indices = matches.map(element => positions.get(element));
      indices.forEach((index, order) => copies[index].setAttribute(${JSON.stringify(marker)}, String(order)));
      return {html: copy.outerHTML, indices, text: matches.map(element => element.textContent)};
    `);
    this._queryHtml = result.html;
    const records = JSON.parse(native.query(result.html, `[${marker}]`, "css"));
    const ordered = Array.from({ length: result.indices.length });
    for (const record of records) {
      const order = Number(native.attrOf(result.html, record.node, marker));
      ordered[order] = { ...record, idx: result.indices[order], liveText: result.text[order] };
    }
    return ordered;
  }
  _all() {
    if (this._needsQuery()) return this._queryRecords();
    let m;
    if (this._parent && this._selector) {
      const seen = new Set();
      m = [];
      for (const parent of this._parent._all()) {
        for (const match of JSON.parse(
          native.query(this._html, this._selector, "auto", parent.node),
        )) {
          if (!seen.has(match.node)) {
            seen.add(match.node);
            m.push(match);
          }
        }
      }
    } else {
      m = this._resolve(this._page._html);
      if (this._parent) {
        const descendants = new Set();
        for (const parent of this._parent._all()) {
          for (const match of JSON.parse(native.query(this._html, "*", "css", parent.node))) {
            descendants.add(match.node);
          }
        }
        m = m.filter((match) => descendants.has(match.node));
      }
    }
    if (this._filter) m = m.filter(this._filter);
    if (this._index == null) return m;
    const i = this._index < 0 ? m.length + this._index : this._index;
    return m[i] ? [m[i]] : [];
  }
  _singleMatch() {
    const matches = this._all();
    if (matches.length > 1) throw strictLocatorError(matches.length);
    return matches[0] ?? null;
  }
  _checkLiveCardinality(matches) {
    if (this._index == null && matches.length > 1) throw strictLocatorError(matches.length);
  }
  _node() {
    return this._singleMatch()?.node ?? null;
  }
  _requireNode() {
    const n = this._node();
    if (n == null) throw new Error("turbo-surf: locator matched no elements");
    return n;
  }
  get _html() {
    return this._needsQuery() && this._queryHtml != null ? this._queryHtml : this._page._html;
  }

  // --- composition (pure JS, no napi) ---
  // Carry `_selector` through composition so `getByTestId(x).locator('input').first()`
  // stays selector-backed (→ drivable in a live session). `filter` makes it
  // unindexable for live dispatch (a JS predicate over matches), so it's dropped there.
  _derive(opts) {
    const loc = new Locator(this._page, this._resolve, {
      ...opts,
      count: this._count,
      getBy: this._getBy,
      testId: this._testId,
      parent: this._parent,
      composite: this._composite,
      scope: this._scope,
      // The base selector survives composition (incl. `.filter()`); the serializable filter
      // spec carries unless this derivation sets its own (a `.filter()` call passes one).
      scopeSel: this._scopeSel ?? this._selector,
      filterSpec: opts.filterSpec !== undefined ? opts.filterSpec : this._filterSpec,
    });
    if (this._selector && opts.filter == null) loc._selector = this._selector;
    return loc;
  }
  first() {
    return this.nth(0);
  }
  last() {
    return this.nth(-1);
  }
  nth(i) {
    if (this._needsQuery()) return new Locator(this._page, null, { querySource: this, index: i });
    // An earlier index has already reduced the set to at most one element.
    const index =
      this._index == null ? i : i === 0 || i === -1 ? this._index : Number.MAX_SAFE_INTEGER;
    return this._derive({ index, filter: this._filter });
  }
  filter(opts = {}) {
    for (const key of ["has", "hasNot"]) {
      if (opts[key] != null && (!(opts[key] instanceof Locator) || opts[key]._page !== this._page))
        throw new Error("Filter locators must belong to the same frame");
    }
    return new Locator(this._page, null, { querySource: this, queryFilter: { ...opts } });
  }
  _combine(other, operation) {
    if (!(other instanceof Locator) || other._page !== this._page)
      throw new Error("Locator operands must belong to the same frame");
    return new Locator(
      this._page,
      () => {
        const left = this._all();
        const right = other._all();
        const keep = new Set(right.map((match) => match.node));
        const matches =
          operation === "and" ? left.filter((match) => keep.has(match.node)) : [...left, ...right];
        const records = new Map(matches.map((match) => [match.node, match]));
        const ordered = [];
        for (const match of JSON.parse(native.query(this._html, "*", "css")))
          if (records.has(match.node)) ordered.push(records.get(match.node));
        return ordered;
      },
      { composite: { left: this, right: other, operation } },
    );
  }
  and(other) {
    return this._combine(other, "and");
  }
  or(other) {
    return this._combine(other, "or");
  }
  _hasComposite() {
    return this._needsQuery() || this._composite != null || !!this._parent?._hasComposite();
  }
  _liveBaseExpression(context = "document") {
    if (this._querySource) return this._querySource._liveElementsExpression(context);
    if (this._composite) {
      const { left, right, operation } = this._composite;
      return `(() => { const left = ${left._liveElementsExpression(context)}; const right = ${right._liveElementsExpression(context)}; const keep = new Set(right); return (${orderedLocatorElements.toString()})(${operation === "and" ? "left.filter(element => keep.has(element))" : "[...left, ...right]"}); })()`;
    }
    if (this._selector) {
      const roots = this._parent ? this._parent._liveElementsExpression(context) : `[${context}]`;
      return `(${orderedLocatorElements.toString()})(${roots}.flatMap(root => Array.from(${!isXPathSelector(this._selector) ? `(${queryVisibleSelector.toString()})(root, ${JSON.stringify(this._selector.replace(/^css=/, ""))}, ${renderedVisibility.toString()})` : `__tsNs.tcQuerySelector(root, ${JSON.stringify(this._selector)})`})))`;
    }
    if (this._testId) {
      const { attribute, source, flags, root } = this._testId;
      return `(() => { const re = new RegExp(${JSON.stringify(source)}, ${JSON.stringify(flags)}); const roots = ${root ? `(${context}).querySelectorAll(${JSON.stringify(root)})` : `[${context}]`}; const matches = []; for (const root of roots) for (const element of root.querySelectorAll(${JSON.stringify(`[${attribute}]`)})) { re.lastIndex = 0; if (re.test(element.getAttribute(${JSON.stringify(attribute)}))) matches.push(element); } return (${orderedLocatorElements.toString()})(matches); })()`;
    }
    if (this._getBy) {
      const g = this._getBy;
      return `(() => { __tsNs.tcGetBy(${JSON.stringify(g.kind)}, ${JSON.stringify(g.value)}, ${JSON.stringify(g.name)}, null, ${JSON.stringify(g.options ?? {})}); const all = Array.from(document.querySelectorAll('*')); const roots = ${g.root ? `Array.from((${context}).querySelectorAll(${JSON.stringify(g.root)}))` : `[${context}]`}; return JSON.parse(globalThis.__RESULT).map(match => all[match.idx]).filter(element => roots.some(root => root !== element && root.contains(element))); })()`;
    }
    throw new Error("Custom locator expressions cannot be composed in a live DOM");
  }
  _liveElementsExpression(context = "document") {
    let source = this._liveBaseExpression(context);
    if (this._parent && !this._selector)
      source = `(() => { const roots = ${this._parent._liveElementsExpression(context)}; return ${source}.filter(element => roots.some(root => root !== element && root.contains(element))); })()`;
    if (this._queryFilter) {
      const opts = this._queryFilter;
      const checks = [];
      for (const key of ["hasText", "hasNotText"]) {
        const value = opts[key];
        if (value == null) continue;
        const check =
          value instanceof RegExp
            ? `new RegExp(${JSON.stringify(value.source)}, ${JSON.stringify(value.flags)}).test(element.textContent || '')`
            : `String(element.textContent || '').replace(/\\s+/g, ' ').trim().toLowerCase().includes(${JSON.stringify(String(value).replace(/\s+/g, " ").trim().toLowerCase())})`;
        checks.push(key === "hasNotText" ? `!(${check})` : check);
      }
      for (const key of ["has", "hasNot"])
        if (opts[key]) {
          const check = `(${opts[key]._liveElementsExpression("element")}).length > 0`;
          checks.push(key === "hasNot" ? `!(${check})` : check);
        }
      if (opts.visible != null)
        checks.push(`(${renderedVisibility.toString()})(element) === ${!!opts.visible}`);
      source = `${source}.filter(element => ${checks.length ? checks.join(" && ") : "true"})`;
    }
    if (this._filter) {
      if (!this._filterSpec)
        throw new Error("This locator filter cannot be composed in a live DOM");
      const spec = this._filterSpec;
      source = `${source}.filter(element => ${spec.hasText != null ? `(element.textContent || '').includes(${JSON.stringify(spec.hasText)})` : "true"} && ${spec.hasNotText != null ? `!(element.textContent || '').includes(${JSON.stringify(spec.hasNotText)})` : "true"})`;
    }
    if (this._index != null)
      source = `(() => { const matches = ${source}; const index = ${this._index}; const found = matches[index < 0 ? matches.length + index : index]; return found ? [found] : []; })()`;
    return source;
  }
  // When this locator (or an ancestor) was indexed via `.nth(i)`, a CSS-concat selector can't
  // express "the i-th match's subtree" — return the scope CHAIN so the child drives via
  // __tcResolveScoped. Null for the common non-indexed case (keep the simple CSS-concat path).
  _indexedScope() {
    const sc = this._childScope();
    return sc.some(
      (s) => s.idx != null || s.filter != null || s.getBy != null || isXPathSelector(s.sel),
    )
      ? sc
      : null;
  }
  // A selector-backed child carrying the nth-aware scope chain (live-driven via the chain).
  _scopedSelectorChild(selector, scope, parent = this) {
    const loc = this._page._locFromSelector(selector);
    loc._scope = scope;
    loc._parent = parent;
    return loc;
  }
  // Nested locators: CSS-concat when this locator is selector-backed (the common
  // `.card`→`button` case); a scope chain when an ancestor was `.nth(i)`. getBy-rooted nesting
  // carries its matcher through the same scope chain.
  locator(selector) {
    if (this._hasComposite()) return this._scopedSelectorChild(selector, this._childScope(), this);
    // A `.nth()`/`.filter()` ancestor → walk the scope chain (it carries the index/filter a
    // CSS-concat can't express), even if `.filter()` dropped our own `_selector`.
    const scope = this._indexedScope();
    if (hasSelectorEngine(selector))
      return this._scopedSelectorChild(selector, scope ?? this._childScope(), this);
    if (selector.includes(":scope"))
      return this._scopedSelectorChild(selector, scope ?? this._childScope(), this);
    if (scope) return this._scopedSelectorChild(selector, scope);
    if (!this._selector) return this._page.locator(selector);
    return this._page.locator(`${this._selector} ${selector}`);
  }
  // Scope getBy* to this locator's subtree when it is selector-backed (descendant matching via
  // the `root` selector, or the nth-aware scope chain when an ancestor was `.nth(i)`). A
  // getBy-rooted parent isn't CSS-expressible → document-scoped.
  _scopedGetBy(kind, value, name, options = null) {
    if (this._hasComposite())
      return new Locator(
        this._page,
        (html) => JSON.parse(native.getBy(html, kind, value, name, null, JSON.stringify(options))),
        { getBy: { kind, value, name, options }, parent: this },
      );
    const scope = this._indexedScope();
    if (scope) {
      return new Locator(
        this._page,
        (h) => JSON.parse(native.getBy(h, kind, value, name, null, JSON.stringify(options))),
        {
          getBy: { kind, value, name, options },
          scope,
          parent: this,
        },
      );
    }
    const root = this._selector;
    return new Locator(
      this._page,
      (h) => JSON.parse(native.getBy(h, kind, value, name, root, JSON.stringify(options))),
      {
        getBy: { kind, value, name, root, options },
      },
    );
  }
  getByRole(role, o) {
    if (!this._selector && !this._indexedScope() && !this._hasComposite())
      return this._page.getByRole(role, o);
    return this._scopedGetBy("role", role, o?.name != null ? String(o.name) : null, o);
  }
  getByText(t, o) {
    if (!this._selector && !this._indexedScope() && !this._hasComposite())
      return this._page.getByText(t, o);
    return this._scopedGetBy("text", String(t), null, o);
  }
  getByLabel(t, o) {
    if (!this._selector && !this._indexedScope() && !this._hasComposite())
      return this._page.getByLabel(t, o);
    return this._scopedGetBy("label", String(t), null, o);
  }
  getByTestId(t) {
    if (t instanceof RegExp) {
      const child = this._page._regexTestId(t, this._selector);
      if (this._hasComposite()) child._parent = this;
      return child;
    }
    // Scope to this locator's subtree when it is selector-backed (like `locator()` does) —
    // `card.getByTestId('x')` must match only descendants of the card, not the whole document.
    // A `.filter()`/`.nth()` ancestor walks the scope chain (it carries the filter/index our
    // own `_selector` can't), so check that BEFORE falling back to a document-wide getByTestId.
    const childSel = `[${this._page._context._testIdAttribute}="${t}"]`;
    if (this._hasComposite()) return this._scopedSelectorChild(childSel, this._childScope(), this);
    const scope = this._indexedScope();
    if (scope) return this._scopedSelectorChild(childSel, scope);
    if (!this._selector) return this._page.getByTestId(t);
    return this._page.locator(`${this._selector} ${childSel}`);
  }
  getByPlaceholder(t, o) {
    return this.locator(`[placeholder*=${cssAttrValue(t)}]`);
  }
  getByAltText(t, o) {
    return this.locator(`[alt*=${cssAttrValue(t)}]`);
  }
  getByTitle(t, o) {
    return this.locator(`[title*=${cssAttrValue(t)}]`);
  }

  // --- counts / text (from the single resolve payload) ---
  async count() {
    await this._prepareQuery();
    if (this._needsQuery())
      return this._page._readEngine(`return (${this._liveElementsExpression()}).length;`);
    if (!this._needsQuery() && this._count && !this._filter && this._scope.length === 0) {
      const count = this._count(this._page._html);
      if (this._index == null) return count;
      const index = this._index < 0 ? count + this._index : this._index;
      return Number.isInteger(index) && index >= 0 && index < count ? 1 : 0;
    }
    return this._all().length;
  }
  async all() {
    await this._prepareQuery();
    return this._all().map((_, i) => this.nth(i));
  }
  async textContent() {
    await this._prepareQuery();
    const match = this._singleMatch();
    return match ? this._textContent(match) : null;
  }
  _textContent(match) {
    if (match.liveText !== undefined) return match.liveText;
    return match.node == null ? (match.text ?? "") : native.textContentOf(this._html, match.node);
  }
  async innerText() {
    await this._prepareQuery();
    return (await this._readElement((element) => element.innerText)) ?? "";
  }
  async innerHTML() {
    await this._prepareQuery();
    if (this._needsQuery()) return this._readElement((element) => element.innerHTML);
    return this._singleMatch()?.html ?? null;
  }
  async allTextContents() {
    await this._prepareQuery();
    return this._all().map((match) => this._textContent(match));
  }
  async allInnerTexts() {
    await this._prepareQuery();
    const matches = this._all();
    const out = [];
    for (let i = 0; i < matches.length; i++) out.push(await this.nth(i).innerText());
    return out;
  }

  // --- accessors (node-handle backed; cross to Rust) ---
  async getAttribute(name) {
    await this._prepareQuery();
    const n = this._node();
    return n == null ? null : (native.attrOf(this._html, n, name) ?? null);
  }
  async _liveRead(fn, arg) {
    await this._prepareQuery();
    const index = await this._resolveLiveIdx();
    if (index == null) return null;
    // Pure DOM reads must not await unrelated exposed host callbacks.
    const result = this._page._readEngine(`
      try { return {value: (${encodeEvaluationValue.toString()})(
        (${fn.toString()})(document.querySelectorAll("*")[${index}],${JSON.stringify(arg ?? null)}))}; }
      catch (error) { return {error: (${encodeEvaluationError.toString()})(error)}; }
    `);
    if (result.error) throw decodeEvaluationError(result.error);
    return decodeEvaluationValue(result.value);
  }
  async inputValue() {
    await this._prepareQuery();
    if (this._page._live) return (await this._liveRead((el) => el.value)) ?? "";
    const n = this._node();
    return n == null ? "" : native.inputValueOf(this._html, n);
  }
  async isVisible() {
    await this._prepareQuery();
    if (this._node() == null) return false;
    await this._page._ensureLive();
    return !!(await this._liveRead(renderedVisibility));
  }
  async isHidden() {
    await this._prepareQuery();
    return !(await this.isVisible());
  }
  async isChecked() {
    await this._prepareQuery();
    return this._readElement((element) => {
      if (element.tagName === "INPUT" && /^(checkbox|radio)$/.test(element.type))
        return element.checked;
      const roles = ["checkbox", "radio", "switch", "menuitemcheckbox", "menuitemradio"];
      if ((element.getAttribute("role") ?? "").split(/\s+/).some((role) => roles.includes(role))) {
        return element.getAttribute("aria-checked") === "true";
      }
      throw new Error("Not a checkbox or radio button");
    });
  }
  async isEnabled() {
    await this._prepareQuery();
    const n = this._node();
    return n != null && native.isEnabled(this._html, n);
  }
  async isDisabled() {
    await this._prepareQuery();
    return !(await this.isEnabled());
  }
  async isEditable() {
    await this._prepareQuery();
    const n = this._node();
    return n != null && native.isEditable(this._html, n);
  }
  async _readElement(fn) {
    await this._prepareQuery();
    const live = this._page._session != null;
    const match = live ? null : this._singleMatch();
    const index = live
      ? await this._resolveLiveIdx()
      : match == null
        ? null
        : JSON.parse(native.query(this._html, "*", "css")).findIndex(
            (node) => node.node === match.node,
          );
    if (index == null) return null;
    const script = `globalThis.__RESULT = (() => {
      try { const element = document.querySelectorAll('*')[${index}];
        return JSON.stringify({value: element ? (${fn.toString()})(element) : null});
      } catch (error) { return JSON.stringify({error: String(error.message)}); }
    })()`;
    const raw = live ? await this._page._evalEngine(script) : native.evaluate(this._html, script);
    const result = JSON.parse(raw);
    if (result.error) throw new Error(result.error);
    return result.value;
  }
  async isEmpty() {
    await this._prepareQuery();
    return this._readElement((element) => {
      const text = /^(INPUT|TEXTAREA)$/.test(element.tagName) ? element.value : element.textContent;
      return String(text ?? "").trim().length === 0;
    });
  }
  async ariaRole() {
    await this._prepareQuery();
    const n = this._node();
    return n == null ? null : native.ariaRoleOf(this._html, n);
  }
  async accessibleName() {
    await this._prepareQuery();
    const n = this._node();
    return n == null ? "" : native.accessibleNameOf(this._html, n);
  }
  async accessibleDescription() {
    await this._prepareQuery();
    const n = this._node();
    return n == null ? "" : native.accessibleDescriptionOf(this._html, n);
  }
  async selectedValues() {
    await this._prepareQuery();
    if (this._page._live)
      return (
        (await this._readElement((element) =>
          Array.from(element.options)
            .filter((option) => option.selected)
            .map((option) => option.value),
        )) ?? []
      );
    const n = this._node();
    return n == null ? [] : native.selectedValuesOf(this._html, n);
  }
  async cssValue(name) {
    await this._prepareQuery();
    if (this._page._live)
      return (
        (await this._liveRead(
          (el, property) => getComputedStyle(el).getPropertyValue(property),
          name,
        )) ?? ""
      );
    const n = this._node();
    return n == null ? "" : native.cssValueOf(this._html, n, name);
  }
  // Batch every boolean/text/role accessor for the matched node in ONE napi
  // crossing — backs expect(locator) chains. Null if no match.
  _snapshot() {
    const n = this._node();
    return n == null ? null : JSON.parse(native.nodeSnapshot(this._html, n));
  }
  async evaluate(fn, arg) {
    await this._prepareQuery();
    await this._page._ensureLive();
    const index = await this._resolveLiveIdx();
    if (index == null) throw new Error("turbo-surf: locator matched no elements");
    return this._page.evaluate(
      `(${fn.toString()})(document.querySelectorAll('*')[${index}], ${evaluationArgument(arg)})`,
    );
  }
  async evaluateAll(fn, arg) {
    await this._prepareQuery();
    await this._page._ensureLive();
    let indices;
    if (!this._needsQuery() && this._selector && !this._filter && this._scope.length === 0) {
      const raw = await this._page._evalEngine(`(() => {
        const all = Array.from(document.querySelectorAll('*'));
        const positions = new Map(all.map((element, index) => [element, index]));
        let matches = Array.from(document.querySelectorAll(${JSON.stringify(this._selector)}));
        const n = ${this._index ?? "null"};
        if (n != null) matches = [matches[n < 0 ? matches.length + n : n]].filter(Boolean);
        globalThis.__RESULT = JSON.stringify(matches.map(element => positions.get(element)));
      })()`);
      indices = JSON.parse(raw);
    } else if (this._needsQuery()) {
      indices = this._all().map((element) => element.idx);
    } else {
      const all = JSON.parse(native.query(this._html, "*", "css"));
      const positions = new Map(all.map((element, index) => [element.node, index]));
      indices = this._all().map((element) => positions.get(element.node));
    }
    return this._page.evaluate(
      `(() => { const all = document.querySelectorAll('*'); return (${fn.toString()})(${JSON.stringify(indices)}.map(index => all[index]), ${evaluationArgument(arg)}); })()`,
    );
  }
  async ariaSnapshot() {
    await this._prepareQuery();
    const n = this._node();
    return n == null ? "" : native.ariaSnapshot(this._html, n);
  }

  // --- actions (mutate the page's cached DOM by handle) ---
  // True when we can drive this locator through the live app (session up + the locator
  // is CSS-selector-backed, so the live isolate can re-resolve it). getBy{Role,Text,…}
  // locators aren't selector-backed → fall back to the static/intent path.
  get _canDriveLive() {
    return this._page._live;
  }
  // Dispatch an interaction into the LIVE isolate. Selector-backed locators dispatch
  // through their CSS selector; getBy*/filtered locators have none, so we target the
  // matched element by its document-order index (`querySelectorAll('*')[idx]`) — this
  // is what lets getByRole/getByLabel/getByText drive the running app (fill/click), not
  // just mutate the static HTML snapshot.
  async _driveLive(kind, value) {
    await this._prepareQuery();
    if (this._hasComposite()) {
      const index = await this._resolveLiveIdx();
      if (index == null) throw new Error("turbo-surf: locator matched no elements");
      return this._page._sessionDispatch("*", index, kind, value);
    }
    if (this._testId) {
      const idx = await this._resolveLiveIdx();
      if (idx == null) throw new Error("turbo-surf: locator matched no elements");
      return this._page._sessionDispatch("*", idx, kind, value);
    }
    // nth-aware scope chain: a CSS selector can't target "the i-th match's subtree", so walk
    // the chain in the live isolate and dispatch on the matched element's global index.
    if (
      this._scope &&
      (this._scope.some((s) => s.idx != null || s.filter != null || s.getBy != null) ||
        this._parent ||
        hasSelectorEngine(this._selector)) &&
      this._page._session != null
    ) {
      const leaf = this._selector
        ? { selector: this._selector }
        : this._getBy
          ? { getBy: this._getBy }
          : null;
      if (leaf) {
        const raw = await this._page._evalEngine(
          `__tsNs.tcResolveScoped(${JSON.stringify(this._scope)},${JSON.stringify(leaf)});`,
        );
        let matches = [];
        try {
          matches = JSON.parse(raw);
        } catch {
          matches = [];
        }
        this._checkLiveCardinality(matches);
        const i =
          this._index == null ? 0 : this._index < 0 ? matches.length + this._index : this._index;
        const target = matches[i];
        if (!target || target.idx == null)
          throw new Error("turbo-surf: locator matched no elements");
        return this._page._sessionDispatch("*", target.idx, kind, value);
      }
    }
    if (this._selector)
      return this._page._sessionDispatch(
        this._selector,
        this._index ?? 0,
        kind,
        value,
        this._index == null,
      );
    // getByRole/getByText/getByLabel: resolve the match IN the live isolate so the index is
    // a LIVE `querySelectorAll('*')` position (the same context we dispatch into). Resolving
    // over a re-serialized snapshot can reorder portal'd nodes → wrong element clicked.
    if (this._getBy && this._page._session != null) {
      const g = this._getBy;
      const raw = await this._page._evalEngine(
        `__tsNs.tcGetBy(${JSON.stringify(g.kind)},${JSON.stringify(g.value)},${g.name == null ? "null" : JSON.stringify(g.name)},${g.root == null ? "null" : JSON.stringify(g.root)},${JSON.stringify(g.options ?? {})});`,
      );
      let matches = [];
      try {
        matches = JSON.parse(raw);
      } catch {
        matches = [];
      }
      if (!matches.length) throw new Error("turbo-surf: locator matched no elements");
      this._checkLiveCardinality(matches);
      const i =
        this._index == null ? 0 : this._index < 0 ? matches.length + this._index : this._index;
      const target = matches[i];
      if (!target || target.idx == null) throw new Error("turbo-surf: locator matched no elements");
      return this._page._sessionDispatch("*", target.idx, kind, value);
    }
    const m = this._all();
    if (!m.length || m[0].idx == null) throw new Error("turbo-surf: locator matched no elements");
    return this._page._sessionDispatch("*", m[0].idx, kind, value);
  }
  async _waitActionable(kind, options = {}) {
    await this._prepareQuery();
    await this._page._ensureLive();
    if (options.force) return;
    const deadline = Date.now() + (options.timeout ?? 30000);
    do {
      const live = this._page._live;
      const state = live
        ? await this._liveRead((element) => ({
            enabled:
              !element.matches(":disabled") && element.getAttribute("aria-disabled") !== "true",
            editable: !element.readOnly,
          }))
        : this._snapshot();
      const visible = live ? await this.isVisible() : state?.visible;
      if (state?.enabled && (kind !== "fill" || state.editable) && visible) return;
      if (Date.now() >= deadline) break;
      await this._page._redrain();
      await new Promise((resolve) => setTimeout(resolve, 10));
    } while (Date.now() < deadline);
    throw Object.assign(new Error(`Timeout waiting for locator to be actionable for ${kind}`), {
      name: "TimeoutError",
    });
  }
  async fill(value, options = {}) {
    await this._prepareQuery();
    await this._waitActionable("fill", options);
    const text = String(value);
    const match = this._singleMatch();
    if (
      /^<input[\s>]/i.test(match?.html ?? "") &&
      native.attrOf(this._html, match.node, "type")?.toLowerCase() === "number" &&
      text !== "" &&
      Number.isNaN(Number(text))
    )
      throw new Error("Cannot type text into input[type=number]");
    if (!this._canDriveLive && match) {
      const editable = native.attrOf(this._html, match.node, "contenteditable");
      if (["", "true", "plaintext-only"].includes(editable?.toLowerCase())) {
        await this._page._ensureLive();
      }
    }
    if (this._canDriveLive) return void (await this._driveLive("fill", text));
    this._page._html = native.fillNode(this._html, this._requireNode(), text);
  }
  async clear(options) {
    await this._prepareQuery();
    return this.fill("", options);
  }
  async type(value) {
    await this._prepareQuery();
    await this._page._ensureLive();
    return void (await this._driveLive("type", String(value)));
  }
  async pressSequentially(value) {
    await this._prepareQuery();
    return this.type(value);
  }
  async check() {
    await this._prepareQuery();
    if (this._canDriveLive) return void (await this._driveLive("check"));
    this._page._html = native.setCheckedNode(this._html, this._requireNode(), true);
  }
  async uncheck() {
    await this._prepareQuery();
    if (this._canDriveLive) return void (await this._driveLive("uncheck"));
    this._page._html = native.setCheckedNode(this._html, this._requireNode(), false);
  }
  async setChecked(on) {
    await this._prepareQuery();
    return on ? this.check() : this.uncheck();
  }
  async selectOption(value, options = {}) {
    await this._prepareQuery();
    const choices = normalizeSelectChoices(value);
    const deadline = Date.now() + (options.timeout ?? 30000);
    do {
      try {
        if (this._canDriveLive) return JSON.parse(await this._driveLive("select", choices));
        const node = this._requireNode();
        const index = JSON.parse(native.query(this._html, "*", "css")).findIndex(
          (match) => match.node === node,
        );
        const result = JSON.parse(
          native.evaluateEngine(
            this._html,
            `
          const values = (${selectOptionOperation.toString()})(document.querySelectorAll('*')[${index}], ${JSON.stringify(choices)}, true);
          return JSON.stringify({ values, html: (${serializeDocumentType.toString()})(document) + document.documentElement.outerHTML });`,
          ),
        );
        this._page._html = result.html;
        return result.values;
      } catch (error) {
        if (!error.message.includes("Options did not match") || Date.now() >= deadline) throw error;
        await this._page._redrain();
        await new Promise((resolve) => setTimeout(resolve, 10));
      }
    } while (true);
  }
  async click(opts = {}) {
    await this._prepareQuery();
    await this._waitActionable("click", opts);
    const modifiers = opts.modifiers?.map((modifier) =>
      modifier === "ControlOrMeta"
        ? process.platform === "darwin"
          ? "Meta"
          : "Control"
        : modifier,
    );
    opts = { ...opts, modifiers };
    if (this._canDriveLive) {
      const urlBefore = this._page._url;
      let r;
      for (let count = 1; count <= (opts.clickCount ?? 1); count++)
        r = await this._driveLive("click", { ...opts, clickCount: count });
      if (opts.button && opts.button !== "left") return;
      // A JS handler that navigated changed the URL (SPA router / form POST + redirect).
      // If the URL is unchanged AND the app didn't preventDefault, the click hit a plain
      // anchor whose navigation still needs the static intent fallback —
      // follow that anchor intent. Native form activation already owns submit
      // events, validation and cancellation. For a JS button (Inert
      // intent) or a handled click (PREVENTED) the dispatch already did the work.
      if (this._page._url === urlBefore && r !== "PREVENTED") {
        const n = this._node();
        if (n != null) {
          const intent = JSON.parse(native.clickNode(this._page._html, n, this._page._url));
          if (intent.action === "navigate") return this._page._followIntent(intent);
        }
      }
      return;
    }
    const intent = JSON.parse(native.clickNode(this._html, this._requireNode(), this._page._url));
    return this._page._followIntent(intent);
  }
  async dblclick(opts = {}) {
    await this._prepareQuery();
    return this.click({ ...opts, clickCount: 2 });
  }
  async tap() {
    await this._prepareQuery();
    return this.click();
  }
  async press(key) {
    await this._prepareQuery();
    await this._page._ensureLive();
    await this.focus();
    return (this._page.keyboard ?? makeKeyboard(this._page)).press(key);
  }
  async focus() {
    await this._prepareQuery();
    if (this._canDriveLive) return void (await this._driveLive("focus"));
  }
  async blur() {
    await this._prepareQuery();
    if (this._canDriveLive) return void (await this._driveLive("blur"));
  }
  async scrollIntoViewIfNeeded() {
    await this._prepareQuery();
    await this.evaluate((element) =>
      element.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "instant" }),
    );
  }
  async highlight() {}
  async _isFocused() {
    await this._prepareQuery();
    if (!this._canDriveLive) return false;
    try {
      const r = await this._driveLive("isfocused");
      return r === "true";
    } catch {
      return false;
    }
  }
  async waitFor(opts = {}) {
    await this._prepareQuery();
    // "Wait for this element" — for an SPA the element only exists after the app runs, so
    // ensure the page is hydrated (a live session). This covers the bare `goto(/login)` +
    // `emailField.waitFor()` login flow (no `waitUntil:'networkidle'`).
    await this._page._ensureLive();
    // Then actually poll for the requested state (Playwright defaults to 'visible'),
    // re-pumping the live app between tries — so the wait reflects reality instead of
    // resolving immediately (which masked "the element/modal never appeared").
    const state = opts.state ?? "visible";
    const timeout = opts.timeout ?? 5000;
    const ok = () => {
      const snap = this._snapshot();
      if (state === "attached") return snap != null;
      if (state === "detached") return snap == null;
      if (state === "hidden") return snap == null || !snap.visible;
      return snap != null && snap.visible; // 'visible'
    };
    if (!this._page._live) return undefined; // static page: best-effort, no polling surface
    const start = Date.now();
    while (!ok() && Date.now() - start < timeout) await this._page._redrain();
    if (!ok()) throw new Error(`turbo-surf: waitFor(state=${state}) timed out`);
    return undefined;
  }

  async boundingBox() {
    await this._prepareQuery();
    await this._page._ensureLive();
    return this._readElement((element) => {
      for (let ancestor = element; ancestor; ancestor = ancestor.parentElement) {
        if (ancestor.hasAttribute("hidden") || getComputedStyle(ancestor).display === "none")
          return null;
      }
      const style = getComputedStyle(element);
      if (
        style.visibility === "hidden" ||
        style.visibility === "collapse" ||
        !element.getClientRects().length
      )
        return null;
      const rect = element.getBoundingClientRect();
      return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    });
  }

  // An ElementHandle stand-in: carries enough to re-resolve the element in the live
  // isolate (selector / getBy + index), so `page.waitForFunction(fn, handle)` can run
  // `fn(element)` against the live node (e.g. wait for a submit button's !disabled).
  async elementHandle() {
    await ensureHandleSession(this._page);
    await this._prepareQuery();
    const index = await this._resolveLiveIdx();
    if (index == null) return null;
    return this._page.evaluateHandle(
      (index) => document.querySelectorAll("*")[index] ?? null,
      index,
    );
  }

  // Hover has no rendering effect here, but JS hover handlers (menu-open) are real.
  // Drive the live app with mouseenter/over events; otherwise best-effort no-op.
  async hover() {
    await this._prepareQuery();
    await this._page._ensureLive();
    if (this._page._live) return void (await this._driveLive("hover"));
  }

  // dispatchEvent(type) — fire a named DOM event into the live app (e.g. a row
  // checkbox's 'click'). Static snapshot has no JS handlers, so it needs the isolate.
  async dispatchEvent(type, eventInit = {}) {
    await this._prepareQuery();
    await this._page._ensureLive();
    if (this._page._live)
      return void (await this._driveLive("dispatch", { type: String(type), eventInit }));
  }

  // setInputFiles: read the file bytes in Node, build File objects in the isolate, and
  // assign them to the <input type=file> so the app's change handler / upload runs. No
  // OS file picker, but the upload PIPELINE is real.
  async setInputFiles(files) {
    await this._prepareQuery();
    const payload = normalizeInputFiles(files);
    await this._page._ensureLive();
    if (this._page._live) return void (await this._driveLive("setfiles", payload));
  }

  // Drag source element to the target locator: resolve both in the live isolate,
  // then drive a pointer+mouse+HTML5-drag sequence on source → target. No compositor,
  // so there's no native drag image, but JS handlers (onDragStart/Over/Drop/End and
  // pointer/mouse listeners) fire exactly as in a real browser (turbo-surf#227).
  async dragTo(target, _opts) {
    await this._prepareQuery();
    await this._page._ensureLive();
    if (!this._page._live) return;
    const src = await this._resolveLiveIdx();
    const dst = await target._resolveLiveIdx();
    if (src == null || dst == null) throw new Error("turbo-surf: locator matched no elements");
    await this._page._sessionEval(dragScript(src, dst));
  }

  // Resolve this locator to the live `querySelectorAll('*')` index of its first match.
  // Returns null when the locator matches nothing. Used by dragTo and page.dragAndDrop.
  async _resolveLiveIdx() {
    await this._prepareQuery();
    if (this._page._session == null) return null;
    if (this._needsQuery()) {
      const indices = this._page._readEngine(`
        const matches = ${this._liveElementsExpression()};
        const positions = new Map(Array.from(document.querySelectorAll('*'), (element, index) => [element, index]));
        return matches.map(element => positions.get(element));
      `);
      this._checkLiveCardinality(indices);
      return indices[0] ?? null;
    }
    if (this._hasComposite()) {
      const raw = await this._page._evalEngine(
        `(() => { const matches = ${this._liveElementsExpression()}; const all = Array.from(document.querySelectorAll('*')); const indices = new Map(all.map((element,index) => [element,index])); globalThis.__RESULT = JSON.stringify(matches.map(element => ({idx:indices.get(element)}))); })()`,
      );
      const matches = JSON.parse(raw);
      this._checkLiveCardinality(matches);
      return matches[0]?.idx ?? null;
    }
    if (this._testId) {
      const { attribute, source, flags, root } = this._testId;
      const raw = await this._page._evalEngine(
        `(() => { const re = new RegExp(${JSON.stringify(source)}, ${JSON.stringify(flags)}); const roots = ${root ? `document.querySelectorAll(${JSON.stringify(root)})` : "[document]"}; const hits = new Set(); for (const root of roots) for (const el of root.querySelectorAll(${JSON.stringify(`[${attribute}]`)})) { re.lastIndex = 0; if (re.test(el.getAttribute(${JSON.stringify(attribute)}))) hits.add(el); } const all = Array.from(document.querySelectorAll('*')); const matches = all.filter(el => hits.has(el)); if (${this._index == null} && matches.length > 1) throw new Error("strict mode violation: locator resolved to " + matches.length + " elements"); const n = ${this._index ?? 0}; globalThis.__RESULT = String(all.indexOf(matches[n < 0 ? matches.length + n : n])); })();`,
      );
      const idx = Number(raw);
      return Number.isInteger(idx) && idx >= 0 ? idx : null;
    }
    if (
      (this._scope &&
        this._scope.some((s) => s.idx != null || s.filter != null || s.getBy != null)) ||
      this._parent ||
      hasSelectorEngine(this._selector)
    ) {
      const leaf = this._selector
        ? { selector: this._selector }
        : this._getBy
          ? { getBy: this._getBy }
          : null;
      if (leaf) {
        const raw = await this._page._evalEngine(
          `__tsNs.tcResolveScoped(${JSON.stringify(this._scope)},${JSON.stringify(leaf)});`,
        );
        let matches = [];
        try {
          matches = JSON.parse(raw);
        } catch {
          matches = [];
        }
        this._checkLiveCardinality(matches);
        const i =
          this._index == null ? 0 : this._index < 0 ? matches.length + this._index : this._index;
        return matches[i]?.idx ?? null;
      }
    }
    if (this._selector) {
      const raw = await this._page._evalEngine(
        `(() => { const els = document.querySelectorAll(${JSON.stringify(this._selector)}); if (${this._index == null} && els.length > 1) throw new Error("strict mode violation: locator resolved to " + els.length + " elements"); const n = ${this._index ?? 0}; const i = n < 0 ? els.length + n : n; const list = []; const all = document.querySelectorAll('*'); for (let k = 0; k < all.length; k++) if (all[k] === els[i]) { globalThis.__RESULT = String(k); return; } globalThis.__RESULT = ''; })();`,
      );
      const n = raw ? Number(raw) : NaN;
      return Number.isFinite(n) ? n : null;
    }
    if (this._getBy) {
      const g = this._getBy;
      const raw = await this._page._evalEngine(
        `__tsNs.tcGetBy(${JSON.stringify(g.kind)},${JSON.stringify(g.value)},${g.name == null ? "null" : JSON.stringify(g.name)},${g.root == null ? "null" : JSON.stringify(g.root)},${JSON.stringify(g.options ?? {})});`,
      );
      let matches = [];
      try {
        matches = JSON.parse(raw);
      } catch {
        matches = [];
      }
      this._checkLiveCardinality(matches);
      const i =
        this._index == null ? 0 : this._index < 0 ? matches.length + this._index : this._index;
      return matches[i]?.idx ?? null;
    }
    return null;
  }

  // --- unsupported (no rendering surface / no input hardware) → honest throws ---
  screenshot = UNSUPPORTED("locator.screenshot", PIXEL);
  selectText = UNSUPPORTED("locator.selectText", INPUT);
}

// filter({ hasText, has, hasNot, hasNotText }) → JS predicate over a match.
function orderedLocatorElements(elements) {
  const selected = new Set(elements);
  return Array.from(document.querySelectorAll("*")).filter((element) => selected.has(element));
}

function strictLocatorError(count) {
  return new Error(`strict mode violation: locator resolved to ${count} elements`);
}

// Build the JS run in a live session to drive an interaction by dispatching REAL DOM
// events on the selector-matched element, so the running app's (React's) delegated
// handlers fire. `kind`: "click" fires a realistic mousedown→focus→mouseup→click
// sequence; "fill" sets the value + fires input/change (React controlled inputs).
// Normalize Playwright setInputFiles args (path string | {name,mimeType,buffer} |
// arrays thereof) into [{ name, type, data }] with data as a latin1 binary string
// (so the isolate's File/Blob → FileReader.readAsDataURL → btoa round-trips bytes).
function normalizeSelectChoices(value) {
  const values = value == null ? [] : Array.isArray(value) ? value : [value];
  return values.map((choice) => {
    if (typeof choice === "string") return { value: choice };
    if (
      !choice ||
      typeof choice !== "object" ||
      !["value", "label", "index"].some((key) => choice[key] !== undefined)
    )
      throw new TypeError("selectOption requires a value, label or index");
    if (choice.index !== undefined && (!Number.isInteger(choice.index) || choice.index < 0))
      throw new TypeError("selectOption index must be a nonnegative integer");
    for (const key of ["value", "label"])
      if (choice[key] !== undefined && typeof choice[key] !== "string")
        throw new TypeError(`selectOption ${key} must be a string`);
    return { value: choice.value, label: choice.label, index: choice.index };
  });
}

function serializeDocumentType(document) {
  const type = document.doctype;
  if (!type) return "";
  const quoted = (value) => (value.includes('"') ? "'" + value + "'" : '"' + value + '"');
  const publicId = type.publicId || "";
  const systemId = type.systemId || "";
  return (
    "<!DOCTYPE " +
    type.name +
    (publicId ? " PUBLIC " + quoted(publicId) : "") +
    (systemId ? (publicId ? " " : " SYSTEM ") + quoted(systemId) : "") +
    ">"
  );
}

function selectOptionOperation(element, choices, snapshot) {
  if (element.tagName !== "SELECT") throw new Error("Element is not a select");
  const options = Array.from(element.options);
  let remaining = choices.slice();
  const selected = [];
  for (let index = 0; index < options.length && remaining.length; index++) {
    const option = options[index];
    const matches = (choice) =>
      (choice.value === undefined || choice.value === option.value) &&
      (choice.label === undefined || choice.label === option.label) &&
      (choice.index === undefined || choice.index === index);
    if (!remaining.some(matches)) continue;
    selected.push(option);
    remaining = element.multiple ? remaining.filter((choice) => !matches(choice)) : [];
  }
  if (remaining.length) throw new Error("Options did not match the requested selection");
  const selectedSet = new Set(selected);
  element.selectedIndex = -1;
  for (const option of options) {
    option.selected = selectedSet.has(option);
    if (snapshot) {
      if (option.selected) option.setAttribute("selected", "");
      else option.removeAttribute("selected");
    }
  }
  if (!snapshot) {
    element.dispatchEvent(new Event("input", { bubbles: true, composed: true }));
    element.dispatchEvent(new Event("change", { bubbles: true }));
  }
  return selected.map((option) => option.value);
}

function normalizeInputFiles(files) {
  const fs = require("node:fs");
  const path = require("node:path");
  const list = Array.isArray(files) ? files : [files];
  return list.map((f) => {
    if (typeof f === "string") {
      const data = fs.readFileSync(f).toString("latin1");
      return {
        name: path.basename(f),
        type: native.fileMimeType(f),
        lastModified: fs.statSync(f).mtimeMs,
        data,
      };
    }
    let data = "";
    if (f.buffer != null) data = Buffer.from(f.buffer).toString("latin1");
    else if (f.body != null) data = String(f.body);
    return { name: f.name ?? "file", type: f.mimeType ?? f.type ?? "", data };
  });
}

function fillInteractionScript(pick, value) {
  return `(() => { ${pick} else {
    el.focus();
    const value = ${value};
    const edit = { bubbles: true, composed: true, data: value === '' ? null : value,
      inputType: value === '' ? 'deleteContentForward' : 'insertText' };
    if (!el.dispatchEvent(new InputEvent('beforeinput', { ...edit, cancelable: true }))) {
      globalThis.__RESULT = 'OK'; return;
    }
    if (el.isContentEditable) {
      const range = el.ownerDocument.createRange();
      range.selectNodeContents(el);
      range.deleteContents();
      if (value !== '') {
        const text = el.ownerDocument.createTextNode(value);
        range.insertNode(text);
        range.setStart(text, value.length);
      }
      range.collapse(true);
      const selection = el.ownerDocument.defaultView.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
    } else {
      const pending = __tsNs.playwrightEditingCommits ??= new WeakMap();
      if (!pending.has(el)) {
        const before = el.value;
        pending.set(el, before);
        el.addEventListener('blur', () => {
          pending.delete(el);
          if (el.value !== before) el.dispatchEvent(new Event('change', { bubbles: true }));
        }, { once: true });
      }
      // The prototype setter bypasses React's instance value tracker before the input event.
      const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement
        : el.tagName === 'SELECT' ? HTMLSelectElement : HTMLInputElement;
      const desc = Object.getOwnPropertyDescriptor(proto.prototype, 'value');
      if (desc?.set) desc.set.call(el, value); else el.value = value;
    }
    el.dispatchEvent(new InputEvent('input', edit));
    globalThis.__RESULT = 'OK';
  } })();`;
}

function typingInteractionScript(pick, value, replaceSelection = "__tsNs.replaceControlSelection") {
  return `(() => { ${pick} else {
    el.focus();
    for (const char of ${value}) {
      const code = /^[a-z]$/i.test(char) ? 'Key' + char.toUpperCase() : '';
      const key = { key: char, code, bubbles: true, cancelable: true, composed: true };
      if (el.dispatchEvent(new KeyboardEvent('keydown', key)) &&
          el.dispatchEvent(new KeyboardEvent('keypress', key))) {
        const edit = { data: char, inputType: 'insertText', bubbles: true, composed: true };
        if (el.dispatchEvent(new InputEvent('beforeinput', { ...edit, cancelable: true }))) {
          if (!${replaceSelection}(el, char)) {
            const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement : HTMLInputElement;
            const setter = Object.getOwnPropertyDescriptor(proto.prototype, 'value').set;
            setter.call(el, el.value + char);
          }
          el.dispatchEvent(new InputEvent('input', edit));
        }
      }
      el.dispatchEvent(new KeyboardEvent('keyup', key));
    }
    globalThis.__RESULT = 'OK';
  } })();`;
}

function interactionScript(selector, index, kind, value, strict = false, frameTyping = false) {
  const sel = JSON.stringify(selector);
  const idx = JSON.stringify(index);
  const val = JSON.stringify(value ?? "");
  const remoteHandle = selector && typeof selector === "object" ? selector.__remoteHandle : null;
  const selection = Number.isInteger(remoteHandle)
    ? `[__tsNs.remoteHandleGet(${remoteHandle},globalThis)]`
    : `document.querySelectorAll(${sel})`;
  const pick = `const els = ${selection};
    if (${strict} && els.length > 1) throw new Error('strict mode violation: locator resolved to ' + els.length + ' elements');
    let __i = ${idx}; if (__i < 0) __i = els.length + __i;
    const el = els[__i];
    if (${Number.isInteger(remoteHandle)} && el && !el.isConnected) throw new Error('Element is not attached to the DOM');
    if (!el) { globalThis.__RESULT = "NO_MATCH"; }`;
  if (kind === "type")
    return typingInteractionScript(
      pick,
      val,
      frameTyping ? "__tsNs.replaceFrameControlSelection" : undefined,
    );
  if (kind === "fill") return fillInteractionScript(pick, val);
  if (kind === "check" || kind === "uncheck") {
    const on = kind === "check";
    return `(() => { ${pick} else {
      if (el.checked !== ${on}) {
        el.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
        if (el.checked !== ${on}) throw new Error("Click did not change checked state");
      }
      globalThis.__RESULT = "OK";
    } })();`;
  }
  if (kind === "select") {
    return `(() => { ${pick} else {
      globalThis.__RESULT = JSON.stringify((${selectOptionOperation.toString()})(el, ${val}, false));
    } })();`;
  }
  if (kind === "setfiles") {
    // value is an array of { name, type, data(latin1) }. Build genuine File objects from
    // the exact bytes, select them through the input's own FileList (the prototype files
    // setter, as Chrome's DOM.setFileInputFiles does), then fire input/change.
    return `(() => { ${pick} else {
      const files = ${val};
      if (el.tagName !== 'INPUT' || el.type !== 'file') throw new Error('Node is not an input[type=file]');
      if (!el.multiple && files.length > 1) throw new Error('Non-multiple file input can only accept single file');
      const data = new globalThis.DataTransfer();
      for (const f of files) {
        const bytes = new Uint8Array(f.data.length);
        for (let i = 0; i < bytes.length; i++) bytes[i] = f.data.charCodeAt(i) & 255;
        const options = { type: f.type };
        if (f.lastModified != null) options.lastModified = f.lastModified;
        data.items.add(new globalThis.File([bytes], f.name, options));
      }
      Object.getOwnPropertyDescriptor(globalThis.HTMLInputElement.prototype, "files").set.call(el, data.files);
      el.dispatchEvent(new Event("input", { bubbles: true, composed: true }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
      globalThis.__RESULT = "OK";
    } })();`;
  }
  if (kind === "dispatch") {
    // Synthetic dispatch targets the locator itself, without pointer hit testing. A click on a
    // checkbox/radio also performs the browser default (toggle + change).
    return `(() => { ${pick} else {
      const t = el;
      const { type, eventInit } = ${val};
      const constructors = [
        [/^pointer/, "PointerEvent"], [/^wheel$/, "WheelEvent"],
        [/^(click|dblclick|mouse|contextmenu|auxclick)/, "MouseEvent"],
        [/^key/, "KeyboardEvent"], [/^(focus|blur)/, "FocusEvent"],
        [/^(beforeinput|input)$/, "InputEvent"], [/^composition/, "CompositionEvent"],
        [/^(drag|drop)/, "DragEvent"], [/^touch/, "TouchEvent"],
      ];
      const className = constructors.find(([pattern]) => pattern.test(type))?.[1] || "Event";
      const EventClass = globalThis[className];
      const box = el.tagName === "INPUT" && (el.type === "checkbox" || el.type === "radio");
      if (type === "click" && box) el.checked = el.type === "radio" ? true : !el.checked;
      const ev = new EventClass(type, { bubbles: true, cancelable: true, composed: true, view: globalThis, ...eventInit });
      t.dispatchEvent(ev);
      if (type === "click" && box) {
        el.dispatchEvent(new Event("input", { bubbles: true }));
        el.dispatchEvent(new Event("change", { bubbles: true }));
      }
      globalThis.__RESULT = "OK";
    } })();`;
  }
  if (kind === "focus") {
    return `(() => { ${pick} else {
      if (typeof el.focus === "function") el.focus();
      globalThis.__RESULT = "OK";
    } })();`;
  }
  if (kind === "blur") {
    return `(() => { ${pick} else {
      if (typeof el.blur === "function") el.blur();
      globalThis.__RESULT = "OK";
    } })();`;
  }
  if (kind === "isfocused") {
    return `(() => { ${pick} else {
      globalThis.__RESULT = document.activeElement === el ? "true" : "false";
    } })();`;
  }
  if (kind === "hover") {
    // No cursor, but the app's hover handlers (MUI menus open on mouseenter/over)
    // are real JS — fire the events they listen for so the menu actually opens.
    // Dispatch on the deepest descendant (a real cursor hits the leaf) so handlers
    // bound to inner nodes (and React's delegation) see a target inside them.
    return `(() => { ${pick} else {
      let t = el; while (t.firstElementChild) t = t.firstElementChild;
      const previous = __tsNs.shimHoverTarget ?? null;
      const ancestry = node => { const path = []; while (node) { path.push(node); node = node.parentElement; } return path; };
      const oldPath = ancestry(previous), newPath = ancestry(t);
      const boundary = (prefix, Constructor) => {
        const emit = (node, type, relatedTarget, bubbles) => node.dispatchEvent(new Constructor(type, {
          bubbles, cancelable: bubbles, view: globalThis, relatedTarget,
          pointerId: 1, pointerType: "mouse", isPrimary: true,
        }));
        if (previous === t) return;
        if (previous) emit(previous, prefix + "out", t, true);
        for (const node of oldPath) if (!newPath.includes(node)) emit(node, prefix + "leave", t, false);
        emit(t, prefix + "over", previous, true);
        for (const node of newPath.slice().reverse()) if (!oldPath.includes(node)) emit(node, prefix + "enter", previous, false);
      };
      boundary("pointer", PointerEvent);
      boundary("mouse", MouseEvent);
      __tsNs.shimHoverTarget = t;
      t.dispatchEvent(new MouseEvent("mousemove", { bubbles: true, cancelable: true, view: globalThis }));
      // Also apply CSS :hover styles — a menu revealed purely by a hover descendant rule has
      // no JS handler; without this it stays display:none (no pointer state in the cascade).
      if (typeof __tsNs.tcApplyHover === "function") __tsNs.tcApplyHover(el);
      globalThis.__RESULT = "OK";
    } })();`;
  }
  // click
  return `(() => { ${pick} else {
    // A real click lands on the deepest element under the cursor and bubbles up;
    // dispatch there so handlers bound to inner nodes (e.g. a MUI Select's inner
    // role="combobox", which opens on its own mousedown) actually fire. Focus and
    // the default-action (form submit) logic stay on the matched control \`el\`.
    let tgt = el; while (tgt.firstElementChild) tgt = tgt.firstElementChild;
    const options = ${val} || {};
    const rect = el.getBoundingClientRect();
    const position = options.position || { x: rect.width / 2, y: rect.height / 2 };
    const button = options.button === "right" ? 2 : options.button === "middle" ? 1 : 0;
    const modifiers = options.modifiers || [];
    const o = { bubbles: true, cancelable: true, composed: true, view: globalThis, button,
      clientX: rect.left + position.x, clientY: rect.top + position.y,
      detail: options.clickCount || 1, shiftKey: modifiers.includes("Shift"),
      ctrlKey: modifiers.includes("Control"),
      altKey: modifiers.includes("Alt"), metaKey: modifiers.includes("Meta") };
    // Full pointer+mouse sequence like a real browser: pointer events FIRST (MUI v5+ /
    // Radix / many libs gate selection on pointerdown/up, and MUI Autocomplete/Select
    // options preventDefault mousedown to keep input focus — without the pointer pair the
    // option click never commits the value). PointerEvent if available, else MouseEvent.
    const PE = globalThis.PointerEvent || globalThis.MouseEvent;
    const pe = (t) => new PE(t, { ...o, detail: 0, buttons: t === "pointerup" ? 0 : [1, 4, 2][button], pointerId: 1, isPrimary: true });
    tgt.dispatchEvent(pe("pointerdown"));
    const md = new MouseEvent("mousedown", o);
    tgt.dispatchEvent(md);
    if (button === 2) tgt.dispatchEvent(new MouseEvent("contextmenu", o));
    // Focus follows the browser rules: a click moves focus to a FOCUSABLE target, BUT a
    // mousedown handler that calls preventDefault() suppresses the focus shift. MUI's
    // Autocomplete/Select listbox preventDefaults mousedown precisely to keep the input
    // focused while an option is clicked — focusing the option <li> (tabindex=-1) instead
    // would blur the input and clearOnBlur would discard the just-made selection.
    const focusable = el.matches && el.matches("input,button,textarea,select,a[href],[tabindex],[contenteditable]");
    if (focusable && !md.defaultPrevented && typeof el.focus === "function") el.focus();
    tgt.dispatchEvent(pe("pointerup"));
    tgt.dispatchEvent(new MouseEvent("mouseup", o));
    const clickEv = new MouseEvent(button === 0 ? "click" : "auxclick", o);
    tgt.dispatchEvent(clickEv);
    if (button === 0 && o.detail === 2) tgt.dispatchEvent(new MouseEvent("dblclick", o));
    // Report whether the app handled the click (preventDefault). If it did, the shim
    // must NOT also perform the browser default action (e.g. navigate an <a href="#">
    // whose React onClick toggles state) — that would reload and wipe the new state.
    globalThis.__RESULT = clickEv.defaultPrevented ? "PREVENTED" : "OK";
  } })();`;
}

// Pathname of an absolute URL (for detecting an in-app route change).
function pathOf(url) {
  try {
    return new URL(url).pathname;
  } catch {
    return url;
  }
}

// ---------------------------------------------------------------------------
// Frame / FrameLocator — engine iframe contexts over the shim's static path.
// A Frame carries its own `_html` (the iframe's child document) and runs the
// Locator / getBy* surface over that document, so queries scoped to the frame
// never bleed into the host page (turbo-surf#225). The host Page implements the
// same surface, so `mainFrame()` returns the page itself. srcdoc iframes expose
// their inner markup; src-loaded iframes list with empty content (live-session
// hydration populates them in the engine, but this snapshot-oriented shim does
// not fetch iframe src here — call `page.content()` after hydration if needed).
// ---------------------------------------------------------------------------
function _collectIframes(parent, selector = "iframe") {
  if (selector === "iframe" && parent._frameEntries) return parent._frameEntries;
  const html = parent._html ?? "";
  if (!html) return [];
  let matches;
  try {
    matches = JSON.parse(native.query(html, selector, "css"));
  } catch {
    return [];
  }
  const out = [];
  for (const m of matches) {
    if (m == null || m.node == null) continue;
    // The selector may be broader than "iframe" (e.g. a user passed "main") — restrict
    // to iframe elements by sniffing the serialized outer-HTML opening tag. Match
    // records don't carry a tag field, so this is the cheapest fidelity check.
    const outer = String(m.html ?? "")
      .trimStart()
      .toLowerCase();
    if (!outer.startsWith("<iframe")) continue;
    const name = native.attrOf(html, m.node, "name") ?? "";
    const id = native.attrOf(html, m.node, "id") ?? "";
    const src = native.attrOf(html, m.node, "src") ?? "";
    const srcdoc = native.attrOf(html, m.node, "srcdoc");
    let url = srcdoc != null ? "about:srcdoc" : "about:blank";
    if (srcdoc == null && src) {
      try {
        url = new URL(src, parent._url).href;
      } catch {
        url = src;
      }
    }
    out.push({
      node: m.node,
      name,
      id,
      src,
      html: srcdoc ?? "",
      url,
    });
  }
  if (parent._frameEntries) {
    const all = JSON.parse(native.query(html, "iframe", "css"));
    return out
      .map((meta) => parent._frameEntries[all.findIndex((match) => match.node === meta.node)])
      .filter(Boolean);
  }
  return out;
}

function networkWaitMatcher(matcher, baseURL) {
  if (matcher == null) return async () => true;
  if (typeof matcher === "function") return async (item) => !!(await matcher(item));
  let pattern = matcher;
  if (!(pattern instanceof RegExp)) {
    let glob = String(pattern);
    if (baseURL && !glob.startsWith("*")) {
      try {
        glob = new URL(glob, baseURL).href;
      } catch {
        /* retain invalid pattern */
      }
    }
    let source = "";
    for (let i = 0; i < glob.length; i++) {
      const char = glob[i];
      if (char === "*") {
        const deep = glob[i + 1] === "*";
        source += deep ? ".*" : "[^/]*";
        if (deep) i++;
      } else {
        source += /[\\^$+?.()|[\]{}]/.test(char) ? `\\${char}` : char;
      }
    }
    pattern = new RegExp(`^${source}$`);
  }
  return async (item) => {
    pattern.lastIndex = 0;
    return pattern.test(item.url());
  };
}

function _matchUrl(url, spec) {
  if (spec == null) return true;
  if (spec instanceof RegExp) return spec.test(url);
  if (typeof spec === "function") {
    try {
      return !!spec(new URL(url));
    } catch {
      return false;
    }
  }
  const s = String(spec);
  // A bare `*` or `**` spec means "match everything". Stripping all stars from a
  // mixed glob like `foo**` must NOT fall into that case — `url.includes("")`
  // would silently match every URL, which was the bug. Only treat
  // stripped-to-empty as match-all when the input was itself pure wildcards.
  const stripped = s.replace(/\*\*/g, "").replace(/\*/g, "");
  if (stripped.length === 0) return /^\*+$/.test(s);
  return url.includes(stripped);
}

class Frame {
  constructor(parentPage, parentFrame, meta) {
    this._parentPage = parentPage;
    this._parentFrame = parentFrame ?? null;
    this._name = meta.name ?? "";
    this._src = meta.src ?? "";
    this._url = meta.url ?? "about:blank";
    this._html = meta.html ?? "";
    this._node = meta.node;
    this._path = meta.path;
    this._frameEntries = meta.children;
    this._nativeIdentity = false;
    this._detached = false;
  }
  get _context() {
    return this._parentPage._context;
  }
  name() {
    return this._name;
  }
  url() {
    return this._url;
  }
  page() {
    return this._parentPage;
  }
  parentFrame() {
    return this._parentFrame;
  }
  childFrames() {
    return this._parentPage.frames().filter((frame) => frame._parentFrame === this);
  }
  _assertAttached() {
    if (this._detached) throw new Error("turbo-surf: frame is detached");
  }
  _realmExpression(source, helpers = false) {
    this._assertAttached();
    return `(() => {
      let child = globalThis;
      for (const index of ${JSON.stringify(this._path)}) {
        const host = child.document.querySelectorAll('iframe')[index];
        if (!host) throw new Error('turbo-surf: frame is detached');
        child = host.contentWindow;
      }
      let code = ${JSON.stringify(`return (${source});`)};
      let helperState;
      if (${helpers}) {
        helperState = __tsNs.remoteHandleFrameState(child.document);
        helperState.remoteHandleGet = __tsNs.remoteHandleGet;
        // Compile the existing surf helpers in the child so their document and
        // intrinsics belong to that realm rather than closing over the host.
        const names = ['tcGetBy', 'tcQuerySelector', 'tcResolveScoped', 'tcApplyHover'];
        let prelude = '';
        for (let index = 0; index < names.length; index++) {
          const name = names[index];
          prelude += '__G.' + name + ' = (' + __tsNs.remoteHandleFunctionSource(__tsNs[name]) + ');';
        }
        code = '"use strict"; const __G = arguments[0]; const __tsNs = __G;' + prelude + code;
      }
      return (${helpers} ? __tsNs.remoteHandleCompile(child, code) : child.Function(code))(helperState);
    })()`;
  }
  async evaluateHandle(script, arg) {
    return handleEvaluation(this, script, arg, true);
  }
  async evaluate(script, arg) {
    if (hasJSHandles(arg)) return handleEvaluation(this, script, arg);
    await this._ensureLive();
    const expression = this._realmExpression(evalSource(script, arg));
    if (this._parentPage._mergedBindings().size)
      return this._parentPage._evaluateWithBindings(expression);
    const raw = await native.liveEvalExpression(
      this._session,
      `(async () => {
      try {
        const value = await (${expression});
        return JSON.stringify({ok:true, value: (${encodeEvaluationValue.toString()})(value)});
      } catch(error) { return JSON.stringify({ok:false, error: (${encodeEvaluationError.toString()})(error)}); }
    })()`,
    );
    await this._parentPage._refreshFromSession();
    await this._parentPage._drainBindings();
    const result = JSON.parse(raw);
    if (!result.ok) throw decodeEvaluationError(result.error);
    return decodeEvaluationValue(result.value);
  }
  async content() {
    this._assertAttached();
    return this._html;
  }
  async title() {
    this._assertAttached();
    return native.title(this._html);
  }
  async innerText(selector) {
    return this.locator(selector).innerText();
  }
  async innerHTML(selector) {
    return this.locator(selector).innerHTML();
  }
  async textContent(selector) {
    return this.locator(selector).textContent();
  }
  isDetached() {
    this._parentPage._syncFrames();
    return this._detached;
  }
  // Live-interaction surface a Locator bound to this Frame expects on `_page` —
  // iframes share the parent Page's V8 isolate (one session per Page), so forward
  // to the parent rather than re-implementing session handling on Frame. Without
  // these the first non-inert Frame-locator interaction (click / waitFor / hover /
  // dispatchEvent / setInputFiles / dragTo) throws TypeError (#280).
  get _session() {
    return this._parentPage._session;
  }
  get _live() {
    return this._parentPage._live;
  }
  _ensureLive() {
    this._assertAttached();
    return this._parentPage._ensureLive();
  }
  _followIntent(intent) {
    return this._parentPage._followIntent(intent);
  }
  _readEngine(script) {
    const expression = this._realmExpression(`(() => { ${script} })()`, true);
    return JSON.parse(native.liveReadEngine(this._session, `return ${expression};`));
  }
  _evalEngine(script) {
    const expression = this._realmExpression(
      `(() => { ${script}; return globalThis.__RESULT; })()`,
      true,
    );
    return native.liveEvalEngine(this._session, `globalThis.__RESULT = ${expression};`);
  }
  async _sessionDispatch(selector, index, kind, value, strict = false) {
    if (kind !== "isfocused") this._parentPage._actionSeq++;
    const script = interactionScript(selector, index ?? 0, kind, value, strict, true);
    const result =
      kind === "type"
        ? await this._typeWithCanonicalSelection(script)
        : await this._sessionEval(script);
    if (result === "NO_MATCH") throw new Error("turbo-surf: locator matched no elements");
    return result;
  }
  async _typeWithCanonicalSelection(script) {
    this._assertAttached();
    const result = await native.liveEvalEngine(
      this._session,
      `(() => {
      'use strict';
      let child = globalThis;
      for (const index of ${JSON.stringify(this._path)}) {
        const host = child.document.querySelectorAll('iframe')[index];
        if (!host) throw new Error('turbo-surf: frame is detached');
        child = host.contentWindow;
      }
      const result = ((globalThis, document, KeyboardEvent, InputEvent, HTMLInputElement, HTMLTextAreaElement) => {
        ${script};
        return globalThis.__RESULT;
      })(child, child.document, child.KeyboardEvent, child.InputEvent, child.HTMLInputElement, child.HTMLTextAreaElement);
      globalThis.__RESULT = result;
    })();`,
    );
    await this._parentPage._refreshFromSession();
    await this._parentPage._drainBindings();
    return result;
  }
  async _sessionEval(script) {
    const result = await this._evalEngine(script);
    await this._parentPage._refreshFromSession();
    await this._parentPage._drainBindings();
    return result;
  }
  _redrain() {
    return this._parentPage._redrain();
  }
  // The locator surface mirrors Page's — the Locator reads from THIS frame's `_html`.
  _locFromSelector(selector, mode = "auto") {
    const loc = new Locator(this, (h) => JSON.parse(native.query(h, selector, mode)), {
      count: (h) => native.queryCount(h, selector, mode),
    });
    loc._selector = selector;
    return loc;
  }
  locator(selector) {
    return this._locFromSelector(selector);
  }
  getByRole(role, opts = {}) {
    const name = opts.name != null ? String(opts.name) : null;
    return new Locator(
      this,
      (h) => JSON.parse(native.getBy(h, "role", role, name, null, JSON.stringify(opts))),
      {
        getBy: { kind: "role", value: role, name, options: opts },
      },
    );
  }
  getByText(text, opts = {}) {
    return new Locator(
      this,
      (h) => JSON.parse(native.getBy(h, "text", String(text), null, null, JSON.stringify(opts))),
      {
        getBy: { kind: "text", value: String(text), name: null, options: opts },
      },
    );
  }
  getByLabel(text, opts = {}) {
    return new Locator(
      this,
      (h) => JSON.parse(native.getBy(h, "label", String(text), null, null, JSON.stringify(opts))),
      {
        getBy: { kind: "label", value: String(text), name: null, options: opts },
      },
    );
  }
  getByPlaceholder(text) {
    return this._locFromSelector(`[placeholder*=${cssAttrValue(text)}]`);
  }
  getByAltText(text) {
    return this._locFromSelector(`[alt*=${cssAttrValue(text)}]`);
  }
  getByTitle(text) {
    return this._locFromSelector(`[title*=${cssAttrValue(text)}]`);
  }
  getByTestId(id) {
    return this._locFromSelector(`[${this._context._testIdAttribute}="${id}"]`);
  }
  frameLocator(selector) {
    return new FrameLocator(this, selector);
  }
}

// FrameLocator defers iframe lookup to the moment a query actually runs, so a
// `page.setContent(...)` between construction and use is reflected.
class FrameLocator {
  constructor(parent, selector) {
    this._parent = parent; // a Page or a Frame
    this._selector = selector;
    this._index = 0;
  }
  _resolveFrame() {
    const parent = this._parent;
    const page = parent instanceof Frame ? parent._parentPage : parent;
    const matches = _collectIframes(parent, this._selector);
    const index = this._index < 0 ? matches.length + this._index : this._index;
    const entry = matches[index];
    if (!entry) throw new Error(`turbo-surf: frameLocator(${this._selector}) matched no iframe`);
    page._syncFrames();
    return page._frames.find(
      (frame) => frame._parentFrame === parent && frame._node === entry.node,
    );
  }

  _withIndex(i) {
    const fl = new FrameLocator(this._parent, this._selector);
    fl._index = i;
    return fl;
  }
  first() {
    return this._withIndex(0);
  }
  last() {
    return this._withIndex(-1);
  }
  nth(i) {
    return this._withIndex(i);
  }
  locator(selector) {
    return this._resolveFrame().locator(selector);
  }
  getByRole(role, opts) {
    return this._resolveFrame().getByRole(role, opts);
  }
  getByText(t, opts) {
    return this._resolveFrame().getByText(t, opts);
  }
  getByLabel(t, opts) {
    return this._resolveFrame().getByLabel(t, opts);
  }
  getByPlaceholder(t) {
    return this._resolveFrame().getByPlaceholder(t);
  }
  getByAltText(t) {
    return this._resolveFrame().getByAltText(t);
  }
  getByTitle(t) {
    return this._resolveFrame().getByTitle(t);
  }
  getByTestId(id) {
    return this._resolveFrame().getByTestId(id);
  }
  frameLocator(selector) {
    return new FrameLocator(this._resolveFrame(), selector);
  }
  owner() {
    return this._parent.locator(this._selector).nth(this._index);
  }
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------
const NETWORK_EVENTS = [
  "response",
  "request",
  "requestfinished",
  "requestfailed",
  "pageerror",
  "console",
];
class Page {
  constructor(context) {
    this._html = "";
    this._url = "about:blank";
    this._documentResponse = { status: 0, referrer: "", headers: [] };
    this._ownsContext = context == null;
    this._context = context ?? new BrowserContext();
    this._viewport = { ...this._context._viewport };
    this._requestState = native.requestStateNew?.(this._context._requestState);
    this._history = [];
    this._fwd = [];
    this._historyCurrent = null;
    this._listeners = new Map();
    this._closed = false;
    this._session = null; // live JS session id (networkidle navigations), else null
    this._loadedPath = null; // path the live DOM was loaded at (to detect in-app nav)
    this._navHops = 0; // auto-followed redirects since the last user navigation
    this._autoHydrate = true; // hydrate-on-demand unless a goto opted out (commit)
    this._actionSeq = 0; // bumps per real user interaction; tags drained responses so
    // waitForResponse can tell a fresh response from one that drained in an earlier step
  }

  get _live() {
    return this._session != null;
  }

  // Hydrate-on-demand: open a live session if one isn't up (and the last navigation
  // didn't opt out via waitUntil:'commit'). Idempotent — at most one hydration per doc.
  async _ensureLive() {
    if (this._closed) throw new Error("page is closed");
    if (!this._live && this._autoHydrate !== false) await this._openLiveSession();
  }

  // Init scripts use the native trusted entry point before authored scripts;
  // inserting a script tag would change parser locations and subject it to CSP.
  _initScriptSource() {
    const all = [...(this._context._initScripts ?? []), ...(this._initScripts ?? [])];
    return all.map((source) => `try{ ${source} }catch(e){}`).join("\n");
  }

  _engineInitScriptSource() {
    return (
      buildExposeInitScript([...this._mergedBindings().keys()], this._routeBindingName) +
      "\n" +
      (this._routeInitScript ?? "")
    );
  }
  async _installNativeBindings() {
    if (!this._live) return;
    const engine = this._engineInitScriptSource();
    await native.liveEvalEngine(
      this._session,
      `"use strict"; __tsNs.shimBindingScripts(${JSON.stringify(engine)}, ${JSON.stringify(this._initScriptSource())}, "");
${engine}`,
    );
  }

  // Merge context-level and page-level exposed bindings; page wins on name clash.
  _mergedBindings() {
    const out = new Map();
    for (const [k, v] of this._context._bindings ?? []) out.set(k, v);
    for (const [k, v] of this._bindings ?? []) out.set(k, v);
    if (this._routeHandlers?.size)
      out.set(this._routeBindingName, {
        fn: (source, data) => this._invokeHostRoute(source, data),
        isBinding: true,
      });
    return out;
  }

  _collectSession(id, script, collect = false) {
    if (!this._contentCollecting) return native.liveEvalEngine(id, script, collect);
    return native.liveEvalEngineExpression(
      id,
      `(async () => { ${script}
return String(globalThis.__RESULT ?? ""); })()`,
    );
  }
  _routeRequestController(frame, id) {
    this._routeRequests ??= new WeakMap();
    let requests = this._routeRequests.get(frame);
    if (!requests) this._routeRequests.set(frame, (requests = new Map()));
    if (!requests.has(id)) requests.set(id, new AbortController());
    return { requests, controller: requests.get(id) };
  }
  async _invokeHostRoute(source, data) {
    const { requests, controller } = this._routeRequestController(
      source.frame,
      data.request?.requestId ?? data.cancel ?? data.complete,
    );
    if (data.cancel !== undefined) {
      controller.abort(new Error("Route's request has been aborted"));
      return {};
    }
    if (data.complete !== undefined) {
      requests.delete(data.complete);
      return {};
    }
    this._routeLifetime ??= new AbortController();
    const signal = AbortSignal.any([this._routeLifetime.signal, controller.signal]);
    if (signal.aborted) throw signal.reason;
    const entry = this._routeHandlers.get(data.id);
    if (!entry) throw new Error("Route registration is no longer available");
    const document = source.frame === this ? "document" : source.frame._realmExpression("document");
    const allowed = await native.liveEvalEngineExpression(
      this._session,
      `String(__tsNs.shimAuthorizeRoute(${document}, ${JSON.stringify(data.request.url)}))`,
    );
    if (allowed !== "true") throw new TypeError("Failed to fetch");
    if (!matchesHostRoutePredicate(entry, data.matchUrl ?? data.request.url))
      return { action: "fallback" };
    if (!data.request.overriddenHeaders)
      data = {
        ...data,
        request: {
          ...data.request,
          headers: {
            ...Object.fromEntries(
              Object.entries(this._context._headers).map(([k, v]) => [k.toLowerCase(), v]),
            ),
            ...data.request.headers,
          },
        },
      };
    const session = this._session;
    const createRequest = (request) =>
      makeCapturedRequest(
        {
          url: request.url,
          method: request.method,
          request,
          context: { resourceType: request.resourceType },
        },
        source.frame,
      );
    const fetchResponse = async (request, options) => {
      if (signal.aborted) throw signal.reason;
      if (this._closed || session !== this._session)
        throw new Error("Route's page has been closed or navigated");
      const expression = `(async () => {
        const request = ${JSON.stringify(request)};
        const window = ${source.frame === this ? "globalThis" : source.frame._realmExpression("globalThis")};
        const realm = __tsNs.shimBindingNamespace(window);
        const state = realm.shimRouteState;
        const response = await realm.shimRouteFetch(request.url, {
          ...request, signal:state.requests.get(request.requestId)?.signal, body:request.bodyBytes === null ? undefined : new Uint8Array(request.bodyBytes),
        });
        return JSON.stringify({url:response.url,status:response.status,statusText:response.statusText,
          headers:Array.from(response.headers),bodyBytes:Array.from(new Uint8Array(await response.arrayBuffer()))});
      })()`;
      if (
        Object.keys(options).some((key) => !["url", "method", "postData", "headers"].includes(key))
      )
        throw new Error("Unsupported route.fetch option");
      const raw = await this._evaluateWithBindings(expression);
      if (this._closed || session !== this._session)
        throw new Error("Route's page has been closed or navigated");
      return routeResponse(JSON.parse(raw));
    };
    const decision = await invokeHostRoute(
      entry.handler,
      data,
      createRequest,
      fetchResponse,
      signal,
    );
    if (decision.fulfilled && !signal.aborted && session === this._session && !this._closed) {
      const value = decision.fulfilled;
      const status = value.status ?? 200;
      const body = [204, 205, 304].includes(status)
        ? Buffer.alloc(0)
        : Buffer.from(value.bodyBytes ?? []);
      native.liveNetworkLogPublishIntercepted(
        session,
        JSON.stringify({
          url: data.request.url,
          method: data.request.method,
          status,
          statusText: "",
          ok: status >= 200 && status < 300,
          headers: Object.entries(value.headers ?? {}),
          request: data.request,
          context: {
            resourceType: data.request.resourceType,
            frame: { path: source.frame === this ? [] : source.frame._path },
          },
        }),
        body,
      );
    }
    return decision;
  }

  async _takeBindingCalls() {
    const raw = await this._collectSession(
      this._session,
      `"use strict"; (() => {
          const out = [];
          const seen = new Set();
          const walk = (w, framePath = []) => {
            if (!w || seen.has(w)) return;
            seen.add(w);
            try {
              const bag = __tsNs.shimBindingBag(w);
              if (bag && bag.q && bag.q.length) {
                const queue=bag.q; bag.q={__proto__:null,length:0};
                for(let i=0;i<queue.length;i++) out.push({...queue[i], framePath, nonce:bag.nonce});
              }
            } catch (_e) {}
            let n = 0;
            try { n = w.frames ? w.frames.length : 0; } catch (_e) { n = 0; }
            for (let i = 0; i < n; i++) {
              let child = null;
              try { child = w.frames[i]; } catch (_e) { child = null; }
              if (child) walk(child, [...framePath, i]);
            }
          };
          walk(globalThis);
          globalThis.__RESULT = JSON.stringify(out);
        })();`,
      true,
    );
    if (!raw) return [];
    let calls;
    try {
      calls = JSON.parse(raw);
    } catch {
      return [];
    }
    return calls;
  }
  async _invokeBindingCall(call, entry) {
    const args = Array.isArray(call.args) ? call.args : [];
    if (!entry.isBinding) return entry.fn(...args);
    const path = call.framePath ?? [];
    if (path.length && !this._requestFrames?.has(call.nonce)) await this._refreshFromSession(true);
    this._syncFrames();
    const frame = path.length === 0 ? this.mainFrame() : this._requestFrames?.get(call.nonce);
    if (!frame || frame._detached || (path.length && frame._requestContextId !== call.nonce))
      throw new Error("Exposed binding caller frame is detached");
    call.originFrame = frame;
    return entry.fn({ page: this, context: this._context, frame }, ...args);
  }
  async _settleBindingCall(call, entry) {
    const session = this._session;
    let payload;
    try {
      const value = await this._invokeBindingCall(call, entry);
      payload = JSON.stringify({ value });
    } catch (error) {
      payload = JSON.stringify({
        error: { name: String(error?.name ?? "Error"), message: String(error?.message ?? error) },
      });
    }
    if (session !== this._session || this._closed) return;
    const path = call.originFrame?._path ?? call.framePath ?? [];
    if (call.originFrame?._detached) return;
    await this._collectSession(
      session,
      `"use strict"; (() => {
      let realm = globalThis;
      for (const index of ${JSON.stringify(path)}) realm = realm.frames[index];
      const bag = __tsNs.shimBindingBag(realm), pending = bag?.pending?.[${call.id}];
      if (!pending || bag.nonce !== ${JSON.stringify(call.nonce)}) return;
      delete bag.pending[${call.id}];
      const result = ${payload};
      if (result.error) {
        pending.reject(bag.error(result.error.message,result.error.name));
      } else pending.resolve(result.value);
    })()`,
      true,
    );
  }
  async _drainBindings(deadline = Infinity, cancelOnTimeout = true, collect = false) {
    if (this._session == null || this._draining) return;
    const handlers = this._mergedBindings();
    if (handlers.size === 0) return;
    this._draining = true;
    let changed = false;
    try {
      const pending = new Set();
      let rounds = 0;
      let advanced = false;
      let failure;
      while (this._session != null) {
        if (Date.now() >= deadline) {
          const error = new Error("Timeout exceeded while waiting for host bindings");
          error.name = "TimeoutError";
          if (cancelOnTimeout) {
            this._routeLifetime?.abort(error);
            this._routeLifetime = undefined;
          }
          throw error;
        }
        const calls = await this._takeBindingCalls();
        if (this._session == null || this._closed) break;
        if (calls.length && ++rounds > 100)
          throw new Error("Exposed binding call budget exhausted");
        for (const call of calls) {
          const entry = handlers.get(call.name);
          if (!entry) continue;
          changed = true;
          const task = this._settleBindingCall(call, entry)
            .catch((error) => {
              failure ??= error;
            })
            .finally(() => pending.delete(task));
          pending.add(task);
        }
        if (calls.length) advanced = false;
        if (!pending.size && !calls.length) {
          if (changed && !advanced && !this._contentCollecting) {
            advanced = true;
            // Observers advance one turn; actions retain full settlement.
            await this._collectSession(this._session, "globalThis.__RESULT = '';", collect);
            continue;
          }
          break;
        }
        // Keep servicing calls while an async host callback awaits another page operation.
        if (pending.size)
          await Promise.race([...pending, new Promise((resolve) => setTimeout(resolve, 1))]);
        if (failure) throw failure;
      }
      if (changed) await this._refreshFromSession(true);
    } finally {
      this._draining = false;
    }
  }
  async _evaluateWithBindings(body) {
    const id = (this._bindingEvalId = (this._bindingEvalId ?? 0) + 1);
    await native.liveEvalEngine(
      this._session,
      `(() => {
      const bag = __tsNs.shimBindings;
      bag.evaluations[${id}] = null;
      Promise.resolve().then(() => (${body})).then(
        value => {
          try { bag.evaluations[${id}] = JSON.stringify({value: (${encodeEvaluationValue.toString()})(value)}); }
          catch (error) { bag.evaluations[${id}] = JSON.stringify({error: (${encodeEvaluationError.toString()})(error)}); }
        },
        error => {bag.evaluations[${id}] = JSON.stringify({error: (${encodeEvaluationError.toString()})(error)});});
    })()`,
    );
    const deadline = Date.now() + 5000;
    try {
      while (Date.now() < deadline) {
        if (this._session == null || this._closed)
          throw new Error("Page has been closed or navigated");
        await this._drainBindings(deadline);
        if (this._session == null || this._closed)
          throw new Error("Page has been closed or navigated");
        const raw = await native.liveEvalEngine(
          this._session,
          `globalThis.__RESULT = __tsNs.shimBindings.evaluations[${id}] ?? '';`,
        );
        if (raw) {
          const result = JSON.parse(raw);
          if (result.error) throw decodeEvaluationError(result.error);
          await this._refreshFromSession(true);
          return decodeEvaluationValue(result.value);
        }
        await this._redrain();
        await new Promise((resolve) => setTimeout(resolve, 1));
      }
      throw new Error("Evaluation waiting for exposed binding timed out");
    } finally {
      if (this._session != null)
        await native.liveEvalEngine(
          this._session,
          `delete __tsNs.shimBindings?.evaluations?.[${id}];`,
        );
    }
  }

  _openLiveSession(timeoutMs) {
    if (this._closed) throw new Error("Page session closed");
    if (this._openingSession) return this._openingSession.promise;
    if (this._session != null) this._closeSession();
    const opening = { id: native.liveNetworkLogCreate() };
    this._openingSession = opening;
    this._openingNetworkId = opening.id;
    this._updateNetworkPump();
    opening.promise = this._startLiveSession(timeoutMs, opening);
    return opening.promise;
  }
  async _startLiveSession(timeoutMs, opening) {
    const started = Date.now();
    try {
      const session = await native.liveOpen(
        this._html,
        this._url,
        this._cookies,
        this._context._userAgent ?? undefined,
        this._requestState,
        JSON.stringify({
          ...this._documentResponse,
          hostNetworkLog: opening.id,
          viewport: [this._viewport.width, this._viewport.height],
        }),
        this._initScriptSource(),
        timeoutMs,
        this._context._maxResponseBytes,
        this._engineInitScriptSource(),
        // Browser editing must not expire future timers between consecutive keys.
        true,
      );
      if (this._closed || this._openingSession !== opening) {
        native.liveClose(session);
        throw new Error("Page session closed");
      }
      this._session = session;
      this._hydrated = true;
      this._loadedPath = pathOf(this._url);
      await this._refreshFromSession(true);
      await this._drainBindings(timeoutMs > 0 ? started + timeoutMs : Infinity);
      if (timeoutMs > 0 && Date.now() - started >= timeoutMs) {
        this._closeSession();
        throw new Error(`navigation timeout after ${timeoutMs}ms`);
      }
    } catch (error) {
      if (/navigation timeout after/.test(error.message)) error.name = "TimeoutError";
      if (this._session === opening.id) this._closeSession();
      else native.liveClose(opening.id);
      if (this._openingSession === opening) this._failPendingNetworkResponses(error.message);
      throw error;
    } finally {
      if (this._openingSession === opening) {
        this._openingSession = null;
        this._openingNetworkId = null;
      }
      this._updateNetworkPump();
    }
  }

  // Pull the current live DOM + URL + cookies back into the Page so reads (which run
  // over `this._html`) and a subsequent navigation see the post-interaction state.
  async _refreshFromSession(collect = false) {
    if (this._session == null) return;
    const collectScript = (id, script) => this._collectSession(id, script, collect);
    const previousUrl = this._url;
    let historyNavigation = null;
    let rscNavigation = false;
    try {
      const encoded = await collectScript(
        this._session,
        "globalThis.__RESULT = __tsNs.takeHistoryNavigation();",
      );
      historyNavigation = JSON.parse(encoded);
    } catch {
      /* A session terminated during navigation retains its last driver state. */
    }
    if (historyNavigation) syncHistory(this, historyNavigation);
    this._html = native.liveSerialize(this._session);
    try {
      const href = await collectScript(
        this._session,
        "globalThis.__RESULT = __tsNs.requestFrameSnapshot?.() || '';",
      );
      if (href) {
        const current = JSON.parse(href);
        this._url = historyNavigation?.url ?? current.url;
        this._requestContextId = current.id;
        this._requestFrames ??= new Map();
        if (current.id) this._requestFrames.set(current.id, this.mainFrame());
      }
    } catch {
      /* keep prior url */
    }
    // Next App Router client navigation (router.push) doesn't change location.href in our
    // engine — it fetches the target's RSC flight, which the runtime records on __rscNav.
    // Treat that as the navigation target (the post-login redirect chain rides this).
    try {
      const nav = await collectScript(
        this._session,
        "globalThis.__RESULT = __tsNs.rscNav || ''; __tsNs.rscNav = '';",
      );
      if (nav && pathOf(nav) !== this._loadedPath) {
        const target = this._resolveUrl(nav);
        if (target !== historyNavigation?.url) {
          this._url = target;
          rscNavigation = true;
        }
      }
    } catch {
      /* no pending rsc nav */
    }
    try {
      this._cookies = native.liveCookies(this._session);
    } catch {
      /* keep prior cookies */
    }
    if (!historyNavigation && previousUrl !== this._url) this._emit("framenavigated", this);
    if (this._html.includes("<iframe")) {
      await this._refreshRequestFrames(collect);
    } else this._frameEntries = undefined;
    this._syncFrames();
    await this._drainNetworkEvents();
    const admitted = historyNavigation?.crossDocument;
    const needsDocument = admitted || rscNavigation;
    if (this._live && /^https?:/.test(this._url) && needsDocument && this._navHops < 10) {
      this._navHops++;
      if (admitted && !admitted.replace) {
        this._history.push(this._historyCurrent ?? previousUrl);
        this._fwd = [];
      }
      await this._reopen(admitted?.url ?? this._url);
    }
  }

  async _refreshRequestFrames(collect = false) {
    const session = this._session;
    const frames = await this._collectSession(
      session,
      `(() => {
        const ids = __tsNs.shimFrameIds ??= new WeakMap();
        __tsNs.shimFrameNext ??= 1;
        const walk = (doc, path) => Array.from(doc.querySelectorAll('iframe'), (el, index) => {
          if (!ids.has(el)) ids.set(el, __tsNs.shimFrameNext++);
          const entry = { node: ids.get(el), contextId: __tsNs.requestContextId?.(el.contentWindow), name: el.getAttribute('name') || '', id: el.id || '', src: el.getAttribute('src') || '', html: el.getAttribute('srcdoc') || '', url: el.src || 'about:blank', path: [...path,index], children: [] };
          try { entry.url = el.contentWindow.location.href; if (el.contentDocument) { entry.html = el.contentDocument.documentElement?.outerHTML || ""; entry.live = true; entry.children = walk(el.contentDocument, entry.path); } } catch (_) {}
          return entry;
        });
        globalThis.__RESULT = JSON.stringify({contextId:__tsNs.requestContextId?.(globalThis),frames:walk(document, [])});
      })();`,
      collect,
    );
    if (this._session !== session) throw new Error("Frame execution context was destroyed");
    const metadata = JSON.parse(frames);
    this._frameEntries = metadata.frames;
    this._requestContextId = metadata.contextId;
    if (metadata.contextId)
      (this._requestFrames ??= new Map()).set(metadata.contextId, this.mainFrame());
  }

  // Re-pump the live app and pull its DOM forward. Backs web-first assertion retry:
  // a UI change that lands just after an action (a modal close, an async-loaded row)
  // becomes observable on the next poll. No-op without a live session.
  async _redrain() {
    if (this._session == null) return;
    try {
      await this._collectSession(this._session, "globalThis.__RESULT = '';");
    } catch {
      /* terminated mid-drain — refresh reads best-effort state */
    }
    await this._refreshFromSession();
    await this._drainBindings();
  }

  // Drain the live app's network log and emit Playwright `response` events. Tests
  // subscribe with `page.on('response', …)` to capture API payloads (the payroll
  // period, employments lists). Each entry → a minimal Response (url/status/json/text).
  _updateNetworkPump() {
    const session = this._session ?? this._openingNetworkId;
    const needed =
      session != null &&
      (this._openingSession ||
        this._responseBodies?.size ||
        NETWORK_EVENTS.some((event) => this._listeners.get(event)?.length));
    if (needed && this._networkPump?.session === session && this._networkPump.active) return;
    if (this._networkPump) {
      this._networkPump.active = false;
      try {
        native.liveNetworkLogWake(this._networkPump.session);
      } catch {}
      this._networkPump = null;
    }
    if (!needed) return;
    const pump = { session, active: true, wake: "0" };
    this._networkPump = pump;
    this._pumpNetworkEvents(pump).catch(() => {});
  }
  async _pumpNetworkEvents(pump) {
    try {
      while (pump.active) {
        const signal = JSON.parse(await native.liveNetworkLogWait(pump.session, pump.wake));
        if (!pump.active) break;
        if (signal.closed) {
          this._closeSession();
          break;
        }
        pump.wake = signal.wake;
        await this._drainNetworkEvents();
      }
    } catch (error) {
      if (pump.active) this._failPendingNetworkResponses(error.message);
    } finally {
      if (this._networkPump === pump) this._networkPump = null;
    }
  }
  async _drainNetworkEvents() {
    if (this._networkDrain) return this._networkDrain;
    this._networkDrain = this._readNetworkEvents();
    try {
      await this._networkDrain;
    } finally {
      this._networkDrain = null;
    }
  }
  async _readNetworkEvents() {
    const session = this._session ?? this._openingNetworkId;
    if (session == null) return;
    let raw;
    try {
      raw = await native.liveNetworkLog(session);
    } catch {
      return;
    }
    if (!raw || (this._session ?? this._openingNetworkId) !== session) return;
    let entries, revision;
    try {
      const batch = JSON.parse(raw);
      entries = Array.isArray(batch) ? batch : batch.entries;
      revision = batch.revision;
    } catch {
      return;
    }
    this._recentResponses = this._recentResponses ?? [];
    let previousHop = null;
    const discardedConsoleHandles = [];
    for (const e of entries) {
      if ((this._session ?? this._openingNetworkId) !== session) break;
      if (e.event === "console") {
        if (this._listeners.get("console")?.length) this._emit("console", consoleMessage(this, e));
        else discardedConsoleHandles.push(...e.args.map((argument) => argument.id));
        continue;
      }
      if (e.event === "networkoverflow") {
        this._failPendingNetworkResponses(e.message);
        this._emit("pageerror", new Error(e.message));
        continue;
      }
      if (e.event === "responsebody") {
        this._finishNetworkResponse(e);
        continue;
      }
      if (e.event === "pageerror") {
        const error = new Error(e.message);
        error.name = e.name;
        if (e.stack) error.stack = e.stack;
        this._emit("pageerror", error);
        continue;
      }
      const { response: resp, lifecycle } = this._captureNetworkResponse(e);
      if (
        e.redirectHop > 0 &&
        previousHop &&
        previousHop.status() >= 300 &&
        previousHop.status() < 400
      ) {
        requestRedirects.get(resp.request()).from = previousHop.request();
        requestRedirects.get(previousHop.request()).to = resp.request();
      }
      previousHop = resp;
      resp._seq = this._actionSeq; // which interaction produced it (see waitForResponse)
      this._emit("request", resp.request());
      if (e.status === 0) {
        this._emit("requestfailed", resp.request());
        continue;
      }
      this._recentResponses.push(resp);
      this._emit("response", resp);
      if (!lifecycle) this._emit("requestfinished", resp.request());
    }
    if (this._recentResponses.length > 100)
      this._recentResponses.splice(0, this._recentResponses.length - 100);
    if (discardedConsoleHandles.length && (this._session ?? this._openingNetworkId) === session)
      native.liveNetworkLogDiscardHandles(session, JSON.stringify(discardedConsoleHandles));
    if (revision != null && (this._session ?? this._openingNetworkId) === session)
      native.liveNetworkLogAcknowledge(session, revision);
  }

  _failPendingNetworkResponses(message) {
    for (const pending of this._responseBodies?.values() ?? []) {
      pending.entry.error = message;
      pending.reject(new Error(message));
      this._emit("requestfailed", pending.response.request());
    }
    this._responseBodies?.clear();
    this._networkResponses?.clear();
    this._updateNetworkPump();
  }
  _finishNetworkResponse(entry) {
    const pending = this._responseBodies?.get(entry.id);
    if (!pending) return;
    for (const name of ["body", "bodyBytes", "bodyUnavailable", "bodyIncomplete", "error"])
      if (Object.hasOwn(entry, name)) pending.entry[name] = entry[name];
    if (entry.error || entry.bodyIncomplete) {
      pending.reject(new Error(entry.error || "Response body incomplete"));
      this._emit("requestfailed", pending.response.request());
    } else {
      pending.resolve();
      this._emit("requestfinished", pending.response.request());
    }
    this._responseBodies.delete(entry.id);
    this._networkResponses?.delete(entry.id);
    this._updateNetworkPump();
  }
  _captureNetworkResponse(entry) {
    let lifecycle;
    if (entry.bodyPending) {
      lifecycle = { entry };
      lifecycle.promise = new Promise((resolve, reject) =>
        Object.assign(lifecycle, { resolve, reject }),
      );
      lifecycle.promise.catch(() => {});
    }
    const response = makeNetResponse(entry, this._requestFrame(entry), lifecycle);
    if (!lifecycle) return { response };
    lifecycle.response = response;
    (this._responseBodies ??= new Map()).set(entry.id, lifecycle);
    (this._networkResponses ??= new Map()).set(entry.id, response);
    const previous = this._networkResponses.get(entry.redirectFrom);
    if (previous) {
      requestRedirects.get(response.request()).from = previous.request();
      requestRedirects.get(previous.request()).to = response.request();
      this._networkResponses.delete(entry.redirectFrom);
    }
    this._updateNetworkPump();
    return { response, lifecycle };
  }

  _requestFrame(entry) {
    const context = entry.context;
    if (!context) return this.mainFrame();
    const metadata = context.frame;
    const path = metadata?.path;
    if (path?.length === 0) return this.mainFrame();
    if (!context.navigation && metadata?.contextId) {
      const known = this._requestFrames?.get(metadata.contextId);
      if (known) return known;
    }
    if (context.navigation && !path) return null;
    if (path)
      return (
        this._frames?.find((frame) => JSON.stringify(frame._path) === JSON.stringify(path)) ?? null
      );
    return this._requestFrames?.get(metadata?.contextId) ?? null;
  }

  // Follow an in-app navigation: load `url` fresh (fetch + hydrate) WITHOUT resetting the
  // redirect-hop budget or pushing history (it's the app navigating, not the user).
  async _reopen(url) {
    await this._navigate(this._resolveUrl(url));
    await this._openLiveSession();
  }

  _closeSession() {
    this._routeLifetime?.abort(new Error("Route's page has been closed or navigated"));
    this._routeLifetime = undefined;
    this._routeRequests = undefined;
    if (this._routeHandlers)
      this._routeHandlers = new Map((this._routes ?? []).map((route) => [route.id, route]));
    if (this._openingNetworkId != null) native.liveClose(this._openingNetworkId);
    this._openingNetworkId = null;
    this._openingSession = null;
    this._failPendingNetworkResponses("Page session closed");
    for (const frame of (this._frames ?? []).toReversed()) {
      frame._detached = true;
      this._emit("framedetached", frame);
    }
    this._frames = [];
    this._frameEntries = undefined;
    this._requestFrames = undefined;
    this._requestContextId = undefined;
    if (this._session != null) {
      try {
        native.liveClose(this._session);
      } catch {
        /* already gone */
      }
      this._session = null;
    }
    this._updateNetworkPump();
  }

  // Drive an interaction in the live isolate (real DOM events → running app handlers →
  // re-render), then refresh the Page snapshot. `kind` is "click" | "fill".
  _readEngine(script) {
    return JSON.parse(native.liveReadEngine(this._session, script));
  }
  _evalEngine(script) {
    return native.liveEvalEngine(this._session, script);
  }
  async _sessionDispatch(selector, index, kind, value, strict = false) {
    // A new user interaction: responses produced by THIS action (and after) are "fresh"
    // for any waitForResponse paired with it; responses from before are stale. The
    // "isfocused" probe is a read, not an interaction — don't tag responses off it.
    if (kind !== "isfocused") this._actionSeq++;
    const r = await native.liveEvalEngine(
      this._session,
      interactionScript(selector, index ?? 0, kind, value, strict),
    );
    await this._refreshFromSession();
    // Surface the NO_MATCH signal before draining so a binding-handler
    // rejection that happens to be in the queue can't shadow the original
    // locator failure. Drain is best-effort; a throwing binding writes to
    // console (same shape as _sessionEval → drain errors).
    if (r === "NO_MATCH") throw new Error("turbo-surf: locator matched no elements");
    // Drain exposed-binding calls queued by handlers that ran during this
    // interaction so a subsequent assert sees them without waiting for the next
    // evaluate — matches `_sessionEval`'s contract.
    try {
      await this._drainBindings();
    } catch (e) {
      console.error("turbo-surf: _drainBindings threw during _sessionDispatch:", e);
    }
    return r;
  }
  // Run arbitrary JS in the live isolate, then refresh the Page snapshot. Backs the
  // keyboard (key-event dispatch) and other live-only side effects.
  async _sessionEval(script) {
    if (this._session == null) return undefined;
    const r = await native.liveEvalEngine(this._session, script);
    await this._refreshFromSession();
    await this._drainBindings();
    return r;
  }
  get _cookies() {
    return this._context._cookies;
  }
  set _cookies(v) {
    this._context._cookies = v;
  }

  _resolveUrl(url) {
    const base = this._context._baseURL;
    return base ? new URL(url, base).href : url;
  }

  _navigationReferrerPolicy() {
    const valid = new Set([
      "no-referrer",
      "no-referrer-when-downgrade",
      "same-origin",
      "origin",
      "strict-origin",
      "origin-when-cross-origin",
      "strict-origin-when-cross-origin",
      "unsafe-url",
    ]);
    let policy = "strict-origin-when-cross-origin";
    for (const [name, value] of this._documentResponse?.headers || [])
      if (name.toLowerCase() === "referrer-policy")
        for (const token of String(value).split(","))
          if (valid.has(token.trim())) policy = token.trim();
    return policy;
  }

  async _navigate(url, method, body, opts) {
    if (this._closed) throw new Error("page is closed");
    // `about:blank` (and other about: URLs) are not network resources — a browser shows an
    // empty document with NO request. Short-circuit so a goto/reload of about:blank yields a
    // blank page instead of a "builder error for url (about:blank)" from the net layer.
    if (/^about:/i.test(url)) {
      this._imageCache = undefined;
      closeScreenshotScene(this);
      this._closeSession();
      this._html = "<html><head></head><body></body></html>";
      this._url = url;
      this._documentResponse = { status: 0, referrer: "", headers: [] };
      this._hydrated = false;
      this._syncFrames();
      this._emit("framenavigated", this);
      this._emit("load");
      this._emit("domcontentloaded");
      return makeResponse({ status: 200, finalUrl: url, html: this._html });
    }
    let raw;
    try {
      raw = await native.fetchWithCookies(
        url,
        this._cookies,
        method ?? null,
        body ?? null,
        this._context._headersJson(),
        false,
        this._requestState,
        JSON.stringify({
          source: Object.hasOwn(opts ?? {}, "referer")
            ? opts.referer
            : /^https?:/.test(this._url)
              ? this._url
              : null,
          destination: "document",
          referrerPolicy: this._navigationReferrerPolicy(),
        }),
        null,
        null,
        null,
        opts?.timeout ?? this._navigationTimeout ?? this._context._navigationTimeout ?? 30000,
        this._context._maxResponseBytes,
      );
    } catch (error) {
      if (/navigation timeout after/.test(error.message)) error.name = "TimeoutError";
      throw error;
    }
    const r = JSON.parse(raw);
    const response = makeResponse(r);
    for (const hop of r.requests ?? []) {
      const entry = {
        ...hop,
        context: { navigation: true, resourceType: "document", frame: { path: [] } },
      };
      const request = makeCapturedRequest(entry, this.mainFrame(), () =>
        hop.url === r.finalUrl ? response : null,
      );
      if (hop.url === r.finalUrl) response.request = () => request;
      this._emit("request", request);
    }
    const attachment = attachmentRecord(r);
    if (attachment) {
      if (r.cookies !== this._cookies) this._context._setCookies(r.cookies);
      this._recordDownload(attachment);
      throw new DownloadNavigationError();
    }
    if (r.status === 204 || r.status === 205) {
      this._cookies = r.cookies;
      throw new Error(`page.goto: net::ERR_ABORTED at ${url}`);
    }
    this._imageCache = undefined;
    closeScreenshotScene(this);
    this._closeSession(); // only a committed response replaces the active document
    this._html = r.html;
    this._documentResponse = {
      status: r.status,
      referrer: r.referrer || "",
      headers: r.headers || [],
    };
    this._url = r.finalUrl;
    this._cookies = r.cookies;
    this._hydrated = false; // fresh document — not yet run its own JS
    this._syncFrames();
    this._emit("framenavigated", this);
    this._emit("load");
    this._emit("domcontentloaded");
    return response;
  }
  _remainingNavigationTimeout(opts, started) {
    const timeout =
      opts?.timeout ?? this._navigationTimeout ?? this._context._navigationTimeout ?? 30000;
    if (timeout === 0) return undefined;
    const remaining = timeout - (Date.now() - started);
    if (remaining > 0) return remaining;
    const error = new Error(`navigation timeout after ${timeout}ms`);
    error.name = "TimeoutError";
    throw error;
  }
  async goto(url, opts) {
    const started = Date.now();
    const previous = this._url;
    this._navHops = 0; // user navigation — reset the redirect-follow budget
    this._autoHydrate = opts?.waitUntil !== "commit"; // 'commit' = inspect raw shell
    const resp = await this._navigate(this._resolveUrl(url), undefined, undefined, {
      ...opts,
      referer: opts?.referer ?? null,
    });
    if (previous !== "about:blank") this._history.push(this._historyCurrent ?? previous);
    this._historyCurrent = null;
    this._fwd = []; // a committed navigation invalidates the forward stack
    // `waitUntil:'networkidle'` (or a later waitForLoadState('networkidle')) opens a LIVE
    // session: hydrate the app to quiescence so an SPA's client-rendered content (forms,
    // dashboards) appears, and keep it mounted so clicks/fills dispatch real events into
    // the running app. Default/'load'/'commit' stay raw (cheap fetch) — hydrating EVERY
    // goto is both slow and unnecessary for static pages.
    if (opts?.waitUntil === "networkidle")
      await this._openLiveSession(this._remainingNavigationTimeout(opts, started));
    // If a `waitForResponse` / `waitForRequest` is already subscribed when the
    // caller does `Promise.all([waitForResponse(...), goto(url)])` without
    // `waitUntil:'networkidle'`, the inline-script fetches that the predicate
    // wants to observe only run inside a live session. Auto-hydrate so the
    // wait short-circuits on a real response instead of timing out with
    // the synthetic 200 fallback. Harmless when nothing is waiting.
    else if (
      !this._live &&
      this._autoHydrate !== false &&
      (this._listeners.has("pageerror") ||
        this._listeners.has("console") ||
        this._listeners.has("response") ||
        this._listeners.has("request") ||
        this._listeners.has("requestfinished") ||
        this._listeners.has("requestfailed"))
    ) {
      await this._openLiveSession(this._remainingNavigationTimeout(opts, started));
    }
    return resp;
  }
  async reload(opts) {
    const started = Date.now();
    // Re-fetch in place — no history entry. Like goto, a `waitUntil:'networkidle'` reload
    // re-opens a LIVE session so a client-rendered SPA re-hydrates (its forms/dashboards
    // re-appear); without this the reloaded doc stays the raw un-hydrated shell and reads
    // (e.g. a settings select's value) see an empty DOM.
    this._autoHydrate = opts?.waitUntil !== "commit";
    const resp = await this._navigate(this._url, undefined, undefined, {
      ...opts,
      referer: opts?.referer ?? this._documentResponse?.referrer ?? null,
    });
    if (opts?.waitUntil === "networkidle")
      await this._openLiveSession(this._remainingNavigationTimeout(opts, started));
    return resp;
  }
  async goBack(opts) {
    const prev = this._history.at(-1);
    if (prev == null) return null;
    if (await traverseLive(this, prev, native)) return null;
    const current = this._historyCurrent ?? this._url;
    this._autoHydrate = opts?.waitUntil !== "commit";
    const response = await this._navigate(historyUrl(prev), undefined, undefined, opts);
    this._historyCurrent = null;
    this._history.pop();
    this._fwd.push(current);
    if (opts?.waitUntil === "networkidle") await this._openLiveSession();
    return response;
  }
  async goForward(opts) {
    const next = this._fwd.at(-1);
    if (next == null) return null;
    if (await traverseLive(this, next, native)) return null;
    const current = this._historyCurrent ?? this._url;
    this._autoHydrate = opts?.waitUntil !== "commit";
    const response = await this._navigate(historyUrl(next), undefined, undefined, opts);
    this._historyCurrent = null;
    this._fwd.pop();
    this._history.push(current);
    if (opts?.waitUntil === "networkidle") await this._openLiveSession();
    return response;
  }

  url() {
    return this._url;
  }
  async content() {
    return native.html(this._html);
  }
  async title() {
    return native.title(this._html);
  }
  async innerText(selector) {
    if (selector) return this.locator(selector).innerText();
    return native.text(this._html);
  }
  async innerHTML(selector) {
    return this.locator(selector).innerHTML();
  }
  async textContent(selector) {
    return this.locator(selector).textContent();
  }
  async getAttribute(selector, name) {
    return this.locator(selector).getAttribute(name);
  }
  async inputValue(selector) {
    return this.locator(selector).inputValue();
  }
  async isVisible(selector) {
    return this.locator(selector).isVisible();
  }
  async isHidden(selector) {
    return this.locator(selector).isHidden();
  }
  async isChecked(selector) {
    return this.locator(selector).isChecked();
  }
  async isEnabled(selector) {
    return this.locator(selector).isEnabled();
  }
  async isDisabled(selector) {
    return this.locator(selector).isDisabled();
  }
  async isEditable(selector) {
    return this.locator(selector).isEditable();
  }
  markdown() {
    return native.markdown(this._html, this._url);
  }
  async links() {
    return native.links(this._html, this._url);
  }
  async ariaSnapshot() {
    return native.ariaSnapshot(this._html, 0);
  }

  // Evaluate in the retained isolate. Native revisions identify actual changes to
  // the driver's snapshots; exposed bindings keep their cooperative settlement pump.
  async evaluate(script, arg) {
    if (this._closed) throw new Error("page is closed");
    if (hasJSHandles(arg)) return handleEvaluation(this, script, arg);
    await this._ensureLive();
    const body = evalSource(script, arg);
    if (this._mergedBindings().size) return this._evaluateWithBindings(body);
    const tracked = JSON.parse(
      await native.liveEvalExpressionTracked(
        this._session,
        `(async () => { try {
        return JSON.stringify({value: (${encodeEvaluationValue.toString()})(await (${body}))});
      } catch(error) { return JSON.stringify({error: (${encodeEvaluationError.toString()})(error)}); } })()`,
      ),
    );
    if (tracked.needsSync) await this._refreshFromSession();
    else if (tracked.events) await this._drainNetworkEvents();
    const result = JSON.parse(tracked.value);
    if (result.error) throw decodeEvaluationError(result.error);
    return decodeEvaluationValue(result.value);
  }
  async evaluateHandle(script, arg) {
    return handleEvaluation(this, script, arg, true);
  }
  // Run the page's own script (promises/timers/fetch/cookies) and replace the
  // cached HTML with the hydrated result — subsequent reads see the new DOM.
  async render(script) {
    this._html = native.render(this._html, this._url, script);
    return this._html;
  }
  // Hydrate: run the page's OWN scripts (inline + dynamically-injected chunks) the
  // way a browser does — fetch+execute each <script>, fire onload, drain to quiescence
  // — so a real SPA bundle mounts. The locator/expect surface then sees the live DOM.
  async hydrate() {
    if (this._hydrated) return this._html; // already run this document's JS
    // Pass the page's cookies so session-authenticated SPAs hydrate as the logged-in
    // user (the auth SDK's "fetch current user" call carries the session cookie).
    this._html = await native.hydrate(
      this._html,
      this._url,
      this._cookies,
      this._context._userAgent ?? undefined,
      this._requestState,
      JSON.stringify(this._documentResponse),
    );
    this._hydrated = true;
    return this._html;
  }
  async addInitScript(script, arg) {
    // No reload pipeline to inject before; run it now over the current DOM.
    this._context._initScripts.push(evalSource(script, arg));
    if (this._html) await this.render(evalSource(script, arg));
  }

  async setContent(html, opts = {}) {
    if (this._closed) throw new Error("page is closed");
    this._routeLifetime?.abort(new Error("Route's document has been replaced"));
    this._routeLifetime = undefined;
    this._routeRequests = undefined;
    const waitUntil = opts.waitUntil ?? "load";
    if (!["load", "domcontentloaded", "networkidle"].includes(waitUntil))
      throw new Error(`Unsupported setContent waitUntil: ${waitUntil}`);
    if (!this._live) {
      const previous = this._html;
      this._html = "";
      try {
        await this._openLiveSession(opts.timeout);
      } catch (error) {
        this._html = previous;
        throw error;
      }
    }
    this._imageCache = undefined;
    closeScreenshotScene(this);
    let failure;
    try {
      this._html = await native.liveSetContent(
        this._session,
        String(html),
        waitUntil,
        opts.timeout,
      );
    } catch (error) {
      failure = error;
    }
    const collecting = this._contentCollecting;
    this._contentCollecting = true;
    try {
      await this._refreshFromSession();
      await this._drainBindings();
    } finally {
      this._contentCollecting = collecting;
    }
    if (failure) {
      if (/setContent timeout/.test(failure.message)) failure.name = "TimeoutError";
      throw failure;
    }
    this._contentLoadPending = waitUntil === "domcontentloaded";
    this._emit("domcontentloaded");
    if (!this._contentLoadPending) this._emit("load");
  }

  // --- page-level actions (selector shortcuts) ---
  async fill(selector, value, options) {
    return this.locator(selector).fill(value, options);
  }
  async type(selector, value) {
    return this.locator(selector).type(value);
  }
  async check(selector) {
    return this.locator(selector).check();
  }
  async uncheck(selector) {
    return this.locator(selector).uncheck();
  }
  async setChecked(selector, on) {
    return this.locator(selector).setChecked(on);
  }
  async selectOption(selector, value, options) {
    return this.locator(selector).selectOption(value, options);
  }
  async setInputFiles(selector, files) {
    return this.locator(selector).setInputFiles(files);
  }
  async hover(selector) {
    return this.locator(selector).hover();
  }
  async click(selector) {
    const intent = JSON.parse(native.click(this._html, selector, this._url));
    return this._followIntent(intent);
  }
  async dblclick(selector) {
    return this.click(selector);
  }
  async tap(selector) {
    return this.click(selector);
  }
  async press(selector, key) {
    return this.locator(selector).press(key);
  }
  async focus(selector) {
    return this.locator(selector).focus();
  }
  async dispatchEvent() {}

  async _followIntent(intent) {
    try {
      if (intent.action === "navigate") return await this.goto(intent.url);
      if (intent.action === "submit") return await this._submit(intent);
      return null; // inert (a JS-only handler — nothing to fire without JS)
    } catch (error) {
      if (error instanceof DownloadNavigationError) return null;
      throw error;
    }
  }
  async _submit(intent) {
    const method = intent.method === "GET" ? null : intent.method;
    const r = JSON.parse(
      await native.fetchWithCookies(
        intent.url,
        this._cookies,
        method,
        intent.body ?? null,
        this._context._headersJson(),
        false,
        this._requestState,
        JSON.stringify({
          source: /^https?:/.test(this._url) ? this._url : null,
          destination: "document",
        }),
        null,
        null,
        false,
        null,
        this._context._maxResponseBytes,
      ),
    );
    this._imageCache = undefined;
    this._html = r.html;
    this._url = r.finalUrl;
    this._cookies = r.cookies;
    return makeResponse(r);
  }

  // --- locators ---
  _locFromSelector(selector, mode = "auto") {
    const loc = new Locator(this, (h) => JSON.parse(native.query(h, selector, mode)), {
      count: (h) => native.queryCount(h, selector, mode),
    });
    loc._selector = selector; // enables CSS-concat nesting
    return loc;
  }
  locator(selector) {
    return this._locFromSelector(selector);
  }
  getByRole(role, opts = {}) {
    const name = opts.name != null ? String(opts.name) : null;
    return new Locator(
      this,
      (h) => JSON.parse(native.getBy(h, "role", role, name, null, JSON.stringify(opts))),
      {
        getBy: { kind: "role", value: role, name, options: opts },
      },
    );
  }
  getByText(text, opts = {}) {
    return new Locator(
      this,
      (h) => JSON.parse(native.getBy(h, "text", String(text), null, null, JSON.stringify(opts))),
      {
        getBy: { kind: "text", value: String(text), name: null, options: opts },
      },
    );
  }
  getByLabel(text, opts = {}) {
    return new Locator(
      this,
      (h) => JSON.parse(native.getBy(h, "label", String(text), null, null, JSON.stringify(opts))),
      {
        getBy: { kind: "label", value: String(text), name: null, options: opts },
      },
    );
  }
  // The native get_by handles role/text/label; placeholder/alt/title map cleanly
  // to attribute selectors (substring, like Playwright's default exact:false).
  getByPlaceholder(text, _o) {
    return this._locFromSelector(`[placeholder*=${cssAttrValue(text)}]`);
  }
  getByAltText(text, _o) {
    return this._locFromSelector(`[alt*=${cssAttrValue(text)}]`);
  }
  getByTitle(text, _o) {
    return this._locFromSelector(`[title*=${cssAttrValue(text)}]`);
  }
  _regexTestId(pattern, root = null) {
    const attribute = this._context._testIdAttribute;
    const re = new RegExp(pattern.source, pattern.flags);
    const selector = `${root ? `${root} ` : ""}[${attribute}]`;
    return new Locator(
      this,
      (html) =>
        JSON.parse(native.query(html, selector)).filter((match) => {
          re.lastIndex = 0;
          return re.test(native.attrOf(html, match.node, attribute));
        }),
      { testId: { attribute, source: pattern.source, flags: pattern.flags, root } },
    );
  }
  getByTestId(id) {
    if (id instanceof RegExp) return this._regexTestId(id);
    return this._locFromSelector(`[${this._context._testIdAttribute}=${cssAttrValue(id)}]`);
  }

  // --- frames ---
  // The host page implements the Frame surface too, so `mainFrame()` keeps returning
  // `this` — Playwright code that does `page.mainFrame().locator(...)` works unchanged.
  // Child `<iframe>` elements are surfaced through Frame objects: `page.frame({name})`
  // and `page.frameLocator('iframe[...]')` scope queries to the child document, not
  // the host (turbo-surf#225). srcdoc iframes carry the child HTML directly;
  // src-loaded iframes are listed with empty content (the shim does not fetch
  // iframe src here — a live session hydrates them in the engine).
  mainFrame() {
    return this;
  }
  page() {
    return this;
  }
  name() {
    return "";
  }
  parentFrame() {
    return null;
  }
  childFrames() {
    return this.frames().filter((frame) => frame._parentFrame === this);
  }
  isDetached() {
    return false;
  }
  _syncFrames() {
    const previous = this._frames ?? [];
    const next = [];
    const visit = (parent) => {
      const nativeIdentity = parent._frameEntries != null;
      for (const [index, meta] of _collectIframes(parent).entries()) {
        let frame = previous.find(
          (candidate) =>
            candidate._parentFrame === parent &&
            (candidate._nativeIdentity === nativeIdentity
              ? candidate._node === meta.node
              : !candidate._nativeIdentity && nativeIdentity && candidate._path?.at(-1) === index),
        );
        if (!frame) {
          frame = new Frame(this, parent, meta);
          this._emit("frameattached", frame);
          this._emit("framenavigated", frame);
        } else if (frame._url !== meta.url) {
          frame._url = meta.url;
          this._emit("framenavigated", frame);
        }
        frame._node = meta.node;
        frame._nativeIdentity = nativeIdentity;
        if (meta.live) frame._html = meta.html;
        frame._name = meta.name;
        frame._path = meta.path ?? [...(parent._path ?? []), index];
        frame._frameEntries = meta.children;
        frame._requestContextId = meta.contextId;
        if (meta.contextId) {
          this._requestFrames ??= new Map();
          this._requestFrames.set(meta.contextId, frame);
        }
        next.push(frame);
        visit(frame);
      }
    };
    visit(this);
    for (const frame of previous.toReversed()) {
      if (!next.includes(frame)) {
        frame._detached = true;
        this._emit("framedetached", frame);
      }
    }
    this._frames = next;
  }
  frames() {
    this._syncFrames();
    return [this, ...this._frames];
  }
  frame(opts) {
    if (opts == null) return this;
    const name = typeof opts === "string" ? opts : opts?.name;
    const urlSpec = typeof opts === "object" && opts ? opts.url : null;
    return (
      this.frames().find(
        (frame) =>
          (name == null || frame.name() === name) &&
          (urlSpec == null || _matchUrl(frame.url(), urlSpec)),
      ) ?? null
    );
  }
  frameLocator(selector) {
    return new FrameLocator(this, selector);
  }

  // --- waiters (static engine: resolve immediately / poll evaluate) ---
  // `waitForLoadState(...)` is the other "page has settled" signal. In Playwright
  // all three states ('load' / 'domcontentloaded' / 'networkidle') end with the
  // SPA's own scripts having run; the shim has one "settled" tier (a live
  // session with the authored scripts drained), so treat them the same and
  // hydrate if we aren't live yet.
  async waitForLoadState(state = "load") {
    if (this._contentLoadPending && state !== "domcontentloaded") {
      const collecting = this._contentCollecting;
      this._contentCollecting = true;
      try {
        if (state === "networkidle") await native.liveEvalEngine(this._session, "");
        else
          await native.liveEvalEngineExpression(
            this._session,
            "__tsNs.waitForDocumentResources().then(() => '')",
          );
        await this._refreshFromSession();
        this._contentLoadPending = false;
        this._emit("load");
      } finally {
        this._contentCollecting = collecting;
      }
    }
    if (!this._live) await this._openLiveSession();
  }
  async waitForTimeout(ms) {
    await new Promise((r) => setTimeout(r, Math.min(ms ?? 0, 50)));
  }
  async waitForURL(url, opts = {}) {
    // Playwright accepts a string (glob), RegExp, or a predicate fn(URL)=>bool.
    const match = (u) => {
      if (typeof url === "function") {
        try {
          return !!url(new URL(u));
        } catch {
          return false;
        }
      }
      if (url instanceof RegExp) return url.test(u);
      return u.includes(String(url).replace(/\*\*/g, "").replace(/\*/g, ""));
    };
    const timeout =
      opts.timeout ?? this._navigationTimeout ?? this._context._navigationTimeout ?? 15000;
    const deadline = timeout === 0 ? Infinity : Date.now() + timeout;
    const timedOut = () =>
      Object.assign(new Error(`turbo-surf: waitForURL(${url}) — current url is ${this._url}`), {
        name: "TimeoutError",
      });
    for (;;) {
      if (match(this._url)) return undefined;
      if (this._closed) throw new Error("Page closed while waiting for URL");
      if (Date.now() >= deadline) throw timedOut();
      if (this._live && this._session != null) {
        try {
          // Observing a URL must not cancel requests owned by a concurrent action.
          await this._drainBindings(deadline, false, true);
          if (this._session != null) {
            await this._collectSession(this._session, "globalThis.__RESULT = '';", true);
            await this._refreshFromSession(true);
          }
          await this._drainBindings(deadline, false, true);
        } catch (error) {
          if (error.name === "TimeoutError" && Date.now() >= deadline) throw timedOut();
          throw error;
        }
      }
      if (match(this._url)) return undefined;
      await new Promise((resolve) =>
        setTimeout(resolve, Math.max(0, Math.min(20, deadline - Date.now()))),
      );
    }
  }
  async waitForSelector(selector, opts = {}) {
    const state = opts.state ?? "visible";
    if (!["attached", "detached", "hidden", "visible"].includes(state)) {
      throw new Error(`Unsupported selector wait state: ${state}`);
    }
    const locator = this.locator(selector);
    await locator.waitFor(opts);
    return state === "hidden" || state === "detached" ? null : locator.elementHandle();
  }
  // Poll `fn(arg)` in the live isolate until truthy. `arg` may be an ElementHandle
  // (from locator.elementHandle()) — resolved to the live element so e.g.
  // `waitForFunction((btn) => !btn.disabled, await uploadButton.elementHandle())` works.
  async waitForFunction(fn, arg, opts) {
    const timeout = opts?.timeout ?? 30000;
    const start = Date.now();
    for (;;) {
      const handle = await this.evaluateHandle(fn, arg);
      if (await handle.evaluate((value) => !!value)) return handle;
      await handle.dispose();
      if (timeout && Date.now() - start >= timeout)
        throw new Error("turbo-surf: waitForFunction timed out");
      await new Promise((resolve) => setTimeout(resolve, 40));
    }
  }

  // `waitForNavigation` resolves when the URL changes from the one at call time.
  // In a live session the change comes from a client redirect pumped by the next
  // drain; outside one it's the paired `goto`/click's synchronous nav. Poll url.
  async waitForNavigation(opts = {}) {
    const startUrl = this._url;
    const timeout = opts.timeout ?? 15000;
    const deadline = Date.now() + timeout;
    while (this._url === startUrl && Date.now() < deadline) {
      if (this._live) {
        try {
          await native.liveEvalEngine(this._session, "globalThis.__RESULT = location.href;");
        } catch {
          /* session gone — fall through */
        }
        await this._refreshFromSession();
      }
      if (this._url !== startUrl) break;
      await new Promise((r) => setTimeout(r, 20));
    }
    return makeResponse({ status: 200, finalUrl: this._url });
  }
  // waitForEvent dispatches on name: 'download' drains the live isolate's
  // __downloads buffer; 'response' / 'request' / 'requestfinished' resolve from
  // the same listener bus the native-log drain feeds (#218/#222). Unknown names
  // resolve to undefined — matches the pre-fix no-op for off-spec names.
  async waitForEvent(event, opts) {
    const timeout = (opts && typeof opts === "object" && opts.timeout) || 30000;
    if (["response", "request", "requestfinished", "requestfailed"].includes(event)) {
      const predicate =
        opts && typeof opts === "object" && typeof opts.predicate === "function"
          ? opts.predicate
          : () => true;
      return new Promise((resolve) => {
        const timer = setTimeout(() => {
          this.off(event, onEvt);
          if (poll) clearInterval(poll);
          resolve(undefined);
        }, timeout);
        const onEvt = (payload) => {
          try {
            if (!predicate(payload)) return;
          } catch {
            return;
          }
          clearTimeout(timer);
          if (poll) clearInterval(poll);
          this.off(event, onEvt);
          resolve(payload);
        };
        this.on(event, onEvt);
        // Same as waitForResponse / waitForRequest: poll unconditionally.
        const poll = setInterval(() => {
          this._drainNetworkEvents().catch(() => {});
        }, 50);
      });
    }
    if (event !== "download") return undefined;
    return this._waitForDownload(opts);
  }
  _recordDownload(entry) {
    const download = makeDownload(entry);
    (this._downloads ??= []).push(download);
    this._emit("download", download);
  }
  async _waitForDownload(opts = {}) {
    const timeout = opts.timeout ?? 30000;
    const start = Date.now();
    while (timeout === 0 || Date.now() - start < timeout) {
      if (this._closed) throw new Error("page is closed");
      if (this._downloads?.length) return this._downloads.shift();
      let raw;
      try {
        raw =
          this._session == null
            ? null
            : await native.liveEvalEngine(
                this._session,
                "globalThis.__RESULT = JSON.stringify((__tsNs.downloads || []).splice(0));",
              );
      } catch {
        raw = null;
      }
      let entries = [];
      try {
        entries = JSON.parse(raw || "[]");
      } catch {
        entries = [];
      }
      for (const entry of entries) this._recordDownload(entry);
      if (this._downloads?.length) return this._downloads.shift();
      await new Promise((r) => setTimeout(r, 30)); // let the concurrent click drive + push
    }
    const error = new Error(`Timeout ${timeout}ms exceeded while waiting for event download`);
    error.name = "TimeoutError";
    throw error;
  }
  async waitForResponse(matcher, opts = {}) {
    const test = networkWaitMatcher(matcher, this._context._baseURL);
    const timeout = opts.timeout ?? this._defaultTimeout ?? this._context._defaultTimeout ?? 30000;
    return new Promise((resolve, reject) => {
      const timer =
        timeout === 0
          ? null
          : setTimeout(() => {
              this.off("response", onResp);
              if (poll) clearInterval(poll);
              reject(
                Object.assign(new Error("Timeout waiting for response"), { name: "TimeoutError" }),
              );
            }, timeout);
      const onResp = async (resp) => {
        try {
          if (!(await test(resp))) return;
        } catch (error) {
          clearTimeout(timer);
          clearInterval(poll);
          this.off("response", onResp);
          reject(error);
          return;
        }
        clearTimeout(timer);
        if (poll) clearInterval(poll);
        this.off("response", onResp);
        resolve(resp);
      };
      this.on("response", onResp);
      // No action is driving `_drainNetworkEvents` here, so poll the native log
      // ourselves — otherwise a response that landed in a few ms sits in the log
      // until the caller's next step and this wait eats its full timeout.
      // Unconditional: `Promise.all([waitForResponse(...), goto(...)])` races
      // the goto that creates the live session, so `_live` is false AT CALL
      // TIME — poll anyway; `_drainNetworkEvents` is a cheap no-op without a
      // session and starts firing emits as soon as goto establishes one.
      const poll = setInterval(() => {
        this._drainNetworkEvents().catch(() => {});
      }, 10);
    });
  }
  async waitForRequest(matcher, opts = {}) {
    const test = networkWaitMatcher(matcher, this._context._baseURL);
    const timeout = opts.timeout ?? this._defaultTimeout ?? this._context._defaultTimeout ?? 30000;
    return new Promise((resolve, reject) => {
      const timer =
        timeout === 0
          ? null
          : setTimeout(() => {
              this.off("request", onReq);
              if (poll) clearInterval(poll);
              reject(
                Object.assign(new Error("Timeout waiting for request"), { name: "TimeoutError" }),
              );
            }, timeout);
      const onReq = async (req) => {
        try {
          if (!(await test(req))) return;
        } catch (error) {
          clearTimeout(timer);
          clearInterval(poll);
          this.off("request", onReq);
          reject(error);
          return;
        }
        clearTimeout(timer);
        if (poll) clearInterval(poll);
        this.off("request", onReq);
        resolve(req);
      };
      this.on("request", onReq);
      // See waitForResponse: poll unconditionally so a request produced after
      // a concurrent goto establishes the live session still short-circuits.
      const poll = setInterval(() => {
        this._drainNetworkEvents().catch(() => {});
      }, 50);
    });
  }

  // --- events (registry; we fire load/domcontentloaded; others are inert) ---
  on(event, fn) {
    const arr = this._listeners.get(event) ?? [];
    arr.push(fn);
    this._listeners.set(event, arr);
    this._updateNetworkPump();
    return this;
  }
  once(event, fn) {
    const once = (payload) => {
      this.off(event, once);
      fn.call(this, payload);
    };
    once.listener = fn;
    return this.on(event, once);
  }
  addListener(event, fn) {
    return this.on(event, fn);
  }
  off(event, fn) {
    const arr = this._listeners.get(event);
    const index =
      arr?.findLastIndex((listener) => listener === fn || listener.listener === fn) ?? -1;
    if (index >= 0) this._listeners.set(event, [...arr.slice(0, index), ...arr.slice(index + 1)]);
    this._updateNetworkPump();
    return this;
  }
  removeListener(event, fn) {
    return this.off(event, fn);
  }
  _emit(event, payload) {
    if (event === "framedetached") {
      const requests = this._routeRequests?.get(payload);
      for (const controller of requests?.values() ?? [])
        controller.abort(new Error("Route's frame has been detached"));
      this._routeRequests?.delete(payload);
    }
    for (const fn of this._listeners.get(event) ?? []) fn(payload);
  }

  // --- context / config / state ---
  context() {
    return this._context;
  }
  get request() {
    return this._context.request;
  }
  viewportSize() {
    return { ...this._viewport };
  }
  async setViewportSize(size) {
    if (![size.width, size.height].every((value) => Number.isInteger(value) && value > 0))
      throw new Error("viewport width and height must be positive integers");
    if (this._live) {
      await native.liveResize(this._session, size.width, size.height);
      await this._refreshFromSession();
      await this._drainBindings();
    }
    this._viewport = { ...size };
  }
  setDefaultTimeout(timeout) {
    this._defaultTimeout = timeout;
  }
  setDefaultNavigationTimeout(timeout) {
    this._navigationTimeout = timeout;
  }
  async setExtraHTTPHeaders(headers) {
    this._context._headers = { ...headers };
    this._context._configureNative();
  }
  _configureNative() {
    native.requestStateConfigure(
      this._requestState,
      JSON.stringify({
        colorScheme: this._media?.colorScheme ?? this._context._colorScheme,
        reducedMotion: this._media?.reducedMotion ?? this._context._reducedMotion,
        locale: this._context._locale,
        extraHTTPHeaders: this._context._headers,
        httpCredentials: this._context._httpCredentials,
      }),
    );
  }
  async emulateMedia(options = {}) {
    this._media = { ...this._media, ...options };
    this._configureNative();
    if (this._live) await this._redrain();
  }
  async bringToFront() {}
  addStyleTag(opts = {}) {
    return this._addResourceTag("style", opts);
  }
  addScriptTag(opts = {}) {
    return this._addResourceTag("script", opts);
  }
  async _addResourceTag(kind, opts) {
    validateResourceTagOptions(kind, opts);
    const content = opts.path != null ? await readFile(opts.path, "utf8") : opts.content;
    if (kind === "script" && opts.type === "module") {
      throw new Error("addScriptTag module injection load completion is not supported");
    }
    const external = opts.url != null;
    const tagName = kind === "script" ? "script" : external ? "link" : "style";
    const waitForLoad = external;
    if (!this._live) await this._openLiveSession();
    const token = `${kind}-${(this._injectedTagSeq = (this._injectedTagSeq ?? 0) + 1)}`;
    const result = await native.liveEvalExpression(
      this._session,
      `(async () => {
      try {
        const tag = document.createElement(${JSON.stringify(tagName)});
        tag.setAttribute('data-ts-injected', ${JSON.stringify(token)});
        ${kind === "script" && opts.type != null ? `tag.type = ${JSON.stringify(String(opts.type))};` : ""}
        ${external ? (kind === "style" ? `tag.rel = 'stylesheet'; tag.href = ${JSON.stringify(String(opts.url))};` : `tag.src = ${JSON.stringify(String(opts.url))};`) : `tag.textContent = ${JSON.stringify(String(content))};`}
        ${waitForLoad ? `const loaded = new Promise((resolve, reject) => { tag.onload = resolve; tag.onerror = () => reject(new Error('Unable to load ${kind}')); });` : ""}
        document.head.appendChild(tag);
        ${waitForLoad ? "await loaded;" : ""}
        return JSON.stringify({ ok: true });
      } catch (error) { return JSON.stringify({ error: String(error.message || error) }); }
    })()`,
    );
    const outcome = JSON.parse(result);
    await this._refreshFromSession();
    if (outcome.error) throw new Error(outcome.error);
    return this.locator(`[data-ts-injected="${token}"]`).elementHandle();
  }
  storageState() {
    return this._context.storageState();
  }
  addCookies(cookies) {
    return this._context.addCookies(cookies);
  }
  isClosed() {
    return this._closed;
  }
  async close() {
    if (this._closed) return;
    this._closeSession();
    if (this._requestState != null) native.requestStateClose(this._requestState);
    this._requestState = undefined;
    this._imageCache = undefined;
    closeScreenshotScene(this);
    this._closed = true;
    this._routeHandlers?.clear();
    this._routes = [];
    const position = this._context._pages.indexOf(this);
    if (position >= 0) this._context._pages.splice(position, 1);
    this._emit("close");
    if (this._ownsContext) await this._context.close();
  }
  video() {
    return null;
  }
  workers() {
    return [];
  }

  // A synthetic screenshot: native layout + paint over the current HTML, no
  // browser. Real pixels, but a *representative* render (honors CSS
  // position/z-index stacking; paints PNG/JPEG `<img>`/background images it can
  // fetch, others fall back to a placeholder). `type: "svg"` (or a `.svg` path)
  // returns a vector document; PNG and JPEG preserve the requested encoding.
  // `fullPage: true` grows the image to
  // the full content height; `clip` crops the rendered pixels while the context
  // viewport drives layout. Returns a Buffer; writes `path` if given.
  async screenshot(opts = {}) {
    const vp = this._viewport;
    const width = opts.width ?? vp.width;
    const height = opts.height ?? vp.height;
    const html = this._html;
    const maskIndices = screenshotMaskIndices(this, opts.mask);
    const deviceScale = screenshotDeviceScale(opts, this._context._deviceScaleFactor);
    const type = screenshotEncoding(opts);
    const asSvg = type === "svg";
    const optionsJson = JSON.stringify({
      fullPage: opts.fullPage === true,
      deviceScale,
      maskIndices,
      maskColor: opts.maskColor ?? "#ff00ff",
      omitBackground: type !== "jpeg" && opts.omitBackground === true,
      systemFonts: opts.systemFonts === true,
    });
    const { css, images, metadataJson } = await captureAssets(
      this,
      native,
      opts,
      width,
      height,
      optionsJson,
      html,
    );
    let out = paintScreenshotScene(this, native, {
      html,
      css,
      images,
      width,
      height,
      optionsJson,
      metadataJson,
      asSvg,
    });
    if (opts.clip) out = cropScreenshot(out, opts.clip, asSvg, deviceScale, opts.fullPage === true);
    if (type === "jpeg") out = native.screenshotJpeg(out, opts.quality);
    if (opts.path) await writeFile(opts.path, out);
    return out;
  }

  // Fetch the page's `<img>` + `background-image` bytes, keyed by their raw ref
  // (the resolver key the raster expects), resolved against the page URL and
  // fetched as raw bytes over the engine's client. Best-effort: non-http pages
  // and failed/unresolvable refs are skipped; count capped to match the engine.
  async _fetchImages(refs = native.imageUrls(this._html)) {
    if (!/^https?:/.test(this._url ?? "")) return {};
    const url = this._url,
      html = this._html;
    const requestState = this._requestState,
      cookies = this._cookies;
    const key = `${url}\u0000${html}`;
    if (this._imageCache?.key !== key) this._imageCache = { key, images: Object.create(null) };
    const images = this._imageCache.images;
    const selected = [...new Set(refs.slice(0, 60))];
    const entries = await mapBounded(selected, 8, async (ref) => {
      // Share pending work without restoring an invalidated document's cache.
      if (Object.hasOwn(images, ref)) return images[ref];
      images[ref] = (async () => {
        try {
          const abs = new URL(ref, url).href;
          const buf = await native.fetchBytes(abs, requestState, url, cookies);
          return buf && buf.length ? [ref, buf] : null;
        } catch {
          return null;
        }
      })();
      return images[ref];
    });
    return Object.fromEntries(entries.filter(Boolean));
  }

  // Concatenate the page's `<link rel="stylesheet">` bodies, resolved against the
  // page URL and fetched over the engine's client (cookies apply). Best-effort:
  // non-http pages and failed/relative-unresolvable sheets are skipped. The
  // fetches are independent and run concurrently; document order of the
  // concatenation is preserved (cascade order matters).
  async _fetchLinkedCss() {
    if (!/^https?:/.test(this._url ?? "")) return "";
    const sheets = await mapBounded(
      native.stylesheetHrefs(this._html).slice(0, 40),
      8,
      async (href) => {
        try {
          const abs = new URL(href, this._url).href;
          const r = JSON.parse(
            await native.fetchWithCookies(
              abs,
              this._cookies,
              null,
              null,
              this._context._headersJson(),
              false,
              this._requestState,
              JSON.stringify({ source: this._url, destination: "style", mode: "no-cors" }),
              null,
              null,
              false,
              null,
              this._context._maxResponseBytes,
            ),
          );
          return r.status >= 200 && r.status < 300 && r.html ? `${r.html}\n` : "";
        } catch {
          return ""; /* skip a sheet that won't resolve/fetch */
        }
      },
    );
    return sheets.join("");
  }

  // Drag an element matched by `source` to one matched by `target`. Thin wrapper
  // over Locator.dragTo so the same pointer/mouse/drag sequence fires (#227).
  async dragAndDrop(source, target, _opts) {
    return this.locator(source).dragTo(this.locator(target));
  }

  // --- unsupported → honest throws ---
  pdf = UNSUPPORTED("page.pdf", PIXEL);
  // Route callbacks run in Node through the existing asynchronous binding pump.
  // Guest fetch/XHR await serialized decisions; route.fetch bypasses the wrapper
  // while retaining the initiating realm's native network services.
  async _installRoutes() {
    this._routeBindingName ??= "__turboHostRoute_" + randomUUID().replaceAll("-", "");
    this._routeHandlers ??= new Map();
    for (const route of this._routes) this._routeHandlers.set(route.id, route);
    this._routeInitScript = buildRouteInitScript(this._routes, this._routeBindingName);
    // Retired callbacks stay until navigation so already-queued requests can settle.
    if (this._live) {
      await this._installNativeBindings();
    }
  }
  async route(urlPattern, handler) {
    this._routes = this._routes ?? [];
    this._routes.push(normalizeRoute(urlPattern, handler));
    await this._installRoutes();
    // Inject into the CURRENT live isolate too, so a route registered after
    // goto intercepts subsequent fetches without requiring a re-navigation
    // (turbo-surf#282). Updating the existing table avoids stale wrapper chains.
  }
  // An optional handler removes only that registration, preserving older matches.
  async unroute(urlPattern, handler) {
    if (!this._routes) return;
    const key =
      typeof urlPattern === "function"
        ? urlPattern
        : urlPattern instanceof RegExp
          ? urlPattern.source
          : String(urlPattern);
    this._routes = this._routes.filter(
      (r) => r.patternKey !== key || (handler && r.handler !== handler),
    );
    await this._installRoutes();
  }
  // page.routeFromHAR(har, options?) — read a Playwright/Chrome HAR file and
  // register each `log.entries[].request.url` as a route returning that entry's
  // response body/status/content-type via the same init-script fetch/XHR
  // wrapper. Playback-only; record mode is a follow-up. `options.url`, when
  // provided, filters entries by URL (RegExp test, or substring match for a
  // string). Unmatched requests fall through to the real network (Playwright's
  // `notFound: 'fallback'`); strict abort mode is not implemented yet.
  async routeFromHAR(har, options = {}) {
    const raw = await readFile(har, "utf8");
    const parsed = JSON.parse(raw);
    const entries = parsed?.log?.entries ?? [];
    const filter = options?.url ?? null;
    const matches = (url) => {
      if (filter == null) return true;
      if (filter instanceof RegExp) return filter.test(url);
      return String(url).indexOf(String(filter)) !== -1;
    };
    this._routes = this._routes ?? [];
    for (const entry of entries) {
      const route = harEntryToRoute(entry);
      if (!route) continue;
      if (!matches(route.pattern)) continue;
      this._routes.push(route);
    }
    await this._installRoutes();
  }
  pause = UNSUPPORTED("page.pause", "no inspector UI");
  // Persistent JS→host binding. The init-script path re-defines
  // `globalThis[name]` on every live session open (so it survives page
  // reloads + in-page navigations); when a session is already up we also
  // define it now so the test can call it before the next navigation.
  async _assertBindingAvailable(name) {
    if (this._mergedBindings().has(String(name)))
      throw new Error(`Function "${name}" has been already registered`);
    if (this._live) {
      const exists = await native.liveEvalEngine(
        this._session,
        `globalThis.__RESULT = String(${JSON.stringify(String(name))} in globalThis);`,
      );
      if (exists === "true") throw new Error(`Function "${name}" has been already registered`);
    }
  }
  async exposeBinding(name, fn, _options) {
    await this._assertBindingAvailable(name);
    this._bindings = this._bindings ?? new Map();
    this._bindings.set(String(name), { fn, isBinding: true });
    if (this._live) {
      await this._installNativeBindings();
    }
  }
  async exposeFunction(name, fn) {
    await this._assertBindingAvailable(name);
    this._bindings = this._bindings ?? new Map();
    this._bindings.set(String(name), { fn, isBinding: false });
    if (this._live) {
      await this._installNativeBindings();
    }
  }

  // Input devices: there's no compositor, but the live isolate runs the page's
  // JS, so pointer/mouse/touch handlers are real. The shim dispatches synthetic
  // events through the Mouse/Touchscreen state machines (turbo-surf#227).
  get mouse() {
    return (this._mouse ??= makeMouse(this));
  }
  get keyboard() {
    return (this._keyboard ??= makeKeyboard(this));
  }
  get touchscreen() {
    return (this._touch ??= makeTouchscreen(this));
  }
}

// page.mouse — minimal state (x, y, button) + event dispatch at
// `document.elementFromPoint(x, y)`. No live session → every call no-ops cleanly
// (same shape as the keyboard). `click` = move → down → up (+ click), `dblclick`
// adds a second click and a `dblclick`. `wheel(dx, dy)` fires a `wheel` event at
// the current cursor target.
function makeMouse(page) {
  const state = { x: 0, y: 0, button: 0, buttons: 0, detail: 1 };
  const run = async (script) => {
    if (!page._live) return;
    await page._sessionEval(script);
  };
  const base = (extra = "") =>
    `{ bubbles: true, cancelable: true, view: globalThis, clientX: ${state.x}, clientY: ${state.y}, screenX: ${state.x}, screenY: ${state.y}, button: ${state.button}, buttons: ${state.buttons}, detail: ${state.detail}${extra} }`;
  // Re-read state.x/state.y on every dispatch — baking them at definition time
  // sent every click/down/up/wheel to document.elementFromPoint(0,0) regardless
  // of the preceding move(x, y).
  const targetExpr = () =>
    `(document.elementFromPoint(${state.x}, ${state.y}) || document.documentElement || document.body)`;
  const dispatch = (pointerType, mouseType) => `(() => {
    const t = ${targetExpr()};
    if (!t) return;
    const PE = globalThis.PointerEvent || globalThis.MouseEvent;
    t.dispatchEvent(new PE(${JSON.stringify(pointerType)}, ${base(", pointerId: 1, isPrimary: true, pointerType: 'mouse'")}));
    t.dispatchEvent(new MouseEvent(${JSON.stringify(mouseType)}, ${base()}));
  })();`;
  async function move(x, y) {
    state.x = Math.round(x);
    state.y = Math.round(y);
    await run(dispatch("pointermove", "mousemove"));
  }
  const buttonValue = (opts) => (opts?.button === "right" ? 2 : opts?.button === "middle" ? 1 : 0);
  const buttonMask = (button) => [1, 4, 2][button];
  async function down(opts) {
    state.detail = opts?.clickCount ?? 1;
    state.button = buttonValue(opts);
    const wasPressed = state.buttons !== 0;
    state.buttons |= buttonMask(state.button);
    await run(dispatch(wasPressed ? "pointermove" : "pointerdown", "mousedown"));
    if (state.button === 2) {
      await run(`(() => {
        const t = ${targetExpr()};
        if (t) t.dispatchEvent(new MouseEvent("contextmenu", ${base()}));
      })();`);
    }
  }
  async function up(opts) {
    state.detail = opts?.clickCount ?? 1;
    state.button = buttonValue(opts);
    const mask = buttonMask(state.button);
    const wasPressed = (state.buttons & mask) !== 0;
    state.buttons &= ~mask;
    await run(dispatch(state.buttons ? "pointermove" : "pointerup", "mouseup"));
    if (wasPressed) {
      await run(`(() => {
        const t = ${targetExpr()};
        if (t) t.dispatchEvent(new MouseEvent(${JSON.stringify(state.button === 0 ? "click" : "auxclick")}, ${base()}));
      })();`);
    }
  }
  async function click(x, y, opts) {
    await move(x, y);
    for (let count = 1; count <= (opts?.clickCount ?? 1); count++) {
      const step = { ...opts, clickCount: count };
      await down(step);
      if (opts?.delay) await new Promise((resolve) => setTimeout(resolve, opts.delay));
      await up(step);
      if (count === 2 && state.button === 0) {
        await run(`(() => {
          const t = ${targetExpr()};
          if (t) t.dispatchEvent(new MouseEvent("dblclick", ${base()}));
        })();`);
      }
    }
  }
  async function dblclick(x, y, opts) {
    await click(x, y, { ...opts, clickCount: 2 });
  }
  async function wheel(dx, dy) {
    if (!page._live) return;
    await page._sessionEval(`(() => {
      const t = ${targetExpr()};
      if (!t) return;
      t.dispatchEvent(new (globalThis.WheelEvent || globalThis.MouseEvent)("wheel", ${base(`, deltaX: ${Number(dx) || 0}, deltaY: ${Number(dy) || 0}, deltaMode: 0`)}));
    })();`);
  }
  return { move, down, up, click, dblclick, wheel };
}

// page.touchscreen — single-touch tap at (x, y): pointerdown/touchstart →
// pointerup/touchend → click on elementFromPoint, so JS tap handlers fire (#227).
function makeTouchscreen(page) {
  return {
    async tap(x, y) {
      if (!page._live) return;
      const xi = Math.round(x);
      const yi = Math.round(y);
      const result = await page._sessionEval(`(() => {
        const t = (document.elementFromPoint(${xi}, ${yi}) || document.documentElement || document.body);
        if (!t) return;
        const common = { bubbles: true, cancelable: true, view: globalThis, clientX: ${xi}, clientY: ${yi}, screenX: ${xi}, screenY: ${yi} };
        let touch, start, end;
        try {
          touch = new Touch({ identifier: 1, target: t, clientX: ${xi}, clientY: ${yi}, screenX: ${xi}, screenY: ${yi}, pageX: ${xi}, pageY: ${yi} });
          start = new TouchEvent("touchstart", { ...common, touches: [touch], targetTouches: [touch], changedTouches: [touch] });
          end = new TouchEvent("touchend", { ...common, touches: [], targetTouches: [], changedTouches: [touch] });
          if (start.touches?.length !== 1 || start.changedTouches?.length !== 1 || end.touches?.length !== 0 || end.changedTouches?.length !== 1) throw new Error("missing touch lists");
        } catch {
          globalThis.__RESULT = "TOUCH_UNAVAILABLE";
          return;
        }
        const PE = globalThis.PointerEvent || globalThis.MouseEvent;
        t.dispatchEvent(new PE("pointerdown", { ...common, pointerId: 1, isPrimary: true, pointerType: 'touch' }));
        const accepted = t.dispatchEvent(start);
        t.dispatchEvent(new PE("pointerup", { ...common, pointerId: 1, isPrimary: true, pointerType: 'touch' }));
        const ended = t.dispatchEvent(end);
        if (accepted && ended) {
          for (const type of ["mousedown", "mouseup", "click"])
            t.dispatchEvent(new MouseEvent(type, { ...common, button: 0, detail: 1 }));
        }
        globalThis.__RESULT = "OK";
      })();`);
      if (result === "TOUCH_UNAVAILABLE")
        throw new Error(
          "turbo-surf: touchscreen.tap unavailable — native Touch/TouchEvent payloads unsupported",
        );
    },
  };
}

function screenshotMaskIndices(page, masks) {
  if (masks == null) return [];
  if (!Array.isArray(masks)) throw new Error("Screenshot mask must be an array of locators");
  if (!masks.length) return [];
  const all = JSON.parse(native.query(page._html, "*", "css"));
  const indices = new Map(all.map((match, index) => [match.node, index]));
  const selected = new Set();
  for (const locator of masks) {
    if (!(locator instanceof Locator) || locator._page !== page) {
      throw new Error("Screenshot mask locators must belong to this page; frame masks unavailable");
    }
    for (const match of locator._all()) {
      const index = indices.get(match.node);
      if (index != null) selected.add(index);
    }
  }
  return [...selected];
}

function screenshotDeviceScale(options, deviceScale = 1) {
  if (options.scale != null && !["css", "device"].includes(options.scale)) {
    throw new Error("Screenshot scale must be css or device");
  }
  return options.scale === "css" ? 1 : deviceScale;
}

function screenshotEncoding(opts) {
  const extension = typeof opts.path === "string" ? opts.path.split(".").pop().toLowerCase() : "";
  const type =
    opts.type ?? ({ svg: "svg", jpg: "jpeg", jpeg: "jpeg", png: "png" }[extension] || "png");
  if (!["png", "jpeg", "svg"].includes(type)) throw new Error("Unsupported screenshot type");
  if (
    opts.quality != null &&
    (type !== "jpeg" || !Number.isInteger(opts.quality) || opts.quality < 0 || opts.quality > 100)
  ) {
    throw new Error(
      "Screenshot quality is only supported for JPEG and must be an integer from 0 to 100",
    );
  }
  return type;
}

function cropScreenshot(png, clip, asSvg, deviceScale = 1, fullPage = false) {
  if (asSvg) throw new Error("Screenshot clip is unavailable for SVG output");
  const { x, y, width, height } = clip;
  if (
    ![x, y, width, height].every(Number.isFinite) ||
    x < 0 ||
    y < 0 ||
    width <= 0 ||
    height <= 0
  ) {
    throw new Error(
      "Screenshot clip requires nonnegative coordinates and positive finite dimensions",
    );
  }
  // Playwright truncates viewport clip dimensions in CSS pixels before device scaling.
  return native.screenshotCrop(
    png,
    Math.round(x * deviceScale),
    Math.round(y * deviceScale),
    Math.round((fullPage ? width : Math.floor(width + 1e-3)) * deviceScale),
    Math.round((fullPage ? height : Math.floor(height + 1e-3)) * deviceScale),
  );
}

// Drag `srcIdx` to `dstIdx` (live `querySelectorAll('*')` positions). Fires the
// full pointer+mouse sequence on source then target, plus the HTML5 drag/drop
// lifecycle (dragstart on source → dragenter/dragover on target → drop on target
// → dragend on source), all sharing one DataTransfer so a real app's handlers
// can set/read types and data. No compositor, so no OS drag image (#227).
function dragScript(srcIdx, dstIdx) {
  return `(() => {
    const all = document.querySelectorAll('*');
    const src = all[${srcIdx}];
    const dst = all[${dstIdx}];
    if (!src || !dst) { globalThis.__RESULT = "NO_MATCH"; return; }
    const PE = globalThis.PointerEvent || globalThis.MouseEvent;
    const dt = (typeof globalThis.DataTransfer === "function") ? new globalThis.DataTransfer() : null;
    const pointerOpts = (t) => ({ bubbles: true, cancelable: true, view: globalThis, pointerId: 1, isPrimary: true, pointerType: 'mouse', button: 0, buttons: t === "pointerup" ? 0 : 1 });
    const mouseOpts = (buttons) => ({ bubbles: true, cancelable: true, view: globalThis, button: 0, buttons });
    const dragOpts = () => {
      const o = { bubbles: true, cancelable: true, view: globalThis, button: 0 };
      if (dt) o.dataTransfer = dt;
      return o;
    };
    src.dispatchEvent(new PE("pointerdown", pointerOpts("pointerdown")));
    src.dispatchEvent(new MouseEvent("mousedown", mouseOpts(1)));
    const DE = globalThis.DragEvent || globalThis.MouseEvent;
    if (!src.dispatchEvent(new DE("dragstart", dragOpts()))) {
      src.dispatchEvent(new PE("pointerup", pointerOpts("pointerup")));
      src.dispatchEvent(new MouseEvent("mouseup", mouseOpts(0)));
      globalThis.__RESULT = "OK";
      return;
    }
    dst.dispatchEvent(new PE("pointermove", pointerOpts("pointermove")));
    dst.dispatchEvent(new MouseEvent("mousemove", mouseOpts(1)));
    dst.dispatchEvent(new DE("dragenter", dragOpts()));
    if (!dst.dispatchEvent(new DE("dragover", dragOpts()))) {
      dst.dispatchEvent(new DE("drop", dragOpts()));
    }
    src.dispatchEvent(new DE("dragend", dragOpts()));
    dst.dispatchEvent(new PE("pointerup", pointerOpts("pointerup")));
    dst.dispatchEvent(new MouseEvent("mouseup", mouseOpts(0)));
    globalThis.__RESULT = "OK";
  })();`;
}
function makeResponse(r, ensureOpen = () => {}) {
  const headers = {};
  const entries = Array.isArray(r.headers) ? r.headers : Object.entries(r.headers ?? {});
  for (const [key, value] of entries) {
    const name = key.toLowerCase();
    headers[name] =
      headers[name] === undefined
        ? value
        : `${headers[name]}${name === "set-cookie" ? "\n" : ", "}${value}`;
  }
  return {
    status: () => r.status,
    ok: () => r.status >= 200 && r.status < 300,
    url: () => r.finalUrl,
    statusText: () => r.statusText ?? "",
    headers: () => ({ ...headers }),
    async body() {
      ensureOpen();
      return Buffer.from(r.bodyBytes ?? r.html ?? "");
    },
    async text() {
      ensureOpen();
      return r.html ?? "";
    },
    async json() {
      ensureOpen();
      return JSON.parse(r.html ?? "null");
    },
  };
}

// A Playwright-like Response built from a captured live-app fetch (net-log entry),
// for `page.on('response')` subscribers.
const requestRedirects = new WeakMap();
function makeCapturedRequest(e, frame, response = () => null) {
  const requestHeaders = Object.fromEntries(
    Object.entries(e.request?.headers ?? {}).map(([name, value]) => [name.toLowerCase(), value]),
  );
  const postData = e.request?.bodyBytes == null ? null : Buffer.from(e.request.bodyBytes);
  const redirects = { from: null, to: null };
  const request = {
    url: () => e.url,
    method: () => e.method || "GET",
    headers: () => ({ ...requestHeaders }),
    async allHeaders() {
      return { ...requestHeaders };
    },
    postData: () => postData?.toString("utf8") ?? null,
    postDataBuffer: () => (postData == null ? null : Buffer.from(postData)),
    postDataJSON: () => (postData == null ? null : JSON.parse(postData.toString("utf8"))),
    redirectedFrom: () => redirects.from,
    redirectedTo: () => redirects.to,
    resourceType: () => e.context?.resourceType ?? "fetch",
    isNavigationRequest: () => !!e.context?.navigation,
    frame() {
      if (!frame) throw new Error("Request frame is unavailable");
      return frame;
    },
    async response() {
      return e.status === 0 ? null : response();
    },
    failure: () => (e.status === 0 || e.error ? { errorText: e.error || "net::ERR_FAILED" } : null),
  };
  requestRedirects.set(request, redirects);
  return request;
}

function makeNetResponse(e, frame, lifecycle) {
  const entries = e.headers ?? (e.contentType ? [["content-type", e.contentType]] : []);
  const all = Object.create(null);
  for (const [key, value] of entries) {
    const name = key.toLowerCase();
    all[name] =
      all[name] === undefined
        ? value
        : `${all[name]}${name === "set-cookie" ? "\n" : ", "}${value}`;
  }
  let response;
  const req = makeCapturedRequest(e, frame, () => response);
  response = {
    url: () => e.url,
    status: () => e.status,
    statusText: () => e.statusText ?? "",
    ok: () => !!e.ok,
    headers: () =>
      Object.fromEntries(
        Object.entries(all).filter(([name]) => !["set-cookie", "set-cookie2"].includes(name)),
      ),
    async allHeaders() {
      return { ...all };
    },
    async headerValue(name) {
      return all[name.toLowerCase()] ?? null;
    },
    async headersArray() {
      return entries.map(([name, value]) => ({ name, value }));
    },
    async body() {
      await lifecycle?.promise;
      if (e.bodyUnavailable) throw new Error("Redirect response body is unavailable");
      if (e.bodyIncomplete) throw new Error("Response body is incomplete");
      return Buffer.from(e.bodyBytes ?? e.body ?? "");
    },
    async text() {
      await lifecycle?.promise;
      if (e.bodyUnavailable) throw new Error("Redirect response body is unavailable");
      return e.body ?? "";
    },
    async json() {
      await lifecycle?.promise;
      if (e.bodyUnavailable) throw new Error("Redirect response body is unavailable");
      return JSON.parse(e.body ?? "null");
    },
    request: () => req,
    frame: () => req.frame(),
    async finished() {
      await lifecycle?.promise;
      return null;
    },
  };
  return response;
}

// A Playwright-like Download built from a captured client-side export (a `<a download>`
// click over a createObjectURL blob — see the env's __downloads). The bytes are written to
// a temp file lazily so download.path()/saveAs() behave like the real thing.
function makeDownload(entry) {
  const fs = require("node:fs");
  const os = require("node:os");
  const path = require("node:path");
  const filename =
    path.basename((entry.filename || "download").replaceAll("\\", "/")) || "download";
  const content =
    entry.bodyBytes == null ? Buffer.from(entry.content ?? "") : Buffer.from(entry.bodyBytes);
  let cached = null;
  const writeTemp = () => {
    if (cached) return cached;
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "turbo-surf-dl-"));
    const p = path.join(dir, filename);
    fs.writeFileSync(p, content);
    cached = p;
    return p;
  };
  return {
    suggestedFilename: () => filename,
    url: () => entry.url || "",
    async path() {
      return writeTemp();
    },
    async saveAs(dest) {
      const fs2 = require("node:fs");
      fs2.writeFileSync(dest, content);
    },
    async failure() {
      return null;
    },
    async createReadStream() {
      return require("node:fs").createReadStream(writeTemp());
    },
    async delete() {},
    async cancel() {},
  };
}

// ---------------------------------------------------------------------------
// BrowserContext
// ---------------------------------------------------------------------------
class BrowserContext {
  constructor(opts = {}, browser = null) {
    this._closed = false;
    this._browser = browser;
    // Playwright CLI serializes the per-project `use` block into TURBO_PW_USE
    // so every supported key can reach fixture-constructed contexts without
    // the test code having to pass it explicitly. In-suite `opts` still wins
    // over the project block.
    let projectUse = {};
    try {
      projectUse = process.env.TURBO_PW_USE ? JSON.parse(process.env.TURBO_PW_USE) : {};
    } catch {
      /* ignore malformed env — a project without a use block is the common case */
    }
    const merged = { ...projectUse, ...opts };
    validateArtifactOptions(merged);
    this._deviceScaleFactor = merged.deviceScaleFactor ?? 1;
    if (!Number.isFinite(this._deviceScaleFactor) || this._deviceScaleFactor <= 0)
      throw new Error("deviceScaleFactor must be finite and positive");
    this.tracing = Object.fromEntries(
      ["start", "startChunk", "stopChunk", "stop", "group", "groupEnd"].map((method) => [
        method,
        UNSUPPORTED(`tracing.${method}`, "no-browser engine has no Playwright trace recorder"),
      ]),
    );
    requireOfflineSupport(merged.offline);
    this._maxResponseBytes = responseBudget(merged.maxResponseBytes);
    const cookies = merged.storageState?.cookies ?? opts.storageState?.cookies ?? [];
    // `playwright.config` `use: { baseURL, testIdAttribute }` isn't read by the
    // shim runner, so the register step maps it in via env (TURBO_SHIM_*).
    this._defaultTimeout = merged.actionTimeout;
    this._baseURL = merged.baseURL ?? process.env.TURBO_SHIM_BASE_URL ?? null;
    this._locale = merged.locale ?? process.env.TURBO_SHIM_LOCALE ?? null;
    this._cookies = JSON.stringify(cookies);
    this._requestState = native.requestStateNew?.();
    const origins = merged.storageState?.origins ?? [];
    if (origins.length) {
      try {
        native.requestStateImportStorage(this._requestState, encodeStorageOrigins(origins));
      } catch (error) {
        native.requestStateClose(this._requestState);
        throw error;
      }
    }
    // Custom User-Agent → page-JS navigator.userAgent + page fetches (newContext({userAgent})).
    this._userAgent = merged.userAgent ?? process.env.TURBO_SHIM_USER_AGENT ?? null;
    this._headers = merged.extraHTTPHeaders ?? {};
    this._viewport = merged.viewport ?? { width: 1280, height: 720 };
    this._testIdAttribute =
      merged.testIdAttribute ?? process.env.TURBO_SHIM_TESTID_ATTR ?? "data-testid";
    // Capture-only remaining per-project `use` keys so a suite that reads them
    // from the context (via a getter or in-suite introspection) sees the real
    // project value. The engine doesn't act on HTTPS/permissions/geolocation
    // today — these are present so a project's config doesn't silently vanish.
    this._httpCredentials = merged.httpCredentials ?? null;
    this._ignoreHTTPSErrors = merged.ignoreHTTPSErrors ?? false;
    this._colorScheme = merged.colorScheme ?? null;
    this._reducedMotion = merged.reducedMotion ?? null;
    this._permissions = merged.permissions ?? [];
    this._geolocation = merged.geolocation ?? null;
    this._timezoneId = merged.timezoneId ?? null;
    this._offline = merged.offline ?? false;
    this._screen = merged.screen ?? null;
    this._initScripts = [];
    this._bindings = new Map();
    this._pages = [];
    this._listeners = new Map();
    this._pageEventListeners = new WeakMap();
    if (
      this._colorScheme ||
      this._reducedMotion ||
      this._locale ||
      Object.keys(this._headers).length ||
      this._httpCredentials
    )
      this._configureNative();
  }
  _configureNative() {
    native.requestStateConfigure(
      this._requestState,
      JSON.stringify({
        colorScheme: this._colorScheme,
        reducedMotion: this._reducedMotion,
        locale: this._locale,
        extraHTTPHeaders: this._headers,
        httpCredentials: this._httpCredentials,
      }),
    );
    for (const page of this._pages) if (!page._closed) page._configureNative();
  }
  _headersJson() {
    // Merge the context User-Agent into the request headers (so goto/request fetches
    // carry it too, matching the live session's navigator.userAgent).
    const h = this._userAgent ? { "user-agent": this._userAgent, ...this._headers } : this._headers;
    return Object.keys(h).length ? JSON.stringify(h) : null;
  }
  async newPage() {
    if (this._closed) throw new Error("browser context is closed");
    const p = new Page(this);
    this._pages.push(p);
    for (const event of NETWORK_EVENTS) this._syncPageEvent(event);
    this._emit("page", p);
    return p;
  }
  pages() {
    return [...this._pages];
  }
  storageState() {
    return {
      cookies: JSON.parse(this._cookies),
      origins: decodeStorageOrigins(native.requestStateStorage(this._requestState)),
    };
  }
  addCookies(cookies) {
    const merged = new Map();
    for (const cookie of [...JSON.parse(this._cookies), ...cookies.map(nativeCookieRecord)]) {
      const domain = String(cookie.domain ?? "")
        .toLowerCase()
        .replace(/^\./, "");
      merged.set(JSON.stringify([cookie.name, domain, cookie.path || "/"]), cookie);
    }
    this._setCookies(JSON.stringify([...merged.values()]));
  }
  async cookies(urls) {
    const cookies = JSON.parse(this._cookies);
    if (urls == null || (Array.isArray(urls) && urls.length === 0)) return cookies;
    const targets = (Array.isArray(urls) ? urls : [urls]).map((url) => new URL(url));
    return cookies.filter((cookie) => targets.some((url) => cookieMatchesUrl(cookie, url)));
  }
  _setCookies(cookies) {
    this._cookies = cookies;
    for (const page of this._pages) {
      if (page._session != null) native.liveSetCookies(page._session, cookies);
    }
  }
  async clearCookies(filters = {}) {
    this._setCookies(
      JSON.stringify(
        JSON.parse(this._cookies).filter((cookie) => !cookieMatchesFilter(cookie, filters)),
      ),
    );
  }
  async addInitScript(script, arg) {
    this._initScripts.push(evalSource(script, arg));
  }
  async setExtraHTTPHeaders(headers) {
    this._headers = { ...headers };
    this._configureNative();
  }
  setDefaultTimeout(timeout) {
    this._defaultTimeout = timeout;
  }
  async setDefaultNavigationTimeout(timeout) {
    this._navigationTimeout = timeout;
  }
  async grantPermissions() {}
  async clearPermissions() {}
  async setGeolocation() {}
  async setOffline(offline) {
    requireOfflineSupport(offline);
    this._offline = false;
  }
  on(event, fn) {
    if (typeof fn !== "function") throw new TypeError("Event listener must be a function");
    const arr = this._listeners.get(event) ?? [];
    arr.push(fn);
    this._listeners.set(event, arr);
    this._syncPageEvent(event);
    return this;
  }
  off(event, fn) {
    const arr = this._listeners.get(event);
    const index =
      arr?.findLastIndex((listener) => listener === fn || listener.listener === fn) ?? -1;
    if (index >= 0) this._listeners.set(event, [...arr.slice(0, index), ...arr.slice(index + 1)]);
    this._syncPageEvent(event);
    return this;
  }
  once(event, fn) {
    const once = (payload) => {
      this.off(event, once);
      fn.call(this, payload);
    };
    once.listener = fn;
    return this.on(event, once);
  }
  _emit(event, payload) {
    for (const fn of this._listeners.get(event) ?? []) fn.call(this, payload);
  }
  _syncPageEvent(event) {
    if (!NETWORK_EVENTS.includes(event)) return;
    for (const page of this._pages) {
      let listeners = this._pageEventListeners.get(page);
      if (!listeners) {
        listeners = new Map();
        this._pageEventListeners.set(page, listeners);
      }
      const previous = listeners.get(event);
      const needed = this._listeners.get(event)?.length > 0;
      if (needed && !previous) {
        const listener = (payload) => this._emit(event, payload);
        listeners.set(event, listener);
        page.on(event, listener);
      } else if (!needed && previous) {
        page.off(event, previous);
        listeners.delete(event);
      }
    }
  }
  waitForEvent(event, options = {}) {
    if (this._closed) return Promise.reject(new Error("Browser context is closed"));
    const predicate = typeof options === "function" ? options : (options.predicate ?? (() => true));
    const timeout = typeof options === "function" ? 30000 : (options.timeout ?? 30000);
    return new Promise((resolve, reject) => {
      let timer;
      let settled = false;
      const finish = (error, payload, failed = error != null) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        this.off(event, onEvent);
        this.off("close", onClose);
        if (failed) reject(error);
        else resolve(payload);
      };
      const onEvent = async (payload) => {
        try {
          if (await predicate(payload)) finish(null, payload);
        } catch (error) {
          finish(error, undefined, true);
        }
      };
      const onClose = () => {
        if (event !== "close") finish(new Error("Browser context closed while waiting for event"));
      };
      this.on(event, onEvent);
      this.on("close", onClose);
      if (timeout !== 0)
        timer = setTimeout(
          () =>
            finish(
              Object.assign(new Error(`Timeout waiting for context event: ${event}`), {
                name: "TimeoutError",
              }),
            ),
          timeout,
        );
    });
  }
  get request() {
    // Lazy: build one APIRequestContext per BrowserContext, bound to its
    // cookie jar (reads + Set-Cookie writeback).
    if (!this._requestCtx) {
      this._requestCtx = request.newContext({
        _browserContext: this,
        baseURL: this._baseURL,
        userAgent: this._userAgent,
      });
    }
    return this._requestCtx;
  }
  browser() {
    return this._browser ?? browserStub;
  }
  async close() {
    if (this._closed) return;
    this._closed = true;
    for (const page of this.pages()) await page.close();
    await this._requestCtx?.dispose();
    if (this._requestState != null) native.requestStateClose(this._requestState);
    this._requestState = undefined;
    this._browser?._forgetContext(this);
    this._emit("close", this);
  }
  route = UNSUPPORTED("context.route", NETIO);
  routeFromHAR = UNSUPPORTED("context.routeFromHAR", NETIO);
  unroute = UNSUPPORTED("context.unroute", NETIO);
  // Context-level expose: every Page in this context (and future ones) sees
  // the binding; live pages pick it up on the next navigation. On an open
  // session we also define it now so the current Page can use it immediately.
  async _assertBindingAvailable(name) {
    if (this._bindings.has(String(name)))
      throw new Error(`Function "${name}" has been already registered`);
    for (const page of this._pages) await page._assertBindingAvailable(name);
  }
  async exposeBinding(name, fn, _options) {
    await this._assertBindingAvailable(name);
    this._bindings.set(String(name), { fn, isBinding: true });
    for (const page of this._pages) {
      if (page._live) {
        await page._installNativeBindings();
      }
    }
  }
  async exposeFunction(name, fn) {
    await this._assertBindingAvailable(name);
    this._bindings.set(String(name), { fn, isBinding: false });
    for (const page of this._pages) {
      if (page._live) {
        await page._installNativeBindings();
      }
    }
  }
}

function makeBrowser() {
  const contexts = new Set();
  let connected = true;
  const ensureConnected = () => {
    if (!connected) throw new Error("Target page, context or browser has been closed");
  };
  return {
    version: () => "turbo-surf",
    browserType: () => ({ name: () => "chromium" }),
    async newContext(opts) {
      ensureConnected();
      const context = new BrowserContext(opts, this);
      contexts.add(context);
      return context;
    },
    async newPage(opts) {
      const page = await (await this.newContext(opts)).newPage();
      page._ownsContext = true;
      return page;
    },
    _forgetContext: (context) => contexts.delete(context),
    contexts: () => [...contexts],
    isConnected: () => connected,
    async close() {
      if (!connected) return;
      connected = false;
      await Promise.all(Array.from(contexts, (context) => context.close()));
    },
  };
}

const browserStub = makeBrowser();

// Minimal browser entry for drop-in feel: chromium.launch() → newPage().
export const chromium = {
  name: () => "chromium",
  async launch() {
    return makeBrowser();
  },
  async launchPersistentContext(_dir, opts) {
    return makeBrowser().newContext(opts);
  },
  async connect() {
    return makeBrowser();
  },
};
export const firefox = chromium;
export const webkit = chromium;

export function newPage(opts) {
  const page = new Page(new BrowserContext(opts));
  page._ownsContext = true;
  return page;
}

export { Locator, Page, BrowserContext };

// ---------------------------------------------------------------------------
// expect — dispatches on the asserted value: a Locator → LocatorAssertions, a
// Page → PageAssertions, an APIResponse → toBeOK, anything else → generic value
// matchers (jest-shaped). `.not` negates. Pixel-snapshot matchers throw.
// ---------------------------------------------------------------------------
class BaseExpect {
  constructor(value, negated) {
    this._v = value;
    this._neg = !!negated;
  }
  _ok(pass, message) {
    if (pass === this._neg) throw new Error(message);
  }
  // Web-first assertion retry: re-evaluate `thunk` (re-pumping the live app between
  // tries) until it satisfies the (possibly negated) expectation or `timeout`
  // elapses. Mirrors Playwright's auto-retrying assertions — a UI change that lands
  // shortly after an action (modal close, async row) is observed instead of missed.
  async _retry(thunk, opts) {
    const timeout = opts?.timeout ?? 5000;
    // Locator → its page; Page assertion → the page itself.
    const page =
      this._v && (this._v._page ?? (typeof this._v._redrain === "function" ? this._v : null));
    const start = Date.now();
    const run = async () => {
      try {
        return !!(await thunk());
      } catch (error) {
        if (/strict mode violation/i.test(error.message)) throw error;
        return false;
      }
    };
    await this._v?._prepareQuery?.();
    let pass = await run();
    while ((this._neg ? pass : !pass) && Date.now() - start < timeout) {
      if (!page) break;
      if (!page._live) {
        if (page._autoHydrate === false) break;
        await page._ensureLive();
        if (!page._live) break;
        pass = await run();
        continue;
      }
      await page._redrain();
      pass = await run();
    }
    return pass;
  }
}

class LocatorExpect extends BaseExpect {
  get not() {
    return new LocatorExpect(this._v, !this._neg);
  }
  _snap() {
    return this._v._snapshot() ?? {};
  }
  async toBeVisible(opts) {
    this._ok(
      await this._retry(async () => (await this._v.isVisible()) === (opts?.visible ?? true), opts),
      "expected element to be visible",
    );
  }
  async toBeHidden(opts) {
    this._ok(
      await this._retry(async () => !(await this._v.isVisible()), opts),
      "expected element to be hidden",
    );
  }
  async toBeAttached(opts) {
    this._ok(
      await this._retry(() => (this._v._node() != null) === (opts?.attached ?? true), opts),
      "expected element to be attached",
    );
  }
  async toBeChecked(opts) {
    this._ok(
      await this._retry(async () => (await this._v.isChecked()) === (opts?.checked ?? true), opts),
      "expected element to be checked",
    );
  }
  async toBeEnabled(opts) {
    this._ok(
      await this._retry(() => this._snap().enabled === (opts?.enabled ?? true), opts),
      "expected element to be enabled",
    );
  }
  async toBeDisabled(opts) {
    this._ok(
      await this._retry(() => this._snap().enabled === false, opts),
      "expected element to be disabled",
    );
  }
  async toBeEditable(opts) {
    this._ok(
      await this._retry(() => this._snap().editable === (opts?.editable ?? true), opts),
      "expected element to be editable",
    );
  }
  async toBeEmpty(opts) {
    this._ok(await this._retry(() => this._v.isEmpty(), opts), "expected element to be empty");
  }
  async toBeFocused(opts) {
    // Static DOMs have no focus — the pre-fix behavior kept `.not.toBeFocused`
    // passing and `.toBeFocused` failing. In a live session probe
    // `document.activeElement` via the isolate.
    if (!this._v._page._live) {
      this._ok(this._neg, "focus state unavailable on a static DOM");
      return;
    }
    this._ok(await this._retry(() => this._v._isFocused(), opts), "expected element to be focused");
  }
  async toBeInViewport(opts) {
    const ratio = opts?.ratio ?? 0;
    if (ratio < 0 || ratio > 1) throw new Error("ratio must be between 0 and 1");
    const pass = await this._retry(async () => {
      const locator = this._v;
      await locator._page._ensureLive();
      const index = await locator._resolveLiveIdx();
      if (index == null) return false;
      return locator._page.evaluate(
        ({ index, viewport, ratio }) => {
          const rect = document.querySelectorAll("*")[index].getBoundingClientRect();
          const width = Math.max(0, Math.min(rect.right, viewport.width) - Math.max(rect.left, 0));
          const height = Math.max(
            0,
            Math.min(rect.bottom, viewport.height) - Math.max(rect.top, 0),
          );
          const area = rect.width * rect.height;
          return area > 0 && width * height > 0 && (width * height) / area >= ratio;
        },
        { index, viewport: locator._page.viewportSize(), ratio },
      );
    }, opts);
    this._ok(pass, "expected element to be in viewport");
  }
  async toHaveCount(n, opts) {
    let got;
    const pass = await this._retry(async () => {
      got = await this._v.count();
      return got === n;
    }, opts);
    this._ok(pass, `expected count ${n}, got ${got}`);
  }
  async _textArrayAssertion(expected, options, contains) {
    let got;
    const pass = await this._retry(async () => {
      got = options?.useInnerText ? await this._v.allInnerTexts() : await this._v.allTextContents();
      return textCollectionMatches(expected, got, contains);
    }, options);
    this._ok(pass, `expected texts ${json(expected)}, got ${json(got)}`);
  }
  async toHaveText(s, opts) {
    if (Array.isArray(s)) return this._textArrayAssertion(s, opts, false);
    let got;
    const test = () => {
      const t = (got = normWs(this._snap().text ?? ""));
      return s instanceof RegExp ? s.test(t) : t === normWs(s);
    };
    this._ok(await this._retry(test, opts), `expected text ${s}, got "${got}"`);
  }
  async toContainText(s, opts) {
    if (Array.isArray(s)) return this._textArrayAssertion(s, opts, true);
    let got;
    const test = () => {
      const t = (got = normWs(this._snap().text ?? ""));
      return s instanceof RegExp ? s.test(t) : t.includes(normWs(s));
    };
    this._ok(await this._retry(test, opts), `expected text to contain "${s}", got "${got}"`);
  }
  async toHaveValue(value, opts) {
    let got;
    const pass = await this._retry(async () => {
      got = await this._v.inputValue();
      return matchText(value, got);
    }, opts);
    this._ok(pass, `expected value ${value}, got "${got}"`);
  }
  async toHaveValues(values) {
    const got = await this._v.selectedValues();
    this._ok(
      JSON.stringify(got) === JSON.stringify(values),
      `expected values ${values}, got ${got}`,
    );
  }
  async toHaveRole(role) {
    const got = (await this._snap()).role;
    this._ok(got === role, `expected role ${role}, got "${got}"`);
  }
  async toHaveAccessibleName(name) {
    const got = (await this._snap()).name;
    this._ok(matchText(name, got), `expected accessible name ${name}, got "${got}"`);
  }
  async toHaveAccessibleDescription(d) {
    const got = (await this._snap()).description;
    this._ok(matchText(d, got), `expected accessible description ${d}, got "${got}"`);
  }
  async toHaveAttribute(name, value, opts) {
    if (value && typeof value === "object" && !(value instanceof RegExp)) {
      opts = value;
      value = undefined;
    }
    let got;
    const test = async () => {
      got = await this._v.getAttribute(name);
      if (value === undefined) return got !== null;
      return got !== null && matchText(value, got);
    };
    this._ok(await this._retry(test, opts), `expected attribute ${name}=${value}, got ${got}`);
  }
  async _classAssertion(expected, options, contains) {
    let got;
    const pass = await this._retry(async () => {
      if (Array.isArray(expected)) {
        got = this._v
          ._all()
          .map((match) => native.attrOf(this._v._html, match.node, "class") ?? "");
        return (
          got.length === expected.length &&
          got.every((value, index) => classMatches(expected[index], value, contains))
        );
      }
      got = (await this._v.getAttribute("class")) ?? "";
      return classMatches(expected, got, contains);
    }, options);
    this._ok(pass, `expected classes ${json(expected)}, got ${json(got)}`);
  }
  async toHaveClass(classes, options) {
    return this._classAssertion(classes, options, false);
  }
  async toContainClass(classes, options) {
    return this._classAssertion(classes, options, true);
  }
  async toHaveId(id, opts) {
    let got;
    const pass = await this._retry(async () => {
      got = await this._v.getAttribute("id");
      return got !== null && matchText(id, got);
    }, opts);
    this._ok(pass, `expected id ${id}, got ${got}`);
  }
  async toHaveCSS(name, value, opts) {
    let got;
    const pass = await this._retry(async () => {
      got = await this._v.cssValue(name);
      return matchText(value, got);
    }, opts);
    this._ok(pass, `expected css ${name}:${value}, got "${got}"`);
  }
  async toHaveJSProperty(name, value, opts) {
    let got;
    const pass = await this._retry(async () => {
      const locator = this._v;
      await locator._page._ensureLive();
      const index = await locator._resolveLiveIdx();
      if (index == null) return false;
      got = await locator._page.evaluate(
        ({ index, name }) => {
          const value = document.querySelectorAll("*")[index][name];
          return { type: typeof value, value };
        },
        { index, name },
      );
      return got?.type === typeof value && (value === undefined || deepEqual(got.value, value));
    }, opts);
    this._ok(pass, `expected property ${name} to equal ${json(value)}, got ${json(got)}`);
  }
  async toMatchAriaSnapshot(expected) {
    const node = this._v._node();
    const pass = node != null && native.matchesAriaSnapshot(this._v._html, node, expected);
    this._ok(pass, "expected element to match the ARIA snapshot");
  }
  toHaveScreenshot = UNSUPPORTED("expect(locator).toHaveScreenshot", PIXEL);
}

class PageExpect extends BaseExpect {
  get not() {
    return new PageExpect(this._v, !this._neg);
  }
  async toHaveURL(url, opts) {
    const pass = await this._retry(() => matchText(url, this._v.url()), opts);
    this._ok(pass, `expected url ${url}, got "${this._v.url()}"`);
  }
  async toHaveTitle(t) {
    const got = await this._v.title();
    this._ok(matchText(t, got), `expected title ${t}, got "${got}"`);
  }
  toHaveScreenshot = UNSUPPORTED("expect(page).toHaveScreenshot", PIXEL);
}

// `expect(promise).resolves.<m>()` / `.rejects.<m>()` — await, then apply the
// matcher to the resolved value (or, for rejects, the thrown error).
function asyncMatchers(awaiter, neg) {
  return new Proxy(
    {},
    {
      get: (_target, name) => {
        if (name === "not") return asyncMatchers(awaiter, !neg);
        return async (...args) => {
          const { value, threw } = await awaiter();
          const received =
            threw && (name === "toThrow" || name === "toThrowError")
              ? () => {
                  throw value;
                }
              : value;
          return new ValueExpect(received, neg)[name](...args);
        };
      },
    },
  );
}

// jest-shaped generic-value matchers — pure JS, never cross the napi boundary.
async function retryAssertion(callback, options = {}, defaultTimeout = 5000) {
  const timeout = options.timeout ?? defaultTimeout;
  const intervals = options.intervals ?? [100, 250, 500, 1000];
  if (timeout < 0 || !Number.isFinite(timeout))
    throw new Error("timeout must be finite and non-negative");
  if (!intervals.length || intervals.some((value) => value < 0 || !Number.isFinite(value)))
    throw new Error("intervals must contain non-negative finite values");
  const deadline = timeout === 0 ? Infinity : Date.now() + timeout;
  let attempt = 0;
  for (;;) {
    let timer;
    try {
      const pending = Promise.resolve().then(callback);
      if (deadline === Infinity) return await pending;
      return await Promise.race([
        pending,
        new Promise((_, reject) => {
          timer = setTimeout(
            () => reject(new Error("Assertion callback timed out")),
            Math.max(0, deadline - Date.now()),
          );
        }),
      ]);
    } catch (error) {
      const remaining = deadline - Date.now();
      if (remaining <= 0) throw error;
      const interval = intervals[Math.min(attempt++, intervals.length - 1)];
      await new Promise((resolve) => setTimeout(resolve, Math.min(interval, remaining)));
      if (Date.now() >= deadline) throw error;
    } finally {
      clearTimeout(timer);
    }
  }
}

class ValueExpect extends BaseExpect {
  get not() {
    return new ValueExpect(this._v, !this._neg);
  }
  get resolves() {
    const p = Promise.resolve(this._v);
    return asyncMatchers(async () => ({ value: await p, threw: false }), this._neg);
  }
  get rejects() {
    const p = Promise.resolve(this._v);
    return asyncMatchers(async () => {
      try {
        await p;
      } catch (e) {
        return { value: e, threw: true };
      }
      throw new Error("expected promise to reject, but it resolved");
    }, this._neg);
  }
  _await() {
    return this._v;
  }
  async toPass(options) {
    if (typeof this._v !== "function") throw new Error("toPass requires a callback");
    await retryAssertion(this._v, options, 0);
  }
  toBe(x) {
    this._ok(Object.is(this._v, x), `expected ${this._v} to be ${x}`);
  }
  toEqual(x) {
    this._ok(deepEqual(this._v, x), `expected ${json(this._v)} to equal ${json(x)}`);
  }
  toStrictEqual(x) {
    this._ok(deepEqual(this._v, x, true), `expected ${json(this._v)} to strictly equal ${json(x)}`);
  }
  toContain(x) {
    const pass =
      typeof this._v === "string" ? this._v.includes(x) : Array.from(this._v ?? []).includes(x);
    this._ok(pass, `expected ${json(this._v)} to contain ${json(x)}`);
  }
  toContainEqual(x) {
    this._ok(
      Array.from(this._v ?? []).some((e) => deepEqual(e, x)),
      `expected to contain ${json(x)}`,
    );
  }
  toMatch(re) {
    this._ok(
      (re instanceof RegExp ? re : new RegExp(re)).test(String(this._v)),
      `expected ${this._v} to match ${re}`,
    );
  }
  toMatchObject(o) {
    this._ok(subset(o, this._v), `expected ${json(this._v)} to match ${json(o)}`);
  }
  toBeNull() {
    this._ok(this._v === null, `expected ${this._v} to be null`);
  }
  toBeUndefined() {
    this._ok(this._v === undefined, `expected ${this._v} to be undefined`);
  }
  toBeDefined() {
    this._ok(this._v !== undefined, "expected value to be defined");
  }
  toBeTruthy() {
    this._ok(!!this._v, `expected ${this._v} to be truthy`);
  }
  toBeFalsy() {
    this._ok(!this._v, `expected ${this._v} to be falsy`);
  }
  toBeNaN() {
    this._ok(Number.isNaN(this._v), `expected ${this._v} to be NaN`);
  }
  _numeric(expected, allowBigInt = true) {
    const valid = (value) =>
      typeof value === "number" || (allowBigInt && typeof value === "bigint");
    if (!valid(this._v) || !valid(expected))
      throw new Error(
        "Numeric matcher requires number" + (allowBigInt ? " or bigint" : "") + " arguments",
      );
  }
  toBeGreaterThan(n) {
    this._numeric(n);
    this._ok(this._v > n, `expected ${this._v} > ${n}`);
  }
  toBeGreaterThanOrEqual(n) {
    this._numeric(n);
    this._ok(this._v >= n, `expected ${this._v} >= ${n}`);
  }
  toBeLessThan(n) {
    this._numeric(n);
    this._ok(this._v < n, `expected ${this._v} < ${n}`);
  }
  toBeLessThanOrEqual(n) {
    this._numeric(n);
    this._ok(this._v <= n, `expected ${this._v} <= ${n}`);
  }
  toBeCloseTo(n, digits = 2) {
    this._numeric(n, false);
    if (typeof digits !== "number") throw new Error("Precision must be a number");
    this._ok(
      (this._v === n && !Number.isFinite(n)) || Math.abs(this._v - n) < 10 ** -digits / 2,
      `expected ${this._v} close to ${n}`,
    );
  }
  toBeInstanceOf(c) {
    this._ok(this._v instanceof c, `expected instance of ${c?.name}`);
  }
  toHaveLength(n) {
    this._ok(this._v?.length === n, `expected length ${n}, got ${this._v?.length}`);
  }
  toHaveProperty(key, value) {
    if (typeof key !== "string" && !Array.isArray(key))
      throw new TypeError("property path must be a string or array");
    const path = Array.isArray(key) ? key : key.replace(/\[(\d+)\]/g, ".$1").split(".");
    if (path.length === 0) throw new TypeError("property path must not be empty");
    let got = this._v;
    let exists = true;
    for (const segment of path) {
      if (got == null || !(segment in Object(got))) {
        exists = false;
        break;
      }
      got = got[segment];
    }
    this._ok(exists && (arguments.length < 2 || deepEqual(got, value)), `expected property ${key}`);
  }
  toThrow(expected) {
    if (typeof this._v !== "function") throw new TypeError("toThrow requires a function");
    let threw = false;
    let error;
    try {
      this._v();
    } catch (caught) {
      threw = true;
      error = caught;
    }
    this._ok(threw && matchesThrownError(error, expected), "expected function to throw");
  }
  toThrowError(expected) {
    return this.toThrow(expected);
  }
  // jest mock matchers (used with spies in suites)
  toHaveBeenCalled() {
    this._ok((this._v?.mock?.calls?.length ?? 0) > 0, "expected mock to have been called");
  }
  toHaveBeenCalledWith(...args) {
    const calls = this._v?.mock?.calls ?? [];
    this._ok(
      calls.some((c) => deepEqual(c, args)),
      `expected mock called with ${json(args)}`,
    );
  }
  toHaveScreenshot = UNSUPPORTED("toHaveScreenshot", PIXEL);
  toMatchSnapshot = UNSUPPORTED("toMatchSnapshot", PIXEL);
}

function textCollectionMatches(expected, actual, contains) {
  const matches = (want, got) => {
    if (want instanceof RegExp) {
      want.lastIndex = 0;
      return want.test(got);
    }
    const text = normWs(got);
    const target = normWs(want);
    return contains ? text.includes(target) : text === target;
  };
  if (!contains)
    return (
      actual.length === expected.length &&
      expected.every((want, index) => matches(want, actual[index]))
    );
  let index = 0;
  for (const got of actual) if (index < expected.length && matches(expected[index], got)) index++;
  return index === expected.length;
}

function classMatches(expected, actual, contains) {
  if (expected instanceof RegExp) {
    expected.lastIndex = 0;
    return expected.test(actual);
  }
  const tokens = (value) => String(value).split(/\s+/).filter(Boolean).sort();
  const wanted = tokens(expected);
  const got = tokens(actual);
  return contains
    ? wanted.every((token) => got.includes(token))
    : wanted.length === got.length && wanted.every((token, index) => token === got[index]);
}

function matchText(expected, got) {
  if (expected instanceof RegExp) {
    expected.lastIndex = 0;
    return expected.test(got);
  }
  if (Array.isArray(expected)) return expected.some((e) => matchText(e, got));
  return got === expected;
}
// Playwright normalizes whitespace in text assertions (nbsp→space, collapse, trim).
const normWs = (s) => String(s).replace(/ /g, " ").replace(/\s+/g, " ").trim();
const json = (x) => {
  try {
    return JSON.stringify(x);
  } catch {
    return String(x);
  }
};
function matchesThrownError(error, expected) {
  if (expected === undefined) return true;
  const message = String(error?.message ?? error);
  if (typeof expected === "string") return message.includes(expected);
  if (expected instanceof RegExp) return expected.test(message);
  if (expected instanceof Error) return error?.message === expected.message;
  if (typeof expected === "function") return error instanceof expected;
  throw new TypeError("toThrow expects a string, RegExp, Error or error constructor");
}

function deepEqual(a, b, strict = false) {
  return equalValues(a, b, strict);
}
function subset(want, got) {
  if (typeof want?.asymmetricMatch === "function") return want.asymmetricMatch(got);
  if (typeof want !== "object" || want == null) return deepEqual(want, got);
  if (got == null || typeof got !== "object") return false;
  if (Array.isArray(want)) {
    if (!Array.isArray(got) || want.length !== got.length) return false;
  } else if (Object.prototype.toString.call(want) !== "[object Object]") {
    return deepEqual(want, got);
  }
  return Reflect.ownKeys(want)
    .filter((key) => Object.prototype.propertyIsEnumerable.call(want, key))
    .every((key) => key in got && subset(want[key], got[key]));
}

export function expect(value) {
  if (value instanceof Locator) return new LocatorExpect(value, false);
  if (value instanceof Page) return new PageExpect(value, false);
  return new ValueExpect(value, false);
}
function softAssertion(assertion) {
  const failed = (error) => {
    const info = activeInfoStore.getStore();
    if (!info) throw error;
    info.errors.push(error);
    info.status = "failed";
  };
  return new Proxy(assertion, {
    get(target, property) {
      const value = Reflect.get(target, property);
      if (typeof value !== "function")
        return value && typeof value === "object" ? softAssertion(value) : value;
      return (...args) => {
        try {
          const result = value.apply(target, args);
          return result?.then ? result.catch(failed) : result;
        } catch (error) {
          return failed(error);
        }
      };
    },
  });
}
expect.soft = (value) => softAssertion(expect(value));
function pollAssertion(callback, options, negative = false) {
  if (typeof callback !== "function") throw new Error("expect.poll requires a callback");
  return new Proxy(
    {},
    {
      get(_target, property) {
        if (property === "not") return pollAssertion(callback, options, !negative);
        const matcher = new ValueExpect(undefined, negative)[property];
        if (typeof matcher !== "function") return undefined;
        return (...args) =>
          retryAssertion(async () => {
            const assertion = new ValueExpect(await callback(), negative);
            await assertion[property](...args);
          }, options);
      },
    },
  );
}
expect.poll = pollAssertion;
expect.configure = () => expect;
expect.extend = () => {};

function asymmetricFactory(match, inverse = false) {
  return { asymmetricMatch: (value) => Boolean(match(value)) !== inverse };
}
function asymmetricContaining(inverse) {
  return {
    objectContaining(sample) {
      if (sample == null || typeof sample !== "object")
        throw new TypeError("objectContaining requires an object");
      return asymmetricFactory(
        (value) =>
          value != null &&
          Reflect.ownKeys(sample).every(
            (key) => key in Object(value) && deepEqual(value[key], sample[key]),
          ),
        inverse,
      );
    },
    arrayContaining(sample) {
      if (!Array.isArray(sample)) throw new TypeError("arrayContaining requires an array");
      return asymmetricFactory(
        (value) =>
          Array.isArray(value) && sample.every((want) => value.some((got) => deepEqual(got, want))),
        inverse,
      );
    },
    stringContaining(sample) {
      if (typeof sample !== "string") throw new TypeError("stringContaining requires a string");
      return asymmetricFactory(
        (value) => typeof value === "string" && value.includes(sample),
        inverse,
      );
    },
    stringMatching(sample) {
      if (typeof sample !== "string" && !(sample instanceof RegExp))
        throw new TypeError("stringMatching requires a string or RegExp");
      const pattern = new RegExp(sample);
      return asymmetricFactory((value) => {
        pattern.lastIndex = 0;
        return typeof value === "string" && pattern.test(value);
      }, inverse);
    },
  };
}
Object.assign(expect, asymmetricContaining(false));
expect.not = asymmetricContaining(true);
expect.anything = () => asymmetricFactory((value) => value != null);
expect.any = (constructor) => {
  if (typeof constructor !== "function") throw new TypeError("any requires a constructor");
  const primitive = new Map([
    [String, "string"],
    [Number, "number"],
    [Boolean, "boolean"],
    [BigInt, "bigint"],
    [Symbol, "symbol"],
    [Function, "function"],
    [Object, "object"],
  ]).get(constructor);
  return asymmetricFactory(
    (value) =>
      (primitive && typeof value === primitive) ||
      (constructor.prototype && value instanceof constructor),
  );
};

// ---------------------------------------------------------------------------
// @playwright/test surface (drop-in over node:test) — fixtures, hooks, steps.
// ---------------------------------------------------------------------------
import { after, describe as nodeDescribe, test as nodeTest } from "node:test";
import { fixtureNames, openFixtures, teardownWorkerFixtures } from "./fixtures.mjs";
import {
  isolatedAttempt,
  executionOptions,
  executeIsolated,
  closeAttemptProcesses,
  validateTimeout,
} from "./attempt-process.mjs";
const attemptPlans = new Map();
const attemptOccurrences = new Map();
let workerAttempt = null;
const initializedWorkerScopes = new Set();
let activeWorkerScopes = [];

// A fixture set: each value is either a plain default or [fn, opts] / async fn
// of ({ ...fixtures }, use). We resolve them lazily and tear down after `use`.
function makeBaseFixtures() {
  return {
    browser: [async (_f, use) => use(browserStub), { scope: "worker" }],
    context: async (_f, use) => use(new BrowserContext()),
    page: async ({ context }, use) => {
      const ctx = context;
      const page = await ctx.newPage();
      try {
        await use(page);
      } finally {
        // Close the live V8 session — without this every test leaks an isolate,
        // and across a 100+-test serial run the bloat slows hydration enough to
        // miss the render budget (flaky "matched no elements" late in the suite).
        await page.close();
      }
    },
    request: async ({ baseURL }, use) => {
      const api = request.newContext({ baseURL });
      try {
        await use(api);
      } finally {
        await api.dispose();
      }
    },
    baseURL: async (_f, use) => use(process.env.TURBO_SHIM_BASE_URL ?? undefined),
  };
}

// Playwright's `test.skip` is overloaded: `skip(title, body)` declares a skipped
// test, but `skip(condition, description)` / `skip()` / `skip(fixturesCb, desc)`
// conditionally skip the enclosing scope. The shim used to treat every call as
// `(title, body)`, so a `skip(false, "reason")` (the common conditional form)
// registered a bogus skipped test. These helpers disambiguate.
const SKIP_SIGNAL = Symbol("turbo-shim-skip");
// Per-test-context storage keyed by the enclosing node:test frame — module-level
// mutables (an `activeT` ref, a plain skip-stack array) would race between
// concurrent tests, since node:test's runner interleaves their callbacks. The
// skip-stack store holds a shared array for the current describe chain (nested
// describes layer their own frame on top and restore on exit); the activeT
// store binds the node:test context to just the body of the running test.
const skipStackStore = new AsyncLocalStorage();
const activeTStore = new AsyncLocalStorage();
const activeAttemptStore = new AsyncLocalStorage();
const activeWorkerHookStore = new AsyncLocalStorage();
const rootSkipScope = { skip: false };
const rootTestHooks = new Map();
const activeInfoStore = new AsyncLocalStorage();
const getSkipStack = () => skipStackStore.getStore() ?? [];
const getActiveT = () => activeTStore.getStore() ?? null;
// Direct context access keeps regression tests out of node:test's registrar.
export const __internals = Object.freeze({
  skipStackStore,
  activeTStore,
  getSkipStack,
  getActiveT,
});
const describeSkipped = () => rootSkipScope.skip || getSkipStack().some((f) => f.skip);
const isDeclareForm = (args) => typeof args[0] === "string" && typeof args[1] === "function";
function skipConditionMet(args, fixtures = {}) {
  if (args.length === 0) return true; // bare test.skip() → always skip
  const c = args[0];
  if (typeof c === "function") {
    try {
      return !!c(fixtures);
    } catch {
      return false;
    }
  }
  return !!c;
}

function applySkip(args) {
  const attempt = activeAttemptStore.getStore();
  const workerHook = activeWorkerHookStore.getStore();
  const current = attempt ?? workerHook;
  const scope = getSkipStack().at(-1) ?? rootSkipScope;
  if (!current && typeof args[0] === "function") {
    (scope.guards ??= []).push(args);
    return;
  }
  if (!skipConditionMet(args, current?.fixtures.arg)) return;
  const reason = typeof args.at(-1) === "string" ? args.at(-1) : undefined;
  if (attempt) {
    attempt.info.status = "skipped";
    attempt.info.expectedStatus = "skipped";
    attempt.info.annotations.push({ type: "skip", description: reason });
    getActiveT()?.skip(reason);
    throw SKIP_SIGNAL;
  }
  if (!workerHook && getActiveT()) {
    getActiveT().skip(reason);
    throw SKIP_SIGNAL;
  }
  const target = workerHook?.scope ?? scope;
  target.skip = true;
  target.skipReason = reason;
  if (workerHook) throw SKIP_SIGNAL;
}

async function applyPlanGuards(plan, fixtures) {
  for (const scope of [rootSkipScope, ...plan.frames]) {
    for (const args of scope.guards ?? []) {
      await fixtures.resolve(fixtureNames(args[0]));
      applySkip(args);
    }
  }
}

let hasFocusedTests = false;
function markFocused() {
  hasFocusedTests = true;
  for (const frame of getSkipStack()) frame.containsFocus = true;
}
function focusedScopeAllowed(scope) {
  return !hasFocusedTests || !scope || scope.only || scope.containsFocus;
}

function cleanupErrors(error) {
  return error instanceof AggregateError ? error.errors.flatMap(cleanupErrors) : [error];
}

const rootCleanupHooks = new Map();
after(async () => {
  await closeAttemptProcesses();
  const errors = [];
  try {
    await runCleanupHooks(rootCleanupHooks.get("afterAll") ?? [], { browser: browserStub });
  } catch (error) {
    errors.push(...cleanupErrors(error));
  }
  try {
    await teardownWorkerFixtures();
  } catch (error) {
    errors.push(...cleanupErrors(error));
  }
  if (errors.length) throw new AggregateError(errors, "Worker cleanup failed");
});
async function runCleanupHooks(hooks, args) {
  const errors = [];
  for (const hook of hooks) {
    try {
      await hook(args);
    } catch (error) {
      errors.push(error);
    }
  }
  if (errors.length) throw new AggregateError(errors, "Cleanup hooks failed");
}

function registerTestHook(kind, fn) {
  const scope = getSkipStack().at(-1);
  const store = scope ? (scope.testHooks ??= new Map()) : rootTestHooks;
  if (!store.has(kind)) store.set(kind, []);
  store.get(kind).push(fn);
}

const rootFixtureUses = new Map();
function fixtureUses(keys, frames = getSkipStack()) {
  const result = {};
  for (const scope of [rootFixtureUses, ...frames.map((frame) => frame.fixtureUses)]) {
    for (const key of keys) Object.assign(result, scope?.get(key));
  }
  return result;
}

function recordTestError(info, error) {
  if (error instanceof AggregateError) {
    for (const nested of error.errors) recordTestError(info, nested);
    return;
  }
  if (error === SKIP_SIGNAL || info.errors.includes(error)) return;
  info.status = "failed";
  info.errors.push(error);
  info.error ??= error;
}

async function invokeTestBody(fn, arg, info, expectedFailure) {
  try {
    await activeInfoStore.run(info, () => fn(arg, info));
    if (info.errors.length) throw new AggregateError(info.errors, "Soft assertions failed");
  } catch (error) {
    if (error !== SKIP_SIGNAL) recordTestError(info, error);
    if (error === SKIP_SIGNAL || !expectedFailure) throw error;
    return;
  }
  if (expectedFailure) throw new Error("Expected test to fail, but it passed");
}

function planHooks(plan, kind, reverse = false) {
  const scopes = [rootTestHooks, ...plan.frames.map((frame) => frame.testHooks)];
  if (reverse) scopes.reverse();
  return scopes.flatMap((scope) => scope?.get(kind) ?? []);
}

async function invokeFixtureFunction(fn, fixtures, info) {
  await fixtures.resolve(fixtureNames(fn));
  return fn(fixtures.arg, info);
}

async function finishAttempt(plan, fixtures, info, errors) {
  for (const hook of planHooks(plan, "afterEach", true)) {
    try {
      await invokeFixtureFunction(hook, fixtures, info);
    } catch (error) {
      recordTestError(info, error);
      errors.push(error);
    }
  }
  try {
    await fixtures.teardown();
  } catch (error) {
    errors.push(error);
  }
}

async function executeAttempt(plan, t) {
  const info = makeTestInfo(plan.name);
  if (plan.expectedFailure) info.expectedStatus = "failed";
  const defs = { ...plan.extDefs, ...fixtureUses(plan.useKeys, plan.frames) };
  const fixtures = await openFixtures(plan.baseDefs, defs, info, (error) =>
    recordTestError(info, error),
  );
  return activeAttemptStore.run({ info, fixtures, plan }, async () => {
    const errors = [];
    try {
      await applyPlanGuards(plan, fixtures);
      await fixtures.resolveAutomatic();
      for (const hook of planHooks(plan, "beforeEach"))
        await invokeFixtureFunction(hook, fixtures, info);
      await fixtures.resolve(fixtureNames(plan.fn));
      await invokeTestBody(plan.fn, fixtures.arg, info, plan.expectedFailure);
    } catch (error) {
      if (error !== SKIP_SIGNAL) {
        recordTestError(info, error);
        errors.push(error);
      } else info.status = "skipped";
    } finally {
      await finishAttempt(plan, fixtures, info, errors);
    }
    if (errors.length === 1) throw errors[0];
    if (errors.length) throw new AggregateError(errors, "Test and cleanup failed");
  });
}

async function invokeWorkerHook(fn, plan) {
  const info = makeTestInfo("worker hook");
  const defs = { ...plan.extDefs, ...fixtureUses(plan.useKeys, plan.frames) };
  const fixtures = await openFixtures(plan.baseDefs, defs, info, undefined, true);
  const scope = plan.frames.at(-1) ?? rootSkipScope;
  await activeWorkerHookStore.run({ scope, info, fixtures }, async () => {
    try {
      await fixtures.resolveAutomatic();
      await invokeFixtureFunction(fn, fixtures, info);
    } catch (error) {
      if (error !== SKIP_SIGNAL) throw error;
    }
  });
}

function makeTestFn(baseDefs, extDefs = {}, parentUseKeys = []) {
  const useKey = Symbol("fixture use");
  const useKeys = [...parentUseKeys, useKey];
  const run = (name, fn, only = false, expectedFailure = false) => {
    const focused = only || getSkipStack().some((frame) => frame.only);
    if (only) markFocused();
    const frames = [...getSkipStack()];
    if (describeSkipped()) {
      nodeTest.skip(name, () => {});
      return;
    }
    const titleKey = JSON.stringify([...frames.map((frame) => frame.title), name]);
    const occurrence = attemptOccurrences.get(titleKey) ?? 0;
    attemptOccurrences.set(titleKey, occurrence + 1);
    const plan = {
      name,
      fn,
      baseDefs,
      extDefs,
      useKeys,
      frames,
      expectedFailure,
      id: `${titleKey}:${occurrence}`,
    };
    attemptPlans.set(plan.id, plan);
    if (isolatedAttempt) return;
    const register = only && process.execArgv.includes("--test-only") ? nodeTest.only : nodeTest;
    register(name, async (t) =>
      activeTStore.run(t, async () => {
        if (hasFocusedTests && !focused) {
          t.skip("excluded by focused tests");
          return;
        }
        const skippedScope = [rootSkipScope, ...frames].find((scope) => scope.skip);
        if (skippedScope) {
          t.skip(skippedScope.skipReason);
          return;
        }
        await executeIsolated(plan, t, executionOptions([rootSkipScope, ...frames]));
      }),
    );
  };
  run.describe = makeDescribe();
  run.skip = (...args) => {
    // Form `skip(title, body)` → declare a skipped test.
    if (isDeclareForm(args)) {
      nodeTest.skip(args[0], () => {});
      return;
    }
    applySkip(args);
  };
  run.only = (name, fn) => run(name, fn, true);
  run.fixme = run.skip;
  run.fail = (name, fn) => run(name, fn, false, true);
  run.slow = () => {
    const current = activeAttemptStore.getStore() ?? activeWorkerHookStore.getStore();
    const timeout =
      current?.info.timeout ?? executionOptions([rootSkipScope, ...getSkipStack()]).timeout;
    run.setTimeout(timeout * 3);
  };
  run.setTimeout = (timeout) => {
    validateTimeout(timeout);
    const current = activeAttemptStore.getStore() ?? activeWorkerHookStore.getStore();
    if (current) current.info.setTimeout(timeout);
    else ((getSkipStack().at(-1) ?? rootSkipScope).execution ??= {}).timeout = timeout;
  };
  run.use = (overrides) => {
    const frame = getSkipStack().at(-1);
    const scope = frame ? (frame.fixtureUses ??= new Map()) : rootFixtureUses;
    scope.set(useKey, { ...scope.get(useKey), ...overrides });
  };
  run.step = async (_name, body) => {
    const skipped = Symbol("step skipped");
    const info = {
      skip(condition = true, _description) {
        if (condition) throw skipped;
      },
    };
    try {
      return await body(info);
    } catch (error) {
      if (error !== skipped) throw error;
    }
  };
  run.step.skip = async () => {};
  run.info = () =>
    activeAttemptStore.getStore()?.info ??
    activeWorkerHookStore.getStore()?.info ??
    makeTestInfo("");
  run.beforeEach = (fn) => registerTestHook("beforeEach", fn);
  run.afterEach = (fn) => registerTestHook("afterEach", fn);
  run.beforeAll = (fn) => {
    const frames = [...getSkipStack()];
    const scope = frames.at(-1);
    const target = scope ?? rootSkipScope;
    (target.workerBefore ??= []).push(() =>
      invokeWorkerHook(fn, { baseDefs, extDefs, useKeys, frames }),
    );
  };
  run.afterAll = (fn) => {
    const frames = [...getSkipStack()];
    const target = frames.at(-1) ?? rootSkipScope;
    (target.workerAfter ??= []).push(() =>
      invokeWorkerHook(fn, { baseDefs, extDefs, useKeys, frames }),
    );
  };
  // A new fixture name extends; reusing a base/ext name OVERRIDES it but the
  // override still sees the prior value (Playwright's `page: ({page}) => …`).
  run.extend = (more) => makeTestFn(baseDefs, { ...extDefs, ...more }, useKeys);
  return run;
}

function makeDescribe() {
  // Push a skip frame around the describe body so a top-of-describe conditional
  // `test.skip(cond, …)` marks exactly the tests registered within it.
  //
  // Register a node:test SUITE (`describe`), not a `test`. A describe must be a
  // suite so the nested `test(...)` calls in its body register as awaited suite
  // members. Wrapping it in `nodeTest(...)` made the nested tests fire on the
  // global runner WHILE the parent test was still running, so node cancelled them
  // ("test did not finish before its parent and was cancelled") — and the dangling
  // async test (holding a live-session V8 isolate) was torn down at process exit,
  // which faulted (SIGBUS) on Linux. The Playwright describe body takes no args.
  const d = (name, fn, only = false) => {
    const focused = only || getSkipStack().some((frame) => frame.only);
    if (only) markFocused();
    if (isolatedAttempt)
      return skipStackStore.run(
        [...getSkipStack(), { title: name, skip: false, only: focused }],
        fn,
      );
    const register =
      only && process.execArgv.includes("--test-only") ? nodeDescribe.only : nodeDescribe;
    return register(name, async () =>
      // Each describe runs in its own stack store — nested describes layer on
      // top via a fresh array, and sibling describes never see each other's
      // frames even when node:test interleaves their bodies.
      skipStackStore.run(
        [...getSkipStack(), { title: name, skip: false, only: focused }],
        async () => fn(),
      ),
    );
  };
  d.skip = (name, fn) => nodeDescribe.skip(name, fn ?? (() => {}));
  d.only = (name, fn) => d(name, fn, true);
  d.serial = d;
  d.parallel = d;
  d.configure = (options) => {
    const scope = getSkipStack().at(-1) ?? rootSkipScope;
    if (options.timeout !== undefined) validateTimeout(options.timeout);
    if (
      options.retries !== undefined &&
      (!Number.isInteger(options.retries) || options.retries < 0)
    )
      throw new Error("Retries must be a non-negative integer");
    scope.execution = { ...scope.execution, ...options };
  };
  return d;
}

function makeTestInfo(title) {
  const info = {
    title,
    titlePath: [title],
    outputDir: "/tmp",
    outputPath: (...p) => ["/tmp", ...p].join("/"),
    snapshotPath: (...p) => ["/tmp", ...p].join("/"),
    attach: async (name, options) => {
      const attachment = { name, contentType: options.contentType ?? "application/octet-stream" };
      if (options.path !== undefined) {
        attachment.path = options.path;
        await readFile(options.path);
      } else
        attachment.body = Buffer.isBuffer(options.body)
          ? Buffer.from(options.body)
          : Buffer.from(String(options.body ?? ""));
      info.attachments.push(attachment);
    },
    attachments: [],
    skip: (...args) => applySkip(args),
    fixme: (...args) => applySkip(args),
    fail: () => {},
    slow: () => info.setTimeout(info.timeout * 3),
    timeout: workerAttempt?.timeout ?? 30000,
    setTimeout: (timeout) => {
      info.timeout = validateTimeout(timeout);
      if (workerAttempt) process.send?.({ kind: "timeout", timeout });
    },
    annotations: [],
    errors: [],
    status: "passed",
    expectedStatus: "passed",
    retry: workerAttempt?.retry ?? 0,
    project: { name: process.env.TURBO_PW_PROJECT_NAME ?? "turbo-surf" },
  };
  return info;
}

export const __attemptWorker = {
  async run(message) {
    workerAttempt = message;
    const plan = attemptPlans.get(message.id);
    if (!plan) throw new Error(`Unknown test attempt ${message.id}`);
    const scopes = [rootSkipScope, ...plan.frames];
    for (const scope of activeWorkerScopes.slice().reverse()) {
      if (!scopes.includes(scope)) {
        await runCleanupHooks(scope.workerAfter ?? [], {});
        initializedWorkerScopes.delete(scope);
      }
    }
    activeWorkerScopes = scopes;
    for (const scope of scopes) {
      if (!initializedWorkerScopes.has(scope)) {
        initializedWorkerScopes.add(scope);
        for (const hook of scope.workerBefore ?? []) await hook();
      }
    }
    const skipped = scopes.find((scope) => scope.skip);
    if (skipped) return { skipped: true, reason: skipped.skipReason };
    let reason;
    const t = {
      skip(value) {
        reason = value ?? "skipped";
      },
    };
    await activeTStore.run(t, () => executeAttempt(plan, t));
    return { skipped: reason !== undefined, reason };
  },
  async close() {
    const errors = [];
    for (const scope of [...initializedWorkerScopes].reverse()) {
      try {
        await runCleanupHooks(scope.workerAfter ?? [], {});
      } catch (error) {
        errors.push(error);
      }
    }
    try {
      await teardownWorkerFixtures();
    } catch (error) {
      errors.push(error);
    }
    if (errors.length) throw new AggregateError(errors, "Worker cleanup failed");
  },
};

export const test = makeTestFn(makeBaseFixtures());

/** `defineConfig` is identity — the Playwright CLI/config is unused under node:test. */
export const defineConfig = (config) => config;
/** Device descriptors are no-ops (no real viewport/UA emulation). */
export const devices = new Proxy({}, { get: () => ({}) });
/** APIRequestContext: fetch over the shared Rust client. The response carries
 * the RAW body (makeResponse.text/json), not a re-serialized DOM.
 *
 * When bound to a parent BrowserContext (via the internal `_browserContext`
 * option, which `page.request()` / `context.request()` pass in), reads/writes
 * share the context's cookie jar: outbound requests carry the current cookies,
 * `Set-Cookie` writes merge back in, so a login via `page.goto` is reflected
 * in a later `ctx.request.get('/api/...')`. */
async function encodeMultipart(fields) {
  const boundary = `----turbo-surf-${randomUUID()}`;
  const chunks = [];
  const escape = (value) =>
    String(value).replace(/\r/g, "%0D").replace(/\n/g, "%0A").replace(/"/g, "%22");
  const entries = fields instanceof FormData ? fields.entries() : Object.entries(fields);
  for (const [name, value] of entries) {
    let content = value;
    let filename;
    let type;
    if (value && typeof value === "object") {
      if (Buffer.isBuffer(value.buffer)) {
        content = value.buffer;
        filename = value.name;
        type = value.mimeType;
      } else if (typeof value.arrayBuffer === "function") {
        content = Buffer.from(await value.arrayBuffer());
        filename = value.name ?? "blob";
        type = value.type;
      } else if (value.path) {
        content = await readFile(value.path);
        filename = basename(String(value.path));
      } else {
        throw new Error("Unsupported multipart file value");
      }
    }
    let header = `--${boundary}\r\nContent-Disposition: form-data; name="${escape(name)}"`;
    if (filename !== undefined) {
      header += `; filename="${escape(filename)}"\r\nContent-Type: ${type || "application/octet-stream"}`;
    }
    chunks.push(
      Buffer.from(`${header}\r\n\r\n`),
      Buffer.isBuffer(content) ? content : Buffer.from(String(content)),
      Buffer.from("\r\n"),
    );
  }
  chunks.push(Buffer.from(`--${boundary}--\r\n`));
  return { body: Buffer.concat(chunks), type: `multipart/form-data; boundary=${boundary}` };
}

export const request = {
  // Synchronous (no await needed) — tests chain `request.newContext({...}).fetch(...)`.
  // Real Playwright returns a Promise here; the synchronous form also works at
  // call-site (`(await request.newContext()).fetch(...)` resolves fine because
  // awaiting a non-thenable returns the value unchanged).
  newContext(opts = {}) {
    const headers = opts.extraHTTPHeaders ? JSON.stringify(opts.extraHTTPHeaders) : null;
    // Honor the context `baseURL` like real Playwright: a relative request path
    // (e.g. `ctx.get('/api/backend/...')`) resolves against it. Without this the
    // schemeless URL reaches reqwest as-is and fails with "builder error".
    const base = opts.baseURL ?? null;
    const resolve = (url) => {
      if (!base) return url;
      try {
        return new URL(url, base).href;
      } catch {
        return url;
      }
    };
    // oxlint-disable no-underscore-dangle
    const bc = opts._browserContext ?? null;
    const maxBytes = responseBudget(opts.maxResponseBytes ?? bc?._maxResponseBytes);
    let cookies = JSON.stringify(opts.storageState?.cookies ?? []);
    const initialState =
      typeof opts.storageState === "string"
        ? readFile(opts.storageState, "utf8").then((text) => {
            cookies = JSON.stringify(JSON.parse(text).cookies ?? []);
          })
        : Promise.resolve();
    const requestState = native.requestStateNew?.(bc?._requestState);
    let disposed = false;
    const responses = new Set();
    const ensureOpen = () => {
      if (disposed) throw new Error("APIRequestContext has been disposed");
    };
    const optHeaders = {
      ...(opts.userAgent ? { "User-Agent": opts.userAgent } : {}),
      ...opts.extraHTTPHeaders,
    };
    const credentials = opts.httpCredentials;
    const authorization = credentials
      ? `Basic ${Buffer.from(`${credentials.username}:${credentials.password}`).toString("base64")}`
      : null;
    const credentialAllowed = (url) =>
      credentials &&
      (!credentials.origin || new URL(url).origin === new URL(credentials.origin).origin);
    // Resolve the context's `extraHTTPHeaders` live from the parent
    // BrowserContext (if bound) so a `context.setExtraHTTPHeaders(...)` call
    // after `request()` is cached takes effect on the next request — without
    // this, the cache snapshotted an empty header set at newContext time and
    // every request from the live context silently sent no custom headers.
    // Precedence: newContext({extraHTTPHeaders}) is the opt-in baseline,
    // overridden by the parent BrowserContext's LIVE headers so a later
    // `context.setExtraHTTPHeaders(...)` call (which mutates bc._headers) is
    // picked up on the next request — the headline fix for the cached
    // _requestCtx path. Per-call `o.headers` still overrides both below.
    // Note: the explicit `request.newContext({_browserContext: bc,
    // extraHTTPHeaders: X})` double-opt case lets bc win for overlapping keys,
    // which is a shim-private surface; direct callers don't hit it.
    const mergeHeaders = (...sources) =>
      Object.fromEntries(
        sources.flatMap((source) =>
          Object.entries(source ?? {}).map(([name, value]) => [name.toLowerCase(), value]),
        ),
      );
    const ctxHeaders = () => mergeHeaders(optHeaders, bc?._headers);
    // Encode a request `data`/body like Playwright: an object → JSON body with
    // `Content-Type: application/json` (unless the caller set one), a string → sent
    // as-is. Per-request `headers` merge over the context's `extraHTTPHeaders`.
    const encode = async (o = {}) => {
      const h = mergeHeaders(ctxHeaders(), o.headers);
      let body = null;
      if ([o.data, o.form, o.multipart].filter((value) => value !== undefined).length > 1) {
        throw new Error("data, form and multipart are mutually exclusive");
      }
      if (o.form !== undefined) {
        const entries = o.form instanceof FormData ? o.form.entries() : o.form;
        body = new URLSearchParams(entries).toString();
        if (!Object.keys(h).some((k) => k.toLowerCase() === "content-type")) {
          h["Content-Type"] = "application/x-www-form-urlencoded";
        }
      }
      if (o.multipart !== undefined) {
        const multipart = await encodeMultipart(o.multipart);
        body = multipart.body;
        if (!Object.keys(h).some((k) => k.toLowerCase() === "content-type")) {
          h["Content-Type"] = multipart.type;
        }
      }
      if (o.data !== null && o.data !== undefined) {
        if (typeof o.data === "string" || Buffer.isBuffer?.(o.data)) {
          body = Buffer.isBuffer(o.data) ? o.data : String(o.data);
          if (!Object.keys(h).some((k) => k.toLowerCase() === "content-type")) {
            h["Content-Type"] = "application/octet-stream";
          }
        } else {
          body = JSON.stringify(o.data);
          if (!Object.keys(h).some((k) => k.toLowerCase() === "content-type")) {
            h["Content-Type"] = "application/json";
          }
        }
      }
      const hdr = Object.keys(h).length ? JSON.stringify(h) : headers;
      return { body, hdr };
    };
    const call = async (url, method, o = {}) => {
      ensureOpen();
      await initialState;
      let target = resolve(url);
      if (o.params !== undefined) {
        const parsed = new URL(target);
        for (const [name, value] of new URLSearchParams(o.params))
          parsed.searchParams.append(name, value);
        target = parsed.href;
      }
      const maxRedirects = o.maxRedirects ?? opts.maxRedirects;
      if (maxRedirects !== undefined && (!Number.isInteger(maxRedirects) || maxRedirects < 0)) {
        throw new Error("maxRedirects must be a non-negative integer");
      }
      const credentialHeaders =
        credentialAllowed(target) && credentials.send === "always"
          ? { Authorization: authorization, ...o.headers }
          : o.headers;
      const { body, hdr } = await encode({ ...o, headers: credentialHeaders });
      const inCookies = bc ? bc._cookies : cookies;
      let raw = await native.fetchWithCookies(
        target,
        inCookies,
        method ?? null,
        Buffer.isBuffer(body) ? null : body,
        hdr,
        false,
        requestState,
        null,
        Buffer.isBuffer(body) ? body : null,
        maxRedirects,
        true,
        o.timeout ?? opts.timeout ?? null,
        maxBytes,
      );
      let parsed = JSON.parse(raw);
      if (
        parsed.status === 401 &&
        credentialAllowed(parsed.finalUrl ?? target) &&
        credentials.send !== "always" &&
        parsed.headers?.some(
          ([name, value]) =>
            name.toLowerCase() === "www-authenticate" && /^Basic(?:\s|$)/i.test(value),
        )
      ) {
        const authHeaders = JSON.parse(hdr ?? "{}");
        authHeaders.Authorization = authorization;
        raw = await native.fetchWithCookies(
          target,
          inCookies,
          method ?? null,
          Buffer.isBuffer(body) ? null : body,
          JSON.stringify(authHeaders),
          false,
          requestState,
          null,
          Buffer.isBuffer(body) ? body : null,
          maxRedirects,
          true,
          o.timeout ?? opts.timeout ?? null,
          maxBytes,
        );
        parsed = JSON.parse(raw);
      }
      // Merge Set-Cookie writes back into the parent BrowserContext's jar so
      // the next page.goto / page.request / context.request sees them.
      if (typeof parsed.cookies === "string") {
        if (bc) bc._cookies = parsed.cookies;
        else cookies = parsed.cookies;
      }
      // oxlint-enable no-underscore-dangle
      ensureOpen();
      if (
        (o.failOnStatusCode ?? opts.failOnStatusCode) &&
        (parsed.status < 200 || parsed.status >= 400)
      ) {
        throw new Error(`API request failed with status ${parsed.status}`);
      }
      responses.add(parsed);
      return makeResponse(parsed, ensureOpen);
    };
    return {
      get: (url, o = {}) => call(url, null, o),
      post: (url, o = {}) => call(url, "POST", o),
      put: (url, o = {}) => call(url, "PUT", o),
      patch: (url, o = {}) => call(url, "PATCH", o),
      delete: (url, o = {}) => call(url, "DELETE", o),
      head: (url, o = {}) => call(url, "HEAD", o),
      fetch: (url, o = {}) => call(url, o.method ?? null, o),
      async storageState(o = {}) {
        ensureOpen();
        await initialState;
        const state = { cookies: JSON.parse(bc ? bc._cookies : cookies), origins: [] };
        if (o.path) await writeFile(o.path, JSON.stringify(state));
        return state;
      },
      async dispose() {
        if (disposed) return;
        disposed = true;
        for (const response of responses) {
          response.html = null;
          response.bodyBytes = null;
        }
        responses.clear();
        cookies = "[]";
        if (requestState != null) native.requestStateClose(requestState);
      },
    };
  },
};

export default { test, expect, chromium, firefox, webkit, newPage, defineConfig, devices, request };
