(async () => {
  "use strict";
  // VisualViewport, Screen and ScreenOrientation: interface shapes, values tied to the layout
  // viewport and document scroll, event handler attributes and lock()/unlock() outside fullscreen.
  const run = (f) => {
    try {
      return ["ok", f()];
    } catch (error) {
      return ["throw", error.name];
    }
  };
  const settle = async (p) => {
    try {
      return ["ok", await p];
    } catch (error) {
      return ["reject", error.name, error.message];
    }
  };
  const fn = (f) =>
    typeof f === "function"
      ? {
          name: f.name,
          length: f.length,
          proto: Object.hasOwn(f, "prototype"),
          native: Function.prototype.toString.call(f) === `function ${f.name}() { [native code] }`,
        }
      : typeof f;
  const desc = (o, k) => {
    const d = Object.getOwnPropertyDescriptor(o, k);
    if (!d) return null;
    const r = { c: d.configurable, e: d.enumerable };
    if ("value" in d) {
      r.w = d.writable;
      r.v = typeof d.value === "function" ? fn(d.value) : typeof d.value;
    } else {
      r.get = fn(d.get);
      r.set = fn(d.set);
    }
    return r;
  };
  const shape = (o) =>
    Object.fromEntries(
      Reflect.ownKeys(o)
        .map((k) => [String(k), k])
        .sort((a, b) => (a[0] < b[0] ? -1 : 1))
        .map(([s, k]) => [s, desc(o, k)]),
    );
  const frame = () => new Promise((done) => requestAnimationFrame(() => setTimeout(done, 20)));
  const out = { interfaces: {} };
  for (const name of ["VisualViewport", "Screen", "ScreenOrientation"]) {
    const C = globalThis[name];
    out.interfaces[name] = {
      ctor: shape(C),
      proto: shape(C.prototype),
      global: desc(globalThis, name),
      construct: run(() => new C())[1],
      tag: Object.prototype.toString.call(C.prototype),
      parent: Object.getPrototypeOf(C) === Function.prototype ? "Function" : Object.getPrototypeOf(C).name,
      protoParent: Object.prototype.toString.call(Object.getPrototypeOf(C.prototype)),
    };
  }
  out.window = {
    visualViewport: desc(window, "visualViewport"),
    screen: desc(window, "screen"),
    identity: [visualViewport === window.visualViewport, screen.orientation === screen.orientation, visualViewport instanceof EventTarget, screen instanceof EventTarget, screen.orientation instanceof EventTarget],
    own: [Reflect.ownKeys(visualViewport).map(String), Reflect.ownKeys(screen).map(String), Reflect.ownKeys(screen.orientation).map(String)],
    tags: [visualViewport, screen, screen.orientation].map((o) => Object.prototype.toString.call(o)),
  };
  const vv = () => [visualViewport.offsetLeft, visualViewport.offsetTop, visualViewport.pageLeft, visualViewport.pageTop, visualViewport.width, visualViewport.height, visualViewport.scale];
  out.viewport = { initial: vv(), inner: [innerWidth, innerHeight] };
  out.screen = {
    values: [screen.width, screen.height, screen.availWidth, screen.availHeight, screen.availLeft, screen.availTop, screen.colorDepth, screen.pixelDepth, screen.isExtended, screen.onchange],
    orientation: [screen.orientation.type, screen.orientation.angle, screen.orientation.onchange],
  };
  const P = VisualViewport.prototype, S = ScreenOrientation.prototype, SC = Screen.prototype;
  out.brand = [
    run(() => Object.getOwnPropertyDescriptor(P, "width").get.call(screen))[1],
    run(() => Object.getOwnPropertyDescriptor(P, "onresize").set.call({}, null))[1],
    run(() => Object.getOwnPropertyDescriptor(S, "type").get.call(visualViewport))[1],
    run(() => Object.getOwnPropertyDescriptor(SC, "availLeft").get.call({}))[1],
    run(() => Object.getOwnPropertyDescriptor(SC, "orientation").get.call(visualViewport))[1],
    await settle(S.lock.call({}, "portrait")),
    run(() => S.unlock.call(screen))[1],
  ];
  // Event handler attributes.
  const handlers = [];
  const handler = (name) => function (event) { handlers.push([name, event.type, this === visualViewport]); };
  visualViewport.onresize = handler("onresize");
  visualViewport.onscroll = handler("onscroll");
  visualViewport.onscrollend = handler("onscrollend");
  const coerced = [];
  for (const value of [1, "x", {}, null]) {
    screen.orientation.onchange = value;
    coerced.push(typeof screen.orientation.onchange);
  }
  out.handlers = { types: [typeof visualViewport.onresize, typeof visualViewport.onscroll, typeof visualViewport.onscrollend], coerced };
  // Document scroll moves the layout viewport: pageLeft/Top follow, offsets stay 0, and the
  // visual viewport fires no scroll event of its own.
  const events = [];
  for (const type of ["scroll", "resize", "scrollend"]) visualViewport.addEventListener(type, (event) => events.push(["vv", type, event.isTrusted, event.bubbles]));
  addEventListener("scroll", () => events.push(["window", "scroll"]));
  document.body.style.height = "5000px";
  document.body.style.width = "5000px";
  await frame();
  scrollTo(120, 340);
  out.viewport.afterScroll = [vv(), scrollX, scrollY];
  await frame();
  await frame();
  scrollTo(0, 0);
  await frame();
  await frame();
  out.viewport.reset = vv();
  out.events = events;
  out.handlers.fired = handlers;
  visualViewport.onresize = null;
  // ScreenOrientation.lock outside fullscreen and with invalid input; unlock is synchronous.
  out.orientation = {
    lock: await settle(screen.orientation.lock("portrait")),
    lockLandscape: await settle(screen.orientation.lock("landscape-primary")),
    lockBad: await settle(screen.orientation.lock("sideways")),
    lockNone: await settle(screen.orientation.lock()),
    unlock: run(() => screen.orientation.unlock()),
    lockLength: S.lock.length,
  };
  return out;
})()
