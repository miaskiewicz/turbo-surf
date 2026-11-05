(() => {
  // The document.fonts accessor's identity (a bot-detection tell when it is a JS
  // closure): name, length, source text, descriptor shape and the brand-check error.
  const descriptor = Object.getOwnPropertyDescriptor(Document.prototype, "fonts");
  const get = descriptor.get;
  const brand = (receiver) => {
    try {
      Reflect.apply(get, receiver, []);
      return null;
    } catch (error) {
      return [error.constructor.name, error.message];
    }
  };
  return JSON.stringify({
    own: Object.hasOwn(document, "fonts"),
    keys: Object.keys(descriptor).sort(),
    set: descriptor.set === undefined,
    enumerable: descriptor.enumerable,
    configurable: descriptor.configurable,
    name: get.name,
    length: get.length,
    source: Function.prototype.toString.call(get),
    prototype: Object.hasOwn(get, "prototype"),
    construct: (() => {
      try {
        Reflect.construct(get, []);
        return null;
      } catch (error) {
        return error.constructor.name;
      }
    })(),
    same: get.call(document) === document.fonts,
    brand: { object: brand({}), undefined: brand(undefined), element: brand(document.body) },
  });
})();
