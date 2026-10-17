(() => {
  const read = getComputedStyle;
  const parent = read(document.getElementById("target"));
  const frame = document.createElement("iframe");
  document.body.appendChild(frame);
  let child;
  try {
    child = frame.contentDocument;
    const style = child.createElement("style");
    style.textContent = ".target {width:61px;color:blue}";
    child.head.appendChild(style);
    const target = child.createElement("div");
    target.className = "target";
    child.body.appendChild(target);
    const sheet = style.sheet;
    const retained = read(target);
    let sheetReads = 0;
    Object.defineProperty(child, "styleSheets", {
      configurable: true,
      get() {
        sheetReads++;
        throw Error("public foreign stylesheet getter");
      },
    });
    const values = [[retained.width, retained.color]];
    sheet.cssRules[0].style.width = "67px";
    values.push([retained.width, retained.color]);
    values.push([parent.width, parent.color]);
    return { values, sheetReads };
  } finally {
    if (child) delete child.styleSheets;
    frame.remove();
  }
})();
