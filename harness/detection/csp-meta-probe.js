(async () => {
  const rows = [];
  for (const kind of [
    "content-tighten",
    "content-loosen",
    "late-content",
    "removed",
    "body-to-head",
    "fragment",
  ]) {
    const element = document.createElement("iframe");
    element.srcdoc = "<!doctype html><head></head><body></body>";
    const loaded = new Promise((resolve) =>
      element.addEventListener("load", resolve, { once: true }),
    );
    document.body.appendChild(element);
    await loaded;
    const doc = element.contentDocument,
      meta = doc.createElement("meta");
    meta.httpEquiv = "Content-Security-Policy";
    if (kind !== "late-content")
      meta.content = kind === "content-tighten" ? "base-uri 'self'" : "base-uri 'none'";
    (kind === "body-to-head" ? doc.body : doc.head).appendChild(meta);
    if (kind === "content-tighten" || kind === "late-content") meta.content = "base-uri 'none'";
    if (kind === "content-loosen") meta.content = "base-uri 'self'";
    if (kind === "removed") meta.remove();
    if (kind === "body-to-head") doc.head.appendChild(meta);
    if (kind === "fragment")
      doc.head.innerHTML =
        "<meta http-equiv='Content-Security-Policy' content=\"base-uri 'none'\"><base href='/fragment/'>";
    else {
      const base = doc.createElement("base");
      base.href = "/allowed/";
      doc.head.appendChild(base);
    }
    rows.push({ kind, base: doc.baseURI.replace(location.origin, "<origin>") });
    element.remove();
  }
  return rows;
})();
