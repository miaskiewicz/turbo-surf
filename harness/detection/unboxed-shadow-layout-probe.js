(() => {
  document.body.innerHTML = "<style>body{margin:0}</style>";
  const host = document.createElement("div");
  host.style.cssText = "position:relative;width:200px;margin:10px";
  const child = document.createElement("div");
  child.style.cssText = "width:40px;height:20px";
  host.append(child);
  document.body.append(host);
  const root = host.attachShadow({ mode: "closed" });
  root.innerHTML =
    "<style>slot{position:relative;overflow:hidden;width:1px;height:1px;transform:translateX(30px)}</style><slot></slot>";
  const slot = root.querySelector("slot");
  const rect = (node) => {
    const value = node.getBoundingClientRect();
    return [value.x, value.y, value.width, value.height];
  };
  const metrics = (node) => [
    node.clientWidth,
    node.clientHeight,
    node.offsetWidth,
    node.offsetHeight,
    node.offsetLeft,
    node.offsetTop,
    node.offsetParent === null,
    node.scrollWidth,
    node.scrollHeight,
    node.scrollLeft,
    node.scrollTop,
  ];
  const before = {
    slot: rect(slot),
    rectCount: slot.getClientRects().length,
    metrics: metrics(slot),
    child: rect(child),
    hit: document.elementFromPoint(20, 15) === child,
  };
  slot.scrollLeft = 20;
  slot.scrollTop = 20;
  const after = metrics(slot);
  root.querySelector("style").textContent = "slot{display:block;width:60px;height:30px}";
  const boxed = { slot: rect(slot), rectCount: slot.getClientRects().length, child: rect(child) };
  return { before, after, boxed };
})();
