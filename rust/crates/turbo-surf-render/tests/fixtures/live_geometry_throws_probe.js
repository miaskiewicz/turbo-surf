(() => {
  // turbo-surf#31: getBoundingClientRect / getComputedStyle / elementFromPoint never throw in
  // Chrome, for table rows and sections, cells with shadow content and visually-hidden
  // (sr-only) positioned spans alike. Each row: [element, rect, style, hit] where every entry
  // is ['ok', value] or ['throw', name, message].
  const props = ["display", "position", "visibility"];
  const attempt = (f) => {
    try {
      return ["ok", f()];
    } catch (e) {
      return ["throw", e.name, e.message];
    }
  };
  const label = (el) => el.tagName + (el.id ? "#" + el.id : "");
  return Array.from(document.querySelectorAll("*"), (el) => {
    const rect = attempt(() => {
      const b = el.getBoundingClientRect();
      return [b.x, b.y, b.width, b.height];
    });
    const style = attempt(() => {
      const cs = getComputedStyle(el);
      return props.map((p) => cs.getPropertyValue(p));
    });
    const hit = attempt(() => {
      const b = el.getBoundingClientRect();
      const h = document.elementFromPoint(b.x + b.width / 2, b.y + b.height / 2);
      return h ? label(h) : null;
    });
    return [label(el), rect, style, hit];
  });
})();
