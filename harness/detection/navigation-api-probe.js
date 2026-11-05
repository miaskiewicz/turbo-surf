(async () => {
  // window.navigation (turbo-surf#108/#143): same-document entries, navigate()
  // with fragments and intercept(), traversal, updateCurrentEntry() and the
  // navigate / currententrychange / navigatesuccess / navigateerror events.
  const out = {};
  const log = [];
  const where = (url) => (url === null ? null : new URL(url).pathname + new URL(url).hash);
  const entry = (item) =>
    item && [
      Object.prototype.toString.call(item),
      where(item.url),
      item.index,
      item.sameDocument,
      typeof item.key,
      typeof item.id,
    ];
  const settle = async (name, callback) => {
    try {
      out[name] = ["ok", await callback()];
    } catch (error) {
      out[name] = [error.name, error.message];
    }
  };
  const tick = () => new Promise((resolve) => setTimeout(resolve, 20));
  const result = async (value) => {
    const settled = [];
    for (const name of ["committed", "finished"])
      try {
        settled.push([name, entry(await value[name])]);
      } catch (error) {
        settled.push([name, error.name, error.message]);
      }
    return settled;
  };
  let intercept = null;
  navigation.addEventListener("navigate", (event) => {
    log.push([
      "navigate",
      Object.prototype.toString.call(event),
      event.navigationType,
      where(event.destination.url),
      event.destination.sameDocument,
      event.destination.index,
      event.hashChange,
      event.canIntercept,
      event.cancelable,
      event.userInitiated,
      event.info === undefined ? "none" : event.info,
      event.signal instanceof AbortSignal,
      event.formData,
      event.downloadRequest,
    ]);
    if (intercept) intercept(event);
  });
  navigation.addEventListener("currententrychange", (event) =>
    log.push([
      "currententrychange",
      Object.prototype.toString.call(event),
      event.navigationType,
      entry(event.from),
      where(navigation.currentEntry.url),
    ]),
  );
  navigation.addEventListener("navigatesuccess", (event) =>
    log.push(["navigatesuccess", Object.prototype.toString.call(event)]),
  );
  navigation.addEventListener("navigateerror", (event) =>
    log.push(["navigateerror", event.error && event.error.name, event.message]),
  );
  addEventListener("popstate", (event) => log.push(["popstate", event.state]));
  addEventListener("hashchange", (event) =>
    log.push(["hashchange", where(event.oldURL), where(event.newURL)]),
  );
  const drain = async () => {
    await tick();
    return log.splice(0);
  };

  await settle("shape", () => [
    Object.prototype.toString.call(navigation),
    navigation instanceof Navigation,
    navigation instanceof EventTarget,
    typeof NavigationHistoryEntry,
    typeof NavigateEvent,
    typeof NavigationDestination,
    typeof NavigationCurrentEntryChangeEvent,
    navigation.entries().length,
    entry(navigation.currentEntry),
    navigation.canGoBack,
    navigation.canGoForward,
    navigation.transition,
    navigation.currentEntry === navigation.entries()[0],
    navigation.currentEntry.getState(),
  ]);
  await settle("fragment", async () => {
    const value = navigation.navigate("#a", { info: "i", state: { n: 1 } });
    const sync = [
      where(location.href),
      navigation.transition && navigation.transition.navigationType,
    ];
    return [sync, await result(value), await drain(), navigation.currentEntry.getState()];
  });
  await settle("afterFragment", () => [
    navigation.entries().map(entry),
    navigation.canGoBack,
    navigation.canGoForward,
    history.length,
  ]);
  await settle("intercepted", async () => {
    let resolveHandler;
    intercept = (event) =>
      event.intercept({
        handler: () =>
          new Promise((resolve) => {
            resolveHandler = resolve;
          }),
      });
    const value = navigation.navigate("/page");
    intercept = null;
    const sync = [
      where(location.href),
      navigation.transition && [
        Object.prototype.toString.call(navigation.transition),
        navigation.transition.navigationType,
        entry(navigation.transition.from),
      ],
    ];
    const committed = entry(await value.committed);
    const before = log.splice(0);
    resolveHandler();
    return [sync, committed, before, await result(value), await drain(), navigation.transition];
  });
  await settle("interceptRejects", async () => {
    intercept = (event) =>
      event.intercept({ handler: () => Promise.reject(new RangeError("handler")) });
    const value = navigation.navigate("/fail");
    intercept = null;
    return [await result(value), await drain(), where(location.href)];
  });
  await settle("prevented", async () => {
    intercept = (event) => event.preventDefault();
    const value = navigation.navigate("#blocked");
    intercept = null;
    return [await result(value), await drain(), where(location.href)];
  });
  await settle("replace", async () => {
    const value = navigation.navigate("#r", { history: "replace" });
    return [await result(value), await drain(), navigation.entries().length];
  });
  await settle("updateCurrentEntry", async () => {
    navigation.updateCurrentEntry({ state: { s: 2 } });
    return [navigation.currentEntry.getState(), await drain()];
  });
  await settle("pushState", async () => {
    history.pushState({ h: 1 }, "", "/pushed");
    return [
      await drain(),
      entry(navigation.currentEntry),
      navigation.currentEntry.getState(),
      navigation.entries().length,
    ];
  });
  await settle("back", async () => {
    const value = navigation.back();
    return [await result(value), await drain(), where(location.href), history.state];
  });
  await settle("traverseTo", async () => {
    const first = navigation.entries()[0];
    const value = navigation.traverseTo(first.key);
    return [await result(value), await drain(), where(location.href), navigation.canGoForward];
  });
  await settle("forward", async () => {
    const value = navigation.forward();
    return [await result(value), await drain(), where(location.href)];
  });
  await settle("backAtStart", async () => {
    await navigation.traverseTo(navigation.entries()[0].key).finished;
    await drain();
    return result(navigation.back());
  });
  await settle("traverseUnknown", () => result(navigation.traverseTo("nope")));
  await settle("badURL", () => result(navigation.navigate("http://[")));
  await settle("badHistory", () => navigation.navigate("#x", { history: "bogus" }));
  await settle("noArgs", () => navigation.navigate());
  await settle("reloadIntercepted", async () => {
    intercept = (event) => event.intercept();
    const value = navigation.reload({ info: "again" });
    intercept = null;
    return [await result(value), await drain()];
  });
  await settle("interceptOutside", async () => {
    let saved;
    intercept = (event) => {
      saved = event;
    };
    await navigation.navigate("#late").finished;
    intercept = null;
    await drain();
    try {
      saved.intercept();
      return "ok";
    } catch (error) {
      return [error.name, error.message];
    }
  });
  await settle("entryIdentity", () => [
    navigation.entries()[0] === navigation.entries()[0],
    navigation.currentEntry === navigation.entries()[navigation.currentEntry.index],
    new Set(navigation.entries().map((item) => item.key)).size,
  ]);
  return JSON.stringify(out);
})();
