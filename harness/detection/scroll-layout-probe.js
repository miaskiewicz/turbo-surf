(() => {
  const cases = [
    ["padded", "padding:5px;border:2px solid;overflow:auto", "width:180px;height:140px"],
    ["visible", "padding:5px;border:2px solid;overflow:visible", "width:180px;height:140px"],
    ["hidden", "padding:5px;border:2px solid;overflow:hidden", "width:180px;height:140px"],
    [
      "negative",
      "padding:5px;border:2px solid;position:relative;overflow:auto",
      "position:absolute;left:-50px;top:-40px;width:20px;height:20px",
    ],
    [
      "absolute",
      "padding:5px;border:2px solid;position:relative;overflow:auto",
      "position:absolute;left:120px;top:90px;width:30px;height:20px",
    ],
    [
      "transform",
      "padding:5px;border:2px solid;overflow:auto",
      "width:50px;height:30px;transform:translate(120px,90px)",
    ],
    [
      "margin",
      "padding:5px;border:2px solid;overflow:auto",
      "width:180px;height:140px;margin:10px",
    ],
    [
      "scaledContainer",
      "padding:5px;border:2px solid;overflow:auto;transform:scale(2);transform-origin:0 0",
      "width:180px;height:140px",
    ],
    [
      "transformedNegative",
      "padding:5px;border:2px solid;overflow:auto",
      "width:180px;height:140px;transform:translate(-100px,-80px)",
    ],
    [
      "nestedclip",
      "padding:5px;border:2px solid;overflow:auto",
      "width:50px;height:30px;overflow:hidden",
    ],
  ];
  document.body.style.cssText = "margin:0";
  const results = [];
  for (const [name, outer, inner] of cases) {
    const element = document.createElement("div");
    element.style.cssText = `width:100px;height:80px;${outer}`;
    const child = document.createElement("div");
    child.style.cssText = inner;
    if (name === "nestedclip") {
      const nested = document.createElement("div");
      nested.style.cssText = "width:250px;height:220px";
      child.append(nested);
    }
    element.append(child);
    document.body.append(element);
    const state = () => ({
      width: element.scrollWidth,
      height: element.scrollHeight,
      left: element.scrollLeft,
      top: element.scrollTop,
      clientWidth: element.clientWidth,
      clientHeight: element.clientHeight,
      rect: [element.getBoundingClientRect().x, element.getBoundingClientRect().y],
      child: [child.getBoundingClientRect().x, child.getBoundingClientRect().y],
    });
    const before = state();
    element.scrollLeft = 1e6;
    element.scrollTop = 1e6;
    const after = state();
    element.scrollLeft = -1e6;
    element.scrollTop = -1e6;
    const negative = state();
    results.push({ name, before, after, negative });
    element.remove();
  }
  return results;
})();
