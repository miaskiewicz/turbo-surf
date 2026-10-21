(async () => {
  const tag = (value) => Object.prototype.toString.call(value);
  const descriptor = (object, key) => {
    const value = Object.getOwnPropertyDescriptor(object, key);
    return (
      value && {
        value:
          typeof value.value === "function"
            ? { name: value.value.name, length: value.value.length }
            : value.value,
        writable: value.writable,
        enumerable: value.enumerable,
        configurable: value.configurable,
      }
    );
  };
  const prototype = XMLHttpRequest.prototype;
  const methods = {
    abort: [],
    getAllResponseHeaders: [],
    getResponseHeader: ["content-type"],
    open: ["GET", "/xhr"],
    send: [],
    setRequestHeader: ["X-Control", "yes"],
  };
  const constants = ["UNSENT", "OPENED", "HEADERS_RECEIVED", "LOADING", "DONE"];
  const result = {
    constructor: {
      name: XMLHttpRequest.name,
      length: XMLHttpRequest.length,
      prototype: {
        writable: Object.getOwnPropertyDescriptor(XMLHttpRequest, "prototype").writable,
      },
    },
    tag: descriptor(prototype, Symbol.toStringTag),
    prototypeBrand: tag(prototype),
    global: {
      writable: Object.getOwnPropertyDescriptor(globalThis, "XMLHttpRequest").writable,
      enumerable: Object.getOwnPropertyDescriptor(globalThis, "XMLHttpRequest").enumerable,
      configurable: Object.getOwnPropertyDescriptor(globalThis, "XMLHttpRequest").configurable,
    },
    methods: Object.fromEntries(
      Object.keys(methods).map((name) => [name, descriptor(prototype, name)]),
    ),
    constants: Object.fromEntries(
      constants.map((name) => [
        name,
        { constructor: descriptor(XMLHttpRequest, name), prototype: descriptor(prototype, name) },
      ]),
    ),
    receivers: {},
  };
  for (const [name, args] of Object.entries(methods)) {
    result.receivers[name] = [];
    for (const receiver of [{}, Object.create(prototype), new Proxy(new XMLHttpRequest(), {})]) {
      try {
        Reflect.apply(prototype[name], receiver, args);
        result.receivers[name].push("accepted");
      } catch (error) {
        result.receivers[name].push(error.name);
      }
    }
  }
  class Derived extends XMLHttpRequest {}
  const derived = new Derived();
  result.subclass = {
    brand: tag(derived),
    headers: Reflect.apply(prototype.getAllResponseHeaders, derived, []),
  };
  const detachedPrototype = new XMLHttpRequest();
  Object.setPrototypeOf(detachedPrototype, null);
  result.detachedPrototypeReceiver = Reflect.apply(
    prototype.getAllResponseHeaders,
    detachedPrototype,
    [],
  );
  const get = WeakMap.prototype.get,
    set = WeakMap.prototype.set;
  let hooks = 0;
  try {
    WeakMap.prototype.get = WeakMap.prototype.set = () => {
      hooks++;
      throw new Error("guest WeakMap hook");
    };
    const request = new XMLHttpRequest();
    result.privateStorage = {
      brand: tag(request),
      headers: Reflect.apply(prototype.getAllResponseHeaders, request, []),
      hooks,
    };
  } finally {
    WeakMap.prototype.get = get;
    WeakMap.prototype.set = set;
  }
  const request = new XMLHttpRequest(),
    states = [];
  request.addEventListener("readystatechange", () => states.push(request.readyState));
  const loaded = new Promise((resolve, reject) => {
    request.addEventListener("load", resolve);
    request.addEventListener("error", () => reject(new Error("XHR fixture failed")));
  });
  request.open("GET", "/xhr");
  result.openBrand = tag(request);
  request.send();
  await loaded;
  result.network = {
    brand: tag(request),
    status: request.status,
    text: request.responseText,
    contentType: request.getResponseHeader("content-type"),
    states,
  };
  return result;
})();
