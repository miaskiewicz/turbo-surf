// Private, native-branded rectangle data; no guest-visible host registry.
(function (core, make, frameWindow) {
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
  define(globalThis, "DOMRectList", {
    __proto__: null,
    value: DOMRectList,
    writable: true,
    configurable: true,
  });
  function rectangle(values) {
    return object("DOMRect", prototypes.DOMRect, values[0], values[1], values[2], values[3]);
  }
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
          ops.op_live_element_scroll(this, name === "scrollLeft" ? 1 : 2, number(value));
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
  define(globalThis, "CaretPosition", {
    __proto__: null,
    value: CaretPosition,
    writable: true,
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
