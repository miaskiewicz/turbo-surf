(() => {
  const rect = (node) => {
    const r = node.getBoundingClientRect();
    return [r.x, r.y, r.width, r.height];
  };
  const result = [];
  for (const mode of ["open", "closed"])
    for (const manual of [false, true]) {
      document.body.innerHTML =
        '<style>body{margin:0;font:16px/20px Arial}#inner{width:900px}span{display:block;width:120px;height:18px}</style><div id="host" style="display:block;width:240px;margin:10px;padding:4px;border:2px solid"><span id="first" slot="named">first</span><span id="second">second</span></div>';
      const host = document.getElementById("host"),
        first = document.getElementById("first"),
        second = document.getElementById("second");
      const root = host.attachShadow({ mode, slotAssignment: manual ? "manual" : "named" });
      root.innerHTML =
        '<style>:host{height:auto}#inner{width:64px;height:24px}slot::slotted(span){width:96px}slot{font-size:20px;line-height:24px}</style><div id="inner"></div><slot name="named"><b id="fallback">fallback</b></slot><slot></slot>';
      const inner = root.getElementById("inner"),
        slots = root.querySelectorAll("slot");
      if (manual) {
        slots[0].assign(first);
        slots[1].assign(second);
      }
      const sample = () => ({
        host: rect(host),
        inner: rect(inner),
        first: rect(first),
        second: rect(second),
        slots: Array.from(slots, rect),
        documentHit: document.elementFromPoint(20, 20) === host,
        shadowHit: root.elementFromPoint(20, 20) === inner,
      });
      const before = sample();
      if (manual) {
        slots[0].assign(second);
        slots[1].assign(first);
      } else first.slot = "";
      const after = sample();
      const range = document.createRange();
      range.selectNodeContents(inner);
      const rangeRect = rect(range);
      result.push({ mode, manual, before, after, range: rangeRect });
    }
  return result;
})();
