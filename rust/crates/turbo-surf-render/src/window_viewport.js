// Screen, ScreenOrientation and VisualViewport (#28, measured in
// harness/detection/viewport-orientation-probe.js). All three are EventTargets with branded
// accessors. The visual viewport is the unpinched layout viewport: no pinch zoom exists here,
// so offsets stay 0, scale 1, pageLeft/pageTop follow the document scroll, and its own
// resize/scroll/scrollend events never fire because the viewport never resizes or pans.
// Orientation lock is unsupported on this desktop device, as Chrome reports it.
(() => {
  "use strict";
  const define = Object.defineProperty,
    apply = Reflect.apply,
    make = __G.makeNativeFunction;
  const ErrorType = __exceptionTypeError,
    Exception = DOMException,
    PromiseValue = Promise;
  const reject = Promise.reject,
    indexOf = Array.prototype.indexOf,
    setPrototype = Object.setPrototypeOf,
    StringValue = String;
  const Target = globalThis.EventTarget,
    targetPrototype = Target.prototype;
  const addListener = targetPrototype.addEventListener,
    removeListener = targetPrototype.removeEventListener;
  const realm = globalThis;
  const documentElement = Object.getOwnPropertyDescriptor(
    Document.prototype,
    "documentElement",
  ).get;
  const interfaceObject = (name) => {
    const constructor = make(
      function () {
        throw new ErrorType("Illegal constructor");
      },
      name,
      0,
      true,
    );
    const prototype = constructor.prototype;
    setPrototype(prototype, targetPrototype);
    setPrototype(constructor, Target);
    define(prototype, Symbol.toStringTag, { __proto__: null, value: name, configurable: true });
    define(constructor, "prototype", { __proto__: null, writable: false });
    define(globalThis, name, {
      __proto__: null,
      value: constructor,
      writable: true,
      configurable: true,
      enumerable: false,
    });
    return prototype;
  };
  const accessor = (object, name, get, set) =>
    define(object, name, {
      __proto__: null,
      get: make(get, "get " + name, 0),
      set: set ? make(set, "set " + name, 1) : undefined,
      enumerable: true,
      configurable: true,
    });
  const instance = (prototype) => {
    const object = new Target();
    setPrototype(object, prototype);
    return object;
  };
  const brand = (object) => (receiver) => {
    if (receiver !== object) throw new ErrorType("Illegal invocation");
  };
  // EventHandler attributes: any object is stored, other values become null; the handler's
  // listener sits at the position it had when last set from null.
  const handlers = new WeakMap(),
    handlerGet = WeakMap.prototype.get,
    handlerSet = WeakMap.prototype.set;
  const handlerAttribute = (prototype, name, check) => {
    const type = name.slice(2);
    accessor(
      prototype,
      name,
      function () {
        check(this);
        const row = apply(handlerGet, handlers, [this]);
        return (row && row[name] && row[name].value) ?? null;
      },
      function (value) {
        check(this);
        value =
          value !== null && (typeof value === "object" || typeof value === "function")
            ? value
            : null;
        let row = apply(handlerGet, handlers, [this]);
        if (!row) apply(handlerSet, handlers, [this, (row = { __proto__: null })]);
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
        if (value && !entry.value) apply(addListener, this, [type, entry.listener]);
        if (!value && entry.value) apply(removeListener, this, [type, entry.listener]);
        entry.value = value;
      },
    );
  };
  // [Replaceable] window attributes: assignment shadows the accessor with own data.
  const replaceable = (name, value) =>
    define(realm, name, {
      __proto__: null,
      get: make(
        function () {
          return value;
        },
        "get " + name,
        0,
      ),
      set: make(
        function (next) {
          define(realm, name, {
            __proto__: null,
            value: next,
            writable: true,
            enumerable: true,
            configurable: true,
          });
        },
        "set " + name,
        1,
      ),
      enumerable: true,
      configurable: true,
    });

  // ---- Screen and ScreenOrientation -----------------------------------------------------
  const source = globalThis.screen || {};
  const number = (value, fallback) => (typeof value === "number" ? value : fallback);
  const screenValues = {
    __proto__: null,
    width: number(source.width, 1920),
    height: number(source.height, 1080),
    availWidth: number(source.availWidth, 1920),
    availHeight: number(source.availHeight, 1080),
    colorDepth: number(source.colorDepth, 24),
    pixelDepth: number(source.pixelDepth, 24),
    availLeft: number(source.availLeft, 0),
    availTop: number(source.availTop, 0),
  };
  const orientationType =
    screenValues.width >= screenValues.height ? "landscape-primary" : "portrait-primary";
  const ScreenPrototype = interfaceObject("Screen");
  const OrientationPrototype = interfaceObject("ScreenOrientation");
  const screen = instance(ScreenPrototype);
  const orientation = instance(OrientationPrototype);
  const checkScreen = brand(screen),
    checkOrientation = brand(orientation);
  for (const name of [
    "availWidth",
    "availHeight",
    "width",
    "height",
    "colorDepth",
    "pixelDepth",
    "availLeft",
    "availTop",
  ])
    accessor(ScreenPrototype, name, function () {
      checkScreen(this);
      return screenValues[name];
    });
  accessor(ScreenPrototype, "orientation", function () {
    checkScreen(this);
    return orientation;
  });
  handlerAttribute(ScreenPrototype, "onchange", checkScreen);
  // One screen: the engine models no secondary display.
  accessor(ScreenPrototype, "isExtended", function () {
    checkScreen(this);
    return false;
  });
  accessor(OrientationPrototype, "type", function () {
    checkOrientation(this);
    return orientationType;
  });
  accessor(OrientationPrototype, "angle", function () {
    checkOrientation(this);
    return 0;
  });
  handlerAttribute(OrientationPrototype, "onchange", checkOrientation);
  const lockTypes = [
    "any",
    "natural",
    "landscape",
    "portrait",
    "portrait-primary",
    "portrait-secondary",
    "landscape-primary",
    "landscape-secondary",
  ];
  const rejected = (error) => apply(reject, PromiseValue, [error]);
  define(OrientationPrototype, "lock", {
    __proto__: null,
    value: make(
      function (type) {
        // A promise-returning operation reports its argument errors as rejections.
        if (this !== orientation)
          return rejected(
            new ErrorType("Failed to execute 'lock' on 'ScreenOrientation': Illegal invocation"),
          );
        if (!arguments.length)
          return rejected(
            new ErrorType(
              "Failed to execute 'lock' on 'ScreenOrientation': 1 argument required, but only 0 present.",
            ),
          );
        let value;
        try {
          if (typeof type === "symbol") throw new ErrorType("Cannot convert a Symbol to a string");
          value = StringValue(type);
        } catch (error) {
          return rejected(error);
        }
        if (apply(indexOf, lockTypes, [value]) < 0)
          return rejected(
            new ErrorType(
              `Failed to execute 'lock' on 'ScreenOrientation': The provided value '${value}' is not a valid enum value of type OrientationLockType.`,
            ),
          );
        return rejected(
          new Exception(
            "screen.orientation.lock() is not available on this device.",
            "NotSupportedError",
          ),
        );
      },
      "lock",
      1,
    ),
    writable: true,
    enumerable: true,
    configurable: true,
  });
  define(OrientationPrototype, "unlock", {
    __proto__: null,
    value: make(
      function () {
        checkOrientation(this);
      },
      "unlock",
      0,
    ),
    writable: true,
    enumerable: true,
    configurable: true,
  });
  replaceable("screen", screen);

  // ---- VisualViewport -------------------------------------------------------------------
  const ViewportPrototype = interfaceObject("VisualViewport");
  const viewport = instance(ViewportPrototype);
  const checkViewport = brand(viewport);
  const fallbackSize = [number(globalThis.innerWidth, 1280), number(globalThis.innerHeight, 800)];
  const root = () => apply(documentElement, __document, []);
  // The layout viewport's scroll position, as Window#scrollX/scrollY read it.
  const scroll = (axis) => {
    const element = root();
    if (element === null) return 0;
    try {
      return ops.op_live_element_scroll(element, 0, 0)[axis ? "top" : "left"];
    } catch {
      return ops.op_live_viewport_offset(realm)[axis];
    }
  };
  // The layout viewport's size (this engine draws no classic scrollbars to exclude).
  const size = (axis) => {
    try {
      const value = ops.op_live_frame_dimension(realm, axis === 1, true);
      if (typeof value === "number") return value;
    } catch {}
    return fallbackSize[axis];
  };
  for (const [name, read] of [
    ["offsetLeft", () => 0],
    ["offsetTop", () => 0],
    ["pageLeft", () => scroll(0)],
    ["pageTop", () => scroll(1)],
    ["width", () => size(0)],
    ["height", () => size(1)],
    ["scale", () => 1],
  ])
    accessor(ViewportPrototype, name, function () {
      checkViewport(this);
      return read();
    });
  for (const name of ["onresize", "onscroll", "onscrollend"])
    handlerAttribute(ViewportPrototype, name, checkViewport);
  replaceable("visualViewport", viewport);
})();
