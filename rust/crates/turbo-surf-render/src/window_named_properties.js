// HTML "named access on the Window object" (turbo-surf#77). The realm's
// WindowProperties object (window_named_properties.rs, a non-masking interceptor
// between Window.prototype and EventTarget.prototype) asks this resolver for
// names nothing else on the chain defines. Supported names, in Chrome 156's
// precedence: a child browsing context's current name, then every connected
// element whose id is the name and every HTML embed/form/img/object whose name
// attribute is the name (tree order; one match is the element, several are one
// live HTMLCollection kept per name).
//
// This runs last in the bootstrap, so bootstrap feature checks never see page
// elements. The resolver uses only intrinsics captured here (a miss on the
// global reaches it, so it must never look a global up itself), and never
// resolves the engine's own bookkeeping globals.
// oxlint-disable unicorn/consistent-function-scoping
(() => {
  const W = globalThis.Window,
    ET = globalThis.EventTarget,
    E = globalThis.Element;
  if (!W?.prototype || !ET?.prototype || !E?.prototype) return;
  const apply = Reflect.apply,
    define = Object.defineProperty,
    describe = Object.getOwnPropertyDescriptor,
    NameMap = Map,
    mapGet = Map.prototype.get,
    mapSet = Map.prototype.set,
    attribute = E.prototype.getAttribute,
    namespace = describe(E.prototype, "namespaceURI")?.get,
    hasOwn = Object.hasOwn,
    InternalTypeError = TypeError,
    setRecord = ops.op_interface_record_set,
    namedElements = ops.op_window_named_elements,
    namedObject = __G.makeNamedObject,
    collectionPrototype = globalThis.HTMLCollection?.prototype,
    childFrames = ops.op_window_child_frames,
    windowView = __G.windowView,
    document = __document;
  if (
    typeof namespace !== "function" ||
    typeof namedObject !== "function" ||
    !collectionPrototype
  )
    return;
  const html = "http://www.w3.org/1999/xhtml";
  // Engine bookkeeping read back from the global at run time (fetch accounting,
  // host results, input/probe drivers, frame depth): never a page element.
  const reserved = {
    __proto__: null,
    __pendingFetches: 1,
    __RESULT: 1,
    __depth: 1,
    __hi: 1,
    __probe: 1,
  };
  const append = (rows, value) =>
    define(rows, rows.length, {
      __proto__: null,
      value,
      writable: true,
      enumerable: true,
      configurable: true,
    });
  // Matching elements in tree order, read from the native tree on every call so
  // the resolver never retains a removed element.
  const lookup = (name) => {
    const rows = namedElements(document, name);
    return rows.length ? rows : undefined;
  };
  // A child's browsing-context name is its window's own `name` data property.
  const frameName = (window) => {
    const descriptor = describe(window, "name");
    return descriptor && hasOwn(descriptor, "value") ? descriptor.value : undefined;
  };
  // An HTMLCollection over `list()` carrying the binding's collection record, so
  // its length/item/namedItem/iterator brand checks accept it (Chrome returns one
  // live collection per name while several elements share it).
  const isIndex = (key) => {
    if (key === "0") return true;
    if (key.length === 0 || key.length > 10 || key[0] < "1" || key[0] > "9") return false;
    for (let at = 1; at < key.length; at++) if (key[at] < "0" || key[at] > "9") return false;
    return +key < 4294967295;
  };
  const namedItem = (rows, key) => {
    for (let at = 0; at < rows.length; at++)
      if (apply(attribute, rows[at], ["id"]) === key) return rows[at];
    for (let at = 0; at < rows.length; at++)
      if (apply(namespace, rows[at], []) === html && apply(attribute, rows[at], ["name"]) === key)
        return rows[at];
    return undefined;
  };
  const collectionKeys = (rows) => {
    const names = [];
    for (let at = 0; at < rows.length; at++) {
      const element = rows[at],
        candidates = [
          apply(attribute, element, ["id"]),
          apply(namespace, element, []) === html ? apply(attribute, element, ["name"]) : null,
        ];
      for (let which = 0; which < 2; which++) {
        const value = candidates[which];
        if (!value || isIndex(value) || value in collectionPrototype) continue;
        let seen = false;
        for (let known = 0; known < names.length; known++) if (names[known] === value) seen = true;
        if (!seen) append(names, value);
      }
    }
    return names;
  };
  const collection = (list) => {
    const object = namedObject(
      collectionPrototype,
      (operation, key) => {
        const rows = list();
        if (operation === "indices") {
          const indices = [];
          for (let at = 0; at < rows.length; at++) append(indices, at);
          return indices;
        }
        if (operation === "keys") return collectionKeys(rows);
        const indexedKey = isIndex(key),
          found = indexedKey
            ? rows[+key]
            : key in collectionPrototype
              ? undefined
              : namedItem(rows, key);
        if (found === undefined) return undefined;
        if (operation === "get") return found;
        if (operation === "has") return true;
        if (operation === "descriptor")
          return {
            __proto__: null,
            value: found,
            writable: false,
            enumerable: indexedKey,
            configurable: true,
          };
        if (operation === "delete") return false;
        if (operation === "set" || operation === "define") return true;
        return undefined;
      },
      true,
    );
    if (!setRecord(object, { __proto__: null, kind: "HTMLCollection", read: list }))
      throw new InternalTypeError("Collection record installation interrupted");
    return object;
  };
  const collections = new NameMap(),
    none = [];
  const resolve = (name) => {
    if (name === "" || reserved[name]) return undefined;
    try {
      const children = childFrames(document);
      for (let at = 0; at < children.length; at++)
        if (frameName(children[at]) === name) return windowView(children[at]);
      const rows = lookup(name);
      if (!rows) return undefined;
      if (rows.length === 1) return rows[0];
      let created = apply(mapGet, collections, [name]);
      if (!created) {
        created = collection(() => lookup(name) || none);
        apply(mapSet, collections, [name, created]);
      }
      return created;
    } catch {
      // Chrome's named getter never throws; a page that replaced its document
      // or frames with non-platform objects just has no named items.
      return undefined;
    }
  };
  ops.op_install_window_named_properties(W.prototype, ET.prototype, resolve);
})();
