// Shared, opt-in interface tracer for Chrome and the native isolate. Descriptor wrappers
// keep native receivers intact. Wrapping still changes observable function identity: always
// compare with an uninstrumented control. This records actual page reads, not a test collector.
(() => {
  const key = Symbol.for("turbo-surf.interface-trace");
  if (globalThis[key]) return;
  const rows = new Map();
  const installed = [];
  const skipped = [];
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
      const origin =
        /<(?:env|rs-[^>]*|initial-hydrate|hydrate|pending|dom-bootstrap|frame-resources|turbo-cdp-host|frame-init|session-eval)>/.test(
          caller,
        )
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
        if (
          result &&
          typeof result === "object" &&
          ((kind === "get" && /^(?:HTMLElement|Element|CSSStyleRule)\.style$/.test(name)) ||
            name === "window.getComputedStyle")
        ) {
          // Native declarations may still expose supported methods as own properties.
          // Inspect only known CSS members; installer descriptor reads are not page queries.
          const previous = recording;
          recording = true;
          try {
            instrument(result, "CSSStyleDeclaration", [
              "getPropertyValue",
              "getPropertyPriority",
              "setProperty",
              "removeProperty",
              "item",
              "length",
              "cssText",
            ]);
          } finally {
            recording = previous;
          }
        }
        return result;
      },
    }.invoke;
    Object.defineProperty(out, "name", { value: fn.name, configurable: true });
    Object.defineProperty(out, "length", { value: fn.length, configurable: true });
    return out;
  };
  const seen = new WeakMap();
  const instrument = (object, label, names) => {
    if (!object) return;
    for (const prop of names || Object.getOwnPropertyNames(object)) {
      if (prop === "constructor") continue;
      let owner = object;
      while (owner && !Object.hasOwn(owner, prop)) owner = Object.getPrototypeOf(owner);
      if (!owner) continue;
      const done = seen.get(owner) || new Set();
      if (done.has(prop)) continue;
      seen.set(owner, done);
      done.add(prop);
      const d = Object.getOwnPropertyDescriptor(owner, prop);
      if (!d.configurable) {
        skipped.push(`${label}.${prop}`);
        continue;
      }
      const updated = { ...d };
      if (d.get) updated.get = wrap(d.get, `${label}.${prop}`, "get");
      if (d.set && (prop === "cookie" || prop === "src"))
        updated.set = wrap(d.set, `${label}.${prop}`, "set");
      if (typeof d.value === "function") updated.value = wrap(d.value, `${label}.${prop}`, "call");
      if (updated.get === d.get && updated.set === d.set && updated.value === d.value) continue;
      try {
        Object.defineProperty(owner, prop, updated);
        installed.push(`${label}.${prop}`);
      } catch (e) {
        skipped.push(`${label}.${prop}: ${e}`);
      }
    }
  };
  Object.defineProperty(globalThis, key, {
    value: {
      snapshot: () => ({ accesses: [...rows.values()], installed, skipped, dropped }),
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
    instrument(globalThis[name]?.prototype, name);
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
