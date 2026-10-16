(() => {
  document.body.style.cssText = "margin:0;padding:0";
  const units = [0x61, 0xd835, 0xdc00, 0xd800, 0x62, 0xdcff, 0x63];
  const original = String.fromCharCode(...units);
  const projection = "a\ud835\udc00\ufffdb\ufffdc";
  const make = (value) => {
    const element = document.createElement("div");
    element.style.cssText =
      "font-family:Times New Roman;font-size:64px;line-height:80px;width:400px";
    const text = document.createTextNode(value);
    element.append(text);
    document.body.append(element);
    return { element, text };
  };
  const native = make(original);
  const reference = make(projection);
  const codeUnits = (value) =>
    Array.from({ length: value.length }, (_, index) => value.charCodeAt(index));
  const ranges = (entry) => {
    const origin = entry.element.getBoundingClientRect();
    const output = [];
    for (let start = 0; start <= units.length; start++)
      for (let end = start; end <= units.length; end++) {
        const range = document.createRange();
        range.setStart(entry.text, start);
        range.setEnd(entry.text, end);
        output.push({
          start,
          end,
          selected: codeUnits(range.toString()),
          boxes: Array.from(range.getClientRects(), (rect) => [
            rect.x - origin.x,
            rect.y - origin.y,
            rect.width,
            rect.height,
          ]),
        });
      }
    return output;
  };
  const actual = ranges(native);
  const expected = ranges(reference);
  const range = document.createRange();
  range.setStart(native.text, 6);
  range.setEnd(native.text, 7);
  const rect = range.getBoundingClientRect();
  const caret = document.caretPositionFromPoint(
    rect.x + rect.width * 0.9,
    rect.y + rect.height / 2,
  );
  let hooks = 0;
  Object.defineProperty(native.text, "data", {
    get() {
      hooks++;
      throw Error("public UTF-16 text shadow");
    },
  });
  const privateBoxes = ranges(native);
  const originalUnits = codeUnits(native.text.nodeValue);
  const retained = { sameText: caret?.offsetNode === native.text, offset: caret?.offset ?? null };
  native.text.nodeValue = "a\ud800\udcffb\ud800";
  const shortened = caret?.getClientRect();
  native.text.nodeValue = original;
  const restored = caret?.getClientRect();
  const fallbackControls = [];
  for (const family of ["Times New Roman", "Arial", "Courier New"])
    for (const size of [13, 15, 32, 64]) {
      const entry = make("a\ud800b\udcffc");
      entry.element.style.cssText = `font-family:${family};font-size:${size}px;line-height:${size * 1.25}px;width:600px`;
      const bounds = entry.element.getBoundingClientRect();
      const boxes = [];
      for (let offset = 0; offset < 5; offset++) {
        const part = document.createRange();
        part.setStart(entry.text, offset);
        part.setEnd(entry.text, offset + 1);
        const box = part.getBoundingClientRect();
        boxes.push([box.x - bounds.x, box.y - bounds.y, box.width, box.height]);
      }
      fallbackControls.push({ family, size, boxes });
      entry.element.remove();
    }
  return {
    fallbackControls,
    originalUnits,
    length: native.text.length,
    selections: actual.map(({ start, end, selected }) => ({ start, end, selected })),
    projectionDifferences: actual.flatMap((value, index) =>
      JSON.stringify(value.boxes) === JSON.stringify(expected[index].boxes)
        ? []
        : [
            {
              start: value.start,
              end: value.end,
              original: value.boxes,
              replacement: expected[index].boxes,
            },
          ],
    ),
    boxesMatchScalarProjection: actual.every(
      (value, index) => JSON.stringify(value.boxes) === JSON.stringify(expected[index].boxes),
    ),
    privateBoxesUnchanged: actual.every(
      (value, index) => JSON.stringify(value.boxes) === JSON.stringify(privateBoxes[index].boxes),
    ),
    allOffsetsPlaced: actual.every((value) => value.boxes.length > 0),
    pairHalvesShareBox:
      JSON.stringify(actual.find((value) => value.start === 1 && value.end === 2).boxes) ===
      JSON.stringify(actual.find((value) => value.start === 2 && value.end === 3).boxes),
    insidePairCollapsedWidthZero: actual
      .find((value) => value.start === 2 && value.end === 2)
      .boxes.every((box) => box[2] === 0),
    retained,
    shortenedRectEmpty: shortened ? shortened.width === 0 && shortened.height === 0 : null,
    restoredRectPlaced: restored ? restored.height > 0 : null,
    hooks,
  };
})();
