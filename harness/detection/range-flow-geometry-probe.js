(() => {
  const rectangle = (r) => [r.x, r.y, r.width, r.height];
  document.body.style.cssText = "margin:0;padding:0";
  document.body.innerHTML =
    '<div id="box" style="font-family:Times New Roman;font-size:64px;line-height:80px;width:400px">ab<!--ignored--><span id="inside">cd</span>ef</div>';
  const box = document.getElementById("box"),
    inside = document.getElementById("inside");
  const a = box.firstChild,
    b = inside.firstChild,
    c = box.lastChild;
  const rows = [];
  for (const [name, start, so, end, eo] of [
    ["cross", a, 1, c, 1],
    ["parent", box, 0, box, 4],
    ["span", box, 2, box, 3],
    ["textEdge", a, 2, b, 0],
    ["comment", box, 1, box, 2],
    ["collapsedParent", box, 2, box, 2],
    ["nestedText", b, 0, b, 2],
  ]) {
    const r = document.createRange();
    r.setStart(start, so);
    r.setEnd(end, eo);
    rows.push({
      name,
      rects: Array.from(r.getClientRects(), rectangle),
      bounds: rectangle(r.getBoundingClientRect()),
    });
  }
  box.style.width = "90px";
  const wrapping = document.createRange();
  wrapping.setStart(a, 1);
  wrapping.setEnd(c, 1);
  rows.push({
    name: "wrapped",
    rects: Array.from(wrapping.getClientRects(), rectangle),
    bounds: rectangle(wrapping.getBoundingClientRect()),
  });
  box.style.cssText =
    "font-family:Times New Roman;font-size:64px;line-height:80px;width:90px;height:70px;overflow:hidden";
  box.scrollTop = 40;
  rows.push({
    name: "scrolled",
    rects: Array.from(wrapping.getClientRects(), rectangle),
    bounds: rectangle(wrapping.getBoundingClientRect()),
  });
  const hits = [5, 35, 65].map((y) => {
    const caret = document.caretPositionFromPoint(20, y),
      owner = document.elementFromPoint(20, y);
    return {
      y,
      hit: owner === inside ? "inside" : owner === box ? "box" : owner?.tagName || null,
      source:
        caret?.offsetNode === a
          ? "a"
          : caret?.offsetNode === b
            ? "b"
            : caret?.offsetNode === c
              ? "c"
              : null,
      offset: caret?.offset ?? null,
    };
  });
  return { rows, hits, scrollTop: box.scrollTop };
})();
