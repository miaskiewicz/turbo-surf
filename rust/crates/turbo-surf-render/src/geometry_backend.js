// Private, native-branded rectangle data; no guest-visible host registry.
(function (core, make, frameWindow, frameStep) {
  "use strict";
  const ops = core.ops;
  const getRecord = ops.op_interface_record_get;
  const setRecord = ops.op_interface_record_set;
  const define = Object.defineProperty;
  const setPrototype = Object.setPrototypeOf;
  const create = Object.create;
  const LocalTypeError = TypeError;
  const LocalDOMException = DOMException;
  const apply = Reflect.apply;
  const isPrototypeOf = Object.prototype.isPrototypeOf;
  const localTypeErrorPrototype = LocalTypeError.prototype;
  const nativeTypeErrorPrototype = Object.getPrototypeOf(core.buildCustomError("TypeError", ""));
  const minimum = Math.min;
  const maximum = Math.max;
  const finite = Number.isFinite;
  const single = Math.fround;
  const prototypes = create(null);

  function frameDimension(receiver, height, read) {
    let value;
    try {
      value = ops.op_live_frame_dimension(receiver, height, read);
    } catch (error) {
      if (
        apply(isPrototypeOf, nativeTypeErrorPrototype, [error]) &&
        !apply(isPrototypeOf, localTypeErrorPrototype, [error])
      )
        throw new LocalTypeError("Illegal invocation");
      throw error;
    }
    if (value === null) throw new LocalDOMException("Cross-origin Window access", "SecurityError");
    return value;
  }

  if (frameWindow) {
    for (const [name, height] of [
      ["innerWidth", false],
      ["innerHeight", true],
    ]) {
      define(globalThis, name, {
        __proto__: null,
        get: make(
          function () {
            return frameDimension(this, height, true);
          },
          `get ${name}`,
          0,
        ),
        set: make(
          function (value) {
            frameDimension(this, height, false);
            define(this, name, {
              __proto__: null,
              value,
              writable: true,
              enumerable: true,
              configurable: true,
            });
          },
          `set ${name}`,
          1,
        ),
        configurable: true,
        enumerable: true,
      });
    }
  }

  function brand(receiver, kind) {
    if (!ops.op_live_geometry_brand(receiver, kind)) throw new LocalTypeError("Illegal invocation");
  }
  function state(receiver, writable = false) {
    if (receiver === null || (typeof receiver !== "object" && typeof receiver !== "function"))
      throw new LocalTypeError("Illegal invocation");
    const record = getRecord(receiver);
    if (
      !record ||
      (record.kind !== "DOMRect" && record.kind !== "DOMRectReadOnly") ||
      (writable && record.kind !== "DOMRect")
    )
      throw new LocalTypeError("Illegal invocation");
    return record;
  }
  function coordinate(value, name) {
    const result = number(value);
    if (!finite(result))
      throw new LocalTypeError(
        `Failed to execute '${name}' on 'Document': The provided double value is non-finite.`,
      );
    return result;
  }
  function floatCoordinate(value, name) {
    const result = single(number(value));
    if (!finite(result))
      throw new LocalTypeError(
        `Failed to execute '${name}' on 'Document': The provided float value is non-finite.`,
      );
    return result;
  }
  function number(value) {
    // Engine ToNumber rejects even a BigInt returned by valueOf/toPrimitive.
    return +value;
  }
  function object(kind, prototype, x, y, width, height) {
    const result = ops.op_geometry_object(prototype);
    const record = { __proto__: null, kind, x, y, width, height };
    if (!setRecord(result, record))
      throw new LocalTypeError("Geometry record installation interrupted");
    return result;
  }
  function construct(kind, target, x, y, width, height) {
    if (!target)
      throw new LocalTypeError(`Failed to construct '${kind}': Please use the 'new' operator`);
    const prototype = target.prototype;
    return object(
      kind,
      typeof prototype === "object" && prototype !== null ? prototype : prototypes[kind],
      x === undefined ? 0 : number(x),
      y === undefined ? 0 : number(y),
      width === undefined ? 0 : number(width),
      height === undefined ? 0 : number(height),
    );
  }
  const DOMRectReadOnly = make(
    function (x, y, width, height) {
      return construct("DOMRectReadOnly", new.target, x, y, width, height);
    },
    "DOMRectReadOnly",
    0,
    true,
    true,
  );
  const DOMRect = make(
    function (x, y, width, height) {
      return construct("DOMRect", new.target, x, y, width, height);
    },
    "DOMRect",
    0,
    true,
    true,
  );
  for (const [kind, constructor, parent] of [
    ["DOMRectReadOnly", DOMRectReadOnly, Object.prototype],
    ["DOMRect", DOMRect, DOMRectReadOnly.prototype],
  ]) {
    const prototype = create(parent);
    prototypes[kind] = prototype;
    define(constructor, "prototype", { __proto__: null, value: prototype, writable: false });
    define(prototype, "constructor", {
      __proto__: null,
      value: constructor,
      writable: true,
      configurable: true,
    });
    define(prototype, Symbol.toStringTag, { __proto__: null, value: kind, configurable: true });
    define(globalThis, kind, {
      __proto__: null,
      value: constructor,
      writable: true,
      enumerable: false,
      configurable: true,
    });
  }
  // The child prototype must inherit the final readonly prototype, after both
  // factories have replaced their temporary default prototype objects.
  setPrototype(DOMRect.prototype, DOMRectReadOnly.prototype);
  setPrototype(DOMRect, DOMRectReadOnly);
  for (const name of ["x", "y", "width", "height"]) {
    const read = make(
      function () {
        return state(this)[name];
      },
      `get ${name}`,
      0,
    );
    define(DOMRectReadOnly.prototype, name, {
      __proto__: null,
      get: read,
      enumerable: true,
      configurable: true,
    });
    define(DOMRect.prototype, name, {
      __proto__: null,
      get: read,
      set: make(
        function (value) {
          state(this, true)[name] = number(value);
        },
        `set ${name}`,
        1,
      ),
      enumerable: true,
      configurable: true,
    });
  }
  const edges = {
    top: (record) => minimum(record.y, record.y + record.height),
    right: (record) => maximum(record.x, record.x + record.width),
    bottom: (record) => maximum(record.y, record.y + record.height),
    left: (record) => minimum(record.x, record.x + record.width),
  };
  for (const name of ["top", "right", "bottom", "left"])
    define(DOMRectReadOnly.prototype, name, {
      __proto__: null,
      get: make(
        function () {
          return edges[name](state(this));
        },
        `get ${name}`,
        0,
      ),
      enumerable: true,
      configurable: true,
    });
  define(DOMRectReadOnly.prototype, "toJSON", {
    __proto__: null,
    value: make(
      function () {
        const record = state(this);
        return {
          x: record.x,
          y: record.y,
          width: record.width,
          height: record.height,
          top: edges.top(record),
          right: edges.right(record),
          bottom: edges.bottom(record),
          left: edges.left(record),
        };
      },
      "toJSON",
      0,
    ),
    enumerable: true,
    writable: true,
    configurable: true,
  });
  for (const [kind, constructor] of [
    ["DOMRectReadOnly", DOMRectReadOnly],
    ["DOMRect", DOMRect],
  ])
    define(constructor, "fromRect", {
      __proto__: null,
      value: make(
        function (input) {
          if (
            input !== undefined &&
            input !== null &&
            typeof input !== "object" &&
            typeof input !== "function"
          )
            throw new LocalTypeError("The provided value is not of type 'DOMRectInit'");
          const dictionary = input ?? create(null);
          // Web IDL dictionaries convert members in lexicographic order.
          const member = (name) => {
            const value = dictionary[name];
            return value === undefined ? 0 : number(value);
          };
          const height = member("height"),
            width = member("width"),
            x = member("x"),
            y = member("y");
          return object(kind, prototypes[kind], x, y, width, height);
        },
        "fromRect",
        0,
      ),
      enumerable: true,
      writable: true,
      configurable: true,
    });
  for (const constructor of [DOMRectReadOnly, DOMRect]) {
    delete constructor.prototype.constructor;
    define(constructor.prototype, "constructor", {
      value: constructor,
      writable: true,
      configurable: true,
    });
  }
  const rectListPrototype = create(Object.prototype);
  const arrayValues = Array.prototype.values;
  const DOMRectList = make(
    function () {
      throw new LocalTypeError("Illegal constructor");
    },
    "DOMRectList",
    0,
    true,
    true,
  );
  DOMRectList.prototype = rectListPrototype;
  define(rectListPrototype, Symbol.toStringTag, {
    __proto__: null,
    value: "DOMRectList",
    configurable: true,
  });
  function listState(receiver) {
    const record = getRecord(receiver);
    if (!record || record.kind !== "DOMRectList") throw new LocalTypeError("Illegal invocation");
    return record;
  }
  define(rectListPrototype, "length", {
    __proto__: null,
    get: make(
      function () {
        return listState(this).items.length;
      },
      "get length",
      0,
    ),
    enumerable: true,
    configurable: true,
  });
  define(rectListPrototype, "item", {
    __proto__: null,
    value: make(
      function (index) {
        const record = listState(this);
        if (arguments.length === 0)
          throw new LocalTypeError("1 argument required, but only 0 present.");
        return record.items[number(index) >>> 0] || null;
      },
      "item",
      1,
    ),
    writable: true,
    enumerable: true,
    configurable: true,
  });
  define(rectListPrototype, "constructor", {
    __proto__: null,
    value: DOMRectList,
    writable: true,
    configurable: true,
  });
  define(rectListPrototype, Symbol.iterator, {
    __proto__: null,
    value: make(
      function values() {
        listState(this);
        return apply(arrayValues, this, []);
      },
      "values",
      0,
    ),
    writable: true,
    configurable: true,
  });
  define(DOMRectList, "prototype", { __proto__: null, writable: false });
  define(globalThis, "DOMRectList", {
    __proto__: null,
    value: DOMRectList,
    writable: true,
    enumerable: false,
    configurable: true,
  });
  function rectangle(values) {
    return object("DOMRect", prototypes.DOMRect, values[0], values[1], values[2], values[3]);
  }
  // Observer entries (observers.js) report DOMRectReadOnly values built here.
  define(globalThis[Symbol.for("ts")], "readOnlyRect", {
    __proto__: null,
    value: (x, y, width, height) =>
      object("DOMRectReadOnly", prototypes.DOMRectReadOnly, x, y, width, height),
    configurable: true,
  });
  function rectList(rects) {
    const result = ops.op_geometry_object(rectListPrototype),
      items = [];
    setPrototype(items, null);
    for (let i = 0; i < rects.length; i++) {
      define(items, i, {
        __proto__: null,
        value: rectangle(rects[i]),
        writable: true,
        enumerable: true,
        configurable: true,
      });
      define(result, i, {
        __proto__: null,
        get: make(
          function () {
            return listState(this).items[i];
          },
          "get " + i,
          0,
        ),
        enumerable: true,
        configurable: true,
      });
    }
    if (!setRecord(result, { __proto__: null, kind: "DOMRectList", items }))
      throw new LocalTypeError("Geometry list record installation interrupted");
    return result;
  }
  define(Element.prototype, "getBoundingClientRect", {
    __proto__: null,
    value: make(
      function () {
        brand(this, 1);
        return rectangle(ops.op_live_element_geometry(this).bounds);
      },
      "getBoundingClientRect",
      0,
    ),
    writable: true,
    enumerable: true,
    configurable: true,
  });
  define(Element.prototype, "getClientRects", {
    __proto__: null,
    value: make(
      function () {
        brand(this, 1);
        return rectList(ops.op_live_element_geometry(this).rects);
      },
      "getClientRects",
      0,
    ),
    writable: true,
    enumerable: true,
    configurable: true,
  });
  for (const [name, group, field, prototype] of [
    ["clientLeft", "client", "left", Element.prototype],
    ["clientTop", "client", "top", Element.prototype],
    ["clientWidth", "client", "width", Element.prototype],
    ["clientHeight", "client", "height", Element.prototype],
    ["offsetWidth", "offset", "width", HTMLElement.prototype],
    ["offsetHeight", "offset", "height", HTMLElement.prototype],
  ])
    define(prototype, name, {
      __proto__: null,
      get: make(
        function () {
          brand(this, group === "offset" ? 2 : 1);
          return ops.op_live_element_geometry(this)[group][field];
        },
        "get " + name,
        0,
      ),
      enumerable: true,
      configurable: true,
    });
  // Scrolling methods follow Chrome156: Element/Window scroll operations return a
  // promise resolved with a fresh ScrollResult after an instant scroll's task.
  const LocalPromise = Promise;
  const promiseReject = Promise.reject;
  const toPrimitiveKey = Symbol.toPrimitive;
  const LocalString = String;
  const includes = Array.prototype.includes;
  const push = Array.prototype.push;
  const splice = Array.prototype.splice;
  const indexOf = Array.prototype.indexOf;
  const HostEvent = Event;
  const hostTimeout = setTimeout;
  const ownerDocument = Object.getOwnPropertyDescriptor(Node.prototype, "ownerDocument").get;
  const documentElement = Object.getOwnPropertyDescriptor(
    Document.prototype,
    "documentElement",
  ).get;
  const windowDocument = Object.getOwnPropertyDescriptor(globalThis, "document");
  const realmWindow = globalThis;
  function currentDocument() {
    return windowDocument && windowDocument.get
      ? apply(windowDocument.get, realmWindow, [])
      : realmWindow.document;
  }
  function task(callback) {
    apply(hostTimeout, realmWindow, [callback, 0]);
  }
  function scrollResult(interrupted) {
    return { interrupted };
  }
  function settled(interrupted = false) {
    let resolve;
    const promise = new LocalPromise((done) => {
      resolve = done;
    });
    task(() => resolve(scrollResult(interrupted)));
    return promise;
  }
  // Instant engine conversion failures carry the binding prefix; author-thrown
  // exceptions from valueOf/toString/getters propagate unchanged.
  function primitive(value, hint, prefix) {
    if (value === null || (typeof value !== "object" && typeof value !== "function")) return value;
    const exotic = value[toPrimitiveKey];
    if (exotic !== undefined && exotic !== null) {
      if (typeof exotic !== "function")
        throw new LocalTypeError(prefix + "Cannot convert object to primitive value");
      const result = apply(exotic, value, [hint]);
      if (result !== null && (typeof result === "object" || typeof result === "function"))
        throw new LocalTypeError(prefix + "Cannot convert object to primitive value");
      return result;
    }
    const order = hint === "string" ? ["toString", "valueOf"] : ["valueOf", "toString"];
    for (let i = 0; i < 2; i++) {
      const method = value[order[i]];
      if (typeof method !== "function") continue;
      const result = apply(method, value, []);
      if (result === null || (typeof result !== "object" && typeof result !== "function"))
        return result;
    }
    throw new LocalTypeError(prefix + "Cannot convert object to primitive value");
  }
  function idlNumber(value, prefix) {
    const result = primitive(value, "number", prefix);
    if (typeof result === "bigint")
      throw new LocalTypeError(prefix + "Cannot convert a BigInt value to a number");
    if (typeof result === "symbol")
      throw new LocalTypeError(prefix + "Cannot convert a Symbol value to a number");
    return +result;
  }
  function idlString(value, prefix) {
    const result = primitive(value, "string", prefix);
    if (typeof result === "symbol")
      throw new LocalTypeError(prefix + "Cannot convert a Symbol value to a string");
    return LocalString(result);
  }
  // Unrestricted doubles are normalized after conversion: non-finite becomes 0.
  function normalized(value, prefix) {
    const result = idlNumber(value, prefix);
    return finite(result) ? result : 0;
  }
  function dictionary(value, prefix, type) {
    if (value === undefined || value === null) return create(null);
    if (typeof value !== "object" && typeof value !== "function")
      throw new LocalTypeError(`${prefix}The provided value is not of type '${type}'.`);
    return value;
  }
  function enumeration(options, member, owner, type, values, fallback, prefix) {
    const value = options[member];
    if (value === undefined) return fallback;
    const read = `${prefix}Failed to read the '${member}' property from '${owner}': `;
    const result = idlString(value, read);
    if (!apply(includes, values, [result]))
      throw new LocalTypeError(
        `${read}The provided value '${result}' is not a valid enum value of type ${type}.`,
      );
    return result;
  }
  const scrollBehaviors = ["auto", "instant", "smooth"];
  function scrollArguments(args, prefix) {
    if (args.length >= 2)
      return { behavior: "auto", x: normalized(args[0], prefix), y: normalized(args[1], prefix) };
    const options = dictionary(args[0], prefix, "ScrollToOptions");
    const behavior = enumeration(
      options,
      "behavior",
      "ScrollOptions",
      "ScrollBehavior",
      scrollBehaviors,
      "auto",
      prefix,
    );
    const member = (name) => {
      const value = options[name];
      return value === undefined
        ? NaN
        : normalized(
            value,
            `${prefix}Failed to read the '${name}' property from 'ScrollToOptions': `,
          );
    };
    const x = member("left");
    return { behavior, x, y: member("top") };
  }

  // Scroll and scrollend for instant programmatic scrolls are queued per scrolling
  // box and delivered, in first-scroll order, before the next frame's callbacks.
  const pendingTargets = [];
  const pendingBubbles = [];
  let pendingFrame = false;
  function frame(step) {
    if (typeof frameStep === "function") frameStep(step);
    else apply(hostTimeout, realmWindow, [step, 16]);
  }
  function fire(target, type, bubbles) {
    const event = new HostEvent(type, { __proto__: null, bubbles });
    ops.op_event_trust(event, -1);
    ops.op_dispatch_host_event(event, target);
  }
  function deliverScrolls() {
    pendingFrame = false;
    const targets = apply(splice, pendingTargets, [0]);
    const bubbles = apply(splice, pendingBubbles, [0]);
    for (let i = 0; i < targets.length; i++) {
      fire(targets[i], "scroll", bubbles[i]);
      fire(targets[i], "scrollend", bubbles[i]);
    }
  }
  // Element scrolling boxes target the element; the viewport targets its document.
  function notify(target, viewport = false) {
    if (!apply(includes, pendingTargets, [target])) {
      apply(push, pendingTargets, [target]);
      apply(push, pendingBubbles, [viewport]);
    }
    if (pendingFrame) return;
    pendingFrame = true;
    frame(deliverScrolls);
  }
  function scrolled(element, result) {
    if (result.changed === 1) notify(element);
    else if (result.changed === 2) notify(apply(ownerDocument, element, []), true);
  }

  // A newer scroll of the same box interrupts its pending smooth scroll.
  const smooth = [];
  function boxKey(element) {
    const document = apply(ownerDocument, element, []);
    return element === apply(documentElement, document, []) ? document : element;
  }
  function interrupt(element) {
    const key = boxKey(element);
    for (let i = 0; i < smooth.length; i++)
      if (smooth[i].key === key) {
        const entry = apply(splice, smooth, [i, 1])[0];
        task(() => entry.resolve(scrollResult(true)));
        return;
      }
  }
  // Smooth scrolls complete at the next rendering frame; intermediate animation
  // positions are not modeled.
  function deferred(element, perform, key) {
    let resolve;
    const promise = new LocalPromise((done) => {
      resolve = done;
    });
    const entry = { key, resolve };
    if (key !== null) apply(push, smooth, [entry]);
    frame(() => {
      if (key !== null) {
        const index = apply(indexOf, smooth, [entry]);
        if (index < 0) return;
        apply(splice, smooth, [index, 1]);
      }
      perform();
      deliverScrolls();
      task(() => resolve(scrollResult(false)));
    });
    return promise;
  }
  function scrollElement(element, request, relative) {
    interrupt(element);
    let { x, y } = request;
    if (request.behavior === "smooth") {
      if (relative) {
        const current = ops.op_live_element_scroll(element, 0, 0);
        x = x !== x ? x : current.left + x;
        y = y !== y ? y : current.top + y;
      }
      return deferred(
        element,
        () => scrolled(element, ops.op_live_element_scroll_to(element, x, y, false)),
        boxKey(element),
      );
    }
    scrolled(element, ops.op_live_element_scroll_to(element, x, y, relative));
    return settled();
  }
  function rejectWith(error) {
    return apply(promiseReject, LocalPromise, [error]);
  }
  for (const [name, relative] of [
    ["scroll", false],
    ["scrollTo", false],
    ["scrollBy", true],
  ]) {
    define(Element.prototype, name, {
      __proto__: null,
      value: make(
        function () {
          const prefix = `Failed to execute '${name}' on 'Element': `;
          try {
            if (!ops.op_live_geometry_brand(this, 1))
              throw new LocalTypeError(prefix + "Illegal invocation");
            return scrollElement(this, scrollArguments(arguments, prefix), relative);
          } catch (error) {
            return rejectWith(error);
          }
        },
        name,
        0,
      ),
      writable: true,
      enumerable: true,
      configurable: true,
    });
    define(realmWindow, name, {
      __proto__: null,
      value: make(
        function () {
          const prefix = `Failed to execute '${name}' on 'Window': `;
          try {
            if (this !== undefined && this !== null && this !== realmWindow)
              throw new LocalTypeError(prefix + "Illegal invocation");
            const request = scrollArguments(arguments, prefix);
            const root = apply(documentElement, currentDocument(), []);
            return root === null ? settled() : scrollElement(root, request, relative);
          } catch (error) {
            return rejectWith(error);
          }
        },
        name,
        0,
      ),
      writable: true,
      enumerable: true,
      configurable: true,
    });
  }
  for (const [name, field] of [
    ["scrollX", "left"],
    ["scrollY", "top"],
    ["pageXOffset", "left"],
    ["pageYOffset", "top"],
  ])
    define(realmWindow, name, {
      __proto__: null,
      get: make(
        function () {
          if (this !== undefined && this !== null && this !== realmWindow)
            throw new LocalTypeError("Illegal invocation");
          const root = apply(documentElement, currentDocument(), []);
          if (root === null) return 0;
          try {
            return ops.op_live_element_scroll(root, 0, 0)[field];
          } catch {
            // Window offsets never throw: unsupported layout keeps the last
            // clamped position instead of re-clamping against current content.
            return ops.op_live_viewport_offset(realmWindow)[field === "left" ? 0 : 1];
          }
        },
        `get ${name}`,
        0,
      ),
      set: make(
        function (value) {
          if (this !== undefined && this !== null && this !== realmWindow)
            throw new LocalTypeError("Illegal invocation");
          // [Replaceable]: the assignment shadows the accessor with own data.
          define(realmWindow, name, {
            __proto__: null,
            value,
            writable: true,
            enumerable: true,
            configurable: true,
          });
        },
        `set ${name}`,
        1,
      ),
      enumerable: true,
      configurable: true,
    });

  const logicalPositions = ["start", "center", "end", "nearest"];
  const scrollContainers = ["all", "nearest"];
  function intoView(element, block, inline, mode, nearest) {
    const changed = ops.op_live_scroll_into_view(element, block, inline, mode, nearest);
    for (let i = 0; i < changed.elements.length; i++) notify(changed.elements[i]);
    if (changed.viewport) notify(apply(ownerDocument, element, []), true);
  }
  define(Element.prototype, "scrollIntoView", {
    __proto__: null,
    value: make(
      function (arg) {
        const prefix = "Failed to execute 'scrollIntoView' on 'Element': ";
        try {
          if (!ops.op_live_geometry_brand(this, 1))
            throw new LocalTypeError(prefix + "Illegal invocation");
          let behavior = "auto",
            block = "start",
            inline = "nearest",
            container = "all";
          if (arg !== null && (typeof arg === "object" || typeof arg === "function")) {
            const read = (member, owner, type, values, fallback) =>
              enumeration(arg, member, owner, type, values, fallback, prefix);
            behavior = read("behavior", "ScrollOptions", "ScrollBehavior", scrollBehaviors, "auto");
            block = read(
              "block",
              "ScrollIntoViewOptions",
              "ScrollLogicalPosition",
              logicalPositions,
              "start",
            );
            container = read(
              "container",
              "ScrollIntoViewOptions",
              "ScrollContainer",
              scrollContainers,
              "all",
            );
            inline = read(
              "inline",
              "ScrollIntoViewOptions",
              "ScrollLogicalPosition",
              logicalPositions,
              "nearest",
            );
          } else if (arg !== undefined && arg !== null && !arg) block = "end";
          const element = this;
          const perform = () =>
            intoView(
              element,
              apply(indexOf, logicalPositions, [block]),
              apply(indexOf, logicalPositions, [inline]),
              0,
              container === "nearest",
            );
          if (behavior === "smooth") return deferred(element, perform, null);
          perform();
          return settled();
        } catch (error) {
          return rejectWith(error);
        }
      },
      "scrollIntoView",
      0,
    ),
    writable: true,
    enumerable: true,
    configurable: true,
  });
  define(Element.prototype, "scrollIntoViewIfNeeded", {
    __proto__: null,
    value: make(
      function (centerIfNeeded) {
        brand(this, 1);
        const center = centerIfNeeded === undefined ? true : !!centerIfNeeded;
        intoView(this, 3, 3, center ? 1 : 2, false);
      },
      "scrollIntoViewIfNeeded",
      0,
    ),
    writable: true,
    enumerable: true,
    configurable: true,
  });
  define(Element.prototype, "checkVisibility", {
    __proto__: null,
    value: make(
      function (options) {
        brand(this, 1);
        const prefix = "Failed to execute 'checkVisibility' on 'Element': ";
        const dict = dictionary(options, prefix, "CheckVisibilityOptions");
        // Web IDL dictionaries convert members in lexicographic order.
        const checkOpacity = !!dict.checkOpacity,
          checkVisibilityCSS = !!dict.checkVisibilityCSS,
          contentVisibilityAuto = !!dict.contentVisibilityAuto,
          opacityProperty = !!dict.opacityProperty,
          visibilityProperty = !!dict.visibilityProperty;
        const flags =
          (checkOpacity || opacityProperty ? 1 : 0) |
          (checkVisibilityCSS || visibilityProperty ? 2 : 0) |
          (contentVisibilityAuto ? 4 : 0);
        return ops.op_live_element_visibility(this, flags);
      },
      "checkVisibility",
      0,
    ),
    writable: true,
    enumerable: true,
    configurable: true,
  });
  for (const [name, field] of [
    ["scrollWidth", "width"],
    ["scrollHeight", "height"],
    ["scrollLeft", "left"],
    ["scrollTop", "top"],
  ]) {
    const descriptor = {
      __proto__: null,
      get: make(
        function () {
          brand(this, 1);
          return ops.op_live_element_scroll(this, 0, 0)[field];
        },
        `get ${name}`,
        0,
      ),
      enumerable: true,
      configurable: true,
    };
    if (name === "scrollLeft" || name === "scrollTop")
      descriptor.set = make(
        function (value) {
          brand(this, 1);
          value = number(value);
          interrupt(this);
          scrolled(this, ops.op_live_element_scroll(this, name === "scrollLeft" ? 1 : 2, value));
        },
        `set ${name}`,
        1,
      );
    define(Element.prototype, name, descriptor);
  }
  for (const [prototype, kind] of [
    [Document.prototype, 9],
    [ShadowRoot.prototype, 11],
  ])
    for (const name of ["elementFromPoint", "elementsFromPoint"])
      define(prototype, name, {
        __proto__: null,
        value: make(
          function (x, y) {
            brand(this, kind);
            if (arguments.length < 2)
              throw new LocalTypeError(
                `Failed to execute '${name}' on 'Document': 2 arguments required, but only ${arguments.length} present.`,
              );
            x = coordinate(x, name);
            y = coordinate(y, name);
            const result = ops.op_live_elements_at_point(this, x, y);
            return name === "elementFromPoint" ? (result[0] ?? null) : result;
          },
          name,
          2,
        ),
        writable: true,
        enumerable: true,
        configurable: true,
      });
  for (const name of ["getClientRects", "getBoundingClientRect"])
    define(Range.prototype, name, {
      __proto__: null,
      value: make(
        function () {
          const result = ops.op_live_range_geometry(this);
          return name === "getClientRects" ? rectList(result.rects) : rectangle(result.bounds);
        },
        name,
        0,
      ),
      writable: true,
      enumerable: true,
      configurable: true,
    });
  const RangeConstructor = Range;
  const rangeStart = Range.prototype.setStart;
  const rangeCollapse = Range.prototype.collapse;
  const CaretPosition = make(
    function () {
      throw new LocalTypeError("Illegal constructor");
    },
    "CaretPosition",
    0,
    true,
    true,
  );
  const caretPrototype = CaretPosition.prototype;
  define(caretPrototype, Symbol.toStringTag, {
    __proto__: null,
    value: "CaretPosition",
    configurable: true,
  });
  function caretState(receiver) {
    const record = getRecord(receiver);
    if (!record || record.kind !== "CaretPosition") throw new LocalTypeError("Illegal invocation");
    return record;
  }
  for (const [name, field] of [
    ["offsetNode", "node"],
    ["offset", "offset"],
  ])
    define(caretPrototype, name, {
      __proto__: null,
      get: make(
        function () {
          return caretState(this)[field];
        },
        "get " + name,
        0,
      ),
      enumerable: true,
      configurable: true,
    });
  define(caretPrototype, "getClientRect", {
    __proto__: null,
    value: make(
      function () {
        const record = caretState(this);
        const rect = ops.op_live_caret_geometry(record.node, record.offset).rect;
        return rect === null ? null : rectangle(rect);
      },
      "getClientRect",
      0,
    ),
    writable: true,
    enumerable: true,
    configurable: true,
  });
  define(CaretPosition, "prototype", { __proto__: null, writable: false });
  define(globalThis, "CaretPosition", {
    __proto__: null,
    value: CaretPosition,
    writable: true,
    enumerable: false,
    configurable: true,
  });
  for (const name of ["caretRangeFromPoint", "caretPositionFromPoint"])
    define(Document.prototype, name, {
      __proto__: null,
      value: make(
        function (x, y) {
          brand(this, 9);
          if (name === "caretPositionFromPoint" && arguments.length < 2)
            throw new LocalTypeError(
              `Failed to execute '${name}' on 'Document': 2 arguments required, but only ${arguments.length} present.`,
            );
          x =
            name === "caretRangeFromPoint"
              ? number(x === undefined ? 0 : x) | 0
              : floatCoordinate(x, name);
          y =
            name === "caretRangeFromPoint"
              ? number(y === undefined ? 0 : y) | 0
              : floatCoordinate(y, name);
          if (name === "caretPositionFromPoint" && arguments[2] != null) {
            const roots = arguments[2].shadowRoots;
            if (roots != null && Array.from(roots).length !== 0)
              throw new Error("Native shadow-root caret selection is unsupported");
          }
          const endpoint = ops.op_live_caret_at_point(this, x, y);
          if (endpoint === null) return null;
          if (name === "caretRangeFromPoint") {
            const range = new RangeConstructor();
            apply(rangeStart, range, [endpoint.node, endpoint.offset]);
            apply(rangeCollapse, range, [true]);
            return range;
          }
          const result = ops.op_geometry_object(caretPrototype);
          if (
            !setRecord(result, {
              __proto__: null,
              kind: "CaretPosition",
              node: endpoint.node,
              offset: endpoint.offset,
              rect: endpoint.rect,
            })
          )
            throw new LocalTypeError("Caret record installation interrupted");
          return result;
        },
        name,
        name === "caretRangeFromPoint" ? 0 : 2,
      ),
      writable: true,
      enumerable: true,
      configurable: true,
    });
  for (const [name, field] of [
    ["offsetLeft", "left"],
    ["offsetTop", "top"],
    ["offsetParent", "parent"],
  ])
    define(HTMLElement.prototype, name, {
      __proto__: null,
      get: make(
        function () {
          brand(this, 2);
          return ops.op_live_element_offset(this)[field];
        },
        `get ${name}`,
        0,
      ),
      enumerable: true,
      configurable: true,
    });
  return make(
    function (x, y, width, height, readOnly) {
      const kind = readOnly ? "DOMRectReadOnly" : "DOMRect";
      return object(kind, prototypes[kind], x, y, width, height);
    },
    "",
    5,
  );
});
