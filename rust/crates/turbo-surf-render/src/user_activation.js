(() => {
  const make = __G.makeNativeFunction,
    define = Object.defineProperty;
  const create = Object.create,
    prototypeOf = Object.getPrototypeOf;
  const read = ops.op_user_activation_read,
    navigatorRead = ops.op_user_activation_navigator;
  const ErrorType = TypeError;
  const Constructor = make(
    function () {
      throw new ErrorType("Failed to construct 'UserActivation': Illegal constructor");
    },
    "UserActivation",
    0,
    true,
  );
  const prototype = Constructor.prototype;
  delete prototype.constructor;
  for (const [name, sticky] of [
    ["hasBeenActive", true],
    ["isActive", false],
  ]) {
    define(prototype, name, {
      get: make(
        function () {
          return read(this, sticky);
        },
        "get " + name,
        0,
      ),
      enumerable: true,
      configurable: true,
    });
  }
  define(prototype, "constructor", { value: Constructor, writable: true, configurable: true });
  define(prototype, Symbol.toStringTag, { value: "UserActivation", configurable: true });
  define(Constructor, "prototype", { writable: false });
  define(globalThis, "UserActivation", {
    value: Constructor,
    writable: true,
    enumerable: false,
    configurable: true,
  });
  const activation = create(prototype);
  if (!ops.op_user_activation_install(navigator, activation))
    throw new ErrorType("Native activation state initialization interrupted");
  define(prototypeOf(navigator), "userActivation", {
    get: make(
      function () {
        return navigatorRead(this);
      },
      "get userActivation",
      0,
    ),
    enumerable: true,
    configurable: true,
  });
})();
