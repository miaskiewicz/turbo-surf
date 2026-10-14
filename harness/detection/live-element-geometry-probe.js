(() => {
  const saved = document.body.innerHTML;
  const style = document.createElement("style");
  style.textContent = "body{margin:0} .fixture{width:40px;height:20px}";
  document.head.appendChild(style);
  const output = { cases: [] };
  const rectangle = (rect) => ({ x: rect.x, y: rect.y, width: rect.width, height: rect.height });
  const dimensions = (element) => ({
    rect: rectangle(element.getBoundingClientRect()),
    rects: Array.from(element.getClientRects(), rectangle),
    client: [element.clientLeft, element.clientTop, element.clientWidth, element.clientHeight],
    offset: [element.offsetWidth, element.offsetHeight],
  });
  try {
    const cases = [
      ["content-box", "width:40px;height:20px;padding:5px;border:2px solid"],
      ["border-box", "width:40px;height:20px;padding:5px;border:2px solid;box-sizing:border-box"],
      [
        "translate-scale",
        "width:40px;height:20px;padding:5px;border:2px solid;transform:translate(3px,7px) scale(2);transform-origin:0 0",
      ],
      ["fractional", "width:40.25px;height:20.5px;padding:1.25px;border:1px solid"],
      ["hidden", "width:40px;height:20px;visibility:hidden"],
      ["transparent", "width:40px;height:20px;opacity:0"],
      ["display-none", "width:40px;height:20px;display:none"],
      ["margin", "width:40px;height:20px;margin:3px 7px"],
    ];
    for (const [name, css] of cases) {
      document.body.innerHTML = `<div id="target" style="${css}"></div>`;
      output.cases.push({ name, ...dimensions(document.getElementById("target")) });
    }
    document.body.innerHTML = '<div id="target" class="fixture"></div>';
    const element = document.getElementById("target");
    const before = dimensions(element);
    style.sheet.cssRules[1].style.width = "63px";
    output.mutation = { before, after: dimensions(element) };
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(".fixture{height:37px}");
    document.adoptedStyleSheets = [sheet];
    output.adoption = dimensions(element);
    Object.defineProperty(style, "textContent", {
      get() {
        throw Error("public text read");
      },
    });
    Object.defineProperty(style.sheet.cssRules[1], "cssText", {
      get() {
        throw Error("public rule read");
      },
    });
    Object.defineProperty(document, "adoptedStyleSheets", {
      get() {
        throw Error("public adoption read");
      },
    });
    output.privateReads = dimensions(element);
    const list = element.getClientRects();
    output.list = {
      tag: Object.prototype.toString.call(list),
      prototype: Object.getPrototypeOf(list) === DOMRectList.prototype,
      array: Array.isArray(list),
      length: list.length,
      indexEqualsItem: list[0] === list.item(0),
      nullOutOfBounds: list.item(9) === null,
      indexTag: Object.prototype.toString.call(list[0]),
      keys: Reflect.ownKeys(list).map(String),
      descriptors: Object.getOwnPropertyNames(DOMRectList.prototype).map((name) => {
        const descriptor = Object.getOwnPropertyDescriptor(DOMRectList.prototype, name);
        return [
          name,
          descriptor.enumerable,
          descriptor.configurable,
          "value" in descriptor ? descriptor.writable : null,
        ];
      }),
    };
    output.errors = [];
    for (const operation of [
      () => Element.prototype.getBoundingClientRect.call({}),
      () => Element.prototype.getClientRects.call({}),
      () => DOMRectList.prototype.item.call({}, 0),
      () => list.item(),
      () => list.item(1n),
      () => new DOMRectList(),
    ]) {
      try {
        operation();
        output.errors.push(null);
      } catch (error) {
        output.errors.push(error.name);
      }
    }
    element.remove();
    output.detached = dimensions(element);
    const inert = document.implementation.createHTMLDocument();
    inert.body.innerHTML = '<div style="width:40px;height:20px"></div>';
    output.inert = dimensions(inert.body.firstChild);
    return output;
  } finally {
    delete document.adoptedStyleSheets;
    document.adoptedStyleSheets = [];
    document.body.innerHTML = saved;
    style.remove();
  }
})();
