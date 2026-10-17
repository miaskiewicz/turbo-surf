// Post-verdict collector: authored DOM operations only, with no Google source or
// host capabilities. Every inserted stylesheet is removed even if a read throws.
(() => {
  const safe = (fn) => {
    try {
      return fn();
    } catch (error) {
      return { error: error.name, message: error.message };
    }
  };
  const sheetState = (style) =>
    safe(() => {
      const sheet = style.sheet,
        listed = Array.from(document.styleSheets),
        found = listed.find((item) => item.ownerNode === style);
      return {
        connected: style.isConnected,
        ownerDocument: style.ownerDocument === document,
        inHead: style.parentNode === document.head,
        directSheet: !!sheet,
        directRules: sheet?.cssRules.length,
        registered: !!found,
        listedCount: listed.length,
        queriedStyles: document.querySelectorAll("style").length,
        sameSheet: found === sheet,
        ownerNode: sheet?.ownerNode === style,
        nonceLength: style.nonce?.length,
        css: sheet
          ? Array.from(sheet.cssRules).map((rule) => ({
              type: rule.type,
              selector: rule.selectorText,
            }))
          : null,
      };
    });
  const rows = [];
  for (const connectFirst of [false, true])
    for (const uppercase of [false, true]) {
      const style = document.createElement(uppercase ? "STYLE" : "style");
      const before = document.styleSheets.length;
      const result = safe(() => {
        style.setAttribute("data-local-css-diagnostic", "");
        if (connectFirst) document.head.appendChild(style);
        style.appendChild(document.createTextNode("body { color: red; } local-css-sentinel {}"));
        if (!connectFirst) document.head.appendChild(style);
        const first = sheetState(style);
        style.textContent = "local-css-changed { color: blue; }";
        return { first, changed: sheetState(style) };
      });
      try {
        rows.push({ connectFirst, uppercase, before, ...result });
      } finally {
        style.remove();
      }
    }
  return JSON.stringify({
    rows,
    document: {
      sheetCount: document.styleSheets.length,
      headOwner: document.head.ownerDocument === document,
      headConnected: document.head.isConnected,
      headCount: document.getElementsByTagName("HEAD").length,
      sheetOwners: Array.from(document.styleSheets).map((s) => ({
        tag: s.ownerNode?.nodeName,
        connected: s.ownerNode?.isConnected,
        sameDocument: s.ownerNode?.ownerDocument === document,
      })),
    },
  });
})();
