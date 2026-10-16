(async () => {
  const mode = document.documentElement.dataset.case,
    trace = [],
    violations = [];
  document.addEventListener("securitypolicyviolation", (e) =>
    violations.push({
      effectiveDirective: e.effectiveDirective,
      blockedURI: e.blockedURI,
      disposition: e.disposition,
      sample: e.sample,
    }),
  );
  if (mode.includes("default"))
    trustedTypes.createPolicy("default", {
      createScript(value, ...args) {
        trace.push([value, args]);
        if (mode.includes("throw")) throw Error("authored callback");
        return mode.includes("null")
          ? null
          : mode.includes("change")
            ? value + " /*changed*/"
            : value;
      },
    });
  const policy = trustedTypes.createPolicy("alpha", { createScript: (s) => s });
  const typed = policy.createScript("lexical + 2"),
    typedGlobal = policy.createScript("globalThis.ttGlobal + 3");
  const error = (run) => {
    try {
      const value = run();
      return {
        ok: true,
        value:
          value !== null && typeof value === "object"
            ? {
                type: typeof value,
                tag: Object.prototype.toString.call(value),
                text: String(value),
                same: value === typed || value === typedGlobal,
              }
            : value === undefined
              ? null
              : value,
      };
    } catch (e) {
      return { error: e.name, message: e.message };
    }
  };
  globalThis.ttGlobal = 20;
  // The lexical binding is read by native direct eval in the authored rows.
  // eslint-disable-next-line no-unused-vars
  const lexical = 10;
  const object = {
    toString() {
      throw Error("should not coerce");
    },
  };
  const rows = [
    ["eval-string", error(() => eval("lexical + 1"))],
    ["eval-typed", error(() => eval(typed))],
    ["eval-object", error(() => eval(object) === object)],
    ["indirect-typed", error(() => (0, eval)(typedGlobal))],
    ["function-string", error(() => new Function("return 7")())],
    ["function-typed", error(() => new Function(policy.createScript("return 8"))())],
  ];
  globalThis.ttTimers = [];
  for (const [name, handler] of [
    ["string", 'ttTimers.push("string")'],
    ["typed", policy.createScript('ttTimers.push("typed")')],
    ["function", () => ttTimers.push("function")],
  ])
    rows.push([
      "timeout-" + name,
      error(() => {
        setTimeout(handler, 0);
        return true;
      }),
    ]);
  rows.push([
    "interval-string",
    error(() => {
      let timer = setInterval('ttTimers.push("interval");clearInterval(globalThis.ttInterval)', 0);
      globalThis.ttInterval = timer;
      return true;
    }),
  ]);
  let repeating = 0,
    cancelled = false;
  const cancel = setTimeout(() => {
    cancelled = true;
  }, 0);
  clearTimeout(cancel);
  const interval = setInterval(() => {
    repeating++;
    if (repeating === 3) clearInterval(interval);
  }, 3);
  await new Promise((resolve) => setTimeout(resolve, 80));
  return { rows, trace, violations, timers: ttTimers, repeating, cancelled };
})();
