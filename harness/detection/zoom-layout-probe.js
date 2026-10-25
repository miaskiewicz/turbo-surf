(async () => {
  // Fixed-pixel boxes only: every observation is independent of installed fonts.
  const out = {};
  const body = document.body;
  const root = document.documentElement;
  const failure = (callback) => {
    try {
      return ["returned", callback()];
    } catch (error) {
      return ["threw", error.constructor.name, error.message];
    }
  };
  const describe = (target, name) => {
    const d = Object.getOwnPropertyDescriptor(target, name);
    if (!d) return null;
    return {
      kind: "value" in d ? "value" : "accessor",
      enumerable: d.enumerable,
      configurable: d.configurable,
      get: d.get ? [d.get.name, d.get.length] : null,
      set: d.set ? [d.set.name, d.set.length] : null,
    };
  };
  const zoomGetter = Object.getOwnPropertyDescriptor(Element.prototype, "currentCSSZoom")?.get;
  out.shape = {
    element: describe(Element.prototype, "currentCSSZoom"),
    html: describe(HTMLElement.prototype, "currentCSSZoom"),
    brands: [{}, document, document.createTextNode("x"), document.createComment("c")].map(
      (receiver) => failure(() => zoomGetter.call(receiver)),
    ),
    svgElement: failure(() =>
      zoomGetter.call(document.createElementNS("http://www.w3.org/2000/svg", "rect")),
    ),
  };

  // Declared value grammar, serialization and computed values.
  const probe = document.createElement("div");
  body.append(probe);
  const computed = (element) => getComputedStyle(element).getPropertyValue("zoom");
  out.values = [];
  for (const value of [
    "2",
    "200%",
    "normal",
    "NORMAL",
    "reset",
    "document",
    "0",
    "-0",
    "0.0",
    "0%",
    "-1",
    "-50%",
    "1.5",
    "0.5",
    ".25",
    "+2",
    "1e1",
    "1E1",
    "1.0",
    "100%",
    "50%",
    "33.33333%",
    "0.3333333",
    "abc",
    "2px",
    "2 3",
    "calc(1 + 1)",
    "calc(50% + 50%)",
    "calc(1 + 10%)",
    "calc(2 * 1.5)",
    "inherit",
    "initial",
    "unset",
    "revert",
  ]) {
    probe.style.zoom = "";
    probe.style.zoom = value;
    const declared = probe.style.zoom;
    probe.removeAttribute("style");
    probe.setAttribute("style", `zoom: ${value}`);
    out.values.push([
      value,
      declared,
      probe.style.getPropertyValue("zoom"),
      computed(probe),
      probe.currentCSSZoom,
      CSS.supports("zoom", value),
      CSS.supports(`zoom: ${value}`),
    ]);
  }
  probe.removeAttribute("style");
  out.declaration = ["zoom" in probe.style, typeof probe.style.zoom, probe.style.cssText];
  probe.style.setProperty("zoom", "3", "important");
  out.important = [probe.style.cssText, probe.style.getPropertyPriority("zoom"), computed(probe)];
  probe.remove();
  const detached = document.createElement("div");
  detached.style.zoom = "4";
  out.detached = [detached.currentCSSZoom, computed(detached), detached.style.zoom];

  // Geometry: layout boxes are zoomed; CSSOM offset/client/scroll metrics are
  // reported in each element's own unzoomed coordinates.
  const rect = (element) => {
    const r = element.getBoundingClientRect();
    return [r.x, r.y, r.width, r.height];
  };
  const metrics = (element) => ({
    zoom: element.currentCSSZoom,
    computed: computed(element),
    rect: rect(element),
    rects: [...element.getClientRects()].map((r) => [r.x, r.y, r.width, r.height]),
    client: [element.clientLeft, element.clientTop, element.clientWidth, element.clientHeight],
    offset: [
      element.offsetLeft,
      element.offsetTop,
      element.offsetWidth,
      element.offsetHeight,
      element.offsetParent?.id ?? null,
    ],
    scroll: [element.scrollLeft, element.scrollTop, element.scrollWidth, element.scrollHeight],
  });
  body.style.cssText = "margin:0";
  body.innerHTML = `
    <div id=outer style="zoom:2;width:100px;height:50px;padding:5px;border:2px solid;margin:10px;position:relative">
      <div id=nested style="width:20px;height:10px;margin-left:7px;margin-top:3px;zoom:1.5;border:1px solid;padding:1px"></div>
      <div id=absolute style="position:absolute;left:11px;top:13px;width:4px;height:6px"></div>
      <div id=reset style="zoom:0.5;width:40px;height:4px"></div>
    </div>
    <div id=scroller style="zoom:2;width:50px;height:40px;overflow:auto;border:3px solid;padding:2px">
      <div id=content style="width:300px;height:200px"></div>
    </div>
    <div id=fraction style="zoom:3;width:11px;height:7px;margin-left:1px"></div>
    <div id=none style="zoom:3;display:none"><span id=insideNone></span></div>
    <div id=contents style="zoom:3;display:contents"><span id=throughContents style="display:inline-block;width:10px;height:10px"></span></div>
    <div id=line style="zoom:2;line-height:10px;width:200px;font-size:0"><div id=inherited style="zoom:3"><span style="display:inline-block;width:1px;height:0"></span></div></div>
    <div id=units style="zoom:2;font-size:10px;width:400px;height:30px"><div id=em style="font-size:2em;width:3em;height:1rem;zoom:3"></div><div id=viewport style="width:10vw;height:50%"></div></div>
    <div id=transform style="zoom:2;width:10px;height:10px;transform:translate(10px,5px)"></div>
    <div id=keywords style="zoom:2;width:10px;height:10px;border:medium solid;border-left-width:thin;border-right-width:thick"></div>
    <div id=percentage style="zoom:2;width:300px"><div id=half style="width:50%;height:5px;padding-left:10%"></div></div>
    <div style="zoom:2;font-size:0"><span id=inline style="zoom:1.5"><span id=inlineBox style="display:inline-block;width:8px;height:6px"></span></span></div>
    <svg id=svg style="zoom:2;display:block" width=10 height=10><rect id=svgRect width=5 height=5 /></svg>`;
  const $ = (id) => document.getElementById(id);
  out.geometry = Object.fromEntries(
    [
      "outer",
      "nested",
      "absolute",
      "reset",
      "scroller",
      "content",
      "fraction",
      "none",
      "insideNone",
      "contents",
      "throughContents",
      "line",
      "inherited",
      "units",
      "em",
      "viewport",
      "transform",
      "keywords",
      "percentage",
      "half",
      "inlineBox",
    ].map((id) => [id, metrics($(id))]),
  );
  out.inline = [$("inline").currentCSSZoom, computed($("inline"))];
  out.svg = [$("svg").currentCSSZoom, $("svgRect").currentCSSZoom, rect($("svg"))];
  out.hits = [
    [15, 15],
    [25, 25],
    [247, 147],
    [250, 150],
    [60, 50],
  ].map(([x, y]) => document.elementFromPoint(x, y)?.id ?? null);

  // Scroll offsets convert through the scroller's own effective zoom.
  const scroller = $("scroller");
  const scrolls = [];
  const position = () => [scroller.scrollLeft, scroller.scrollTop];
  scroller.scrollTop = 20;
  scroller.scrollLeft = 30;
  scrolls.push(position());
  scroller.scrollTop = 20.25;
  scroller.scrollLeft = 7.3;
  scrolls.push(position());
  scroller.scrollTop = 1000;
  scroller.scrollLeft = 1000;
  scrolls.push(position());
  await scroller.scrollTo(5, 6);
  scrolls.push(position());
  await scroller.scrollBy(3, 4);
  scrolls.push(position());
  scroller.scrollTop = 0;
  scroller.scrollLeft = 0;
  scrolls.push(position(), rect($("content")));
  scroller.scrollTop = 25;
  scrolls.push(rect($("content")));
  out.scrolls = scrolls;

  // Flat-tree products through shadow trees and slots.
  body.innerHTML = "";
  const host = document.createElement("div");
  host.style.zoom = "2";
  body.append(host);
  const shadow = host.attachShadow({ mode: "open" });
  shadow.innerHTML =
    "<div style='zoom:3'><slot></slot></div><div id=internal style='zoom:5;width:4px;height:4px'></div><slot name=hidden style='display:none'></slot>";
  const slotted = document.createElement("span");
  slotted.style.cssText = "zoom:7;display:block;width:2px;height:2px";
  const unslotted = document.createElement("span");
  unslotted.slot = "missing";
  const hidden = document.createElement("span");
  hidden.slot = "hidden";
  host.append(slotted, unslotted, hidden);
  out.flat = {
    host: host.currentCSSZoom,
    slotted: [slotted.currentCSSZoom, rect(slotted)],
    internal: [
      shadow.getElementById("internal").currentCSSZoom,
      rect(shadow.getElementById("internal")),
    ],
    unslotted: unslotted.currentCSSZoom,
    hidden: hidden.currentCSSZoom,
  };

  // Root and body zoom: viewport client sizes and window metrics stay unzoomed.
  body.innerHTML = "<div id=box style='width:100px;height:100px'></div>";
  const rootRead = () => ({
    root: metrics(root),
    body: metrics(body),
    box: metrics($("box")),
    window: [innerWidth, innerHeight, scrollX, scrollY],
  });
  root.style.zoom = "2";
  out.rootZoom = rootRead();
  root.style.zoom = "";
  body.style.zoom = "2";
  out.bodyZoom = rootRead();
  body.style.zoom = "";
  // Changing zoom relayouts on the next read.
  const toggled = $("box");
  toggled.style.zoom = "3";
  const first = rect(toggled);
  toggled.style.zoom = "0.5";
  out.toggle = [first, rect(toggled), toggled.offsetWidth, toggled.currentCSSZoom];
  body.removeAttribute("style");
  body.innerHTML = "";
  return out;
})();
