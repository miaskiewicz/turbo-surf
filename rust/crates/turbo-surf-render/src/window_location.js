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
    stringify = JSON.stringify,
    setPrototypeOf = Object.setPrototypeOf,
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
  // Navigation API identity: replacing an entry keeps its key and mints a new id.
  const uid = () =>
    "xxxxxxxx-xxxx-4xxx-8xxx-xxxxxxxxxxxx".replace(/x/g, () =>
      ((Math.random() * 16) | 0).toString(16),
    );
  let entrySerial = 0;
  const stamp = (entry, key, navState) => {
    entry.hostId = ++entrySerial;
    entry.key = key || uid();
    entry.id = uid();
    entry.navState = navState;
    return entry;
  };
  let navigationHooks = null;
  const entries = [stamp({ __proto__: null, url: seeded, serialized: null, scroll: "auto" })];
  let index = 0,
    stateObject = null;
  const documentId = ops.op_history_document_id(__document);
  const initialEntry = { __proto__: null, id: entries[0].hostId, url: seeded };
  const changeList = () => apply(setPrototypeOf, null, [[], null]);
  let changes = changeList(),
    crossDocument = null;
  const admitDocument = (href, replace = false) => {
    crossDocument = { __proto__: null, url: href, replace };
    ops.op_script_navigation_admitted(href);
  };
  // Driver-only metadata: entry identities never come from URL equality or guest getters.
  __G.takeHistoryNavigation = () => {
    const result = {
      __proto__: null,
      document: documentId,
      initial: initialEntry,
      changes,
      crossDocument,
      url: url(),
      current: entries[index].hostId,
    };
    changes = changeList();
    crossDocument = null;
    return apply(stringify, null, [result]);
  };
  __G.traverseHistoryEntry = (id) => {
    for (let at = 0; at < entries.length; at++) {
      if (entries[at].hostId === id) {
        traverseTo(at);
        return true;
      }
    }
    return false;
  };
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
  // `nav` carries a navigation.navigate() call's info, state and promises; every other
  // fragment navigation (Location, anchors) fires the navigate event itself.
  function navigateToFragment(href, replace, nav) {
    const kind = replace ? "replace" : "push";
    const pending = navigationHooks.begin(kind, href, nav || { __proto__: null });
    if (!pending) return;
    const oldURL = url();
    const from = activeEntry();
    const entry = stamp(
      { __proto__: null, url: href, serialized: null, scroll: activeEntry().scroll },
      replace ? from.key : null,
      pending.navState,
    );
    if (replace) entries[index] = entry;
    else appendEntry(entry);
    moveTo(href);
    stateObject = null;
    navigationHooks.committed(kind, from, pending);
    trusted(new PopState("popstate", { state: null }));
    navigationHooks.settle(pending, true);
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
    admitDocument(href, replace);
  }
  // The native cross-origin Location (native engine) navigates this document through its
  // own Location: same-document fragments, session history and the sandbox check
  // (turbo-surf#84).
  ops.op_set_host_capabilities({
    __proto__: null,
    navigateLocation: (href, replace, reload) =>
      reload ? admitDocument(href, true) : navigate(href, replace),
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
        admitDocument(url(), true);
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
    const stringList = (origins) => {
      const list = __G.makeNamedObject(
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
      apply(listSet, lists, [list, origins]);
      return list;
    };
    const ancestors = stringList(ops.op_location_ancestor_origins());
    // Static DOMStringList views for the other runtime interfaces (IDB objectStoreNames).
    define(__G, "domStringList", {
      __proto__: null,
      value: stringList,
      writable: true,
      configurable: true,
    });
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
    const kind = operation === "pushState" ? "push" : "replace";
    const pending = navigationHooks.begin(kind, target, {
      __proto__: null,
      uncancelable: true,
      sameDocument: true,
    });
    const from = activeEntry();
    const entry = stamp(
      { __proto__: null, url: target, serialized, scroll: from.scroll },
      kind === "replace" ? from.key : null,
    );
    if (kind === "push") appendEntry(entry);
    else entries[index] = entry;
    if (target !== url()) {
      moveTo(target);
    }
    stateObject = deserialize(serialized);
    navigationHooks.committed(kind, from, pending);
    navigationHooks.settle(pending, true);
  }
  // A same-document traversal: navigate, currententrychange and navigatesuccess, then
  // popstate and a queued hashchange, as Chrome 156 orders them.
  function traverseTo(target, nav) {
    task(() => {
      if (target < 0 || target >= entries.length || target === index) return;
      const entry = entries[target];
      const pending = navigationHooks.begin("traverse", entry.url, {
        __proto__: null,
        ...nav,
        index: target,
        entry,
      });
      if (!pending) return;
      const oldURL = url();
      const from = activeEntry();
      index = target;
      if (entry.url !== oldURL) moveTo(entry.url);
      stateObject = deserialize(entry.serialized);
      navigationHooks.committed("traverse", from, pending);
      navigationHooks.settle(pending, false);
      trusted(new PopState("popstate", { state: stateObject }));
      hashChange(oldURL, entry.url);
    });
  }
  const traverse = (delta) => traverseTo(index + delta);
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
    if (delta === 0) admitDocument(url(), true);
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

  // ---- Navigation API -----------------------------------------------------------------------
  // window.navigation over this document's session history (#108, #143, measured in
  // harness/detection/navigation-api-probe.js). Same-document navigations (fragments,
  // pushState, intercept()ed navigations, traversals) commit here; any other navigation
  // keeps the engine's contract and is admitted for the session driver.
  const HostPromise = Promise,
    HostEvent = Event,
    HostErrorEvent = ErrorEvent,
    HostAbort = AbortController;
  const microtask = (callback) => HostPromise.resolve().then(callback);
  const records = new WeakMap();
  const record = (receiver, kind) => {
    const value =
      receiver !== null && typeof receiver === "object" ? records.get(receiver) : undefined;
    if (!value || value.kind !== kind) throw new ErrorType("Illegal invocation");
    return value;
  };
  const fire = (target, event) => {
    ops.op_event_trust(event, -1);
    ops.op_dispatch_host_event(event, target);
    return event;
  };
  const getters = (constructor, kind, table) => {
    for (const [name, read] of table)
      accessor(constructor.prototype, name, function () {
        return read(record(this, kind));
      });
  };
  const cloneState = (value) => (value === undefined ? undefined : clone(value));
  const serializeState = (value, method, owner) =>
    value === undefined
      ? undefined
      : ops.op_surf_structured_clone(
          value,
          globalThis,
          undefined,
          `Failed to execute '${method}' on '${owner}': `,
        );
  const Navigation = __defineInterface("Navigation", 0, EventTarget);
  const HistoryEntry = __defineInterface("NavigationHistoryEntry", 0, EventTarget);
  const Destination = __defineInterface("NavigationDestination", 0, null);
  const Transition = __defineInterface("NavigationTransition", 0, null);
  const eventInterface = (name, kind, required) =>
    __defineInterface(name, 2, HostEvent, function (type, init = undefined) {
      if (!new.target)
        throw new ErrorType(
          `Failed to construct '${name}': Please use the 'new' operator, this DOM object constructor cannot be called as a function.`,
        );
      if (arguments.length < 2)
        throw new ErrorType(
          `Failed to construct '${name}': 2 arguments required, but only ${arguments.length} present.`,
        );
      const options = init || {};
      for (const member of required)
        if (options[member] === undefined)
          throw new ErrorType(
            `Failed to construct '${name}': Failed to read the '${member}' property from '${name}Init': Required member is undefined.`,
          );
      const event = Reflect.construct(HostEvent, [type, init], new.target);
      records.set(event, { __proto__: null, kind, init: options, handlers: [], active: false });
      return event;
    });
  const NavigateEvent = eventInterface("NavigateEvent", "navigate", ["destination"]);
  const ChangeEvent = eventInterface("NavigationCurrentEntryChangeEvent", "change", ["from"]);
  getters(NavigateEvent, "navigate", [
    ["navigationType", (state) => state.init.navigationType || "push"],
    ["destination", (state) => state.init.destination],
    ["canIntercept", (state) => !!state.init.canIntercept],
    ["userInitiated", (state) => !!state.init.userInitiated],
    ["hashChange", (state) => !!state.init.hashChange],
    ["signal", (state) => state.init.signal],
    ["formData", (state) => state.init.formData || null],
    ["downloadRequest", (state) => state.init.downloadRequest ?? null],
    ["info", (state) => state.init.info],
    ["hasUAVisualTransition", (state) => !!state.init.hasUAVisualTransition],
    ["sourceElement", (state) => state.init.sourceElement || null],
  ]);
  method(NavigateEvent.prototype, "intercept", 0, function (options = {}) {
    const state = record(this, "navigate");
    if (!state.active)
      throw new Exception(
        "Failed to execute 'intercept' on 'NavigateEvent': intercept() may only be called while the navigate event is being dispatched.",
        "InvalidStateError",
      );
    state.intercepted = true;
    if (options && typeof options.handler === "function") state.handlers.push(options.handler);
  });
  method(NavigateEvent.prototype, "scroll", 0, function () {
    record(this, "navigate");
  });
  getters(ChangeEvent, "change", [
    ["navigationType", (state) => state.init.navigationType ?? null],
    ["from", (state) => state.init.from],
  ]);
  const entryObject = (entry) => {
    if (!entry.object) {
      entry.object = Object.setPrototypeOf(new EventTarget(), HistoryEntry.prototype);
      records.set(entry.object, { __proto__: null, kind: "entry", entry });
    }
    return entry.object;
  };
  getters(HistoryEntry, "entry", [
    ["url", (state) => state.entry.url],
    ["key", (state) => state.entry.key],
    ["id", (state) => state.entry.id],
    ["index", (state) => entries.indexOf(state.entry)],
    ["sameDocument", () => true],
  ]);
  method(HistoryEntry.prototype, "getState", 0, function () {
    return cloneState(record(this, "entry").entry.navState);
  });
  __defineEventHandler(HistoryEntry.prototype, "ondispose", (receiver) =>
    record(receiver, "entry"),
  );
  getters(Destination, "destination", [
    ["url", (state) => state.url],
    ["key", (state) => state.key],
    ["id", (state) => state.id],
    ["index", (state) => state.index],
    ["sameDocument", (state) => state.sameDocument],
  ]);
  method(Destination.prototype, "getState", 0, function () {
    return cloneState(record(this, "destination").navState);
  });
  getters(Transition, "transition", [
    ["navigationType", (state) => state.navigationType],
    ["from", (state) => state.from],
    ["finished", (state) => state.finished],
  ]);
  const navigation = Object.setPrototypeOf(new EventTarget(), Navigation.prototype);
  records.set(navigation, { __proto__: null, kind: "navigation" });
  let transition = null;
  const deferred = () => {
    let resolve, reject;
    const promise = new HostPromise((ok, fail) => {
      resolve = ok;
      reject = fail;
    });
    promise.catch(() => {});
    return { promise, resolve, reject };
  };
  const results = () => ({ committed: deferred(), finished: deferred() });
  const exposed = (result) => ({
    committed: result.committed.promise,
    finished: result.finished.promise,
  });
  const failed = (result, error) => {
    result.committed.reject(error);
    result.finished.reject(error);
    return exposed(result);
  };
  const navigateError = (error, message) =>
    fire(navigation, new HostErrorEvent("navigateerror", { error, message }));
  const sameDocumentURL = (href) => withoutFragment(href) === withoutFragment(url());
  const hashChanged = (href) =>
    sameDocumentURL(href) && fragmentOf(href) !== null && fragmentOf(href) !== fragmentOf(url());
  navigationHooks = {
    __proto__: null,
    // Fire navigate; null when it was canceled (its promises reject with AbortError).
    begin(kind, href, nav) {
      const result = nav.result || results();
      const entry = nav.entry;
      const destination = Object.create(Destination.prototype);
      records.set(destination, {
        __proto__: null,
        kind: "destination",
        url: href,
        key: entry ? entry.key : "",
        id: entry ? entry.id : "",
        index: entry ? nav.index : -1,
        sameDocument:
          nav.sameDocument ||
          (kind === "traverse" || kind === "reload" ? !!entry : sameDocumentURL(href)),
        navState: entry ? entry.navState : nav.navState,
      });
      const controller = new HostAbort();
      const event = new NavigateEvent("navigate", {
        navigationType: kind,
        destination,
        canIntercept: true,
        cancelable: true,
        hashChange: kind !== "reload" && hashChanged(href),
        info: nav.info,
        signal: controller.signal,
      });
      const state = records.get(event);
      state.active = true;
      fire(navigation, event);
      state.active = false;
      const pending = {
        __proto__: null,
        result,
        handlers: state.handlers,
        intercepted: !!state.intercepted,
        navState: nav.navState,
      };
      if (event.defaultPrevented && !nav.uncancelable) {
        const error = new Exception("Navigation was aborted", "AbortError");
        controller.abort(error);
        failed(result, error);
        navigateError(error, "Uncaught ");
        return null;
      }
      return pending;
    },
    committed(kind, from, pending) {
      apply(arrayPush, changes, [
        { __proto__: null, kind, id: activeEntry().hostId, url: activeEntry().url },
      ]);
      fire(
        navigation,
        new ChangeEvent("currententrychange", { navigationType: kind, from: entryObject(from) }),
      );
      pending.result.committed.resolve(entryObject(activeEntry()));
      if (pending.handlers.length) {
        const state = {
          __proto__: null,
          kind: "transition",
          navigationType: kind,
          from: entryObject(from),
          finished: pending.result.finished.promise,
        };
        transition = Object.create(Transition.prototype);
        records.set(transition, state);
      }
    },
    settle(pending, later) {
      const success = () => {
        transition = null;
        fire(navigation, new HostEvent("navigatesuccess"));
        pending.result.finished.resolve(entryObject(activeEntry()));
      };
      if (!pending.handlers.length) {
        if (later) microtask(success);
        else success();
        return;
      }
      HostPromise.all(pending.handlers.map((handler) => microtask(() => handler()))).then(
        success,
        (error) => {
          transition = null;
          navigateError(error, "Uncaught " + StringValue(error));
          pending.result.finished.reject(error);
        },
      );
    },
  };
  // A navigate() or reload() that a handler intercept()ed commits as a same-document
  // navigation: the URL changes in place, as pushState/replaceState change it.
  const commitIntercepted = (kind, href, pending) => {
    const from = activeEntry();
    if (kind !== "reload") {
      const entry = stamp(
        { __proto__: null, url: href, serialized: null, scroll: from.scroll },
        kind === "replace" ? from.key : null,
        pending.navState,
      );
      if (kind === "push") appendEntry(entry);
      else entries[index] = entry;
      stateObject = null;
      if (href !== url()) {
        moveTo(href);
      }
    } else if (pending.navState !== undefined) from.navState = pending.navState;
    navigationHooks.committed(kind, from, pending);
    navigationHooks.settle(pending, true);
  };
  const checkNavigation = (receiver) => record(receiver, "navigation");
  const arity = (method, count) => {
    if (count < 1)
      throw new ErrorType(
        `Failed to execute '${method}' on 'Navigation': 1 argument required, but only 0 present.`,
      );
  };
  accessor(Navigation.prototype, "currentEntry", function () {
    checkNavigation(this);
    return entryObject(activeEntry());
  });
  accessor(Navigation.prototype, "transition", function () {
    checkNavigation(this);
    return transition;
  });
  accessor(Navigation.prototype, "canGoBack", function () {
    checkNavigation(this);
    return index > 0;
  });
  accessor(Navigation.prototype, "canGoForward", function () {
    checkNavigation(this);
    return index < entries.length - 1;
  });
  method(Navigation.prototype, "entries", 0, function () {
    checkNavigation(this);
    return entries.map(entryObject);
  });
  method(Navigation.prototype, "updateCurrentEntry", 1, function (options) {
    checkNavigation(this);
    arity("updateCurrentEntry", arguments.length);
    const state = serializeState((options || {}).state, "updateCurrentEntry", "Navigation");
    const entry = activeEntry();
    entry.navState = state;
    fire(
      navigation,
      new ChangeEvent("currententrychange", { navigationType: null, from: entryObject(entry) }),
    );
  });
  const behaviors = ["auto", "push", "replace"];
  method(Navigation.prototype, "navigate", 1, function (address, options = {}) {
    checkNavigation(this);
    arity("navigate", arguments.length);
    address = string(address);
    options = options || {};
    const behavior = options.history === undefined ? "auto" : string(options.history);
    if (!behaviors.includes(behavior))
      throw new ErrorType(
        `Failed to execute 'navigate' on 'Navigation': Failed to read the 'history' property from 'NavigationNavigateOptions': The provided value '${behavior}' is not a valid enum value of type NavigationHistoryBehavior.`,
      );
    const navState = serializeState(options.state, "navigate", "Navigation");
    const result = results();
    const parsed = parse(address, documentBase());
    if (!parsed) return failed(result, new Exception(`Invalid URL '${address}'.`, "SyntaxError"));
    const href = parsed.href;
    const replace = behavior === "replace" || (behavior === "auto" && href === url());
    const nav = { __proto__: null, info: options.info, navState, result };
    if (fragmentOf(href) !== null && sameDocumentURL(href)) {
      navigateToFragment(href, replace, nav);
      return exposed(result);
    }
    const kind = replace ? "replace" : "push";
    const pending = navigationHooks.begin(kind, href, nav);
    if (!pending) return exposed(result);
    if (pending.intercepted) commitIntercepted(kind, href, pending);
    else navigate(href, replace);
    return exposed(result);
  });
  method(Navigation.prototype, "reload", 0, function (options = {}) {
    checkNavigation(this);
    options = options || {};
    const navState =
      options.state === undefined
        ? activeEntry().navState
        : serializeState(options.state, "reload", "Navigation");
    const result = results();
    const pending = navigationHooks.begin("reload", url(), {
      __proto__: null,
      info: options.info,
      navState,
      result,
    });
    if (!pending) return exposed(result);
    if (pending.intercepted) commitIntercepted("reload", url(), pending);
    else admitDocument(url(), true);
    return exposed(result);
  });
  const traverseResult = (target, options, message) => {
    const result = results();
    if (target < 0 || target >= entries.length)
      return failed(result, new Exception(message, "InvalidStateError"));
    if (target === index) {
      result.committed.resolve(entryObject(activeEntry()));
      result.finished.resolve(entryObject(activeEntry()));
    } else traverseTo(target, { __proto__: null, info: (options || {}).info, result });
    return exposed(result);
  };
  method(Navigation.prototype, "traverseTo", 1, function (key, options = {}) {
    checkNavigation(this);
    arity("traverseTo", arguments.length);
    key = string(key);
    return traverseResult(
      entries.findIndex((entry) => entry.key === key),
      options,
      "Invalid key",
    );
  });
  method(Navigation.prototype, "back", 0, function (options = {}) {
    checkNavigation(this);
    return traverseResult(index - 1, options, "Cannot go back");
  });
  method(Navigation.prototype, "forward", 0, function (options = {}) {
    checkNavigation(this);
    return traverseResult(index + 1, options, "Cannot go forward");
  });
  for (const name of ["onnavigate", "onnavigatesuccess", "onnavigateerror", "oncurrententrychange"])
    __defineEventHandler(Navigation.prototype, name, checkNavigation);
  define(globalThis, "navigation", {
    __proto__: null,
    get: make(
      function () {
        return navigation;
      },
      "get navigation",
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
