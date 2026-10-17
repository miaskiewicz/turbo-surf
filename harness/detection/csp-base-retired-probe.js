(async () => {
  const rows = [];
  for (const restricted of [false, true]) {
    const frame = document.createElement("iframe");
    frame.srcdoc =
      "<!doctype html><head><base href='/original/'>" +
      (restricted
        ? "<meta http-equiv='Content-Security-Policy' content=\"base-uri 'none'\">"
        : "") +
      "</head><body></body>";
    const firstLoad = new Promise((resolve) =>
      frame.addEventListener("load", resolve, { once: true }),
    );
    document.body.appendChild(frame);
    await firstLoad;
    const old = frame.contentDocument,
      base = old.querySelector("base");
    const before = old.baseURI.replace(location.origin, "<origin>");
    const nextLoad = new Promise((resolve) =>
      frame.addEventListener("load", resolve, { once: true }),
    );
    frame.srcdoc = "<!doctype html><head></head><body></body>";
    await nextLoad;
    base.setAttribute("href", "/changed/");
    const after = old.baseURI.replace(location.origin, "<origin>");
    base.remove();
    const replacement = old.createElement("base");
    replacement.setAttribute("href", "/replacement/");
    old.head.appendChild(replacement);
    const replaced = old.baseURI.replace(location.origin, "<origin>");
    rows.push({ restricted, before, after, replaced });
    frame.remove();
  }
  return rows;
})();
