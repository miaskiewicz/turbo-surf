// CSS Font Loading (turbo-surf#130): FontFace, document.fonts (FontFaceSet) and the
// @font-face faces of the document's sheets. A face's source is fetched over the page's
// network stack (destination "font", CORS, CSP font-src); the host decodes it (WOFF/WOFF2)
// and parses it with turbo-dom's FontFace::from_web_font_bytes, and a loaded face in the
// set joins the registry live layout and canvases use. CSS faces load when a layout uses
// their family (the host reports it), as Chrome's style update starts them; the set's
// events and `ready` follow Chrome 156 (harness/detection/fixtures/font-face-chrome156.json).
(() => {
  if (!globalThis.document) return;
  const __fontDocument = __document;
  const __fontTask = (callback) => setTimeout(callback, 0);
  const __fontArrayBuffer = Response.prototype.arrayBuffer;
  // Live geometry replaces Element#getBoundingClientRect after this bootstrap.
  let __fontRect = null;
  const __fontRoot = Object.getOwnPropertyDescriptor(Document.prototype, "documentElement").get;
  const __fontCssText = Object.getOwnPropertyDescriptor(CSSRule.prototype, "cssText").get;
  const __fontSheets = Object.getOwnPropertyDescriptor(Document.prototype, "styleSheets").get;
  const __fontDescriptor = (css) => JSON.parse(ops.op_font_face_rule(css));
  const __fontWeight = (text) => {
    const word = String(text || "normal")
      .trim()
      .split(/\s+/)[0];
    return word === "bold" ? 700 : word === "normal" ? 400 : Number(word) || 400;
  };
  const __fontItalic = (text) => /^(italic|oblique)/.test(String(text || ""));
  const __fontQuote = (family) =>
    /^-?[A-Za-z_][\w-]*$/.test(family) ? family : `"${family.replace(/["\\]/g, "\\$&")}"`;
  const __fontUnquote = (family) => {
    const parsed = __fontDescriptor(`@font-face{font-family:${family}}`);
    return parsed && typeof parsed.family === "string" ? parsed.family : family;
  };
  const __fontFormats = ["truetype", "opentype", "woff", "woff2", "collection"];
  const __fontState = new WeakMap();
  const state = (face) => {
    const value = __fontState.get(face);
    if (!value) throw new TypeError("Illegal invocation");
    return value;
  };
  const __fontToken = Symbol();

  const fail = (face, error) => {
    const item = state(face);
    item.status = "error";
    item.reject(error);
    item.loaded.catch(() => {});
  };
  const settle = (face, ok, error) => {
    const item = state(face);
    if (ok) {
      item.status = "loaded";
      item.resolve(face);
    } else fail(face, error || new DOMException("A network error occurred.", "NetworkError"));
    for (const set of item.sets) set.finished(face, ok);
  };
  const parsed = (face, bytes) => {
    const item = state(face);
    const id = ops.op_font_face_parse(
      bytes,
      __fontUnquote(item.family),
      __fontWeight(item.weight),
      __fontItalic(item.style),
    );
    if (id) item.id = id;
    return id;
  };
  const dataBytes = (url) => {
    const comma = url.indexOf(",");
    const head = url.slice(5, comma);
    const body = url.slice(comma + 1);
    const text = /;base64$/i.test(head) ? atob(decodeURIComponent(body)) : decodeURIComponent(body);
    return Uint8Array.from(text, (character) => character.charCodeAt(0) & 255);
  };
  async function fetchSource(face, sources, base) {
    for (const source of sources) {
      if (!source.url) continue;
      if (source.format && !__fontFormats.includes(source.format)) continue;
      let url;
      try {
        url = new URL(source.url, base).href;
      } catch {
        continue;
      }
      if (!ops.op_csp_request(__fontDocument, url, "font-src", "")) continue;
      try {
        let bytes;
        if (/^data:/i.test(url)) bytes = dataBytes(url);
        else {
          const response = await __fetchFromSource(
            url,
            { destination: "font", mode: "cors", credentials: "same-origin" },
            base,
          );
          if (!response.ok) continue;
          bytes = new Uint8Array(await __intrinsicApply(__fontArrayBuffer, response, []));
        }
        if (parsed(face, bytes)) return true;
      } catch {}
    }
    return false;
  }
  // Start a URL face's load: status changes synchronously; CSP-blocked sources are
  // dropped at once (a face with none left errors immediately, as in Chrome).
  const startLoad = (face) => {
    const item = state(face);
    if (item.status !== "unloaded") return item.loaded;
    item.status = "loading";
    for (const set of item.sets) set.started(face);
    const allowed = item.sources.filter((source) => {
      if (!source.url) return false;
      try {
        return ops.op_csp_request(
          __fontDocument,
          new URL(source.url, item.base).href,
          "font-src",
          "",
        );
      } catch {
        return false;
      }
    });
    if (!allowed.length) settle(face, false);
    else fetchSource(face, allowed, item.base).then((ok) => settle(face, ok));
    return item.loaded;
  };

  class FontFace {
    constructor(family, source, descriptors = {}) {
      if (arguments.length < 2)
        throw new TypeError(
          `Failed to construct 'FontFace': 2 arguments required, but only ${arguments.length} present.`,
        );
      let resolve, reject;
      const loaded = new Promise((done, failed) => {
        resolve = done;
        reject = failed;
      });
      const item = {
        family: __fontQuote(String(family)),
        style: "normal",
        weight: "normal",
        stretch: "normal",
        unicodeRange: "U+0-10FFFF",
        status: "unloaded",
        loaded,
        resolve,
        reject,
        sets: new Set(),
        sources: [],
        base: __fontDocument.baseURI,
        id: 0,
        css: false,
      };
      __fontState.set(this, item);
      if (descriptors === __fontToken) return;
      const css = ["style", "weight", "stretch", "unicodeRange"];
      const names = {
        style: "font-style",
        weight: "font-weight",
        stretch: "font-stretch",
        unicodeRange: "unicode-range",
      };
      for (const key of css) {
        if (descriptors[key] === undefined) continue;
        const text = String(descriptors[key]);
        const value = __fontDescriptor(`@font-face{${names[key]}:${text}}`);
        if (!value || value[key] === undefined) {
          fail(
            this,
            new DOMException(`Failed to set '${text}' as a property value.`, "SyntaxError"),
          );
          return;
        }
        item[key] = value[key];
      }
      if (typeof source === "string") {
        const value = __fontDescriptor(`@font-face{src:${source}}`);
        if (!value || !value.src) {
          fail(
            this,
            new DOMException(`Failed to set '${source}' as a property value.`, "SyntaxError"),
          );
          return;
        }
        item.sources = value.src;
        return;
      }
      const bytes = ArrayBuffer.isView(source)
        ? new Uint8Array(source.buffer, source.byteOffset, source.byteLength)
        : new Uint8Array(source);
      item.status = "loading";
      if (parsed(this, bytes)) {
        item.status = "loaded";
        resolve(this);
      } else
        fail(
          this,
          new DOMException(
            "The source provided could not be parsed as a value list.",
            "SyntaxError",
          ),
        );
    }
    load() {
      return startLoad(this);
    }
    get loaded() {
      return state(this).loaded;
    }
    get status() {
      return state(this).status;
    }
    get family() {
      return state(this).family;
    }
    get style() {
      return state(this).style;
    }
    get weight() {
      return state(this).weight;
    }
    get stretch() {
      return state(this).stretch;
    }
    get unicodeRange() {
      return state(this).unicodeRange;
    }
    get display() {
      return "auto";
    }
    get featureSettings() {
      return "normal";
    }
  }
  Object.defineProperty(FontFace.prototype, Symbol.toStringTag, {
    value: "FontFace",
    configurable: true,
  });

  // CSS-connected faces, one per @font-face rule text and sheet base.
  const cssFaces = new Map();
  const cssFace = (text, base) => {
    const key = `${base}\n${text}`;
    let face = cssFaces.get(key);
    if (face) return face;
    const value = __fontDescriptor(text);
    if (!value || typeof value.family !== "string" || !value.src) return null;
    face = new FontFace(value.family, "", __fontToken);
    const item = state(face);
    Object.assign(item, {
      family: value.family,
      style: value.style || "normal",
      weight: value.weight || "normal",
      stretch: value.stretch || "normal",
      unicodeRange: value.unicodeRange || "U+0-10FFFF",
      sources: value.src,
      base,
      css: true,
    });
    cssFaces.set(key, face);
    return face;
  };
  const collect = (rules, base, out) => {
    for (let index = 0; index < rules.length; index++) {
      const rule = rules[index];
      if (rule.type === 5) {
        const face = cssFace(__intrinsicApply(__fontCssText, rule, []), base);
        if (face) out.push(face);
      } else if (rule.type === 4 && rule.media && matchMedia(rule.media.mediaText).matches)
        collect(rule.cssRules, base, out);
      else if (rule.type === 12 && CSS.supports(rule.conditionText))
        collect(rule.cssRules, base, out);
    }
  };
  const documentFaces = () => {
    const out = [];
    try {
      const list = __intrinsicApply(__fontSheets, __fontDocument, []);
      for (let index = 0; index < list.length; index++) {
        const sheet = list[index];
        if (sheet.disabled) continue;
        collect(__resourceSheetRules(sheet), sheet.href || __fontDocument.baseURI, out);
      }
    } catch {}
    return out;
  };

  class FontFaceSetLoadEvent extends Event {
    #faces;
    constructor(type, init = {}) {
      super(type, init);
      this.#faces = Object.freeze(Array.from(init.fontfaces || []));
    }
    get fontfaces() {
      return this.#faces;
    }
  }
  const sets = new WeakMap();
  class FontFaceSet extends EventTarget {
    constructor(token) {
      if (token !== __fontToken) throw new TypeError("Illegal constructor");
      super();
      let resolve;
      const ready = new Promise((done) => (resolve = done));
      resolve(this);
      sets.set(this, {
        added: new Set(),
        css: [],
        loading: 0,
        done: [],
        failed: [],
        ready,
        resolve: null,
        started: (face) => this.#started(face),
        finished: (face, ok) => this.#finished(face, ok),
      });
    }
    #state() {
      const value = sets.get(this);
      if (!value) throw new TypeError("Illegal invocation");
      return value;
    }
    #faces() {
      const item = this.#state();
      const css = documentFaces();
      for (const face of item.css) if (!css.includes(face)) this.#leave(face);
      for (const face of css) if (!item.css.includes(face)) this.#join(face);
      item.css = css;
      return [...css, ...item.added];
    }
    #join(face) {
      const value = state(face);
      value.sets.add(this.#state());
      if (value.status === "loaded" && value.id) ops.op_font_face_attach(__fontDocument, value.id);
      if (value.status === "loading") this.#started(face);
    }
    #leave(face) {
      const value = state(face);
      value.sets.delete(this.#state());
      if (value.id) ops.op_font_face_detach(__fontDocument, value.id);
    }
    #started() {
      const item = this.#state();
      if (item.loading++ === 0 && !item.resolve) {
        item.ready = new Promise((done) => (item.resolve = done));
        __fontTask(() =>
          this.dispatchEvent(new FontFaceSetLoadEvent("loading", { fontfaces: [] })),
        );
      }
    }
    #finished(face, ok) {
      const item = this.#state();
      (ok ? item.done : item.failed).push(face);
      if (ok && state(face).id) ops.op_font_face_attach(__fontDocument, state(face).id);
      if (--item.loading) return;
      __fontTask(() => {
        if (item.loading) return;
        const done = item.done.splice(0);
        const failed = item.failed.splice(0);
        this.dispatchEvent(new FontFaceSetLoadEvent("loadingdone", { fontfaces: done }));
        if (failed.length)
          this.dispatchEvent(new FontFaceSetLoadEvent("loadingerror", { fontfaces: failed }));
        const resolve = item.resolve;
        item.resolve = null;
        resolve?.(this);
      });
    }
    // The host reports the @font-face families a layout used.
    used(names) {
      for (const face of this.#faces()) {
        const value = state(face);
        if (
          value.css &&
          value.status === "unloaded" &&
          names.includes(__fontUnquote(value.family).toLowerCase())
        )
          startLoad(face);
      }
    }
    get ready() {
      // Chrome has laid the page out by the time a script waits on ready; surf lays
      // out on demand, so a wait first runs the style/layout that starts used faces.
      if (this.#faces().some((face) => state(face).css && state(face).status === "unloaded")) {
        try {
          const root = __intrinsicApply(__fontRoot, __fontDocument, []);
          __fontRect ||= Element.prototype.getBoundingClientRect;
          if (root) __intrinsicApply(__fontRect, root, []);
        } catch {}
      }
      return this.#state().ready;
    }
    get status() {
      return this.#faces().some((face) => state(face).status === "loading") ? "loading" : "loaded";
    }
    get size() {
      return this.#faces().length;
    }
    add(face) {
      state(face);
      const item = this.#state();
      if (state(face).css)
        throw new DOMException("Cannot add a CSS-connected FontFace.", "InvalidModificationError");
      if (!item.added.has(face)) {
        item.added.add(face);
        this.#join(face);
      }
      return this;
    }
    delete(face) {
      const item = this.#state();
      if (!item.added.delete(face)) return false;
      this.#leave(face);
      return true;
    }
    clear() {
      const item = this.#state();
      for (const face of item.added) this.#leave(face);
      item.added.clear();
    }
    has(face) {
      return this.#faces().includes(face);
    }
    #matching(font) {
      const probe = document.createElement("canvas").getContext("2d");
      probe.font = String(font);
      const parsed = probe.font;
      const family = parsed.replace(/^.*?\d+(\.\d+)?px(\/\S+)?\s+/, "");
      const names = family.split(",").map((name) => __fontUnquote(name.trim()).toLowerCase());
      return this.#faces().filter((face) =>
        names.includes(__fontUnquote(state(face).family).toLowerCase()),
      );
    }
    check(font, _text = " ") {
      return this.#matching(font).every((face) => state(face).status === "loaded");
    }
    load(font, _text = " ") {
      const faces = this.#matching(font);
      return Promise.all(faces.map((face) => startLoad(face))).then(() => faces);
    }
    forEach(callback, thisArg) {
      for (const face of this.#faces()) callback.call(thisArg, face, face, this);
    }
    values() {
      return this.#faces()[Symbol.iterator]();
    }
    keys() {
      return this.values();
    }
    entries() {
      return this.#faces()
        .map((face) => [face, face])
        [Symbol.iterator]();
    }
    [Symbol.iterator]() {
      return this.values();
    }
  }
  Object.defineProperty(FontFaceSet.prototype, Symbol.toStringTag, {
    value: "FontFaceSet",
    configurable: true,
  });
  for (const type of ["loading", "loadingdone", "loadingerror"]) {
    let handler = null;
    Object.defineProperty(FontFaceSet.prototype, `on${type}`, {
      configurable: true,
      enumerable: true,
      get() {
        return handler;
      },
      set(value) {
        if (handler) this.removeEventListener(type, handler);
        handler = typeof value === "function" ? value : null;
        if (handler) this.addEventListener(type, handler);
      },
    });
  }
  const fonts = new FontFaceSet(__fontToken);
  const used = fonts.used;
  delete FontFaceSet.prototype.used;
  __G.fontLayoutUsed = (names) => {
    try {
      __intrinsicApply(used, fonts, [names]);
    } catch {}
  };
  for (const [name, value] of [
    ["FontFace", FontFace],
    ["FontFaceSet", FontFaceSet],
    ["FontFaceSetLoadEvent", FontFaceSetLoadEvent],
  ])
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value });
  // On the prototype, as in Chrome: an own accessor on the document wrapper would make
  // the binding keep that wrapper (and its realm) alive past a retired frame.
  Object.defineProperty(Document.prototype, "fonts", {
    configurable: true,
    enumerable: true,
    get() {
      return this === __fontDocument ? fonts : undefined;
    },
  });
})();
