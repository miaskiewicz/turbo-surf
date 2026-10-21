(() => {
  const rectangle = (rect) => [rect.x, rect.y, rect.width, rect.height];
  const describe = (prototype, name) => {
    const descriptor = Object.getOwnPropertyDescriptor(prototype, name);
    return descriptor
      ? {
          enumerable: descriptor.enumerable,
          configurable: descriptor.configurable,
          writable: descriptor.writable ?? null,
          name: (descriptor.value ?? descriptor.get).name,
          length: (descriptor.value ?? descriptor.get).length,
        }
      : null;
  };
  document.body.innerHTML =
    '<div id="target" style="font-family:Times New Roman;font-size:64px;line-height:80px;width:400px">ab</div>';
  document.body.style.cssText = "margin:0;padding:0";
  const target = document.getElementById("target");
  const text = target.firstChild;
  const ranges = [];
  for (let start = 0; start <= 2; start++)
    for (let end = start; end <= 2; end++) {
      const range = document.createRange();
      range.setStart(text, start);
      range.setEnd(text, end);
      ranges.push({
        start,
        end,
        rects: Array.from(range.getClientRects(), rectangle),
        bounds: rectangle(range.getBoundingClientRect()),
      });
    }
  const sample = document.createRange();
  sample.setStart(text, 0);
  sample.setEnd(text, 2);
  const rect = sample.getBoundingClientRect();
  const carets = [];
  for (const fraction of [0.1, 0.4, 0.6, 0.9]) {
    const x = rect.x + rect.width * fraction,
      y = rect.y + rect.height / 2;
    const position = document.caretPositionFromPoint(x, y),
      range = document.caretRangeFromPoint(x, y);
    carets.push({
      sameText: position?.offsetNode === text,
      offset: position?.offset ?? null,
      rect: position ? rectangle(position.getClientRect()) : null,
      rangeText: range?.startContainer === text,
      rangeOffset: range?.startOffset ?? null,
      collapsed: range?.collapsed ?? null,
    });
  }
  const hit =
    document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2) === target;
  let hooks = 0;
  Object.defineProperty(sample, "startContainer", {
    get() {
      hooks++;
      throw Error("public range shadow");
    },
  });
  Object.defineProperty(text, "data", {
    get() {
      hooks++;
      throw Error("public text shadow");
    },
  });
  const privateRange = rectangle(Range.prototype.getBoundingClientRect.call(sample));
  const errors = {};
  for (const [key, run] of [
    ["rangeBrand", () => Range.prototype.getClientRects.call({})],
    ["positionBrand", () => CaretPosition.prototype.getClientRect.call({})],
    ["positionConstructor", () => new CaretPosition()],
    ["caretArguments", () => document.caretPositionFromPoint(1)],
    ["caretNonfinite", () => document.caretRangeFromPoint(Infinity, 0)],
    ["positionInfinity", () => document.caretPositionFromPoint(Infinity, 0)],
    ["positionNaN", () => document.caretPositionFromPoint(NaN, 0)],
    ["positionFloatOverflow", () => document.caretPositionFromPoint(2 ** 128, 0)],
    ["positionNegativeFloatOverflow", () => document.caretPositionFromPoint(-(2 ** 128), 0)],
    ["positionBigInt", () => document.caretPositionFromPoint(1n, 0)],
    ["positionSymbol", () => document.caretPositionFromPoint(Symbol(), 0)],
  ]) {
    try {
      run();
      errors[key] = null;
    } catch (error) {
      errors[key] = error.name;
    }
  }
  const legacyConversions = [
    [],
    [undefined, undefined],
    [14.19, 40],
    [14.21, 40],
    [Infinity, 0],
    [NaN, 40],
    [0.5, 40],
    [15, 40],
    [2 ** 32 + 15, 40],
    [2 ** 31 + 15, 40],
  ].map((args) => {
    const range = document.caretRangeFromPoint(...args);
    return {
      isNull: range === null,
      sameText: range?.startContainer === text,
      offset: range?.startOffset ?? null,
    };
  });
  const positionConversions = [
    [14.19, 40],
    [14.21, 40],
    [0.5, 40],
    [15, 40],
    [15 + 2 ** -25, 40],
    [2 ** 32 + 15, 40],
    [2 ** 31 + 15, 40],
    [3.4028234663852886e38, 0],
    [-3.4028234663852886e38, 0],
  ].map((args) => {
    const position = document.caretPositionFromPoint(...args);
    return {
      isNull: position === null,
      sameText: position?.offsetNode === text,
      offset: position?.offset ?? null,
    };
  });
  const retained = document.caretPositionFromPoint(
    rect.x + rect.width * 0.9,
    rect.y + rect.height / 2,
  );
  const lifecycle = [];
  target.style.fontSize = "32px";
  target.style.lineHeight = "48px";
  lifecycle.push({
    step: "font",
    offset: retained.offset,
    rect: retained.getClientRect() ? rectangle(retained.getClientRect()) : null,
  });
  target.remove();
  lifecycle.push({
    step: "detached",
    offset: retained.offset,
    rect: retained.getClientRect() ? rectangle(retained.getClientRect()) : null,
  });
  document.body.appendChild(target);
  text.nodeValue = "a";
  lifecycle.push({
    step: "shortened",
    offset: retained.offset,
    rect: retained.getClientRect() ? rectangle(retained.getClientRect()) : null,
  });
  text.nodeValue = "abcd";
  lifecycle.push({
    step: "restored",
    offset: retained.offset,
    rect: retained.getClientRect() ? rectangle(retained.getClientRect()) : null,
  });
  return {
    ranges,
    carets,
    hit,
    privateRange,
    hooks,
    errors,
    legacyConversions,
    positionConversions,
    lifecycle,
    outOfView: document.caretPositionFromPoint(-1, -1) === null,
    interfaces: Object.fromEntries([
      ["rangeRects", describe(Range.prototype, "getClientRects")],
      ["rangeBounds", describe(Range.prototype, "getBoundingClientRect")],
      ["caretRange", describe(Document.prototype, "caretRangeFromPoint")],
      ["caretPosition", describe(Document.prototype, "caretPositionFromPoint")],
      ["caretNode", describe(CaretPosition.prototype, "offsetNode")],
      ["caretOffset", describe(CaretPosition.prototype, "offset")],
      ["caretRect", describe(CaretPosition.prototype, "getClientRect")],
    ]),
  };
})();
