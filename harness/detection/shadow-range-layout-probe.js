(() => {
  document.body.innerHTML = "<style>body{margin:0;font:16px/20px monospace}</style>";
  const host = document.createElement("div");
  host.style.cssText = "width:240px;margin:10px";
  const light = document.createElement("span");
  light.textContent = "light";
  host.append(light);
  document.body.append(host);
  const root = host.attachShadow({ mode: "closed", slotAssignment: "manual" });
  root.innerHTML =
    '<style>span{display:block}slot{display:contents}</style><span id="shadow">shadow</span><slot></slot><slot></slot>';
  const slots = root.querySelectorAll("slot");
  slots[0].assign(light);
  const shadow = root.querySelector("#shadow");
  const ranges = [document.createRange(), document.createRange(), document.createRange()];
  ranges[0].selectNodeContents(host);
  ranges[1].selectNodeContents(light.firstChild);
  ranges[2].selectNodeContents(shadow.firstChild);
  const rect = (value) => {
    const r = value.getBoundingClientRect();
    return [r.x, r.y, r.width, r.height];
  };
  const sample = () =>
    ranges.map((range) => ({
      text: range.toString(),
      rect: rect(range),
      rects: Array.from(range.getClientRects(), (value) => [
        value.x,
        value.y,
        value.width,
        value.height,
      ]),
    }));
  const before = sample();
  slots[1].assign(light);
  const reassigned = sample();
  slots[1].assign();
  const unassigned = {
    ranges: sample(),
    light: rect(light),
    client: [light.clientWidth, light.clientHeight],
    offset: [
      light.offsetWidth,
      light.offsetHeight,
      light.offsetLeft,
      light.offsetTop,
      light.offsetParent === null,
    ],
    scroll: [light.scrollWidth, light.scrollHeight],
  };
  host.remove();
  const retired = sample();
  return { before, reassigned, unassigned, retired };
})();
