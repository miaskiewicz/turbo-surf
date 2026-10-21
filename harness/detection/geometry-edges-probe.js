(() => {
  document.body.style.cssText = "margin:0";
  const error = (method, x, y) => {
    try {
      const value = document[method](x, y);
      return {
        ok: true,
        value:
          method === "elementFromPoint"
            ? value?.localName || null
            : value.map((node) => node.localName),
      };
    } catch (error) {
      return { ok: false, name: error.name, message: error.message };
    }
  };
  const points = [];
  for (const method of ["elementFromPoint", "elementsFromPoint"])
    for (const value of [NaN, Infinity, -Infinity, undefined, -1])
      points.push({ method, input: String(value), result: error(method, value, 5) });
  const parent = document.createElement("div");
  parent.style.cssText = "position:relative;width:100px;height:80px";
  document.body.append(parent);
  const offsets = [];
  for (const value of [-1.5, -0.75, -0.5, -0.25, 0.1, 0.25, 0.5, 0.75, 1.5]) {
    const child = document.createElement("div");
    child.style.cssText = `position:absolute;left:${value}px;top:${value}px;width:10px;height:10px`;
    parent.append(child);
    const rect = child.getBoundingClientRect();
    offsets.push({
      input: value,
      left: child.offsetLeft,
      top: child.offsetTop,
      rect: [rect.x, rect.y],
    });
    child.remove();
  }
  parent.remove();
  const scrolls = [];
  for (const mode of [
    "overflow:visible hidden",
    "overflow-x:visible;overflow-y:hidden",
    "overflow-x:clip;overflow-y:auto",
    "overflow:clip",
    "overflow:auto",
  ]) {
    const box = document.createElement("div");
    box.style.cssText = `width:100.5px;height:80.5px;padding:5.25px;border:1px solid;${mode}`;
    const child = document.createElement("div");
    child.style.cssText = "width:180.75px;height:140.125px";
    box.append(child);
    document.body.append(box);
    const samples = [];
    for (const value of [0.125, 0.25, 0.5, 0.75, 1.25, 1e6]) {
      box.scrollLeft = value;
      box.scrollTop = value;
      samples.push([box.scrollLeft, box.scrollTop]);
    }
    scrolls.push({
      mode,
      width: box.scrollWidth,
      height: box.scrollHeight,
      client: [box.clientWidth, box.clientHeight],
      samples,
    });
    box.remove();
  }
  return { points, offsets, scrolls };
})();
