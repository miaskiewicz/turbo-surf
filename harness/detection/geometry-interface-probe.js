(() => {
  const describe = (constructor) => ({
    length: constructor.length,
    fromLength: constructor.fromRect.length,
    properties: Object.getOwnPropertyNames(constructor.prototype),
    tag: Object.prototype.toString.call(new constructor()),
    x: Object.getOwnPropertyDescriptor(constructor.prototype, "x"),
  });
  const shape = Object.fromEntries(
    [DOMRectReadOnly, DOMRect].map((constructor) => {
      const description = describe(constructor);
      description.x = {
        enumerable: description.x.enumerable,
        configurable: description.x.configurable,
        getName: description.x.get.name,
        getLength: description.x.get.length,
        setName: description.x.set?.name ?? null,
        setLength: description.x.set?.length ?? null,
      };
      return [constructor.name, description];
    }),
  );
  const negative = new DOMRect(10, 20, -4, -6);
  const initial = negative.toJSON();
  negative.x = "12";
  const edited = negative.toJSON();
  Object.defineProperty(negative, "x", {
    get() {
      throw Error("public shadow");
    },
  });
  const privateJSON = negative.toJSON();
  const logs = [];
  const dictionary = {};
  for (const name of ["x", "y", "width", "height"])
    Object.defineProperty(dictionary, name, {
      get() {
        logs.push(`get:${name}`);
        return {
          valueOf() {
            logs.push(`number:${name}`);
            return 2;
          },
        };
      },
    });
  const converted = DOMRect.fromRect(dictionary).toJSON();
  const errors = {};
  const fail = (name, callback) => {
    try {
      callback();
      errors[name] = "accepted";
    } catch (error) {
      errors[name] = error.name;
    }
  };
  const read = Object.getOwnPropertyDescriptor(DOMRectReadOnly.prototype, "x").get;
  const write = Object.getOwnPropertyDescriptor(DOMRect.prototype, "x").set;
  fail("forged", () => read.call({}));
  fail("primitive", () => read.call(2));
  fail("readOnlySetter", () => write.call(new DOMRectReadOnly(), 2));
  fail("directCall", () => DOMRect());
  fail("bigint", () => new DOMRect(1n));
  fail(
    "wrappedBigint",
    () =>
      new DOMRect({
        valueOf() {
          return 1n;
        },
      }),
  );
  fail("dictionaryPrimitive", () => DOMRect.fromRect(2));
  const child = class extends DOMRect {};
  const subclass = new child(1, 2, 3, 4);
  const special = new DOMRect(-0, NaN, Infinity, -Infinity);
  return {
    shape,
    inheritedConstructor: Object.getPrototypeOf(DOMRect) === DOMRectReadOnly,
    initial,
    edited,
    privateJSON,
    logs,
    converted,
    errors,
    subclass: {
      ownPrototype: Object.getPrototypeOf(subclass) === child.prototype,
      rect: subclass.toJSON(),
    },
    special: {
      negativeZero: Object.is(special.x, -0),
      nan: Number.isNaN(special.y),
      infinity: special.width === Infinity,
      json: special.toJSON(),
    },
    fromBorrowed: Object.prototype.toString.call(DOMRect.fromRect.call(DOMRectReadOnly, { x: 3 })),
    native: Function.prototype.toString.call(read),
  };
})();
