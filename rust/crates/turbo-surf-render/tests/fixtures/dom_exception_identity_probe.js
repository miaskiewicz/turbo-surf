(async () => {
  "use strict";
  // turbo-surf#42: every DOMException a page sees — authored, binding-raised (decode()),
  // natively raised (a selector SyntaxError) — is an instance of its realm's one DOMException
  // interface, serializable by structuredClone, and brand-checked across same-origin realms.
  const child = document.getElementById("f").contentWindow;
  const shape = (error, Exception) => [
    error instanceof Exception,
    Object.getPrototypeOf(error) === Exception.prototype,
    error.constructor === Exception,
    error.name,
    error.code,
  ];
  const clone = (value) => {
    try {
      const copy = structuredClone(value);
      return [copy instanceof DOMException, copy.name ?? null, copy.message ?? null];
    } catch (error) {
      return ["throw", error.name];
    }
  };
  const decode = await document
    .createElement("img")
    .decode()
    .catch((error) => error);
  const childDecode = await child.document
    .createElement("img")
    .decode()
    .catch((error) => error);
  const authored = new DOMException("authored", "InvalidStateError");
  const foreign = new child.DOMException("m", "SecurityError");
  const getter = (name) => Object.getOwnPropertyDescriptor(DOMException.prototype, name).get;
  let native;
  try {
    document.querySelector("##");
  } catch (error) {
    native = error;
  }
  return {
    authored: shape(authored, DOMException),
    authoredClone: clone(authored),
    decode: shape(decode, DOMException),
    decodeClone: clone(decode),
    childDecode: [childDecode instanceof child.DOMException, childDecode instanceof DOMException],
    selector: shape(native, DOMException),
    crossRealmGetters: ["name", "message", "code"].map((name) => {
      try {
        return getter(name).call(foreign);
      } catch (error) {
        return "throw " + error.name;
      }
    }),
    foreignClone: clone(foreign),
  };
})();
