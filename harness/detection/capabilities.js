// A collector run AFTER the recorded verdict. These are our queries, not Google's.
(() => {
  const safe = (fn) => {
    try {
      return fn();
    } catch (e) {
      return { error: String(e) };
    }
  };
  const style = document.createElement("style");
  style.textContent = "body { color: red; } sentinel {}";
  document.head.appendChild(style);
  const frame = document.createElement("iframe");
  document.body.appendChild(frame);
  const controls = document.createElement("div");
  controls.innerHTML =
    '<form id="trace-controls"><input type="hidden" name="token" value="parsed-token"><textarea name="text">parsed-text</textarea><select name="choice"><option value="first">First</option><option value="chosen" selected>Chosen</option></select></form>';
  document.body.appendChild(controls);
  const data = {
    navigator: safe(() => ({
      userAgent: navigator.userAgent,
      platform: navigator.platform,
      brands: navigator.userAgentData?.brands,
      hardwareConcurrency: navigator.hardwareConcurrency,
      deviceMemory: navigator.deviceMemory,
      languages: navigator.languages,
    })),
    viewport: { innerWidth, innerHeight, outerWidth, outerHeight, devicePixelRatio },
    timezone: safe(() => Intl.DateTimeFormat().resolvedOptions().timeZone),
    cssom: safe(() => {
      const sheet = Array.from(document.styleSheets).find((s) => s.ownerNode === style);
      return {
        registered: !!sheet,
        ruleCount: sheet?.cssRules?.length,
        lastSelector: sheet?.cssRules?.[sheet.cssRules.length - 1]?.selectorText,
        lastIsCSSStyleRule:
          typeof CSSStyleRule !== "undefined" &&
          sheet?.cssRules?.[sheet.cssRules.length - 1] instanceof CSSStyleRule,
      };
    }),
    realm: safe(() => ({
      hasWindow: !!frame.contentWindow,
      distinctArray: frame.contentWindow?.Array !== Array,
      distinctFunction: frame.contentWindow?.Function !== Function,
      hasDocument: !!frame.contentDocument,
      distinctArrayPrototype: frame.contentWindow?.Array.prototype !== Array.prototype,
      functionUsesChildGlobal:
        frame.contentWindow?.Function("return globalThis")() === frame.contentWindow,
      literalUsesChildArray:
        !!frame.contentWindow &&
        frame.contentWindow.Function("return []")() instanceof frame.contentWindow.Array,
    })),
    hiddenInputs: safe(() =>
      Array.from(document.querySelectorAll("input[type=hidden][name]")).map((e) => ({
        name: e.getAttribute("name"),
        property: e.value,
        attribute: e.getAttribute("value"),
      })),
    ),
    parsedControls: safe(() =>
      Array.from(controls.querySelectorAll("input, textarea, select")).map((e) => ({
        name: e.name,
        value: e.value,
        form: e.form?.id,
      })),
    ),
  };
  controls.remove();
  style.remove();
  frame.remove();
  return JSON.stringify(data);
})();
