(async () => {
  "use strict";
  // IntersectionObserver, ResizeObserver and PerformanceObserver: interface shapes, argument
  // handling and delivery over fixed-pixel boxes (no text, so layout is engine-independent).
  const run = (f) => {
    try {
      return ["ok", f()];
    } catch (error) {
      return ["throw", error.name];
    }
  };
  const fn = (f) => (typeof f === "function" ? { name: f.name, length: f.length, native: Function.prototype.toString.call(f) === `function ${f.name}() { [native code] }` } : typeof f);
  const desc = (o, k) => {
    const d = Object.getOwnPropertyDescriptor(o, k);
    if (!d) return null;
    const r = { c: d.configurable, e: d.enumerable };
    if ("value" in d) {
      r.w = d.writable;
      r.v = typeof d.value === "function" ? fn(d.value) : d.value !== null && typeof d.value === "object" ? "object" : d.value;
    } else {
      r.get = fn(d.get);
      r.set = fn(d.set);
    }
    return r;
  };
  const shape = (o) => Object.fromEntries(Reflect.ownKeys(o).map((k) => [String(k), k]).sort((a, b) => (a[0] < b[0] ? -1 : 1)).map(([s, k]) => [s, desc(o, k)]));
  const frame = () => new Promise((done) => requestAnimationFrame(() => setTimeout(done, 0)));
  const settle = async () => {
    for (let i = 0; i < 4; i++) await frame();
    await new Promise((done) => setTimeout(done, 50));
  };
  const out = { interfaces: {} };
  for (const name of ["IntersectionObserver", "IntersectionObserverEntry", "ResizeObserver", "ResizeObserverEntry", "ResizeObserverSize", "PerformanceObserver", "PerformanceObserverEntryList"]) {
    const C = globalThis[name];
    out.interfaces[name] = typeof C === "function" ? { ctor: shape(C), proto: shape(C.prototype), parent: Object.getPrototypeOf(C) === Function.prototype, global: desc(globalThis, name) } : typeof C;
  }
  const box = (style) => {
    const element = document.createElement("div");
    element.style.cssText = "position:absolute;left:0;top:0;width:100px;height:100px;margin:0;" + style;
    document.body.append(element);
    return element;
  };
  const rect = (r) => r && [r.x, r.y, r.width, r.height];
  // ---- IntersectionObserver ----
  const noop = () => {};
  const io = {};
  io.construct = [
    run(() => new IntersectionObserver())[1],
    run(() => new IntersectionObserver(1))[1],
    run(() => IntersectionObserver(noop))[1],
    run(() => new IntersectionObserver(noop, { rootMargin: "1em" }))[1],
    run(() => new IntersectionObserver(noop, { rootMargin: "1px 2px 3px 4px 5px" }))[1],
    run(() => new IntersectionObserver(noop, { threshold: 1.5 }))[1],
    run(() => new IntersectionObserver(noop, { threshold: [-0.1] }))[1],
    run(() => new IntersectionObserver(noop, { threshold: NaN }))[1],
    run(() => new IntersectionObserver(noop, { root: {} }))[1],
    run(() => new IntersectionObserver(noop, { trackVisibility: true }))[1],
  ];
  const attrs = (o) => [o.root === null ? null : o.root === document ? "document" : o.root.localName, o.rootMargin, o.scrollMargin, o.thresholds, Object.isFrozen(o.thresholds), o.thresholds === o.thresholds, o.delay, o.trackVisibility];
  io.attributes = [
    attrs(new IntersectionObserver(noop)),
    attrs(new IntersectionObserver(noop, { rootMargin: "10px", threshold: [1, 0, 0.5, 0.5] })),
    attrs(new IntersectionObserver(noop, { rootMargin: "5% 10px", scrollMargin: "2px 3px 4px", root: document, threshold: 0.25 })),
    attrs(new IntersectionObserver(noop, { root: document.body, delay: 120, trackVisibility: true })),
  ];
  const inView = box("");
  const below = box("top:3000px");
  const partial = box("top:750px");
  const hidden = box("display:none");
  const zero = box("width:0;height:0");
  const records = [];
  const observer = new IntersectionObserver(function (entries, self) {
    records.push(["batch", this === observer, self === observer, entries.length]);
    for (const e of entries)
      records.push([
        [inView, below, partial, hidden, zero].indexOf(e.target),
        e.isIntersecting,
        Math.round(e.intersectionRatio * 1000) / 1000,
        rect(e.boundingClientRect),
        rect(e.intersectionRect),
        rect(e.rootBounds),
        e.isVisible,
        typeof e.time === "number" && e.time > 0,
        Object.prototype.toString.call(e),
        Object.prototype.toString.call(e.boundingClientRect),
      ]);
  }, { threshold: [0, 0.5, 1] });
  io.observeErrors = [run(() => observer.observe({}))[1], run(() => observer.observe())[1], run(() => observer.unobserve({}))[1]];
  for (const element of [inView, below, partial, hidden, zero]) observer.observe(element);
  observer.observe(inView);
  io.syncRecords = records.length;
  await settle();
  io.initial = records.splice(0);
  scrollTo(0, 2950);
  await settle();
  io.afterScroll = records.splice(0);
  scrollTo(0, 0);
  await settle();
  io.afterReturn = records.splice(0);
  partial.style.top = "0px";
  await settle();
  io.afterMove = records.splice(0);
  observer.unobserve(partial);
  partial.style.top = "3000px";
  await settle();
  io.afterUnobserve = records.splice(0);
  const pending = new IntersectionObserver(() => records.push("pending-callback"));
  pending.observe(inView);
  // Nothing is computed before the next rendering update.
  io.takeRecords = pending.takeRecords().length;
  await settle();
  io.afterTake = [records.splice(0), pending.takeRecords().length];
  pending.disconnect();
  const quiet = new IntersectionObserver(() => records.push("disconnected-callback"));
  quiet.observe(inView);
  quiet.disconnect();
  await settle();
  io.afterDisconnect = records.splice(0);
  // An explicit root clips to its padding box; a non-descendant never intersects it.
  const scroller = box("left:200px;top:0;width:100px;height:100px;overflow:hidden;padding:10px;border:5px solid");
  const inner = document.createElement("div");
  inner.style.cssText = "width:50px;height:50px;margin-top:90px";
  scroller.append(inner);
  const rooted = [];
  const rootObserver = new IntersectionObserver((entries) => {
    for (const e of entries) rooted.push([e.target === inner, e.isIntersecting, Math.round(e.intersectionRatio * 1000) / 1000, rect(e.intersectionRect), rect(e.rootBounds)]);
  }, { root: scroller, rootMargin: "0px" });
  rootObserver.observe(inner);
  rootObserver.observe(inView);
  await settle();
  io.rooted = rooted;
  observer.disconnect();
  rootObserver.disconnect();
  out.intersection = io;
  // ---- ResizeObserver ----
  const ro = {};
  ro.construct = [run(() => new ResizeObserver())[1], run(() => new ResizeObserver({}))[1], run(() => ResizeObserver(noop))[1]];
  const sized = box("left:400px;width:80px;height:40px;padding:5px 7px;border:2px solid");
  const none = box("display:none");
  const resizes = [];
  const order = [];
  const sizes = (list) => list.map((s) => [s.inlineSize, s.blockSize, Object.prototype.toString.call(s)]);
  const resizeObserver = new ResizeObserver(function (entries, self) {
    order.push("resize");
    resizes.push(["batch", this === resizeObserver, self === resizeObserver, entries.length]);
    for (const e of entries)
      resizes.push([
        [sized, none].indexOf(e.target),
        rect(e.contentRect),
        sizes(e.borderBoxSize),
        sizes(e.contentBoxSize),
        sizes(e.devicePixelContentBoxSize),
        Object.isFrozen(e.borderBoxSize),
        Object.prototype.toString.call(e),
        Object.prototype.toString.call(e.contentRect),
      ]);
  });
  ro.observeErrors = [run(() => resizeObserver.observe({}))[1], run(() => resizeObserver.observe(sized, { box: "padding-box" }))[1], run(() => resizeObserver.observe())[1]];
  resizeObserver.observe(sized);
  resizeObserver.observe(none, { box: "border-box" });
  requestAnimationFrame(() => order.push("raf"));
  await settle();
  ro.initial = resizes.splice(0);
  ro.devicePixelRatio = devicePixelRatio;
  ro.order = order.splice(0);
  sized.style.width = "120px";
  await settle();
  ro.afterResize = resizes.splice(0);
  await settle();
  ro.unchanged = resizes.splice(0);
  resizeObserver.unobserve(sized);
  sized.style.width = "60px";
  await settle();
  ro.afterUnobserve = resizes.splice(0);
  resizeObserver.observe(sized, { box: "border-box" });
  await settle();
  ro.reobserve = resizes.splice(0);
  resizeObserver.disconnect();
  sized.style.width = "30px";
  await settle();
  ro.afterDisconnect = resizes.splice(0);
  out.resize = ro;
  // ---- PerformanceObserver ----
  const po = {};
  po.supported = [PerformanceObserver.supportedEntryTypes, PerformanceObserver.supportedEntryTypes === PerformanceObserver.supportedEntryTypes, Object.isFrozen(PerformanceObserver.supportedEntryTypes)];
  po.construct = [run(() => new PerformanceObserver())[1], run(() => new PerformanceObserver(1))[1], run(() => PerformanceObserver(noop))[1]];
  const delivered = [];
  const pobs = new PerformanceObserver(function (list, self, options) {
    delivered.push([this === pobs, self === pobs, Object.prototype.toString.call(list), list.getEntries().map((e) => [e.entryType, e.name]), list.getEntriesByType("mark").length, list.getEntriesByName("m1").length, list.getEntriesByName("m1", "measure").length, options && Object.keys(options), options && options.droppedEntriesCount]);
  });
  const probe = () => new PerformanceObserver(noop);
  po.observeErrors = [
    run(() => probe().observe())[1],
    run(() => probe().observe({}))[1],
    run(() => probe().observe({ entryTypes: ["mark"], type: "mark" }))[1],
    run(() => probe().observe({ entryTypes: ["mark"], buffered: true }))[1],
    run(() => probe().observe({ type: "bogus" }))[1],
    run(() => probe().observe({ entryTypes: [] }))[1],
  ];
  // Explicit start times: the entry-list order (startTime, then buffer order) must not
  // depend on whether two marks land in the same clock tick (turbo-surf#88).
  performance.mark("before", { startTime: 10 });
  pobs.observe({ type: "mark", buffered: true });
  po.modeSwitch = run(() => pobs.observe({ entryTypes: ["measure"] }))[1];
  pobs.observe({ type: "measure" });
  performance.mark("m1", { startTime: 20 });
  performance.measure("m1", "before", "m1");
  po.sync = delivered.length;
  await settle();
  po.delivered = delivered.splice(0);
  performance.mark("m2");
  po.takeRecords = [pobs.takeRecords().map((e) => [e.entryType, e.name]), pobs.takeRecords().length];
  await settle();
  po.afterTake = delivered.splice(0);
  pobs.disconnect();
  performance.mark("m3");
  await settle();
  po.afterDisconnect = delivered.splice(0);
  const list = new PerformanceObserver(noop);
  list.observe({ entryTypes: ["mark", "bogus", "measure"] });
  po.entryTypesMode = run(() => list.observe({ type: "mark" }))[1];
  list.disconnect();
  out.performance = po;
  return out;
})()
