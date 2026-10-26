(async () => {
  "use strict";
  // turbo-surf#30: which image responses an <img> decodes, and its natural size, per format.
  // Each parser <img data-id> is a data: case; each id in #http-cases is served by the test's
  // server at /<id>. Rows: parser images [complete, naturalWidth, naturalHeight] once they
  // settled, script-created images (new Image(), detached) [event, complete, naturalWidth,
  // naturalHeight].
  const settle = (img) =>
    new Promise((done) => {
      if (img.complete) return done(img.naturalWidth ? "load" : "error");
      img.addEventListener("load", () => done("load"), { once: true });
      img.addEventListener("error", () => done("error"), { once: true });
      setTimeout(() => done("timeout"), 5000);
    });
  const created = async (src) => {
    const img = new Image();
    const event = new Promise((done) => {
      img.addEventListener("load", () => done("load"), { once: true });
      img.addEventListener("error", () => done("error"), { once: true });
      setTimeout(() => done("timeout"), 5000);
    });
    img.src = src;
    return [await event, img.complete, img.naturalWidth, img.naturalHeight];
  };
  const out = { parsed: {}, created: {}, http: {} };
  const parsed = Array.from(document.querySelectorAll("img[data-id]"));
  await Promise.all(parsed.map(settle));
  for (const img of parsed)
    out.parsed[img.dataset.id] = [img.complete, img.naturalWidth, img.naturalHeight];
  for (const img of parsed) out.created[img.dataset.id] = await created(img.getAttribute("src"));
  for (const id of JSON.parse(document.getElementById("http-cases").textContent))
    out.http[id] = await created("/" + id);
  return out;
})();
