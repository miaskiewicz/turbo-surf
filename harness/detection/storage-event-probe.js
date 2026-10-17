(async () => {
  const f = document.createElement("iframe");
  const loaded = new Promise((resolve) => (f.onload = resolve));
  document.body.append(f);
  await loaded;
  const w = f.contentWindow;
  localStorage.clear();
  sessionStorage.clear();
  await new Promise((resolve) => setTimeout(resolve, 20));
  const borrowed = Object.getOwnPropertyDescriptor(globalThis, "localStorage").get.call(w);
  const borrowedShape = {
    same: borrowed === w.localStorage,
    prototype: Object.getPrototypeOf(borrowed) === w.Storage.prototype,
  };
  const handler = [];
  const handlerInitial = w.onstorage === null;
  w.onstorage = function (event) {
    handler.push([
      event.key === null
        ? null
        : Array.from({ length: event.key.length }, (_, i) => event.key.charCodeAt(i)),
      this === w,
      event.isTrusted,
      event.storageArea === w.localStorage ? "local" : "session",
    ]);
  };
  const events = [],
    self = [];
  w.addEventListener("storage", (event) =>
    events.push({
      key:
        event.key === null
          ? null
          : Array.from({ length: event.key.length }, (_, i) => event.key.charCodeAt(i)),
      old:
        event.oldValue === null
          ? null
          : Array.from({ length: event.oldValue.length }, (_, i) => event.oldValue.charCodeAt(i)),
      new:
        event.newValue === null
          ? null
          : Array.from({ length: event.newValue.length }, (_, i) => event.newValue.charCodeAt(i)),
      trusted: event.isTrusted,
      target: event.target === w,
      area:
        event.storageArea === w.localStorage
          ? "local"
          : event.storageArea === w.sessionStorage
            ? "session"
            : "other",
      url: event.url.replace(/:[0-9]+\//, ":PORT/"),
    }),
  );
  addEventListener("storage", (event) => self.push(event.key));
  localStorage.setItem("k\ud800", "v\udc00");
  localStorage.setItem("k\ud800", "v\udc00");
  localStorage.setItem("k\ud800", "next");
  localStorage.removeItem("missing");
  localStorage.removeItem("k\ud800");
  localStorage.clear();
  localStorage.setItem("other", "one");
  localStorage.clear();
  sessionStorage.setItem("session", "two");
  await new Promise((resolve) => setTimeout(resolve, 30));
  const childSource = [];
  addEventListener("storage", (event) =>
    childSource.push({
      key: event.key,
      url: event.url.replace(/:[0-9]+\//, ":PORT/"),
      area: event.storageArea === localStorage,
    }),
  );
  const childSelf = [];
  w.addEventListener("storage", (event) => childSelf.push(event.key));
  borrowed.setItem("child-source", "yes");
  await new Promise((resolve) => setTimeout(resolve, 30));
  const members = Object.getOwnPropertyNames(StorageEvent.prototype).map((name) => {
    const d = Object.getOwnPropertyDescriptor(StorageEvent.prototype, name);
    return [
      name,
      d.enumerable,
      d.configurable,
      d.writable ?? null,
      (d.get ?? d.value)?.length ?? null,
    ];
  });
  const legacy = document.createEvent("StorageEvent");
  const legacyInitial = {
    type: legacy.type,
    key: legacy.key,
    old: legacy.oldValue,
    new: legacy.newValue,
    url: legacy.url,
    area: legacy.storageArea,
    trusted: legacy.isTrusted,
  };
  legacy.initStorageEvent("legacy", true, true, "key", "old", "new", "url", localStorage);
  const foreignLegacy = Document.prototype.createEvent.call(w.document, "storageevent");
  const legacyShape = {
    initial: legacyInitial,
    initialized: [
      legacy.type,
      legacy.bubbles,
      legacy.cancelable,
      legacy.key,
      legacy.oldValue,
      legacy.newValue,
      legacy.url,
      legacy.storageArea === localStorage,
    ],
    foreign: Object.getPrototypeOf(foreignLegacy) === w.StorageEvent.prototype,
  };
  const authored = new StorageEvent("storage", {
    key: "\ud800",
    oldValue: 7,
    url: "x\ud800",
    storageArea: localStorage,
  });
  const conversion = [];
  const init = {};
  for (const key of [
    "bubbles",
    "cancelable",
    "composed",
    "key",
    "newValue",
    "oldValue",
    "storageArea",
    "url",
  ])
    Object.defineProperty(init, key, {
      get() {
        conversion.push(key);
        return undefined;
      },
    });
  new StorageEvent(
    {
      toString() {
        conversion.push("type");
        return "type";
      },
    },
    init,
  );
  const shape = {
    handler: {
      local: handler.filter((row) => row[3] === "local"),
      session: handler.filter((row) => row[3] === "session"),
    },
    handlerInitial,
    childSource,
    childSelf,
    borrowedShape,
    legacyShape,
    conversion,
    nullURL: new StorageEvent("x", { url: null }).url,
    members,
    globalFlags: [
      Object.getOwnPropertyDescriptor(window, "StorageEvent").writable,
      Object.getOwnPropertyDescriptor(window, "StorageEvent").enumerable,
      Object.getOwnPropertyDescriptor(window, "StorageEvent").configurable,
    ],
    constructorLength: StorageEvent.length,
    constructorParent: Object.getPrototypeOf(StorageEvent) === Event,
    prototypeParent: Object.getPrototypeOf(StorageEvent.prototype) === Event.prototype,
    ctorNative: Function.prototype.toString.call(StorageEvent).includes("[native code]"),
    tag: Object.prototype.toString.call(authored),
    authored: {
      key: authored.key.charCodeAt(0),
      old: authored.oldValue,
      url: authored.url,
      trusted: authored.isTrusted,
      area: authored.storageArea === localStorage,
    },
    brands: [],
  };
  for (const run of [
    () => new StorageEvent(),
    () => new StorageEvent("x", { storageArea: {} }),
    () => Object.getOwnPropertyDescriptor(StorageEvent.prototype, "key").get.call({}),
    () => authored.initStorageEvent(),
  ]) {
    try {
      run();
      shape.brands.push("none");
    } catch (e) {
      shape.brands.push(e.name);
    }
  }
  f.remove();
  return {
    events: {
      local: events.filter((event) => event.area === "local"),
      session: events.filter((event) => event.area === "session"),
    },
    self,
    shape,
  };
})();
