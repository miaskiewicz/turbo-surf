(() => {
  const cases = [
    document.implementation.createHTMLDocument(""),
    new Document(),
    document.implementation.createDocument("urn:fixture", "p:Root"),
    document.implementation.createDocument("http://www.w3.org/1999/xhtml", "html"),
  ];
  const out = [];
  for (const doc of cases) {
    const element = doc.createElement("child");
    element.setAttribute("style", "width:40px;height:20px");
    (doc.body || doc.documentElement || doc).append(element);
    const rect = element.getBoundingClientRect();
    out.push({
      type: doc.contentType,
      rect: [rect.x, rect.y, rect.width, rect.height],
      rects: element.getClientRects().length,
      client: [element.clientWidth, element.clientHeight],
      scroll: [element.scrollWidth, element.scrollHeight, element.scrollLeft, element.scrollTop],
      hit: doc.elementFromPoint(1, 1),
      hits: doc.elementsFromPoint(1, 1).length,
    });
  }
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  document.body.append(svg);
  const detached = document.createElement("div");
  const rect = detached.getBoundingClientRect();
  out.push({
    detached: [rect.x, rect.y, rect.width, rect.height],
    rects: detached.getClientRects().length,
  });
  svg.remove();
  return out;
})();
