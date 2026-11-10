/* eslint-disable eqeqeq, unicorn/consistent-function-scoping, no-shadow, no-underscore-dangle, prefer-const */
// IndexedDB as Chrome 156 ships it (#107, harness/detection/indexeddb-probe.js): an in-memory
// backing store per storage partition (op_idb_registry, the DOM storage key), shared by every
// realm of that partition. Requests run one per task in placement order, each event dispatch is
// followed by a microtask checkpoint, transactions auto-commit once inactive with no pending
// requests, and abort rolls back to a snapshot taken before the first write.
(() => {
  "use strict";
  const apply = Reflect.apply,
    make = __G.makeNativeFunction,
    define = Object.defineProperty,
    create = Object.create,
    keysOf = Object.keys,
    isArray = Array.isArray;
  const ErrorType = __exceptionTypeError,
    Exception = DOMException,
    HostEvent = __hostEvent,
    Target = EventTarget,
    DateType = Date,
    ArrayBufferType = ArrayBuffer,
    Bytes = Uint8Array,
    PromiseType = Promise,
    StringValue = String;
  const timeout = globalThis.setTimeout,
    clone = ops.op_surf_structured_clone,
    realm = globalThis,
    // turbo-dom#381: embedder-settable parent hook so the binding walks
    // request -> transaction -> database for us, running capture -> AT_TARGET ->
    // bubble on one event object with per-listener microtask checkpoints
    // (turbo-dom#382).
    setParent = __G.setEventTargetParent;
  const getTime = DateType.prototype.getTime,
    isView = ArrayBuffer.isView,
    byteLength = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, "byteLength").get,
    viewBuffer = Object.getOwnPropertyDescriptor(
      Object.getPrototypeOf(Uint8Array.prototype),
      "buffer",
    ).get,
    viewOffset = Object.getOwnPropertyDescriptor(
      Object.getPrototypeOf(Uint8Array.prototype),
      "byteOffset",
    ).get,
    viewLength = Object.getOwnPropertyDescriptor(
      Object.getPrototypeOf(Uint8Array.prototype),
      "byteLength",
    ).get,
    hasOwn = Object.hasOwn,
    sortStrings = Array.prototype.sort,
    prevented = Object.getOwnPropertyDescriptor(HostEvent.prototype, "defaultPrevented").get;
  const states = new WeakMap(),
    stateGet = WeakMap.prototype.get,
    stateSet = WeakMap.prototype.set;
  const record = (receiver, kind) => {
    const value =
      receiver !== null && (typeof receiver === "object" || typeof receiver === "function")
        ? apply(stateGet, states, [receiver])
        : undefined;
    if (!value || (kind && value.kind !== kind && value.kind2 !== kind))
      throw new ErrorType("Illegal invocation");
    return value;
  };
  const bind = (object, value) => {
    apply(stateSet, states, [object, value]);
    return object;
  };
  const fail = (iface, method, message, name) =>
    new Exception(`Failed to execute '${method}' on '${iface}': ${message}`, name);
  const typeFail = (iface, method, message) =>
    new ErrorType(`Failed to execute '${method}' on '${iface}': ${message}`);
  const requireArgs = (iface, method, count, given) => {
    if (given < count)
      throw typeFail(
        iface,
        method,
        `${count} argument${count === 1 ? "" : "s"} required, but only ${given} present.`,
      );
  };
  const string = (value) => {
    if (typeof value === "symbol") throw new ErrorType("Cannot convert a Symbol to a string");
    return StringValue(value);
  };
  const enforce = (iface, method, value, max, type) => {
    const number = +value;
    if (!(number === number) || number === Infinity || number === -Infinity)
      throw typeFail(iface, method, `Value is outside the '${type}' value range.`);
    const truncated = number < 0 ? -Math.floor(-number) : Math.floor(number);
    if (truncated < 0 || truncated > max)
      throw typeFail(iface, method, `Value is outside the '${type}' value range.`);
    return truncated;
  };
  // The engine's request results arrive after timers already queued by the placing task.
  const task = (callback) => timeout(callback, 0);
  const lateTask = (callback) => timeout(() => timeout(callback, 0), 0);

  // ---- keys: number < date < string < binary < array, held as {t, v} -------------------
  const NUMBER = 0,
    DATE = 1,
    STRING = 2,
    BINARY = 3,
    ARRAY = 4;
  const dateValue = (value) => {
    try {
      return apply(getTime, value, []);
    } catch {
      return undefined;
    }
  };
  const bufferBytes = (value) => {
    try {
      if (isView(value)) {
        const buffer = apply(viewBuffer, value, []);
        return new Bytes(buffer, apply(viewOffset, value, []), apply(viewLength, value, []));
      }
      apply(byteLength, value, []);
      return new Bytes(value);
    } catch {
      return undefined;
    }
  };
  const toKey = (value, seen) => {
    if (typeof value === "number") return value === value ? { t: NUMBER, v: value } : null;
    if (typeof value === "string") return { t: STRING, v: value };
    if (value === null || typeof value !== "object") return null;
    const time = dateValue(value);
    if (time !== undefined) return time === time ? { t: DATE, v: time } : null;
    if (isArray(value)) {
      seen = seen || [];
      for (let at = 0; at < seen.length; at++) if (seen[at] === value) return null;
      seen[seen.length] = value;
      const length = value.length,
        items = [];
      for (let at = 0; at < length; at++) {
        if (!hasOwn(value, at)) return null;
        const item = toKey(value[at], seen);
        if (!item) return null;
        items[at] = item;
      }
      seen.length--;
      return { t: ARRAY, v: items };
    }
    const bytes = bufferBytes(value);
    if (bytes) {
      const copy = [];
      for (let at = 0; at < bytes.length; at++) copy[at] = bytes[at];
      return { t: BINARY, v: copy };
    }
    return null;
  };
  const compare = (left, right) => {
    if (left.t !== right.t) return left.t < right.t ? -1 : 1;
    if (left.t === ARRAY || left.t === BINARY) {
      const a = left.v,
        b = right.v,
        length = a.length < b.length ? a.length : b.length;
      for (let at = 0; at < length; at++) {
        const order = left.t === ARRAY ? compare(a[at], b[at]) : a[at] - b[at];
        if (order) return order < 0 ? -1 : 1;
      }
      return a.length === b.length ? 0 : a.length < b.length ? -1 : 1;
    }
    return left.v === right.v ? 0 : left.v < right.v ? -1 : 1;
  };
  const fromKey = (key) => {
    switch (key.t) {
      case DATE:
        return new DateType(key.v);
      case BINARY: {
        const buffer = new ArrayBufferType(key.v.length),
          view = new Bytes(buffer);
        for (let at = 0; at < key.v.length; at++) view[at] = key.v[at];
        return buffer;
      }
      case ARRAY: {
        const items = [];
        for (let at = 0; at < key.v.length; at++) items[at] = fromKey(key.v[at]);
        return items;
      }
      default:
        return key.v;
    }
  };
  const validKey = (iface, method, value) => {
    const key = toKey(value);
    if (!key) throw fail(iface, method, "The parameter is not a valid key.", "DataError");
    return key;
  };

  // ---- key paths ---------------------------------------------------------------------------
  const identifier = /^[$A-Z_a-zª-￿][$0-9A-Z_a-zª-￿]*$/;
  const validPathString = (path) => {
    if (path === "") return true;
    const parts = path.split(".");
    for (let at = 0; at < parts.length; at++) if (!identifier.test(parts[at])) return false;
    return true;
  };
  // undefined: no key path; false: invalid.
  const keyPathOf = (value) => {
    if (value === undefined || value === null) return null;
    if (isArray(value)) {
      const parts = [];
      for (let at = 0; at < value.length; at++) {
        parts[at] = string(value[at]);
        if (!validPathString(parts[at])) return false;
      }
      return parts;
    }
    const path = string(value);
    return validPathString(path) ? path : false;
  };
  const copyPath = (path) => (isArray(path) ? path.slice() : path);
  const NONE = {};
  const evaluateString = (value, path) => {
    if (path === "") return value;
    const parts = path.split(".");
    for (let at = 0; at < parts.length; at++) {
      const part = parts[at];
      if (typeof value === "string" && part === "length") value = value.length;
      else if (value === null || typeof value !== "object") return NONE;
      else if (isArray(value) && part === "length") value = value.length;
      else {
        if (!hasOwn(value, part)) return NONE;
        value = value[part];
      }
    }
    return value;
  };
  // The key a value yields under a key path: NONE (no value), null (invalid) or a key.
  const extractKey = (value, path, multiEntry) => {
    if (isArray(path)) {
      const items = [];
      for (let at = 0; at < path.length; at++) {
        const item = evaluateString(value, path[at]);
        if (item === NONE) return NONE;
        const key = toKey(item);
        if (!key) return null;
        items[at] = key;
      }
      return { t: ARRAY, v: items };
    }
    const found = evaluateString(value, path);
    if (found === NONE) return NONE;
    if (multiEntry && isArray(found)) {
      const keys = [];
      for (let at = 0; at < found.length; at++) {
        const key = toKey(found[at]);
        if (!key) continue;
        let duplicate = false;
        for (let other = 0; other < keys.length; other++)
          if (!compare(keys[other], key)) duplicate = true;
        if (!duplicate) keys[keys.length] = key;
      }
      return { t: -1, v: keys };
    }
    return toKey(found);
  };
  const canInject = (value, path) => {
    const parts = path.split(".");
    for (let at = 0; at < parts.length - 1; at++) {
      if (value === null || typeof value !== "object") return false;
      if (!hasOwn(value, parts[at])) return true;
      value = value[parts[at]];
    }
    return value !== null && typeof value === "object";
  };
  const inject = (value, path, key) => {
    const parts = path.split(".");
    for (let at = 0; at < parts.length - 1; at++) {
      if (!hasOwn(value, parts[at]))
        define(value, parts[at], {
          __proto__: null,
          value: {},
          writable: true,
          enumerable: true,
          configurable: true,
        });
      value = value[parts[at]];
    }
    define(value, parts[parts.length - 1], {
      __proto__: null,
      value: key,
      writable: true,
      enumerable: true,
      configurable: true,
    });
  };

  // ---- sorted record arrays ------------------------------------------------------------------
  // The first position whose entry is not "before" the probe (binary search on order(entry)).
  const seek = (rows, before) => {
    let low = 0,
      high = rows.length;
    while (low < high) {
      const middle = (low + high) >> 1;
      if (before(rows[middle])) low = middle + 1;
      else high = middle;
    }
    return low;
  };
  const entryOrder = (left, right) => compare(left.k, right.k) || compare(left.p, right.p);
  const findRecord = (store, key) => {
    const at = seek(store.records, (row) => compare(row.k, key) < 0);
    return at < store.records.length && !compare(store.records[at].k, key) ? at : -1;
  };
  const indexKeys = (index, value) => {
    const key = extractKey(value, index.keyPath, index.multiEntry);
    if (key === NONE || key === null) return [];
    return key.t === -1 ? key.v : [key];
  };
  const removeRecordAt = (store, at) => {
    const row = store.records[at];
    store.records.splice(at, 1);
    for (const name of keysOf(store.indexes)) {
      const index = store.indexes[name];
      index.entries = index.entries.filter((entry) => compare(entry.p, row.k));
    }
  };
  const insertEntry = (index, entry) => {
    const at = seek(index.entries, (row) => entryOrder(row, entry) < 0);
    index.entries.splice(at, 0, entry);
  };
  const uniqueClash = (index, keys, primary) => {
    if (!index.unique) return false;
    for (let at = 0; at < keys.length; at++) {
      const found = seek(index.entries, (row) => compare(row.k, keys[at]) < 0);
      const row = index.entries[found];
      if (row && !compare(row.k, keys[at]) && compare(row.p, primary)) return true;
    }
    return false;
  };

  // ---- ranges ------------------------------------------------------------------------------
  const inRange = (range, key) => {
    if (!range) return true;
    if (range.lower) {
      const order = compare(key, range.lower);
      if (order < 0 || (order === 0 && range.lowerOpen)) return false;
    }
    if (range.upper) {
      const order = compare(key, range.upper);
      if (order > 0 || (order === 0 && range.upperOpen)) return false;
    }
    return true;
  };
  const rangeOf = (iface, method, query, nullable) => {
    if (nullable && (query === undefined || query === null)) return null;
    const existing =
      query !== null && typeof query === "object" ? apply(stateGet, states, [query]) : undefined;
    if (existing && existing.kind === "range") return existing;
    const key = validKey(iface, method, query);
    return { kind: "range", lower: key, upper: key, lowerOpen: false, upperOpen: false };
  };
  const recordsInRange = (store, range) => {
    let at =
      range && range.lower ? seek(store.records, (row) => compare(row.k, range.lower) < 0) : 0;
    const rows = [];
    for (; at < store.records.length; at++) {
      const row = store.records[at];
      if (range && range.upper && compare(row.k, range.upper) > 0) break;
      if (inRange(range, row.k)) rows[rows.length] = row;
    }
    return rows;
  };
  const entriesInRange = (index, range) => {
    const rows = [];
    for (let at = 0; at < index.entries.length; at++)
      if (inRange(range, index.entries[at].k)) rows[rows.length] = index.entries[at];
    return rows;
  };

  // ---- events --------------------------------------------------------------------------------
  const makeEvent = (Ctor, type, init) => {
    const event = new Ctor(type, init);
    ops.op_event_trust(event, -1);
    return event;
  };
  // One event travels request -> transaction -> database. The parent chain is set on each
  // target via `setParent` (turbo-dom#381), so the binding walks it: capture on the
  // ancestors first (phase 1), AT_TARGET at `target` (phase 2, listeners in registration
  // order across capture + bubble), then bubble back up (phase 3), running a microtask
  // checkpoint after every listener (turbo-dom#382). Returns [defaultPrevented, whether a
  // listener threw]: the binding reports each listener's exception and flags it
  // (turbo-dom#301); listeners of events dispatched from inside the listeners do not count.
  const fire = (target, type, options) => {
    const Ctor = options.Ctor || HostEvent,
      init = { bubbles: !!options.bubbles, cancelable: !!options.cancelable, ...options.init };
    const event = makeEvent(Ctor, type, init);
    const exception = ops.op_idb_dispatch(event, target);
    return [apply(prevented, event, []), exception];
  };

  // ---- interfaces ----------------------------------------------------------------------------
  const illegal = (name) =>
    function () {
      throw new ErrorType(`Failed to construct '${name}': Illegal constructor`);
    };
  const iface = (name, parent, construct, length) => {
    const constructor = make(construct || illegal(name), name, length || 0, true, !!construct);
    const prototype = constructor.prototype;
    if (parent) {
      Object.setPrototypeOf(prototype, parent.prototype);
      Object.setPrototypeOf(constructor, parent);
    }
    define(prototype, Symbol.toStringTag, { __proto__: null, value: name, configurable: true });
    define(constructor, "prototype", { __proto__: null, writable: false });
    define(realm, name, {
      __proto__: null,
      value: constructor,
      writable: true,
      configurable: true,
      enumerable: false,
    });
    return constructor;
  };
  const getter = (prototype, kind, name, read, write) =>
    define(prototype, name, {
      __proto__: null,
      get: make(
        function () {
          return read(record(this, kind), this);
        },
        "get " + name,
        0,
      ),
      set: write
        ? make(
            function (value) {
              write(record(this, kind), value);
            },
            "set " + name,
            1,
          )
        : undefined,
      enumerable: true,
      configurable: true,
    });
  const method = (prototype, kind, name, length, body) =>
    define(prototype, name, {
      __proto__: null,
      value: make(
        {
          [name](...args) {
            return apply(body, this, [record(this, kind), args]);
          },
        }[name],
        name,
        length,
      ),
      writable: true,
      enumerable: true,
      configurable: true,
    });
  const handlers = (prototype, kind, names) => {
    for (const name of names)
      __defineEventHandler(prototype, name, (target) => record(target, kind));
  };
  const newTarget = (prototype, state) => {
    const target = new Target();
    Object.setPrototypeOf(target, prototype);
    return bind(target, state);
  };
  const stringList = (names) => {
    const values = apply(sortStrings, names.slice(), []);
    return __G.domStringList ? __G.domStringList(values) : values;
  };

  const IDBFactory = iface("IDBFactory");
  const IDBDatabase = iface("IDBDatabase", Target);
  const IDBTransaction = iface("IDBTransaction", Target);
  const IDBRequest = iface("IDBRequest", Target);
  const IDBOpenDBRequest = iface("IDBOpenDBRequest", IDBRequest);
  const IDBObjectStore = iface("IDBObjectStore");
  const IDBIndex = iface("IDBIndex");
  const IDBCursor = iface("IDBCursor");
  const IDBCursorWithValue = iface("IDBCursorWithValue", IDBCursor);
  const IDBKeyRange = iface("IDBKeyRange");
  const IDBRecord = iface("IDBRecord");

  // IDBVersionChangeEvent(type, {oldVersion, newVersion}) — a constructible Event.
  const versionStates = new WeakMap();
  class VersionChangeEvent extends HostEvent {
    constructor(type, init) {
      if (!arguments.length)
        throw typeFail(
          "IDBVersionChangeEvent",
          "construct",
          "1 argument required, but only 0 present.",
        );
      super(type, init);
      const options = init == null ? {} : init;
      versionStates.set(this, {
        oldVersion: options.oldVersion === undefined ? 0 : +options.oldVersion,
        newVersion:
          options.newVersion === undefined || options.newVersion === null
            ? null
            : +options.newVersion,
        dataLoss: options.dataLoss === undefined ? "none" : string(options.dataLoss),
      });
    }
  }
  const IDBVersionChangeEvent = make(VersionChangeEvent, "IDBVersionChangeEvent", 1, true, true);
  IDBVersionChangeEvent.prototype = VersionChangeEvent.prototype;
  Object.setPrototypeOf(IDBVersionChangeEvent, HostEvent);
  define(VersionChangeEvent.prototype, "constructor", {
    __proto__: null,
    value: IDBVersionChangeEvent,
    writable: true,
    configurable: true,
  });
  define(VersionChangeEvent.prototype, Symbol.toStringTag, {
    __proto__: null,
    value: "IDBVersionChangeEvent",
    configurable: true,
  });
  define(IDBVersionChangeEvent, "prototype", { __proto__: null, writable: false });
  for (const name of ["oldVersion", "newVersion", "dataLoss"])
    define(VersionChangeEvent.prototype, name, {
      __proto__: null,
      get: make(
        function () {
          const value = versionStates.get(this);
          if (!value) throw new ErrorType("Illegal invocation");
          return value[name];
        },
        "get " + name,
        0,
      ),
      enumerable: true,
      configurable: true,
    });
  define(realm, "IDBVersionChangeEvent", {
    __proto__: null,
    value: IDBVersionChangeEvent,
    writable: true,
    configurable: true,
    enumerable: false,
  });
  const versionEvent = (oldVersion, newVersion) => ({
    Ctor: IDBVersionChangeEvent,
    init: { oldVersion, newVersion },
  });

  // ---- requests ------------------------------------------------------------------------------
  const unfinished = (property) =>
    new Exception(
      `Failed to read the '${property}' property from 'IDBRequest': The request has not finished.`,
      "InvalidStateError",
    );
  getter(IDBRequest.prototype, "request", "result", (state) => {
    if (state.readyState !== "done") throw unfinished("result");
    return state.result;
  });
  getter(IDBRequest.prototype, "request", "error", (state) => {
    if (state.readyState !== "done") throw unfinished("error");
    return state.error;
  });
  getter(IDBRequest.prototype, "request", "source", (state) => state.source);
  getter(IDBRequest.prototype, "request", "transaction", (state) =>
    state.transaction ? state.transaction.public : null,
  );
  getter(IDBRequest.prototype, "request", "readyState", (state) => state.readyState);
  handlers(IDBRequest.prototype, "request", ["onsuccess", "onerror"]);
  handlers(IDBOpenDBRequest.prototype, "open", ["onblocked", "onupgradeneeded"]);
  const newRequest = (prototype, source, transaction, open) => {
    const state = {
      kind: "request",
      kind2: open ? "open" : undefined,
      readyState: "pending",
      result: undefined,
      error: null,
      source,
      transaction,
      public: null,
    };
    state.public = newTarget(prototype, state);
    // Chain for the engine-walked event path (turbo-dom#381): request -> transaction.
    // Resolved lazily so an open() request whose transaction is still null (no tx yet,
    // the `blocked` event) terminates the walk at the request.
    setParent(state.public, () => (state.transaction ? state.transaction.public : null));
    return state;
  };

  // ---- factory -------------------------------------------------------------------------------
  const registryOf = (factory) => {
    if (!factory.registry) {
      let registry;
      try {
        registry = ops.op_idb_registry(realm, factory.unpartitioned);
      } catch {
        throw new Exception("The user denied permission to access the database.", "SecurityError");
      }
      if (!registry.dbs) {
        registry.dbs = create(null);
        registry.queues = create(null);
      }
      factory.registry = registry;
    }
    return factory.registry;
  };
  define(__G, "idbFactory", {
    __proto__: null,
    value: (unpartitioned) => {
      const state = { kind: "factory", unpartitioned, registry: null };
      return bind(create(IDBFactory.prototype), state);
    },
    writable: true,
    configurable: true,
  });
  const queueOperation = (registry, name, operation) => {
    const queue = registry.queues[name] || (registry.queues[name] = []);
    queue[queue.length] = operation;
    if (queue.length === 1) lateTask(() => operation.run());
  };
  const nextOperation = (registry, name) => {
    const queue = registry.queues[name];
    queue.shift();
    if (queue.length) task(() => queue[0].run());
  };
  // Fires versionchange at the other connections, then blocked while any stays open, and runs
  // `proceed` once every one has closed.
  const awaitClosed = (db, except, request, newVersion, proceed) => {
    const open = () =>
      db.connections.filter((connection) => connection !== except && !connection.closed);
    const others = open();
    for (const connection of others)
      if (!connection.closePending)
        connection.fire("versionchange", versionEvent(db.version, newVersion));
    if (open().length) fire(request.public, "blocked", versionEvent(db.version, newVersion));
    if (open().length)
      db.waiters[db.waiters.length] = () => {
        if (open().length) return false;
        task(proceed);
        return true;
      };
    else proceed();
  };
  const finishOpen = (request, kind, value) => {
    request.readyState = "done";
    if (kind === "error") {
      request.result = undefined;
      request.error = value;
      fire(request.public, "error", { bubbles: true, cancelable: true });
    } else {
      request.result = value;
      fire(request.public, "success", {});
    }
  };
  const openDatabase = (factory, request, name, version) => {
    const registry = registryOf(factory);
    const run = () => {
      let db = registry.dbs[name];
      const created = !db;
      if (!db)
        db = registry.dbs[name] = {
          name,
          version: 0,
          stores: create(null),
          connections: [],
          transactions: [],
          waiters: [],
        };
      if (version === undefined) version = db.version || 1;
      const done = () => nextOperation(registry, name);
      if (version < db.version) {
        finishOpen(
          request,
          "error",
          new Exception(
            `The requested version (${version}) is less than the existing version (${db.version}).`,
            "VersionError",
          ),
        );
        return done();
      }
      const connection = newConnection(db);
      if (version === db.version) {
        finishOpen(request, "success", connection.public);
        return done();
      }
      awaitClosed(db, connection, request, version, () => {
        const oldVersion = db.version;
        const snapshot = snapshotDatabase(db);
        db.version = version;
        connection.version = version;
        const transaction = newTransaction(connection, keysOf(db.stores), "versionchange");
        transaction.upgrade = { snapshot, created, request };
        request.transaction = transaction;
        request.result = connection.public;
        request.readyState = "done";
        transaction.onFinish = (aborted) => {
          if (aborted) {
            if (created) delete registry.dbs[name];
            connection.closed = true;
            connection.version = oldVersion || 0;
            request.transaction = null;
            task(() => {
              finishOpen(
                request,
                "error",
                new Exception(
                  "Version change transaction was aborted in upgradeneeded event handler.",
                  "AbortError",
                ),
              );
              done();
            });
          } else {
            request.transaction = null;
            task(() => {
              finishOpen(
                request,
                connection.closePending ? "error" : "success",
                connection.closePending
                  ? new Exception("The connection was closed.", "AbortError")
                  : connection.public,
              );
              done();
            });
          }
        };
        transaction.state = "active";
        const [, exception] = fire(
          request.public,
          "upgradeneeded",
          versionEvent(oldVersion, version),
        );
        if (transaction.state === "active") transaction.state = "inactive";
        if (exception && transaction.state !== "finished")
          abortTransaction(
            transaction,
            new Exception("Uncaught exception in event handler.", "AbortError"),
          );
        pump(transaction);
      });
    };
    queueOperation(registry, name, { run });
  };
  const deleteDatabase = (factory, request, name) => {
    const registry = registryOf(factory);
    queueOperation(registry, name, {
      run() {
        const db = registry.dbs[name];
        const finish = () => {
          const oldVersion = db ? db.version : 0;
          if (db) delete registry.dbs[name];
          request.readyState = "done";
          request.result = undefined;
          fire(request.public, "success", versionEvent(oldVersion, null));
          nextOperation(registry, name);
        };
        if (!db) return finish();
        awaitClosed(db, null, request, null, finish);
      },
    });
  };
  const factoryVersion = (value) => {
    const number = +value;
    if (
      !(number === number) ||
      number === Infinity ||
      number === -Infinity ||
      number < 0 ||
      Math.floor(number) > 9007199254740991
    )
      throw typeFail(
        "IDBFactory",
        "open",
        "Value is outside the 'unsigned long long' value range.",
      );
    const version = Math.floor(number);
    if (version === 0) throw typeFail("IDBFactory", "open", "The version provided must not be 0.");
    return version;
  };
  method(IDBFactory.prototype, "factory", "open", 1, function (factory, args) {
    requireArgs("IDBFactory", "open", 1, args.length);
    const name = string(args[0]);
    const version = args[1] === undefined ? undefined : factoryVersion(args[1]);
    registryOf(factory);
    const request = newRequest(IDBOpenDBRequest.prototype, null, null, true);
    openDatabase(factory, request, name, version);
    return request.public;
  });
  method(IDBFactory.prototype, "factory", "deleteDatabase", 1, function (factory, args) {
    requireArgs("IDBFactory", "deleteDatabase", 1, args.length);
    const name = string(args[0]);
    registryOf(factory);
    const request = newRequest(IDBOpenDBRequest.prototype, null, null, true);
    deleteDatabase(factory, request, name);
    return request.public;
  });
  method(IDBFactory.prototype, "factory", "databases", 0, function (factory) {
    let registry;
    try {
      registry = registryOf(factory);
    } catch (error) {
      return PromiseType.reject(error);
    }
    return new PromiseType((resolve) =>
      lateTask(() => {
        const list = [];
        for (const name of keysOf(registry.dbs)) {
          const db = registry.dbs[name];
          if (db.version > 0) list[list.length] = { name: db.name, version: db.version };
        }
        resolve(list);
      }),
    );
  });
  method(IDBFactory.prototype, "factory", "cmp", 2, function (_factory, args) {
    requireArgs("IDBFactory", "cmp", 2, args.length);
    return compare(validKey("IDBFactory", "cmp", args[0]), validKey("IDBFactory", "cmp", args[1]));
  });

  // ---- database / connection -----------------------------------------------------------------
  const snapshotStore = (store) => {
    const indexes = create(null);
    for (const name of keysOf(store.indexes)) {
      const index = store.indexes[name];
      indexes[name] = { index, name: index.name, entries: index.entries.slice() };
    }
    return {
      store,
      name: store.name,
      records: store.records.slice(),
      current: store.current,
      indexes,
    };
  };
  const restoreStore = (snapshot) => {
    const store = snapshot.store;
    store.name = snapshot.name;
    store.records = snapshot.records;
    store.current = snapshot.current;
    store.indexes = create(null);
    for (const name of keysOf(snapshot.indexes)) {
      const saved = snapshot.indexes[name];
      saved.index.name = saved.name;
      saved.index.entries = saved.entries;
      saved.index.deleted = false;
      store.indexes[name] = saved.index;
    }
    store.deleted = false;
  };
  const snapshotDatabase = (db) => {
    const stores = create(null);
    for (const name of keysOf(db.stores)) stores[name] = snapshotStore(db.stores[name]);
    return { version: db.version, stores, all: db.allStores ? db.allStores.slice() : [] };
  };
  const restoreDatabase = (db, snapshot) => {
    db.version = snapshot.version;
    for (const name of keysOf(db.stores)) db.stores[name].deleted = true;
    db.stores = create(null);
    for (const name of keysOf(snapshot.stores)) {
      restoreStore(snapshot.stores[name]);
      db.stores[name] = snapshot.stores[name].store;
    }
  };
  const newConnection = (db) => {
    const connection = {
      kind: "database",
      db,
      version: db.version,
      closePending: false,
      closed: false,
      public: null,
    };
    connection.public = newTarget(IDBDatabase.prototype, connection);
    connection.fire = (type, options) => fire(connection.public, type, options);
    db.connections[db.connections.length] = connection;
    return connection;
  };
  const closeConnection = (connection) => {
    connection.closePending = true;
    const finish = () => {
      if (connection.db.transactions.some((transaction) => transaction.connection === connection))
        return;
      connection.closed = true;
      const db = connection.db;
      db.connections = db.connections.filter((other) => other !== connection);
      const waiters = db.waiters;
      db.waiters = [];
      for (const waiter of waiters) if (!waiter()) db.waiters[db.waiters.length] = waiter;
    };
    connection.onIdle = finish;
    finish();
  };
  const DB = IDBDatabase.prototype;
  getter(DB, "database", "name", (state) => state.db.name);
  getter(DB, "database", "version", (state) => state.version);
  getter(DB, "database", "objectStoreNames", (state) => stringList(keysOf(state.db.stores)));
  handlers(DB, "database", ["onabort", "onclose", "onerror", "onversionchange"]);
  method(DB, "database", "close", 0, function (state) {
    closeConnection(state);
  });
  const upgradeOf = (state, method) => {
    const transaction = state.db.transactions.find(
      (candidate) => candidate.connection === state && candidate.mode === "versionchange",
    );
    if (!transaction)
      throw fail(
        "IDBDatabase",
        method,
        "The database is not running a version change transaction.",
        "InvalidStateError",
      );
    if (transaction.state !== "active")
      throw fail(
        "IDBDatabase",
        method,
        "The transaction is not active.",
        "TransactionInactiveError",
      );
    return transaction;
  };
  method(DB, "database", "createObjectStore", 1, function (state, args) {
    requireArgs("IDBDatabase", "createObjectStore", 1, args.length);
    const name = string(args[0]);
    const options = args[1] == null ? {} : args[1];
    const transaction = upgradeOf(state, "createObjectStore");
    const keyPath = keyPathOf(options.keyPath);
    const autoIncrement = !!options.autoIncrement;
    if (keyPath === false)
      throw fail(
        "IDBDatabase",
        "createObjectStore",
        "The keyPath option is not a valid key path.",
        "SyntaxError",
      );
    if (state.db.stores[name])
      throw fail(
        "IDBDatabase",
        "createObjectStore",
        "An object store with the specified name already exists.",
        "ConstraintError",
      );
    if (autoIncrement && (keyPath === "" || isArray(keyPath)))
      throw fail(
        "IDBDatabase",
        "createObjectStore",
        "The autoIncrement option was set but the keyPath option was empty or an array.",
        "InvalidAccessError",
      );
    const store = {
      name,
      keyPath,
      autoIncrement,
      current: 1,
      records: [],
      indexes: create(null),
      deleted: false,
    };
    state.db.stores[name] = store;
    transaction.scope[transaction.scope.length] = name;
    return storeHandle(transaction, store);
  });
  method(DB, "database", "deleteObjectStore", 1, function (state, args) {
    requireArgs("IDBDatabase", "deleteObjectStore", 1, args.length);
    const name = string(args[0]);
    const transaction = upgradeOf(state, "deleteObjectStore");
    const store = state.db.stores[name];
    if (!store)
      throw fail(
        "IDBDatabase",
        "deleteObjectStore",
        "The specified object store was not found.",
        "NotFoundError",
      );
    store.deleted = true;
    delete state.db.stores[name];
    transaction.scope = transaction.scope.filter((entry) => entry !== name);
  });
  method(DB, "database", "transaction", 1, function (state, args) {
    requireArgs("IDBDatabase", "transaction", 1, args.length);
    let names =
      typeof args[0] === "object" &&
      args[0] !== null &&
      typeof args[0][Symbol.iterator] === "function"
        ? Array.from(args[0], string)
        : [string(args[0])];
    const mode = args[1] === undefined ? "readonly" : string(args[1]);
    if (mode !== "readonly" && mode !== "readwrite" && mode !== "versionchange")
      throw typeFail(
        "IDBDatabase",
        "transaction",
        `The provided value '${mode}' is not a valid enum value of type IDBTransactionMode.`,
      );
    if (state.closePending)
      throw fail(
        "IDBDatabase",
        "transaction",
        "The database connection is closing.",
        "InvalidStateError",
      );
    if (
      state.db.transactions.some(
        (candidate) => candidate.mode === "versionchange" && candidate.connection === state,
      )
    )
      throw fail(
        "IDBDatabase",
        "transaction",
        "A version change transaction is running.",
        "InvalidStateError",
      );
    for (const name of names)
      if (!state.db.stores[name])
        throw fail(
          "IDBDatabase",
          "transaction",
          "One of the specified object stores was not found.",
          "NotFoundError",
        );
    if (!names.length)
      throw fail(
        "IDBDatabase",
        "transaction",
        "The storeNames parameter was empty.",
        "InvalidAccessError",
      );
    if (mode === "versionchange")
      throw typeFail(
        "IDBDatabase",
        "transaction",
        "The mode provided ('versionchange') is not one of 'readonly' or 'readwrite'.",
      );
    names = names.filter((name, at) => names.indexOf(name) === at);
    const transaction = newTransaction(state, names, mode);
    task(() => {
      if (transaction.state === "active") transaction.state = "inactive";
      pump(transaction);
    });
    return transaction.public;
  });

  // ---- transactions --------------------------------------------------------------------------
  const newTransaction = (connection, scope, mode) => {
    const transaction = {
      kind: "transaction",
      connection,
      db: connection.db,
      scope: scope.slice(),
      mode,
      state: "active",
      started: false,
      scheduled: false,
      requests: [],
      error: null,
      handles: create(null),
      snapshots: [],
      upgrade: null,
      onFinish: null,
      public: null,
    };
    transaction.public = newTarget(IDBTransaction.prototype, transaction);
    // Chain for the engine-walked event path (turbo-dom#381): transaction -> database.
    setParent(transaction.public, () => transaction.connection.public);
    connection.db.transactions[connection.db.transactions.length] = transaction;
    return transaction;
  };
  const overlaps = (left, right) => left.scope.some((name) => right.scope.indexOf(name) >= 0);
  const canStart = (transaction) => {
    for (const other of transaction.db.transactions) {
      if (other === transaction) return true;
      if (other.state === "finished") continue;
      if (
        (other.mode !== "readonly" || transaction.mode !== "readonly") &&
        overlaps(other, transaction)
      )
        return false;
      if (other.mode === "versionchange") return false;
    }
    return true;
  };
  const finishTransaction = (transaction, aborted) => {
    transaction.state = "finished";
    const db = transaction.db;
    db.transactions = db.transactions.filter((other) => other !== transaction);
    if (transaction.onFinish) transaction.onFinish(aborted);
    if (transaction.connection.onIdle) transaction.connection.onIdle();
    for (const other of db.transactions.slice()) pump(other);
  };
  const commit = (transaction) => {
    transaction.state = "committing";
    task(() => {
      if (transaction.state !== "committing") return;
      transaction.state = "finished";
      fire(transaction.public, "complete", {});
      finishTransaction(transaction, false);
    });
  };
  const pump = (transaction) => {
    if (transaction.scheduled || transaction.state === "finished") return;
    if (!transaction.started) {
      if (!canStart(transaction)) return;
      transaction.started = true;
    }
    if (transaction.requests.length) {
      transaction.scheduled = true;
      const run = () => {
        transaction.scheduled = false;
        if (transaction.state === "finished" || !transaction.requests.length) return;
        processRequest(transaction, transaction.requests.shift());
      };
      if (transaction.ranOnce) task(run);
      else {
        transaction.ranOnce = true;
        lateTask(run);
      }
      return;
    }
    if (transaction.state === "inactive" || transaction.state === "committing") commit(transaction);
  };
  const processRequest = (transaction, entry) => {
    const request = entry.request;
    const committing = transaction.state === "committing";
    let outcome;
    try {
      outcome = ["success", entry.operation()];
    } catch (error) {
      outcome = ["error", error];
    }
    request.readyState = "done";
    if (outcome[0] === "success") {
      request.result = outcome[1];
      request.error = null;
      if (!committing) transaction.state = "active";
      const [, exception] = fire(request.public, "success", {});
      if (!committing && transaction.state === "active") transaction.state = "inactive";
      if (exception && transaction.state !== "finished")
        abortTransaction(
          transaction,
          new Exception("Uncaught exception in event handler.", "AbortError"),
        );
    } else {
      request.result = undefined;
      request.error = outcome[1];
      if (!committing) transaction.state = "active";
      const [cancelled, exception] = fire(request.public, "error", {
        bubbles: true,
        cancelable: true,
      });
      if (!committing && transaction.state === "active") transaction.state = "inactive";
      if (transaction.state !== "finished") {
        if (exception)
          abortTransaction(
            transaction,
            new Exception("Uncaught exception in event handler.", "AbortError"),
          );
        else if (!cancelled) abortTransaction(transaction, outcome[1]);
      }
    }
    pump(transaction);
  };
  const abortTransaction = (transaction, error) => {
    transaction.state = "finished";
    transaction.error = error;
    for (let at = transaction.snapshots.length - 1; at >= 0; at--)
      restoreStore(transaction.snapshots[at]);
    transaction.snapshots = [];
    if (transaction.upgrade) {
      restoreDatabase(transaction.db, transaction.upgrade.snapshot);
      transaction.connection.version = transaction.upgrade.snapshot.version;
    }
    const pending = transaction.requests;
    transaction.requests = [];
    task(() => {
      for (const entry of pending) {
        const request = entry.request;
        request.readyState = "done";
        request.result = undefined;
        request.error = new Exception(
          "The transaction was aborted, so the request cannot be fulfilled.",
          "AbortError",
        );
        fire(request.public, "error", { bubbles: true, cancelable: true });
      }
      fire(transaction.public, "abort", { bubbles: true });
      finishTransaction(transaction, true);
    });
  };
  const ensureSnapshot = (transaction, store) => {
    if (transaction.upgrade) return;
    for (const snapshot of transaction.snapshots) if (snapshot.store === store) return;
    transaction.snapshots[transaction.snapshots.length] = snapshotStore(store);
  };
  const TX = IDBTransaction.prototype;
  getter(TX, "transaction", "objectStoreNames", (state) => stringList(state.scope));
  getter(TX, "transaction", "mode", (state) => state.mode);
  getter(TX, "transaction", "durability", () => "default");
  getter(TX, "transaction", "db", (state) => state.connection.public);
  getter(TX, "transaction", "error", (state) => state.error);
  handlers(TX, "transaction", ["onabort", "oncomplete", "onerror"]);
  const isFinished = (state) => state.state === "finished" || state.state === "committing";
  method(TX, "transaction", "objectStore", 1, function (state, args) {
    requireArgs("IDBTransaction", "objectStore", 1, args.length);
    const name = string(args[0]);
    if (isFinished(state))
      throw fail(
        "IDBTransaction",
        "objectStore",
        "The transaction has finished.",
        "InvalidStateError",
      );
    const store = state.db.stores[name];
    if (!store || state.scope.indexOf(name) < 0)
      throw fail(
        "IDBTransaction",
        "objectStore",
        "The specified object store was not found.",
        "NotFoundError",
      );
    return storeHandle(state, store);
  });
  method(TX, "transaction", "abort", 0, function (state) {
    if (isFinished(state))
      throw fail("IDBTransaction", "abort", "The transaction has finished.", "InvalidStateError");
    abortTransaction(state, null);
  });
  method(TX, "transaction", "commit", 0, function (state) {
    if (isFinished(state))
      throw fail("IDBTransaction", "commit", "The transaction has finished.", "InvalidStateError");
    if (state.state !== "active")
      throw fail("IDBTransaction", "commit", "The transaction is not active.", "InvalidStateError");
    state.state = "committing";
    pump(state);
  });

  // ---- object stores and indexes ----------------------------------------------------------
  const storeHandle = (transaction, store) => {
    for (const handle of transaction.handles[store.name] ? [transaction.handles[store.name]] : [])
      if (record(handle).store === store) return handle;
    const state = { kind: "store", transaction, store, indexes: create(null) };
    const handle = bind(create(IDBObjectStore.prototype), state);
    transaction.handles[store.name] = handle;
    return handle;
  };
  const indexHandle = (storeState, index) => {
    const existing = storeState.indexes[index.name];
    if (existing && record(existing).index === index) return existing;
    const state = { kind: "index", store: storeState, index, transaction: storeState.transaction };
    const handle = bind(create(IDBIndex.prototype), state);
    storeState.indexes[index.name] = handle;
    return handle;
  };
  // Shared request precondition checks (deleted source, finished/inactive transaction, read-only).
  const usable = (iface, method, transaction, deleted, write) => {
    if (deleted)
      throw fail(
        iface,
        method,
        iface === "IDBIndex"
          ? "The index or its object store has been deleted."
          : "The object store has been deleted.",
        "InvalidStateError",
      );
    if (isFinished(transaction))
      throw fail(iface, method, "The transaction has finished.", "TransactionInactiveError");
    if (transaction.state !== "active")
      throw fail(iface, method, "The transaction is not active.", "TransactionInactiveError");
    if (write && transaction.mode === "readonly")
      throw fail(iface, method, "The transaction is read-only.", "ReadOnlyError");
  };
  const place = (transaction, source, operation, existing) => {
    const request = existing || newRequest(IDBRequest.prototype, source, transaction);
    transaction.requests[transaction.requests.length] = { request, operation };
    pump(transaction);
    return request;
  };
  const read = (value) => clone(value, realm, undefined, "");
  const storeValue = (iface, method, value) =>
    clone(value, realm, undefined, `Failed to execute '${method}' on '${iface}': `);
  // Writes one record (put/add/cursor update) after the generator and constraint checks.
  const storeRecord = (transaction, store, value, key, noOverwrite) => {
    if (!key) {
      if (store.current > 9007199254740992)
        throw new Exception("Key generator has reached its maximum value.", "ConstraintError");
      key = { t: NUMBER, v: store.current++ };
      if (store.keyPath !== null) inject(value, store.keyPath, key.v);
    } else if (store.autoIncrement && key.t === NUMBER && key.v >= store.current)
      store.current = Math.floor(Math.min(key.v, 9007199254740992)) + 1;
    const at = findRecord(store, key);
    if (at >= 0 && noOverwrite)
      throw new Exception("Key already exists in the object store.", "ConstraintError");
    const indexed = [];
    for (const name of keysOf(store.indexes)) {
      const index = store.indexes[name];
      const keys = indexKeys(index, value);
      if (uniqueClash(index, keys, key))
        throw new Exception(
          `Unable to add key to index '${index.name}': at least one key does not satisfy the uniqueness requirements.`,
          "ConstraintError",
        );
      indexed[indexed.length] = [index, keys];
    }
    ensureSnapshot(transaction, store);
    if (at >= 0) removeRecordAt(store, at);
    store.records.splice(
      seek(store.records, (row) => compare(row.k, key) < 0),
      0,
      { k: key, v: value },
    );
    for (const [index, keys] of indexed)
      for (const indexKey of keys) insertEntry(index, { k: indexKey, p: key });
    return fromKey(key);
  };
  const write = (storeState, method, args, noOverwrite) => {
    requireArgs("IDBObjectStore", method, 1, args.length);
    const { transaction, store } = storeState;
    usable("IDBObjectStore", method, transaction, store.deleted, true);
    const inline = store.keyPath !== null;
    let key = null;
    if (inline && args[1] !== undefined)
      throw fail(
        "IDBObjectStore",
        method,
        "The object store uses in-line keys and the key parameter was provided.",
        "DataError",
      );
    if (!inline && !store.autoIncrement && args[1] === undefined)
      throw fail(
        "IDBObjectStore",
        method,
        "The object store uses out-of-line keys and has no key generator and the key parameter was not provided.",
        "DataError",
      );
    if (args[1] !== undefined) key = validKey("IDBObjectStore", method, args[1]);
    const value = storeValue("IDBObjectStore", method, args[0]);
    if (inline) {
      const found = extractKey(value, store.keyPath);
      if (found === null)
        throw fail(
          "IDBObjectStore",
          method,
          "Evaluating the object store's key path yielded a value that is not a valid key.",
          "DataError",
        );
      if (found === NONE) {
        if (!store.autoIncrement)
          throw fail(
            "IDBObjectStore",
            method,
            "Evaluating the object store's key path did not yield a value.",
            "DataError",
          );
        if (!canInject(value, store.keyPath))
          throw fail(
            "IDBObjectStore",
            method,
            "A generated key could not be inserted into the value.",
            "DataError",
          );
      } else key = found;
    }
    return place(transaction, storeState.public || storeHandle(transaction, store), () =>
      storeRecord(transaction, store, value, key, noOverwrite),
    ).public;
  };
  const countOf = (iface, method, value) =>
    value === undefined ? 0 : enforce(iface, method, value, 4294967295, "unsigned long");
  // Rows a source (store or index) yields for a range, as [key, primaryKey, value] triples.
  const sourceRows = (state, range, direction) => {
    const rows = state.index
      ? entriesInRange(state.index, range).map((entry) => [entry.k, entry.p])
      : recordsInRange(state.store, range).map((row) => [row.k, row.k, row.v]);
    if (state.index) {
      for (const row of rows) {
        const at = findRecord(state.index.store || state.store.store, row[1]);
        row[2] = at >= 0 ? (state.index.store || state.store.store).records[at].v : undefined;
      }
    }
    if (direction === "prev" || direction === "prevunique") rows.reverse();
    if (direction === "nextunique" || direction === "prevunique") {
      const unique = [];
      for (const row of rows) {
        const last = unique[unique.length - 1];
        if (last && !compare(last[0], row[0])) {
          if (direction === "prevunique") unique[unique.length - 1] = row;
          continue;
        }
        unique[unique.length] = row;
      }
      return unique;
    }
    return rows;
  };
  const sourceInfo = (state) =>
    state.kind === "index"
      ? {
          iface: "IDBIndex",
          transaction: state.transaction,
          deleted: state.index.deleted || state.store.store.deleted,
        }
      : { iface: "IDBObjectStore", transaction: state.transaction, deleted: state.store.deleted };
  const directionOf = (iface, method, value) => {
    const direction = value === undefined ? "next" : string(value);
    if (
      direction !== "next" &&
      direction !== "nextunique" &&
      direction !== "prev" &&
      direction !== "prevunique"
    )
      throw typeFail(
        iface,
        method,
        `The provided value '${direction}' is not a valid enum value of type IDBCursorDirection.`,
      );
    return direction;
  };
  const reader = (name, length, body) => [name, length, body];
  const readers = [
    reader("get", 1, (state, info, args) => {
      requireArgs(info.iface, "get", 1, args.length);
      usable(info.iface, "get", info.transaction, info.deleted);
      const range = rangeOf(info.iface, "get", args[0]);
      return () => {
        const row = sourceRows(state, range, "next")[0];
        return row ? read(row[2]) : undefined;
      };
    }),
    reader("getKey", 1, (state, info, args) => {
      requireArgs(info.iface, "getKey", 1, args.length);
      usable(info.iface, "getKey", info.transaction, info.deleted);
      const range = rangeOf(info.iface, "getKey", args[0]);
      return () => {
        const row = sourceRows(state, range, "next")[0];
        return row ? fromKey(row[1]) : undefined;
      };
    }),
    reader("getAll", 0, (state, info, args) => {
      usable(info.iface, "getAll", info.transaction, info.deleted);
      const range = rangeOf(info.iface, "getAll", args[0], true);
      const count = countOf(info.iface, "getAll", args[1]);
      return () => {
        const rows = sourceRows(state, range, "next");
        return rows.slice(0, count || rows.length).map((row) => read(row[2]));
      };
    }),
    reader("getAllKeys", 0, (state, info, args) => {
      usable(info.iface, "getAllKeys", info.transaction, info.deleted);
      const range = rangeOf(info.iface, "getAllKeys", args[0], true);
      const count = countOf(info.iface, "getAllKeys", args[1]);
      return () => {
        const rows = sourceRows(state, range, "next");
        return rows.slice(0, count || rows.length).map((row) => fromKey(row[1]));
      };
    }),
    reader("getAllRecords", 0, (state, info, args) => {
      usable(info.iface, "getAllRecords", info.transaction, info.deleted);
      const options = args[0] == null ? {} : args[0];
      const range = rangeOf(info.iface, "getAllRecords", options.query, true);
      const count = countOf(info.iface, "getAllRecords", options.count);
      const direction = directionOf(info.iface, "getAllRecords", options.direction);
      return () => {
        const rows = sourceRows(state, range, direction);
        return rows.slice(0, count || rows.length).map((row) =>
          bind(create(IDBRecord.prototype), {
            kind: "record",
            key: fromKey(row[0]),
            primaryKey: fromKey(row[1]),
            value: read(row[2]),
          }),
        );
      };
    }),
    reader("count", 0, (state, info, args) => {
      usable(info.iface, "count", info.transaction, info.deleted);
      const range = rangeOf(info.iface, "count", args[0], true);
      return () => sourceRows(state, range, "next").length;
    }),
  ];
  for (const [name, length, body] of readers)
    for (const [prototype, kind] of [
      [IDBObjectStore.prototype, "store"],
      [IDBIndex.prototype, "index"],
    ])
      method(prototype, kind, name, length, function (state, args) {
        const info = sourceInfo(state);
        return place(info.transaction, this, body(state, info, args)).public;
      });

  const OS = IDBObjectStore.prototype;
  getter(
    OS,
    "store",
    "name",
    (state) => state.store.name,
    (state, value) => {
      value = string(value);
      upgradeOf(state.transaction.connection, "name");
      if (value === state.store.name) return;
      if (state.transaction.db.stores[value])
        throw new Exception(
          "An object store with the specified name already exists.",
          "ConstraintError",
        );
      delete state.transaction.db.stores[state.store.name];
      state.transaction.scope = state.transaction.scope.filter((name) => name !== state.store.name);
      state.store.name = value;
      state.transaction.db.stores[value] = state.store;
      state.transaction.scope[state.transaction.scope.length] = value;
    },
  );
  getter(OS, "store", "keyPath", (state) => copyPath(state.store.keyPath));
  getter(OS, "store", "indexNames", (state) => stringList(keysOf(state.store.indexes)));
  getter(OS, "store", "transaction", (state) => state.transaction.public);
  getter(OS, "store", "autoIncrement", (state) => state.store.autoIncrement);
  method(OS, "store", "put", 1, function (state, args) {
    state.public = this;
    return write(state, "put", args, false);
  });
  method(OS, "store", "add", 1, function (state, args) {
    state.public = this;
    return write(state, "add", args, true);
  });
  method(OS, "store", "delete", 1, function (state, args) {
    requireArgs("IDBObjectStore", "delete", 1, args.length);
    usable("IDBObjectStore", "delete", state.transaction, state.store.deleted, true);
    const range = rangeOf("IDBObjectStore", "delete", args[0]);
    const { store, transaction } = state;
    return place(transaction, this, () => {
      const rows = recordsInRange(store, range);
      if (rows.length) ensureSnapshot(transaction, store);
      for (const row of rows) removeRecordAt(store, findRecord(store, row.k));
      return undefined;
    }).public;
  });
  method(OS, "store", "clear", 0, function (state) {
    usable("IDBObjectStore", "clear", state.transaction, state.store.deleted, true);
    const { store, transaction } = state;
    return place(transaction, this, () => {
      ensureSnapshot(transaction, store);
      store.records = [];
      for (const name of keysOf(store.indexes)) store.indexes[name].entries = [];
      return undefined;
    }).public;
  });
  method(OS, "store", "index", 1, function (state, args) {
    requireArgs("IDBObjectStore", "index", 1, args.length);
    const name = string(args[0]);
    if (state.store.deleted)
      throw fail(
        "IDBObjectStore",
        "index",
        "The object store has been deleted.",
        "InvalidStateError",
      );
    if (isFinished(state.transaction))
      throw fail("IDBObjectStore", "index", "The transaction has finished.", "InvalidStateError");
    const index = state.store.indexes[name];
    if (!index)
      throw fail("IDBObjectStore", "index", "The specified index was not found.", "NotFoundError");
    return indexHandle(state, index);
  });
  method(OS, "store", "createIndex", 2, function (state, args) {
    requireArgs("IDBObjectStore", "createIndex", 2, args.length);
    const name = string(args[0]);
    const options = args[2] == null ? {} : args[2];
    const transaction = state.transaction;
    if (transaction.mode !== "versionchange")
      throw fail(
        "IDBObjectStore",
        "createIndex",
        "The database is not running a version change transaction.",
        "InvalidStateError",
      );
    usable("IDBObjectStore", "createIndex", transaction, state.store.deleted);
    const keyPath = keyPathOf(args[1]);
    if (state.store.indexes[name])
      throw fail(
        "IDBObjectStore",
        "createIndex",
        "An index with the specified name already exists.",
        "ConstraintError",
      );
    if (keyPath === false || keyPath === null)
      throw fail(
        "IDBObjectStore",
        "createIndex",
        "The keyPath argument contains an invalid key path.",
        "SyntaxError",
      );
    const unique = !!options.unique,
      multiEntry = !!options.multiEntry;
    if (isArray(keyPath) && multiEntry)
      throw fail(
        "IDBObjectStore",
        "createIndex",
        "The keyPath argument was an array and the multiEntry option is true.",
        "InvalidAccessError",
      );
    const index = {
      name,
      keyPath,
      unique,
      multiEntry,
      entries: [],
      deleted: false,
      store: state.store,
    };
    let clash = false;
    for (const row of state.store.records) {
      const keys = indexKeys(index, row.v);
      if (uniqueClash(index, keys, row.k)) clash = true;
      for (const key of keys) insertEntry(index, { k: key, p: row.k });
    }
    state.store.indexes[name] = index;
    if (clash)
      place(transaction, null, () => {
        throw new Exception(
          `Unable to add key to index '${name}': at least one key does not satisfy the uniqueness requirements.`,
          "ConstraintError",
        );
      });
    return indexHandle(state, index);
  });
  method(OS, "store", "deleteIndex", 1, function (state, args) {
    requireArgs("IDBObjectStore", "deleteIndex", 1, args.length);
    const name = string(args[0]);
    if (state.transaction.mode !== "versionchange")
      throw fail(
        "IDBObjectStore",
        "deleteIndex",
        "The database is not running a version change transaction.",
        "InvalidStateError",
      );
    usable("IDBObjectStore", "deleteIndex", state.transaction, state.store.deleted);
    const index = state.store.indexes[name];
    if (!index)
      throw fail(
        "IDBObjectStore",
        "deleteIndex",
        "The specified index was not found.",
        "NotFoundError",
      );
    index.deleted = true;
    delete state.store.indexes[name];
  });

  const IX = IDBIndex.prototype;
  getter(
    IX,
    "index",
    "name",
    (state) => state.index.name,
    (state, value) => {
      value = string(value);
      upgradeOf(state.transaction.connection, "name");
      if (value === state.index.name) return;
      const indexes = state.store.store.indexes;
      if (indexes[value])
        throw new Exception("An index with the specified name already exists.", "ConstraintError");
      delete indexes[state.index.name];
      state.index.name = value;
      indexes[value] = state.index;
    },
  );
  getter(IX, "index", "objectStore", (state) => storeHandle(state.transaction, state.store.store));
  getter(IX, "index", "keyPath", (state) => copyPath(state.index.keyPath));
  getter(IX, "index", "multiEntry", (state) => state.index.multiEntry);
  getter(IX, "index", "unique", (state) => state.index.unique);

  // ---- cursors -------------------------------------------------------------------------------
  const cursorOpen = (state, info, args, keyOnly) => {
    const method = keyOnly ? "openKeyCursor" : "openCursor";
    usable(info.iface, method, info.transaction, info.deleted);
    const range = rangeOf(info.iface, method, args[0], true);
    const direction = directionOf(info.iface, method, args[1]);
    const cursor = {
      kind: "cursor",
      source: state,
      sourcePublic: null,
      transaction: info.transaction,
      range,
      direction,
      keyOnly,
      key: undefined,
      primaryKey: undefined,
      value: undefined,
      position: null,
      objectPosition: null,
      gotValue: false,
      request: null,
      public: null,
    };
    cursor.public = bind(
      create(keyOnly ? IDBCursor.prototype : IDBCursorWithValue.prototype),
      cursor,
    );
    return cursor;
  };
  for (const [prototype, kind] of [
    [IDBObjectStore.prototype, "store"],
    [IDBIndex.prototype, "index"],
  ])
    for (const name of ["openCursor", "openKeyCursor"])
      method(prototype, kind, name, 0, function (state, args) {
        const info = sourceInfo(state);
        const cursor = cursorOpen(state, info, args, name === "openKeyCursor");
        cursor.sourcePublic = this;
        const request = place(info.transaction, this, () => iterate(cursor, null, null, 1));
        cursor.request = request;
        return request.public;
      });
  // Moves the cursor `count` steps (to `key`/`primaryKey` when given); null when exhausted.
  const iterate = (cursor, key, primaryKey, count) => {
    const forward = cursor.direction === "next" || cursor.direction === "nextunique";
    const unique = cursor.direction === "nextunique" || cursor.direction === "prevunique";
    const rows = sourceRows(cursor.source, cursor.range, cursor.direction);
    let found = null;
    for (const row of rows) {
      if (cursor.position) {
        const order = compare(row[0], cursor.position) * (forward ? 1 : -1);
        if (order < 0) continue;
        if (order === 0) {
          if (unique) continue;
          if (!cursor.source.index) continue;
          if (compare(row[1], cursor.objectPosition) * (forward ? 1 : -1) <= 0) continue;
        }
      }
      if (key) {
        const order = compare(row[0], key) * (forward ? 1 : -1);
        if (order < 0) continue;
        if (primaryKey && order === 0 && compare(row[1], primaryKey) < 0) continue;
      }
      found = row;
      if (--count === 0) break;
    }
    if (!found || count > 0) {
      cursor.key = cursor.primaryKey = cursor.value = undefined;
      cursor.position = null;
      cursor.gotValue = false;
      return null;
    }
    cursor.position = found[0];
    cursor.objectPosition = found[1];
    cursor.key = fromKey(found[0]);
    cursor.primaryKey = fromKey(found[1]);
    cursor.value = cursor.keyOnly ? undefined : read(found[2]);
    cursor.gotValue = true;
    return cursor.public;
  };
  const CU = IDBCursor.prototype;
  getter(CU, "cursor", "source", (state) => state.sourcePublic);
  getter(CU, "cursor", "direction", (state) => state.direction);
  getter(CU, "cursor", "key", (state) => state.key);
  getter(CU, "cursor", "primaryKey", (state) => state.primaryKey);
  getter(CU, "cursor", "request", (state) => state.request.public);
  getter(IDBCursorWithValue.prototype, "cursor", "value", (state) => state.value);
  const cursorReady = (cursor, method) => {
    const transaction = cursor.transaction;
    if (isFinished(transaction) || transaction.state !== "active")
      throw fail(
        "IDBCursor",
        method,
        isFinished(transaction)
          ? "The transaction has finished."
          : "The transaction is not active.",
        "TransactionInactiveError",
      );
    if (sourceInfo(cursor.source).deleted)
      throw fail(
        "IDBCursor",
        method,
        "The cursor's source or effective object store has been deleted.",
        "InvalidStateError",
      );
    if (!cursor.gotValue)
      throw fail(
        "IDBCursor",
        method,
        "The cursor is being iterated or has iterated past its end.",
        "InvalidStateError",
      );
  };
  const advanceRequest = (cursor, key, primaryKey, count) => {
    cursor.gotValue = false;
    cursor.request.readyState = "pending";
    place(
      cursor.transaction,
      cursor.sourcePublic,
      () => iterate(cursor, key, primaryKey, count),
      cursor.request,
    );
  };
  method(CU, "cursor", "advance", 1, function (state, args) {
    requireArgs("IDBCursor", "advance", 1, args.length);
    const count = enforce("IDBCursor", "advance", args[0], 4294967295, "unsigned long");
    if (count === 0)
      throw typeFail(
        "IDBCursor",
        "advance",
        "A count argument with value 0 (zero) was supplied, must be greater than 0.",
      );
    cursorReady(state, "advance");
    advanceRequest(state, null, null, count);
  });
  method(CU, "cursor", "continue", 0, function (state, args) {
    let key = null;
    if (args[0] !== undefined) key = validKey("IDBCursor", "continue", args[0]);
    cursorReady(state, "continue");
    if (key) {
      const forward = state.direction === "next" || state.direction === "nextunique";
      const order = compare(key, state.position);
      if (forward ? order <= 0 : order >= 0)
        throw fail(
          "IDBCursor",
          "continue",
          forward
            ? "The parameter is less than or equal to this cursor's position."
            : "The parameter is greater than or equal to this cursor's position.",
          "DataError",
        );
    }
    advanceRequest(state, key, null, 1);
  });
  method(CU, "cursor", "continuePrimaryKey", 2, function (state, args) {
    requireArgs("IDBCursor", "continuePrimaryKey", 2, args.length);
    const key = validKey("IDBCursor", "continuePrimaryKey", args[0]);
    const primaryKey = validKey("IDBCursor", "continuePrimaryKey", args[1]);
    cursorReady(state, "continuePrimaryKey");
    if (!state.source.index)
      throw fail(
        "IDBCursor",
        "continuePrimaryKey",
        "The cursor's source is not an index.",
        "InvalidAccessError",
      );
    if (state.direction === "nextunique" || state.direction === "prevunique")
      throw fail(
        "IDBCursor",
        "continuePrimaryKey",
        "The cursor's direction is not 'next' or 'prev'.",
        "InvalidAccessError",
      );
    const forward = state.direction === "next";
    const order = compare(key, state.position);
    if (
      forward
        ? order < 0 || (order === 0 && compare(primaryKey, state.objectPosition) <= 0)
        : order > 0 || (order === 0 && compare(primaryKey, state.objectPosition) >= 0)
    )
      throw fail(
        "IDBCursor",
        "continuePrimaryKey",
        forward
          ? "The parameter is less than or equal to this cursor's position."
          : "The parameter is greater than or equal to this cursor's position.",
        "DataError",
      );
    advanceRequest(state, key, primaryKey, 1);
  });
  const effectiveStore = (cursor) =>
    cursor.source.index ? cursor.source.store.store : cursor.source.store;
  method(CU, "cursor", "update", 1, function (state, args) {
    requireArgs("IDBCursor", "update", 1, args.length);
    const transaction = state.transaction;
    if (
      !isFinished(transaction) &&
      transaction.state === "active" &&
      transaction.mode === "readonly"
    )
      throw fail(
        "IDBCursor",
        "update",
        "The record may not be updated inside a read-only transaction.",
        "ReadOnlyError",
      );
    cursorReady(state, "update");
    if (state.keyOnly)
      throw fail("IDBCursor", "update", "The cursor is a key cursor.", "InvalidStateError");
    const store = effectiveStore(state);
    const value = storeValue("IDBCursor", "update", args[0]);
    const primary = toKey(state.primaryKey);
    if (store.keyPath !== null) {
      const found = extractKey(value, store.keyPath);
      if (found === NONE || found === null || compare(found, primary))
        throw fail(
          "IDBCursor",
          "update",
          "The effective object store of this cursor uses in-line keys and evaluating the key path of the value parameter results in a different value than the cursor's effective key.",
          "DataError",
        );
    }
    return place(transaction, this, () => storeRecord(transaction, store, value, primary, false))
      .public;
  });
  method(CU, "cursor", "delete", 0, function (state) {
    const transaction = state.transaction;
    if (
      !isFinished(transaction) &&
      transaction.state === "active" &&
      transaction.mode === "readonly"
    )
      throw fail(
        "IDBCursor",
        "delete",
        "The record may not be deleted inside a read-only transaction.",
        "ReadOnlyError",
      );
    cursorReady(state, "delete");
    if (state.keyOnly)
      throw fail("IDBCursor", "delete", "The cursor is a key cursor.", "InvalidStateError");
    const store = effectiveStore(state);
    const primary = toKey(state.primaryKey);
    return place(transaction, this, () => {
      const at = findRecord(store, primary);
      if (at >= 0) {
        ensureSnapshot(transaction, store);
        removeRecordAt(store, at);
      }
      return undefined;
    }).public;
  });

  // ---- IDBRecord, IDBKeyRange ----------------------------------------------------------------
  for (const name of ["key", "primaryKey", "value"])
    getter(IDBRecord.prototype, "record", name, (state) => state[name]);
  const KR = IDBKeyRange.prototype;
  getter(KR, "range", "lower", (state) => (state.lower ? fromKey(state.lower) : undefined));
  getter(KR, "range", "upper", (state) => (state.upper ? fromKey(state.upper) : undefined));
  getter(KR, "range", "lowerOpen", (state) => state.lowerOpen);
  getter(KR, "range", "upperOpen", (state) => state.upperOpen);
  method(KR, "range", "includes", 1, function (state, args) {
    requireArgs("IDBKeyRange", "includes", 1, args.length);
    return inRange(state, validKey("IDBKeyRange", "includes", args[0]));
  });
  const newRange = (lower, upper, lowerOpen, upperOpen) =>
    bind(create(KR), { kind: "range", lower, upper, lowerOpen, upperOpen });
  const staticMethod = (name, length, body) =>
    define(IDBKeyRange, name, {
      __proto__: null,
      value: make(
        {
          [name](...args) {
            requireArgs("IDBKeyRange", name, length, args.length);
            return body(args);
          },
        }[name],
        name,
        length,
      ),
      writable: true,
      enumerable: true,
      configurable: true,
    });
  staticMethod("only", 1, (args) => {
    const key = validKey("IDBKeyRange", "only", args[0]);
    return newRange(key, key, false, false);
  });
  staticMethod("lowerBound", 1, (args) =>
    newRange(validKey("IDBKeyRange", "lowerBound", args[0]), null, !!args[1], true),
  );
  staticMethod("upperBound", 1, (args) =>
    newRange(null, validKey("IDBKeyRange", "upperBound", args[0]), true, !!args[1]),
  );
  staticMethod("bound", 2, (args) => {
    const lower = validKey("IDBKeyRange", "bound", args[0]),
      upper = validKey("IDBKeyRange", "bound", args[1]);
    const lowerOpen = !!args[2],
      upperOpen = !!args[3];
    const order = compare(lower, upper);
    if (order > 0)
      throw fail(
        "IDBKeyRange",
        "bound",
        "The lower key is greater than the upper key.",
        "DataError",
      );
    if (order === 0 && (lowerOpen || upperOpen))
      throw fail(
        "IDBKeyRange",
        "bound",
        "The lower key and upper key are equal and one of the bounds is open.",
        "DataError",
      );
    return newRange(lower, upper, lowerOpen, upperOpen);
  });

  define(realm, "indexedDB", {
    __proto__: null,
    value: __G.idbFactory(false),
    writable: true,
    enumerable: true,
    configurable: true,
  });
})();
