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
  // checkVisibility over boxes, ancestors and CSS options.
  document.body.innerHTML = `
    <div id=plain>x</div>
    <div id=noneSelf style="display:none"></div>
    <div style="display:none"><div id=noneParent></div></div>
    <div id=contents style="display:contents"><div id=contentsChild></div></div>
    <div id=hiddenSelf style="visibility:hidden"></div>
    <div id=collapseSelf style="visibility:collapse"></div>
    <div style="visibility:hidden"><div id=hiddenChild></div><div id=visibleChild style="visibility:visible"></div></div>
    <div id=transparent style="opacity:0"><div id=transparentChild></div></div>
    <div id=translucent style="opacity:0.5"></div>
    <div id=cvHidden style="content-visibility:hidden"><div id=cvHiddenChild></div></div>
    <div id=cvAuto style="content-visibility:auto"><div id=cvAutoChild></div></div>
    <div id=host></div>
    <div id=zero style="width:0;height:0"></div>
    <div style="height:3000px"></div>
    <div id=cvAutoFar style="content-visibility:auto;height:10px"><div id=cvAutoFarChild></div></div>`;
  const host = $("host");
  const shadow = host.attachShadow({ mode: "open" });
  shadow.innerHTML = "<span id=shadowSpan>s</span>";
  host.innerHTML = "<i id=unslotted></i>";
  // Content relevance is decided by a rendering update, not at insertion time.
  await settle();
  const detached = document.createElement("div");
  const variants = [
    undefined,
    { checkOpacity: true },
    { opacityProperty: true },
    { checkVisibilityCSS: true },
    { visibilityProperty: true },
    { contentVisibilityAuto: true },
  ];
  const candidates = [
    ...[
      "plain",
      "noneSelf",
      "noneParent",
      "contents",
      "contentsChild",
      "hiddenSelf",
      "collapseSelf",
      "hiddenChild",
      "visibleChild",
      "transparent",
      "transparentChild",
      "translucent",
      "cvHidden",
      "cvHiddenChild",
      "cvAuto",
      "cvAutoChild",
      "host",
      "unslotted",
      "zero",
      "cvAutoFar",
      "cvAutoFarChild",
    ].map((id) => [id, $(id)]),
    ["shadowSpan", shadow.getElementById("shadowSpan")],
    ["detached", detached],
    ["html", document.documentElement],
    ["head", document.head],
  ];
  out.visibility = candidates.map(([id, element]) => [
    id,
    variants.map((options) => element.checkVisibility(options)),
  ]);
  log.length = 0;
  $("plain").checkVisibility(
    options([
      "visibilityProperty",
      "opacityProperty",
      "contentVisibilityAuto",
      "checkVisibilityCSS",
      "checkOpacity",
    ]),
  );
  out.visibilityOrder = [...log];
  out.visibilityArguments = [];
  for (const value of [1, "x", null, true])
    out.visibilityArguments.push(await failure(() => $("plain").checkVisibility(value)));
  return out;
})();

