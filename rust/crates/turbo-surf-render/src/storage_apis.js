// Web Locks, Cookie Store and Cache Storage (#108, harness/detection/storage-apis-probe.js):
// each performs its real operation. Locks and caches are held per realm, in memory;
// cookieStore reads and writes the document's cookie jar (its partition jar in a frame).
(() => {
  "use strict";
  if (!ops.op_is_secure_context()) return;
  const apply = Reflect.apply,
    make = __G.makeNativeFunction,
    define = Object.defineProperty;
  const ErrorType = __exceptionTypeError,
    Exception = DOMException,
    HostPromise = Promise,
    HostRequest = Request,
    HostResponse = Response,
    HostURL = URL,
    StringValue = String;
  const later = (callback) => HostPromise.resolve().then(callback);
  const records = new WeakMap();
  const record = (receiver, kind) => {
    const value =
      receiver !== null && typeof receiver === "object" ? records.get(receiver) : undefined;
    if (!value || value.kind !== kind) throw new ErrorType("Illegal invocation");
    return value;
  };
  const string = (value) => {
    if (typeof value === "symbol") throw new ErrorType("Cannot convert a Symbol to a string");
    return StringValue(value);
  };
  const failure = (method, owner, message) =>
    `Failed to execute '${method}' on '${owner}': ${message}`;
  const arity = (method, owner, count, needed) => {
    if (count < needed)
      throw new ErrorType(
        failure(
          method,
          owner,
          `${needed} argument${needed === 1 ? "" : "s"} required, but only ${count} present.`,
        ),
      );
  };
  const instance = (constructor, kind, state) => {
    const object = Object.create(constructor.prototype);
    records.set(object, { kind, ...state });
    return object;
  };
  const methods = (constructor, kind, table) => {
    for (const [name, length, body] of table)
      define(constructor.prototype, name, {
        __proto__: null,
        value: make(
          function (...args) {
            try {
              return HostPromise.resolve(apply(body, null, [record(this, kind), args]));
            } catch (error) {
              return HostPromise.reject(error);
            }
          },
          name,
          length,
        ),
        writable: true,
        enumerable: true,
        configurable: true,
      });
  };
  const getters = (constructor, kind, names) => {
    for (const name of names)
      define(constructor.prototype, name, {
        __proto__: null,
        get: make(
          function () {
            return record(this, kind)[name];
          },
          "get " + name,
          0,
        ),
        enumerable: true,
        configurable: true,
      });
  };
  const singleton = (target, name, value) =>
    define(target, name, {
      __proto__: null,
      get: make(() => value, "get " + name, 0),
      enumerable: true,
      configurable: true,
    });

  // Web Locks: a FIFO queue per name; a request is granted when nothing ahead of it
  // conflicts, and its lock is held until the callback's result settles.
  const Lock = __defineInterface("Lock", 0, null);
  getters(Lock, "lock", ["name", "mode"]);
  const LockManager = __defineInterface("LockManager", 0, null);
  const clientId = "xxxxxxxx-xxxx-4xxx-8xxx-xxxxxxxxxxxx".replace(/x/g, () =>
    ((Math.random() * 16) | 0).toString(16),
  );
  const held = [],
    pending = [];
  const conflicts = (request, others) =>
    others.some(
      (other) =>
        other.name === request.name && (request.mode === "exclusive" || other.mode === "exclusive"),
    );
  const grantable = (request) =>
    !conflicts(request, held) && !conflicts(request, pending.slice(0, pending.indexOf(request)));
  const release = (request) => {
    const at = held.indexOf(request);
    if (at < 0) return;
    held.splice(at, 1);
    process();
  };
  const grant = (request) => {
    held.push(request);
    run(request, instance(Lock, "lock", { name: request.name, mode: request.mode }));
  };
  const run = (request, lock) =>
    later(() => {
      let result;
      try {
        result = HostPromise.resolve(request.callback(lock));
      } catch (error) {
        result = HostPromise.reject(error);
      }
      result.then(
        (value) => {
          release(request);
          request.resolve(value);
        },
        (error) => {
          release(request);
          request.reject(error);
        },
      );
    });
  const process = () => {
    for (const request of [...pending])
      if (grantable(request)) {
        pending.splice(pending.indexOf(request), 1);
        grant(request);
      }
  };
  const lockOptions = (options) => {
    const signal = options.signal === undefined ? undefined : options.signal;
    const mode = options.mode === undefined ? "exclusive" : string(options.mode);
    if (mode !== "exclusive" && mode !== "shared")
      throw new ErrorType(
        failure(
          "request",
          "LockManager",
          `Failed to read the 'mode' property from 'LockOptions': The provided value '${mode}' is not a valid enum value of type LockMode.`,
        ),
      );
    return { mode, ifAvailable: !!options.ifAvailable, steal: !!options.steal, signal };
  };
  const unsupported = (message) =>
    new Exception(failure("request", "LockManager", message), "NotSupportedError");
  methods(LockManager, "locks", [
    [
      "request",
      2,
      (_state, args) => {
        arity("request", "LockManager", args.length, 2);
        const name = string(args[0]);
        const options =
          args.length > 2 && args[1] !== null && args[1] !== undefined
            ? lockOptions(args[1])
            : lockOptions({});
        const callback = args.length > 2 ? args[2] : args[1];
        if (typeof callback !== "function")
          throw new ErrorType(
            failure(
              "request",
              "LockManager",
              "The callback provided as parameter 3 is not a function.",
            ),
          );
        if (name.startsWith("-")) throw unsupported("Names cannot start with '-'.");
        if (options.steal && options.ifAvailable)
          throw unsupported("The 'steal' and 'ifAvailable' options cannot be used together.");
        if (options.steal && options.mode !== "exclusive")
          throw unsupported("The 'steal' option may only be used with 'exclusive' locks.");
        if (options.signal && (options.steal || options.ifAvailable))
          throw unsupported(
            `The 'signal' and '${options.steal ? "steal" : "ifAvailable"}' options cannot be used together.`,
          );
        if (options.signal && options.signal.aborted) throw options.signal.reason;
        return new HostPromise((resolve, reject) => {
          const request = { name, mode: options.mode, callback, resolve, reject };
          if (options.steal) {
            for (const other of held.filter((other) => other.name === name)) {
              held.splice(held.indexOf(other), 1);
              other.reject(
                new Exception(
                  "Lock broken by another request with the 'steal' option.",
                  "AbortError",
                ),
              );
            }
            grant(request);
            return;
          }
          pending.push(request);
          if (grantable(request)) {
            pending.splice(pending.indexOf(request), 1);
            grant(request);
          } else if (options.ifAvailable) {
            pending.splice(pending.indexOf(request), 1);
            run(request, null);
          } else if (options.signal)
            options.signal.addEventListener("abort", () => {
              const at = pending.indexOf(request);
              if (at < 0) return;
              pending.splice(at, 1);
              reject(options.signal.reason);
            });
        });
      },
    ],
    [
      "query",
      0,
      () => {
        const snapshot = (list) => list.map(({ name, mode }) => ({ name, mode, clientId }));
        return { held: snapshot(held), pending: snapshot(pending) };
      },
    ],
  ]);
  const locks = instance(LockManager, "locks", {});
  singleton(Navigator.prototype, "locks", locks);

  // Cookie Store over the document's jar; changes it makes fire `change` on a later task.
  const CookieStore = __defineInterface("CookieStore", 0, EventTarget);
  const changeRecords = new WeakMap();
  const CookieChangeEvent = __defineInterface(
    "CookieChangeEvent",
    1,
    Event,
    function (type, init = undefined) {
      if (!new.target)
        throw new ErrorType(
          "Failed to construct 'CookieChangeEvent': Please use the 'new' operator, this DOM object constructor cannot be called as a function.",
        );
      arity("construct", "CookieChangeEvent", arguments.length, 1);
      const event = Reflect.construct(Event, [type, init], new.target);
      const options = init || {};
      changeRecords.set(event, {
        changed: Object.freeze([...(options.changed || [])]),
        deleted: Object.freeze([...(options.deleted || [])]),
      });
      return event;
    },
  );
  for (const name of ["changed", "deleted"])
    define(CookieChangeEvent.prototype, name, {
      __proto__: null,
      get: make(
        function () {
          const value = changeRecords.get(this);
          if (!value) throw new ErrorType("Illegal invocation");
          return value[name];
        },
        "get " + name,
        0,
      ),
      enumerable: true,
      configurable: true,
    });
  const list = () => ops.op_cookie_store_list(__document);
  const query = (method, args) => {
    const first = args[0];
    if (first === undefined) return {};
    if (typeof first === "object" && first !== null)
      return { name: first.name === undefined ? undefined : string(first.name) };
    return { name: string(first) };
  };
  const matching = (method, args) => {
    const { name } = query(method, args);
    return list().filter((cookie) => name === undefined || cookie.name === name);
  };
  const malformed = /[\u0000-\u0008\u000a-\u001f\u007f;]/;
  const write = (method, name, value, options) => {
    const fail = (message) => new ErrorType(failure(method, "CookieStore", message));
    if (name.includes("=")) throw fail("Cookie name cannot contain '='");
    if (!name && !value) throw fail("Cookie name and value both cannot be empty");
    if (malformed.test(name) || malformed.test(value))
      throw fail("Cookie was malformed and could not be stored, due to problem(s) while parsing.");
    const before = list();
    let line = name ? `${name}=${value}` : value;
    line += `; Path=${options.path === undefined ? "/" : string(options.path)}`;
    if (options.domain !== undefined && options.domain !== null)
      line += `; Domain=${string(options.domain)}`;
    if (options.expires !== undefined && options.expires !== null)
      line += `; Expires=${new Date(Number(options.expires)).toUTCString()}`;
    line += `; SameSite=${options.sameSite === undefined ? "Strict" : string(options.sameSite)}; Secure`;
    __documentRuntimeState.writeCookie(line);
    const after = list();
    const key = (cookie) => `${cookie.domain} ${cookie.path} ${cookie.name}`;
    const changed = after.filter(
      (cookie) => !before.some((old) => key(old) === key(cookie) && old.value === cookie.value),
    );
    const deleted = before
      .filter((cookie) => !after.some((now) => key(now) === key(cookie)))
      .map(({ name, path }) => ({ name, path }));
    if (changed.length || deleted.length)
      later(() =>
        __dispatchTrusted(cookieStore, new CookieChangeEvent("change", { changed, deleted })),
      );
  };
  const setArguments = (args) => {
    arity("set", "CookieStore", args.length, 1);
    if (typeof args[0] === "object" && args[0] !== null) {
      const options = args[0];
      return [string(options.name), string(options.value), options];
    }
    arity("set", "CookieStore", args.length, 2);
    return [string(args[0]), string(args[1]), {}];
  };
  methods(CookieStore, "cookies", [
    ["get", 0, (_state, args) => matching("get", args)[0] || null],
    ["getAll", 0, (_state, args) => matching("getAll", args)],
    ["set", 1, (_state, args) => apply(write, null, ["set", ...setArguments(args)])],
    [
      "delete",
      1,
      (_state, args) => {
        arity("delete", "CookieStore", args.length, 1);
        const options = typeof args[0] === "object" && args[0] !== null ? args[0] : {};
        const name = string(options === args[0] ? options.name : args[0]);
        write("delete", name, "", { ...options, expires: 0 });
      },
    ],
  ]);
  const cookieStore = Object.setPrototypeOf(new EventTarget(), CookieStore.prototype);
  records.set(cookieStore, { kind: "cookies" });
  __defineEventHandler(CookieStore.prototype, "onchange", (receiver) =>
    record(receiver, "cookies"),
  );
  singleton(globalThis, "cookieStore", cookieStore);

  // Cache Storage: named caches of request/response pairs in insertion order.
  const Cache = __defineInterface("Cache", 0, null);
  const CacheStorage = __defineInterface("CacheStorage", 0, null);
  const stores = new Map();
  const toRequest = (input) => (input instanceof HostRequest ? input : new HostRequest(input));
  const strip = (url, ignoreSearch) => {
    const parsed = new HostURL(url);
    parsed.hash = "";
    if (ignoreSearch) parsed.search = "";
    return parsed.href;
  };
  const matches = (entry, request, options = {}) =>
    (options.ignoreMethod || request.method === "GET") &&
    strip(entry.request.url, options.ignoreSearch) === strip(request.url, options.ignoreSearch);
  const lookup = (entries, input, options) => {
    if (input === undefined) return entries;
    const request = toRequest(input);
    return entries.filter((entry) => matches(entry, request, options || {}));
  };
  const put = (entries, input, response) => {
    const fail = (message) => new ErrorType(failure("put", "Cache", message));
    const request = toRequest(input);
    const scheme = new HostURL(request.url).protocol.slice(0, -1);
    if (scheme !== "http" && scheme !== "https")
      throw fail(`Request scheme '${scheme}' is unsupported`);
    if (request.method !== "GET") throw fail(`Request method '${request.method}' is unsupported`);
    if (!(response instanceof HostResponse)) throw fail("parameter 2 is not of type 'Response'.");
    if (response.status === 206) throw fail("Partial response (status code 206) is unsupported");
    if (response.bodyUsed) throw fail("Response body is already used");
    for (const entry of lookup(entries, request)) entries.splice(entries.indexOf(entry), 1);
    entries.push({ request: request.clone(), response: response.clone() });
  };
  const add = async (entries, inputs) => {
    const requests = inputs.map(toRequest);
    const responses = await HostPromise.all(requests.map((request) => fetch(request)));
    responses.forEach((response) => {
      if (!response.ok) throw new ErrorType(failure("addAll", "Cache", "Request failed"));
    });
    requests.forEach((request, at) => put(entries, request, responses[at]));
  };
  methods(Cache, "cache", [
    [
      "match",
      1,
      ({ entries }, [input, options]) => {
        const [entry] = lookup(entries, input, options);
        return entry ? entry.response.clone() : undefined;
      },
    ],
    [
      "matchAll",
      0,
      ({ entries }, [input, options]) =>
        lookup(entries, input, options).map((entry) => entry.response.clone()),
    ],
    ["add", 1, ({ entries }, [input]) => add(entries, [input])],
    ["addAll", 1, ({ entries }, [inputs]) => add(entries, [...inputs])],
    [
      "put",
      2,
      ({ entries }, args) => {
        arity("put", "Cache", args.length, 2);
        put(entries, args[0], args[1]);
      },
    ],
    [
      "delete",
      1,
      ({ entries }, [input, options]) => {
        const found = lookup(entries, input, options);
        for (const entry of found) entries.splice(entries.indexOf(entry), 1);
        return found.length > 0;
      },
    ],
    [
      "keys",
      0,
      ({ entries }, [input, options]) =>
        lookup(entries, input, options).map((entry) => entry.request.clone()),
    ],
  ]);
  methods(CacheStorage, "caches", [
    [
      "open",
      1,
      (_state, args) => {
        arity("open", "CacheStorage", args.length, 1);
        const name = string(args[0]);
        if (!stores.has(name)) stores.set(name, []);
        return instance(Cache, "cache", { entries: stores.get(name) });
      },
    ],
    ["has", 1, (_state, [name]) => stores.has(string(name))],
    ["delete", 1, (_state, [name]) => stores.delete(string(name))],
    ["keys", 0, () => [...stores.keys()]],
    [
      "match",
      1,
      (_state, [input, options = {}]) => {
        const names =
          options && options.cacheName !== undefined
            ? [string(options.cacheName)]
            : [...stores.keys()];
        for (const name of names) {
          const [entry] = lookup(stores.get(name) || [], input, options);
          if (entry) return entry.response.clone();
        }
        return undefined;
      },
    ],
  ]);
  singleton(globalThis, "caches", instance(CacheStorage, "caches", {}));
})();
