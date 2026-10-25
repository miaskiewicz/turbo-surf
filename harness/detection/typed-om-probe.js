(() => {
  // CSS Typed OM members that need the embedding: the CSS.* numeric factories on the CSS
  // namespace, DOMMatrix-backed toMatrix() and CSSMatrixComponent, matrix() reification, and
  // computedStyleMap() in the full page runtime. The binding's own control covers the rest.
  const out = {};
  const N = (x) => (Object.is(x, -0) ? "-0" : Number.isFinite(x) ? x : String(x));
  const E = (f) => {
    try {
      const v = f();
      return ["ok", v];
    } catch (e) {
      return [e.name, e.message];
    }
  };
  // Entries are rounded: DOMMatrix trigonometry precision belongs to the geometry layer.
  const m = (matrix) => [Array.from(matrix.toFloat64Array(), (v) => { const r = Math.round(v * 1e9) / 1e9; return r === 0 ? 0 : r; }).join(", "), matrix.is2D, Object.prototype.toString.call(matrix)];
  const px = (v) => new CSSUnitValue(v, "px");
  const deg = (v) => new CSSUnitValue(v, "deg");
  const omit = new Set(["highlights", "registerProperty", "paintWorklet"]);
  const globalCss = Object.getOwnPropertyDescriptor(window, "CSS");
  out.global = [Object.prototype.toString.call(CSS), globalCss.writable, globalCss.enumerable, globalCss.configurable, typeof CSS];
  out.namespace = Object.getOwnPropertyNames(CSS)
    .filter((k) => !omit.has(k))
    .map((k) => {
      const d = Object.getOwnPropertyDescriptor(CSS, k);
      return [
        k,
        typeof d.value,
        d.value.name,
        d.value.length,
        d.writable,
        d.enumerable,
        d.configurable,
        Object.prototype.hasOwnProperty.call(d.value, "prototype"),
      ];
    });
  const units = Object.getOwnPropertyNames(CSS).filter(
    (k) => !omit.has(k) && k !== "escape" && k !== "supports",
  );
  out.factories = units.map((u) => [
    u,
    E(() => {
      const v = CSS[u](1.5);
      return [v instanceof CSSUnitValue, v.unit, N(v.value), String(v)];
    }),
  ]);
  out.factoryErrors = [
    E(() => CSS.px()),
    E(() => CSS.px("x")),
    E(() => CSS.px(Infinity)),
    E(() => String(CSS.px("2"))),
    E(() => String(CSS.px.call(null, 3))),
    E(() => new CSS.px(1)),
    E(() => CSS.number(1n)),
    E(() => CSS.percent(Symbol.iterator)),
    E(() => String(CSS.Q(1))),
    E(() => String(CSS.kHz(1))),
    E(() => CSS.px(1) === CSS.px(1)),
    E(() => String(CSS.px(1).add(CSS.percent(50)))),
    E(() => String(CSS.em(2).mul(CSS.number(3)))),
  ];
  out.toMatrix = [
    E(() => m(new CSSTranslate(px(1), px(2)).toMatrix())),
    E(() => m(new CSSTranslate(px(1), px(2), px(3)).toMatrix())),
    E(() => m(new CSSTranslate(new CSSUnitValue(1, "in"), px(0)).toMatrix())),
    E(() => m(new CSSTranslate(new CSSUnitValue(1, "em"), px(0)).toMatrix())),
    E(() => m(new CSSTranslate(new CSSUnitValue(10, "percent"), px(0)).toMatrix())),
    E(() => m(new CSSRotate(deg(90)).toMatrix())),
    E(() => m(new CSSRotate(1, 0, 0, deg(90)).toMatrix())),
    E(() => m(new CSSRotate(new CSSUnitValue(0.25, "turn")).toMatrix())),
    E(() => m(new CSSScale(2, 3).toMatrix())),
    E(() => m(new CSSScale(2, 3, 4).toMatrix())),
    E(() => m(new CSSSkew(deg(45), deg(0)).toMatrix())),
    E(() => m(new CSSSkewX(deg(45)).toMatrix())),
    E(() => m(new CSSSkewY(deg(30)).toMatrix())),
    E(() => m(new CSSPerspective(px(100)).toMatrix())),
    E(() => m(new CSSPerspective(px(0.5)).toMatrix())),
    E(() => m(new CSSPerspective("none").toMatrix())),
    E(() => m(new CSSPerspective(new CSSUnitValue(1, "em")).toMatrix())),
    E(() =>
      m(
        new CSSTransformValue([
          new CSSTranslate(px(10), px(0)),
          new CSSRotate(deg(90)),
          new CSSScale(2, 2),
        ]).toMatrix(),
      ),
    ),
    E(() => m(new CSSTransformValue([new CSSTranslate(px(1), px(1), px(1))]).toMatrix())),
    E(() => {
      const t = new CSSTranslate(px(1), px(2));
      t.is2D = false;
      return m(t.toMatrix());
    }),
    E(() => {
      const r = new CSSRotate(1, 1, 0, deg(10));
      r.is2D = true;
      return m(r.toMatrix());
    }),
  ];
  out.matrixComponent = [
    E(() => {
      const c = new CSSMatrixComponent(new DOMMatrix([1, 2, 3, 4, 5, 6]));
      return [
        String(c),
        c.is2D,
        c.matrix instanceof DOMMatrix,
        c.matrix === c.matrix,
        m(c.toMatrix()),
      ];
    }),
    E(() => {
      const c = new CSSMatrixComponent(new DOMMatrixReadOnly([1, 0, 0, 1, 7, 8]));
      return [String(c), c.is2D, Object.prototype.toString.call(c.matrix)];
    }),
    E(() => {
      const c = new CSSMatrixComponent(
        new DOMMatrix([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 5, 6, 7, 1]),
      );
      return [String(c), c.is2D, m(c.toMatrix())];
    }),
    E(() => {
      const c = new CSSMatrixComponent(
        new DOMMatrix([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 5, 6, 7, 1]),
        { is2D: true },
      );
      return [String(c), c.is2D, m(c.toMatrix())];
    }),
    E(() => {
      const c = new CSSMatrixComponent(new DOMMatrix([1, 2, 3, 4, 5, 6]), { is2D: false });
      return [String(c), c.is2D];
    }),
    E(() => new CSSMatrixComponent()),
    E(() => new CSSMatrixComponent({})),
    E(() => new CSSMatrixComponent(new DOMMatrix(), 5)),
    E(() => {
      const c = new CSSMatrixComponent(new DOMMatrix());
      c.matrix = new DOMMatrixReadOnly();
      return "set";
    }),
    E(() => {
      const c = new CSSMatrixComponent(new DOMMatrix());
      c.matrix = new DOMMatrix([2, 0, 0, 2, 0, 0]);
      return String(c);
    }),
    E(() => {
      const c = new CSSMatrixComponent(new DOMMatrix([1.5, 0, 0, 1, 0, 0]));
      return String(new CSSTransformValue([c, new CSSScale(2, 2)]));
    }),
  ];
  out.reify = [
    "matrix(1, 2, 3, 4, 5, 6)",
    "matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 2, 3, 1)",
    "translate(10px) matrix(1, 0, 0, 1, 0, 0)",
    "matrix(1, 0, 0, 1, calc(1px + 1px), 0)",
  ].map((text) => [
    text,
    E(() => {
      const v = CSSStyleValue.parse("transform", text);
      return [
        v.constructor.name,
        String(v),
        v instanceof CSSTransformValue
          ? [...v].map((c) => [c.constructor.name, c.is2D, String(c)])
          : null,
      ];
    }),
  ]);
  const host = document.createElement("div");
  host.style.cssText =
    "width: 50%; background-image: url(img/a.png); transform: rotate(45deg) translateX(1em); font-size: 10px";
  document.body.append(host);
  const map = host.computedStyleMap();
  const origin = location.origin;
  out.computed = [
    map.size,
    String(map.get("width")),
    String(map.get("background-image")).split(origin).join("<origin>"),
    map.get("background-image").constructor.name,
    String(map.get("transform")),
    map.get("transform").constructor.name,
    E(() => m(map.get("transform").toMatrix())),
    String(document.body.computedStyleMap().get("margin-top")),
  ];
  host.remove();
  return out;
})();
