(async () => {
  const apply = Reflect.apply,
    define = Object.defineProperty,
    own = Object.hasOwn;
  const prototypeOf = Object.getPrototypeOf,
    stringify = JSON.stringify;
  const weakGet = WeakMap.prototype.get,
    setAdd = Set.prototype.add;
  const setEach = Set.prototype.forEach,
    setHas = Set.prototype.has;
  const setIterator = Set.prototype[Symbol.iterator];
  const results = {};
  const exercise = async (setup) => {
    const frame = document.createElement("iframe");
    let hooks = 0;
    const restore = setup(frame, () => {
      hooks++;
    });
    let distinct = false;
    try {
      frame.srcdoc = "<!doctype html><body>owned frame</body>";
      const loaded = new Promise((resolve) =>
        frame.addEventListener("load", resolve, { once: true }),
      );
      document.body.appendChild(frame);
      await loaded;
      const child = frame.contentWindow;
      distinct = !!child && child !== window && child.Array !== Array;
      frame.remove();
    } finally {
      restore();
      if (frame.isConnected) frame.remove();
    }
    return { hooks, distinct };
  };
  results.weakMap = await exercise((frame, hook) => {
    WeakMap.prototype.get = function (key) {
      const result = apply(weakGet, this, [key]);
      if (
        key === frame &&
        result &&
        own(result, "realm") &&
        own(result, "started") &&
        own(result, "owner")
      )
        hook();
      return result;
    };
    return () => {
      WeakMap.prototype.get = weakGet;
    };
  });
  results.hostSet = await exercise((frame, hook) => {
    Set.prototype.add = function (value) {
      if (value === frame) hook();
      return apply(setAdd, this, [value]);
    };
    Set.prototype.forEach = function (...args) {
      if (apply(setHas, this, [frame])) hook();
      return apply(setEach, this, args);
    };
    Set.prototype[Symbol.iterator] = function () {
      if (apply(setHas, this, [frame])) hook();
      return apply(setIterator, this, []);
    };
    return () => {
      Set.prototype.add = setAdd;
      Set.prototype.forEach = setEach;
      Set.prototype[Symbol.iterator] = setIterator;
    };
  });
  results.recordSetters = await exercise((_frame, hook) => {
    const names = ["metadata", "navigationURL", "navigationSource", "navigation"];
    for (const name of names)
      define(Object.prototype, name, {
        __proto__: null,
        configurable: true,
        set(value) {
          if (own(this, "realm") && own(this, "started") && own(this, "owner")) hook();
          define(this, name, {
            __proto__: null,
            value,
            writable: true,
            configurable: true,
            enumerable: true,
          });
        },
      });
    return () => {
      for (const name of names) delete Object.prototype[name];
    };
  });
  let descriptorHooks = 0,
    descriptorError = "";
  define(Object.prototype, "get", {
    __proto__: null,
    configurable: true,
    get() {
      const value = own(this, "value") ? this.value : undefined;
      if (value && typeof value === "object" && prototypeOf(value) === null) descriptorHooks++;
      return undefined;
    },
  });
  try {
    new MessageEvent("message", { data: 1 });
  } catch (error) {
    descriptorError = error.name;
  } finally {
    delete Object.prototype.get;
  }
  results.interfaceDescriptors = { hooks: descriptorHooks, error: descriptorError };
  results.metadataJSON = await exercise((_frame, hook) => {
    define(Object.prototype, "toJSON", {
      __proto__: null,
      configurable: true,
      value() {
        if (own(this, "origin") && own(this, "inherited") && own(this, "location")) hook();
        return this;
      },
    });
    return () => {
      delete Object.prototype.toJSON;
    };
  });
  const call = Function.prototype.call;
  results.functionCall = await exercise((_frame, hook) => {
    Function.prototype.call = function (...args) {
      if (
        ["get tagName", "get ownerDocument", "get isConnected", "get defaultView"].includes(
          this.name,
        )
      )
        hook();
      return apply(call, this, args);
    };
    return () => {
      Function.prototype.call = call;
    };
  });
  const key = Symbol.for("ts"),
    originalNamespace = window[key];
  const namespace = originalNamespace || {};
  const privateNames = [
    "frameRecordFor",
    "frameRealm",
    "loadFrame",
    "prepareFrameAdoption",
    "syncFrameConnections",
    "accessFrame",
    "frameSourceChanged",
    "recaptchaBframeHandshake",
  ];
  const exposed = privateNames.filter((name) => typeof namespace[name] === "function");
  const saved = privateNames.map((name) => Object.getOwnPropertyDescriptor(namespace, name));
  let namespaceHooks = 0,
    namespaceDistinct = false;
  if (!originalNamespace)
    define(window, key, { __proto__: null, configurable: true, value: namespace });
  for (const name of privateNames)
    define(namespace, name, {
      __proto__: null,
      configurable: true,
      writable: true,
      value() {
        namespaceHooks++;
      },
    });
  const namespaceFrame = document.createElement("iframe");
  try {
    document.body.appendChild(namespaceFrame);
    const child = namespaceFrame.contentWindow;
    namespaceDistinct = !!child && child !== window && child.Array !== Array;
  } finally {
    for (let index = 0; index < privateNames.length; index++) {
      const name = privateNames[index];
      if (saved[index]) define(namespace, name, saved[index]);
      else delete namespace[name];
    }
    namespaceFrame.remove();
    if (!originalNamespace) delete window[key];
  }
  results.namespace = { hooks: namespaceHooks, exposed, distinct: namespaceDistinct };
  results.shadowedNode = await exercise((frame) => {
    define(frame, "ownerDocument", {
      __proto__: null,
      configurable: true,
      value: { defaultView: null },
    });
    define(frame, "isConnected", { __proto__: null, configurable: true, value: false });
    return () => {
      delete frame.ownerDocument;
      delete frame.isConnected;
    };
  });
  results.nativeSources = await exercise((frame, hook) => {
    const query = Document.prototype.querySelectorAll,
      attribute = Element.prototype.getAttribute;
    const fields = ["URL", "baseURI", "nodeType"],
      savedFields = fields.map((name) => Object.getOwnPropertyDescriptor(document, name));
    const queryOwn = Object.getOwnPropertyDescriptor(document, "querySelectorAll");
    const nodeType = Object.getOwnPropertyDescriptor(Node.prototype, "nodeType").get;
    const source = Object.getOwnPropertyDescriptor(Document.prototype, "URL").get;
    const base = Object.getOwnPropertyDescriptor(Node.prototype, "baseURI").get;
    define(document, "querySelectorAll", {
      __proto__: null,
      configurable: true,
      value(...args) {
        hook();
        return apply(query, this, args);
      },
    });
    Element.prototype.getAttribute = function (name) {
      if (this === frame && ["src", "srcdoc"].includes(name)) hook();
      return apply(attribute, this, [name]);
    };
    for (let index = 0; index < fields.length; index++) {
      const getter = [source, base, nodeType][index];
      define(document, fields[index], {
        __proto__: null,
        configurable: true,
        get() {
          hook();
          return apply(getter, this, []);
        },
      });
    }
    return () => {
      if (queryOwn) define(document, "querySelectorAll", queryOwn);
      else delete document.querySelectorAll;
      Element.prototype.getAttribute = attribute;
      for (let index = 0; index < fields.length; index++)
        if (savedFields[index]) define(document, fields[index], savedFields[index]);
        else delete document[fields[index]];
    };
  });
  return stringify(results);
})();
