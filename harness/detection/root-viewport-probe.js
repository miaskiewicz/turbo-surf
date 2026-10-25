(async () => {
  // Root/body CSSOM View metrics. The same fixed-pixel cases run in a standards
  // and a quirks document; html/body heights are explicit wherever the quirks
  // percentage-height/body-fill layout rules would otherwise decide them.
  const out = { mode: document.compatMode };
  const root = document.documentElement;
  const body = document.body;
  const rect = (element) => {
    const r = element.getBoundingClientRect();
    return [r.x, r.y, r.width, r.height];
  };
  const metrics = (element) => ({
    client: [element.clientLeft, element.clientTop, element.clientWidth, element.clientHeight],
    scroll: [element.scrollLeft, element.scrollTop, element.scrollWidth, element.scrollHeight],
    offset: [element.offsetLeft, element.offsetTop, element.offsetWidth, element.offsetHeight],
    rect: rect(element),
  });
  const scrolling = () => {
    const element = document.scrollingElement;
    return element === root ? "html" : element === body ? "body" : String(element);
  };
  const read = () => ({
    html: metrics(root),
    body: metrics(body),
    scrollingElement: scrolling(),
    window: [innerWidth, innerHeight, scrollX, scrollY],
  });
  out.window = [
    innerWidth,
    innerHeight,
    outerWidth - innerWidth >= 0,
    outerHeight - innerHeight >= 0,
    devicePixelRatio,
  ];
  out.visualViewport = visualViewport
    ? [
        visualViewport.width,
        visualViewport.height,
        visualViewport.scale,
        visualViewport.offsetLeft,
        visualViewport.offsetTop,
        visualViewport.pageLeft,
        visualViewport.pageTop,
      ]
    : null;
  const cases = [
    ["sized", "height:100px", "margin:0;height:50px", "<div style='width:20px;height:20px'></div>"],
    [
      "boxes",
      "height:200px;border:5px solid;padding:7px;margin:2px;width:300px",
      "margin:10px;border:3px solid;padding:4px;height:60px",
      "<div style='width:50px;height:20px'></div>",
    ],
    ["tall", "", "margin:0", "<div style='width:3000px;height:3000px'></div>"],
    [
      "tallBoxes",
      "",
      "margin:10px;border:3px solid;padding:4px",
      "<div style='width:3000px;height:3000px'></div>",
    ],
    [
      "rootHidden",
      "overflow:hidden",
      "margin:10px;border:3px solid;padding:4px",
      "<div style='width:3000px;height:3000px'></div>",
    ],
    [
      "rootScroll",
      "overflow:scroll;height:500px",
      "margin:0",
      "<div style='width:3000px;height:3000px'></div>",
    ],
    [
      "bodyScroll",
      "height:150px",
      "margin:0;overflow:scroll;width:200px;height:100px",
      "<div style='width:3000px;height:3000px'></div>",
    ],
    [
      "bothScroll",
      "overflow:auto;height:150px",
      "margin:0;overflow:scroll;width:200px;height:100px",
      "<div style='width:3000px;height:3000px'></div>",
    ],
    [
      "bodyHidden",
      "height:150px",
      "margin:0;overflow:hidden;height:100px",
      "<div style='width:3000px;height:3000px'></div>",
    ],
    ["rootNone", "display:none", "margin:0", "<div style='width:3000px;height:3000px'></div>"],
    ["bodyNone", "height:120px", "display:none", "<div style='width:3000px;height:3000px'></div>"],
    [
      "absolute",
      "height:100px",
      "margin:0;height:40px",
      "<div style='position:absolute;left:1500px;top:900px;width:10px;height:10px'></div>",
    ],
  ];
  out.cases = {};
  for (const [name, html, bodyStyle, content] of cases) {
    root.style.cssText = html;
    body.style.cssText = bodyStyle;
    body.innerHTML = content;
    out.cases[name] = read();
  }

  // Root/body scroll setters address the scrolling element (the viewport) or
  // their own box depending on the document mode.
  root.style.cssText = "";
  body.style.cssText = "margin:0";
  body.innerHTML = "<div style='width:3000px;height:3000px'></div>";
  const scrolls = [];
  for (const [element, label] of [
    [root, "html"],
    [body, "body"],
  ]) {
    scrollTo(0, 0);
    element.scrollTop = 120;
    element.scrollLeft = 40;
    scrolls.push([label, "set", element.scrollTop, element.scrollLeft, scrollY, scrollX]);
    await element.scrollTo(7, 300);
    scrolls.push([label, "scrollTo", element.scrollTop, element.scrollLeft, scrollY, scrollX]);
    await element.scrollBy(3, 5);
    scrolls.push([label, "scrollBy", element.scrollTop, element.scrollLeft, scrollY, scrollX]);
    scrolls.push([label, "client", element.clientWidth, element.clientHeight, rect(element)]);
  }
  await scrollTo(0, 0);
  out.scrolls = scrolls;

  // A second body (not document.body) is an ordinary element in both modes.
  body.innerHTML = "<div style='width:3000px;height:3000px'></div>";
  const second = document.createElement("body");
  second.style.cssText = "display:block;width:70px;height:30px;margin:0;border:2px solid";
  root.append(second);
  out.secondBody = [document.body === body, metrics(second)];
  second.remove();

  // A frame's root/body use the frame's own viewport.
  body.innerHTML = "";
  const frame = document.createElement("iframe");
  frame.style.cssText = "width:300px;height:150px;border:0";
  const loaded = new Promise((done) => frame.addEventListener("load", done, { once: true }));
  frame.srcdoc =
    "<!doctype html><body style='margin:0'><div style='width:500px;height:700px'></div></body>";
  body.append(frame);
  await loaded;
  const inner = frame.contentDocument;
  out.frame = {
    mode: inner.compatMode,
    html: metrics(inner.documentElement),
    body: metrics(inner.body),
    window: [frame.contentWindow.innerWidth, frame.contentWindow.innerHeight],
  };
  frame.remove();
  // An iframe srcdoc document is never in quirks mode, even without a doctype.
  const bare = document.createElement("iframe");
  bare.style.cssText = "width:300px;height:150px;border:0";
  const bareLoaded = new Promise((done) => bare.addEventListener("load", done, { once: true }));
  bare.srcdoc = "<body style='margin:0'><div style='width:500px;height:700px'></div></body>";
  body.append(bare);
  await bareLoaded;
  const bareDocument = bare.contentDocument;
  bareDocument.documentElement.scrollTop = 40;
  out.srcdoc = {
    mode: bareDocument.compatMode,
    doctype: bareDocument.doctype === null,
    scrollingElement: bareDocument.scrollingElement === bareDocument.documentElement,
    html: metrics(bareDocument.documentElement),
    body: metrics(bareDocument.body),
    scrollY: bare.contentWindow.scrollY,
  };
  bare.remove();
  root.removeAttribute("style");
  body.removeAttribute("style");
  body.innerHTML = "";
  return out;
})();
