(async () => {
  const rows = [];
  const frame = async () => {
    const element = document.createElement("iframe");
    element.srcdoc = "<!doctype html><html><head></head><body></body></html>";
    const loaded = new Promise((resolve) =>
      element.addEventListener("load", resolve, { once: true }),
    );
    document.body.appendChild(element);
    await loaded;
    return element;
  };
  const policy = (doc, content, body = false) => {
    const meta = doc.createElement("meta");
    meta.httpEquiv = "Content-Security-Policy";
    meta.content = content;
    (body ? doc.body : doc.head).appendChild(meta);
  };
  const base = (doc, href) => {
    const element = doc.createElement("base");
    element.href = href;
    doc.head.appendChild(element);
    return element;
  };
  const normalize = (value) => value.replace(location.origin, "<origin>");
  for (const kind of ["none", "default", "frozen", "self", "body-meta", "two-bases"]) {
    const element = await frame(),
      doc = element.contentDocument;
    if (kind === "none") policy(doc, "base-uri 'none'");
    if (kind === "default") policy(doc, "default-src 'none'");
    if (kind === "self") policy(doc, "base-uri 'self'");
    if (kind === "body-meta") policy(doc, "base-uri 'none'", true);
    const first = base(doc, kind === "self" ? "http://localhost:1/cross/" : "/allowed/root/");
    const before = normalize(doc.baseURI);
    if (kind === "frozen") policy(doc, "base-uri 'none'");
    const afterPolicy = normalize(doc.baseURI);
    first.setAttribute("href", kind === "self" ? "/same/root/" : "/changed/root/");
    const afterWrite = normalize(doc.baseURI);
    const second = base(doc, "/second/root/");
    const beforeRemove = normalize(doc.baseURI);
    first.remove();
    const afterRemove = normalize(doc.baseURI);
    rows.push({
      kind,
      before,
      afterPolicy,
      afterWrite,
      beforeRemove,
      afterRemove,
      secondHref: second.getAttribute("href"),
    });
    element.remove();
  }
  return rows;
})();
