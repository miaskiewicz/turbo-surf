(() => {
  // turbo-surf#105: size container queries, unnamed and named, evaluate against the
  // nearest ancestor container; geometry reads never throw. Every element: [label, rect].
  const label = (el) => el.tagName + (el.id ? "#" + el.id : el.className ? "." + el.className : "");
  return Array.from(document.body.querySelectorAll("*"), (el) => {
    try {
      const b = el.getBoundingClientRect();
      return [label(el), "ok", b.width, b.height];
    } catch (e) {
      return [label(el), "throw", e.name, e.message];
    }
  });
})();
