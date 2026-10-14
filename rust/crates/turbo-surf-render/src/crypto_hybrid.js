// Private adapter for the native X-Wing cppgc key. No key material or factory
// is attached to a public interface; all primitives and permissions stay native.
(function (Raw, backend, apply, DOMError) {
  "use strict";
  const descriptors = Object.getOwnPropertyDescriptors(Raw.prototype);
  const native = {};
  for (const name of [
    "isKey",
    "supports",
    "generate",
    "importBytes",
    "importJwk",
    "getPublicKey",
    "exportBytes",
    "exportJwk",
    "exportSerializedJwk",
    "importSerializedJwk",
    "encapsulateBits",
    "decapsulateBits",
  ])
    native[name] = Raw[name];
  const PromiseValue = Promise,
    then = Promise.prototype.then;
  const StringValue = String,
    TypeErrorValue = TypeError,
    ArrayValue = Array;
  const arrayFrom = Array.from,
    define = Object.defineProperty,
    create = Object.create;
  const upper = String.prototype.toUpperCase;
  const Uint8 = Uint8Array,
    descriptor = Object.getOwnPropertyDescriptor;
  const abLength = descriptor(ArrayBuffer.prototype, "byteLength").get;
  const typedPrototype = Object.getPrototypeOf(Uint8Array.prototype);
  const typedBuffer = descriptor(typedPrototype, "buffer").get,
    typedOffset = descriptor(typedPrototype, "byteOffset").get,
    typedLength = descriptor(typedPrototype, "byteLength").get;
  const viewBuffer = descriptor(DataView.prototype, "buffer").get,
    viewOffset = descriptor(DataView.prototype, "byteOffset").get,
    viewLength = descriptor(DataView.prototype, "byteLength").get;

  function string(value) {
    if (typeof value === "symbol") throw new TypeErrorValue("Cannot convert a Symbol to a string");
    return StringValue(value);
  }
  function sequence(value) {
    if (
      value === null ||
      (typeof value !== "object" && typeof value !== "function") ||
      typeof value[Symbol.iterator] !== "function"
    )
      throw new TypeErrorValue("Key usages must be iterable");
    return apply(arrayFrom, ArrayValue, [value, string]);
  }
  function call(name, args) {
    return apply(native[name], Raw, args);
  }
  function isKey(key) {
    return call("isKey", [key]);
  }
  function get(name, key) {
    const source = isKey(key) ? descriptors : backend.key;
    return apply(source[name].get, key, []);
  }
  function bytes(value) {
    try {
      apply(abLength, value, []);
      return new Uint8(value);
    } catch {}
    let buffer, offset, length;
    try {
      buffer = apply(typedBuffer, value, []);
      offset = apply(typedOffset, value, []);
      length = apply(typedLength, value, []);
    } catch {
      buffer = apply(viewBuffer, value, []);
      offset = apply(viewOffset, value, []);
      length = apply(viewLength, value, []);
    }
    apply(abLength, buffer, []);
    return new Uint8(buffer, offset, length);
  }
  function jwk(value) {
    if (value === null || (typeof value !== "object" && typeof value !== "function"))
      throw new TypeErrorValue("JWK dictionary required");
    const result = create(null);
    const fields = ["alg", "ext", "key_ops", "kty", "priv", "pub", "use"];
    for (let index = 0; index < fields.length; index++) {
      const name = fields[index];
      const member = value[name];
      if (member === undefined) continue;
      const converted =
        name === "ext" ? !!member : name === "key_ops" ? sequence(member) : string(member);
      define(result, name, {
        value: converted,
        enumerable: true,
        writable: true,
        configurable: true,
      });
    }
    return result;
  }
  function importKey(format, data, extractable, usages) {
    return format === "jwk"
      ? call("importJwk", [jwk(data), !!extractable, usages])
      : call("importBytes", [format, bytes(data), !!extractable, usages]);
  }
  function exportKey(format, key) {
    return format === "jwk" ? call("exportJwk", [key]) : call("exportBytes", [format, key]);
  }
  function bridge(value) {
    const promise = new PromiseValue((resolve, reject) => apply(then, value, [resolve, reject]));
    // Private await must not consult a page-mutated foreign Promise prototype.
    define(promise, "constructor", { value: PromiseValue });
    return promise;
  }
  function raw(name, receiver, args) {
    return apply(backend.subtle[name].value, receiver, args);
  }
  const payloadAlgorithm = { name: "HMAC", hash: { name: "SHA-256" } };
  async function wrap(args, receiver) {
    const format = string(args[0]);
    const payload =
      format === "jwk" ? call("exportSerializedJwk", [args[1]]) : exportKey(format, args[1]);
    // Native wrapKey enforces the original wrapping-key usage, including AES-KW.
    // The private HMAC key transports exactly the authorized serialized payload.
    const temporary = raw("importKey", receiver, [
      "raw",
      payload,
      payloadAlgorithm,
      true,
      ["sign"],
    ]);
    return await bridge(raw("wrapKey", receiver, ["raw", temporary, args[2], args[3]]));
  }
  async function unwrap(args, receiver) {
    const format = string(args[0]),
      usages = sequence(args[6]);
    const temporary = raw("unwrapKey", receiver, [
      "raw",
      args[1],
      args[2],
      args[3],
      payloadAlgorithm,
      true,
      ["sign"],
    ]);
    const payload = await bridge(raw("exportKey", receiver, ["raw", temporary]));
    return format === "jwk"
      ? call("importSerializedJwk", [bytes(payload), !!args[5], usages])
      : importKey(format, payload, args[5], usages);
  }
  function hybridAlgorithm(value) {
    return value && typeof value === "object" && value.name === "MLKEM768-X25519";
  }
  function validateInputs(name, args) {
    const position =
      name === "unwrapKey"
        ? 2
        : name === "wrapKey"
          ? 2
          : name === "deriveKey" ||
              name === "deriveBits" ||
              name === "encrypt" ||
              name === "decrypt" ||
              name === "sign" ||
              name === "verify"
            ? 1
            : -1;
    if (position >= 0 && isKey(args[position]))
      throw new DOMError("Key algorithm does not match operation", "InvalidAccessError");
    if ((name === "deriveKey" || name === "deriveBits") && args[0]?.public && isKey(args[0].public))
      throw new DOMError("Public key algorithm does not match operation", "InvalidAccessError");
  }
  function handles(name, args) {
    validateInputs(name, args);
    if (name === "exportKey" || name === "wrapKey") return isKey(args[1]);
    if (name === "getPublicKey") return isKey(args[0]);
    if (name === "importKey") return hybridAlgorithm(args[2]);
    if (name === "unwrapKey") return hybridAlgorithm(args[4]);
    return hybridAlgorithm(args[0]);
  }
  function run(name, args, receiver) {
    if (name === "generateKey")
      return { value: call("generate", [!!args[1], sequence(args[2])]), async: false };
    if (name === "importKey")
      return {
        value: importKey(string(args[0]), args[1], args[3], sequence(args[4])),
        async: false,
      };
    if (name === "exportKey") return { value: exportKey(string(args[0]), args[1]), async: false };
    if (name === "getPublicKey")
      return { value: call("getPublicKey", [args[0], sequence(args[1])]), async: false };
    if (name === "wrapKey") return { value: wrap(args, receiver), async: true };
    if (name === "unwrapKey") return { value: unwrap(args, receiver), async: true };
    get("type", args[1]);
    if (!isKey(args[1]))
      throw new DOMError("Key algorithm does not match X-Wing", "InvalidAccessError");
    if (name === "encapsulateBits")
      return { value: call("encapsulateBits", [args[1], false]), async: false };
    if (name === "decapsulateBits")
      return { value: call("decapsulateBits", [args[1], bytes(args[2]), false]), async: false };
    if (name === "encapsulateKey") {
      const result = call("encapsulateBits", [args[1], true]);
      return {
        value: {
          ciphertext: result.ciphertext,
          sharedKey: raw("importKey", receiver, [
            "raw",
            result.sharedKey,
            args[2],
            !!args[3],
            sequence(args[4]),
          ]),
        },
        async: false,
      };
    }
    if (name === "decapsulateKey") {
      const result = call("decapsulateBits", [args[1], bytes(args[2]), true]);
      return {
        value: raw("importKey", receiver, ["raw", result, args[3], !!args[4], sequence(args[5])]),
        async: false,
      };
    }
    throw new DOMError("X-Wing operation unsupported", "NotSupportedError");
  }
  function supports(operation, algorithm, additional) {
    const name = typeof algorithm === "string" ? algorithm : algorithm?.name;
    if (typeof name === "string" && apply(upper, name, []) === "MLKEM768-X25519")
      return call("supports", [operation]);
    return apply(backend.supports, undefined, [operation, algorithm, additional]);
  }
  return { isKey, get, handles, run, supports };
});
