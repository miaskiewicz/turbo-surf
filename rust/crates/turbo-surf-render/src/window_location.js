// History and Location (#28, measured in harness/detection/history-location-probe.js).
// Location carries Chrome's [LegacyUnforgeable] own members on an immutable-prototype
// instance; History keeps this document's session history entries, so pushState and
// fragment navigations update the URL in place and traversals fire popstate/hashchange
// as Chrome orders them. Cross-document navigations keep the engine's contract: the URL
// is admitted and the session driver loads it as a new page.
(() => {
  "use strict";
  const define = Object.defineProperty,
    apply = Reflect.apply,
    make = __G.makeNativeFunction;
  const LocationURL = globalThis.URL,
    StringValue = String,
    ErrorType = __exceptionTypeError;
  const Exception = DOMException,
    clone = globalThis.structuredClone,
    hostTimeout = globalThis.setTimeout;
  const PopState = globalThis.PopStateEvent,
    HashChange = globalThis.HashChangeEvent;
  const arrayPush = Array.prototype.push,
    arraySplice = Array.prototype.splice,
    indexOf = String.prototype.indexOf,
    slice = String.prototype.slice;
  const nativeLocation = ops.op_native_location_installed();
  const realm = globalThis;
  const string = (value) => {
    if (typeof value === "symbol") throw new ErrorType("Cannot convert a Symbol to a string");
    return StringValue(value);
  };
  const parse = (value, base) => {
    try {
      return new LocationURL(value, base);
    } catch {
      return null;
    }
  };
  const withoutFragment = (href) => {
    const at = apply(indexOf, href, ["#"]);
    return at < 0 ? href : apply(slice, href, [0, at]);
  };
  const fragmentOf = (href) => {
    const at = apply(indexOf, href, ["#"]);
    return at < 0 ? null : apply(slice, href, [at + 1]);
  };
  const method = (object, name, length, implementation, attributes) =>
    define(object, name, {
      __proto__: null,
      value: make(implementation, name, length),
      writable: true,
      enumerable: true,
      configurable: true,
      ...attributes,
    });
  const accessor = (object, name, get, set, attributes) =>
    define(object, name, {
      __proto__: null,
      get: make(get, "get " + name, 0),
      set: set ? make(set, "set " + name, 1) : undefined,
      enumerable: true,
      configurable: true,
      ...attributes,
    });
  const interfaceObject = (name) => {
    const constructor = make(
      function () {
        throw new ErrorType("Illegal constructor");
      },
      name,
      0,
      true,
    );
    define(constructor.prototype, Symbol.toStringTag, {
      __proto__: null,
      value: name,
      configurable: true,
    });
    define(constructor, "prototype", { __proto__: null, writable: false });
    define(globalThis, name, {
      __proto__: null,
      value: constructor,
      writable: true,
      configurable: true,
      enumerable: false,
    });
    return constructor;
  };
  const documentBase = () => {
    try {
      return apply(__baseURI, __document, []);
    } catch {
      return url();
    }
  };
  const trusted = (event) => {
    ops.op_event_trust(event, -1);
    ops.op_dispatch_host_event(event, realm);
  };
  const task = (callback) => apply(hostTimeout, realm, [callback, 0]);

  // ---- Location -------------------------------------------------------------------------
  const seeded = (globalThis.location && globalThis.location.href) || "about:blank";
  let location,
    current = seeded,
    nativeHref = null;
  if (nativeLocation) {
    location = globalThis.location;
    nativeHref = Object.getOwnPropertyDescriptor(location, "href").get;
  } else {
    const pair = ops.op_location_object();
    const Location = pair[0];
    location = pair[1];
    define(Location.prototype, Symbol.toStringTag, {
      __proto__: null,
      value: "Location",
      configurable: true,
    });
    define(globalThis, "Location", {
      __proto__: null,
      value: Location,
      writable: true,
      configurable: true,
      enumerable: false,
    });
  }
  const url = () => (nativeHref ? apply(nativeHref, location, []) : current);
  // Same-document URL updates; the native-proxy Location keeps its own copy of the URL.
  const moveTo = (href) => {
    if (nativeHref) ops.op_location_seed(href);
    else current = href;
  };
  const checkLocation = (receiver) => {
    if (receiver !== location) throw new ErrorType("Illegal invocation");
    if (!ops.op_windows_same_origin(realm))
      throw new Exception("Cross-origin Location access", "SecurityError");
  };

  // ---- Session history --------------------------------------------------------------------
  // Entries of this document only: the engine loads every other document as a new page.
  const entries = [{ __proto__: null, url: seeded, serialized: null, scroll: "auto" }];
  let index = 0,
    stateObject = null;
  const activeEntry = () => entries[index];
  const appendEntry = (entry) => {
    apply(arraySplice, entries, [index + 1, entries.length - index - 1]);
    apply(arrayPush, entries, [entry]);
    index = entries.length - 1;
  };
  const deserialize = (serialized) => (serialized === null ? null : clone(serialized));
  const hashChange = (oldURL, newURL) => {
    if (
      withoutFragment(oldURL) === withoutFragment(newURL) &&
      fragmentOf(oldURL) !== fragmentOf(newURL)
    )
      task(() => trusted(new HashChange("hashchange", { oldURL, newURL })));
  };
  // HTML "navigate to a fragment": a new (or replacing) entry, popstate synchronously and
  // hashchange as a queued task, as Chrome 156 delivers them.
  function navigateToFragment(href, replace) {
    const oldURL = url();
    const entry = { __proto__: null, url: href, serialized: null, scroll: activeEntry().scroll };
    if (replace) entries[index] = entry;
    else appendEntry(entry);
    moveTo(href);
    stateObject = null;
    trusted(new PopState("popstate", { state: null }));
    hashChange(oldURL, href);
  }
  function navigate(href, replace) {
    // A sandboxed frame without allow-top-navigation may not navigate top (turbo-surf#84).
    if (ops.op_location_navigation_sandboxed(realm))
      throw new Exception(
        `The current window does not have permission to navigate the target frame to '${href}'.`,
        "SecurityError",
      );
    const target = url();
    if (fragmentOf(href) !== null && withoutFragment(href) === withoutFragment(target)) {
      navigateToFragment(href, replace || href === target);
      return;
    }
    // A cross-document navigation: the session driver loads the admitted URL.
    if (nativeHref) {
      ops.op_location_seed(href);
    } else current = href;
    ops.op_script_navigation_admitted(href);
  }
  // The native cross-origin Location (native engine) navigates this document through its
  // own Location: same-document fragments, session history and the sandbox check
  // (turbo-surf#84).
  ops.op_set_host_capabilities({
    __proto__: null,
    navigateLocation: (href) => navigate(href, false),
  });
  const resolve = (value, operation) => {
    const target = parse(value, documentBase());
    if (!target) throw new Exception(`${operation}: '${value}' is not a valid URL.`, "SyntaxError");
    return target.href;
  };

  if (!nativeLocation) {
    const component = (name) => {
      const parsed = parse(url());
      return parsed ? parsed[name] : "";
    };
    accessor(
      location,
      "href",
      function () {
        checkLocation(this);
        return url();
      },
      function (value) {
        checkLocation(this);
        value = string(value);
        navigate(resolve(value, "Failed to set the 'href' property on 'Location'"), false);
      },
      { configurable: false },
    );
    for (const name of [
      "origin",
      "protocol",
      "host",
      "hostname",
      "port",
      "pathname",
      "search",
      "hash",
    ]) {
      accessor(
        location,
        name,
        function () {
          checkLocation(this);
          return component(name);
        },
        name === "origin"
          ? null
          : function (value) {
              checkLocation(this);
              value = string(value);
              const copy = parse(url());
              if (!copy) return;
              if (name === "hash") {
                // Setting the current fragment again is not a navigation.
                const before = fragmentOf(copy.href) ?? "";
                copy.hash = value;
                if ((fragmentOf(copy.href) ?? "") === before) return;
              } else {
                copy[name] = value;
                if (name === "protocol" && copy.protocol !== "http:" && copy.protocol !== "https:")
                  return;
              }
              navigate(copy.href, false);
            },
        { configurable: false },
      );
    }
    for (const [name, replace] of [
      ["assign", false],
      ["replace", true],
    ])
      method(
        location,
        name,
        1,
        function (value) {
          checkLocation(this);
          if (!arguments.length)
            throw new ErrorType(
              `Failed to execute '${name}' on 'Location': 1 argument required, but only 0 present.`,
            );
          value = string(value);
          navigate(resolve(value, `Failed to execute '${name}' on 'Location'`), replace);
        },
        { writable: false, configurable: false },
      );
    // A reload is a navigation to the current URL: the engine admits it for the driver.
    method(
      location,
      "reload",
      0,
      function () {
        checkLocation(this);
        ops.op_script_navigation_admitted(url());
      },
      { writable: false, configurable: false },
    );
    method(
      location,
      "toString",
      0,
      function () {
        checkLocation(this);
        return url();
      },
      { writable: false, configurable: false },
    );
    define(location, "valueOf", {
      __proto__: null,
      value: Object.prototype.valueOf,
      writable: false,
      enumerable: false,
      configurable: false,
    });
    define(location, Symbol.toPrimitive, {
      __proto__: null,
      value: undefined,
      writable: false,
      enumerable: false,
      configurable: false,
    });
    // ancestorOrigins: a DOMStringList of the ancestor browsing contexts' origins.
    const lists = new WeakMap(),
      listGet = WeakMap.prototype.get,
      listSet = WeakMap.prototype.set;
    const StringList = interfaceObject("DOMStringList");
    {
      const P = StringList.prototype;
      const rows = (receiver) => {
        const value =
          receiver !== null && typeof receiver === "object"
            ? apply(listGet, lists, [receiver])
            : undefined;
        if (!value) throw new ErrorType("Illegal invocation");
        return value;
      };
      accessor(P, "length", function () {
        return rows(this).length;
      });
      method(P, "item", 1, function (at) {
        const values = rows(this);
        if (!arguments.length)
          throw new ErrorType(
            "Failed to execute 'item' on 'DOMStringList': 1 argument required, but only 0 present.",
          );
        return values[+at >>> 0] ?? null;
      });
      method(P, "contains", 1, function (value) {
        const values = rows(this);
        if (!arguments.length)
          throw new ErrorType(
            "Failed to execute 'contains' on 'DOMStringList': 1 argument required, but only 0 present.",
          );
        value = string(value);
        for (let at = 0; at < values.length; at++) if (values[at] === value) return true;
        return false;
      });
      define(P, Symbol.iterator, {
        __proto__: null,
        value: Array.prototype.values,
        writable: true,
        configurable: true,
      });
    }
    const origins = ops.op_location_ancestor_origins();
    const ancestors = __G.makeNamedObject(
      StringList.prototype,
      function (operation, key) {
        const indexed = /^(0|[1-9][0-9]*)$/.test(key) && +key < origins.length;
        if (operation === "indices") {
          const indices = [];
          for (let at = 0; at < origins.length; at++) indices[at] = at;
          return indices;
        }
        if (operation === "keys") return [];
        if (!indexed) return undefined;
        if (operation === "get") return origins[+key];
        if (operation === "has") return true;
        if (operation === "descriptor")
          return { value: origins[+key], writable: false, enumerable: true, configurable: true };
        if (operation === "delete") return false;
        if (operation === "set" || operation === "define") return true;
        return undefined;
      },
      true,
    );
    apply(listSet, lists, [ancestors, origins]);
    accessor(
      location,
      "ancestorOrigins",
      function () {
        checkLocation(this);
        return ancestors;
      },
      null,
      { configurable: false },
    );
    // window.location is [LegacyUnforgeable] (non-configurable). A realm reused for its next
    // document re-runs this bootstrap: the existing accessor then follows the new Location.
    const installed = Object.getOwnPropertyDescriptor(globalThis, "location");
    if (installed && !installed.configurable && typeof __G.retargetLocation === "function")
      __G.retargetLocation(location);
    else {
      let installedLocation = location;
      define(__G, "retargetLocation", {
        __proto__: null,
        value: (next) => {
          installedLocation = next;
        },
        configurable: true,
      });
      define(globalThis, "location", {
        __proto__: null,
        get: make(
          function () {
            return installedLocation;
          },
          "get location",
          0,
        ),
        set: make(
          function (value) {
            installedLocation.href = value;
          },
          "set location",
          1,
        ),
        enumerable: true,
        configurable: false,
      });
    }
  }

  // ---- History ------------------------------------------------------------------------
  const History = interfaceObject("History");
  const H = History.prototype;
  const history = Object.create(H);
  const checkHistory = (receiver) => {
    if (receiver !== history) throw new ErrorType("Illegal invocation");
  };
  // "Can have its URL rewritten": same scheme/credentials/host/port; beyond http(s), only the
  // query and fragment of a file: URL, and only the fragment otherwise, may change.
  const rewritable = (document, target) => {
    if (
      document.protocol !== target.protocol ||
      document.username !== target.username ||
      document.password !== target.password ||
      document.host !== target.host
    )
      return false;
    if (document.protocol === "http:" || document.protocol === "https:") return true;
    if (document.protocol === "file:") return document.pathname === target.pathname;
    return withoutFragment(document.href) === withoutFragment(target.href);
  };
  function updateState(operation, data, unused, address) {
    unused = string(unused);
    const hasURL = address !== undefined && address !== null;
    if (hasURL) address = string(address);
    let serialized;
    try {
      serialized = ops.op_surf_structured_clone(
        data,
        globalThis,
        undefined,
        `Failed to execute '${operation}' on 'History': `,
      );
    } catch (error) {
      if (error && error.name === "DataCloneError") throw error;
      throw new Exception(
        `Failed to execute '${operation}' on 'History': ${(error && error.message) || "An object could not be cloned."}`,
        "DataCloneError",
      );
    }
    let target = url();
    if (hasURL) {
      const documentURL = parse(target);
      const parsed = parse(address, documentBase());
      if (!parsed || !documentURL || !rewritable(documentURL, parsed))
        throw new Exception(
          `Failed to execute '${operation}' on 'History': A history state object with URL '${parsed ? parsed.href : address}' cannot be created in a document with origin '${documentURL ? documentURL.origin : "null"}' and URL '${target}'.`,
          "SecurityError",
        );
      target = parsed.href;
    }
    const entry = { __proto__: null, url: target, serialized, scroll: activeEntry().scroll };
    if (operation === "pushState") appendEntry(entry);
    else entries[index] = entry;
    if (target !== url()) {
      moveTo(target);
      // The session driver follows a changed document URL as it does any script navigation.
      ops.op_script_navigation_admitted(target);
    }
    stateObject = deserialize(serialized);
  }
  function traverse(delta) {
    task(() => {
      const target = index + delta;
      if (target < 0 || target >= entries.length || delta === 0) return;
      const oldURL = url();
      index = target;
      const entry = entries[index];
      if (entry.url !== oldURL) moveTo(entry.url);
      stateObject = deserialize(entry.serialized);
      trusted(new PopState("popstate", { state: stateObject }));
      hashChange(oldURL, entry.url);
    });
  }
  accessor(H, "length", function () {
    checkHistory(this);
    return entries.length;
  });
  accessor(
    H,
    "scrollRestoration",
    function () {
      checkHistory(this);
      return activeEntry().scroll;
    },
    function (value) {
      checkHistory(this);
      value = string(value);
      if (value === "auto" || value === "manual") activeEntry().scroll = value;
    },
  );
  accessor(H, "state", function () {
    checkHistory(this);
    return stateObject;
  });
  method(H, "back", 0, function () {
    checkHistory(this);
    traverse(-1);
  });
  method(H, "forward", 0, function () {
    checkHistory(this);
    traverse(1);
  });
  method(H, "go", 0, function (delta = 0) {
    checkHistory(this);
    delta = +delta | 0;
    // go(0) reloads the document, which the engine admits as a navigation to its URL.
    if (delta === 0) ops.op_script_navigation_admitted(url());
    else traverse(delta);
  });
  for (const name of ["pushState", "replaceState"])
    method(H, name, 2, function (data, unused, address = undefined) {
      checkHistory(this);
      if (arguments.length < 2)
        throw new ErrorType(
          `Failed to execute '${name}' on 'History': 2 arguments required, but only ${arguments.length} present.`,
        );
      updateState(name, data, unused, address);
    });
  define(globalThis, "history", {
    __proto__: null,
    get: make(
      function () {
        return history;
      },
      "get history",
      0,
    ),
    set: undefined,
    enumerable: true,
    configurable: true,
  });

  // document.location is the window's Location while the document is active ([PutForwards=href]).
  if (globalThis.document)
    define(globalThis.document, "location", {
      __proto__: null,
      get: make(
        function () {
          return __documentState(this).window.location;
        },
        "get location",
        0,
      ),
      set: make(
        function (value) {
          const target = __documentState(this).window.location;
          if (target) target.href = value;
        },
        "set location",
        1,
      ),
      enumerable: true,
      configurable: false,
    });
})();
