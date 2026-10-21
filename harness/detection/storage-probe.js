(async () => {
  const units = (text) => Array.from({ length: text.length }, (_, i) => text.charCodeAt(i));
  const error = (action) => {
    try {
      action();
      return null;
    } catch (e) {
      return e.name;
    }
  };
  const storage = localStorage;
  storage.clear();
  const quota = [];
  for (const length of [2621439, 2621440, 5242879, 5242880]) {
    storage.clear();
    try {
      storage.setItem("q", "x".repeat(length));
      quota.push({ length, result: "stored" });
    } catch (e) {
      quota.push({ length, result: e.name });
    }
  }
  storage.clear();
  storage.setItem("q", "before");
  const quotaAtomic = {
    failure: error(() => storage.setItem("q", "x".repeat(5242880))),
    value: storage.getItem("q"),
    length: storage.length,
  };
  storage.clear();
  const key = "k\ud800",
    value = "v\udc00\u0000";
  storage.setItem(key, value);
  const lossless = {
    key: units(storage.key(0)),
    value: units(storage.getItem(key)),
    property: units(storage[key]),
    keys: Object.keys(storage).map(units),
    length: storage.length,
  };
  const conversion = {
    keySymbol: error(() => storage.getItem(Symbol())),
    valueSymbol: error(() => storage.setItem("other", Symbol())),
    missing: error(() => storage.setItem("other")),
    unchanged: storage.length,
  };
  storage.clear();
  storage.setItem("b", "1");
  storage.setItem("a", "2");
  storage.setItem("b", "3");
  const order = {
    keys: Object.keys(storage).sort(),
    length: storage.length,
    value: storage.b,
    same: storage === localStorage,
    tag: Object.prototype.toString.call(storage),
  };
  const surface = {
    names: Object.getOwnPropertyNames(Storage.prototype),
    native: String(Storage).includes("[native code]"),
    members: Object.fromEntries(
      ["length", "key", "getItem", "setItem", "removeItem", "clear"].map((name) => {
        const d = Object.getOwnPropertyDescriptor(Storage.prototype, name);
        return [
          name,
          {
            native: String(d.get || d.value).includes("[native code]"),
            length: (d.get || d.value).length,
            enumerable: d.enumerable,
            configurable: d.configurable,
            writable: d.writable ?? null,
          },
        ];
      }),
    ),
  };
  const access = {
    hasType: typeof document.hasStorageAccess,
    requestType: typeof document.requestStorageAccess,
  };
  if (access.hasType === "function") access.has = await document.hasStorageAccess();
  if (access.requestType === "function") {
    try {
      await document.requestStorageAccess();
      access.request = "resolved";
    } catch (e) {
      access.request = e.name;
    }
  }
  access.options = [];
  for (const options of [{}, { all: false }, { localStorage: true }, { all: true }]) {
    try {
      const handle = await document.requestStorageAccess(options);
      access.options.push({
        tag: Object.prototype.toString.call(handle),
        hasLocal: typeof handle?.localStorage,
        hasSession: typeof handle?.sessionStorage,
        localSame: handle?.localStorage === localStorage,
      });
    } catch (e) {
      access.options.push({ error: e.name });
    }
  }
  const inactive = document.implementation.createHTMLDocument("inactive");
  for (const name of ["hasStorageAccess", "requestStorageAccess"]) {
    try {
      access[name + "Inactive"] = await inactive[name]();
    } catch (e) {
      access[name + "Inactive"] = e.name;
    }
  }
  const iframe = document.createElement("iframe");
  iframe.src = location.protocol + "//127.0.0.1:" + location.port + "/widget";
  document.body.append(iframe);
  const partition = await new Promise((resolve) => {
    const listener = (event) => {
      if (event.source === iframe.contentWindow) {
        removeEventListener("message", listener);
        resolve(event.data);
      }
    };
    addEventListener("message", listener);
  });
  iframe.remove();
  const nestedFrame = document.createElement("iframe");
  nestedFrame.src = "http://localhost:" + location.port + "/middle";
  const nested = await new Promise((resolve) => {
    const listener = (event) => {
      if (event.source === nestedFrame.contentWindow) {
        removeEventListener("message", listener);
        resolve(event.data);
      }
    };
    addEventListener("message", listener);
    document.body.append(nestedFrame);
  });
  nestedFrame.remove();
  const policies = [];
  for (const attributes of [
    { sandbox: "allow-scripts" },
    { sandbox: "allow-scripts allow-same-origin" },
    { sandbox: "allow-scripts allow-same-origin allow-storage-access-by-user-activation" },
    { credentialless: "" },
    { allow: "storage-access 'none'" },
  ]) {
    const frame = document.createElement("iframe");
    for (const [name, value] of Object.entries(attributes)) frame.setAttribute(name, value);
    frame.srcdoc = `<script>(async()=>{const result={};for(const name of ['hasStorageAccess','requestStorageAccess']){try {const value=await document[name]();result[name]=value===undefined?'resolved':value;}catch(e){result[name]=e.name;}}parent.postMessage(result,'*');})()</script>`;
    const result = await new Promise((resolve) => {
      const listener = (event) => {
        if (event.source === frame.contentWindow) {
          removeEventListener("message", listener);
          resolve(event.data);
        }
      };
      addEventListener("message", listener);
      document.body.append(frame);
    });
    policies.push(result);
    frame.remove();
  }
  const foreign = document.createElement("iframe");
  document.body.append(foreign);
  const child = foreign.contentWindow;
  const promise = Document.prototype.hasStorageAccess.call(child.document);
  access.foreignPromise = {
    parent: promise instanceof Promise,
    child: promise instanceof child.Promise,
  };
  await promise;
  try {
    const result = Document.prototype.hasStorageAccess.call({});
    try {
      await result;
      access.invalidReceiver = "resolved";
    } catch (e) {
      access.invalidReceiver = "rejected " + e.name;
    }
  } catch (e) {
    access.invalidReceiver = "threw " + e.name;
  }
  const originals = {
    String,
    Number,
    TypeError,
    Promise,
    DOMException,
    resolve: Promise.resolve,
    reject: Promise.reject,
  };
  try {
    globalThis.String = () => "spoofed";
    globalThis.Number = () => 999;
    globalThis.TypeError = function ReplacedError() {};
    globalThis.DOMException = function ReplacedException() {};
    originals.Promise.resolve = () => {
      throw "overwritten resolve";
    };
    originals.Promise.reject = () => {
      throw "overwritten reject";
    };
    globalThis.Promise = {};
    storage.setItem(123, 456);
    let failure;
    try {
      await document.requestStorageAccess({});
    } catch (e) {
      failure = e.name;
    }
    access.tamper = {
      value: storage.getItem(123),
      symbol: error(() => storage.getItem(Symbol())),
      has: await document.hasStorageAccess(),
      failure,
    };
  } finally {
    globalThis.String = originals.String;
    globalThis.Number = originals.Number;
    globalThis.TypeError = originals.TypeError;
    globalThis.Promise = originals.Promise;
    globalThis.DOMException = originals.DOMException;
    originals.Promise.resolve = originals.resolve;
    originals.Promise.reject = originals.reject;
  }
  foreign.remove();
  return {
    quota,
    quotaAtomic,
    lossless,
    conversion,
    order,
    surface,
    access,
    partition,
    nested,
    policies,
  };
})();
