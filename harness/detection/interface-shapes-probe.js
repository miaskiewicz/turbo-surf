(() => {
  "use strict";
  // Descriptor shapes of the interfaces the #28 interface sweep normalises: every own member of
  // each prototype and constructor (sorted; key order is measured elsewhere), the constructor's
  // parent and its global binding. A member is summarised by kind, attributes and, for functions,
  // name/length/constructibility/native source.
  const fn = (f) => {
    if (typeof f !== "function") return typeof f;
    let constructible = true;
    try {
      Reflect.construct(String, [], f);
    } catch {
      constructible = false;
    }
    return {
      name: f.name,
      length: f.length,
      constructible,
      prototype: Object.hasOwn(f, "prototype"),
      native: Function.prototype.toString.call(f) === `function ${f.name}() { [native code] }`,
    };
  };
  const desc = (d) => {
    if (!d) return null;
    const r = { c: d.configurable, e: d.enumerable };
    if ("value" in d) {
      r.w = d.writable;
      r.v = typeof d.value === "function" ? fn(d.value) : d.value === null ? "null" : typeof d.value === "object" ? "object" : d.value;
    } else {
      r.get = fn(d.get);
      r.set = fn(d.set);
    }
    return r;
  };
  const members = (object, only) =>
    Object.fromEntries(
      Reflect.ownKeys(object)
        .filter((k) => !only || only.includes(String(k)))
        .map((k) => [String(k), desc(Object.getOwnPropertyDescriptor(object, k))])
        .sort((a, b) => (a[0] < b[0] ? -1 : 1)),
    );
  const names = [
    "AbortController", "AbortSignal", "Blob", "File", "FormData", "Headers", "Request", "Response",
    "URL", "URLSearchParams", "TextEncoder", "TextDecoder", "DOMMatrix", "DOMMatrixReadOnly",
    "DOMPoint", "DOMPointReadOnly", "DOMRect", "DOMRectReadOnly", "DOMRectList", "CaretPosition",
    "DOMTokenList", "HTMLCollection", "NodeList", "IntersectionObserver", "ResizeObserver",
    "PerformanceObserver", "MutationObserver", "FileReader", "XMLHttpRequest",
    "XMLHttpRequestEventTarget", "XMLHttpRequestUpload", "WebSocket", "Navigator", "Window",
    "Screen", "CustomEvent", "PopStateEvent", "HashChangeEvent", "BroadcastChannel", "MessageChannel",
    "MessagePort", "Performance", "Storage",
  ];
  // Interfaces whose other members belong to other parity work: only these members are compared.
  const subsets = {
    Document: ["createNodeIterator", "createTreeWalker", "embeds", "plugins"],
    HTMLCanvasElement: ["toBlob", "toDataURL", "getContext"],
    HTMLMediaElement: ["loading", "canPlayType", "NETWORK_IDLE", "HAVE_ENOUGH_DATA"],
  };
  const out = {};
  for (const name of [...names, ...Object.keys(subsets)]) {
    const C = globalThis[name];
    if (typeof C !== "function") {
      out[name] = typeof C;
      continue;
    }
    const parent = Object.getPrototypeOf(C);
    out[name] = {
      global: desc(Object.getOwnPropertyDescriptor(globalThis, name)),
      parent: parent === Function.prototype ? "Function.prototype" : typeof parent === "function" ? parent.name : String(parent),
      shape: { ...fn(C), prototypeDescriptor: desc(Object.getOwnPropertyDescriptor(C, "prototype")) },
      constructor: members(C, subsets[name] && ["length", "name", "prototype", ...subsets[name]]),
      prototype: C.prototype !== null && typeof C.prototype === "object" ? members(C.prototype, subsets[name]) : String(C.prototype),
    };
  }
  return out;
})()
