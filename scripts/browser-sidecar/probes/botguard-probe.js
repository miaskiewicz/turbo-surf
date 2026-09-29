// BotGuard/SearchGuard integrity-trap probe. A self-contained JS expression returning a JSON
// string; run it in BOTH real Chrome and the turbo-surf isolate and diff, to see which traps we
// still fail. Focuses on the checks the research surfaced: the toString anti-hook, the chronometric
// trap (performance.now vs Date.now), native-fn SHAPE (native fns have no own `prototype`), and
// the env/property-count surface (window/navigator/document).
//
//   real Chrome : node scripts/browser-sidecar/run-probe.mjs scripts/browser-sidecar/probes/botguard-probe.js [url]
//   turbo-surf  : cargo run -p turbo-surf-mcp --features gpu-metal --example fp_snapshot -- scripts/browser-sidecar/probes/botguard-probe.js
//
// Reusable + extend as we iterate: add a trap, re-run both sides, diff.
(function () {
  const S = (fn) => {
    try {
      const v = fn();
      return v === undefined ? "<undef>" : v;
    } catch (e) {
      return "<throw:" + (e && e.name) + ">";
    }
  };
  const g = typeof globalThis !== "undefined" ? globalThis : window;
  const FPT = Function.prototype.toString;
  const isNative = (f) => S(() => FPT.call(f).includes("[native code]"));
  // A JS function masquerading as native leaks via: it still has an own `prototype` property
  // (real native methods don't), a `.length`/`.name` mismatch, or ownKeys beyond name/length.
  const hasProto = (f) => S(() => Object.prototype.hasOwnProperty.call(f, "prototype"));
  const ownKeys = (f) => S(() => Object.getOwnPropertyNames(f).sort().join(","));

  // --- toString anti-hook probe ---
  const toStringTrap = {
    fpt_selfNative: isNative(FPT), // FPT.toString() → native?
    fpt_callSelf: S(() => FPT.call(FPT).slice(0, 40)), // FPT.call(FPT)
    fpt_hasPrototype: hasProto(FPT), // native FPT has NO own prototype → false
    fpt_length: S(() => FPT.length), // native → 0
    fpt_name: S(() => FPT.name), // "toString"
    fpt_ownKeys: ownKeys(FPT), // native → "length,name"
    // A guaranteed-native builtin for comparison (Array.prototype.push).
    push_native: isNative([].push),
    push_hasPrototype: hasProto([].push), // false on real
    push_ownKeys: ownKeys([].push),
  };

  // --- native-fn SHAPE of our shimmed builtins (the real detection vector) ---
  const shimShape = {};
  const probe = (label, get) => {
    const f = S(get);
    if (typeof f !== "function") {
      shimShape[label] = "<" + f + ">";
      return;
    }
    shimShape[label] = { native: isNative(f), hasProto: hasProto(f), keys: ownKeys(f) };
  };
  probe("canvas.toDataURL", () => g.document.createElement("canvas").toDataURL);
  probe("canvas.getContext", () => g.document.createElement("canvas").getContext);
  probe("gl.getParameter", () => {
    const c = g.document.createElement("canvas").getContext("webgl");
    return c && c.getParameter;
  });
  probe("navigator.sendBeacon", () => g.navigator.sendBeacon);
  probe(
    "navigator.permissions.query",
    () => g.navigator.permissions && g.navigator.permissions.query,
  );
  probe("performance.now", () => g.performance.now);
  probe("Date.now", () => Date.now);
  probe("fetch", () => g.fetch);
  probe("setTimeout", () => g.setTimeout);
  probe("addEventListener", () => g.addEventListener);
  probe("requestAnimationFrame", () => g.requestAnimationFrame);

  // --- chronometric trap: performance.now vs Date.now ---
  const chrono = S(() => {
    const p0 = performance.now(),
      d0 = Date.now();
    // busy ~ a few ms
    let x = 0;
    const start = performance.now();
    while (performance.now() - start < 5) x++;
    const p1 = performance.now(),
      d1 = Date.now();
    // resolution: smallest non-zero delta between consecutive perf.now()
    let minDelta = Infinity,
      prev = performance.now();
    for (let i = 0; i < 5000; i++) {
      const t = performance.now();
      const d = t - prev;
      if (d > 0 && d < minDelta) minDelta = d;
      prev = t;
    }
    return {
      perfAdvanced: p1 - p0 > 0,
      dateAdvanced: d1 - d0 >= 0,
      perfVsDate_ratio: +((p1 - p0) / Math.max(d1 - d0, 0.001)).toFixed(2), // ~1.0 if coherent
      perfFractional: performance.now() % 1 !== 0, // Chrome: fractional
      minResolution: minDelta === Infinity ? -1 : +minDelta.toFixed(4), // Chrome clamps ~0.005–0.1ms
      timeOriginType: typeof performance.timeOrigin,
      nowLEQtimeOriginPlusUptime: performance.now() >= 0,
    };
  });

  // --- environment surface / property counts (the "100+ properties") ---
  const counts = S(() => ({
    windowOwnProps: Object.getOwnPropertyNames(g).length,
    navigatorProtoProps: Object.getOwnPropertyNames(Object.getPrototypeOf(g.navigator)).length,
    documentProtoProps: Object.getOwnPropertyNames(Object.getPrototypeOf(g.document)).length,
    naveratorInstProps: Object.getOwnPropertyNames(g.navigator).length,
    hasChrome: typeof g.chrome,
    chromeKeys: g.chrome ? Object.getOwnPropertyNames(g.chrome).sort().join(",") : "<none>",
    webdriver: g.navigator.webdriver,
  }));

  // --- misc anti-tamper env ---
  const env = S(() => ({
    // eval must be native; a Proxy/Reflect tamper leaks.
    evalNative: isNative(g.eval),
    // toStringTag spoofing check on window/navigator.
    windowTag: Object.prototype.toString.call(g),
    // error stack shape.
    errStack: (() => {
      try {
        null.x;
      } catch (e) {
        return String(e.stack || "").split("\n").length;
      }
    })(),
    // Reflect + Proxy present (VM uses them).
    hasProxy: typeof g.Proxy,
    hasReflect: typeof g.Reflect,
  }));

  return JSON.stringify({ toStringTrap, shimShape, chrono, counts, env });
})();
