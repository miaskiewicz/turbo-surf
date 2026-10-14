// Called privately during realm bootstrap. Algorithms, key material and brands
// remain in the official deno_crypto cppgc Rust classes. Public interface
// prototypes belong to this realm; the shared native factories stay private.
(function (core, make, secure, algorithmFactory, hybridFactory) {
  "use strict";
  const ops = core.ops;
  const rawCrypto = ops.Crypto,
    rawSubtle = ops.SubtleCrypto,
    rawKey = ops.CryptoKey;
  const define = Object.defineProperty,
    descriptors = Object.getOwnPropertyDescriptors;
  const create = Object.create,
    setPrototype = Object.setPrototypeOf,
    own = Object.hasOwn;
  const isArray = Array.isArray;
  const entries = Object.entries,
    apply = Reflect.apply;
  const LocalPromise = Promise,
    then = Promise.prototype.then,
    LocalTypeError = TypeError;
  const NativeDOMException = globalThis.DOMException;
  const Uint8 = Uint8Array;
  const bufferPrototype = ArrayBuffer.prototype;
  const get = ops.op_interface_record_get,
    set = ops.op_interface_record_set;
  // Captured on the first bootstrap and never installed on a public constructor.
  let shared = get(rawCrypto);
  if (!shared) {
    shared = create(null);
    shared.kind = "NativeCryptoBackend";
    set(rawCrypto, shared);
  }
  if (!own(shared, "cryptoBackend")) {
    const backend = create(null);
    backend.createCrypto = rawCrypto.create;
    backend.createSubtle = rawSubtle.create;
    backend.crypto = descriptors(rawCrypto.prototype);
    backend.subtle = descriptors(rawSubtle.prototype);
    backend.key = descriptors(rawKey.prototype);
    backend.supports = rawSubtle.supports;
    backend.typeErrorPrototype = Object.getPrototypeOf(core.buildCustomError("TypeError", ""));
    define(shared, "cryptoBackend", { value: backend });
  }
  const backend = shared.cryptoBackend;
  const isPrototypeOf = Object.prototype.isPrototypeOf;
  const domName = Object.getOwnPropertyDescriptor(NativeDOMException.prototype, "name").get;
  const domMessage = Object.getOwnPropertyDescriptor(NativeDOMException.prototype, "message").get;
  function localError(error) {
    if (
      apply(isPrototypeOf, backend.typeErrorPrototype, [error]) &&
      !apply(isPrototypeOf, LocalTypeError.prototype, [error])
    )
      return new LocalTypeError(error.message);
    try {
      const name = apply(domName, error, []),
        message = apply(domMessage, error, []);
      return error instanceof NativeDOMException ? error : new NativeDOMException(message, name);
    } catch {
      return error;
    }
  }
  for (const name of [
    "DataCloneError",
    "DataError",
    "InvalidAccessError",
    "NotSupportedError",
    "OperationError",
    "SyntaxError",
    "QuotaExceededError",
    "TypeMismatchError",
  ]) {
    const nativeName = "DOMException" + name;
    if (core.buildCustomError(nativeName, "") === undefined)
      core.registerErrorBuilder(nativeName, (message) => new NativeDOMException(message, name));
  }
  const hybrid = hybridFactory(ops.NativeHybridKey, backend, apply, NativeDOMException);
  const nativeKeyGet = (name, key) => hybrid.get(name, key);
  const profile = algorithmFactory(
    hybrid.supports,
    function () {
      return nativeKeyGet("type", this);
    },
    function () {
      return nativeKeyGet("algorithm", this);
    },
    apply,
    NativeDOMException,
  );
  function interfaceConstructor(name, source) {
    const constructor = make(
      function () {
        throw new LocalTypeError("Illegal constructor");
      },
      name,
      0,
      true,
    );
    const prototype = constructor.prototype;
    for (const [member, descriptor] of entries(source)) {
      if (member === "constructor") continue;
      if (!secure && name === "Crypto" && (member === "subtle" || member === "randomUUID"))
        continue;
      const copy = { ...descriptor, enumerable: true };
      for (const kind of ["get", "set", "value"]) {
        if (typeof copy[kind] !== "function") continue;
        const native = copy[kind];
        copy[kind] = make(
          function (...args) {
            try {
              return apply(native, this, args);
            } catch (error) {
              throw localError(error);
            }
          },
          kind === "value" ? member : kind + " " + member,
          native.length,
        );
      }
      define(prototype, member, copy);
    }
    define(prototype, Symbol.toStringTag, { value: name, configurable: true });
    define(constructor, "prototype", { writable: false });
    return constructor;
  }
  const Crypto = interfaceConstructor("Crypto", backend.crypto);
  const SubtleCrypto = interfaceConstructor("SubtleCrypto", backend.subtle);
  const CryptoKey = interfaceConstructor("CryptoKey", backend.key);
  define(SubtleCrypto, "supports", {
    value: make(
      function (...args) {
        try {
          if (args.length < 2) throw new LocalTypeError("supports requires 2 arguments");
          return apply(profile.supports, this, args);
        } catch (error) {
          throw localError(error);
        }
      },
      "supports",
      2,
    ),
    writable: true,
    enumerable: true,
    configurable: true,
  });
  function keyState(key) {
    // The native getter proves the cppgc brand before any private record lookup.
    nativeKeyGet("type", key);
    let state = get(key);
    if (!state || !own(state, "cryptoMetadata"))
      throw new LocalTypeError("Native key metadata unavailable");
    return state.cryptoMetadata;
  }
  for (const name of ["type", "extractable"])
    define(CryptoKey.prototype, name, {
      get: make(
        function () {
          try {
            return nativeKeyGet(name, this);
          } catch (error) {
            throw localError(error);
          }
        },
        "get " + name,
        0,
      ),
      enumerable: true,
      configurable: true,
    });
  function copyArray(source) {
    const result = [];
    for (let index = 0; index < source.length; index++)
      define(result, String(index), {
        value: source[index],
        writable: true,
        enumerable: true,
        configurable: true,
      });
    return result;
  }
  function dictionary(source) {
    const result = {};
    for (const [name, value] of entries(source)) {
      const valueCopy =
        name === "publicExponent"
          ? new Uint8(value)
          : isArray(value)
            ? copyArray(value)
            : value && typeof value === "object"
              ? dictionary(value)
              : value;
      define(result, name, {
        value: valueCopy,
        writable: true,
        enumerable: true,
        configurable: true,
      });
    }
    return result;
  }
  for (const name of ["algorithm", "usages"])
    define(CryptoKey.prototype, name, {
      get: make(
        function () {
          try {
            const metadata = keyState(this);
            return name === "algorithm"
              ? dictionary(metadata.algorithm)
              : copyArray(metadata.usages);
          } catch (error) {
            throw localError(error);
          }
        },
        "get " + name,
        0,
      ),
      enumerable: true,
      configurable: true,
    });
  function ownKey(key) {
    nativeKeyGet("type", key);
    const state = get(key) || create(null);
    if (!own(state, "cryptoMetadata")) {
      const metadata = create(null);
      metadata.algorithm = dictionary(nativeKeyGet("algorithm", key));
      metadata.usages = profile.usages(nativeKeyGet("usages", key));
      setPrototype(metadata.usages, null);
      define(state, "cryptoMetadata", { value: metadata });
      set(key, state);
      // Only freshly-created native result keys are adapted. A caller's input or
      // a foreign key passed to sign/export/derive is never changed.
      setPrototype(key, CryptoKey.prototype);
    }
    return key;
  }
  function ownBuffer(value) {
    setPrototype(value, bufferPrototype);
    return value;
  }
  ops.op_install_crypto_clone_adapter(
    ownKey,
    (message, name = "DataCloneError") => new NativeDOMException(message, name),
  );
  function resultKey(name, value) {
    if (name === "generateKey") {
      if (value && own(value, "publicKey"))
        return { publicKey: ownKey(value.publicKey), privateKey: ownKey(value.privateKey) };
      return ownKey(value);
    }
    if (name === "encapsulateKey")
      return { ciphertext: ownBuffer(value.ciphertext), sharedKey: ownKey(value.sharedKey) };
    if (
      name === "importKey" ||
      name === "getPublicKey" ||
      name === "deriveKey" ||
      name === "unwrapKey" ||
      name === "decapsulateKey"
    )
      return ownKey(value);
    if (name === "encapsulateBits")
      return { ciphertext: ownBuffer(value.ciphertext), sharedKey: ownBuffer(value.sharedKey) };
    if (name === "exportKey") {
      if (typeof value === "object" && own(value, "kty")) return dictionary(value);
      return ownBuffer(value);
    }
    if (name === "verify") return value;
    return ownBuffer(value);
  }
  const arities = {
    digest: 2,
    encrypt: 3,
    decrypt: 3,
    sign: 3,
    verify: 4,
    deriveBits: 2,
    deriveKey: 5,
    generateKey: 3,
    importKey: 5,
    exportKey: 2,
    wrapKey: 4,
    unwrapKey: 7,
    getPublicKey: 2,
    encapsulateBits: 2,
    decapsulateBits: 3,
    encapsulateKey: 5,
    decapsulateKey: 6,
  };
  for (const [name, arity] of entries(arities)) {
    const native = backend.subtle[name]?.value;
    if (typeof native !== "function") throw new Error("Native WebCrypto method missing: " + name);
    define(SubtleCrypto.prototype, name, {
      value: make(
        function (...args) {
          const receiver = this;
          const record =
            receiver !== null && (typeof receiver === "object" || typeof receiver === "function")
              ? get(receiver)
              : undefined;
          const owner = record?.cryptoOwner || {
            promise: LocalPromise,
            error: localError,
            result: resultKey,
          };
          return new owner.promise((resolve, reject) => {
            try {
              if (!record || !own(record, "cryptoOwner"))
                throw new LocalTypeError("Illegal invocation");
              if (args.length < arity)
                throw new LocalTypeError(name + " requires " + arity + " arguments");
              const normalized = profile.admit(name, args);
              const hybridResult = hybrid.handles(name, normalized)
                ? hybrid.run(name, normalized, receiver)
                : undefined;
              const value = hybridResult ? hybridResult.value : apply(native, receiver, normalized);
              const complete = (value) => {
                try {
                  resolve(owner.result(name, value));
                } catch (error) {
                  reject(owner.error(error));
                }
              };
              // Borrow the captured native then; guest parent Promise prototypes must
              // not substitute a foreign input key as a native operation result.
              if (
                hybridResult
                  ? !hybridResult.async
                  : name === "importKey" ||
                    name === "getPublicKey" ||
                    name === "unwrapKey" ||
                    name === "encapsulateKey" ||
                    name === "decapsulateKey"
              )
                complete(value);
              else apply(then, value, [complete, (error) => reject(owner.error(error))]);
            } catch (error) {
              reject(owner.error(error));
            }
          });
        },
        name,
        arity,
      ),
      writable: true,
      enumerable: true,
      configurable: true,
    });
  }
  const subtle = apply(backend.createSubtle, rawSubtle, []);
  const crypto = apply(backend.createCrypto, rawCrypto, [subtle]);
  const subtleRecord = get(subtle) || create(null);
  define(subtleRecord, "cryptoOwner", {
    value: { promise: LocalPromise, error: localError, result: resultKey },
  });
  set(subtle, subtleRecord);
  const windowRecord = get(globalThis) || create(null);
  define(windowRecord, "cryptoWindow", { value: crypto, configurable: true });
  set(globalThis, windowRecord);
  setPrototype(subtle, SubtleCrypto.prototype);
  setPrototype(crypto, Crypto.prototype);
  define(globalThis, "Crypto", { value: Crypto, writable: true, configurable: true });
  if (secure) {
    define(globalThis, "SubtleCrypto", { value: SubtleCrypto, writable: true, configurable: true });
    define(globalThis, "CryptoKey", { value: CryptoKey, writable: true, configurable: true });
  } else {
    delete globalThis.SubtleCrypto;
    delete globalThis.CryptoKey;
  }
  delete globalThis.__bootstrap;
  define(globalThis, "crypto", {
    get: make(
      function () {
        if (this == null) return crypto;
        if (typeof this !== "object" && typeof this !== "function")
          throw new LocalTypeError("Illegal invocation");
        const record = get(this);
        if (!record || !own(record, "cryptoWindow")) throw new LocalTypeError("Illegal invocation");
        return record.cryptoWindow;
      },
      "get crypto",
      0,
    ),
    configurable: true,
    enumerable: true,
  });
});
