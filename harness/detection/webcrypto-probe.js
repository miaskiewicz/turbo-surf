(async () => {
  const hex = (value) =>
    Array.from(new Uint8Array(value), (v) => v.toString(16).padStart(2, "0")).join("");
  const safe = async (fn) => {
    try {
      const value = fn();
      const promise = value instanceof Promise;
      return { promise, value: await value };
    } catch (error) {
      return { error: error.name };
    }
  };
  const supports = {};
  for (const operation of [
    "digest",
    "generateKey",
    "importKey",
    "exportKey",
    "sign",
    "verify",
    "encrypt",
    "decrypt",
    "deriveBits",
    "deriveKey",
    "getPublicKey",
    "encapsulateBits",
    "decapsulateBits",
    "encapsulateKey",
    "decapsulateKey",
  ]) {
    supports[operation] = {};
    for (const algorithm of [
      "SHA-256",
      "SHA-384",
      "SHA-512",
      "SHA-1",
      "AES-GCM",
      "HMAC",
      "Ed25519",
      "X25519",
      "RSA-PSS",
      "ECDSA",
      "ML-KEM-512",
      "ML-KEM-768",
      "ML-KEM-1024",
      "MLKEM768-X25519",
      "ML-DSA-44",
      "ML-DSA-65",
      "ML-DSA-87",
      "ChaCha20-Poly1305",
      "unknown",
    ])
      supports[operation][algorithm] = await safe(() =>
        SubtleCrypto.supports(operation, algorithm),
      );
  }
  const shape = {};
  for (const name of [
    "getPublicKey",
    "encapsulateBits",
    "decapsulateBits",
    "encapsulateKey",
    "decapsulateKey",
  ])
    shape[name] = {
      length: SubtleCrypto.prototype[name].length,
      invalid: await safe(() => SubtleCrypto.prototype[name].call({})),
      missing: await safe(() => crypto.subtle[name]()),
    };
  const parameters = {};
  const cases = [
    ["aesGenerate", "generateKey", { name: "AES-GCM", length: 128 }],
    ["aesBadLength", "generateKey", { name: "AES-GCM", length: 7 }],
    ["aesEncrypt", "encrypt", { name: "AES-GCM", iv: new Uint8Array(12) }],
    ["aesBadTag", "encrypt", { name: "AES-GCM", iv: new Uint8Array(12), tagLength: 17 }],
    ["hmacGenerate", "generateKey", { name: "HMAC", hash: "SHA-256" }],
    [
      "rsaGenerate",
      "generateKey",
      {
        name: "RSA-PSS",
        modulusLength: 2048,
        publicExponent: new Uint8Array([1, 0, 1]),
        hash: "SHA-256",
      },
    ],
    ["rsaSign", "sign", { name: "RSA-PSS", saltLength: 32 }],
    ["ecGenerate", "generateKey", { name: "ECDSA", namedCurve: "P-256" }],
    ["ecSign", "sign", { name: "ECDSA", hash: "SHA-256" }],
    ["kemTarget", "encapsulateKey", "ML-KEM-768", { name: "AES-GCM", length: 256 }],
    ["mlDsa", "generateKey", "ML-DSA-44"],
    ["hybridKem", "generateKey", "MLKEM768-X25519"],
    ["chacha", "generateKey", "ChaCha20-Poly1305"],
    ["backendOnly", "digest", "SHA3-256"],
  ];
  for (const [label, operation, algorithm, additional] of cases)
    parameters[label] = await safe(() => SubtleCrypto.supports(operation, algorithm, additional));
  const pair = await crypto.subtle.generateKey("Ed25519", false, ["sign", "verify"]);
  const keyInfo = (key) => ({
    tag: Object.prototype.toString.call(key),
    type: key.type,
    extractable: key.extractable,
    algorithm: key.algorithm,
    usages: key.usages,
  });
  const publicCall = crypto.subtle.getPublicKey(pair.privateKey, ["verify"]);
  const publicKey = await publicCall;
  const message = new TextEncoder().encode("native cryptography parity"),
    signature = await crypto.subtle.sign("Ed25519", pair.privateKey, message);
  const ed = {
    publicPromise: publicCall instanceof Promise,
    privateKey: keyInfo(pair.privateKey),
    publicKey: keyInfo(publicKey),
    verify: await crypto.subtle.verify("Ed25519", publicKey, signature, message),
    samePublic:
      hex(await crypto.subtle.exportKey("raw", publicKey)) ===
      hex(await crypto.subtle.exportKey("raw", pair.publicKey)),
    privateExport: await safe(() => crypto.subtle.exportKey("pkcs8", pair.privateKey)),
    publicArgument: await safe(() => crypto.subtle.getPublicKey(pair.publicKey, [])),
  };
  const kem = {};
  for (const name of ["ML-KEM-512", "ML-KEM-768", "ML-KEM-1024", "MLKEM768-X25519"]) {
    try {
      const keys = await crypto.subtle.generateKey(name, false, [
        "encapsulateBits",
        "decapsulateBits",
        "encapsulateKey",
        "decapsulateKey",
      ]);
      const bits = await crypto.subtle.encapsulateBits(name, keys.publicKey);
      const recovered = await crypto.subtle.decapsulateBits(name, keys.privateKey, bits.ciphertext);
      const wrappedCall = crypto.subtle.encapsulateKey(
        name,
        keys.publicKey,
        { name: "AES-GCM", length: 256 },
        true,
        ["encrypt", "decrypt"],
      );
      const wrapped = await wrappedCall;
      const secretCall = crypto.subtle.decapsulateKey(
        name,
        keys.privateKey,
        wrapped.ciphertext,
        { name: "AES-GCM", length: 256 },
        true,
        ["encrypt", "decrypt"],
      );
      const secret = await secretCall;
      kem[name] = {
        privateKey: keyInfo(keys.privateKey),
        publicKey: keyInfo(keys.publicKey),
        bitsKeys: Object.keys(bits).sort(),
        ciphertextBytes: bits.ciphertext.byteLength,
        secretBytes: bits.sharedKey.byteLength,
        bitsMatch: hex(bits.sharedKey) === hex(recovered),
        keyPromise: wrappedCall instanceof Promise,
        decapsulatePromise: secretCall instanceof Promise,
        keyKeys: Object.keys(wrapped).sort(),
        secretKey: keyInfo(secret),
        keyMatch:
          hex(await crypto.subtle.exportKey("raw", wrapped.sharedKey)) ===
          hex(await crypto.subtle.exportKey("raw", secret)),
      };
    } catch (error) {
      kem[name] = { error: error.name, message: error.message };
    }
  }
  const digest = {};
  for (const name of ["SHA-1", "SHA-256", "SHA-384", "SHA-512"])
    digest[name] = hex(await crypto.subtle.digest(name, new TextEncoder().encode("abc")));
  return JSON.stringify({ supports, parameters, shape, ed, kem, digest });
})();
