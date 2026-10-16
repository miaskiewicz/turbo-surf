(async () => {
  const rows = [],
    errors = [];
  const units = (text) => Array.from({ length: text.length }, (_, i) => text.charCodeAt(i));
  const listener = (event) =>
    rows.push({
      target: event.target === document ? "document" : event.target.id || event.target.nodeName,
      blocked: event.blockedURI.replace(/http:\/\/(127\.0\.0\.1|localhost):\d+/g, "<origin>"),
      directive: event.effectiveDirective,
      disposition: event.disposition,
      source: event.sourceFile.replace(/http:\/\/(127\.0\.0\.1|localhost):\d+/g, "<origin>"),
      line: event.lineNumber,
      column: event.columnNumber,
      sampleUnits: units(event.sample),
      status: event.statusCode,
      referrer: event.referrer,
      trusted: event.isTrusted,
    });
  document.addEventListener("securitypolicyviolation", listener);
  for (const [id, type] of [
    ["externalClassic", ""],
    ["externalModule", "module"],
  ]) {
    const script = document.createElement("script");
    script.id = id;
    script.type = type;
    script.src = new URL("/" + id + ".js", location.href);
    document.body.append(script);
  }
  const image = new Image();
  image.id = "externalImage";
  Object.defineProperty(image, "ownerDocument", {
    get() {
      throw new Error("guest image owner");
    },
  });
  image.src = new URL("/blocked.png", location.href);
  document.body.append(image);
  for (const [id, text] of [
    ["emoji", "/*" + "😀".repeat(24) + "*/"],
    ["splitSurrogate", "a".repeat(39) + "😀x"],
    ["loneUnits", "/*" + String.fromCharCode(0xd800) + "x" + String.fromCharCode(0xdc00) + "*/"],
    ["whitespace", " \t\n" + "b".repeat(45) + " \n"],
    ["nonAsciiWhitespace", "\u00a0\ufeff" + "c".repeat(41)],
    ["verticalTab", "\v" + "d".repeat(41)],
    ["prefixWhitespace", "e".repeat(39) + " \n\t" + "f".repeat(10)],
  ]) {
    const script = document.createElement("script");
    script.id = id;
    script.textContent = text;
    document.body.append(script);
    const style = document.createElement("style");
    style.id = id + "Style";
    style.textContent = text;
    document.head.append(style);
  }
  try {
    eval("globalThis.evalRan=true");
  } catch (error) {
    errors.push(error.name);
  }
  try {
    new Function("globalThis.functionRan=true")();
  } catch (error) {
    errors.push(error.name);
  }
  await new Promise((resolve) => setTimeout(resolve, 200));
  document.removeEventListener("securitypolicyviolation", listener);
  return { rows, errors };
})();
//# sourceURL=https://csp-probe.test/resource-probe.js
