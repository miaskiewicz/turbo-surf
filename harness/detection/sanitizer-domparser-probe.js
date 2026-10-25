(async () => {
  const out = {};
  const run = (fn) => {
    try {
      const value = fn();
      return { value: value === undefined ? "<undefined>" : value };
    } catch (error) {
      return { error: error.name, message: error.message };
    }
  };
  const describe = (owner, key) => {
    const descriptor = Object.getOwnPropertyDescriptor(owner, key);
    if (!descriptor) return null;
    const row = { enumerable: descriptor.enumerable, configurable: descriptor.configurable };
    if ("writable" in descriptor) row.writable = descriptor.writable;
    for (const part of ["get", "set", "value"])
      if (typeof descriptor[part] === "function") row[part] = [descriptor[part].name, descriptor[part].length];
      else if (part in descriptor && part !== "value") row[part] = null;
      else if (part === "value" && part in descriptor) row.value = String(descriptor.value);
    return row;
  };
  out.surface = {
    global: describe(globalThis, "DOMParser"),
    length: DOMParser.length,
    keys: Reflect.ownKeys(DOMParser.prototype).map(String),
    parseFromString: describe(DOMParser.prototype, "parseFromString"),
    tag: describe(DOMParser.prototype, Symbol.toStringTag),
    constructor: describe(DOMParser.prototype, "constructor"),
    prototype: describe(DOMParser, "prototype"),
    source: [Function.prototype.toString.call(DOMParser), Function.prototype.toString.call(DOMParser.prototype.parseFromString)],
    statics: Reflect.ownKeys(DOMParser).map(String).sort(),
    chain: [Object.getPrototypeOf(DOMParser) === Function.prototype, Object.getPrototypeOf(DOMParser.prototype) === Object.prototype],
    instance: run(() => [Object.prototype.toString.call(new DOMParser(1, 2)), Reflect.ownKeys(new DOMParser()).length]),
    subclass: run(() => {
      class Sub extends DOMParser {}
      const parser = new Sub();
      return [Object.getPrototypeOf(parser) === Sub.prototype, parser.parseFromString("<p>s</p>", "text/html").body.innerHTML];
    }),
  };
  const parser = new DOMParser();
  out.errors = {
    call: run(() => DOMParser()),
    brand: run(() => DOMParser.prototype.parseFromString.call({}, "<p>", "text/html")),
    brandUndefined: run(() => DOMParser.prototype.parseFromString.call(undefined, "<p>", "text/html")),
    brandProto: run(() => DOMParser.prototype.parseFromString.call(Object.create(DOMParser.prototype), "<p>", "text/html")),
    zero: run(() => parser.parseFromString()),
    one: run(() => parser.parseFromString("<p>")),
    upper: run(() => parser.parseFromString("<p>", "TEXT/HTML")),
    plain: run(() => parser.parseFromString("<p>", "text/plain")),
    symbolString: run(() => parser.parseFromString(Symbol(), "text/html")),
    symbolType: run(() => parser.parseFromString("<p>", Symbol())),
    order: run(() => {
      const log = [];
      try {
        parser.parseFromString({ toString() { log.push("string"); return "x"; } }, { toString() { log.push("type"); return "bad"; } });
      } catch (error) {
        log.push(error.name);
      }
      return log;
    }),
  };
  const shape = (doc) => [Object.prototype.toString.call(doc), doc.contentType, doc.URL === document.URL, doc.documentURI === document.URL, doc.baseURI === document.baseURI, doc.compatMode, doc.characterSet, doc.readyState, doc.defaultView, doc.location, doc.doctype && doc.doctype.name, doc.head && doc.head.innerHTML, doc.body && doc.body.innerHTML, doc.title, doc.hidden, doc.visibilityState, doc.referrer, doc.cookie, doc === document];
  out.html = {
    standards: run(() => shape(parser.parseFromString('<!doctype html><title>t</title><p id=x>y</p><script>window.__domParserRan=1</script><div><template shadowrootmode=open><b>x</b></template></div>', "text/html"))),
    quirks: run(() => shape(parser.parseFromString("<p>q", "text/html"))),
    nullInput: run(() => parser.parseFromString(null, "text/html").body.innerHTML),
    numberInput: run(() => parser.parseFromString(5, "text/html").body.innerHTML),
    noscript: run(() => {
      const doc = parser.parseFromString("<body><noscript><b>n</b></noscript>", "text/html");
      return [doc.body.firstChild.firstChild.nodeType, doc.body.innerHTML];
    }),
    declarative: run(() => {
      const doc = parser.parseFromString('<div id=h><template shadowrootmode=open><b>x</b></template></div>', "text/html");
      return [doc.getElementById("h").shadowRoot, doc.body.innerHTML];
    }),
    scriptsInert: run(() => {
      const doc = parser.parseFromString("<body><script>window.__domParserMoved=1</script>", "text/html");
      const script = doc.querySelector("script");
      document.body.append(document.adoptNode(script));
      script.remove();
      return [window.__domParserRan ?? null, window.__domParserMoved ?? null];
    }),
    independent: run(() => {
      const first = parser.parseFromString("<p>a</p>", "text/html");
      const second = parser.parseFromString("<p>a</p>", "text/html");
      return [first !== second, first.body.firstChild.ownerDocument === first, first.implementation !== document.implementation];
    }),
  };
  // The XML types parse into XMLDocuments in Chrome.
  out.xml = {
    applicationXml: run(() => {
      const doc = parser.parseFromString("<a><b/></a>", "application/xml");
      return [Object.prototype.toString.call(doc), doc.contentType, doc.documentElement.outerHTML];
    }),
    svg: run(() => Object.prototype.toString.call(parser.parseFromString('<svg xmlns="http://www.w3.org/2000/svg"/>', "image/svg+xml"))),
    xhtml: run(() => Object.prototype.toString.call(parser.parseFromString('<html xmlns="http://www.w3.org/1999/xhtml"/>', "application/xhtml+xml"))),
    textXml: run(() => Object.prototype.toString.call(parser.parseFromString("<a/>", "text/xml"))),
  };
  return out;
})()
