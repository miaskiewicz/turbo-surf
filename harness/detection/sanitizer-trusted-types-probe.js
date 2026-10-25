(async () => {
  const run = (fn) => {
    try {
      const value = fn();
      return { value: value === undefined ? "<undefined>" : value };
    } catch (error) {
      return { error: error.name, message: error.message };
    }
  };
  const host = document.createElement("div");
  const parsed = (doc) => doc.body.innerHTML;
  const out = {};
  out.strings = {
    parseHTMLUnsafe: run(() => parsed(Document.parseHTMLUnsafe("<b>x</b>"))),
    parseHTMLUnsafeOptions: run(() => parsed(Document.parseHTMLUnsafe("<b>x</b>", { sanitizer: {} }))),
    parseHTMLUnsafeBadOptions: run(() => Document.parseHTMLUnsafe("<b>x</b>", 5)),
    parseHTML: run(() => parsed(Document.parseHTML("<b>x</b>"))),
    setHTML: run(() => {
      host.setHTML("<b>x</b>");
      return host.innerHTML;
    }),
    setHTMLUnsafe: run(() => host.setHTMLUnsafe("<b>x</b>")),
    shadowSetHTMLUnsafe: run(() => document.createElement("div").attachShadow({ mode: "open" }).setHTMLUnsafe("<b>x</b>")),
    domParser: run(() => new DOMParser().parseFromString("<b>x</b>", "text/html")),
  };
  const policy = trustedTypes.createPolicy("policy", { createHTML: (value) => value + "<i>p</i>" });
  out.trusted = {
    parseHTMLUnsafe: run(() => parsed(Document.parseHTMLUnsafe(policy.createHTML("<b>x</b>")))),
    parseHTMLUnsafeSanitized: run(() => parsed(Document.parseHTMLUnsafe(policy.createHTML("<b onclick=1>x</b>"), { sanitizer: { removeElements: ["i"] } }))),
    parseHTML: run(() => parsed(Document.parseHTML(policy.createHTML("<b>x</b>")))),
    setHTMLUnsafe: run(() => {
      host.setHTMLUnsafe(policy.createHTML("<b>x</b>"));
      return host.innerHTML;
    }),
    setHTML: run(() => {
      host.setHTML(policy.createHTML("<b>x</b>"));
      return host.innerHTML;
    }),
    domParser: run(() => parsed(new DOMParser().parseFromString(policy.createHTML("<b>x</b>"), "text/html"))),
  };
  const calls = [];
  trustedTypes.createPolicy("default", {
    createHTML: (value, type, sink) => {
      calls.push([value, type, sink]);
      return value.replace("x", "X");
    },
  });
  out.defaultPolicy = {
    parseHTMLUnsafe: run(() => parsed(Document.parseHTMLUnsafe("<b>x</b>"))),
    parseHTMLUnsafeConversionOrder: run(() => {
      const log = [];
      try {
        Document.parseHTMLUnsafe({ toString() { log.push("html"); return "<b>x</b>"; } }, { get sanitizer() { log.push("sanitizer"); return "bogus"; } });
      } catch (error) {
        log.push(error.name);
      }
      return [log, calls.length];
    }),
    parseHTMLUnsafeInvalidConfig: run(() => Document.parseHTMLUnsafe("<b>x</b>", { sanitizer: { elements: [], removeElements: [] } })),
    callsAfterInvalid: calls.length,
    parseHTML: run(() => parsed(Document.parseHTML("<b>x</b>"))),
    setHTML: run(() => {
      host.setHTML("<b>x</b>");
      return host.innerHTML;
    }),
    setHTMLUnsafe: run(() => {
      host.setHTMLUnsafe("<b>x</b>");
      return host.innerHTML;
    }),
    domParser: run(() => parsed(new DOMParser().parseFromString("<b>x</b>", "text/html"))),
    domParserBadType: run(() => new DOMParser().parseFromString("<b>x</b>", "bogus")),
    calls,
  };
  return out;
})()
