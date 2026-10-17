(async () => {
  const rect = (node) => {
    const r = node.getBoundingClientRect();
    return [r.x, r.y, r.width, r.height];
  };
  document.body.innerHTML =
    '<style>body{margin:0}#host{display:block;width:120px}#light{display:block;width:31px;height:7px}</style><div id="host"><div id="light" slot="content">light</div></div>';
  const host = document.getElementById("host"),
    light = document.getElementById("light");
  const outer = host.attachShadow({ mode: "closed" });
  outer.innerHTML =
    '<style>:host{padding:3px;border:1px solid}::slotted(#light){height:13px}#nested{display:block;width:60px}</style><slot name="content"></slot><div id="nested"></div>';
  const nested = outer.getElementById("nested"),
    inner = nested.attachShadow({ mode: "closed" });
  inner.innerHTML =
    '<style>:host{height:auto}#leaf{display:block;width:17px;height:11px}</style><div id="leaf"></div>';
  const leaf = inner.getElementById("leaf"),
    sheet = new CSSStyleSheet();
  sheet.replaceSync("#leaf{width:23px}");
  inner.adoptedStyleSheets = [sheet];
  const range = document.createRange();
  range.selectNode(light);
  const sample = () => ({
    host: rect(host),
    light: rect(light),
    nested: rect(nested),
    leaf: rect(leaf),
    range: rect(range),
  });
  const out = [{ name: "nested", ...sample() }];
  sheet.cssRules[0].style.width = "29px";
  out.push({ name: "sheet-mutation", ...sample() });
  light.slot = "missing";
  out.push({ name: "unassigned", ...sample() });
  light.slot = "content";
  out.push({ name: "reassigned", ...sample() });
  host.remove();
  out.push({ name: "disconnected", ...sample() });
  document.body.append(host);
  out.push({ name: "reconnected", ...sample() });
  const frame = document.createElement("iframe");
  frame.style = "border:0;width:200px;height:100px";
  document.body.prepend(frame);
  if (!frame.contentDocument) await new Promise((resolve) => (frame.onload = resolve));
  const doc = frame.contentDocument;
  doc.body.style = "margin:0";
  doc.body.append(doc.adoptNode(host));
  const migrated = doc.createRange();
  migrated.selectNode(light);
  out.push({
    name: "adopted-frame",
    ...sample(),
    migratedRange: rect(migrated),
    owner: light.ownerDocument === doc,
    shadowOwner: inner.ownerDocument === doc,
  });
  frame.remove();
  out.push({ name: "retired-frame", ...sample(), migratedRange: rect(migrated) });
  return out;
})();
