// Shared, opt-in interface tracer for Chrome and the native isolate. Descriptor wrappers
// keep native receivers intact. Wrapping still changes observable function identity: always
// compare with an uninstrumented control. This records actual page reads, not a test collector.
(() => {
  const key = Symbol.for("turbo-surf.interface-trace");
  if (globalThis[key]) return;
  const rows = new Map();
  const installed = [];
  const skipped = [];
  const coverage = new Map();
  let droppedCoverage = 0;
  const cover = (name, scope, status, reason) => {
    const id = `${name}:${scope}:${status}:${reason || ""}`;
    const old = coverage.get(id);
    if (old) old.count++;
    else if (coverage.size < 3000) coverage.set(id, { name, scope, status, reason, count: 1 });
    else droppedCoverage++;
  };
  const now = performance.now.bind(performance);
  const ErrorCtor = Error;
  const apply = Reflect.apply;
  let recording = true; // Do not count the installer's own interface inspection.
  let dropped = 0;
  const describe = (v) => {
    if (v === undefined) return { type: "undefined" };
    if (v === null || typeof v === "boolean" || typeof v === "number") return v;
    if (typeof v === "string") return { type: "string", length: v.length, sample: v.slice(0, 160) };
    if (typeof v === "function") return { type: "function" };
    // Do not enumerate arbitrary returned objects or call their getters/toString.
    return { type: typeof v };
  };
  const record = (name, kind, args, result, error, durationMs) => {
    if (recording) return;
    recording = true;
    try {
      const stack = new ErrorCtor().stack?.split("\n").slice(3, 9).join("\n");
      const caller = stack?.split("\n")[0] || "";
      // Native host pumps are implemented in JS. Preserve their observations separately
      // so polling/loading does not masquerade as target-script interface queries.
      // Source-less execScriptEl frames occur in the native module bootstrap. Match
      // only the immediate frame: real page code can have this host in its ancestry.
      const origin =
        /<(?:env|rs-[^>]*|initial-hydrate|hydrate|pending|dom-bootstrap|frame-resources|turbo-cdp-host|frame-init|session-eval)>/.test(
          caller,
        ) || /^\s*at (?:Object\.)?__G\.execScriptEl \(<anonymous>:\d+:\d+\)$/.test(caller)
          ? "runtime"
          : "page";
      const id = `${origin}:${name}:${kind}`;
      const old = rows.get(id);
      if (old) {
        old.count++;
        old.totalDurationMs += durationMs;
        old.maxDurationMs = Math.max(old.maxDurationMs, durationMs);
        const sample = {
          durationMs,
          args: args.map(describe),
          result: describe(result),
          error: error ? String(error) : null,
        };
        const encoded = JSON.stringify(sample);
        if (old.samples.length < 12 && !old.samples.some((s) => JSON.stringify(s) === encoded))
          old.samples.push(sample);
      } else if (rows.size < 3000) {
        rows.set(id, {
          name,
          kind,
          origin,
          count: 1,
          durationMs,
          totalDurationMs: durationMs,
          maxDurationMs: durationMs,
          at: now(),
          args: args.map(describe),
          result: describe(result),
          error: error ? String(error) : null,
          samples: [],
          stack,
        });
      } else dropped++;
    } finally {
      recording = false;
    }
  };
  const wrap = (fn, name, kind) => {
    // Concise methods are non-constructible, like DOM methods/getters.
    const out = {
      invoke(...args) {
        let result;
        let durationMs;
        const started = now();
        try {
          try {
            result = apply(fn, this, args);
          } finally {
            durationMs = now() - started;
          }
        } catch (error) {
          record(name, kind, args, undefined, error, durationMs);
          throw error;
        }
        record(name, kind, args, result, null, durationMs);
        // Descriptor inspection must not replace a successful page call with a tracer
        // failure. These are opt-in wrappers, not proxies: returned identities stay intact.
        const previous = recording;
        recording = true;
        try {
          instrumentResult(name, kind, args, result);
        } catch {
          cover(name, "return", "skipped", "inspection-threw");
        } finally {
          recording = previous;
        }
        return result;
      },
    }.invoke;
    Object.defineProperty(out, "name", { value: fn.name, configurable: true });
    Object.defineProperty(out, "length", { value: fn.length, configurable: true });
    return out;
  };
  const seen = new WeakMap();
  const instrument = (object, label, names, scope) => {
    if (!object) return;
    for (const prop of names || Object.getOwnPropertyNames(object)) {
      if (prop === "constructor") continue;
      let owner = object;
      let depth = 0;
      while (owner && !Object.hasOwn(owner, prop)) {
        if (depth++ === 32) {
          if (scope) cover(`${label}.${prop}`, scope, "skipped", "descriptor-depth-limit");
          owner = null;
          break;
        }
        owner = Object.getPrototypeOf(owner);
      }
      if (!owner) {
        if (scope) cover(`${label}.${prop}`, scope, "unavailable", "missing-descriptor");
        continue;
      }
      const done = seen.get(owner) || new Set();
      if (done.has(prop)) continue;
      seen.set(owner, done);
      done.add(prop);
      const d = Object.getOwnPropertyDescriptor(owner, prop);
      if (!d) {
        if (scope) cover(`${label}.${prop}`, scope, "unavailable", "missing-descriptor");
        continue;
      }
      if (!d.configurable) {
        skipped.push(`${label}.${prop}`);
        if (scope) cover(`${label}.${prop}`, scope, "skipped", "nonconfigurable");
        continue;
      }
      const updated = { ...d };
      if (d.get) updated.get = wrap(d.get, `${label}.${prop}`, "get");
      if (d.set && (prop === "cookie" || prop === "src"))
        updated.set = wrap(d.set, `${label}.${prop}`, "set");
      if (typeof d.value === "function") updated.value = wrap(d.value, `${label}.${prop}`, "call");
      if (updated.get === d.get && updated.set === d.set && updated.value === d.value) {
        if (scope) cover(`${label}.${prop}`, scope, "unobserved", "data-property-or-setter");
        continue;
      }
      try {
        Object.defineProperty(owner, prop, updated);
        installed.push(`${label}.${prop}`);
        if (scope) cover(`${label}.${prop}`, scope, "installed");
      } catch (e) {
        skipped.push(`${label}.${prop}: ${e}`);
        if (scope) cover(`${label}.${prop}`, scope, "skipped", "define-property-threw");
      }
    }
  };
  const contexts = new WeakSet();
  const instrumentSurface = (object, label) => {
    if (contexts.has(object)) return;
    contexts.add(object);
    let owner = object;
    let depth = 0;
    while (owner && owner !== Object.prototype) {
      const parent = Object.getPrototypeOf(owner);
      // A foreign realm has a different Object.prototype. Do not patch its root,
      // even if a replaced getContext returns that root directly. Read descriptors
      // rather than invoking a page-defined constructor/name/prototype getter.
      if (parent === null) {
        const constructor = Object.getOwnPropertyDescriptor(owner, "constructor")?.value;
        const intrinsicRoot =
          typeof constructor === "function" &&
          Object.getOwnPropertyDescriptor(constructor, "name")?.value === "Object" &&
          Object.getOwnPropertyDescriptor(constructor, "prototype")?.value === owner;
        if (owner !== object || intrinsicRoot) {
          cover(label, "return", "unobserved", "object-prototype-root");
          break;
        }
      }
      if (depth++ === 32) {
        cover(label, "return", "skipped", "prototype-depth-limit");
        return;
      }
      instrument(owner, label, undefined, owner === object ? "own" : "prototype");
      owner = parent;
    }
    cover(label, "return", "inspected");
  };
  const contextLabels = new Map([
    ["webgl", "WebGLRenderingContext"],
    ["experimental-webgl", "WebGLRenderingContext"],
    ["webgl2", "WebGL2RenderingContext"],
    ["experimental-webgl2", "WebGL2RenderingContext"],
    ["2d", "CanvasRenderingContext2D"],
  ]);
  const instrumentResult = (name, kind, args, result) => {
    if (kind !== "call" && kind !== "get") return;
    if (kind === "call" && /^(?:HTMLCanvasElement|OffscreenCanvas)\.getContext$/.test(name)) {
      const label = contextLabels.get(args[0]);
      if (!label) {
        // Do not coerce the kind again: a page-supplied toString may have side effects.
        cover(name, "return", "unobserved", "unrecognized-context-kind");
      } else if (result && typeof result === "object") instrumentSurface(result, label);
      else cover(label, "return", "unavailable", "null-or-nonobject-context");
    } else if (
      kind === "call" &&
      /^(?:WebGLRenderingContext|WebGL2RenderingContext)\.getExtension$/.test(name)
    ) {
      if (result && typeof result === "object") instrumentSurface(result, "WebGLExtension");
      else cover("WebGLExtension", "return", "unavailable", "null-or-nonobject-extension");
    } else if (result && typeof result === "object" && /\.createElement(?:NS)?$/.test(name)) {
      const tag = args[name.endsWith("NS") ? 1 : 0];
      if (typeof tag === "string" && tag.toLowerCase() === "canvas")
        instrument(result, "HTMLCanvasElement", ["getContext"], "returned-canvas");
    } else if (
      result &&
      typeof result === "object" &&
      ((kind === "get" && /^(?:HTMLElement|Element|CSSStyleRule)\.style$/.test(name)) ||
        name === "window.getComputedStyle")
    ) {
      // Native declarations may expose supported methods as own properties.
      instrument(result, "CSSStyleDeclaration", [
        "getPropertyValue",
        "getPropertyPriority",
        "setProperty",
        "removeProperty",
        "item",
        "length",
        "cssText",
      ]);
    }
  };
  Object.defineProperty(globalThis, key, {
    value: {
      snapshot: () => ({
        accesses: [...rows.values()],
        installed,
        skipped,
        dropped,
        coverage: [...coverage.values()],
        droppedCoverage,
        coverageScope: {
          gpu: "observed-getContext-results-and-returned-extensions",
          ownDataReads: false,
          symbolMembers: false,
          previouslyCapturedFunctions: false,
          lateReplacements: false,
          changesFunctionIdentity: true,
        },
      }),
    },
  });
  for (const name of [
    "Navigator",
    "WorkerNavigator",
    "WorkerGlobalScope",
    "DedicatedWorkerGlobalScope",
    "Screen",
    "Document",
    "HTMLDocument",
    "Performance",
    "HTMLCanvasElement",
    "OffscreenCanvas",
    "CanvasRenderingContext2D",
    "WebGLRenderingContext",
    "WebGL2RenderingContext",
    "OfflineAudioContext",
    "AudioContext",
    "AudioBuffer",
    "AnalyserNode",
    "Permissions",
    "NavigatorUAData",
    "Storage",
    "XMLHttpRequest",
    "Element",
    "HTMLElement",
    "CSSStyleSheet",
    "CSSStyleRule",
    "CSSRule",
    "CSSStyleDeclaration",
    "HTMLIFrameElement",
    "Event",
    "MouseEvent",
    "KeyboardEvent",
    "InputEvent",
    "EventTarget",
  ]) {
    const prototype = globalThis[name]?.prototype;
    const scope =
      /^(?:HTMLCanvasElement|OffscreenCanvas|CanvasRenderingContext2D|WebGL2?RenderingContext)$/.test(
        name,
      )
        ? "prototype"
        : undefined;
    if (scope && !prototype) cover(name, scope, "unavailable", "missing-interface");
    instrument(prototype, name, undefined, scope);
  }
  // Some native shims have own methods rather than prototype methods.
  instrument(globalThis.navigator, "navigator");
  instrument(globalThis.screen, "screen");
  instrument(globalThis.performance, "performance");
  instrument(globalThis.navigator?.permissions, "permissions");
  instrument(globalThis.navigator?.userAgentData, "userAgentData");
  instrument(globalThis.document, "document", [
    "cookie",
    "readyState",
    "contentType",
    "characterSet",
    "currentScript",
    "styleSheets",
    "createElement",
    "createElementNS",
    "querySelector",
    "querySelectorAll",
    "getElementById",
  ]);
  instrument(globalThis, "window", [
    "fetch",
    "getComputedStyle",
    "matchMedia",
    "requestAnimationFrame",
    "innerWidth",
    "innerHeight",
    "outerWidth",
    "outerHeight",
    "devicePixelRatio",
  ]);
  instrument(Object, "Object", [
    "getOwnPropertyDescriptor",
    "getOwnPropertyDescriptors",
    "getOwnPropertyNames",
  ]);
  instrument(Function.prototype, "Function.prototype", ["toString"]);
  instrument(Date, "Date", ["now"]);
  instrument(Intl.DateTimeFormat.prototype, "Intl.DateTimeFormat", ["resolvedOptions"]);
  recording = false;
})();
