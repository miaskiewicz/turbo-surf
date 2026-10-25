// Shared Web IDL plumbing for runtime-implemented EventTarget interfaces (#28): EventHandler
// attributes and trusted engine dispatch. Any object is stored as a handler and other values
// become null; the handler's listener keeps the position it had when last set from null.
const __handlerStates = new WeakMap(),
  __handlerGet = WeakMap.prototype.get,
  __handlerSet = WeakMap.prototype.set;
const __handlerAdd = EventTarget.prototype.addEventListener,
  __handlerRemove = EventTarget.prototype.removeEventListener;
const __defineEventHandler = (prototype, name, check) => {
  const type = name.slice(2),
    make = __G.makeNativeFunction,
    apply = Reflect.apply;
  Object.defineProperty(prototype, name, {
    __proto__: null,
    get: make(
      function () {
        check(this);
        const row = apply(__handlerGet, __handlerStates, [this]);
        return (row && row[name] && row[name].value) ?? null;
      },
      "get " + name,
      0,
    ),
    set: make(
      function (value) {
        check(this);
        value =
          value !== null && (typeof value === "object" || typeof value === "function")
            ? value
            : null;
        let row = apply(__handlerGet, __handlerStates, [this]);
        if (!row) apply(__handlerSet, __handlerStates, [this, (row = { __proto__: null })]);
        let entry = row[name];
        if (!entry) {
          entry = row[name] = {
            __proto__: null,
            value: null,
            listener: make(
              function (event) {
                const callback = entry.value;
                if (typeof callback !== "function") return;
                if (apply(callback, this, [event]) === false) event.preventDefault();
              },
              name,
              1,
            ),
          };
        }
        if (value && !entry.value) apply(__handlerAdd, this, [type, entry.listener]);
        if (!value && entry.value) apply(__handlerRemove, this, [type, entry.listener]);
        entry.value = value;
      },
      "set " + name,
      1,
    ),
    enumerable: true,
    configurable: true,
  });
};
// Engine-originated events are trusted and dispatched through the host path.
const __dispatchTrusted = (target, event) => {
  ops.op_event_trust(event, -1);
  ops.op_dispatch_host_event(event, target);
  return event;
};
// An interface object with Chrome's shape: native constructor (illegal unless `construct`),
// read-only prototype, a tag, an optional parent interface, and a non-enumerable global.
const __defineInterface = (name, length, parent, construct) => {
  const make = __G.makeNativeFunction;
  const constructor = make(
    construct ||
      function () {
        throw new __exceptionTypeError("Illegal constructor");
      },
    name,
    length,
    true,
    !!construct,
  );
  const prototype = constructor.prototype;
  if (parent) {
    Object.setPrototypeOf(prototype, parent.prototype);
    Object.setPrototypeOf(constructor, parent);
  }
  Object.defineProperty(prototype, Symbol.toStringTag, {
    __proto__: null,
    value: name,
    configurable: true,
  });
  Object.defineProperty(constructor, "prototype", { __proto__: null, writable: false });
  Object.defineProperty(globalThis, name, {
    __proto__: null,
    value: constructor,
    writable: true,
    configurable: true,
    enumerable: false,
  });
  return constructor;
};
const __defineConstants = (constructor, constants) => {
  // Configurable so the binding's key-order step can move these Web IDL constants
  // after the interface's attributes (Blink's order) and re-add them non-configurable,
  // as it does for the binding's own constants. They are frozen before any guest runs.
  for (const [name, value] of constants)
    for (const object of [constructor, constructor.prototype])
      Object.defineProperty(object, name, {
        __proto__: null,
        value,
        writable: false,
        enumerable: true,
        configurable: true,
      });
};
