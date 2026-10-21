(() => {
  const el = document.createElement("div");
  el.style.cssText = "width:40px;height:20px";
  document.body.append(el);
  const define = Object.defineProperty,
    get = Object.getOwnPropertyDescriptor,
    prototype = Object.prototype;
  let hooks = 0,
    forged = false,
    lists = false;
  const value = {
    valueOf() {
      hooks++;
      return 5;
    },
  };
  const scroll = get(Element.prototype, "scrollLeft").set;
  try {
    scroll.call({}, value);
  } catch (error) {
    forged = error.name === "TypeError";
  }
  try {
    Document.prototype.elementFromPoint.call({}, value, value);
  } catch (error) {
    forged = forged && error.name === "TypeError";
  }
  const before = hooks,
    savedGet = get(prototype, "get"),
    savedSet = get(prototype, "set");
  define(prototype, "get", {
    get() {
      hooks++;
      throw Error("descriptor getter");
    },
    configurable: true,
  });
  define(prototype, "set", {
    get() {
      hooks++;
      throw Error("descriptor setter");
    },
    configurable: true,
  });
  try {
    const rect = el.getBoundingClientRect(),
      list = el.getClientRects();
    lists = rect.width === 40 && list.length === 1 && list[0].height === 20;
  } finally {
    if (savedGet) define(prototype, "get", savedGet);
    else delete prototype.get;
    if (savedSet) define(prototype, "set", savedSet);
    else delete prototype.set;
  }
  el.remove();
  return { forged, lists, before, hooks };
})();
