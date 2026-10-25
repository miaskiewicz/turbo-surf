(async () => {
  // HTMLInputElement width/height: rendered image buttons report their content
  // box; otherwise the attribute, then the available image, then zero.
  const out = {};
  const body = document.body;
  const failure = (callback) => {
    try {
      return ["returned", callback()];
    } catch (error) {
      return ["threw", error.constructor.name, error.message];
    }
  };
  const proto = HTMLInputElement.prototype;
  const describe = (name) => {
    const d = Object.getOwnPropertyDescriptor(proto, name);
    return (
      d && {
        kind: "value" in d ? "value" : "accessor",
        enumerable: d.enumerable,
        configurable: d.configurable,
        get: d.get ? [d.get.name, d.get.length] : null,
        set: d.set ? [d.set.name, d.set.length] : null,
      }
    );
  };
  out.shape = {
    width: describe("width"),
    height: describe("height"),
    brands: ["width", "height"].map((name) => {
      const d = Object.getOwnPropertyDescriptor(proto, name);
      return [
        failure(() => d.get.call({})),
        failure(() => d.get.call(document.createElement("img"))),
        failure(() => d.set.call({}, 1)),
        failure(() => d.set.call(document.createElement("img"), 1)),
      ];
    }),
  };
  body.style.cssText = "margin:0";
  const make = (markup) => {
    body.innerHTML = markup;
    const input = body.querySelector("input");
    // Run style and layout so every rendered control has its layout box.
    void input.offsetWidth;
    return input;
  };
  const read = (input) => [
    input.width,
    input.height,
    input.getAttribute("width"),
    input.getAttribute("height"),
  ];
  const cases = [
    ["imageAttributes", "<input type=image width=40 height=30>"],
    ["imageCss", "<input type=image width=40 height=30 style='width:77px;height:21px'>"],
    [
      "imageBoxes",
      "<input type=image width=40 height=30 style='padding:3px 5px;border:2px solid;box-sizing:border-box'>",
    ],
    [
      "imageContentBox",
      "<input type=image width=40 height=30 style='padding:3px 5px;border:2px solid'>",
    ],
    ["imageZoom", "<div style='zoom:2'><input type=image width=40 height=30></div>"],
    ["imageZoomCss", "<div style='zoom:2'><input type=image style='width:15px;height:9px'></div>"],
    ["imageNone", "<input type=image width=40 height=30 style='display:none;width:77px'>"],
    ["imageNoneInvalid", "<input type=image width=abc height=-4 style='display:none'>"],
    ["imageNoneUnits", "<input type=image width=40px height=' 12.9' style='display:none'>"],
    ["imageNoneEmpty", "<input type=image style='display:none'>"],
    ["imageInsideNone", "<div style='display:none'><input type=image width=40 height=30></div>"],
    ["imageContents", "<input type=image width=40 height=30 style='display:contents'>"],
    ["imageBlock", "<input type=image width=40 height=30 style='display:block;width:auto'>"],
    ["imageFloat", "<input type=image width=12 height=8 style='float:left'>"],
    ["text", "<input type=text width=40 height=30>"],
    ["textCss", "<input width=40 height=30 style='width:77px;height:21px'>"],
    ["hidden", "<input type=hidden width=40 height=30>"],
    ["checkbox", "<input type=checkbox width=40 height=30>"],
    ["submit", "<input type=submit width=40 height=30>"],
    ["imageUpper", "<input type=IMAGE width=40 height=30 style='width:25px;height:35px'>"],
  ];
  out.cases = {};
  for (const [name, markup] of cases) out.cases[name] = read(make(markup));

  // Disconnected and frame-less inputs use the attribute (or zero).
  const detached = document.createElement("input");
  detached.type = "image";
  detached.setAttribute("width", "33");
  detached.setAttribute("height", "x");
  out.detached = read(detached);
  const inert = document.implementation.createHTMLDocument("").createElement("input");
  inert.type = "image";
  inert.setAttribute("width", "44");
  inert.style.width = "60px";
  out.inert = read(inert);

  // Type switches and style changes are observed on the next read.
  const input = make("<input width=40 height=30 style='width:50px;height:20px'>");
  const switching = [read(input)];
  input.type = "image";
  switching.push(read(input));
  input.style.width = "61px";
  switching.push(read(input));
  input.type = "text";
  switching.push(read(input));
  out.switching = switching;

  // Setter conversion: [CEReactions] unsigned long reflection on every type.
  const setter = document.createElement("input");
  const conversions = [];
  for (const value of [
    5,
    3.7,
    -1,
    2147483647,
    2147483648,
    4294967295,
    4294967296,
    "12",
    "abc",
    null,
    NaN,
  ]) {
    setter.width = value;
    setter.height = value;
    conversions.push([
      String(value),
      setter.getAttribute("width"),
      setter.getAttribute("height"),
      setter.width,
    ]);
  }
  out.conversions = conversions;
  setter.removeAttribute("width");
  out.removed = [setter.width, setter.getAttribute("width")];
  const typed = document.createElement("input");
  typed.type = "image";
  typed.width = 9;
  out.imageSetter = [typed.getAttribute("width"), typed.width];

  // An available image supplies its natural size when no attribute applies.
  const source =
    "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAYAAAAECAYAAACtBE5DAAAAEklEQVR4nGP4z8DwHxtmoJ4EAFeLL9Fq2vKHAAAAAElFTkSuQmCC";
  const loadedInput = async (markup) => {
    body.innerHTML = markup;
    const element = body.querySelector("input");
    await new Promise((done) => {
      element.addEventListener("load", done, { once: true });
      element.addEventListener("error", done, { once: true });
      setTimeout(done, 1000);
    });
    void element.offsetWidth;
    return element;
  };
  out.images = {
    hidden: read(await loadedInput(`<input type=image src="${source}" style="display:none">`)),
    hiddenAttribute: read(
      await loadedInput(`<input type=image width=9 src="${source}" style="display:none">`),
    ),
    rendered: read(await loadedInput(`<input type=image src="${source}">`)),
    renderedZoom: read(
      await loadedInput(`<div style="zoom:2"><input type=image src="${source}"></div>`),
    ),
  };

  // A freshly inserted control has no layout box until the next style update.
  body.innerHTML = "";
  const fresh = document.createElement("input");
  fresh.type = "image";
  fresh.setAttribute("width", "40");
  fresh.style.width = "70px";
  body.append(fresh);
  const before = fresh.width;
  void fresh.offsetWidth;
  out.fresh = [before, fresh.width];
  body.removeAttribute("style");
  body.innerHTML = "";
  return out;
})();
