(() => {
  const style = document.createElement("style");
  const box = document.createElement("div");
  box.id = "supports-box";
  document.body.append(style, box);
  const descriptor = Object.getOwnPropertyDescriptor(CSS, "supports");
  Object.defineProperty(CSS, "supports", {
    configurable: true,
    value() {
      throw new Error("public supports must stay unreachable");
    },
  });
  const conditions = [
    "(display: grid)",
    "(width: var(--w))",
    "(width: inherit)",
    "(width: nonsense)",
    "(width: nonsense inherit)",
    "(unknown: 1)",
    "(--token: 3)",
    "not (unknown: 1)",
    "(display: grid) and (not (width: nonsense))",
    "(unknown: 1) or (display: flex)",
    "future(1)",
  ];
  const widths = conditions.map((condition) => {
    style.textContent = `#supports-box{width:11px;height:7px}@supports ${condition}{#supports-box{width:29px}}`;
    return box.getBoundingClientRect().width;
  });
  style.textContent =
    "#supports-box{width:11px;height:7px}@supports(display:grid){#supports-box{width:29px}}#supports-box{width:37px}";
  const after = box.getBoundingClientRect().width;
  style.textContent =
    "#supports-box{width:11px;height:7px}@supports(display:grid){@media screen{#supports-box{width:41px}}}";
  const nested = box.getBoundingClientRect().width;
  Object.defineProperty(CSS, "supports", descriptor);
  style.remove();
  box.remove();
  return { widths, after, nested };
})();
