(async () => {
  "use strict";
  // History and Location: interface shapes, [LegacyUnforgeable] own members, same-document
  // navigation state (pushState/replaceState/fragments/traversal) and popstate/hashchange order.
  const run = (f) => {
    try {
      return ["ok", f()];
    } catch (error) {
      return ["throw", error.name];
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
  const shape = (o) => {
    const keys = Reflect.ownKeys(o);
    return Object.fromEntries(
      keys
        .map((k) => [String(k), k])
        .sort((a, b) => (a[0] < b[0] ? -1 : 1))
        .map(([s, k]) => [s, desc(o, k)]),
    );
  };
  const tick = (ms = 30) => new Promise((done) => setTimeout(done, ms));
  // Traversals are asynchronous (Chrome routes them through the browser process): wait for the
  // popstate they fire, or a bounded quiet period for one that must not happen.
  const popstate = (ms = 5000) =>
    new Promise((done) => {
      const handler = () => {
        clearTimeout(timer);
        removeEventListener("popstate", handler);
        done(true);
      };
      const timer = setTimeout(() => {
        removeEventListener("popstate", handler);
        done(false);
      }, ms);
      addEventListener("popstate", handler);
    });
  const traverse = async (step, quiet) => {
    const moved = popstate(quiet ? 400 : 5000);
    step();
    const fired = await moved;
    await tick(100);
    return fired;
  };
  const out = {};
  const origin = location.origin;
  const rel = (href) => (href.startsWith(origin) ? href.slice(origin.length) : href);
  out.interfaces = {};
  for (const name of ["History", "Location", "PopStateEvent", "HashChangeEvent"]) {
    const C = globalThis[name];
    out.interfaces[name] = {
      ctor: shape(C),
      proto: shape(C.prototype),
      global: desc(globalThis, name),
      construct: run(() => new C("x"))[1],
      call: run(() => C("x"))[1],
      tag: Object.prototype.toString.call(C.prototype),
      parent: Object.getPrototypeOf(C) === Function.prototype ? "Function" : Object.getPrototypeOf(C).name,
    };
  }
  out.window = {
    history: desc(window, "history"),
    location: desc(window, "location"),
    documentLocation: desc(document, "location"),
    documentProtoLocation: desc(Document.prototype, "location"),
    windowProtoLocation: desc(Window.prototype, "location"),
    same: [document.location === location, window.location === location, history === window.history],
    tags: [Object.prototype.toString.call(history), Object.prototype.toString.call(location)],
    instance: [history instanceof History, location instanceof Location, Object.getPrototypeOf(location) === Location.prototype],
  };
  out.locationOwn = shape(location);
  out.historyOwn = Reflect.ownKeys(history).map(String);
  const L = Object.getOwnPropertyDescriptors(location);
  out.locationValues = {
    parts: ["href", "origin", "protocol", "host", "hostname", "port", "pathname", "search", "hash"].map((k) =>
      k === "href" || k === "origin" ? location[k].startsWith(origin) : k === "host" || k === "port" ? location[k] === new URL(location.href)[k] : location[k],
    ),
    toString: [String(location) === location.href, location.toString() === location.href, location.valueOf() === location],
    ancestorOrigins: [Object.prototype.toString.call(location.ancestorOrigins), location.ancestorOrigins.length, location.ancestorOrigins === location.ancestorOrigins, typeof location.ancestorOrigins.contains],
    brand: [
      run(() => L.href.get.call({}))[1],
      run(() => L.href.get.call(history))[1],
      run(() => L.assign.value.call({}, "#x"))[1],
      run(() => L.toString.value.call({}))[1],
      run(() => L.valueOf.value.call({}) )[0],
    ],
    redefine: [
      run(() => Object.defineProperty(location, "href", { value: 1 }))[1],
      run(() => { delete location.href; })[1],
      run(() => Object.defineProperty(location, "fresh", { value: 1 }) === location)[1],
      run(() => Object.setPrototypeOf(location, null))[1],
      run(() => Object.preventExtensions(location) === location)[1],
      run(() => { location.expando = 1; return location.expando; })[1],
    ],
    arity: [run(() => location.assign())[1], run(() => location.replace())[1]],
    badUrl: [run(() => location.assign("http://[bad"))[1], run(() => { location.href = "http://[bad"; })[1], run(() => { location.hash = ""; return rel(location.href); })[1]],
  };
  const H = History.prototype;
  out.historyValues = {
    // Absolute length depends on the entries before this document (Chrome's harness tab starts
    // at about:blank); everything below is relative to it.
    initial: [history.length >= 1, history.state, history.scrollRestoration],
    scroll: (() => {
      const steps = [];
      for (const value of ["manual", "bogus", "auto", "MANUAL"]) {
        const r = run(() => {
          history.scrollRestoration = value;
        });
        steps.push([r[0], r[1] ?? null, history.scrollRestoration]);
      }
      return steps;
    })(),
    brand: [
      run(() => Object.getOwnPropertyDescriptor(H, "length").get.call(location))[1],
      run(() => Object.getOwnPropertyDescriptor(H, "state").get.call({}))[1],
      run(() => H.pushState.call({}, null, ""))[1],
      run(() => H.back.call(null))[1],
    ],
    arity: [run(() => history.pushState())[1], run(() => history.pushState({}))[1], run(() => history.replaceState(1))[1]],
    errors: [
      run(() => history.pushState(null, "", "https://example.invalid/x"))[1],
      run(() => history.pushState(() => 1, ""))[1],
      run(() => history.replaceState(Symbol("s"), ""))[1],
      run(() => history.pushState(null, "", "http://[bad"))[1],
    ],
  };
  // Event log for same-document navigation and traversal.
  const log = [];
  const record = (event) =>
    log.push([
      event.type,
      event.isTrusted,
      Object.prototype.toString.call(event),
      event.type === "popstate" ? JSON.stringify(event.state) : [rel(event.oldURL), rel(event.newURL)],
      rel(location.href),
      JSON.stringify(history.state),
      event.bubbles,
      event.cancelable,
      event.target === window,
    ]);
  addEventListener("popstate", record);
  addEventListener("hashchange", record);
  const start = history.length;
  const mark = (label) => log.push([label, rel(location.href), history.length - start, JSON.stringify(history.state)]);
  const stateObject = { n: 1, nested: { list: [1, 2] } };
  history.pushState(stateObject, "", "/pushed?a=1#f");
  out.pushed = [rel(location.href), history.length - start, JSON.stringify(history.state), history.state === stateObject, history.state === history.state, location.pathname, location.search, location.hash];
  mark("after-push");
  history.replaceState({ r: 2 }, "ignored-title", "replaced");
  out.replaced = [rel(location.href), history.length - start, JSON.stringify(history.state)];
  history.pushState(null, "");
  out.pushedNoUrl = [rel(location.href), history.length - start, history.state];
  await tick();
  mark("pushes-settled");
  location.hash = "frag2";
  mark("hash-set-sync");
  await tick();
  mark("hash-set-async");
  location.hash = "frag2";
  await tick();
  mark("same-hash");
  location.href = "#frag3";
  await tick();
  mark("href-fragment");
  location.assign("#frag4");
  await tick();
  mark("assign-fragment");
  location.replace("#frag5");
  await tick();
  mark("replace-fragment");
  out.lengthBeforeTraverse = history.length - start;
  out.traversals = [];
  out.traversals.push(await traverse(() => {
    history.back();
    mark("back-sync");
  }));
  mark("back-async");
  out.traversals.push(await traverse(() => history.go(-2)));
  mark("go-minus-2");
  out.traversals.push(await traverse(() => history.forward()));
  mark("forward");
  out.traversals.push(await traverse(() => history.go(1)));
  mark("go-plus-1");
  out.traversals.push(await traverse(() => history.go(100), true));
  mark("go-out-of-range");
  history.pushState({ tail: true }, "", "/tail");
  out.truncated = [history.length - start, rel(location.href)];
  out.traversals.push(await traverse(() => history.back()));
  out.traversals.push(await traverse(() => history.back()));
  mark("double-back");
  out.traversals.push(await traverse(() => history.forward()));
  mark("forward-after-double-back");
  removeEventListener("popstate", record);
  removeEventListener("hashchange", record);
  // Handler attributes fire with the listeners.
  const handlers = [];
  onpopstate = (event) => handlers.push(["onpopstate", JSON.stringify(event.state)]);
  onhashchange = (event) => handlers.push(["onhashchange", rel(event.newURL)]);
  history.pushState({ h: 1 }, "", "#handlers");
  location.hash = "handlers2";
  await tick();
  await traverse(() => history.back());
  onpopstate = null;
  onhashchange = null;
  out.handlers = handlers;
  out.log = log;
  out.returns = [history.back(), history.forward(), history.go(100)];
  await tick(400);
  return out;
})()
