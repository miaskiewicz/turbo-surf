(async () => {
  const rows = [],
    order = [];
  const listener = (event) => {
    order.push("violation:" + event.effectiveDirective);
    rows.push({
      target: event.target === document ? "document" : event.target.id || event.target.nodeName,
      brand: Object.prototype.toString.call(event),
      trusted: event.isTrusted,
      bubbles: event.bubbles,
      cancelable: event.cancelable,
      composed: event.composed,
      documentURI: event.documentURI.replace(/http:\/\/(127\.0\.0\.1|localhost):\d+/g, "<origin>"),
      referrer: event.referrer,
      blockedURI: event.blockedURI.replace(/http:\/\/(127\.0\.0\.1|localhost):\d+/g, "<origin>"),
      violatedDirective: event.violatedDirective,
      effectiveDirective: event.effectiveDirective,
      originalPolicy: event.originalPolicy,
      disposition: event.disposition,
      sourceFile: event.sourceFile.replace(/http:\/\/(127\.0\.0\.1|localhost):\d+/g, "<origin>"),
      statusCode: event.statusCode,
      lineNumber: event.lineNumber,
      columnNumber: event.columnNumber,
      sample: event.sample,
    });
  };
  document.addEventListener("securitypolicyviolation", listener);
  const script = document.createElement("script");
  script.id = "blockedScript";
  script.textContent = "globalThis.__cspExecuted=true";
  document.body.append(script);
  const style = document.createElement("style");
  style.id = "blockedStyle";
  style.textContent = "body{color:red}";
  document.head.append(style);
  const el = document.createElement("div");
  el.id = "blockedAttribute";
  el.setAttribute("style", "color:blue");
  document.body.append(el);
  order.push("sync");
  Promise.resolve().then(() => order.push("promise"));
  await fetch(new URL("/denied", location.href)).catch(() => {});
  await new Promise((resolve) => setTimeout(resolve, 200));
  document.removeEventListener("securitypolicyviolation", listener);
  script.remove();
  style.remove();
  el.remove();
  return { rows, order, executed: globalThis.__cspExecuted === true };
})();
//# sourceURL=https://csp-probe.test/security-probe.js
