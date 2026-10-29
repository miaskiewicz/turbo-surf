(async () => {
  "use strict";
  // turbo-dom#23 (surf side): which <img> loads start, and when. The binding owns the
  // element (reflected src, decode(), the Image() factory); the embedder starts a load for
  // every image its document owns when src/srcset changes or a tree inserts it, connected
  // or not, and never for images of an inert document (template contents, DOMParser,
  // createHTMLDocument). Each row: [events in order, complete, naturalWidth, naturalHeight,
  // own property names] once the image has settled (an event, or a quiet period for images
  // that never load).
  const red = document.getElementById("red").getAttribute("src"); // 3x2
  const blue = document.getElementById("blue").getAttribute("src"); // 5x4
  const broken = "data:image/png;base64,AAAA";
  const quiet = () => new Promise((done) => setTimeout(done, 200));
  const watch = (img) => {
    const events = [];
    let wake = null;
    for (const type of ["load", "error"])
      img.addEventListener(type, () => {
        events.push(type);
        if (wake) wake();
      });
    // Wait for `count` events (a cap keeps a missing event from hanging the probe).
    const until = (count) =>
      new Promise((done) => {
        if (events.length >= count) return done();
        wake = () => events.length >= count && done();
        setTimeout(done, 5000);
      });
    const row = () => [
      events.slice(),
      img.complete,
      img.naturalWidth,
      img.naturalHeight,
      Object.getOwnPropertyNames(img),
    ];
    return { events, until, row };
  };
  const loads = async (img, start, count = 1) => {
    const w = watch(img);
    start(img);
    await w.until(count);
    return w.row();
  };
  const idle = async (img, start) => {
    const w = watch(img);
    start(img);
    await quiet();
    return w.row();
  };
  const out = {};
  out.factorySrc = await loads(new Image(), (img) => {
    img.src = red;
  });
  out.factorySized = await loads(new Image(7, 9), (img) => {
    img.src = blue;
  });
  out.createdSetAttribute = await loads(document.createElement("img"), (img) =>
    img.setAttribute("src", red),
  );
  out.createdBroken = await loads(document.createElement("img"), (img) => {
    img.src = broken;
  });
  out.srcset = await idle(document.createElement("img"), (img) =>
    img.setAttribute("srcset", red + " 1x"),
  );
  // Fragment-parsed images: the element exists before the watch, so watch the host.
  const parsedInto = async (make) => {
    const events = [];
    const holder = document.createElement("div");
    holder.addEventListener("load", (e) => events.push(["load", e.target.localName]), true);
    holder.addEventListener("error", (e) => events.push(["error", e.target.localName]), true);
    const img = make(holder);
    for (let i = 0; i < 50 && !events.length; i++)
      await new Promise((done) => setTimeout(done, 20));
    return [events, img.complete, img.naturalWidth, img.naturalHeight, img.isConnected];
  };
  out.detachedInnerHTML = await parsedInto((holder) => {
    holder.innerHTML = "<p><span><img src='" + red + "'></span></p>";
    return holder.querySelector("img");
  });
  out.detachedInsertAdjacent = await parsedInto((holder) => {
    holder.insertAdjacentHTML("beforeend", "<img src='" + blue + "'>");
    return holder.querySelector("img");
  });
  out.contextualFragment = await parsedInto((holder) => {
    const range = document.createRange();
    range.selectNodeContents(document.body);
    holder.append(range.createContextualFragment("<img src='" + red + "'>"));
    return holder.querySelector("img");
  });
  out.appendedCreated = await parsedInto((holder) => {
    const img = document.createElement("img");
    img.setAttribute("src", blue);
    holder.append(img);
    return img;
  });
  out.template = await idle(
    (() => {
      const t = document.createElement("template");
      t.innerHTML = "<img src='" + red + "'>";
      return t.content.firstChild;
    })(),
    () => {},
  );
  out.templateDocument = await idle(
    document.createElement("template").content.ownerDocument.createElement("img"),
    (img) => {
      img.src = red;
    },
  );
  out.domParser = await idle(
    new DOMParser().parseFromString("<img src='" + red + "'>", "text/html").images[0],
    () => {},
  );
  out.htmlDocument = await idle(
    document.implementation.createHTMLDocument("").createElement("img"),
    (img) => {
      img.src = red;
    },
  );
  // A src change restarts the load; removing src leaves a broken, complete image.
  const changing = document.createElement("img");
  const cw = watch(changing);
  changing.src = red;
  await cw.until(1);
  changing.src = blue;
  await cw.until(2);
  out.srcChange = cw.row();
  changing.removeAttribute("src");
  await quiet();
  out.srcRemoved = cw.row();
  const parsed = document.getElementById("red");
  const pw = watch(parsed);
  parsed.setAttribute("alt", "x");
  parsed.setAttribute("width", "3");
  await quiet();
  out.unrelatedAttribute = pw.row();
  // Moving a loaded image into the document does not reload it.
  const moved = document.createElement("img");
  const mw = watch(moved);
  moved.src = blue;
  await mw.until(1);
  document.body.append(moved);
  await quiet();
  out.movedAfterLoad = mw.row();
  out.decodeDetached = await (async () => {
    const img = new Image();
    img.src = blue;
    try {
      await img.decode();
      return ["resolved", img.complete, img.naturalWidth, img.naturalHeight];
    } catch (e) {
      return ["rejected", e.name];
    }
  })();
  out.decodeBroken = await (async () => {
    const img = new Image();
    img.src = broken;
    try {
      await img.decode();
      return ["resolved"];
    } catch (e) {
      return ["rejected", e.name, img.complete];
    }
  })();
  return out;
})();
