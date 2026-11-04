(async () => {
  // navigator.locks, cookieStore and caches perform their real operations
  // (turbo-surf#108): lock grants and queueing, cookie round-trips through
  // document.cookie, and Cache put/match/keys/delete round-trips.
  const out = {};
  const settle = async (name, callback) => {
    try {
      out[name] = ["ok", await callback()];
    } catch (error) {
      out[name] = [error.name, error.message];
    }
  };
  const path = (url) => new URL(url).pathname + new URL(url).search;

  // Web Locks.
  await settle("lockShape", () => [
    Object.prototype.toString.call(navigator.locks),
    typeof LockManager,
    typeof Lock,
    navigator.locks instanceof LockManager,
  ]);
  await settle("lockGrant", () =>
    navigator.locks.request("a", (lock) => [
      Object.prototype.toString.call(lock),
      lock.name,
      lock.mode,
      lock instanceof Lock,
    ]),
  );
  await settle("lockOrder", async () => {
    const log = [];
    let release;
    const first = navigator.locks.request("b", () => {
      log.push("first");
      return new Promise((resolve) => {
        release = resolve;
      });
    });
    const second = navigator.locks.request("b", () => log.push("second"));
    const shared = navigator.locks.request("c", { mode: "shared" }, () => log.push("shared"));
    await new Promise((resolve) => setTimeout(resolve, 10));
    const query = await navigator.locks.query();
    const snapshot = {
      held: query.held.map((lock) => [lock.name, lock.mode, typeof lock.clientId]),
      pending: query.pending.map((lock) => [lock.name, lock.mode]),
    };
    log.push("query");
    release("done");
    log.push(await first, await second, await shared);
    return [log, snapshot];
  });
  await settle("lockIfAvailable", async () => {
    let release;
    const held = navigator.locks.request("d", () => new Promise((resolve) => (release = resolve)));
    await new Promise((resolve) => setTimeout(resolve, 0));
    const result = await navigator.locks.request("d", { ifAvailable: true }, (lock) => lock);
    release();
    await held;
    return result;
  });
  await settle("lockSteal", async () => {
    const log = [];
    const held = navigator.locks
      .request("e", () => new Promise(() => {}))
      .catch((error) => log.push([error.name, error.message]));
    await new Promise((resolve) => setTimeout(resolve, 0));
    log.push(await navigator.locks.request("e", { steal: true }, (lock) => lock.name));
    await held;
    return log;
  });
  await settle("lockAbort", async () => {
    const controller = new AbortController();
    controller.abort();
    return navigator.locks.request("f", { signal: controller.signal }, () => "ran");
  });
  await settle("lockBadName", () => navigator.locks.request("-x", () => "ran"));
  await settle("lockBadOptions", () =>
    navigator.locks.request("g", { mode: "shared", steal: true }, () => "ran"),
  );
  await settle("lockNoCallback", () => navigator.locks.request("h"));
  await settle("lockThrows", () =>
    navigator.locks.request("i", () => {
      throw new RangeError("boom");
    }),
  );

  // Cookie Store.
  await settle("cookieShape", () => [
    Object.prototype.toString.call(cookieStore),
    typeof CookieStore,
    cookieStore instanceof CookieStore,
    cookieStore instanceof EventTarget,
  ]);
  await settle("cookieSet", async () => {
    await cookieStore.set("alpha", "1");
    await cookieStore.set({ name: "beta", value: "2" });
    return [document.cookie, await cookieStore.get("alpha")];
  });
  await settle("cookieFromDocument", async () => {
    document.cookie = "gamma=3";
    const all = await cookieStore.getAll();
    return [
      await cookieStore.get("gamma"),
      await cookieStore.get({ name: "gamma" }),
      all.map((cookie) => cookie.name),
      (await cookieStore.getAll("beta")).length,
      await cookieStore.get("missing"),
    ];
  });
  await settle("cookieDelete", async () => {
    await cookieStore.delete("alpha");
    await cookieStore.delete({ name: "beta" });
    return [document.cookie, await cookieStore.get("alpha")];
  });
  await settle("cookieChange", async () => {
    const events = [];
    const listener = (event) =>
      events.push([
        event.type,
        Object.prototype.toString.call(event),
        event.changed.map((cookie) => [cookie.name, cookie.value]),
        event.deleted.map((cookie) => [cookie.name, cookie.value]),
      ]);
    cookieStore.addEventListener("change", listener);
    await cookieStore.set("delta", "4");
    await new Promise((resolve) => setTimeout(resolve, 20));
    await cookieStore.delete("delta");
    await new Promise((resolve) => setTimeout(resolve, 20));
    cookieStore.removeEventListener("change", listener);
    return events;
  });
  await settle("cookieNoArgs", () => cookieStore.set());
  await settle("cookieEmpty", () => cookieStore.set("", ""));
  await settle("cookieBadName", () => cookieStore.set("a=b", "1"));
  await settle("cookieBadValue", () => cookieStore.set("epsilon", "x;y"));
  document.cookie = "gamma=; max-age=0";

  // Cache Storage.
  await settle("cacheShape", () => [
    Object.prototype.toString.call(caches),
    typeof CacheStorage,
    caches instanceof CacheStorage,
    typeof Cache,
  ]);
  await settle("cacheRoundTrip", async () => {
    const cache = await caches.open("v1");
    await cache.put("/a?x=1", new Response("alpha", { headers: { "X-Test": "yes" } }));
    await cache.put(new Request("/b"), new Response("beta", { status: 201 }));
    const hit = await cache.match("/a?x=1");
    const miss = await cache.match("/a");
    const loose = await cache.match("/a", { ignoreSearch: true });
    const keys = await cache.keys();
    return [
      cache instanceof Cache,
      hit instanceof Response,
      await hit.text(),
      hit.headers.get("x-test"),
      miss,
      loose && (await loose.text()),
      keys.map((request) => [request instanceof Request, path(request.url), request.method]),
      (await cache.matchAll()).map((response) => response.status),
    ];
  });
  await settle("cacheStorage", async () => {
    await caches.open("v2");
    const names = await caches.keys();
    const global = await caches.match("/b");
    const scoped = await caches.match("/b", { cacheName: "v2" });
    const has = [await caches.has("v1"), await caches.has("nope")];
    const deleted = [await caches.delete("v2"), await caches.delete("v2")];
    return [names, global && (await global.text()), scoped, has, deleted, await caches.keys()];
  });
  await settle("cacheDelete", async () => {
    const cache = await caches.open("v1");
    return [
      await cache.delete("/a?x=1"),
      await cache.delete("/a?x=1"),
      (await cache.keys()).map((request) => path(request.url)),
    ];
  });
  await settle("cacheAddPersist", async () => {
    const first = await caches.open("v1");
    const second = await caches.open("v1");
    return [first === second, (await second.keys()).length];
  });
  await settle("cachePost", async () => {
    const cache = await caches.open("v1");
    return cache.put(new Request("/p", { method: "POST" }), new Response("x"));
  });
  await settle("cacheBadScheme", async () => {
    const cache = await caches.open("v1");
    return cache.put("data:text/plain,x", new Response("x"));
  });
  await settle("cachePartial", async () => {
    const cache = await caches.open("v1");
    return cache.put("/q", new Response("x", { status: 206 }));
  });
  await settle("cacheUsedBody", async () => {
    const cache = await caches.open("v1");
    const response = new Response("x");
    await response.text();
    return cache.put("/r", response);
  });
  await caches.delete("v1");
  return JSON.stringify(out);
})();
