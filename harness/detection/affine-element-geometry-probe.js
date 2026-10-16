(() => {
  const rectangle = (r) => [r.x, r.y, r.width, r.height];
  document.documentElement.style.cssText = "margin:0;padding:0";
  document.body.style.cssText = "margin:0;padding:0";
  const rows = [];
  for (const tag of ["div", "img"])
    for (const origin of ["0px 0px", "50% 50%"]) {
      const width = tag === "div" ? 400 : 100,
        height = tag === "div" ? 80 : 60;
      document.body.innerHTML = `<${tag} id="target" style="display:block;position:relative;left:100px;top:100px;width:${width}px;height:${height}px;font-family:Times New Roman;font-size:64px;line-height:80px;transform:rotate(10deg);transform-origin:${origin}">${tag === "div" ? "ab</div>" : ""}`;
      const target = document.getElementById("target"),
        text = tag === "div" ? target.firstChild : null;
      const range = text ? document.createRange() : null;
      if (range) range.selectNodeContents(target);
      const points = [0.1, 0.9].map((fraction) => {
        const angle = (10 * Math.PI) / 180,
          x = (tag === "div" ? 60.40625 : width) * fraction,
          y = height / 2;
        const ox = origin === "0px 0px" ? 0 : width / 2,
          oy = origin === "0px 0px" ? 0 : height / 2;
        return [
          100 + ox + Math.cos(angle) * (x - ox) - Math.sin(angle) * (y - oy),
          100 + oy + Math.sin(angle) * (x - ox) + Math.cos(angle) * (y - oy),
        ];
      });
      const hits = points.map(([x, y]) => {
        const caret = text ? document.caretPositionFromPoint(x, y) : null,
          legacy = text ? document.caretRangeFromPoint(x, y) : null;
        return {
          point: [x, y],
          elements: document
            .elementsFromPoint(x, y)
            .map((n) => (n === target ? "target" : n.tagName)),
          sameText: text ? caret?.offsetNode === text : null,
          offset: caret?.offset ?? null,
          caret: caret ? rectangle(caret.getClientRect()) : null,
          legacyText: text ? legacy?.startContainer === text : null,
          legacyOffset: legacy?.startOffset ?? null,
        };
      });
      rows.push({
        tag,
        origin,
        rect: rectangle(target.getBoundingClientRect()),
        rects: Array.from(target.getClientRects(), rectangle),
        client: [target.clientWidth, target.clientHeight],
        offset: [target.offsetLeft, target.offsetTop, target.offsetWidth, target.offsetHeight],
        scroll: [target.scrollWidth, target.scrollHeight, target.scrollLeft, target.scrollTop],
        range: range ? rectangle(range.getBoundingClientRect()) : null,
        rangeRects: range ? Array.from(range.getClientRects(), rectangle) : null,
        hits,
      });
    }
  return { rows };
})();
