(async () => {
  // CanvasRenderingContext2D.measureText over a committed web font (turbo-surf#133): every
  // TextMetrics field comes from the face's real glyph outlines and font metrics, never from
  // size ratios, and the result is a branded TextMetrics. IBM Plex Mono only, so the control
  // is deterministic on any host.
  await document.fonts.load('20px "Probe Plex"');
  const KEYS = [
    "width",
    "actualBoundingBoxLeft",
    "actualBoundingBoxRight",
    "actualBoundingBoxAscent",
    "actualBoundingBoxDescent",
    "fontBoundingBoxAscent",
    "fontBoundingBoxDescent",
    "emHeightAscent",
    "emHeightDescent",
    "hangingBaseline",
    "alphabeticBaseline",
    "ideographicBaseline",
  ];
  const ctx = document.createElement("canvas").getContext("2d");
  const read = (text) => {
    const m = ctx.measureText(text);
    return KEYS.map((key) => m[key]);
  };
  const FONTS = ['20px "Probe Plex"', '13px "Probe Plex"', '31.5px "Probe Plex", serif'];
  const TEXTS = ["", " ", "Typography", "AVATAR Wave", "gjpqy", "Hello, World!", "-_-"];
  const out = { keys: KEYS, metrics: [], aligned: [] };
  for (const font of FONTS) {
    ctx.font = font;
    for (const text of TEXTS) out.metrics.push([font, text, read(text)]);
  }
  ctx.font = '16px "Probe Plex"';
  for (const align of ["start", "end", "left", "right", "center"])
    for (const baseline of ["alphabetic", "top", "hanging", "middle", "ideographic", "bottom"]) {
      ctx.textAlign = align;
      ctx.textBaseline = baseline;
      out.aligned.push([align, baseline, read("Wave gj")]);
    }
  const metrics = ctx.measureText("x");
  out.shape = {
    tag: Object.prototype.toString.call(metrics),
    branded: typeof TextMetrics === "function" && metrics instanceof TextMetrics,
    own: Object.getOwnPropertyNames(metrics),
  };
  return JSON.stringify(out);
})();
