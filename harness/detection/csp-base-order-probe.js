(async () => {
  const rows = [];
  const policy = "<meta http-equiv='Content-Security-Policy' content=\"base-uri 'none'\">";
  const base = "<base href='/allowed/'>";
  for (const kind of [
    "parsed-before",
    "parsed-after",
    "fragment-before",
    "fragment-after",
    "svg-base",
    "body-policy",
  ]) {
    const element = document.createElement("iframe");
    let head = "",
      body = "";
    if (kind === "parsed-before") head = base + policy;
    if (kind === "parsed-after") head = policy + base;
    if (kind === "svg-base") head = "<svg><base href='/wrong/'/></svg>" + base;
    if (kind === "body-policy") body = policy + base;
    element.srcdoc =
      "<!doctype html><html><head>" + head + "</head><body>" + body + "</body></html>";
    const loaded = new Promise((resolve) =>
      element.addEventListener("load", resolve, { once: true }),
    );
    document.body.appendChild(element);
    await loaded;
    const doc = element.contentDocument;
    if (kind === "fragment-before") doc.head.innerHTML = base + policy;
    if (kind === "fragment-after") doc.head.innerHTML = policy + base;
    const before = doc.baseURI.replace(location.origin, "<origin>");
    const first = [...doc.querySelectorAll("base")].find(
      (node) => node.namespaceURI === "http://www.w3.org/1999/xhtml",
    );
    first.setAttribute("href", "/changed/");
    const after = doc.baseURI.replace(location.origin, "<origin>");
    rows.push({ kind, before, after });
    element.remove();
  }
  return rows;
})();
