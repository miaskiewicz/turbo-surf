(async () => {
  "use strict";
  // turbo-surf#40: an <img> loads the srcset candidate that HTML's source selection picks for
  // the page's devicePixelRatio (density descriptors; `w` descriptors against `sizes`, whose
  // default is 100vw; `src` as the 1x candidate), else its src. naturalWidth/naturalHeight are
  // the image's size divided by the chosen density. Candidates are served at /c/<case>/<x>.png
  // (r 3x2, b 5x4, g 7x6, s 9x8 pixels; no-store), so no cached image sways the choice.
  // Rows: [chosen x, naturalWidth, naturalHeight, complete].
  const load = (id, attributes) =>
    new Promise((done) => {
      const url = (x) => "/c/" + id + "/" + x + ".png";
      const img = document.createElement("img");
      const row = () => {
        const chosen = (img.currentSrc.match(/\/(\w)\.png$/) || [, img.currentSrc])[1];
        done([chosen, img.naturalWidth, img.naturalHeight, img.complete]);
      };
      img.addEventListener("load", row, { once: true });
      img.addEventListener("error", row, { once: true });
      setTimeout(row, 1000);
      for (const [name, value] of attributes(url)) img.setAttribute(name, value);
    });
  const out = { devicePixelRatio, innerWidth };
  const cases = {
    only1x: (u) => [["srcset", u("r") + " 1x"]],
    noDescriptor: (u) => [["srcset", u("r")]],
    densities: (u) => [["srcset", u("r") + " 1x, " + u("b") + " 2x, " + u("g") + " 3x"]],
    densitiesUnordered: (u) => [["srcset", u("g") + " 3x, " + u("r") + " 1x, " + u("b") + " 2x"]],
    geometricMean: (u) => [["srcset", u("r") + " 1x, " + u("b") + " 1.5x, " + u("g") + " 2.5x"]],
    belowRatio: (u) => [["srcset", u("r") + " 0.5x, " + u("b") + " 1.5x"]],
    allBelow: (u) => [["srcset", u("r") + " 1.2x, " + u("b") + " 1.8x"]],
    srcIs1x: (u) => [
      ["src", u("s")],
      ["srcset", u("b") + " 2x"],
    ],
    srcAnd1x: (u) => [
      ["src", u("s")],
      ["srcset", u("r") + " 1x, " + u("b") + " 2x"],
    ],
    widthsSized: (u) => [
      ["sizes", "100px"],
      ["srcset", u("r") + " 150w, " + u("b") + " 300w, " + u("g") + " 600w"],
    ],
    widthsViewport: (u) => [
      ["srcset", u("r") + " 640w, " + u("b") + " 2560w, " + u("g") + " 5120w"],
    ],
    widthsMediaSizes: (u) => [
      ["sizes", "(max-width: 10px) 5000px, 50vw"],
      ["srcset", u("r") + " 640w, " + u("b") + " 1280w, " + u("g") + " 2560w"],
    ],
    emptySrcset: (u) => [
      ["src", u("s")],
      ["srcset", ""],
    ],
    invalidOnly: (u) => [
      ["src", u("s")],
      ["srcset", u("r") + " 2q"],
    ],
    trailingComma: (u) => [["srcset", u("r") + ", " + u("b") + " 2x,"]],
  };
  for (const [id, attributes] of Object.entries(cases)) out[id] = await load(id, attributes);
  return out;
})();
