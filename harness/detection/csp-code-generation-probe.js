(async () => {
  const doc = document,
    rows = [],
    results = {};
  const units = (text) => Array.from({ length: text.length }, (_, index) => text.charCodeAt(index));
  doc.addEventListener("securitypolicyviolation", (event) =>
    rows.push({
      target: event.target === doc ? "document" : event.target.nodeName,
      directive: event.effectiveDirective,
      blocked: event.blockedURI,
      disposition: event.disposition,
      source: event.sourceFile,
      line: event.lineNumber,
      column: event.columnNumber,
      sample: units(event.sample),
      trusted: event.isTrusted,
    }),
  );
  Object.defineProperty(doc, "URL", {
    get() {
      throw new Error("guest URL getter");
    },
  });
  if (new URL(location.href).searchParams.get("case") === "6") {
    const meta = doc.createElement("meta");
    meta.httpEquiv = "Content-Security-Policy";
    meta.content = "script-src 'none' 'report-sample'";
    doc.head.append(meta);
  }
  const shadow = 7; // eslint-disable-line no-unused-vars -- Direct eval reads this binding.
  const check = (name, action) => {
    try {
      results[name] = action();
    } catch (error) {
      results[name] = error.name;
    }
  };
  check("direct", () => eval("shadow + 1"));
  check("indirect", () => (0, eval)("typeof shadow"));
  check("function", () => new Function("return typeof shadow")());
  check("leadingSample", () => typeof eval("  /*" + "q".repeat(50) + "*/"));
  check("prefixOnlyWhitespace", () => typeof eval(" ".repeat(50) + "/*tail*/"));
  check("asyncFunction", () => Object.getPrototypeOf(async () => {}).constructor("return 9"));
  check("generatorFunction", () => Object.getPrototypeOf(function* () {}).constructor("yield 10"));
  const object = {
    toString() {
      throw new Error("eval object was coerced");
    },
  };
  check("objectIdentity", () => eval(object) === object);
  const boxed = new String("2 + 2");
  check("boxedIdentity", () => eval(boxed) === boxed);
  check(
    "failedConversion",
    () =>
      new Function({
        toString() {
          throw new RangeError("conversion");
        },
      }),
  );
  if (typeof results.asyncFunction === "function")
    results.asyncFunction = await results.asyncFunction();
  if (typeof results.generatorFunction === "function")
    results.generatorFunction = results.generatorFunction().next().value;
  await new Promise((resolve) => setTimeout(resolve, 100));
  return { rows, results };
})();
//# sourceURL=https://csp-probe.test/code-generation-probe.js
