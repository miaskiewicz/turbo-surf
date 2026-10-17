async (operation) => {
  if (operation.init) {
    const local = localStorage,
      session = sessionStorage,
      OriginalEvent = StorageEvent;
    const codeUnit = String.prototype.charCodeAt;
    const units = (text) => {
      if (text === null) return null;
      const result = [];
      for (let index = 0; index < text.length; index++) result.push(codeUnit.call(text, index));
      return result;
    };
    globalThis.__peerEvents = [];
    globalThis.__peerWriteTrace = [];
    globalThis.__peerWrite = (steps) => {
      for (const [area, method, key, value] of steps) {
        const storage = area === "local" ? local : session;
        if (method === "set") storage.setItem(key, value);
        else if (method === "delete") storage.removeItem(key);
        else storage.clear();
      }
    };
    globalThis.__peerRead = () => ({
      local: local.getItem("back"),
      session: session.getItem("secret"),
    });
    addEventListener("storage", (event) => {
      const source = new URL(event.url);
      __peerEvents.push([
        "event",
        units(event.key),
        units(event.oldValue),
        units(event.newValue),
        source.pathname + source.search + source.hash,
        event.isTrusted,
        event.storageArea === local ? "local" : event.storageArea === session ? "session" : "wrong",
        event instanceof OriginalEvent,
        event.target === window,
        event.currentTarget === window,
        event.bubbles,
        event.cancelable,
        event.composed,
      ]);
      queueMicrotask(() => __peerEvents.push(["microtask", units(event.key)]));
    });
  }
  if (operation.tamper) {
    Object.defineProperty(globalThis, "localStorage", {
      get() {
        throw new Error("public getter");
      },
    });
    globalThis.StorageEvent = function () {
      throw new Error("public constructor");
    };
  }
  if (operation.steps) {
    __peerWriteTrace.length = 0;
    __peerWrite(operation.steps);
    __peerWriteTrace.push("write");
    await Promise.resolve();
    __peerWriteTrace.push("microtask");
  }
  return { events: __peerEvents.slice(), writeTrace: __peerWriteTrace.slice(), ...__peerRead() };
};
