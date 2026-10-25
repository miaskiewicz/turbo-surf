(async () => {
  // Fixed-pixel scrollers keep every observation independent of installed fonts.
  const out = {};
  // A capped frame wait keeps throttled background windows from stalling the control.
  const frame = () =>
    new Promise((done) => {
      requestAnimationFrame(() => done());
      setTimeout(done, 100);
    });
  const settle = async (count = 3) => {
    for (let i = 0; i < count; i++) await frame();
    await new Promise((done) => setTimeout(done, 30));
  };
  // Chrome156 scroll methods return promises: abrupt conversions become rejections.
  const invoke = (callback) => {
    try {
      const value = callback();
      return {
        value,
        sync: value instanceof Promise ? "promise" : value === undefined ? "undefined" : typeof value,
      };
    } catch (error) {
      return { sync: ["threw", error.constructor.name, error.message] };
    }
  };
  const outcome = async (value) => {
    if (!(value instanceof Promise)) return null;
    try {
      const result = await value;
      return [
        "resolved",
        result === undefined ? "undefined" : result === null ? "null" : String(result),
      ];
    } catch (error) {
      return ["rejected", error.constructor.name, error.message];
    }
  };
  const failure = async (callback) => {
    const call = invoke(callback);
    return [call.sync, await outcome(call.value)];
  };
  const describe = (target, name) => {
    const d = Object.getOwnPropertyDescriptor(target, name);
    if (!d) return null;
    const f = d.value || d.get;
    return {
      kind: "value" in d ? "value" : "accessor",
      writable: d.writable ?? null,
      enumerable: d.enumerable,
      configurable: d.configurable,
      setter: d.set ? [d.set.name, d.set.length] : null,
      name: f.name,
      length: f.length,
    };
  };
  const methods = ["scroll", "scrollTo", "scrollBy", "scrollIntoView", "scrollIntoViewIfNeeded"];
  out.shape = {
    element: Object.fromEntries(
      [...methods, "checkVisibility", "currentCSSZoom"].map((n) => [
        n,
        describe(Element.prototype, n),
      ]),
    ),
    window: Object.fromEntries(
      ["scroll", "scrollTo", "scrollBy", "scrollX", "scrollY", "pageXOffset", "pageYOffset"].map(
        (n) => [n, describe(window, n)],
      ),
    ),
    distinct: Element.prototype.scroll !== Element.prototype.scrollTo,
    windowDistinct: window.scroll !== window.scrollTo,
  };
  out.shape.brands = [];
  for (const n of [...methods, "checkVisibility"])
    out.shape.brands.push([
      n,
      await failure(() => Element.prototype[n].call({})),
      await failure(() => Element.prototype[n].call(document.createTextNode("x"))),
    ]);
  out.shape.windowBrands = [];
  for (const n of ["scroll", "scrollTo", "scrollBy"])
    out.shape.windowBrands.push([n, await failure(() => window[n].call({}, 0, 0))]);

  document.body.style.cssText = "margin:0";
  document.body.innerHTML = `
    <div id=spacer style="height:1000px;width:10px"></div>
    <div id=outer style="width:200px;height:150px;overflow:auto">
      <div style="height:300px;width:10px"></div>
      <div id=inner style="width:150px;height:100px;overflow:auto;margin-left:400px">
        <div style="height:250px;width:10px"></div>
        <div id=target style="width:20px;height:30px;margin-left:200px"></div>
        <div style="height:250px;width:10px"></div>
      </div>
      <div style="height:300px;width:10px"></div>
    </div>
    <div id=box style="width:100px;height:80px;overflow:auto"><div style="width:300px;height:400px"></div></div>
    <div id=hidden style="width:100px;height:80px;overflow:hidden"><div style="width:300px;height:400px"></div></div>
    <div id=visible style="width:100px;height:80px;overflow:visible"><div style="width:300px;height:400px"></div></div>
    <div id=clip style="width:100px;height:80px;overflow:clip"><div style="width:300px;height:400px"></div></div>
    <div id=none style="display:none;width:100px;height:80px;overflow:auto"><div style="width:300px;height:400px"></div></div>
    <div id=tall style="height:2000px;width:2500px"></div>`;
  const $ = (id) => document.getElementById(id);
  const [outer, inner, target, box] = ["outer", "inner", "target", "box"].map($);
  const offsets = (element) => [element.scrollLeft, element.scrollTop];
  const reset = () => {
    for (const element of [outer, inner, box]) element.scroll(0, 0);
    window.scroll(0, 0);
  };
  out.viewport = [innerWidth, innerHeight, devicePixelRatio];

  // Argument conversion order and overload/normalization behavior.
  const log = [];
  const watched = (name, value) => ({
    valueOf() {
      log.push(name);
      return value;
    },
  });
  const options = (names, values = {}) => {
    const object = {};
    for (const name of names)
      Object.defineProperty(object, name, {
        get() {
          log.push(name);
          return values[name];
        },
      });
    return object;
  };
  const conversions = {};
  for (const name of ["scroll", "scrollTo", "scrollBy"]) {
    const rows = [];
    for (const [label, args] of [
      ["none", []],
      ["number", [1]],
      ["string", ["x"]],
      ["null", [null]],
      ["undefined", [undefined]],
      ["pair", [10, 20]],
      ["nonFinitePair", [NaN, Infinity]],
      ["undefinedY", [7, undefined]],
      ["fraction", [0.3, 10.7]],
      ["three", [3, 4, 5]],
      ["leftOnly", [{ left: 15 }]],
      ["topOnly", [{ top: 25 }]],
      ["nonFiniteOptions", [{ left: NaN, top: -Infinity }]],
      ["badBehavior", [{ behavior: "bogus", left: 1 }]],
      ["instant", [{ behavior: "instant", left: 2, top: 3 }]],
      ["huge", [1e6, 1e6]],
      ["negative", [-1e6, -1e6]],
      ["bigint", [1n, 2]],
      ["symbolLeft", [{ left: Symbol() }]],
      ["valueOfBigInt", [{ valueOf: () => 1n }, 0]],
      ["userTypeError", [{ valueOf() { throw new TypeError("user"); } }, 0]],
      ["getterTypeError", [{ get left() { throw new TypeError("getter"); } }]],
      ["symbolBehavior", [{ behavior: Symbol() }]],
    ]) {
      await box.scroll(40, 50);
      log.length = 0;
      const call = invoke(() => box[name](...args));
      const now = offsets(box);
      rows.push([label, call.sync, now, await outcome(call.value), offsets(box)]);
    }
    log.length = 0;
    box[name](watched("x", 1), watched("y", 2));
    const pairOrder = [...log];
    log.length = 0;
    box[name](options(["top", "left", "behavior"], { left: 3, top: 4 }));
    const optionOrder = [...log];
    log.length = 0;
    const abrupt = await failure(() =>
      box[name](
        {
          valueOf() {
            log.push("x");
            throw new RangeError("x");
          },
        },
        watched("y", 0),
      ),
    );
    conversions[name] = { rows, pairOrder, optionOrder, abrupt, abruptLog: [...log] };
  }
  out.conversions = conversions;

  // Which boxes are programmatically scrollable.
  out.containers = ["box", "hidden", "visible", "clip", "none"].map((id) => {
    const element = $(id);
    const returned = invoke(() => element.scroll(30, 40)).sync;
    const after = offsets(element);
    element.scrollTop = 5;
    return [id, returned, after, offsets(element), element.scrollWidth];
  });
  box.scroll(0, 0);

  // The root element and body forward to the viewport in standards mode.
  const root = document.documentElement;
  out.root = [];
  for (const [label, action] of [
    ["window.scroll", () => window.scroll(11, 12)],
    ["window.scrollTo", () => window.scrollTo({ top: 120 })],
    ["window.scrollBy", () => window.scrollBy(5, 5)],
    ["window.scrollByOptions", () => window.scrollBy({ left: -100 })],
    ["root.scroll", () => root.scroll(30, 300)],
    ["root.scrollTop", () => (root.scrollTop = 333)],
    ["root.scrollLeft", () => (root.scrollLeft = 44)],
    ["body.scrollTop", () => (document.body.scrollTop = 99)],
    ["body.scroll", () => document.body.scroll(1, 1)],
    ["clamp", () => window.scroll(1e6, 1e6)],
    ["negative", () => window.scroll(-5, -5)],
    ["fraction", () => window.scroll(0.3, 10.7)],
  ]) {
    action();
    out.root.push([
      label,
      scrollX,
      scrollY,
      pageXOffset,
      pageYOffset,
      root.scrollLeft,
      root.scrollTop,
      document.body.scrollLeft,
      document.body.scrollTop,
      target.getBoundingClientRect().y,
    ]);
  }
  window.scroll(0, 0);
  out.rootExtent = [root.scrollWidth, root.scrollHeight];

  // scrollIntoView over nested element scrollers and the viewport.
  const snapshot = () => [
    ...offsets(inner),
    ...offsets(outer),
    scrollX,
    scrollY,
    target.getBoundingClientRect().x,
    target.getBoundingClientRect().y,
  ];
  out.intoView = [];
  for (const [label, arg, prepare] of [
    ["default", undefined],
    ["true", true],
    ["false", false],
    ["null", null],
    ["zero", 0],
    ["one", 1],
    ["empty", {}],
    ["startStart", { block: "start", inline: "start" }],
    ["centerCenter", { block: "center", inline: "center" }],
    ["endEnd", { block: "end", inline: "end" }],
    ["nearestNearest", { block: "nearest", inline: "nearest" }],
    ["nearestFromBelow", { block: "nearest", inline: "nearest" }, () => {
      inner.scroll(400, 400);
      outer.scroll(400, 600);
      window.scroll(0, 2000);
    }],
    ["nearestVisible", { block: "nearest", inline: "nearest" }, () => {
      target.scrollIntoView({ block: "center", inline: "center" });
    }],
    ["containerNearest", { block: "start", container: "nearest" }],
    ["smooth", { behavior: "smooth", block: "start" }],
  ]) {
    reset();
    prepare?.();
    target.scrollIntoView(arg);
    out.intoView.push([label, snapshot()]);
  }
  reset();
  await settle();
  log.length = 0;
  target.scrollIntoView(options(["inline", "container", "block", "behavior"], {}));
  out.intoViewOrder = [...log];
  out.intoViewErrors = [];
  for (const options of [
    { block: "middle" },
    { inline: "left" },
    { behavior: "fast" },
    { container: "self" },
  ])
    out.intoViewErrors.push(await failure(() => target.scrollIntoView(options)));
  reset();

  // Large element: nearest must not move when it already covers the scrollport.
  box.innerHTML =
    "<div style='height:50px'></div><div id=big style='width:300px;height:300px'></div><div style='height:100px'></div>";
  const big = $("big");
  box.scroll(0, 60);
  big.scrollIntoView({ block: "nearest" });
  out.bigNearest = offsets(box);
  box.scroll(0, 0);
  big.scrollIntoView({ block: "nearest" });
  out.bigNearestPartial = offsets(box);
  box.scroll(0, 400);
  big.scrollIntoView({ block: "nearest" });
  out.bigNearestBelow = offsets(box);

  // WebKit scrollIntoViewIfNeeded: fully visible, partial and hidden targets.
  out.ifNeeded = [];
  for (const [label, arg, prepare] of [
    ["visibleTrue", true, () => target.scrollIntoView({ block: "center", inline: "center" })],
    ["visibleFalse", false, () => target.scrollIntoView({ block: "center", inline: "center" })],
    ["hiddenDefault", undefined],
    ["hiddenTrue", true],
    ["hiddenFalse", false],
    ["hiddenZero", 0],
    ["hiddenObject", {}],
    ["partialTrue", true, () => {
      target.scrollIntoView({ block: "start", inline: "start" });
      inner.scrollTop -= 15;
    }],
    ["partialFalse", false, () => {
      target.scrollIntoView({ block: "start", inline: "start" });
      inner.scrollTop -= 15;
    }],
  ]) {
    reset();
    prepare?.();
    target.scrollIntoViewIfNeeded(arg);
    out.ifNeeded.push([label, snapshot()]);
  }
  reset();

  // Out-of-flow targets follow the containing-block chain.
  outer.insertAdjacentHTML(
    "beforeend",
    "<div id=fixed style='position:fixed;left:0;top:0;width:10px;height:10px'></div>",
  );
  inner.insertAdjacentHTML(
    "beforeend",
    "<div id=abs style='position:absolute;left:300px;top:3000px;width:10px;height:10px'></div>",
  );
  $("fixed").scrollIntoView();
  out.fixedIntoView = snapshot();
  reset();
  $("abs").scrollIntoView();
  out.absoluteIntoView = snapshot();
  $("fixed").remove();
  $("abs").remove();
  reset();
  await settle();

  return out;
})();

