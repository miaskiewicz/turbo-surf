// Chrome 155's registered algorithms and Web-IDL dictionaries. The profile
// normalizes inputs; the official native backend performs every primitive.
(function (nativeSupports, keyType, keyAlgorithm, apply, DOMError) {
  "use strict";
  const create = Object.create,
    define = Object.defineProperty;
  const StringValue = String,
    TypeErrorValue = TypeError;
  const upper = String.prototype.toUpperCase,
    split = String.prototype.split,
    slice = String.prototype.slice;
  const own = Object.hasOwn;
  const finite = Number.isFinite,
    trunc = Math.trunc;
  const descriptor = Object.getOwnPropertyDescriptor;
  const errorMessage = descriptor(DOMError.prototype, "message").get;
  const arrayBufferLength = descriptor(ArrayBuffer.prototype, "byteLength").get;
  const typedPrototype = Object.getPrototypeOf(Uint8Array.prototype);
  const typedLength = descriptor(typedPrototype, "byteLength").get;
  const typedBuffer = descriptor(typedPrototype, "buffer").get;
  const typedName = descriptor(typedPrototype, Symbol.toStringTag).get;
  const viewLength = descriptor(DataView.prototype, "byteLength").get;
  const viewBuffer = descriptor(DataView.prototype, "buffer").get;
  const registry = create(null);
  function register(names, operations) {
    for (const name of names) registry[apply(upper, name, [])] = { name, operations };
  }
  const aes = { generateKey: "length:u16", importKey: "", "get key length": "length:u16" };
  register(["AES-CBC"], { ...aes, encrypt: "iv:buffer", decrypt: "iv:buffer" });
  register(["AES-CTR"], {
    ...aes,
    encrypt: "counter:buffer length:u8",
    decrypt: "counter:buffer length:u8",
  });
  register(["AES-GCM"], {
    ...aes,
    encrypt: "iv:buffer additionalData:buffer? tagLength:u8?",
    decrypt: "iv:buffer additionalData:buffer? tagLength:u8?",
  });
  register(["AES-KW"], { ...aes, wrapKey: "", unwrapKey: "" });
  register(["HMAC"], {
    generateKey: "hash:hash length:u32?",
    importKey: "hash:hash length:u32?",
    "get key length": "hash:hash length:u32?",
    sign: "",
    verify: "",
  });
  const rsa = {
    generateKey: "modulusLength:u32 publicExponent:uint8 hash:hash",
    importKey: "hash:hash",
    getPublicKey: "",
  };
  register(["RSASSA-PKCS1-v1_5"], { ...rsa, sign: "", verify: "" });
  register(["RSA-PSS"], { ...rsa, sign: "saltLength:u32", verify: "saltLength:u32" });
  register(["RSA-OAEP"], { ...rsa, encrypt: "label:buffer?", decrypt: "label:buffer?" });
  const ec = { generateKey: "namedCurve:string", importKey: "namedCurve:string", getPublicKey: "" };
  register(["ECDSA"], { ...ec, sign: "hash:hash", verify: "hash:hash" });
  register(["ECDH"], { ...ec, deriveBits: "public:key" });
  register(["Ed25519"], { generateKey: "", importKey: "", sign: "", verify: "", getPublicKey: "" });
  register(["X25519"], {
    generateKey: "",
    importKey: "",
    deriveBits: "public:key",
    getPublicKey: "",
  });
  register(["HKDF"], { importKey: "", deriveBits: "hash:hash salt:buffer info:buffer" });
  register(["PBKDF2"], { importKey: "", deriveBits: "hash:hash salt:buffer iterations:u32" });
  register(["SHA-1", "SHA-256", "SHA-384", "SHA-512"], { digest: "" });
  register(["ML-KEM-768", "ML-KEM-1024", "MLKEM768-X25519"], {
    generateKey: "",
    importKey: "",
    encapsulate: "",
    decapsulate: "",
    getPublicKey: "",
  });
  register(["ML-DSA-44", "ML-DSA-65", "ML-DSA-87"], {
    generateKey: "",
    importKey: "",
    sign: "context:buffer?",
    verify: "context:buffer?",
    getPublicKey: "",
  });
  register(["ChaCha20-Poly1305"], {
    generateKey: "",
    importKey: "",
    "get key length": "",
    encrypt: "iv:buffer additionalData:buffer? tagLength:u8?",
    decrypt: "iv:buffer additionalData:buffer? tagLength:u8?",
  });
  function string(value) {
    if (typeof value === "symbol") throw new TypeErrorValue("Cannot convert a Symbol to a string");
    return StringValue(value);
  }
  function information(value) {
    const object = value !== null && (typeof value === "object" || typeof value === "function");
    const name = object ? value.name : value;
    if (object && name === undefined) throw new TypeErrorValue("Algorithm name is required");
    const info = registry[apply(upper, string(name), [])];
    if (!info) throw new DOMError("Unrecognized algorithm", "NotSupportedError");
    return { info, source: object ? value : create(null) };
  }
  function bufferLength(value) {
    try {
      return apply(arrayBufferLength, value, []);
    } catch {}
    try {
      apply(arrayBufferLength, apply(typedBuffer, value, []), []);
      return apply(typedLength, value, []);
    } catch {}
    apply(arrayBufferLength, apply(viewBuffer, value, []), []);
    return apply(viewLength, value, []);
  }
  function convert(value, type) {
    if (type === "string") return string(value);
    if (type === "hash") return normalize(value, "digest");
    if (type === "buffer") {
      bufferLength(value);
      return value;
    }
    if (type === "uint8") {
      if (apply(typedName, value, []) !== "Uint8Array")
        throw new TypeErrorValue("Uint8Array required");
      return value;
    }
    if (type === "key") {
      apply(keyType, value, []);
      return value;
    }
    const number = +value,
      integer = trunc(number),
      maximum = type === "u8" ? 255 : type === "u16" ? 65535 : 4294967295;
    if (!finite(number) || integer < 0 || integer > maximum)
      throw new TypeErrorValue("Integer outside Web-IDL range");
    return integer;
  }
  function mappedOperation(operation) {
    if (operation === "deriveKey") return "deriveBits";
    if (operation === "encapsulateKey" || operation === "encapsulateBits") return "encapsulate";
    if (operation === "decapsulateKey" || operation === "decapsulateBits") return "decapsulate";
    return operation;
  }
  function normalize(value, operation) {
    const { info, source } = information(value);
    const mapped = mappedOperation(operation);
    let schema = own(info.operations, mapped) ? info.operations[mapped] : undefined;
    if (schema === undefined && (operation === "wrapKey" || operation === "unwrapKey"))
      schema = info.operations[operation === "wrapKey" ? "encrypt" : "decrypt"];
    if (schema === undefined)
      throw new DOMError("Algorithm does not support operation", "NotSupportedError");
    const result = { name: info.name };
    if (!schema) return result;
    const fields = apply(split, schema, [" "]);
    for (let index = 0; index < fields.length; index++) {
      const field = fields[index];
      const parts = apply(split, field, [":"]),
        name = parts[0],
        declared = parts[1];
      const optional = declared[declared.length - 1] === "?";
      const type = optional ? apply(slice, declared, [0, -1]) : declared;
      const value = source[name];
      if (value === undefined) {
        if (optional) continue;
        throw new TypeErrorValue("Algorithm " + name + " is required");
      }
      define(result, name, {
        value: convert(value, type),
        enumerable: true,
        configurable: true,
        writable: true,
      });
    }
    return result;
  }
  const positions = {
    digest: [[0, "digest"]],
    encrypt: [[0, "encrypt"]],
    decrypt: [[0, "decrypt"]],
    sign: [[0, "sign"]],
    verify: [[0, "verify"]],
    generateKey: [[0, "generateKey"]],
    importKey: [[2, "importKey"]],
    deriveBits: [[0, "deriveBits"]],
    deriveKey: [
      [0, "deriveBits"],
      [2, "get key length"],
    ],
    wrapKey: [[3, "wrapKey"]],
    unwrapKey: [
      [3, "unwrapKey"],
      [4, "importKey"],
    ],
    encapsulateBits: [[0, "encapsulate"]],
    decapsulateBits: [[0, "decapsulate"]],
    encapsulateKey: [
      [0, "encapsulate"],
      [2, "importKey"],
    ],
    decapsulateKey: [
      [0, "decapsulate"],
      [3, "importKey"],
    ],
  };
  function admitInner(operation, args) {
    if (operation === "exportKey" || operation === "getPublicKey") {
      const key = args[operation === "exportKey" ? 1 : 0];
      apply(keyType, key, []);
      const { info } = information(apply(keyAlgorithm, key, []));
      if (operation === "getPublicKey" && info.operations.getPublicKey === undefined)
        throw new DOMError("Public key cannot be derived", "InvalidAccessError");
    }
    const targets = positions[operation] || [];
    for (let index = 0; index < targets.length; index++) {
      const target = targets[index];
      args[target[0]] = normalize(args[target[0]], target[1]);
    }
    return args;
  }
  function admit(operation, args) {
    try {
      return admitInner(operation, args);
    } catch (error) {
      let unknown = false;
      try {
        unknown = apply(errorMessage, error, []) === "Unrecognized algorithm";
      } catch {}
      if (unknown)
        throw new DOMError(
          "Failed to execute '" + operation + "' on 'SubtleCrypto': Algorithm: Unrecognized name",
          "NotSupportedError",
        );
      throw error;
    }
  }
  function supports(operation, algorithm, additional) {
    // Required argument conversion belongs to the public Web-IDL callback.
    operation = string(operation);
    try {
      const normalized = normalize(algorithm, operation);
      if (additional !== undefined && additional !== null && typeof additional !== "number") {
        if (operation === "wrapKey") return false;
        if (operation === "deriveKey") additional = normalize(additional, "get key length");
        else if (
          operation === "unwrapKey" ||
          operation === "encapsulateKey" ||
          operation === "decapsulateKey"
        )
          additional = normalize(additional, "importKey");
      }
      if (
        (normalized.name === "ML-DSA-44" ||
          normalized.name === "ML-DSA-65" ||
          normalized.name === "ML-DSA-87") &&
        normalized.context !== undefined &&
        bufferLength(normalized.context) > 255
      )
        return false;
      return apply(nativeSupports, undefined, [operation, normalized, additional]);
    } catch {
      return false;
    }
  }
  const order = [
    "encrypt",
    "decrypt",
    "sign",
    "verify",
    "deriveKey",
    "deriveBits",
    "wrapKey",
    "unwrapKey",
    "encapsulateKey",
    "encapsulateBits",
    "decapsulateKey",
    "decapsulateBits",
  ];
  function usages(values) {
    const result = [];
    for (let orderIndex = 0; orderIndex < order.length; orderIndex++)
      for (let index = 0; index < values.length; index++)
        if (values[index] === order[orderIndex]) {
          define(result, StringValue(result.length), {
            value: order[orderIndex],
            writable: true,
            enumerable: true,
            configurable: true,
          });
          break;
        }
    return result;
  }
  return { admit, supports, usages };
});
