(async () => {
  document.body.style = "margin:0";
  const frame = document.createElement("iframe");
  frame.style =
    "display:block;box-sizing:content-box;width:200px;height:100px;border:3px solid;padding:5px;transform:scale(1.25)";
  document.body.append(frame);
  if (!frame.contentDocument) await new Promise((resolve) => (frame.onload = resolve));
  const win = frame.contentWindow,
    doc = frame.contentDocument;
  doc.body.style = "margin:0";
  const box = doc.createElement("div");
  box.style = "width:100%;height:50vh";
  doc.body.append(box);
  const nested = doc.createElement("iframe");
  nested.style = "width:50%;height:40px;border:0";
  doc.body.append(nested);
  if (!nested.contentDocument) await new Promise((resolve) => (nested.onload = resolve));
  const nestedWin = nested.contentWindow,
    nestedDoc = nested.contentDocument;
  nestedDoc.body.style = "margin:0";
  const leaf = nestedDoc.createElement("div");
  leaf.style = "width:100%;height:10px";
  nestedDoc.body.append(leaf);
  const range = doc.createRange();
  range.selectNode(box);
  const rect = (node) => {
    const r = node.getBoundingClientRect();
    return [r.x, r.y, r.width, r.height];
  };
  const retainedQuery = win.matchMedia("(max-width:220px)");
  const retainedAll = win.matchMedia("all");
  const mediaState = (value) => (value === null ? null : { matches: value.matches });
  const borrowedMatchMedia = win.matchMedia;
  const widthDescriptor = Object.getOwnPropertyDescriptor(win, "innerWidth");
  const capture = (fn) => {
    try {
      return { ok: true, value: fn() };
    } catch (error) {
      return {
        ok: false,
        name: error.name,
        definitionTypeError: error instanceof win.TypeError,
        definitionDOMException: error instanceof win.DOMException,
      };
    }
  };
  const sample = () => ({
    media: [retainedQuery.matches, borrowedMatchMedia("(min-width:500px)").matches],
    viewport: [win.innerWidth, win.innerHeight],
    box: rect(box),
    range: rect(range),
    nestedViewport: [nestedWin.innerWidth, nestedWin.innerHeight],
    leaf: rect(leaf),
  });
  const out = [{ name: "initial", ...sample() }];
  out.push({
    name: "descriptor",
    get: typeof widthDescriptor.get,
    set: typeof widthDescriptor.set,
    enumerable: widthDescriptor.enumerable,
    configurable: widthDescriptor.configurable,
    forgedGet: capture(() => widthDescriptor.get.call({})),
    forgedSet: capture(() => {
      const object = {};
      widthDescriptor.set.call(object, 17);
      return object.innerWidth;
    }),
  });
  out.push({
    name: "media-receivers",
    ownPrototype: Object.hasOwn(borrowedMatchMedia, "prototype"),
    construct: capture(() => Reflect.construct(borrowedMatchMedia, ["all"])),
    parent: capture(() => borrowedMatchMedia.call(window, "(min-width:500px)").matches),
    forged: capture(() => borrowedMatchMedia.call({}, "(min-width:500px)").matches),
    nullReceiver: capture(() => borrowedMatchMedia.call(null, "(min-width:500px)").matches),
  });
  frame.style.width = "240px";
  frame.style.height = "120px";
  out.push({ name: "resized", ...sample() });
  frame.style.width = "240.5px";
  frame.style.height = "120.5px";
  out.push({ name: "fractional", ...sample() });
  out.push({
    name: "assignment",
    result: capture(() => {
      "use strict";
      win.innerWidth = 777;
      return win.innerWidth;
    }),
    nativeBox: rect(box),
  });
  Object.defineProperty(frame, "clientWidth", {
    get() {
      throw new Error("guest clientWidth accessed");
    },
  });
  Object.defineProperty(win, "innerWidth", { value: 999, configurable: true });
  out.push({
    name: "guest-shadowed",
    box: rect(box),
    leaf: rect(leaf),
    media: [retainedQuery.matches, borrowedMatchMedia("(min-width:500px)").matches],
  });
  const foreign = document.createElement("iframe");
  foreign.src = location.href.replace("127.0.0.1", "localhost");
  const foreignLoaded = new Promise((resolve) => (foreign.onload = resolve));
  document.body.append(foreign);
  await foreignLoaded;
  out.push({
    name: "cross-origin-get",
    result: capture(() => widthDescriptor.get.call(foreign.contentWindow)),
    setterResult: capture(() => widthDescriptor.set.call(foreign.contentWindow, 1)),
  });
  foreign.remove();
  frame.remove();
  out.push({
    name: "retired",
    box: rect(box),
    range: rect(range),
    leaf: rect(leaf),
    retainedDocument: box.ownerDocument === doc,
    retainedDefaultView: doc.defaultView === win,
    retainedNestedDefaultView: nestedDoc.defaultView === nestedWin,
    retainedNestedFrameElement: nestedWin.frameElement === nested,
    retiredAssociations: {
      outerDocument: win.document === doc,
      nestedDocument: nestedWin.document === nestedDoc,
      outerWindowDocumentType: typeof win.document,
      nestedWindowDocumentType: typeof nestedWin.document,
      nestedConnected: nested.isConnected,
      nestedDocConnected: nestedDoc.isConnected,
      leafConnected: leaf.isConnected,
    },
    retiredMetrics: {
      client: [box.clientLeft, box.clientTop, box.clientWidth, box.clientHeight],
      offset: [box.offsetLeft, box.offsetTop, box.offsetWidth, box.offsetHeight],
      scroll: [box.scrollLeft, box.scrollTop, box.scrollWidth, box.scrollHeight],
      offsetParentIsNull: box.offsetParent === null,
      hit: [doc.elementFromPoint(0, 0) === null, doc.elementsFromPoint(0, 0).length],
      caret: [doc.caretPositionFromPoint(0, 0) === null, doc.caretRangeFromPoint(0, 0) === null],
      media: {
        retainedMax: retainedQuery.matches,
        retainedAll: retainedAll.matches,
        newMax: mediaState(win.matchMedia("(max-width:220px)")),
        newAll: mediaState(win.matchMedia("all")),
        newZero: mediaState(win.matchMedia("(width:0px)")),
        nestedAll: mediaState(nestedWin.matchMedia("all")),
      },
    },
    retainedViewport: [
      widthDescriptor.get.call(win),
      win.innerHeight,
      nestedWin.innerWidth,
      nestedWin.innerHeight,
    ],
  });
  const fresh = doc.createElement("p");
  fresh.textContent = "usable";
  doc.body.append(fresh);
  out.push({
    name: "retired-mutation",
    text: fresh.textContent,
    connected: fresh.isConnected,
    box: rect(fresh),
  });
  document.body.append(frame);
  const replacementWin = frame.contentWindow,
    replacementDoc = frame.contentDocument;
  out.push({
    name: "reinserted-host",
    distinctWindow: replacementWin !== win,
    distinctDocument: replacementDoc !== doc,
    oldViewport: [widthDescriptor.get.call(win), win.innerHeight],
    oldQueries: [retainedQuery.matches, retainedAll.matches],
    oldNewQuery: mediaState(win.matchMedia("all")),
    newViewport: [replacementWin.innerWidth, replacementWin.innerHeight],
    newQuery: mediaState(replacementWin.matchMedia("all")),
    oldBox: rect(box),
  });
  return out;
})();
