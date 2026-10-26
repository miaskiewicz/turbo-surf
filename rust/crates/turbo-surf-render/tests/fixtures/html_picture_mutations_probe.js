(async () => {
  "use strict";
  // turbo-dom#71 (surf's load path): a <picture>'s image reselects its source when a
  // <source> attribute changes and when a <source> is inserted or removed; each new
  // choice loads and reports its URL and natural size.
  const picture = document.getElementById("pic"),
    img = document.getElementById("img");
  const settle = () =>
    new Promise((done) => {
      const finish = () => {
        img.removeEventListener("load", finish);
        img.removeEventListener("error", finish);
        done();
      };
      img.addEventListener("load", finish);
      img.addEventListener("error", finish);
    });
  const state = () => [
    new URL(img.currentSrc).pathname,
    img.complete,
    img.naturalWidth,
    img.naturalHeight,
  ];
  const out = { initial: state() };
  const first = document.getElementById("first");
  let loaded = settle();
  first.setAttribute("srcset", "/img/60x30/changed.png");
  await loaded;
  out.sourceSrcset = state();
  loaded = settle();
  first.setAttribute("type", "image/x-unsupported");
  await loaded;
  out.sourceType = state();
  loaded = settle();
  const inserted = document.createElement("source");
  inserted.setAttribute("srcset", "/img/80x40/inserted.png");
  picture.insertBefore(inserted, picture.firstChild);
  await loaded;
  out.insertedSource = state();
  loaded = settle();
  inserted.remove();
  await loaded;
  out.removedSource = state();
  return out;
})();
