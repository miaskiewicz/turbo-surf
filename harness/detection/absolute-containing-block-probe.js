(() => {
  document.body.style.cssText = "margin:0";
  const cases = [
    [
      "pixels",
      "width:100px;height:80px;padding:5px;border:2px solid",
      "left:120px;top:90px;width:30px;height:20px",
    ],
    [
      "percent",
      "width:100px;height:80px;padding:5px;border:2px solid",
      "left:50%;top:50%;width:50%;height:50%",
    ],
    ["inset", "width:100px;height:80px;padding:5px;border:2px solid", "inset:0"],
    [
      "asymmetric",
      "width:100px;height:80px;padding:3px 7px 11px 13px;border-width:2px 4px 6px 8px;border-style:solid",
      "left:50%;top:50%;width:50%;height:50%",
    ],
    [
      "borderbox",
      "box-sizing:border-box;width:100px;height:80px;padding:5px;border:2px solid",
      "inset:0",
    ],
    [
      "autoheight",
      "width:100px;padding:5px;border:2px solid",
      "left:50%;bottom:0;width:50%;height:50%",
    ],
  ];
  const out = [];
  for (const [name, parentCSS, childCSS] of cases) {
    const parent = document.createElement("div");
    parent.style.cssText = `position:relative;${parentCSS}`;
    const normal = document.createElement("div");
    normal.style.cssText = "height:80px";
    parent.append(normal);
    const child = document.createElement("div");
    child.style.cssText = `position:absolute;${childCSS}`;
    parent.append(child);
    document.body.append(parent);
    const rect = (element) => {
      const r = element.getBoundingClientRect();
      return [r.x, r.y, r.width, r.height];
    };
    out.push({ name, parent: rect(parent), child: rect(child) });
    parent.remove();
  }
  return out;
})();
