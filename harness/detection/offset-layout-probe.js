(() => {
  document.body.style.cssText = "margin:12px;padding:5px;border:3px solid";
  const cases = [
    ["static", "", "width:40px;height:20px;margin:4px"],
    [
      "relativeParent",
      "position:relative;padding:7px;border:2px solid",
      "width:40px;height:20px;margin:4px",
    ],
    [
      "absolute",
      "position:relative;padding:7px;border:2px solid",
      "position:absolute;left:20px;top:30px;width:40px;height:20px",
    ],
    [
      "relative",
      "position:relative;padding:7px;border:2px solid",
      "position:relative;left:6px;top:8px;width:40px;height:20px",
    ],
    [
      "fixed",
      "padding:7px;border:2px solid",
      "position:fixed;left:20px;top:30px;width:40px;height:20px",
    ],
    [
      "transform",
      "transform:translate(50px,40px);padding:7px;border:2px solid",
      "width:40px;height:20px;margin:4px",
    ],
    ["none", "", "display:none;width:40px;height:20px"],
  ];
  const out = [];
  const summary = (element) => ({
    left: element.offsetLeft,
    top: element.offsetTop,
    parent: element.offsetParent?.id || element.offsetParent?.localName || null,
    width: element.offsetWidth,
    height: element.offsetHeight,
    rect: [element.getBoundingClientRect().x, element.getBoundingClientRect().y],
  });
  for (const [name, outer, inner] of cases) {
    const parent = document.createElement("div");
    parent.id = "parent";
    parent.style.cssText = `width:100px;height:80px;${outer}`;
    const child = document.createElement("div");
    child.id = "child";
    child.style.cssText = inner;
    parent.append(child);
    document.body.append(parent);
    out.push({ name, parent: summary(parent), child: summary(child) });
    parent.remove();
  }
  out.push({
    name: "document",
    html: summary(document.documentElement),
    body: summary(document.body),
  });
  return out;
})();
