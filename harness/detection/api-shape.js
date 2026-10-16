// API contract collector. Run uninstrumented in both engines; temporary timing marks
// are cleared after capture. These
// queries are ours and do not identify which differences a site actually uses.
(() => {
  const safe = (fn) => {
    try {
      return fn();
    } catch (error) {
      return { error: error.name };
    }
  };
  const keys = (value) => Reflect.ownKeys(value).map(String).sort();
  const constructible = (fn) => {
    try {
      Reflect.construct(function () {}, [], fn);
      return true;
    } catch {
      return false;
    }
  };
  const shape = (fn) =>
    typeof fn === "function"
      ? {
          name: fn.name,
          length: fn.length,
          ownKeys: keys(fn),
          constructible: constructible(fn),
          source: Function.prototype.toString.call(fn),
          functionPrototype: Object.getPrototypeOf(fn) === Function.prototype,
          nameDescriptor: descriptor(Object.getOwnPropertyDescriptor(fn, "name")),
          lengthDescriptor: descriptor(Object.getOwnPropertyDescriptor(fn, "length")),
          prototypeDescriptor: descriptor(Object.getOwnPropertyDescriptor(fn, "prototype")),
        }
      : { type: typeof fn };
  const descriptor = (d) => {
    if (!d) return null;
    const result = { configurable: d.configurable, enumerable: d.enumerable };
    if ("value" in d) {
      result.kind = "data";
      result.writable = d.writable;
      result.valueType = typeof d.value;
    } else {
      result.kind = "accessor";
      result.get = shape(d.get);
      result.set = shape(d.set);
    }
    return result;
  };
  const members = (prototype) =>
    Object.fromEntries(
      Reflect.ownKeys(prototype).map((key) => {
        const own = Object.getOwnPropertyDescriptor(prototype, key);
        const entry = descriptor(own);
        if (typeof own.value === "function") entry.function = shape(own.value);
        return [String(key), entry];
      }),
    );
  const owner = (object, property) => {
    let depth = 0;
    for (let current = object; current; current = Object.getPrototypeOf(current), depth++) {
      const d = Object.getOwnPropertyDescriptor(current, property);
      if (d) {
        return {
          depth,
          constructor: current.constructor?.name,
          descriptor: descriptor(d),
        };
      }
    }
    return null;
  };
  const canvas = document.createElement("canvas");
  const element = document.createElement("div");
  const objects = {
    navigator,
    document,
    element,
    canvas,
    performance,
    localStorage,
    permissions: navigator.permissions,
  };
  const methods = {
    "Function.toString": [Function.prototype, "toString", []],
    "Array.push": [Array.prototype, "push"],
    "Date.now": [Date, "now", []],
    "Date.parse": [Date, "parse"],
    "Date.UTC": [Date, "UTC"],
    "document.createElement": [document, "createElement", ["div"]],
    "document.querySelector": [document, "querySelector", ["body"]],
    "element.getAttribute": [element, "getAttribute", ["id"]],
    "canvas.getContext": [canvas, "getContext", ["2d"]],
    "canvas.toDataURL": [canvas, "toDataURL", []],
    "performance.now": [performance, "now", []],
    "Storage.getItem": [localStorage, "getItem", ["api-shape-missing"]],
    "navigator.sendBeacon": [navigator, "sendBeacon"],
    "permissions.query": [navigator.permissions, "query"],
    "window.fetch": [globalThis, "fetch"],
    "window.setTimeout": [globalThis, "setTimeout"],
    "window.requestAnimationFrame": [globalThis, "requestAnimationFrame"],
    "window.addEventListener": [globalThis, "addEventListener"],
    "window.postMessage": [globalThis, "postMessage"],
  };
  const data = { objects: {}, methods: {}, constructors: {}, accessors: {} };
  for (const [name, object] of Object.entries(objects)) {
    data.objects[name] = safe(() => ({
      tag: Object.prototype.toString.call(object),
      constructor: object.constructor?.name,
      ownKeys: keys(object),
      prototypeConstructor: Object.getPrototypeOf(object)?.constructor?.name,
    }));
  }
  for (const [name, [object, property, invalidArgs]] of Object.entries(methods)) {
    data.methods[name] = safe(() => {
      const fn = object[property];
      const result = { shape: shape(fn), owner: owner(object, property) };
      if (invalidArgs) {
        result.invalidReceiver = safe(() => ({
          returned: typeof Reflect.apply(fn, null, invalidArgs),
        }));
      }
      return result;
    });
  }
  for (const name of [
    "Date",
    "Node",
    "Element",
    "DocumentFragment",
    "ShadowRoot",
    "HTMLElement",
    "HTMLCanvasElement",
    "HTMLFormElement",
    "HTMLInputElement",
    "HTMLButtonElement",
    "Document",
    "Range",
    "AbstractRange",
    "NodeRange",
    "StaticRange",
    "CustomElementRegistry",
    "ElementInternals",
    "MutationObserver",
    "Navigator",
    "Performance",
    "Storage",
    "URL",
    "Request",
    "Response",
    "Event",
    "EventTarget",
    "MessageEvent",
    "MessageChannel",
    "MessagePort",
    "SubmitEvent",
    "AbortController",
    "AbortSignal",
    "ReadableStream",
    "ReadableStreamDefaultReader",
    "ReadableStreamBYOBReader",
    "WritableStream",
    "WritableStreamDefaultWriter",
    "TransformStream",
    "TrustedHTML",
    "TrustedScript",
    "TrustedScriptURL",
    "TrustedTypePolicy",
    "TrustedTypePolicyFactory",
    "ErrorEvent",
    "DOMException",
    "CSSStyleSheet",
    "StyleSheet",
    "MediaList",
    "Headers",
    "URLSearchParams",
    "TextEncoder",
    "TextDecoder",
    "Crypto",
    "SubtleCrypto",
    "DOMMatrix",
    "DOMMatrixReadOnly",
    "DOMPoint",
    "DOMPointReadOnly",
  ]) {
    data.constructors[name] = safe(() => {
      const ctor = globalThis[name];
      return {
        shape: shape(ctor),
        globalDescriptor: descriptor(Object.getOwnPropertyDescriptor(globalThis, name)),
        prototypeDescriptor: descriptor(Object.getOwnPropertyDescriptor(ctor, "prototype")),
        prototypeTag: Object.prototype.toString.call(ctor.prototype),
        prototypeKeys: keys(ctor.prototype),
        prototypeMembers: members(ctor.prototype),
      };
    });
  }
  for (const [name, object, property] of [
    ["navigator.userAgent", navigator, "userAgent"],
    ["document.URL", document, "URL"],
    ["element.id", element, "id"],
    ["canvas.width", canvas, "width"],
    ["performance.timeOrigin", performance, "timeOrigin"],
  ]) {
    data.accessors[name] = safe(() => {
      const result = { owner: owner(object, property) };
      let current = object;
      while (current && !Object.hasOwn(current, property)) current = Object.getPrototypeOf(current);
      const getter = current && Object.getOwnPropertyDescriptor(current, property)?.get;
      if (getter) {
        result.invalidReceiver = safe(() => ({ returned: typeof Reflect.apply(getter, {}, []) }));
      }
      return result;
    });
  }
  data.cssomLists = safe(() => {
    const listShape = (list) => ({
      array: Array.isArray(list),
      tag: Object.prototype.toString.call(list),
      constructor: list.constructor?.name,
      ownKeys: keys(list),
      lengthOwner: owner(list, "length"),
      indexDescriptor: descriptor(Object.getOwnPropertyDescriptor(list, "0")),
      itemShape: shape(list.item),
      itemOwner: owner(list, "item"),
      itemMatchesIndex: list.item(0) === list[0],
      missingItemIsNull: list.item(999) === null,
      iterationCount: Array.from(list).length,
    });
    const styles = document.styleSheets;
    const initialLength = styles.length;
    const style = document.createElement("style");
    style.textContent = "body {} sentinel {}";
    document.head.appendChild(style);
    try {
      const rules = style.sheet.cssRules;
      const styleSheets = listShape(styles);
      const cssRules = listShape(rules);
      const stableRules = rules === style.sheet.cssRules;
      const stableStyles = styles === document.styleSheets;
      const insertedLength = styles.length;
      const constructed = new CSSStyleSheet();
      const liveRules = constructed.cssRules;
      constructed.replaceSync("body {} sentinel {}");
      const replacement = {
        stable: liveRules === constructed.cssRules,
        length: liveRules.length,
        firstSelector: liveRules[0]?.selectorText,
      };
      style.remove();
      return {
        styleSheets,
        cssRules,
        stableRules,
        stableStyles,
        addedOne: insertedLength === initialLength + 1,
        removedOne: styles.length === initialLength,
        replacement,
      };
    } finally {
      style.remove();
    }
  });
  data.cssStyleSheets = safe(() => {
    const sheet = new CSSStyleSheet({ disabled: true, media: "screen, print" });
    const members = {};
    for (const [Ctor, names] of [
      [StyleSheet, ["type", "href", "ownerNode", "parentStyleSheet", "title", "media", "disabled"]],
      [
        CSSStyleSheet,
        [
          "ownerRule",
          "cssRules",
          "rules",
          "insertRule",
          "deleteRule",
          "replace",
          "replaceSync",
          "addRule",
          "removeRule",
        ],
      ],
      [MediaList, ["length", "mediaText", "item", "appendMedium", "deleteMedium", "toString"]],
    ]) {
      for (const name of names) {
        const d = Object.getOwnPropertyDescriptor(Ctor.prototype, name);
        const member = { descriptor: descriptor(d) };
        if (d?.value) member.shape = shape(d.value);
        if (d?.get) member.invalidReceiver = safe(() => ({ returned: typeof d.get.call({}) }));
        members[`${Ctor.name}.${name}`] = member;
      }
    }
    const initial = {
      tag: Object.prototype.toString.call(sheet),
      ownKeys: keys(sheet),
      prototypeKeys: keys(CSSStyleSheet.prototype),
      basePrototypeKeys: keys(StyleSheet.prototype),
      constructorInheritance: Object.getPrototypeOf(CSSStyleSheet) === StyleSheet,
      prototypeInheritance: Object.getPrototypeOf(CSSStyleSheet.prototype) === StyleSheet.prototype,
      styleSheetInstance: sheet instanceof StyleSheet,
      type: sheet.type,
      href: sheet.href,
      ownerNode: sheet.ownerNode === null,
      parentStyleSheet: sheet.parentStyleSheet === null,
      ownerRule: sheet.ownerRule === null,
      title: sheet.title,
      disabled: sheet.disabled,
    };
    const media = sheet.media;
    const mediaInitial = {
      tag: Object.prototype.toString.call(media),
      ownKeys: keys(media),
      prototypeKeys: keys(MediaList.prototype),
      names: Array.from(media),
      text: String(media),
      missing: media.item(99),
    };
    media.appendMedium("screen");
    const duplicate = Array.from(media);
    media.deleteMedium("print");
    sheet.media = "print";
    const mediaMutation = {
      duplicate,
      stable: sheet.media === media,
      text: media.mediaText,
      missingDelete: safe(() => {
        media.deleteMedium("speech");
        return "accepted";
      }),
    };
    sheet.disabled = false;
    const rules = sheet.cssRules;
    const firstIndex = sheet.insertRule("a { color: red; }");
    const removed = rules[0];
    sheet.deleteRule(0);
    const deletedParent = removed.parentStyleSheet === null;
    sheet.replaceSync('@import "unfetched.css"; sentinel { color: blue; }');
    const mutation = {
      firstIndex,
      deletedParent,
      stableRules: rules === sheet.cssRules,
      rulesAlias: sheet.rules === rules,
      disabled: sheet.disabled,
      replacement: Array.from(rules, (rule) => rule.cssText),
      invalidNeighbor: safe(() => {
        sheet.insertRule("??? {} valid {}");
        return "accepted";
      }),
      constructedImport: safe(() => {
        sheet.insertRule('@import "unfetched.css";');
        return "accepted";
      }),
    };
    const style = document.createElement("style");
    style.title = "sheet-contract";
    style.media = "screen";
    style.textContent = "owned {}";
    document.head.appendChild(style);
    try {
      const owned = style.sheet;
      const ownedState = {
        ownerNode: owned.ownerNode === style,
        href: owned.href,
        title: owned.title,
        media: owned.media.mediaText,
        replacement: safe(() => {
          owned.replaceSync("blocked {}");
          return "accepted";
        }),
      };
      style.title = "changed";
      style.media = "print";
      ownedState.refreshedTitle = owned.title;
      ownedState.refreshedMedia = owned.media.mediaText;
      owned.insertRule("inserted {}", owned.cssRules.length);
      ownedState.source = style.textContent;
      style.remove();
      ownedState.detachedSheet = style.sheet === null;
      ownedState.detachedOwner = owned.ownerNode === null;
      return { members, initial, mediaInitial, mediaMutation, mutation, owned: ownedState };
    } finally {
      style.remove();
    }
  });
  data.cssDeclarations = safe(() => {
    const node = document.createElement("div"),
      styleNode = document.createElement("style");
    node.setAttribute("style", "color: red; width: 12px;");
    styleNode.textContent = "sentinel { color: blue; }";
    document.body.appendChild(node);
    document.head.appendChild(styleNode);
    try {
      const inline = node.style,
        rule = styleNode.sheet.cssRules[0],
        ruleStyle = rule.style;
      const members = {};
      for (const name of [
        "cssText",
        "length",
        "parentRule",
        "cssFloat",
        "getPropertyValue",
        "getPropertyPriority",
        "item",
        "removeProperty",
        "setProperty",
      ]) {
        const d = Object.getOwnPropertyDescriptor(CSSStyleDeclaration.prototype, name);
        const callback = d?.get || d?.value;
        members[name] = {
          descriptor: descriptor(d),
          owner: owner(inline, name),
          invalidReceiver: safe(() => ({
            returned: typeof Reflect.apply(callback, {}, ["color", "red"]),
          })),
        };
      }
      const initial = {
        inlineTag: Object.prototype.toString.call(inline),
        ruleTag: Object.prototype.toString.call(ruleStyle),
        samePrototype: Object.getPrototypeOf(inline) === Object.getPrototypeOf(ruleStyle),
        prototypeKeys: keys(CSSStyleDeclaration.prototype),
        stableInline: inline === node.style,
        stableRule: ruleStyle === rule.style,
        numericKeys: keys(inline).filter((name) => /^[0-9]+$/.test(name)),
        privateKeys: keys(inline).filter((name) => name.startsWith("__")),
        colorDescriptor: descriptor(Object.getOwnPropertyDescriptor(inline, "color")),
        colorOwner: owner(inline, "color"),
        color: inline.color,
        text: inline.cssText,
        inlineParent: inline.parentRule === null,
        ruleParent: ruleStyle.parentRule === rule,
        names: Array.from(inline),
        missingItem: inline.item(99),
      };
      inline.setProperty("height", "20px", "important");
      const mutation = {
        value: inline.height,
        priority: inline.getPropertyPriority("height"),
        attribute: node.getAttribute("style"),
      };
      inline.width = "garbage";
      mutation.invalidValuePreserved = inline.width === "12px";
      node.setAttribute("style", "color: green;");
      const refreshed = {
        stable: inline === node.style,
        color: inline.color,
        width: inline.width,
        length: inline.length,
      };
      ruleStyle.color = "red";
      const ruleMutation = {
        text: rule.cssText,
        source: styleNode.textContent,
        stable: ruleStyle === rule.style,
      };
      const computed = getComputedStyle(node);
      const computedContract = {
        prototype: Object.getPrototypeOf(computed) === CSSStyleDeclaration.prototype,
        readonly: safe(() => {
          computed.setProperty("color", "blue");
          return "accepted";
        }),
        text: computed.cssText,
      };
      return { members, initial, mutation, refreshed, ruleMutation, computed: computedContract };
    } finally {
      node.remove();
      styleNode.remove();
    }
  });
  data.eventContracts = safe(() => {
    const event = new Event("api-shape-event", { bubbles: true, cancelable: true, composed: true });
    const ownKeys = keys(event);
    const accessors = {};
    for (const name of [
      "type",
      "target",
      "currentTarget",
      "eventPhase",
      "bubbles",
      "cancelable",
      "defaultPrevented",
      "composed",
      "timeStamp",
      "srcElement",
      "returnValue",
      "cancelBubble",
      "isTrusted",
    ]) {
      accessors[name] = {
        owner: owner(event, name),
        invalidReceiver: safe(() => {
          const d =
            name === "isTrusted"
              ? Object.getOwnPropertyDescriptor(event, name)
              : Object.getOwnPropertyDescriptor(Event.prototype, name);
          return { returned: typeof Reflect.apply(d.get, {}, []) };
        }),
      };
    }
    const methods = {};
    for (const name of [
      "composedPath",
      "preventDefault",
      "stopPropagation",
      "stopImmediatePropagation",
      "initEvent",
    ]) {
      methods[name] = {
        shape: shape(event[name]),
        invalidReceiver: safe(() => ({
          returned: typeof Reflect.apply(event[name], {}, ["other"]),
        })),
      };
    }
    event.__trusted = true;
    const flagCannotTrust = event.isTrusted === false;
    delete event.__trusted;
    const bus = new EventTarget();
    let during;
    bus.addEventListener(event.type, (received) => {
      during = {
        target: received.target === bus,
        current: received.currentTarget === bus,
        phase: received.eventPhase,
        path: received.composedPath().length,
      };
      received.preventDefault();
    });
    const returned = bus.dispatchEvent(event);
    const message = new MessageEvent("message", { data: 17, origin: "https://example.test" });
    return {
      ownKeys,
      accessors,
      methods,
      flagCannotTrust,
      tag: Object.prototype.toString.call(event),
      detailAbsent: !("detail" in event),
      timestampRelative: event.timeStamp >= 0 && event.timeStamp <= performance.now(),
      during,
      after: {
        returned,
        defaultPrevented: event.defaultPrevented,
        current: event.currentTarget,
        phase: event.eventPhase,
        path: event.composedPath().length,
      },
      message: {
        base: message instanceof Event,
        ownKeys: keys(message),
        tag: Object.prototype.toString.call(message),
        data: message.data,
        origin: message.origin,
        frozenPorts: Object.isFrozen(message.ports),
        dataOwner: owner(message, "data"),
      },
    };
  });
  data.exceptionContracts = safe(() => {
    const exception = new DOMException("bad", "SyntaxError");
    const accessors = {};
    for (const name of ["name", "message", "code"]) {
      const d = Object.getOwnPropertyDescriptor(DOMException.prototype, name);
      accessors[name] = {
        descriptor: descriptor(d),
        invalidReceiver: safe(() => ({ returned: typeof Reflect.apply(d.get, {}, []) })),
      };
    }
    return {
      ownKeys: keys(exception),
      tag: Object.prototype.toString.call(exception),
      errorPrototype: Object.getPrototypeOf(DOMException.prototype) === Error.prototype,
      accessors,
      name: exception.name,
      message: exception.message,
      code: exception.code,
    };
  });
  data.dateContracts = safe(() => {
    class DerivedDate extends Date {}
    const instance = new DerivedDate(42);
    return {
      callable: typeof Date() === "string",
      constructor: new Date(42).getTime() === 42,
      subclass:
        instance instanceof DerivedDate && instance instanceof Date && instance.getTime() === 42,
      backlink: new Date(42).constructor === Date,
      parse: Date.parse("2000-01-01T00:00:00Z") === 946684800000,
      utc: Date.UTC(2000, 0, 1) === 946684800000,
    };
  });
  data.performanceContracts = safe(() => {
    const prefix = "__turbo_api_timing_probe_";
    try {
      const source = { value: 1 };
      const mark = performance.mark(prefix + "start", { startTime: 0, detail: source });
      source.value = 9;
      performance.mark(prefix + "start", { startTime: 5 });
      performance.mark(prefix + "end", { startTime: 12 });
      const measure = performance.measure(prefix + "delta", prefix + "start", prefix + "end");
      const duration = performance.measure(prefix + "duration", {
        start: 3,
        duration: 4,
        detail: 0,
      });
      const detached = new PerformanceMark(prefix + "detached", { startTime: 2 });
      const counts = performance.eventCounts;
      return {
        mark: {
          tag: Object.prototype.toString.call(mark),
          ownKeys: keys(mark),
          base: mark instanceof PerformanceEntry,
          startTime: mark.startTime,
          detail: mark.detail,
          duration: mark.duration,
          prototypeKeys: keys(PerformanceMark.prototype),
        },
        measure: {
          tag: Object.prototype.toString.call(measure),
          ownKeys: keys(measure),
          base: measure instanceof PerformanceEntry,
          startTime: measure.startTime,
          duration: measure.duration,
          prototypeKeys: keys(PerformanceMeasure.prototype),
        },
        duration: {
          startTime: duration.startTime,
          duration: duration.duration,
          detail: duration.detail,
        },
        query: {
          marks: performance.getEntriesByName(prefix + "start", "mark").length,
          detached: performance.getEntriesByName(detached.name).length,
        },
        errors: [
          safe(() => performance.mark(prefix + "bad", { startTime: -1 })),
          safe(() => performance.mark("navigationStart")),
          safe(() => performance.mark(prefix + "clone", { detail: () => {} })),
          safe(() => performance.measure(prefix + "bad", prefix + "missing")),
          safe(() => performance.measure(prefix + "bad", { start: 1, end: 2, duration: 1 })),
        ],
        navigation: {
          integer: Number.isInteger(mark.navigationId),
          positive: mark.navigationId > 0,
          shared: [
            measure,
            duration,
            detached,
            performance.getEntriesByType("navigation")[0],
          ].every((entry) => entry.navigationId === mark.navigationId),
          serialized: mark.toJSON().navigationId === mark.navigationId,
          jsonKeys: Object.keys(mark.toJSON()).sort(),
        },
        entryPrototypeKeys: keys(PerformanceEntry.prototype),
        eventCounts: {
          tag: Object.prototype.toString.call(counts),
          ownKeys: keys(counts),
          size: counts.size,
          names: Array.from(counts.keys()).sort(),
        },
        interactionCount: performance.interactionCount,
      };
    } finally {
      for (const suffix of ["start", "end", "detached", "bad", "clone"])
        performance.clearMarks(prefix + suffix);
      for (const suffix of ["delta", "duration", "bad"]) performance.clearMeasures(prefix + suffix);
    }
  });
  return JSON.stringify(data);
})();
