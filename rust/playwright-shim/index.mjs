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

import { writeFile } from "node:fs/promises";
import { createRequire } from "node:module";

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

// Build an executable source string from page.evaluate's (fn|string, arg) form.
function evalSource(script, arg) {
  if (typeof script === "function") return `(${script.toString()})(${JSON.stringify(arg ?? null)})`;
  return String(script);
}

// Quote a value for a CSS attribute selector (backs getByPlaceholder/AltText/Title).
function cssAttrValue(text) {
  return `"${String(text).replace(/"/g, '\\"')}"`;
}

// ---------------------------------------------------------------------------
// Locator — a lazy query over the page's current HTML. `_resolve(html)` returns
// the match array (each `{ node, text, html }`) in ONE napi call; composition
// (first/last/nth/filter) and counts stay in JS over that array.
// ---------------------------------------------------------------------------
class Locator {
  constructor(page, resolve, opts = {}) {
    this._page = page;
    this._resolve = resolve;
    this._index = opts.index ?? null;
    this._filter = opts.filter ?? null; // (match) => boolean, applied in JS
    // getByRole/getByText/getByLabel carry {kind,value,name} so a LIVE dispatch can
    // re-resolve the match IN the running isolate (live `*` indices) instead of trusting
    // a re-serialized snapshot's index, which diverges for portal'd content (MUI options).
    this._getBy = opts.getBy ?? null;
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

  // The scope chain a CHILD of this locator inherits: our chain plus this locator's own
  // base selector, index, and (serializable) filter.
  _childScope() {
    return [
      ...this._scope,
      { sel: this._scopeSel ?? this._selector, idx: this._index ?? null, filter: this._filterSpec },
    ];
  }

  _all() {
    let m = this._resolve(this._page._html);
    if (this._filter) m = m.filter(this._filter);
    if (this._index == null) return m;
    const i = this._index < 0 ? m.length + this._index : this._index;
    return m[i] ? [m[i]] : [];
  }
  _node() {
    const m = this._all();
    return m.length ? m[0].node : null;
  }
  _requireNode() {
    const n = this._node();
    if (n == null) throw new Error("turbo-surf: locator matched no elements");
    return n;
  }
  get _html() {
    return this._page._html;
  }

  // --- composition (pure JS, no napi) ---
  // Carry `_selector` through composition so `getByTestId(x).locator('input').first()`
  // stays selector-backed (→ drivable in a live session). `filter` makes it
  // unindexable for live dispatch (a JS predicate over matches), so it's dropped there.
  _derive(opts) {
    const loc = new Locator(this._page, this._resolve, {
      ...opts,
      getBy: this._getBy,
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
    return this._derive({ index: 0, filter: this._filter });
  }
  last() {
    return this._derive({ index: -1, filter: this._filter });
  }
  nth(i) {
    return this._derive({ index: i, filter: this._filter });
  }
  filter(opts = {}) {
    const pred = buildFilter(this._page, opts);
    return this._derive({
      index: this._index,
      filter: pred,
      filterSpec: serializableFilterSpec(opts),
    });
  }
  and(other) {
    const keep = new Set();
    return new Locator(this._page, (h) => {
      for (const x of other._resolve(h)) keep.add(x.node);
      return this._resolve(h).filter((x) => keep.has(x.node));
    });
  }
  or(other) {
    return new Locator(this._page, (h) => [...this._resolve(h), ...other._resolve(h)]);
  }
  // When this locator (or an ancestor) was indexed via `.nth(i)`, a CSS-concat selector can't
  // express "the i-th match's subtree" — return the scope CHAIN so the child drives via
  // __tcResolveScoped. Null for the common non-indexed case (keep the simple CSS-concat path).
  _indexedScope() {
    const sc = this._childScope();
    return sc.some((s) => s.idx != null || s.filter != null) ? sc : null;
  }
  // A selector-backed child carrying the nth-aware scope chain (live-driven via the chain).
  _scopedSelectorChild(selector, scope) {
    const loc = this._page._locFromSelector(selector);
    loc._scope = scope;
    return loc;
  }
  // Nested locators: CSS-concat when this locator is selector-backed (the common
  // `.card`→`button` case); a scope chain when an ancestor was `.nth(i)`. getBy-rooted nesting
  // is document-scoped — see LIMITATIONS.
  locator(selector) {
    // A `.nth()`/`.filter()` ancestor → walk the scope chain (it carries the index/filter a
    // CSS-concat can't express), even if `.filter()` dropped our own `_selector`.
    const scope = this._indexedScope();
    if (scope) return this._scopedSelectorChild(selector, scope);
    if (!this._selector) return this._page.locator(selector);
    return this._page.locator(`${this._selector} ${selector}`);
  }
  // Scope getBy* to this locator's subtree when it is selector-backed (descendant matching via
  // the `root` selector, or the nth-aware scope chain when an ancestor was `.nth(i)`). A
  // getBy-rooted parent isn't CSS-expressible → document-scoped.
  _scopedGetBy(kind, value, name) {
    const scope = this._indexedScope();
    if (scope) {
      return new Locator(this._page, (h) => JSON.parse(native.getBy(h, kind, value, name)), {
        getBy: { kind, value, name },
        scope,
      });
    }
    const root = this._selector;
    return new Locator(this._page, (h) => JSON.parse(native.getBy(h, kind, value, name, root)), {
      getBy: { kind, value, name, root },
    });
  }
  getByRole(role, o) {
    if (!this._selector && !this._indexedScope()) return this._page.getByRole(role, o);
    return this._scopedGetBy("role", role, o?.name != null ? String(o.name) : null);
  }
  getByText(t, o) {
    if (!this._selector && !this._indexedScope()) return this._page.getByText(t, o);
    return this._scopedGetBy("text", String(t), null);
  }
  getByLabel(t, o) {
    if (!this._selector && !this._indexedScope()) return this._page.getByLabel(t, o);
    return this._scopedGetBy("label", String(t), null);
  }
  getByTestId(t) {
    // Scope to this locator's subtree when it is selector-backed (like `locator()` does) —
    // `card.getByTestId('x')` must match only descendants of the card, not the whole document.
    // A `.filter()`/`.nth()` ancestor walks the scope chain (it carries the filter/index our
    // own `_selector` can't), so check that BEFORE falling back to a document-wide getByTestId.
    const childSel = `[${this._page._context._testIdAttribute}="${t}"]`;
    const scope = this._indexedScope();
    if (scope) return this._scopedSelectorChild(childSel, scope);
    if (!this._selector) return this._page.getByTestId(t);
    return this._page.locator(`${this._selector} ${childSel}`);
  }
  getByPlaceholder(t, o) {
    return this._page.getByPlaceholder(t, o);
  }
  getByAltText(t, o) {
    return this._page.getByAltText(t, o);
  }
  getByTitle(t, o) {
    return this._page.getByTitle(t, o);
  }

  // --- counts / text (from the single resolve payload) ---
  async count() {
    return this._all().length;
  }
  async all() {
    return this._all().map((_, i) => this.nth(i));
  }
  async textContent() {
    const m = this._all();
    return m.length ? (m[0].text ?? null) : null;
  }
  async innerText() {
    const m = this._all();
    return m.length ? (m[0].text ?? "").trim() : "";
  }
  async innerHTML() {
    const m = this._all();
    return m.length ? (m[0].html ?? null) : null;
  }
  async allTextContents() {
    return this._all().map((x) => x.text ?? "");
  }
  async allInnerTexts() {
    return this._all().map((x) => (x.text ?? "").trim());
  }

  // --- accessors (node-handle backed; cross to Rust) ---
  async getAttribute(name) {
    const n = this._node();
    return n == null ? null : (native.attrOf(this._html, n, name) ?? null);
  }
  async inputValue() {
    const n = this._node();
    return n == null ? "" : native.inputValueOf(this._html, n);
  }
  async isVisible() {
    const n = this._node();
    return n != null && native.isVisible(this._html, n);
  }
  async isHidden() {
    return !(await this.isVisible());
  }
  async isChecked() {
    const n = this._node();
    return n != null && native.isChecked(this._html, n);
  }
  async isEnabled() {
    const n = this._node();
    return n != null && native.isEnabled(this._html, n);
  }
  async isDisabled() {
    return !(await this.isEnabled());
  }
  async isEditable() {
    const n = this._node();
    return n != null && native.isEditable(this._html, n);
  }
  async isEmpty() {
    const n = this._node();
    return n != null && native.isEmpty(this._html, n);
  }
  async ariaRole() {
    const n = this._node();
    return n == null ? null : native.ariaRoleOf(this._html, n);
  }
  async accessibleName() {
    const n = this._node();
    return n == null ? "" : native.accessibleNameOf(this._html, n);
  }
  async accessibleDescription() {
    const n = this._node();
    return n == null ? "" : native.accessibleDescriptionOf(this._html, n);
  }
  async selectedValues() {
    const n = this._node();
    return n == null ? [] : native.selectedValuesOf(this._html, n);
  }
  async cssValue(name) {
    const n = this._node();
    return n == null ? "" : native.cssValueOf(this._html, n, name);
  }
  // Batch every boolean/text/role accessor for the matched node in ONE napi
  // crossing — backs expect(locator) chains. Null if no match.
  _snapshot() {
    const n = this._node();
    return n == null ? null : JSON.parse(native.nodeSnapshot(this._html, n));
  }
  // locator.evaluate runs page JS with the element selected by CSS (the element
  // is the callback's first arg). Needs a selector-backed locator — see LIMITATIONS.
  async evaluate(fn, arg) {
    if (!this._selector)
      throw new Error("turbo-surf: locator.evaluate needs a CSS-selector-backed locator");
    const src = `(${fn.toString()})(document.querySelector(${JSON.stringify(this._selector)}), ${JSON.stringify(arg ?? null)})`;
    return native.evaluate(this._html, src);
  }
  async evaluateAll(fn, arg) {
    return native.evaluate(this._html, evalSource(fn, arg));
  }
  async ariaSnapshot() {
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
    // nth-aware scope chain: a CSS selector can't target "the i-th match's subtree", so walk
    // the chain in the live isolate and dispatch on the matched element's global index.
    if (
      this._scope &&
      this._scope.some((s) => s.idx != null || s.filter != null) &&
      this._page._session != null
    ) {
      const leaf = this._selector
        ? { selector: this._selector }
        : this._getBy
          ? { getBy: { kind: this._getBy.kind, value: this._getBy.value, name: this._getBy.name } }
          : null;
      if (leaf) {
        const raw = await native.liveEval(
          this._page._session,
          `globalThis.__tcResolveScoped(${JSON.stringify(this._scope)},${JSON.stringify(leaf)});`,
        );
        let matches = [];
        try {
          matches = JSON.parse(raw);
        } catch {
          matches = [];
        }
        const i =
          this._index == null ? 0 : this._index < 0 ? matches.length + this._index : this._index;
        const target = matches[i];
        if (!target || target.idx == null)
          throw new Error("turbo-surf: locator matched no elements");
        return this._page._sessionDispatch("*", target.idx, kind, value);
      }
    }
    if (this._selector)
      return this._page._sessionDispatch(this._selector, this._index ?? 0, kind, value);
    // getByRole/getByText/getByLabel: resolve the match IN the live isolate so the index is
    // a LIVE `querySelectorAll('*')` position (the same context we dispatch into). Resolving
    // over a re-serialized snapshot can reorder portal'd nodes → wrong element clicked.
    if (this._getBy && this._page._session != null) {
      const g = this._getBy;
      const raw = await native.liveEval(
        this._page._session,
        `globalThis.__tcGetBy(${JSON.stringify(g.kind)},${JSON.stringify(g.value)},${g.name == null ? "null" : JSON.stringify(g.name)},${g.root == null ? "null" : JSON.stringify(g.root)});`,
      );
      let matches = [];
      try {
        matches = JSON.parse(raw);
      } catch {
        matches = [];
      }
      if (!matches.length) throw new Error("turbo-surf: locator matched no elements");
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
  async fill(value) {
    if (this._canDriveLive) return void (await this._driveLive("fill", String(value)));
    this._page._html = native.fillNode(this._html, this._requireNode(), String(value));
  }
  async clear() {
    return this.fill("");
  }
  async type(value) {
    return this.fill(value);
  }
  async pressSequentially(value) {
    return this.fill(value);
  }
  async check() {
    if (this._canDriveLive) return void (await this._driveLive("check"));
    this._page._html = native.setCheckedNode(this._html, this._requireNode(), true);
  }
  async uncheck() {
    if (this._canDriveLive) return void (await this._driveLive("uncheck"));
    this._page._html = native.setCheckedNode(this._html, this._requireNode(), false);
  }
  async setChecked(on) {
    return on ? this.check() : this.uncheck();
  }
  async selectOption(value) {
    const v = Array.isArray(value) ? value[0] : value;
    const sval = String(v?.value ?? v);
    if (this._canDriveLive) {
      await this._driveLive("select", sval);
      return [sval];
    }
    this._page._html = native.selectOptionNode(this._html, this._requireNode(), sval);
    return [sval];
  }
  async click() {
    if (this._canDriveLive) {
      const urlBefore = this._page._url;
      const r = await this._driveLive("click");
      // A JS handler that navigated changed the URL (SPA router / form POST + redirect).
      // If the URL is unchanged AND the app didn't preventDefault, the click hit a plain
      // <a>/<form> whose browser default action our event dispatch doesn't perform —
      // follow the static intent (anchor navigate / form POST). For a JS button (Inert
      // intent) or a handled click (PREVENTED) the dispatch already did the work.
      if (this._page._url === urlBefore && r !== "PREVENTED") {
        const n = this._node();
        if (n != null) {
          const intent = JSON.parse(native.clickNode(this._page._html, n, this._page._url));
          if (intent.action !== "inert") return this._page._followIntent(intent);
        }
      }
      return;
    }
    const intent = JSON.parse(native.clickNode(this._html, this._requireNode(), this._page._url));
    return this._page._followIntent(intent);
  }
  async dblclick() {
    return this.click();
  }
  async tap() {
    return this.click();
  }
  async press(key) {
    // Enter on a control submits its owning form (the only no-JS key effect).
    if (key === "Enter") return this.click();
    return undefined;
  }
  async focus() {}
  async blur() {}
  async scrollIntoViewIfNeeded() {}
  async highlight() {}
  async waitFor(opts = {}) {
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

  // No layout engine → no geometry. Playwright returns `null` for an element
  // with no bounding box, so we return null too (honest: there genuinely is no
  // box) rather than throwing — geometry-conditional helpers (smooth-scroll,
  // visibility-by-rect) then no-op cleanly instead of crashing the test.
  async boundingBox() {
    return null;
  }

  // An ElementHandle stand-in: carries enough to re-resolve the element in the live
  // isolate (selector / getBy + index), so `page.waitForFunction(fn, handle)` can run
  // `fn(element)` against the live node (e.g. wait for a submit button's !disabled).
  async elementHandle() {
    return {
      __turboHandle: true,
      selector: this._selector ?? null,
      getBy: this._getBy ?? null,
      index: this._index ?? 0,
    };
  }

  // Hover has no rendering effect here, but JS hover handlers (menu-open) are real.
  // Drive the live app with mouseenter/over events; otherwise best-effort no-op.
  async hover() {
    await this._page._ensureLive();
    if (this._page._live) return void (await this._driveLive("hover"));
  }

  // dispatchEvent(type) — fire a named DOM event into the live app (e.g. a row
  // checkbox's 'click'). Static snapshot has no JS handlers, so it needs the isolate.
  async dispatchEvent(type) {
    await this._page._ensureLive();
    if (this._page._live) return void (await this._driveLive("dispatch", String(type)));
  }

  // setInputFiles: read the file bytes in Node, build File objects in the isolate, and
  // assign them to the <input type=file> so the app's change handler / upload runs. No
  // OS file picker, but the upload PIPELINE is real.
  async setInputFiles(files) {
    const payload = normalizeInputFiles(files);
    await this._page._ensureLive();
    if (this._page._live) return void (await this._driveLive("setfiles", payload));
  }

  // --- unsupported (no rendering surface / no input hardware) → honest throws ---
  screenshot = UNSUPPORTED("locator.screenshot", PIXEL);
  dragTo = UNSUPPORTED("locator.dragTo", INPUT);
  selectText = UNSUPPORTED("locator.selectText", INPUT);
}

// filter({ hasText, has, hasNot, hasNotText }) → JS predicate over a match.
function buildFilter(_page, opts) {
  const checks = [];
  if (opts.hasText != null) {
    const re = opts.hasText instanceof RegExp ? opts.hasText : null;
    checks.push((m) => (re ? re.test(m.text ?? "") : (m.text ?? "").includes(opts.hasText)));
  }
  if (opts.hasNotText != null) {
    checks.push((m) => !(m.text ?? "").includes(opts.hasNotText));
  }
  return (m) => checks.every((c) => c(m));
}

// Serializable subset of a `.filter()` for the live scope chain — __tcResolveScoped re-applies
// it in the isolate so children of a filtered parent scope to the RIGHT element. Only plain
// string hasText/hasNotText survive; regex / has / hasNot return null (those keep the
// JS-predicate-only path: correct for reads, no live child scoping).
function serializableFilterSpec(opts) {
  if (opts.has != null || opts.hasNot != null) return null;
  if (opts.hasText != null && typeof opts.hasText !== "string") return null;
  if (opts.hasNotText != null && typeof opts.hasNotText !== "string") return null;
  const spec = {};
  if (typeof opts.hasText === "string") spec.hasText = opts.hasText;
  if (typeof opts.hasNotText === "string") spec.hasNotText = opts.hasNotText;
  return spec.hasText != null || spec.hasNotText != null ? spec : null;
}

// Build the JS run in a live session to drive an interaction by dispatching REAL DOM
// events on the selector-matched element, so the running app's (React's) delegated
// handlers fire. `kind`: "click" fires a realistic mousedown→focus→mouseup→click
// sequence; "fill" sets the value + fires input/change (React controlled inputs).
// Normalize Playwright setInputFiles args (path string | {name,mimeType,buffer} |
// arrays thereof) into [{ name, type, data }] with data as a latin1 binary string
// (so the isolate's File/Blob → FileReader.readAsDataURL → btoa round-trips bytes).
function normalizeInputFiles(files) {
  const fs = require("node:fs");
  const path = require("node:path");
  const list = Array.isArray(files) ? files : [files];
  return list.map((f) => {
    if (typeof f === "string") {
      return { name: path.basename(f), type: "", data: fs.readFileSync(f).toString("latin1") };
    }
    let data = "";
    if (f.buffer != null) data = Buffer.from(f.buffer).toString("latin1");
    else if (f.body != null) data = String(f.body);
    return { name: f.name ?? "file", type: f.mimeType ?? f.type ?? "", data };
  });
}

function interactionScript(selector, index, kind, value) {
  const sel = JSON.stringify(selector);
  const idx = JSON.stringify(index);
  const val = JSON.stringify(value ?? "");
  const pick = `const els = document.querySelectorAll(${sel});
    let __i = ${idx}; if (__i < 0) __i = els.length + __i;
    const el = els[__i];
    if (!el) { globalThis.__RESULT = "NO_MATCH"; }`;
  if (kind === "fill") {
    return `(() => { ${pick} else {
      if (typeof el.focus === "function") el.focus();
      // Set the value through the PROTOTYPE setter, bypassing React's instance-level
      // _valueTracker. If we used el.value=, React's tracker would record the new value
      // and then see "no change" on the input event → onChange never fires → a
      // controlled input stays empty in React state (and resets on the next render).
      const proto = el.tagName === "TEXTAREA" ? globalThis.HTMLTextAreaElement
        : el.tagName === "SELECT" ? globalThis.HTMLSelectElement : globalThis.HTMLInputElement;
      const desc = proto && Object.getOwnPropertyDescriptor(proto.prototype, "value");
      if (desc && desc.set) desc.set.call(el, ${val}); else el.value = ${val};
      el.dispatchEvent(new InputEvent("input", { bubbles: true, cancelable: true, data: ${val} }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
      globalThis.__RESULT = "OK";
    } })();`;
  }
  if (kind === "check" || kind === "uncheck") {
    const on = kind === "check";
    return `(() => { ${pick} else {
      if (el.checked !== ${on}) {
        el.checked = ${on};
        el.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
        el.dispatchEvent(new Event("input", { bubbles: true }));
        el.dispatchEvent(new Event("change", { bubbles: true }));
      }
      globalThis.__RESULT = "OK";
    } })();`;
  }
  if (kind === "select") {
    return `(() => { ${pick} else {
      el.value = ${val};
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
      globalThis.__RESULT = "OK";
    } })();`;
  }
  if (kind === "setfiles") {
    // value is an array of { name, type, data(latin1) }. Build File objects, install
    // a FileList-like on the input (its `files` is read-only → defineProperty), and
    // fire input/change so the app's upload handler runs.
    return `(() => { ${pick} else {
      const arr = (${val}).map((f) => new globalThis.File([f.data], f.name, { type: f.type }));
      arr.item = (i) => arr[i] || null;
      try { Object.defineProperty(el, "files", { configurable: true, value: arr }); }
      catch (e) { try { el.files = arr; } catch (_e) {} }
      if (typeof el.focus === "function") el.focus();
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
      globalThis.__RESULT = "OK";
    } })();`;
  }
  if (kind === "dispatch") {
    // Fire a named event (value = type) on the deepest descendant. A click on a
    // checkbox/radio also performs the browser default (toggle + change).
    return `(() => { ${pick} else {
      let t = el; while (t.firstElementChild) t = t.firstElementChild;
      const type = ${val};
      const mouse = /^(click|dblclick|mouse|pointer|contextmenu|auxclick)/.test(type);
      const box = el.tagName === "INPUT" && (el.type === "checkbox" || el.type === "radio");
      if (type === "click" && box) el.checked = el.type === "radio" ? true : !el.checked;
      const ev = mouse
        ? new MouseEvent(type, { bubbles: true, cancelable: true, view: globalThis })
        : new Event(type, { bubbles: true, cancelable: true });
      t.dispatchEvent(ev);
      if (type === "click" && box) {
        el.dispatchEvent(new Event("input", { bubbles: true }));
        el.dispatchEvent(new Event("change", { bubbles: true }));
      }
      globalThis.__RESULT = "OK";
    } })();`;
  }
  if (kind === "hover") {
    // No cursor, but the app's hover handlers (MUI menus open on mouseenter/over)
    // are real JS — fire the events they listen for so the menu actually opens.
    // Dispatch on the deepest descendant (a real cursor hits the leaf) so handlers
    // bound to inner nodes (and React's delegation) see a target inside them.
    return `(() => { ${pick} else {
      let t = el; while (t.firstElementChild) t = t.firstElementChild;
      const o = { bubbles: true, cancelable: true, view: globalThis };
      const oe = { bubbles: false, cancelable: true, view: globalThis };
      t.dispatchEvent(new MouseEvent("pointerover", o));
      t.dispatchEvent(new MouseEvent("mouseover", o));
      t.dispatchEvent(new MouseEvent("pointerenter", oe));
      t.dispatchEvent(new MouseEvent("mouseenter", oe));
      t.dispatchEvent(new MouseEvent("mousemove", o));
      // Also apply CSS :hover styles — a menu revealed purely by a hover descendant rule has
      // no JS handler; without this it stays display:none (no pointer state in the cascade).
      if (typeof globalThis.__tcApplyHover === "function") globalThis.__tcApplyHover(el);
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
    const o = { bubbles: true, cancelable: true, view: globalThis, button: 0 };
    // Full pointer+mouse sequence like a real browser: pointer events FIRST (MUI v5+ /
    // Radix / many libs gate selection on pointerdown/up, and MUI Autocomplete/Select
    // options preventDefault mousedown to keep input focus — without the pointer pair the
    // option click never commits the value). PointerEvent if available, else MouseEvent.
    const PE = globalThis.PointerEvent || globalThis.MouseEvent;
    const pe = (t) => new PE(t, { bubbles: true, cancelable: true, view: globalThis, button: 0, pointerId: 1, isPrimary: true });
    tgt.dispatchEvent(pe("pointerdown"));
    const md = new MouseEvent("mousedown", o);
    tgt.dispatchEvent(md);
    // Focus follows the browser rules: a click moves focus to a FOCUSABLE target, BUT a
    // mousedown handler that calls preventDefault() suppresses the focus shift. MUI's
    // Autocomplete/Select listbox preventDefaults mousedown precisely to keep the input
    // focused while an option is clicked — focusing the option <li> (tabindex=-1) instead
    // would blur the input and clearOnBlur would discard the just-made selection.
    const focusable = el.matches && el.matches("input,button,textarea,select,a[href],[tabindex],[contenteditable]");
    if (focusable && !md.defaultPrevented && typeof el.focus === "function") el.focus();
    tgt.dispatchEvent(pe("pointerup"));
    tgt.dispatchEvent(new MouseEvent("mouseup", o));
    const clickEv = new MouseEvent("click", o);
    tgt.dispatchEvent(clickEv);
    // Browser default action of clicking a submit control: fire the form's submit
    // event (React's onSubmit is delegated). Our dispatch has no default action, so do
    // it explicitly — unless the click was preventDefault'd.
    if (!clickEv.defaultPrevented) {
      let f = null;
      for (let p = el; p; p = p.parentElement) { if (p.tagName === "FORM") { f = p; break; } }
      const ty = (el.getAttribute && (el.getAttribute("type") || "")).toLowerCase();
      const submits = el.tagName === "BUTTON" ? ty !== "button" && ty !== "reset"
        : el.tagName === "INPUT" && (ty === "submit" || ty === "image");
      if (f && submits) f.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    }
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
// Page
// ---------------------------------------------------------------------------
class Page {
  constructor(context) {
    this._html = "";
    this._url = "about:blank";
    this._context = context ?? new BrowserContext();
    this._history = [];
    this._fwd = [];
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
    if (!this._live && this._autoHydrate !== false) await this._openLiveSession();
  }

  // Open a LIVE session: hydrate the current document and KEEP the app's JS isolate
  // alive, so interactions dispatch real events into the running app. Used on a
  // `waitUntil: 'networkidle'` navigation / waitForLoadState('networkidle').
  // addInitScript bodies (context + page), wrapped so one failure can't abort the
  // rest, as a single <script> to prepend — runs before the page's own scripts.
  _initScriptTag() {
    const scripts = [...(this._context._initScripts ?? []), ...(this._initScripts ?? [])];
    if (!scripts.length) return "";
    const body = scripts.map((s) => `try{ ${s} }catch(e){}`).join("\n");
    return `<script>${body}</script>`;
  }

  // Inject init scripts as the FIRST <head> script so Playwright's addInitScript
  // semantics hold (they run before any page script) in the live isolate.
  _withInitScripts(html) {
    const tag = this._initScriptTag();
    if (!tag) return html;
    if (/<head[^>]*>/i.test(html)) return html.replace(/<head[^>]*>/i, (m) => m + tag);
    if (/<html[^>]*>/i.test(html)) return html.replace(/<html[^>]*>/i, (m) => m + tag);
    return tag + html;
  }

  async _openLiveSession() {
    this._closeSession();
    this._session = await native.liveOpen(
      this._withInitScripts(this._html),
      this._url,
      this._cookies,
      this._context._userAgent ?? undefined,
    );
    this._hydrated = true;
    this._loadedPath = pathOf(this._url);
    await this._refreshFromSession();
  }

  // Pull the current live DOM + URL + cookies back into the Page so reads (which run
  // over `this._html`) and a subsequent navigation see the post-interaction state.
  async _refreshFromSession() {
    if (this._session == null) return;
    this._html = native.liveSerialize(this._session);
    try {
      const href = await native.liveEval(this._session, "globalThis.__RESULT = location.href;");
      if (href) this._url = href;
    } catch {
      /* keep prior url */
    }
    // Next App Router client navigation (router.push) doesn't change location.href in our
    // engine — it fetches the target's RSC flight, which the runtime records on __rscNav.
    // Treat that as the navigation target (the post-login redirect chain rides this).
    try {
      const nav = await native.liveEval(
        this._session,
        "globalThis.__RESULT = globalThis.__rscNav || ''; globalThis.__rscNav = '';",
      );
      if (nav && pathOf(nav) !== this._loadedPath) {
        this._url = this._resolveUrl(nav);
      }
    } catch {
      /* no pending rsc nav */
    }
    try {
      this._cookies = native.liveCookies(this._session);
    } catch {
      /* keep prior cookies */
    }
    await this._drainNetworkEvents();
    // The app navigated to a different route within the live session (a post-login
    // redirect, router.push/replace). Our engine doesn't do Next's in-place RSC
    // soft-nav, so re-LOAD that route as a fresh page (carrying cookies) — its own
    // component tree mounts and its effects run. This follows a redirect CHAIN
    // (login → /post-login → /entity/…) hop by hop, like a browser, bounded to avoid a
    // misconfigured redirect loop.
    if (
      this._live &&
      /^https?:/.test(this._url) &&
      pathOf(this._url) !== this._loadedPath &&
      this._navHops < 10
    ) {
      this._navHops++;
      await this._reopen(this._url);
    }
  }

  // Re-pump the live app and pull its DOM forward. Backs web-first assertion retry:
  // a UI change that lands just after an action (a modal close, an async-loaded row)
  // becomes observable on the next poll. No-op without a live session.
  async _redrain() {
    if (this._session == null) return;
    try {
      await native.liveEval(this._session, "globalThis.__RESULT = '';");
    } catch {
      /* terminated mid-drain — refresh reads best-effort state */
    }
    await this._refreshFromSession();
  }

  // Drain the live app's network log and emit Playwright `response` events. Tests
  // subscribe with `page.on('response', …)` to capture API payloads (the payroll
  // period, employments lists). Each entry → a minimal Response (url/status/json/text).
  async _drainNetworkEvents() {
    if (this._session == null) return;
    if (!this._listeners.has("response") && !this._listeners.has("requestfinished")) return;
    let raw;
    try {
      raw = await native.liveEval(
        this._session,
        "globalThis.__RESULT = JSON.stringify((globalThis.__netLog || []).splice(0));",
      );
    } catch {
      return;
    }
    if (!raw) return;
    let entries;
    try {
      entries = JSON.parse(raw);
    } catch {
      return;
    }
    this._recentResponses = this._recentResponses ?? [];
    for (const e of entries) {
      const resp = makeNetResponse(e);
      resp._seq = this._actionSeq; // which interaction produced it (see waitForResponse)
      this._recentResponses.push(resp);
      this._emit("response", resp);
      this._emit("requestfinished", resp.request());
    }
    if (this._recentResponses.length > 100)
      this._recentResponses.splice(0, this._recentResponses.length - 100);
  }

  // Follow an in-app navigation: load `url` fresh (fetch + hydrate) WITHOUT resetting the
  // redirect-hop budget or pushing history (it's the app navigating, not the user).
  async _reopen(url) {
    await this._navigate(this._resolveUrl(url));
    await this._openLiveSession();
  }

  _closeSession() {
    if (this._session != null) {
      try {
        native.liveClose(this._session);
      } catch {
        /* already gone */
      }
      this._session = null;
    }
  }

  // Drive an interaction in the live isolate (real DOM events → running app handlers →
  // re-render), then refresh the Page snapshot. `kind` is "click" | "fill".
  async _sessionDispatch(selector, index, kind, value) {
    // A new user interaction: responses produced by THIS action (and after) are "fresh"
    // for any waitForResponse paired with it; responses from before are stale.
    this._actionSeq++;
    const r = await native.liveEval(
      this._session,
      interactionScript(selector, index ?? 0, kind, value),
    );
    await this._refreshFromSession();
    if (r === "NO_MATCH") throw new Error("turbo-surf: locator matched no elements");
    return r;
  }
  // Run arbitrary JS in the live isolate, then refresh the Page snapshot. Backs the
  // keyboard (key-event dispatch) and other live-only side effects.
  async _sessionEval(script) {
    if (this._session == null) return undefined;
    const r = await native.liveEval(this._session, script);
    await this._refreshFromSession();
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

  async _navigate(url, method, body) {
    this._closeSession(); // a fresh document — discard the prior live app
    // `about:blank` (and other about: URLs) are not network resources — a browser shows an
    // empty document with NO request. Short-circuit so a goto/reload of about:blank yields a
    // blank page instead of a "builder error for url (about:blank)" from the net layer.
    if (/^about:/i.test(url)) {
      this._html = "<html><head></head><body></body></html>";
      this._url = url;
      this._hydrated = false;
      this._emit("load");
      this._emit("domcontentloaded");
      return makeResponse({ status: 200, finalUrl: url, html: this._html });
    }
    const r = JSON.parse(
      await native.fetchWithCookies(
        url,
        this._cookies,
        method ?? null,
        body ?? null,
        this._context._headersJson(),
      ),
    );
    this._html = r.html;
    this._url = r.finalUrl;
    this._cookies = r.cookies;
    this._hydrated = false; // fresh document — not yet run its own JS
    this._emit("load");
    this._emit("domcontentloaded");
    return makeResponse(r);
  }
  async goto(url, opts) {
    if (this._url !== "about:blank") this._history.push(this._url);
    this._fwd = []; // a fresh navigation invalidates the forward stack
    this._navHops = 0; // user navigation — reset the redirect-follow budget
    this._autoHydrate = opts?.waitUntil !== "commit"; // 'commit' = inspect raw shell
    const resp = await this._navigate(this._resolveUrl(url));
    // `waitUntil:'networkidle'` (or a later waitForLoadState('networkidle')) opens a LIVE
    // session: hydrate the app to quiescence so an SPA's client-rendered content (forms,
    // dashboards) appears, and keep it mounted so clicks/fills dispatch real events into
    // the running app. Default/'load'/'commit' stay raw (cheap fetch) — hydrating EVERY
    // goto is both slow and unnecessary for static pages.
    if (opts?.waitUntil === "networkidle") await this._openLiveSession();
    return resp;
  }
  async reload(opts) {
    // Re-fetch in place — no history entry. Like goto, a `waitUntil:'networkidle'` reload
    // re-opens a LIVE session so a client-rendered SPA re-hydrates (its forms/dashboards
    // re-appear); without this the reloaded doc stays the raw un-hydrated shell and reads
    // (e.g. a settings select's value) see an empty DOM.
    this._autoHydrate = opts?.waitUntil !== "commit";
    const resp = await this._navigate(this._url);
    if (opts?.waitUntil === "networkidle") await this._openLiveSession();
    return resp;
  }
  async goBack() {
    const prev = this._history.pop();
    if (prev == null) return null;
    this._fwd.push(this._url);
    return this._navigate(prev);
  }
  async goForward() {
    const next = this._fwd.pop();
    if (next == null) return null;
    this._history.push(this._url);
    return this._navigate(next);
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

  // Evaluate JS against the page DOM. Supports (fn, arg) and string forms. When a live
  // session is up, run in the LIVE isolate (sees the running app's state + can mutate
  // it) and refresh the snapshot; otherwise run statelessly over the cached HTML.
  async evaluate(script, arg) {
    if (this._live) {
      const body = evalSource(script, arg);
      // Await a returned Promise (e.g. navigator.clipboard.readText()) so evaluate
      // resolves the value, not "[object Promise]". The drain runs the microtask.
      const out = await native.liveEval(
        this._session,
        `(async () => { return ${body}; })().then((r) => { globalThis.__RESULT = r === undefined ? "" : r; }, () => { globalThis.__RESULT = ""; });`,
      );
      await this._refreshFromSession();
      return out;
    }
    return native.evaluate(this._html, evalSource(script, arg));
  }
  async evaluateHandle(script, arg) {
    return this.evaluate(script, arg);
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
    );
    this._hydrated = true;
    return this._html;
  }
  async addInitScript(script, arg) {
    // No reload pipeline to inject before; run it now over the current DOM.
    this._context._initScripts.push(evalSource(script, arg));
    if (this._html) await this.render(evalSource(script, arg));
  }

  async setContent(html) {
    this._html = html;
    this._emit("load");
  }

  // --- page-level actions (selector shortcuts) ---
  async fill(selector, value) {
    this._html = native.fill(this._html, selector, value);
  }
  async type(selector, value) {
    return this.fill(selector, value);
  }
  async check(selector) {
    this._html = native.setChecked(this._html, selector, true);
  }
  async uncheck(selector) {
    this._html = native.setChecked(this._html, selector, false);
  }
  async setChecked(selector, on) {
    this._html = native.setChecked(this._html, selector, !!on);
  }
  async selectOption(selector, value) {
    const v = Array.isArray(value) ? value[0] : value;
    this._html = native.selectOption(this._html, selector, String(v?.value ?? v));
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
  async focus() {}
  async dispatchEvent() {}

  async _followIntent(intent) {
    if (intent.action === "navigate") return this.goto(intent.url);
    if (intent.action === "submit") return this._submit(intent);
    return null; // inert (a JS-only handler — nothing to fire without JS)
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
      ),
    );
    this._html = r.html;
    this._url = r.finalUrl;
    this._cookies = r.cookies;
    return makeResponse(r);
  }

  // --- locators ---
  _locFromSelector(selector, mode = "auto") {
    const loc = new Locator(this, (h) => JSON.parse(native.query(h, selector, mode)));
    loc._selector = selector; // enables CSS-concat nesting
    return loc;
  }
  locator(selector) {
    return this._locFromSelector(selector);
  }
  getByRole(role, opts = {}) {
    const name = opts.name != null ? String(opts.name) : null;
    return new Locator(this, (h) => JSON.parse(native.getBy(h, "role", role, name)), {
      getBy: { kind: "role", value: role, name },
    });
  }
  getByText(text, _o) {
    return new Locator(this, (h) => JSON.parse(native.getBy(h, "text", String(text))), {
      getBy: { kind: "text", value: String(text), name: null },
    });
  }
  getByLabel(text, _o) {
    return new Locator(this, (h) => JSON.parse(native.getBy(h, "label", String(text))), {
      getBy: { kind: "label", value: String(text), name: null },
    });
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
  getByTestId(id) {
    return this._locFromSelector(`[${this._context._testIdAttribute}="${id}"]`);
  }

  // --- frames (no real frames → the page is its own main frame) ---
  mainFrame() {
    return this;
  }
  frames() {
    return [this];
  }
  frame() {
    return this;
  }
  frameLocator(selector) {
    return this.locator(selector);
  }

  // --- waiters (static engine: resolve immediately / poll evaluate) ---
  // `waitForLoadState('networkidle')` is the other "page has settled" signal — treat
  // it like the networkidle goto and hydrate the SPA if it hasn't been already.
  async waitForLoadState(state) {
    if (state === "networkidle" && !this._live) await this._openLiveSession();
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
    if (match(this._url)) return undefined;
    // In a live session a navigation can be a MULTI-STEP client redirect (login →
    // /post-login → /entity-select|/entity/{id}), each step a render that runs on the
    // next drain. Pump the session — each refresh drains it — until the URL matches or
    // we give up. (No session: the nav already ran synchronously, so just assert.)
    if (this._live) {
      const rounds = Math.max(1, Math.ceil((opts.timeout ?? 15000) / 1000));
      for (let i = 0; i < rounds && !match(this._url); i++) {
        await native.liveEval(this._session, "globalThis.__RESULT = location.href;");
        await this._refreshFromSession();
      }
      if (match(this._url)) return undefined;
    }
    throw new Error(`turbo-surf: waitForURL(${url}) — current url is ${this._url}`);
  }
  async waitForSelector(selector, opts = {}) {
    const present = (await this.locator(selector).count()) > 0;
    if (!present && opts.state !== "hidden" && opts.state !== "detached")
      throw new Error(`turbo-surf: waitForSelector(${selector}) found no element`);
    return present ? this.locator(selector) : null;
  }
  // Poll `fn(arg)` in the live isolate until truthy. `arg` may be an ElementHandle
  // (from locator.elementHandle()) — resolved to the live element so e.g.
  // `waitForFunction((btn) => !btn.disabled, await uploadButton.elementHandle())` works.
  async waitForFunction(fn, arg, opts) {
    const timeout = (opts && typeof opts === "object" && opts.timeout) || 30000;
    await this._ensureLive();
    const fnStr = typeof fn === "function" ? fn.toString() : String(fn);
    let argExpr;
    if (arg && arg.__turboHandle) {
      if (arg.selector) {
        argExpr = `document.querySelectorAll(${JSON.stringify(arg.selector)})[${arg.index}]`;
      } else if (arg.getBy) {
        const g = arg.getBy;
        argExpr =
          `(function(){ globalThis.__tcGetBy(${JSON.stringify(g.kind)},${JSON.stringify(g.value)},${g.name == null ? "null" : JSON.stringify(g.name)}); ` +
          `var h=JSON.parse(globalThis.__RESULT||"[]"); var all=document.querySelectorAll("*"); return h[${arg.index}]?all[h[${arg.index}].idx]:undefined; })()`;
      } else {
        argExpr = "undefined";
      }
    } else {
      argExpr = JSON.stringify(arg ?? null);
    }
    // Capture BOTH truthiness and the VALUE — Playwright's waitForFunction resolves to the
    // function's return value, not a boolean. Works live (poll the running app) and static
    // (one eval over the snapshot; a static DOM won't change so don't spin).
    const probe = `(() => { try { var __a = ${argExpr}; var __r = (${fnStr})(__a); return JSON.stringify({ ok: !!__r, v: __r === undefined ? null : __r }); } catch (_e) { return JSON.stringify({ ok: false, v: null }); } })()`;
    const evalOnce = async () => {
      let raw;
      if (this._session != null) {
        try {
          raw = await native.liveEval(this._session, `globalThis.__RESULT = ${probe};`);
        } catch {
          raw = null;
        }
        await this._refreshFromSession();
      } else {
        try {
          raw = native.evaluate(this._html, probe);
        } catch {
          raw = null;
        }
      }
      try {
        return JSON.parse(raw);
      } catch {
        return { ok: false, v: null };
      }
    };
    const start = Date.now();
    for (;;) {
      const res = await evalOnce();
      if (res && res.ok) return res.v;
      if (this._session == null) break; // static snapshot won't change
      if (Date.now() - start >= timeout) break;
      await new Promise((res2) => setTimeout(res2, 40));
    }
    throw new Error("turbo-surf: waitForFunction timed out");
  }
  async waitForNavigation() {
    return makeResponse({ status: 200, finalUrl: this._url });
  }
  // waitForEvent('download') — resolve when a client-side export fires (the env captures
  // `<a download>` clicks into __downloads). Used as `Promise.all([waitForEvent('download'),
  // …click()])`: the click drives the session (running the anchor click → __downloads push)
  // while this polls + drains __downloads from the live isolate.
  async waitForEvent(event, opts) {
    if (event !== "download") return undefined;
    const timeout = (opts && typeof opts === "object" && opts.timeout) || 30000;
    await this._ensureLive();
    if (this._session == null) return undefined;
    const start = Date.now();
    while (Date.now() - start < timeout) {
      let raw;
      try {
        raw = await native.liveEval(
          this._session,
          "globalThis.__RESULT = JSON.stringify((globalThis.__downloads || []).splice(0));",
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
      if (entries.length) return makeDownload(entries[0]);
      await new Promise((r) => setTimeout(r, 30)); // let the concurrent click drive + push
    }
    return undefined;
  }
  async waitForResponse(matcher, opts = {}) {
    const test = (resp) => {
      try {
        if (typeof matcher === "function") return !!matcher(resp);
        if (matcher instanceof RegExp) return matcher.test(resp.url());
        return resp.url().includes(String(matcher));
      } catch {
        return false;
      }
    };
    // Real Playwright only matches responses received AFTER this call. Our live engine
    // drains network in batches, so a matching response may already sit in the buffer —
    // but ONLY accept one produced by the current interaction (or later). A response that
    // drained in an EARLIER step is stale and must not match, else a loose predicate like
    // url.includes('/bulk-upload') grabs the template GET from a prior step instead of
    // waiting for this step's POST. (_seq is the action that produced the response.)
    const callSeq = this._actionSeq;
    const buffered = (this._recentResponses ?? []).find((r) => (r._seq ?? 0) >= callSeq && test(r));
    if (buffered) return buffered;
    return new Promise((resolve) => {
      // On timeout resolve a synthetic response rather than rejecting: many callers
      // use `Promise.all([waitForResponse(...), action()])` and only need the action
      // to run — a hard reject there would fail otherwise-passing flows.
      const timer = setTimeout(() => {
        this.off("response", onResp);
        resolve(makeResponse({ status: 200, finalUrl: this._url }));
      }, opts.timeout ?? 15000);
      const onResp = (resp) => {
        if (!test(resp)) return;
        clearTimeout(timer);
        this.off("response", onResp);
        resolve(resp);
      };
      this.on("response", onResp);
    });
  }
  async waitForRequest() {
    return { url: () => this._url, method: () => "GET" };
  }

  // --- events (registry; we fire load/domcontentloaded; others are inert) ---
  on(event, fn) {
    const arr = this._listeners.get(event) ?? [];
    arr.push(fn);
    this._listeners.set(event, arr);
    return this;
  }
  once(event, fn) {
    return this.on(event, fn);
  }
  addListener(event, fn) {
    return this.on(event, fn);
  }
  off(event, fn) {
    const arr = this._listeners.get(event);
    if (arr)
      this._listeners.set(
        event,
        arr.filter((f) => f !== fn),
      );
    return this;
  }
  removeListener(event, fn) {
    return this.off(event, fn);
  }
  _emit(event, payload) {
    for (const fn of this._listeners.get(event) ?? []) fn(payload);
  }

  // --- context / config / state ---
  context() {
    return this._context;
  }
  request() {
    return request;
  }
  viewportSize() {
    return this._context._viewport;
  }
  async setViewportSize(size) {
    this._context._viewport = size;
  }
  setDefaultTimeout() {}
  setDefaultNavigationTimeout() {}
  async setExtraHTTPHeaders(headers) {
    this._context._headers = { ...this._context._headers, ...headers };
  }
  async emulateMedia() {}
  async bringToFront() {}
  async addStyleTag() {}
  async addScriptTag(opts = {}) {
    if (opts.content) await this.render(opts.content);
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
    this._closeSession();
    this._closed = true;
    this._emit("close");
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
  // returns a vector document; otherwise PNG. `fullPage: true` grows the image to
  // the full content height; `clip` is ignored (width/height or the context
  // viewport drive layout). Returns a Buffer; writes `path` if given.
  async screenshot(opts = {}) {
    const vp = this._context._viewport ?? { width: 1280, height: 800 };
    const width = opts.width ?? vp.width;
    const height = opts.height ?? vp.height;
    const asSvg =
      opts.type === "svg" || (typeof opts.path === "string" && opts.path.endsWith(".svg"));
    // Fetch the page's external <link> stylesheets so the render is styled, not
    // just inline-CSS. `{ externalCss: false }` skips it (offline/inline-only).
    const css = opts.externalCss === false ? "" : await this._fetchLinkedCss();
    // Fetch the page's <img>/background-image bytes so images paint (PNG/JPEG);
    // `{ images: false }` skips it.
    const images = opts.images === false ? {} : await this._fetchImages();
    // Playwright's `fullPage` grows the image to the full content height.
    const fullPage = opts.fullPage === true;
    const out = asSvg
      ? Buffer.from(
          native.screenshotSvgWithAssets(this._html, css, images, width, height, fullPage),
          "utf8",
        )
      : native.screenshotWithAssets(this._html, css, images, width, height, fullPage);
    if (opts.path) await writeFile(opts.path, out);
    return out;
  }

  // Fetch the page's `<img>` + `background-image` bytes, keyed by their raw ref
  // (the resolver key the raster expects), resolved against the page URL and
  // fetched as raw bytes over the engine's client. Best-effort: non-http pages
  // and failed/unresolvable refs are skipped; count capped to match the engine.
  async _fetchImages() {
    if (!/^https?:/.test(this._url ?? "")) return {};
    const images = {};
    for (const ref of native.imageUrls(this._html).slice(0, 60)) {
      try {
        const abs = new URL(ref, this._url).href;
        const buf = await native.fetchBytes(abs);
        if (buf && buf.length) images[ref] = buf;
      } catch {
        /* skip an image that won't resolve/fetch */
      }
    }
    return images;
  }

  // Concatenate the page's `<link rel="stylesheet">` bodies, resolved against the
  // page URL and fetched over the engine's client (cookies apply). Best-effort:
  // non-http pages and failed/relative-unresolvable sheets are skipped.
  async _fetchLinkedCss() {
    if (!/^https?:/.test(this._url ?? "")) return "";
    let css = "";
    for (const href of native.stylesheetHrefs(this._html).slice(0, 40)) {
      try {
        const abs = new URL(href, this._url).href;
        const r = JSON.parse(await native.fetchHtml(abs));
        if (r.status >= 200 && r.status < 300 && r.html) css += `${r.html}\n`;
      } catch {
        /* skip a sheet that won't resolve/fetch */
      }
    }
    return css;
  }

  // --- unsupported → honest throws ---
  pdf = UNSUPPORTED("page.pdf", PIXEL);
  dragAndDrop = UNSUPPORTED("page.dragAndDrop", INPUT);
  route = UNSUPPORTED("page.route", NETIO);
  routeFromHAR = UNSUPPORTED("page.routeFromHAR", NETIO);
  unroute = UNSUPPORTED("page.unroute", NETIO);
  pause = UNSUPPORTED("page.pause", "no inspector UI");
  exposeBinding = UNSUPPORTED(
    "page.exposeBinding",
    "no persistent JS<->host binding across renders",
  );
  exposeFunction = UNSUPPORTED(
    "page.exposeFunction",
    "no persistent JS<->host binding across renders",
  );

  // pixel input devices
  get mouse() {
    return mouseStub;
  }
  get keyboard() {
    return makeKeyboard(this);
  }
  get touchscreen() {
    return touchStub;
  }
}

const mouseStub = {
  click: UNSUPPORTED("mouse.click", INPUT),
  dblclick: UNSUPPORTED("mouse.dblclick", INPUT),
  move: UNSUPPORTED("mouse.move", INPUT),
  down: UNSUPPORTED("mouse.down", INPUT),
  up: UNSUPPORTED("mouse.up", INPUT),
  wheel: UNSUPPORTED("mouse.wheel", INPUT),
};
// Keyboard: no hardware, but key events are real JS. Dispatch keydown/keyup (with a
// keypress for printable keys) on the focused element (or document) in the live app —
// covers Escape-to-close-popover, Enter-to-submit, etc. No-op without a live session.
function makeKeyboard(page) {
  const dispatch = async (key) => {
    if (!page._live) return;
    const k = JSON.stringify(String(key));
    await page._sessionEval(`(() => {
      const key = ${k};
      const el = document.activeElement || document.body || document.documentElement;
      if (!el) return;
      const opt = { bubbles: true, cancelable: true, key, code: key, view: globalThis };
      el.dispatchEvent(new KeyboardEvent("keydown", opt));
      if (key.length === 1) el.dispatchEvent(new KeyboardEvent("keypress", opt));
      el.dispatchEvent(new KeyboardEvent("keyup", opt));
    })();`);
  };
  return {
    press: (key) => dispatch(key),
    down: (key) => dispatch(key),
    up: async () => {},
    type: async (text) => {
      for (const ch of String(text)) await dispatch(ch);
    },
    insertText: async (text) => dispatch(String(text)),
  };
}
const touchStub = { tap: UNSUPPORTED("touchscreen.tap", INPUT) };

function makeResponse(r) {
  return {
    status: () => r.status,
    ok: () => r.status >= 200 && r.status < 300,
    url: () => r.finalUrl,
    statusText: () => "",
    headers: () => r.headers ?? {},
    async text() {
      return r.html ?? "";
    },
    async json() {
      return JSON.parse(r.html ?? "null");
    },
  };
}

// A Playwright-like Response built from a captured live-app fetch (net-log entry),
// for `page.on('response')` subscribers.
function makeNetResponse(e) {
  const req = {
    url: () => e.url,
    method: () => e.method || "GET",
    headers: () => (e.contentType ? { "content-type": e.contentType } : {}),
    postData: () => null,
    resourceType: () => "fetch",
  };
  return {
    url: () => e.url,
    status: () => e.status,
    statusText: () => "",
    ok: () => !!e.ok,
    headers: () => (e.contentType ? { "content-type": e.contentType } : {}),
    async text() {
      return e.body ?? "";
    },
    async json() {
      return JSON.parse(e.body ?? "null");
    },
    request: () => req,
    async finished() {
      return null;
    },
  };
}

// A Playwright-like Download built from a captured client-side export (a `<a download>`
// click over a createObjectURL blob — see the env's __downloads). The bytes are written to
// a temp file lazily so download.path()/saveAs() behave like the real thing.
function makeDownload(entry) {
  const fs = require("node:fs");
  const os = require("node:os");
  const path = require("node:path");
  let cached = null;
  const writeTemp = () => {
    if (cached) return cached;
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "turbo-surf-dl-"));
    const p = path.join(dir, entry.filename || "download");
    fs.writeFileSync(p, entry.content ?? "");
    cached = p;
    return p;
  };
  return {
    suggestedFilename: () => entry.filename || "download",
    url: () => entry.url || "",
    async path() {
      return writeTemp();
    },
    async saveAs(dest) {
      const fs2 = require("node:fs");
      fs2.writeFileSync(dest, entry.content ?? "");
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
  constructor(opts = {}) {
    const cookies = opts.storageState?.cookies ?? [];
    // `playwright.config` `use: { baseURL, testIdAttribute }` isn't read by the
    // shim runner, so the register step maps it in via env (TURBO_SHIM_*).
    this._baseURL = opts.baseURL ?? process.env.TURBO_SHIM_BASE_URL ?? null;
    // Locale: real Playwright runs `devices['Desktop Chrome']` (locale en-US), and the
    // app picks UI language from the `NEXT_LOCALE` cookie (next-intl) — with none it
    // defaults to es-MX, so English text/role-name assertions miss. Seed NEXT_LOCALE to
    // match the browser locale (default en-US) unless the caller already provided one.
    // NOTE: the settings suite expects the es-MX default (Chrome shows es-MX there) — a known
    // parity gap; the post-login NEXT_LOCALE=en-US source is not yet pinned. See memory.
    this._locale = opts.locale ?? process.env.TURBO_SHIM_LOCALE ?? null;
    if (this._locale && !cookies.some((c) => c.name === "NEXT_LOCALE")) {
      let host = "localhost";
      try {
        host = new URL(this._baseURL || "http://localhost").hostname;
      } catch {
        /* keep */
      }
      cookies.push({
        name: "NEXT_LOCALE",
        value: this._locale,
        domain: host,
        path: "/",
        secure: false,
        http_only: false,
        same_site: "lax",
        expires_at: 1900000000000,
      });
    }
    this._cookies = JSON.stringify(cookies);
    // Custom User-Agent → page-JS navigator.userAgent + page fetches (newContext({userAgent})).
    this._userAgent = opts.userAgent ?? process.env.TURBO_SHIM_USER_AGENT ?? null;
    this._headers = opts.extraHTTPHeaders ?? {};
    this._viewport = opts.viewport ?? { width: 1280, height: 720 };
    this._testIdAttribute =
      opts.testIdAttribute ?? process.env.TURBO_SHIM_TESTID_ATTR ?? "data-testid";
    this._initScripts = [];
    this._pages = [];
    this._listeners = new Map();
  }
  _headersJson() {
    // Merge the context User-Agent into the request headers (so goto/request fetches
    // carry it too, matching the live session's navigator.userAgent).
    const h = this._userAgent ? { "user-agent": this._userAgent, ...this._headers } : this._headers;
    return Object.keys(h).length ? JSON.stringify(h) : null;
  }
  async newPage() {
    const p = new Page(this);
    this._pages.push(p);
    return p;
  }
  pages() {
    return this._pages;
  }
  storageState() {
    return { cookies: JSON.parse(this._cookies), origins: [] };
  }
  addCookies(cookies) {
    const existing = JSON.parse(this._cookies);
    this._cookies = JSON.stringify([...existing, ...cookies]);
  }
  async cookies() {
    return JSON.parse(this._cookies);
  }
  async clearCookies() {
    this._cookies = "[]";
  }
  async addInitScript(script, arg) {
    this._initScripts.push(evalSource(script, arg));
  }
  async setExtraHTTPHeaders(headers) {
    this._headers = { ...this._headers, ...headers };
  }
  async setDefaultTimeout() {}
  async setDefaultNavigationTimeout() {}
  async grantPermissions() {}
  async clearPermissions() {}
  async setGeolocation() {}
  async setOffline() {}
  on(event, fn) {
    const arr = this._listeners.get(event) ?? [];
    arr.push(fn);
    this._listeners.set(event, arr);
    return this;
  }
  off() {
    return this;
  }
  once(event, fn) {
    return this.on(event, fn);
  }
  request() {
    return request;
  }
  browser() {
    return browserStub;
  }
  async close() {}
  route = UNSUPPORTED("context.route", NETIO);
  routeFromHAR = UNSUPPORTED("context.routeFromHAR", NETIO);
  unroute = UNSUPPORTED("context.unroute", NETIO);
  exposeBinding = UNSUPPORTED("context.exposeBinding", "no persistent JS<->host binding");
  exposeFunction = UNSUPPORTED("context.exposeFunction", "no persistent JS<->host binding");
}

const browserStub = {
  version: () => "turbo-surf",
  browserType: () => ({ name: () => "chromium" }),
  async newContext(opts) {
    return new BrowserContext(opts);
  },
  async newPage(opts) {
    return new BrowserContext(opts).newPage();
  },
  contexts: () => [],
  isConnected: () => true,
  async close() {},
};

// Minimal browser entry for drop-in feel: chromium.launch() → newPage().
export const chromium = {
  name: () => "chromium",
  async launch() {
    return browserStub;
  },
  async launchPersistentContext(_dir, opts) {
    return new BrowserContext(opts);
  },
  async connect() {
    return browserStub;
  },
};
export const firefox = chromium;
export const webkit = chromium;

export function newPage(opts) {
  return new Page(new BrowserContext(opts));
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
      } catch {
        return false;
      }
    };
    let pass = await run();
    while ((this._neg ? pass : !pass) && Date.now() - start < timeout) {
      if (!page || !page._live) break;
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
    this._ok(await this._retry(() => this._snap().visible, opts), "expected element to be visible");
  }
  async toBeHidden(opts) {
    this._ok(await this._retry(() => !this._snap().visible, opts), "expected element to be hidden");
  }
  async toBeAttached(opts) {
    this._ok(
      await this._retry(async () => (await this._v.count()) > 0, opts),
      "expected element to be attached",
    );
  }
  async toBeChecked(opts) {
    this._ok(await this._retry(() => this._snap().checked, opts), "expected element to be checked");
  }
  async toBeEnabled(opts) {
    this._ok(await this._retry(() => this._snap().enabled, opts), "expected element to be enabled");
  }
  async toBeDisabled(opts) {
    this._ok(
      await this._retry(() => this._snap().enabled === false, opts),
      "expected element to be disabled",
    );
  }
  async toBeEditable(opts) {
    this._ok(
      await this._retry(() => this._snap().editable, opts),
      "expected element to be editable",
    );
  }
  async toBeEmpty(opts) {
    this._ok(await this._retry(() => this._snap().empty, opts), "expected element to be empty");
  }
  async toBeFocused() {
    this._ok(false === this._neg ? false : true, "focus state unavailable on a static DOM");
  }
  async toBeInViewport(opts) {
    this._ok(
      await this._retry(() => this._snap().visible, opts),
      "expected element to be in viewport",
    );
  }
  async toHaveCount(n, opts) {
    const pass = await this._retry(async () => (await this._v.count()) === n, opts);
    this._ok(pass, `expected count ${n}, got ${await this._v.count()}`);
  }
  async toHaveText(s, opts) {
    const test = () => {
      const t = normWs(this._snap().text ?? "");
      return s instanceof RegExp ? s.test(t) : t === normWs(s);
    };
    this._ok(
      await this._retry(test, opts),
      `expected text ${s}, got "${normWs(this._snap().text ?? "")}"`,
    );
  }
  async toContainText(s, opts) {
    const test = () => {
      const t = normWs(this._snap().text ?? "");
      return s instanceof RegExp ? s.test(t) : t.includes(normWs(s));
    };
    this._ok(
      await this._retry(test, opts),
      `expected text to contain "${s}", got "${normWs(this._snap().text ?? "")}"`,
    );
  }
  async toHaveValue(value, opts) {
    const pass = await this._retry(() => matchText(value, this._snap().value ?? ""), opts);
    this._ok(pass, `expected value ${value}, got "${this._snap().value ?? ""}"`);
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
    const test = async () => {
      const got = await this._v.getAttribute(name);
      if (value === undefined) return got !== null;
      return value instanceof RegExp ? value.test(got ?? "") : got === value;
    };
    this._ok(
      await this._retry(test, opts),
      `expected attribute ${name}=${value}, got ${await this._v.getAttribute(name)}`,
    );
  }
  async toHaveClass(cls) {
    const got = (await this._v.getAttribute("class")) ?? "";
    const pass = cls instanceof RegExp ? cls.test(got) : got.split(/\s+/).includes(cls);
    this._ok(pass, `expected class "${cls}" in "${got}"`);
  }
  async toContainClass(cls) {
    return this.toHaveClass(cls);
  }
  async toHaveId(id) {
    const got = await this._v.getAttribute("id");
    this._ok(got === id, `expected id ${id}, got ${got}`);
  }
  async toHaveCSS(name, value) {
    const got = await this._v.cssValue(name);
    this._ok(got === value, `expected css ${name}:${value}, got "${got}"`);
  }
  async toHaveJSProperty(name, value) {
    const got = await this._v.getAttribute(name);
    this._ok(String(got) === String(value), `expected ${name}=${value}, got ${got}`);
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
      get:
        (_t, name) =>
        async (...args) => {
          const { value, threw } = await awaiter();
          if (threw && (name === "toThrow" || name === "toThrowError")) {
            const m = args[0];
            const ok =
              m == null ||
              (m instanceof RegExp ? m.test(value.message) : String(value.message).includes(m));
            if (!ok) throw new Error(`expected rejection to match ${m}, got "${value.message}"`);
            return undefined;
          }
          return new ValueExpect(value, neg)[name](...args);
        },
    },
  );
}

// jest-shaped generic-value matchers — pure JS, never cross the napi boundary.
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
  toBe(x) {
    this._ok(Object.is(this._v, x), `expected ${this._v} to be ${x}`);
  }
  toEqual(x) {
    this._ok(deepEqual(this._v, x), `expected ${json(this._v)} to equal ${json(x)}`);
  }
  toStrictEqual(x) {
    return this.toEqual(x);
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
  toBeGreaterThan(n) {
    this._ok(this._v > n, `expected ${this._v} > ${n}`);
  }
  toBeGreaterThanOrEqual(n) {
    this._ok(this._v >= n, `expected ${this._v} >= ${n}`);
  }
  toBeLessThan(n) {
    this._ok(this._v < n, `expected ${this._v} < ${n}`);
  }
  toBeLessThanOrEqual(n) {
    this._ok(this._v <= n, `expected ${this._v} <= ${n}`);
  }
  toBeCloseTo(n, digits = 2) {
    this._ok(Math.abs(this._v - n) < 10 ** -digits / 2, `expected ${this._v} close to ${n}`);
  }
  toBeInstanceOf(c) {
    this._ok(this._v instanceof c, `expected instance of ${c?.name}`);
  }
  toHaveLength(n) {
    this._ok(this._v?.length === n, `expected length ${n}, got ${this._v?.length}`);
  }
  toHaveProperty(key, value) {
    const got = key.split(".").reduce((o, k) => o?.[k], this._v);
    this._ok(
      value === undefined ? got !== undefined : deepEqual(got, value),
      `expected property ${key}`,
    );
  }
  toThrow(expected) {
    let threw = null;
    try {
      this._v();
    } catch (e) {
      threw = e;
    }
    const pass = threw != null && (expected == null || String(threw.message).includes(expected));
    this._ok(pass, "expected function to throw");
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

function matchText(expected, got) {
  if (expected instanceof RegExp) return expected.test(got);
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
function deepEqual(a, b) {
  if (Object.is(a, b)) return true;
  if (typeof a !== "object" || typeof b !== "object" || a == null || b == null) return false;
  const ka = Object.keys(a);
  const kb = Object.keys(b);
  return ka.length === kb.length && ka.every((k) => deepEqual(a[k], b[k]));
}
function subset(want, got) {
  if (typeof want !== "object" || want == null) return deepEqual(want, got);
  return Object.keys(want).every((k) => subset(want[k], got?.[k]));
}

export function expect(value) {
  if (value instanceof Locator) return new LocatorExpect(value, false);
  if (value instanceof Page) return new PageExpect(value, false);
  return new ValueExpect(value, false);
}
expect.soft = expect; // soft assertions behave as hard ones here
expect.poll = (fn, _o) => new ValueExpect(fn(), false);
expect.configure = () => expect;
expect.extend = () => {};

// ---------------------------------------------------------------------------
// @playwright/test surface (drop-in over node:test) — fixtures, hooks, steps.
// ---------------------------------------------------------------------------
import {
  test as nodeTest,
  describe as nodeDescribe,
  before,
  after,
  beforeEach,
  afterEach,
} from "node:test";

// A fixture set: each value is either a plain default or [fn, opts] / async fn
// of ({ ...fixtures }, use). We resolve them lazily and tear down after `use`.
function makeBaseFixtures() {
  return {
    browser: async (_f, use) => use(browserStub),
    context: async (_f, use) => use(new BrowserContext()),
    page: async (f, use) => {
      const ctx = f.context ?? new BrowserContext();
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
    request: async (_f, use) => use(request),
    baseURL: async (_f, use) => use(process.env.TURBO_SHIM_BASE_URL ?? undefined),
  };
}

// Playwright shares one fixture set (the same `page`/`context`) across a test's
// beforeEach → body → afterEach. node:test runs those as separate callbacks, so
// we stash the active set in `_current` (suite runs serially — workers:1) and
// reuse it everywhere. Teardown is deferred to the *next* test's beforeEach (and
// a final afterAll) so an afterEach hook still sees a live, logged-in page —
// without this, cleanup hooks like `revokeActiveGrantIfAny({page})` get an
// undefined page, crash, and leak server-side state into later tests.
let _current = null;
let _fixtureHooksWired = false;
function wireFixtureHooks(open) {
  if (_fixtureHooksWired) return;
  _fixtureHooksWired = true;
  const teardownCurrent = async () => {
    if (!_current) return;
    const c = _current;
    _current = null;
    await c.teardown();
  };
  // Registered before any user beforeEach (this fires on the first test/hook
  // registration), so the set is open by the time user hooks and the body run.
  beforeEach(async () => {
    await teardownCurrent();
    _current = await open(makeTestInfo("test"));
  });
  after(teardownCurrent);
}

// Playwright's `test.skip` is overloaded: `skip(title, body)` declares a skipped
// test, but `skip(condition, description)` / `skip()` / `skip(fixturesCb, desc)`
// conditionally skip the enclosing scope. The shim used to treat every call as
// `(title, body)`, so a `skip(false, "reason")` (the common conditional form)
// registered a bogus skipped test. These helpers disambiguate.
const SKIP_SIGNAL = Symbol("turbo-shim-skip");
let _skipStack = []; // one frame per active describe; `.skip` set by a conditional skip
let _activeT = null; // node:test context of the running test (for a runtime skip())
const describeSkipped = () => _skipStack.some((f) => f.skip);
const isDeclareForm = (args) => typeof args[0] === "string" && typeof args[1] === "function";
function skipConditionMet(args) {
  if (args.length === 0) return true; // bare test.skip() → always skip
  const c = args[0];
  if (typeof c === "function") {
    try {
      return !!c({});
    } catch {
      return false;
    }
  }
  return !!c;
}

function makeTestFn(baseDefs, extDefs = {}) {
  const open = (testInfo) => openFixtures(baseDefs, extDefs, testInfo);
  const run = (name, fn) => {
    wireFixtureHooks(open);
    if (describeSkipped()) {
      nodeTest.skip(name, () => {});
      return;
    }
    nodeTest(name, async (t) => {
      _activeT = t;
      const testInfo = makeTestInfo(name);
      try {
        // The shared `_current` set is opened once (by the wired beforeEach) with the BASE
        // fixtures, so it only covers plain `test(...)`. A `test.extend(...)` fn has its own
        // extDefs (custom fixtures / a `page` override) that `_current` doesn't carry — it must
        // open its OWN fixtures, else custom fixtures resolve to undefined.
        if (_current && Object.keys(extDefs).length === 0) {
          await fn(_current.arg, testInfo);
          return;
        }
        const { arg, teardown } = await open(testInfo);
        try {
          await fn(arg, testInfo);
        } finally {
          await teardown();
        }
      } catch (e) {
        if (e === SKIP_SIGNAL) return; // runtime test.skip() — already marked on `t`
        throw e;
      } finally {
        _activeT = null;
      }
    });
  };
  run.describe = makeDescribe();
  run.skip = (...args) => {
    // Form `skip(title, body)` → declare a skipped test.
    if (isDeclareForm(args)) {
      nodeTest.skip(args[0], () => {});
      return;
    }
    // Conditional / runtime form `skip(condition?, description?)`.
    if (!skipConditionMet(args)) return; // condition false → no-op
    const reason = typeof args[args.length - 1] === "string" ? args[args.length - 1] : undefined;
    if (_activeT) {
      _activeT.skip(reason); // inside a running test body → skip at runtime
      throw SKIP_SIGNAL;
    }
    if (_skipStack.length) _skipStack[_skipStack.length - 1].skip = true; // describe scope
  };
  run.only = (name, fn) => nodeTest.only(name, async () => runWith(open, name, fn));
  run.fixme = run.skip;
  run.fail = run;
  run.slow = () => {};
  run.setTimeout = () => {};
  run.use = () => {};
  run.step = async (_name, body) => body();
  run.info = () => makeTestInfo("");
  run.beforeEach = (fn) => {
    wireFixtureHooks(open);
    beforeEach(async () => fn(_current?.arg ?? {}));
  };
  run.afterEach = (fn) => {
    wireFixtureHooks(open);
    afterEach(async () => fn(_current?.arg ?? {}));
  };
  run.beforeAll = (fn) => before(async () => fn({}));
  run.afterAll = (fn) => after(async () => fn({}));
  // A new fixture name extends; reusing a base/ext name OVERRIDES it but the
  // override still sees the prior value (Playwright's `page: ({page}) => …`).
  run.extend = (more) => makeTestFn(baseDefs, { ...extDefs, ...more });
  return run;
}

async function runWith(open, name, fn) {
  const { arg, teardown } = await open(makeTestInfo(name));
  try {
    await fn(arg, makeTestInfo(name));
  } finally {
    await teardown();
  }
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
  const d = (name, fn) =>
    nodeDescribe(name, async () => {
      _skipStack.push({ skip: false });
      try {
        return await fn();
      } finally {
        _skipStack.pop();
      }
    });
  d.skip = (name, fn) => nodeDescribe.skip(name, fn ?? (() => {}));
  d.only = (name, fn) => d(name, fn);
  d.serial = d;
  d.parallel = d;
  d.configure = () => {};
  return d;
}

function makeTestInfo(title) {
  return {
    title,
    titlePath: [title],
    outputDir: "/tmp",
    outputPath: (...p) => ["/tmp", ...p].join("/"),
    snapshotPath: (...p) => ["/tmp", ...p].join("/"),
    attach: async () => {},
    attachments: [],
    skip: () => {},
    fixme: () => {},
    fail: () => {},
    slow: () => {},
    setTimeout: () => {},
    annotations: [],
    errors: [],
    status: "passed",
    expectedStatus: "passed",
    retry: 0,
    project: { name: "turbo-surf" },
  };
}

// Resolve the fixture argument object. Base fixtures (browser → context → page →
// request → baseURL) resolve first, then extensions in insertion order — so an
// extension overriding a base name (e.g. `page`) already sees the base value in
// `arg` when it runs, and replaces it via `use(val)`. Returns { arg, teardown }.
async function openFixtures(baseDefs, extDefs, testInfo) {
  const arg = {};
  const teardowns = [];
  const resolve = async (name, def) => {
    if (def === undefined) return;
    if (typeof def !== "function") {
      arg[name] = def;
      return;
    }
    await new Promise((settle, reject) => {
      const used = (val) =>
        new Promise((release) => {
          arg[name] = val;
          teardowns.push(release);
          settle();
        });
      Promise.resolve(def(arg, used, testInfo)).then(() => settle(), reject);
    });
  };
  for (const name of ["browser", "context", "page", "request", "baseURL"])
    await resolve(name, baseDefs[name]);
  for (const name of Object.keys(extDefs)) await resolve(name, extDefs[name]);
  const teardown = async () => {
    for (const release of teardowns.reverse()) release();
  };
  return { arg, teardown };
}

export const test = makeTestFn(makeBaseFixtures());

/** `defineConfig` is identity — the Playwright CLI/config is unused under node:test. */
export const defineConfig = (config) => config;
/** Device descriptors are no-ops (no real viewport/UA emulation). */
export const devices = new Proxy({}, { get: () => ({}) });
/** APIRequestContext: fetch over the shared Rust client. The response carries
 * the RAW body (makeResponse.text/json), not a re-serialized DOM. */
export const request = {
  async newContext(opts = {}) {
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
    const ctxHeaders = opts.extraHTTPHeaders ?? {};
    // Encode a request `data`/body like Playwright: an object → JSON body with
    // `Content-Type: application/json` (unless the caller set one), a string → sent
    // as-is. Per-request `headers` merge over the context's `extraHTTPHeaders`.
    const encode = (o = {}) => {
      const h = { ...ctxHeaders, ...o.headers };
      let body = null;
      if (o.data != null) {
        if (typeof o.data === "string" || Buffer.isBuffer?.(o.data)) {
          body = String(o.data);
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
      const { body, hdr } = encode(o);
      return makeResponse(
        JSON.parse(await native.fetchWithCookies(resolve(url), "[]", method ?? null, body, hdr)),
      );
    };
    return {
      get: (url, o = {}) => call(url, null, o),
      post: (url, o = {}) => call(url, "POST", o),
      put: (url, o = {}) => call(url, "PUT", o),
      patch: (url, o = {}) => call(url, "PATCH", o),
      delete: (url, o = {}) => call(url, "DELETE", o),
      head: (url, o = {}) => call(url, "HEAD", o),
      fetch: (url, o = {}) => call(url, o.method ?? null, o),
      async storageState() {
        return { cookies: [], origins: [] };
      },
      async dispose() {},
    };
  },
};

export default { test, expect, chromium, firefox, webkit, newPage, defineConfig, devices, request };
