// IntersectionObserver, ResizeObserver and PerformanceObserver (#28, measured in
// harness/detection/observers-probe.js). Resize and intersection observations run in the
// rendering update after animation frame callbacks, over the live layout boxes; a frame is
// requested when targets are observed and whenever the document mutates while observations
// exist. Intersection callbacks are delivered in a queued task, resize callbacks inside the
// rendering update, performance entries in a task after they are recorded.
// Not modelled: clipping by an iframe's embedding document (the implicit root is the realm's own
// viewport), CSS transforms on observed boxes, ResizeObserver depth iteration (one pass per
// update), and IntersectionObserver v2 visibility (isVisible stays false).
(() => {
  "use strict";
  const define = Object.defineProperty,
    apply = Reflect.apply,
    make = __G.makeNativeFunction;
  const ErrorType = __exceptionTypeError,
    Exception = DOMException,
    RangeErrorType = RangeError;
  const freeze = Object.freeze,
    create = Object.create,
    StringValue = String,
    NumberValue = Number;
  const finite = Number.isFinite,
    minimum = Math.min,
    maximum = Math.max,
    round = Math.round;
  const arrayPush = Array.prototype.push,
    arraySort = Array.prototype.sort,
    arrayIndexOf = Array.prototype.indexOf,
    arraySplice = Array.prototype.splice;
  const exec = RegExp.prototype.exec,
    split = String.prototype.split,
    trim = String.prototype.trim,
    lower = String.prototype.toLowerCase;
  const weakGet = WeakMap.prototype.get,
    weakSet = WeakMap.prototype.set;
  const hostTimeout = globalThis.setTimeout,
    computedStyle = globalThis.getComputedStyle;
  const HostMutationObserver = globalThis.MutationObserver,
    mutationObserve = HostMutationObserver.prototype.observe,
    mutationDisconnect = HostMutationObserver.prototype.disconnect;
  const contains = Node.prototype.contains;
  const checkElement = __G.domAccessor("nodeType", false, "Element");
  const connected = __G.domAccessor("isConnected", false, "Node");
  const parentElement = __G.domAccessor("parentElement", false, "Node");
  const realm = globalThis;
  const push = (list, value) => apply(arrayPush, list, [value]);
  const map = () => {
    const store = new WeakMap();
    return {
      get: (key) => apply(weakGet, store, [key]),
      set: (key, value) => apply(weakSet, store, [key, value]),
    };
  };
  const records = map();
  const recordOf = (receiver, kind) => {
    const record =
      receiver !== null && typeof receiver === "object" ? records.get(receiver) : undefined;
    if (!record || record.kind !== kind) throw new ErrorType("Illegal invocation");
    return record;
  };
  const report = (error) =>
    apply(hostTimeout, realm, [
      () => {
        throw error;
      },
      0,
    ]);
  const task = (callback) => apply(hostTimeout, realm, [callback, 0]);
  const interfaceObject = (name, length, construct) => {
    const constructor = make(
      construct ||
        function () {
          throw new ErrorType("Illegal constructor");
        },
      name,
      length,
      true,
      !!construct,
    );
    const prototype = constructor.prototype;
    define(prototype, Symbol.toStringTag, { __proto__: null, value: name, configurable: true });
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
  const getter = (prototype, kind, name, read) =>
    define(prototype, name, {
      __proto__: null,
      get: make(
        function () {
          return read(recordOf(this, kind));
        },
        "get " + name,
        0,
      ),
      enumerable: true,
      configurable: true,
    });
  const operation = (prototype, name, length, implementation) =>
    define(prototype, name, {
      __proto__: null,
      value: make(implementation, name, length),
      writable: true,
      enumerable: true,
      configurable: true,
    });
  const element = (value, interfaceName, method) => {
    try {
      apply(checkElement, value, []);
    } catch {
      throw new ErrorType(
        `Failed to execute '${method}' on '${interfaceName}': parameter 1 is not of type 'Element'.`,
      );
    }
    return value;
  };
  const rect = (x, y, width, height) => __G.readOnlyRect(x, y, width, height);
  const px = (value) => {
    const number = NumberValue(apply(split, StringValue(value || "0"), ["px"])[0]);
    return finite(number) ? number : 0;
  };

  // ---- Layout reads -----------------------------------------------------------------------
  // One rendering update reads each box and style once.
  let frameBoxes = null,
    frameStyles = null;
  const mapGet = Map.prototype.get,
    mapSet = Map.prototype.set,
    mapHas = Map.prototype.has;
  const cached = (cache, key, read) => {
    if (!cache) return read();
    if (apply(mapHas, cache, [key])) return apply(mapGet, cache, [key]);
    const value = read();
    apply(mapSet, cache, [key, value]);
    return value;
  };
  // [x, y, width, height] of the border box in viewport coordinates, or null without a box.
  const borderBox = (target) =>
    cached(frameBoxes, target, () => {
      if (!apply(connected, target, [])) return null;
      let geometry;
      try {
        geometry = ops.op_live_element_geometry(target);
      } catch {
        return null;
      }
      return geometry && geometry.rects.length
        ? [geometry.bounds[0], geometry.bounds[1], geometry.bounds[2], geometry.bounds[3]]
        : null;
    });
  const styleOf = (target) =>
    cached(frameStyles, target, () => {
      try {
        return apply(computedStyle, realm, [target]);
      } catch {
        return null;
      }
    });
  const edges = (style, property) =>
    style
      ? ["top", "right", "bottom", "left"].map((side) =>
          px(
            style.getPropertyValue(
              property === "border" ? `border-${side}-width` : `${property}-${side}`,
            ),
          ),
        )
      : [0, 0, 0, 0];
  const paddingBox = (target, box) => {
    const border = edges(styleOf(target), "border");
    return [
      box[0] + border[3],
      box[1] + border[0],
      maximum(0, box[2] - border[1] - border[3]),
      maximum(0, box[3] - border[0] - border[2]),
    ];
  };
  const viewport = () => {
    const size = [0, 0];
    for (const axis of [0, 1]) {
      try {
        const value = ops.op_live_frame_dimension(realm, axis === 1, true);
        size[axis] = typeof value === "number" ? value : 0;
      } catch {
        size[axis] = axis
          ? NumberValue(realm.innerHeight) || 0
          : NumberValue(realm.innerWidth) || 0;
      }
    }
    return [0, 0, size[0], size[1]];
  };
  // Edge-inclusive intersection: touching boxes intersect with zero area.
  const intersect = (a, b) => {
    const left = maximum(a[0], b[0]),
      top = maximum(a[1], b[1]);
    const right = minimum(a[0] + a[2], b[0] + b[2]),
      bottom = minimum(a[1] + a[3], b[1] + b[3]);
    return right < left || bottom < top ? null : [left, top, right - left, bottom - top];
  };

  // ---- Rendering update ---------------------------------------------------------------------
  const resizeObservers = [],
    intersectionObservers = [];
  let watching = false,
    watcher = null;
  const requestFrame = () => __scheduleFrame();
  // While anything is observed, a document mutation can move or resize a box: request an update.
  const watch = () => {
    const active = resizeObservers.length > 0 || intersectionObservers.length > 0;
    if (active === watching || !__document) return;
    watching = active;
    if (!watcher) watcher = new HostMutationObserver(requestFrame);
    if (active)
      apply(mutationObserve, watcher, [
        __document,
        { __proto__: null, subtree: true, childList: true, attributes: true, characterData: true },
      ]);
    else apply(mutationDisconnect, watcher, []);
  };
  const register = (list, record) => {
    if (apply(arrayIndexOf, list, [record]) < 0) push(list, record);
    watch();
  };
  const unregister = (list, record) => {
    const at = apply(arrayIndexOf, list, [record]);
    if (at >= 0) apply(arraySplice, list, [at, 1]);
    watch();
  };

  // ---- ResizeObserver ---------------------------------------------------------------------
  const ResizeObserverSize = interfaceObject("ResizeObserverSize", 0);
  getter(
    ResizeObserverSize.prototype,
    "ResizeObserverSize",
    "inlineSize",
    (record) => record.inline,
  );
  getter(ResizeObserverSize.prototype, "ResizeObserverSize", "blockSize", (record) => record.block);
  const observerSize = (inline, block) => {
    const size = create(ResizeObserverSize.prototype);
    records.set(size, { __proto__: null, kind: "ResizeObserverSize", inline, block });
    return freeze([size]);
  };
  const ResizeObserverEntry = interfaceObject("ResizeObserverEntry", 0);
  for (const name of [
    "target",
    "contentRect",
    "borderBoxSize",
    "contentBoxSize",
    "devicePixelContentBoxSize",
  ])
    getter(ResizeObserverEntry.prototype, "ResizeObserverEntry", name, (record) => record[name]);
  const boxes = ["content-box", "border-box", "device-pixel-content-box"];
  // Border and content box sizes (and the content box's padding offset) of an observed target.
  const measureResize = (target) => {
    const box = borderBox(target);
    if (!box) return { border: [0, 0], content: [0, 0], offset: [0, 0] };
    const style = styleOf(target),
      padding = edges(style, "padding"),
      border = edges(style, "border");
    return {
      border: [box[2], box[3]],
      content: [
        maximum(0, box[2] - padding[1] - padding[3] - border[1] - border[3]),
        maximum(0, box[3] - padding[0] - padding[2] - border[0] - border[2]),
      ],
      offset: [padding[3], padding[0]],
    };
  };
  const reported = (box, sizes) =>
    box === "border-box"
      ? sizes.border
      : box === "device-pixel-content-box"
        ? devicePixels(sizes.content)
        : sizes.content;
  const devicePixels = (size) => {
    const ratio = NumberValue(realm.devicePixelRatio) || 1;
    return [round(size[0] * ratio), round(size[1] * ratio)];
  };
  const ResizeObserver = interfaceObject("ResizeObserver", 1, function (callback) {
    if (!new.target)
      throw new ErrorType(
        "Failed to construct 'ResizeObserver': Please use the 'new' operator, this DOM object constructor cannot be called as a function.",
      );
    if (typeof callback !== "function")
      throw new ErrorType(
        "Failed to construct 'ResizeObserver': parameter 1 is not of type 'ResizeObserverCallback'.",
      );
    records.set(this, {
      __proto__: null,
      kind: "ResizeObserver",
      self: this,
      callback,
      observations: [],
    });
  });
  const ResizePrototype = ResizeObserver.prototype;
  operation(ResizePrototype, "observe", 1, function (target, options = undefined) {
    const record = recordOf(this, "ResizeObserver");
    if (!arguments.length)
      throw new ErrorType(
        "Failed to execute 'observe' on 'ResizeObserver': 1 argument required, but only 0 present.",
      );
    element(target, "ResizeObserver", "observe");
    let box = "content-box";
    if (options !== undefined && options !== null) {
      if (typeof options !== "object" && typeof options !== "function")
        throw new ErrorType(
          "Failed to execute 'observe' on 'ResizeObserver': The provided value is not of type 'ResizeObserverOptions'.",
        );
      if (options.box !== undefined) {
        box = StringValue(options.box);
        if (apply(arrayIndexOf, boxes, [box]) < 0)
          throw new ErrorType(
            `Failed to execute 'observe' on 'ResizeObserver': Failed to read the 'box' property from 'ResizeObserverOptions': The provided value '${box}' is not a valid enum value of type ResizeObserverBoxOptions.`,
          );
      }
    }
    const list = record.observations;
    for (let at = 0; at < list.length; at++)
      if (list[at].target === target) apply(arraySplice, list, [at, 1]);
    push(list, { __proto__: null, target, box, last: null });
    register(resizeObservers, record);
    requestFrame();
  });
  operation(ResizePrototype, "unobserve", 1, function (target) {
    const record = recordOf(this, "ResizeObserver");
    if (!arguments.length)
      throw new ErrorType(
        "Failed to execute 'unobserve' on 'ResizeObserver': 1 argument required, but only 0 present.",
      );
    element(target, "ResizeObserver", "unobserve");
    const list = record.observations;
    for (let at = 0; at < list.length; at++)
      if (list[at].target === target) apply(arraySplice, list, [at, 1]);
    if (!list.length) unregister(resizeObservers, record);
  });
  operation(ResizePrototype, "disconnect", 0, function () {
    const record = recordOf(this, "ResizeObserver");
    record.observations = [];
    unregister(resizeObservers, record);
  });
  function broadcastResizes() {
    for (const record of [...resizeObservers]) {
      const entries = [];
      for (const observation of record.observations) {
        const sizes = measureResize(observation.target);
        const current = reported(observation.box, sizes);
        if (
          observation.last &&
          observation.last[0] === current[0] &&
          observation.last[1] === current[1]
        )
          continue;
        observation.last = current;
        const entry = create(ResizeObserverEntry.prototype);
        records.set(entry, {
          __proto__: null,
          kind: "ResizeObserverEntry",
          target: observation.target,
          contentRect: rect(sizes.offset[0], sizes.offset[1], sizes.content[0], sizes.content[1]),
          borderBoxSize: observerSize(sizes.border[0], sizes.border[1]),
          contentBoxSize: observerSize(sizes.content[0], sizes.content[1]),
          devicePixelContentBoxSize: observerSize(...devicePixels(sizes.content)),
        });
        push(entries, entry);
      }
      if (!entries.length) continue;
      const observer = record.self;
      try {
        apply(record.callback, observer, [entries, observer]);
      } catch (error) {
        report(error);
      }
    }
  }

  // ---- IntersectionObserver -----------------------------------------------------------------
  const IntersectionObserverEntry = interfaceObject("IntersectionObserverEntry", 0);
  for (const name of [
    "time",
    "rootBounds",
    "boundingClientRect",
    "intersectionRect",
    "isIntersecting",
    "isVisible",
    "intersectionRatio",
    "target",
  ])
    getter(
      IntersectionObserverEntry.prototype,
      "IntersectionObserverEntry",
      name,
      (record) => record[name],
    );
  // rootMargin/scrollMargin: one to four <length-percentage>s, pixels or percents only.
  const margin = (value, name) => {
    const tokens = apply(split, apply(trim, StringValue(value), []), [/\s+/]);
    if (tokens.length === 1 && tokens[0] === "") tokens[0] = "0px";
    if (tokens.length > 4)
      throw new Exception(
        `Failed to construct 'IntersectionObserver': Extra text found at the end of ${name}.`,
        "SyntaxError",
      );
    const parsed = tokens.map((token) => {
      const match = apply(exec, /^(-?(?:\d+\.?\d*|\.\d+)(?:e[-+]?\d+)?)(px|%)?$/i, [token]);
      if (!match || (!match[2] && NumberValue(match[1]) !== 0))
        throw new Exception(
          `Failed to construct 'IntersectionObserver': ${name} must be specified in pixels or percent.`,
          "SyntaxError",
        );
      return { value: NumberValue(match[1]), unit: match[2] ? apply(lower, match[2], []) : "px" };
    });
    const [top, right = top, bottom = top, left = right] = parsed;
    return [top, right, bottom, left];
  };
  const serializeMargin = (sides) => sides.map((side) => side.value + side.unit).join(" ");
  const ratioValue = (value) => {
    if (typeof value === "symbol") throw new ErrorType("Cannot convert a Symbol value to a number");
    const number = NumberValue(value);
    if (!finite(number))
      throw new ErrorType(
        "Failed to construct 'IntersectionObserver': The provided double value is non-finite.",
      );
    if (number < 0 || number > 1)
      throw new RangeErrorType(
        "Failed to construct 'IntersectionObserver': Threshold values must be numbers between 0 and 1",
      );
    return number;
  };
  const IntersectionObserver = interfaceObject(
    "IntersectionObserver",
    1,
    function (callback, options = undefined) {
      if (!new.target)
        throw new ErrorType(
          "Failed to construct 'IntersectionObserver': Please use the 'new' operator, this DOM object constructor cannot be called as a function.",
        );
      if (typeof callback !== "function")
        throw new ErrorType(
          "Failed to construct 'IntersectionObserver': parameter 1 is not of type 'IntersectionObserverCallback'.",
        );
      if (
        options !== undefined &&
        options !== null &&
        typeof options !== "object" &&
        typeof options !== "function"
      )
        throw new ErrorType(
          "Failed to construct 'IntersectionObserver': The provided value is not of type 'IntersectionObserverInit'.",
        );
      options = options || {};
      // Dictionary members in lexicographic order, as Web IDL reads them.
      const delay = options.delay === undefined ? 0 : NumberValue(options.delay);
      let root = options.root === undefined ? null : options.root;
      if (root !== null) {
        try {
          apply(checkElement, root, []);
        } catch {
          if (root !== __document)
            throw new ErrorType(
              "Failed to construct 'IntersectionObserver': Failed to read the 'root' property from 'IntersectionObserverInit': The provided value is not of type '(Document or Element)'.",
            );
        }
      }
      const rootMargin = margin(
        options.rootMargin === undefined ? "0px" : options.rootMargin,
        "rootMargin",
      );
      const scrollMargin = margin(
        options.scrollMargin === undefined ? "0px" : options.scrollMargin,
        "scrollMargin",
      );
      let thresholds = options.threshold === undefined ? [0] : options.threshold;
      thresholds =
        typeof thresholds === "object" &&
        thresholds !== null &&
        typeof thresholds[Symbol.iterator] === "function"
          ? [...thresholds].map(ratioValue)
          : [ratioValue(thresholds)];
      if (!thresholds.length) thresholds = [0];
      apply(arraySort, thresholds, [(a, b) => a - b]);
      const trackVisibility = !!options.trackVisibility;
      if (trackVisibility && !(delay >= 100))
        throw new Exception(
          "Failed to construct 'IntersectionObserver': To enable the 'trackVisibility' option, you must also use a value of 100 or greater for the 'delay' option.",
          "NotSupportedError",
        );
      records.set(this, {
        __proto__: null,
        kind: "IntersectionObserver",
        self: this,
        callback,
        root,
        rootMargin,
        scrollMargin,
        thresholds: freeze(thresholds),
        delay: finite(delay) ? delay : 0,
        trackVisibility,
        observations: [],
        queue: [],
        pending: false,
      });
    },
  );
  const IntersectionPrototype = IntersectionObserver.prototype;
  getter(IntersectionPrototype, "IntersectionObserver", "root", (record) => record.root);
  getter(IntersectionPrototype, "IntersectionObserver", "rootMargin", (record) =>
    serializeMargin(record.rootMargin),
  );
  getter(IntersectionPrototype, "IntersectionObserver", "scrollMargin", (record) =>
    serializeMargin(record.scrollMargin),
  );
  getter(IntersectionPrototype, "IntersectionObserver", "thresholds", (record) =>
    freeze([...record.thresholds]),
  );
  getter(IntersectionPrototype, "IntersectionObserver", "delay", (record) => record.delay);
  getter(
    IntersectionPrototype,
    "IntersectionObserver",
    "trackVisibility",
    (record) => record.trackVisibility,
  );
  operation(IntersectionPrototype, "observe", 1, function (target) {
    const record = recordOf(this, "IntersectionObserver");
    if (!arguments.length)
      throw new ErrorType(
        "Failed to execute 'observe' on 'IntersectionObserver': 1 argument required, but only 0 present.",
      );
    element(target, "IntersectionObserver", "observe");
    for (const observation of record.observations) if (observation.target === target) return;
    push(record.observations, { __proto__: null, target, index: -1, intersecting: false });
    register(intersectionObservers, record);
    requestFrame();
  });
  operation(IntersectionPrototype, "unobserve", 1, function (target) {
    const record = recordOf(this, "IntersectionObserver");
    if (!arguments.length)
      throw new ErrorType(
        "Failed to execute 'unobserve' on 'IntersectionObserver': 1 argument required, but only 0 present.",
      );
    element(target, "IntersectionObserver", "unobserve");
    const list = record.observations;
    for (let at = 0; at < list.length; at++)
      if (list[at].target === target) apply(arraySplice, list, [at, 1]);
    record.queue = record.queue.filter((entry) => records.get(entry).target !== target);
    if (!list.length) unregister(intersectionObservers, record);
  });
  operation(IntersectionPrototype, "disconnect", 0, function () {
    const record = recordOf(this, "IntersectionObserver");
    record.observations = [];
    record.queue = [];
    unregister(intersectionObservers, record);
  });
  operation(IntersectionPrototype, "takeRecords", 0, function () {
    const record = recordOf(this, "IntersectionObserver");
    const queue = record.queue;
    record.queue = [];
    return queue;
  });
  const marginRect = (box, sides) => {
    const amount = (side, extent) => (side.unit === "%" ? (side.value * extent) / 100 : side.value);
    const top = amount(sides[0], box[3]),
      right = amount(sides[1], box[2]),
      bottom = amount(sides[2], box[3]),
      left = amount(sides[3], box[2]);
    return [box[0] - left, box[1] - top, box[2] + left + right, box[3] + top + bottom];
  };
  // An ancestor clips when either overflow axis is not visible (computed "" reads as visible).
  const clips = (style) => {
    if (!style) return false;
    const overflow = style.getPropertyValue("overflow");
    const axis = (name) => style.getPropertyValue(name) || overflow || "visible";
    return axis("overflow-x") !== "visible" || axis("overflow-y") !== "visible";
  };
  // HTML "run the update intersection observations steps" for one target.
  function observe(record, target, time) {
    const box = borderBox(target);
    const explicit = record.root !== null && record.root !== __document;
    let rootBounds = null,
      intersection = null;
    if (box) {
      if (explicit) {
        const rootBox =
          apply(contains, record.root, [target]) && record.root !== target
            ? borderBox(record.root)
            : null;
        rootBounds = rootBox && marginRect(paddingBox(record.root, rootBox), record.rootMargin);
      } else rootBounds = marginRect(viewport(), record.rootMargin);
      if (rootBounds) {
        intersection = box;
        // Clip by every overflow-clipping ancestor up to the root.
        for (
          let ancestor = apply(parentElement, target, []);
          intersection && ancestor && ancestor !== record.root;
          ancestor = apply(parentElement, ancestor, [])
        ) {
          if (!clips(styleOf(ancestor))) continue;
          const ancestorBox = borderBox(ancestor);
          if (ancestorBox)
            intersection = intersect(intersection, paddingBox(ancestor, ancestorBox));
        }
        if (intersection) intersection = intersect(intersection, rootBounds);
      }
    }
    const intersecting = intersection !== null;
    const targetArea = box ? box[2] * box[3] : 0;
    const ratio = !intersecting
      ? 0
      : targetArea > 0
        ? (intersection[2] * intersection[3]) / targetArea
        : 1;
    let index = 0;
    if (intersecting) {
      index = record.thresholds.length;
      for (let at = 0; at < record.thresholds.length; at++)
        if (record.thresholds[at] > ratio) {
          index = at;
          break;
        }
    }
    return { box, rootBounds, intersection, intersecting, ratio, index, time };
  }
  function updateIntersections(time) {
    for (const record of [...intersectionObservers]) {
      for (const observation of record.observations) {
        const state = observe(record, observation.target, time);
        if (state.index === observation.index && state.intersecting === observation.intersecting)
          continue;
        observation.index = state.index;
        observation.intersecting = state.intersecting;
        const entry = create(IntersectionObserverEntry.prototype);
        const zero = [0, 0, 0, 0];
        records.set(entry, {
          __proto__: null,
          kind: "IntersectionObserverEntry",
          time,
          rootBounds: rect(...(state.box && state.rootBounds ? state.rootBounds : zero)),
          boundingClientRect: rect(...(state.box || zero)),
          intersectionRect: rect(...(state.intersection || zero)),
          isIntersecting: state.intersecting,
          isVisible: false,
          intersectionRatio: state.ratio,
          target: observation.target,
        });
        push(record.queue, entry);
      }
      if (record.queue.length && !record.pending) {
        record.pending = true;
        task(() => {
          record.pending = false;
          const queue = record.queue;
          record.queue = [];
          if (!queue.length) return;
          try {
            apply(record.callback, record.self, [queue, record.self]);
          } catch (error) {
            report(error);
          }
        });
      }
    }
  }
  push(__renderingSteps, () => {
    if (resizeObservers.length) {
      frameBoxes = new Map();
      frameStyles = new Map();
      try {
        broadcastResizes();
      } finally {
        frameBoxes = frameStyles = null;
      }
    }
    // Resize callbacks may have changed layout: intersections read fresh boxes.
    if (intersectionObservers.length) {
      frameBoxes = new Map();
      frameStyles = new Map();
      try {
        updateIntersections(__perfNow());
      } finally {
        frameBoxes = frameStyles = null;
      }
    }
  });

  // ---- PerformanceObserver -----------------------------------------------------------------
  const supported = freeze([
    "element",
    "event",
    "first-input",
    "interaction-contentful-paint",
    "largest-contentful-paint",
    "layout-shift",
    "long-animation-frame",
    "longtask",
    "mark",
    "measure",
    "navigation",
    "paint",
    "resource",
    "soft-navigation",
    "visibility-state",
  ]);
  const performanceObservers = [];
  const entryList = interfaceObject("PerformanceObserverEntryList", 0);
  const byTime = (entries) => apply(arraySort, [...entries], [(a, b) => a.startTime - b.startTime]);
  operation(entryList.prototype, "getEntries", 0, function () {
    return byTime(recordOf(this, "PerformanceObserverEntryList").entries);
  });
  operation(entryList.prototype, "getEntriesByType", 1, function (type) {
    const record = recordOf(this, "PerformanceObserverEntryList");
    if (!arguments.length)
      throw new ErrorType(
        "Failed to execute 'getEntriesByType' on 'PerformanceObserverEntryList': 1 argument required, but only 0 present.",
      );
    type = StringValue(type);
    return byTime(record.entries.filter((entry) => entry.entryType === type));
  });
  operation(entryList.prototype, "getEntriesByName", 1, function (name, type = undefined) {
    const record = recordOf(this, "PerformanceObserverEntryList");
    if (!arguments.length)
      throw new ErrorType(
        "Failed to execute 'getEntriesByName' on 'PerformanceObserverEntryList': 1 argument required, but only 0 present.",
      );
    name = StringValue(name);
    if (type !== undefined) type = StringValue(type);
    return byTime(
      record.entries.filter(
        (entry) => entry.name === name && (type === undefined || entry.entryType === type),
      ),
    );
  });
  const PerformanceObserver = interfaceObject("PerformanceObserver", 1, function (callback) {
    if (!new.target)
      throw new ErrorType(
        "Failed to construct 'PerformanceObserver': Please use the 'new' operator, this DOM object constructor cannot be called as a function.",
      );
    if (typeof callback !== "function")
      throw new ErrorType(
        "Failed to construct 'PerformanceObserver': parameter 1 is not of type 'PerformanceObserverCallback'.",
      );
    records.set(this, {
      __proto__: null,
      kind: "PerformanceObserver",
      self: this,
      callback,
      mode: null,
      types: [],
      buffer: [],
      pending: false,
      buffered: false,
    });
  });
  define(PerformanceObserver, "supportedEntryTypes", {
    __proto__: null,
    get: make(
      function () {
        return supported;
      },
      "get supportedEntryTypes",
      0,
    ),
    enumerable: true,
    configurable: true,
  });
  const PerformancePrototype = PerformanceObserver.prototype;
  const deliverPerformance = (record) => {
    if (record.pending) return;
    record.pending = true;
    task(() => {
      record.pending = false;
      const entries = record.buffer;
      record.buffer = [];
      if (!entries.length) return;
      const list = create(entryList.prototype);
      records.set(list, { __proto__: null, kind: "PerformanceObserverEntryList", entries });
      const options = { droppedEntriesCount: 0 };
      try {
        apply(
          record.callback,
          record.self,
          record.buffered ? [list, record.self, options] : [list, record.self],
        );
      } catch (error) {
        report(error);
      }
      record.buffered = false;
    });
  };
  operation(PerformancePrototype, "observe", 0, function (options = undefined) {
    const record = recordOf(this, "PerformanceObserver");
    if (
      options !== undefined &&
      options !== null &&
      typeof options !== "object" &&
      typeof options !== "function"
    )
      throw new ErrorType(
        "Failed to execute 'observe' on 'PerformanceObserver': The provided value is not of type 'PerformanceObserverInit'.",
      );
    options = options || {};
    const hasTypes = options.entryTypes !== undefined,
      hasType = options.type !== undefined;
    if (!hasTypes && !hasType)
      throw new ErrorType(
        "Failed to execute 'observe' on 'PerformanceObserver': An observe() call must include either entryTypes or type arguments.",
      );
    if (hasTypes && hasType)
      throw new ErrorType(
        "Failed to execute 'observe' on 'PerformanceObserver': An observe() call must not include both entryTypes and type arguments.",
      );
    if (record.mode === "multiple" && hasType)
      throw new Exception(
        "Failed to execute 'observe' on 'PerformanceObserver': This observer has performed observe({entryTypes:...}, therefore it cannot perform observe({type:...})",
        "InvalidModificationError",
      );
    if (record.mode === "single" && hasTypes)
      throw new Exception(
        "Failed to execute 'observe' on 'PerformanceObserver': This PerformanceObserver has performed observe({type:...}, therefore it cannot perform observe({entryTypes:...})",
        "InvalidModificationError",
      );
    if (hasTypes) {
      const types = [...options.entryTypes]
        .map(StringValue)
        .filter((type) => apply(arrayIndexOf, supported, [type]) >= 0);
      if (!types.length) return;
      record.mode = "multiple";
      record.types = types;
    } else {
      const type = StringValue(options.type);
      if (apply(arrayIndexOf, supported, [type]) < 0) return;
      record.mode = "single";
      if (apply(arrayIndexOf, record.types, [type]) < 0) push(record.types, type);
      if (options.buffered) {
        record.buffered = true;
        for (const entry of __performanceTimeline)
          if (entry.entryType === type) push(record.buffer, entry);
        deliverPerformance(record);
      }
    }
    register(performanceObservers, record);
  });
  operation(PerformancePrototype, "disconnect", 0, function () {
    const record = recordOf(this, "PerformanceObserver");
    record.buffer = [];
    record.types = [];
    record.mode = null;
    unregister(performanceObservers, record);
  });
  operation(PerformancePrototype, "takeRecords", 0, function () {
    const record = recordOf(this, "PerformanceObserver");
    const entries = record.buffer;
    record.buffer = [];
    return entries;
  });
  // Every timeline append (marks, measures, resource and navigation entries) reaches observers.
  define(__performanceTimeline, "push", {
    __proto__: null,
    value(...entries) {
      const length = apply(arrayPush, this, entries);
      for (const entry of entries)
        for (const record of performanceObservers)
          if (apply(arrayIndexOf, record.types, [entry.entryType]) >= 0) {
            push(record.buffer, entry);
            deliverPerformance(record);
          }
      return length;
    },
    writable: true,
    configurable: true,
  });
})();
