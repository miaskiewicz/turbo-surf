// Web IDL attribute/operation descriptors for the interfaces this runtime implements with class
// syntax (#28, harness/detection/interface-shapes-probe.js): every regular and static member is
// enumerable, as Chrome exposes it, and arities follow Chrome 156. Constants are already
// non-configurable and enumerable; symbol-keyed members keep their own attributes.
(() => {
  "use strict";
  const define = Object.defineProperty,
    describe = Object.getOwnPropertyDescriptor,
    keys = Object.getOwnPropertyNames;
  const enumerable = (object, skip) => {
    if (!object) return;
    for (const key of keys(object)) {
      if (skip.includes(key)) continue;
      const descriptor = describe(object, key);
      if (descriptor && descriptor.configurable && !descriptor.enumerable)
        define(object, key, { __proto__: null, enumerable: true });
    }
  };
  for (const name of [
    "DOMMatrix",
    "DOMMatrixReadOnly",
    "DOMPoint",
    "DOMPointReadOnly",
    "DOMQuad",
    "Headers",
    "Request",
    "Response",
    "TextDecoder",
    "TextEncoder",
    "URL",
    "URLSearchParams",
  ]) {
    const constructor = globalThis[name];
    if (typeof constructor !== "function") continue;
    enumerable(constructor.prototype, ["constructor"]);
    enumerable(constructor, ["length", "name", "prototype"]);
  }
  const arity = (object, name, length) => {
    const descriptor = object && describe(object, name);
    if (descriptor && typeof descriptor.value === "function")
      define(descriptor.value, "length", { __proto__: null, value: length });
  };
  arity(globalThis.URL, "revokeObjectURL", 1);
})();
