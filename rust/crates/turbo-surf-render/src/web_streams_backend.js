// The official algorithms and their symbol records never enter a guest object.
(function (core, make) {
  "use strict";
  const ops = core.ops,
    get = ops.op_interface_record_get,
    set = ops.op_interface_record_set;
  const define = Object.defineProperty,
    create = Object.create,
    descriptors = Object.getOwnPropertyDescriptors;
  const apply = Reflect.apply,
    construct = Reflect.construct,
    own = Object.hasOwn;
  const PromiseType = Promise,
    then = Promise.prototype.then,
    resolvePromise = Promise.resolve.bind(Promise),
    rejectPromise = Promise.reject.bind(Promise),
    TypeErrorType = TypeError;
  const Exception = DOMException,
    ControllerBefore = AbortController;
  const oldSignal = Object.getOwnPropertyDescriptor(ControllerBefore.prototype, "signal").get;
  const oldAbort = ControllerBefore.prototype.abort;
  const rawOwner = ops.Crypto;
  let shared = get(rawOwner);
  if (!shared) {
    shared = create(null);
    shared.kind = "NativeCryptoBackend";
    set(rawOwner, shared);
  }
  if (!own(shared, "webBackend")) {
    const streams = core.loadExtScript("ext:deno_web/06_streams.js");
    const abort = core.loadExtScript("ext:deno_web/03_abort_signal.js");
    core.loadExtScript("ext:deno_web/02_event.js");
    const exceptions = core.loadExtScript("ext:deno_web/01_dom_exception.js");
    const signal = descriptors(abort.AbortSignal.prototype);
    const backend = {
      streams,
      abort,
      signal,
      authoredErrors: new WeakSet(),
      controller: descriptors(abort.AbortController.prototype),
      typeErrorPrototype: Object.getPrototypeOf(core.buildCustomError("TypeError", "")),
      rangeErrorPrototype: Object.getPrototypeOf(core.buildCustomError("RangeError", "")),
      exception: descriptors(exceptions.DOMException.prototype),
    };
    // This function belongs to the runtime's main realm. A live ancestor signal
    // retains only a numeric native weak identity, never a child realm closure.
    const rawAdd = signal.addEventListener.value,
      rawRemove = signal.removeEventListener.value;
    backend.listen = (raw, id) => {
      const listener = () => ops.op_web_abort_dispatch(id, apply(signal.reason.get, raw, []));
      apply(rawAdd, raw, ["abort", listener, { once: true }]);
      return listener;
    };
    backend.cleanup = (raw, listener) => apply(rawRemove, raw, ["abort", listener]);
    const frontMap = new WeakMap(),
      frontGet = WeakMap.prototype.get.bind(frontMap),
      frontSet = WeakMap.prototype.set.bind(frontMap),
      deref = WeakRef.prototype.deref;
    backend.frontGet = (raw) => {
      const weak = frontGet(raw);
      return weak === undefined ? undefined : apply(deref, weak, []);
    };
    backend.frontSet = (raw, front) => frontSet(raw, new WeakRef(front));
    define(backend, "authoredHas", { value: WeakSet.prototype.has.bind(backend.authoredErrors) });
    define(backend, "authoredAdd", { value: WeakSet.prototype.add.bind(backend.authoredErrors) });
    define(shared, "webBackend", { value: backend });
  }
  const backend = shared.webBackend,
    constructors = create(null);
  const getFront = backend.frontGet,
    setFront = backend.frontSet,
    sizeFunctions = new WeakMap(),
    getSize = WeakMap.prototype.get.bind(sizeFunctions),
    setSize = WeakMap.prototype.set.bind(sizeFunctions);
  const isPrototypeOf = Object.prototype.isPrototypeOf;
  const localError = (error) => {
    if (
      error === null ||
      (typeof error !== "object" && typeof error !== "function") ||
      backend.authoredHas(error)
    )
      return error;
    if (
      apply(isPrototypeOf, backend.typeErrorPrototype, [error]) &&
      !(error instanceof TypeErrorType)
    )
      return new TypeErrorType(error.message);
    if (
      apply(isPrototypeOf, backend.rangeErrorPrototype, [error]) &&
      !(error instanceof RangeError)
    )
      return new RangeError(error.message);
    try {
      return new Exception(
        apply(backend.exception.message.get, error, []),
        apply(backend.exception.name.get, error, []),
      );
    } catch {
      return error;
    }
  };
  const authored = (value) => {
    if (value !== null && (typeof value === "object" || typeof value === "function"))
      backend.authoredAdd(value);
    return value;
  };
  const record = (object, name) => {
    if (object === null || (typeof object !== "object" && typeof object !== "function"))
      throw new TypeErrorType("Illegal invocation");
    const state = get(object)?.web;
    if (!state || state.name !== name) throw new TypeErrorType("Illegal invocation");
    return state;
  };
  const save = (front, name, raw) => {
    let state = get(front);
    if (!state) {
      state = create(null);
      state.kind = "WebInterface";
      set(front, state);
    }
    define(state, "web", { value: { name, raw, realm }, configurable: true });
    setFront(raw, front);
    return front;
  };
  const unwrap = (front, name) => record(front, name).raw;
  const wrap = (raw, name) => {
    if (raw === null || raw === undefined) return raw;
    const previous = getFront(raw);
    if (previous) return previous;
    return save(create(constructors[name].prototype), name, raw);
  };
  const promise = (value, convert = (value) => value) =>
    new PromiseType((resolve, reject) => {
      apply(then, resolvePromise(value), [
        (value) => {
          try {
            resolve(convert(value));
          } catch (error) {
            reject(localError(error));
          }
        },
        (error) => reject(localError(error)),
      ]);
    });
  const result = (name, member, value) => {
    if (member === "size" && typeof value === "function") {
      let fn = getSize(value);
      if (!fn) {
        fn =
          name === "ByteLengthQueuingStrategy"
            ? make(
                function (chunk) {
                  return chunk.byteLength;
                },
                "size",
                1,
              )
            : make(
                function () {
                  return 1;
                },
                "size",
                0,
              );
        setSize(value, fn);
      }
      return fn;
    }
    if (member === "read" || member === "next" || member === "return")
      return promise(value, (item) => ({ value: item.value, done: item.done }));
    if (["closed", "ready", "write", "close", "abort", "cancel", "pipeTo"].includes(member))
      return promise(value);
    if (member === "getReader")
      return wrap(
        value,
        value instanceof backend.streams.ReadableStreamBYOBReader
          ? "ReadableStreamBYOBReader"
          : "ReadableStreamDefaultReader",
      );
    if (member === "getWriter") return wrap(value, "WritableStreamDefaultWriter");
    if (member === "byobRequest") return wrap(value, "ReadableStreamBYOBRequest");
    if (member === "signal") return signalFront(value);
    if (member === "readable") return wrap(value, "ReadableStream");
    if (member === "writable") return wrap(value, "WritableStream");
    if (member === "pipeThrough") return wrap(value, "ReadableStream");
    if (member === "tee")
      return [wrap(value[0], "ReadableStream"), wrap(value[1], "ReadableStream")];
    if (member === "values") return iteratorFront(value);
    return value;
  };
  const argumentsFor = (name, member, args) => {
    if (member === "pipeTo") return [unwrap(args[0], "WritableStream"), pipeOptions(args[1])];
    if (member === "pipeThrough")
      return [
        {
          readable: unwrap(args[0].readable, "ReadableStream"),
          writable: unwrap(args[0].writable, "WritableStream"),
        },
        pipeOptions(args[1]),
      ];
    if (["error", "cancel", "abort"].includes(member) && args.length) authored(args[0]);
    return args;
  };
  const pipeOptions = (value) =>
    value === null || value === undefined
      ? value
      : {
          preventAbort: value.preventAbort,
          preventCancel: value.preventCancel,
          preventClose: value.preventClose,
          signal: value.signal === undefined ? undefined : unwrap(value.signal, "AbortSignal"),
        };
  const asyncIteratorBase = Object.getPrototypeOf(
    Object.getPrototypeOf(Object.getPrototypeOf((async function* () {})())),
  );
  const iteratorPrototype = create(asyncIteratorBase);
  for (const member of ["next", "return"])
    define(iteratorPrototype, member, {
      value: make(
        function (...args) {
          try {
            const state = record(this, "StreamIterator");
            return state.realm.result(
              "Iterator",
              member,
              apply(state.raw[member], state.raw, args),
            );
          } catch (error) {
            return rejectPromise(localError(error));
          }
        },
        member,
        member === "next" ? 0 : 1,
      ),
      writable: true,
      configurable: true,
      enumerable: true,
    });
  define(iteratorPrototype, Symbol.toStringTag, {
    value: "ReadableStream AsyncIterator",
    configurable: true,
  });
  function iteratorFront(raw) {
    return save(create(iteratorPrototype), "StreamIterator", raw);
  }
  const controllerName = (raw, fallback) =>
    raw instanceof backend.streams.ReadableByteStreamController
      ? "ReadableByteStreamController"
      : fallback;
  function sourceAdapter(source, kind) {
    if (source === null || source === undefined) return source;
    const adapter = create(null);
    const methods =
      kind === "WritableStream"
        ? ["start", "write", "close", "abort"]
        : kind === "TransformStream"
          ? ["start", "transform", "flush", "cancel"]
          : ["start", "pull", "cancel"];
    for (const member of ["type", "autoAllocateChunkSize", "readableType", "writableType"])
      define(adapter, member, {
        get() {
          try {
            return source[member];
          } catch (error) {
            throw authored(error);
          }
        },
        enumerable: true,
      });
    for (const member of methods)
      define(adapter, member, {
        get() {
          let callback;
          try {
            callback = source[member];
          } catch (error) {
            throw authored(error);
          }
          if (callback === undefined) return undefined;
          if (typeof callback !== "function")
            throw new TypeErrorType("Underlying callback must be a function");
          return function (...args) {
            const index =
              (kind === "WritableStream" && member === "write") ||
              (kind === "TransformStream" && member === "transform")
                ? 1
                : 0;
            if (!["cancel", "abort", "close"].includes(member))
              args[index] = wrap(
                args[index],
                controllerName(
                  args[index],
                  kind === "WritableStream"
                    ? "WritableStreamDefaultController"
                    : kind === "TransformStream"
                      ? "TransformStreamDefaultController"
                      : "ReadableStreamDefaultController",
                ),
              );
            try {
              const outcome = apply(callback, source, args);
              return new PromiseType((resolve, reject) =>
                apply(then, resolvePromise(outcome), [resolve, (error) => reject(authored(error))]),
              );
            } catch (error) {
              throw authored(error);
            }
          };
        },
        enumerable: true,
      });
    return adapter;
  }
  function strategyAdapter(strategy) {
    if (
      strategy === null ||
      strategy === undefined ||
      (typeof strategy !== "object" && typeof strategy !== "function")
    )
      return strategy;
    const adapter = create(null);
    for (const name of ["highWaterMark", "size"])
      define(adapter, name, {
        enumerable: true,
        get() {
          let value;
          try {
            value = strategy[name];
          } catch (error) {
            throw authored(error);
          }
          if (name !== "size" || typeof value !== "function") return value;
          return function (...args) {
            try {
              return apply(value, undefined, args);
            } catch (error) {
              throw authored(error);
            }
          };
        },
      });
    return adapter;
  }
  const realmWindow = globalThis;
  const realm = { result, localError, wrap, active: () => ops.op_window_active(realmWindow) };
  const names = [
    "ReadableStream",
    "ReadableStreamDefaultReader",
    "ReadableStreamBYOBReader",
    "ReadableStreamDefaultController",
    "ReadableByteStreamController",
    "ReadableStreamBYOBRequest",
    "WritableStream",
    "WritableStreamDefaultWriter",
    "WritableStreamDefaultController",
    "TransformStream",
    "TransformStreamDefaultController",
    "ByteLengthQueuingStrategy",
    "CountQueuingStrategy",
  ];
  for (const name of names) {
    const Raw = backend.streams[name],
      publicConstructor = [
        "ReadableStream",
        "WritableStream",
        "TransformStream",
        "ReadableStreamDefaultReader",
        "ReadableStreamBYOBReader",
        "WritableStreamDefaultWriter",
        "ByteLengthQueuingStrategy",
        "CountQueuingStrategy",
      ].includes(name);
    const ctor = make(
      function (...args) {
        if (!publicConstructor) throw new TypeErrorType("Illegal constructor");
        try {
          if (["ReadableStream", "WritableStream", "TransformStream"].includes(name))
            args[0] = sourceAdapter(args[0], name);
          if (["ReadableStream", "WritableStream", "TransformStream"].includes(name))
            args[1] = strategyAdapter(args[1]);
          if (name === "TransformStream") args[2] = strategyAdapter(args[2]);
          if (name === "ByteLengthQueuingStrategy" || name === "CountQueuingStrategy")
            args[0] = strategyAdapter(args[0]);
          if (name === "ReadableStreamDefaultReader" || name === "ReadableStreamBYOBReader")
            args[0] = unwrap(args[0], "ReadableStream");
          if (name === "WritableStreamDefaultWriter") args[0] = unwrap(args[0], "WritableStream");
          save(this, name, construct(Raw, args));
        } catch (error) {
          throw localError(error);
        }
      },
      name,
      Raw.length,
      true,
      true,
    );
    constructors[name] = ctor;
    for (const [member, descriptor] of Object.entries(descriptors(Raw.prototype))) {
      if (member === "constructor") continue;
      const copy = { ...descriptor, enumerable: true };
      for (const type of ["get", "set", "value"])
        if (typeof copy[type] === "function") {
          const native = copy[type];
          copy[type] = make(
            function (...args) {
              try {
                const state = record(this, name);
                if (
                  name === "ReadableStreamDefaultReader" &&
                  member === "read" &&
                  !state.realm.active()
                )
                  throw new TypeErrorType(
                    "Failed to execute 'read' on 'ReadableStreamDefaultReader': Context is detached",
                  );
                if (state.transferred && name === "TransformStream")
                  return wrap(
                    state.transferred[member],
                    member === "readable" ? "ReadableStream" : "WritableStream",
                  );
                const previous = state.realm.cancelPrefix;
                if (member === "cancel")
                  state.realm.cancelPrefix = "Failed to execute 'cancel' on '" + name + "': ";
                try {
                  return state.realm.result(
                    name,
                    member,
                    apply(native, state.raw, argumentsFor(name, member, args)),
                  );
                } finally {
                  state.realm.cancelPrefix = previous;
                }
              } catch (error) {
                if (["cancel", "pipeTo", "read", "write", "close", "abort"].includes(member))
                  return rejectPromise(localError(error));
                throw localError(error);
              }
            },
            type === "value" ? member : type + " " + member,
            native.length,
          );
        }
      define(ctor.prototype, member, copy);
    }
    define(ctor.prototype, Symbol.toStringTag, { value: name, configurable: true });
    define(ctor, "prototype", { writable: false });
    define(globalThis, name, { value: ctor, writable: true, configurable: true });
  }
  define(constructors.ReadableStream.prototype, Symbol.asyncIterator, {
    value: constructors.ReadableStream.prototype.values,
    writable: true,
    configurable: true,
  });
  const Signal = make(
    function () {
      throw new TypeErrorType("Illegal constructor");
    },
    "AbortSignal",
    0,
    true,
  );
  Object.setPrototypeOf(Signal.prototype, EventTarget.prototype);
  Object.setPrototypeOf(Signal, EventTarget);
  constructors.AbortSignal = Signal;
  function signalFront(raw) {
    const previous = getFront(raw);
    if (previous) return previous;
    const controller = new ControllerBefore(),
      front = apply(oldSignal, controller, []);
    Object.setPrototypeOf(front, Signal.prototype);
    save(front, "AbortSignal", raw);
    const state = record(front, "AbortSignal");
    state.dispatch = (reason) => apply(oldAbort, controller, [localError(reason)]);
    if (apply(backend.signal.aborted.get, raw, []))
      state.dispatch(apply(backend.signal.reason.get, raw, []));
    else {
      const id = ops.op_web_abort_bind(front, state.dispatch, raw, backend.cleanup);
      ops.op_web_abort_listener(id, backend.listen(raw, id));
    }
    return front;
  }
  for (const [name, descriptor] of Object.entries(backend.signal)) {
    if (
      [
        "constructor",
        "onabort",
        "addEventListener",
        "removeEventListener",
        "dispatchEvent",
      ].includes(name)
    )
      continue;
    const copy = { ...descriptor, enumerable: true };
    for (const type of ["get", "value"])
      if (typeof copy[type] === "function") {
        const native = copy[type];
        copy[type] = make(
          function (...args) {
            const raw = unwrap(this, "AbortSignal");
            try {
              return apply(native, raw, args);
            } catch (error) {
              throw localError(error);
            }
          },
          type === "value" ? name : type + " " + name,
          native.length,
        );
      }
    define(Signal.prototype, name, copy);
  }
  const previousReason = Object.getOwnPropertyDescriptor(
    Object.getPrototypeOf(apply(oldSignal, new ControllerBefore(), [])),
    "reason",
  ).get;
  define(Signal.prototype, "reason", {
    get: make(
      function () {
        record(this, "AbortSignal");
        return apply(previousReason, this, []);
      },
      "get reason",
      0,
    ),
    enumerable: true,
    configurable: true,
  });
  define(Signal.prototype, "throwIfAborted", {
    value: make(
      function () {
        if (apply(backend.signal.aborted.get, unwrap(this, "AbortSignal"), []))
          throw apply(previousReason, this, []);
      },
      "throwIfAborted",
      0,
    ),
    enumerable: true,
    configurable: true,
    writable: true,
  });
  const previousOnabort = Object.getOwnPropertyDescriptor(
    Object.getPrototypeOf(apply(oldSignal, new ControllerBefore(), [])),
    "onabort",
  );
  define(Signal.prototype, "onabort", previousOnabort);
  for (const name of ["abort", "any", "timeout"])
    define(Signal, name, {
      value: make(
        function (...args) {
          try {
            if (name === "any") {
              if (!args.length) throw new TypeErrorType("AbortSignal.any requires an argument");
              const input = args[0],
                iterator = input?.[Symbol.iterator];
              if (typeof iterator !== "function")
                throw new TypeErrorType("AbortSignal.any requires an iterable");
              const iterable = create(null);
              define(iterable, Symbol.iterator, { value: () => apply(iterator, input, []) });
              args[0] = Array.from(iterable, (value) => unwrap(value, "AbortSignal"));
            }
            if (name === "abort" && args[0] !== undefined) authored(args[0]);
            if (name === "abort" && args[0] === undefined)
              args[0] = new Exception("signal is aborted without reason", "AbortError");
            return signalFront(
              apply(backend.abort.AbortSignal[name], backend.abort.AbortSignal, args),
            );
          } catch (error) {
            throw localError(error);
          }
        },
        name,
        backend.abort.AbortSignal[name].length,
      ),
      writable: true,
      configurable: true,
      enumerable: true,
    });
  define(Signal.prototype, Symbol.toStringTag, { value: "AbortSignal", configurable: true });
  define(Signal, "prototype", { writable: false });
  const Controller = make(
    function () {
      const raw = new backend.abort.AbortController(),
        front = construct(ControllerBefore, [], new.target);
      save(front, "AbortController", raw);
      record(front, "AbortController").signal = signalFront(
        apply(backend.controller.signal.get, raw, []),
      );
      return front;
    },
    "AbortController",
    0,
    true,
    true,
  );
  define(Controller.prototype, "signal", {
    get: make(
      function () {
        return record(this, "AbortController").signal;
      },
      "get signal",
      0,
    ),
    enumerable: true,
    configurable: true,
  });
  define(Controller.prototype, "abort", {
    value: make(
      function (reason) {
        const raw = unwrap(this, "AbortController");
        if (reason !== undefined) authored(reason);
        return apply(backend.controller.abort.value, raw, [
          reason === undefined
            ? new Exception("signal is aborted without reason", "AbortError")
            : reason,
        ]);
      },
      "abort",
      0,
    ),
    enumerable: true,
    configurable: true,
    writable: true,
  });
  define(Controller.prototype, Symbol.toStringTag, {
    value: "AbortController",
    configurable: true,
  });
  define(Controller, "prototype", { writable: false });
  define(globalThis, "AbortSignal", { value: Signal, writable: true, configurable: true });
  define(globalThis, "AbortController", { value: Controller, writable: true, configurable: true });
  return {
    transferBackend() {
      const streams = backend.streams;
      const transformDescriptors = descriptors(streams.TransformStream.prototype);
      const exceptionDescriptors = descriptors(Exception.prototype);
      return {
        Readable: streams.ReadableStream,
        Writable: streams.WritableStream,
        ReadableController: streams.ReadableStreamDefaultController,
        WritableController: streams.WritableStreamDefaultController,
        wrap,
        cancelFailure(error) {
          if (!realm.cancelPrefix) return error;
          return new Exception(
            realm.cancelPrefix + apply(exceptionDescriptors.message.get, error, []),
            apply(exceptionDescriptors.name.get, error, []),
          );
        },
        state(front) {
          if (front === null || (typeof front !== "object" && typeof front !== "function")) return;
          const state = get(front)?.web;
          if (!state) return;
          if (state.name === "ReadableStream")
            return { kind: 1, raw: state.raw, active: state.realm.active };
          if (state.name === "WritableStream")
            return { kind: 2, raw: state.raw, active: state.realm.active };
          if (state.name !== "TransformStream") return;
          return {
            kind: 3,
            active: state.realm.active,
            readable:
              state.transferred?.readable ??
              apply(transformDescriptors.readable.get, state.raw, []),
            writable:
              state.transferred?.writable ??
              apply(transformDescriptors.writable.get, state.raw, []),
          };
        },
        transform(readable, writable) {
          const front = save(
            create(constructors.TransformStream.prototype),
            "TransformStream",
            create(null),
          );
          record(front, "TransformStream").transferred = { readable, writable };
          return front;
        },
      };
    },
    dependent(signal) {
      return signalFront(
        backend.abort.createDependentAbortSignal(
          signal === null ? [] : [unwrap(signal, "AbortSignal")],
          "Request signal",
        ),
      );
    },
    readable(stream) {
      return (
        stream !== null &&
        (typeof stream === "object" || typeof stream === "function") &&
        get(stream)?.web?.name === "ReadableStream"
      );
    },
    disturbed(stream) {
      return backend.streams.isReadableStreamDisturbed(unwrap(stream, "ReadableStream"));
    },
  };
});
