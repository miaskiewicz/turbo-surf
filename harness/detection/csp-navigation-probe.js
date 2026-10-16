(async () => {
  const doc = document,
    rows = [],
    referrer = doc.referrer;
  doc.addEventListener("securitypolicyviolation", (event) =>
    rows.push({
      referrer: event.referrer,
      status: event.statusCode,
      document: event.documentURI,
      blocked: event.blockedURI,
      source: event.sourceFile,
      line: event.lineNumber,
      column: event.columnNumber,
      target: event.target.nodeName,
      trusted: event.isTrusted,
    }),
  );
  Object.defineProperty(doc, "referrer", {
    get() {
      throw new Error("guest referrer getter");
    },
  });
  Object.defineProperty(doc, "URL", {
    get() {
      throw new Error("guest URL getter");
    },
  });
  const style = doc.createElement("style");
  style.textContent = ".metadata{color:red}";
  doc.head.append(style);
  await new Promise((resolve) => setTimeout(resolve, 100));
  return { referrer, location: location.protocol === "data:" ? "data:" : location.href, rows };
})();
//# sourceURL=https://csp-probe.test/navigation-probe.js
