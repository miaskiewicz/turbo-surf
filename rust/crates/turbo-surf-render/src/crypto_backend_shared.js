// The native factories and their descriptor metadata live for the isolate.
// Capture them in its main realm before a document can bootstrap child realms.
(function (core) {
  "use strict";
  const ops = core.ops;
  const rawCrypto = ops.Crypto,
    rawSubtle = ops.SubtleCrypto,
    rawKey = ops.CryptoKey;
  const get = ops.op_interface_record_get,
    set = ops.op_interface_record_set;
  let shared = get(rawCrypto);
  if (!shared) {
    shared = Object.create(null);
    shared.kind = "NativeCryptoBackend";
    set(rawCrypto, shared);
  }
  if (Object.hasOwn(shared, "cryptoBackend")) return;
  const backend = Object.create(null);
  backend.createCrypto = rawCrypto.create;
  backend.createSubtle = rawSubtle.create;
  backend.crypto = Object.getOwnPropertyDescriptors(rawCrypto.prototype);
  backend.subtle = Object.getOwnPropertyDescriptors(rawSubtle.prototype);
  backend.key = Object.getOwnPropertyDescriptors(rawKey.prototype);
  backend.supports = rawSubtle.supports;
  backend.typeErrorPrototype = Object.getPrototypeOf(core.buildCustomError("TypeError", ""));
  Object.defineProperty(shared, "cryptoBackend", { value: backend });
});
