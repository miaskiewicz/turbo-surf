(() => {
  const inert = document.implementation.createHTMLDocument("authored");
  inert.body.innerHTML = "<p>retained</p>";
  return Object.fromEntries(
    ["clear", "captureEvents", "releaseEvents"].map((name) => {
      const descriptor = Object.getOwnPropertyDescriptor(Document.prototype, name);
      if (!descriptor) return [name, { missing: true }];
      const fn = descriptor.value;
      let hooks = 0;
      const ignored = {
        toString() {
          hooks++;
          throw new Error("Ignored argument evaluated");
        },
      };
      const before = inert.body.innerHTML;
      const result = fn.call(inert, ignored);
      const errors = [{}, null, Object.create(Document.prototype)].map((receiver) => {
        try {
          fn.call(receiver);
          return null;
        } catch (error) {
          return error.name;
        }
      });
      return [
        name,
        {
          name: fn.name,
          length: fn.length,
          enumerable: descriptor.enumerable,
          configurable: descriptor.configurable,
          writable: descriptor.writable,
          constructible: Object.hasOwn(fn, "prototype"),
          native: Function.prototype.toString.call(fn).includes("[native code]"),
          undefined: result === undefined,
          unchanged: inert.body.innerHTML === before,
          hooks,
          errors,
        },
      ];
    }),
  );
})();
