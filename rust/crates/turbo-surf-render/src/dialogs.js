{
  const owner = globalThis,
    invoke = ops.op_dialog,
    text = String,
    ErrorType = TypeError;
  const toText = (value) => {
    if (typeof value === "symbol") throw new ErrorType("Cannot convert a Symbol value to a string");
    return text(value);
  };
  const receiver = (value) => {
    const window = value == null ? owner : value;
    if (!ops.op_surf_is_window(window)) throw new ErrorType("Illegal invocation");
    return window;
  };
  const methods = {
    alert(message = "") {
      invoke(receiver(this), "alert", toText(message), "");
    },
    confirm(message = "") {
      return invoke(receiver(this), "confirm", toText(message), "");
    },
    prompt(message = "", defaultValue = "") {
      return invoke(receiver(this), "prompt", toText(message), toText(defaultValue));
    },
  };
  for (const name of ["alert", "confirm", "prompt"]) {
    Object.defineProperty(owner, name, {
      __proto__: null,
      value: __documentFunction(methods[name], name, 0),
      writable: true,
      configurable: true,
      enumerable: true,
    });
  }
}
