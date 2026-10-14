(() => {
  const define = Object.defineProperty,
    apply = Reflect.apply;
  const base = document.createElement("base"),
    frame = document.createElement("iframe");
  base.setAttribute("href", "/native-frame-base/");
  document.head.appendChild(base);
  frame.setAttribute("src", "child");
  const attribute = Element.prototype.getAttribute,
    query = Document.prototype.querySelector;
  const docOwn = Object.getOwnPropertyDescriptor(document, "URL"),
    queryOwn = Object.getOwnPropertyDescriptor(document, "querySelector");
  const url = URL,
    urlHref = Object.getOwnPropertyDescriptor(URL.prototype, "href");
  let hooks = 0,
    result;
  define(document, "URL", {
    __proto__: null,
    configurable: true,
    get() {
      hooks++;
      return "https://forged.test/";
    },
  });
  define(document, "querySelector", {
    __proto__: null,
    configurable: true,
    value(...args) {
      hooks++;
      return apply(query, this, args);
    },
  });
  Element.prototype.getAttribute = function (name) {
    if (this === frame || this === base) hooks++;
    return apply(attribute, this, [name]);
  };
  define(URL.prototype, "href", {
    __proto__: null,
    configurable: true,
    get() {
      hooks++;
      return apply(urlHref.get, this, []);
    },
    set: urlHref.set,
  });
  globalThis.URL = function () {
    hooks++;
    throw new Error("page URL constructor must not run during native iframe source resolution");
  };
  try {
    result = {
      hooksBefore: hooks,
      resolved: frame.src.endsWith("/native-frame-base/child"),
      hooks,
    };
  } finally {
    globalThis.URL = url;
    define(url.prototype, "href", urlHref);
    Element.prototype.getAttribute = attribute;
    if (docOwn) define(document, "URL", docOwn);
    else delete document.URL;
    if (queryOwn) define(document, "querySelector", queryOwn);
    else delete document.querySelector;
    base.remove();
  }
  return JSON.stringify(result);
})();
