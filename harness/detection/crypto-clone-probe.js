// Authored local fixture: reports behavior, never private material or factories.
(async () => {
  const key = await crypto.subtle.importKey("raw", new Uint8Array(16), "AES-GCM", false, [
    "encrypt",
  ]);
  const source = { key, again: key, date: new Date(1234), bytes: new Uint8Array([1, 2, 3]) };
  source.self = source;
  source.map = new Map([[key, source]]);
  source.set = new Set([key]);
  const copy = structuredClone(source);
  const error = (callback) => {
    try {
      callback();
      return null;
    } catch (caught) {
      return { name: caught.name, dom: caught instanceof DOMException };
    }
  };
  const graph = {
    distinct: copy.key !== key,
    alias: copy.key === copy.again,
    cycle: copy.self === copy,
    map: copy.map.get(copy.key) === copy,
    set: copy.set.has(copy.key),
    date: copy.date.getTime(),
    bytes: Array.from(copy.bytes),
    key: copy.key instanceof CryptoKey,
    type: copy.key.type,
    extractable: copy.key.extractable,
    algorithm: copy.key.algorithm,
    usages: copy.key.usages,
    ownKeys: Reflect.ownKeys(copy.key).length,
  };
  let hooks = 0;
  const poison = () => {
    hooks++;
    throw new Error("Authored poison hook");
  };
  for (const name of ["algorithm", "extractable", "type", "usages"])
    Object.defineProperty(key, name, { get: poison, configurable: true });
  Object.defineProperty(key, Symbol.for("Deno.core.hostObject"), {
    get: poison,
    configurable: true,
  });
  const protectedCopy = structuredClone(key);
  const protection = {
    hooks,
    extractable: protectedCopy.extractable,
    ownKeys: Reflect.ownKeys(protectedCopy).length,
  };
  const exception = new DOMException("authored", "InvalidStateError");
  const exceptionCopy = structuredClone({ exception, again: exception });
  const platform = {
    exception: {
      brand: exceptionCopy.exception instanceof DOMException,
      distinct: exceptionCopy.exception !== exception,
      alias: exceptionCopy.exception === exceptionCopy.again,
      name: exceptionCopy.exception.name,
      message: exceptionCopy.exception.message,
      code: exceptionCopy.exception.code,
    },
  };
  const mark = performance.mark("authored-crypto-clone", { detail: { key: copy.key, exception } });
  const timing = {
    key: mark.detail.key instanceof CryptoKey,
    distinct: mark.detail.key !== copy.key,
    permission: !mark.detail.key.extractable,
    exception: mark.detail.exception instanceof DOMException,
    exceptionName: mark.detail.exception.name,
    same: mark.detail === mark.detail,
  };
  performance.clearMarks("authored-crypto-clone");
  const frame = document.createElement("iframe");
  document.body.append(frame);
  const child = frame.contentWindow;
  const childCopy = child.structuredClone(copy),
    borrowed = structuredClone.call(child, copy.key);
  const realms = {
    child: childCopy.key instanceof child.CryptoKey && !(childCopy.key instanceof CryptoKey),
    object: Object.getPrototypeOf(childCopy) === child.Object.prototype,
    date: childCopy.date instanceof child.Date,
    borrowed: borrowed instanceof child.CryptoKey && !(borrowed instanceof CryptoKey),
    parentReceiver: child.structuredClone.call(window, copy.key) instanceof CryptoKey,
    invalidReceiver: error(() => structuredClone.call({}, copy.key)),
    nullReceiver: error(() => structuredClone.call(null, copy.key)),
    original: Object.getPrototypeOf(copy.key) === CryptoKey.prototype,
    exception: child.structuredClone(exception) instanceof child.DOMException,
  };
  const received = new Promise((resolve) =>
    child.addEventListener(
      "message",
      (event) => {
        const data = event.data;
        resolve({
          key: data.key instanceof child.CryptoKey && !(data.key instanceof CryptoKey),
          alias: data.key === data.again,
          distinct: data.key !== copy.key,
          permission: !data.key.extractable,
          source: event.source === window,
          origin: event.origin === location.origin,
        });
      },
      { once: true },
    ),
  );
  const sentinel = new TypeError("Authored message getter");
  let mismatchedGetter = false;
  try {
    child.postMessage(
      {
        get value() {
          throw sentinel;
        },
      },
      "https://different.test",
    );
  } catch (caught) {
    mismatchedGetter = caught === sentinel;
  }
  let invalidOrigin;
  try {
    child.postMessage(copy.key, "invalid-target-origin");
  } catch (caught) {
    invalidOrigin = {
      name: caught.name,
      dom: caught instanceof DOMException,
      childDom: caught instanceof child.DOMException,
      syntax: caught instanceof SyntaxError,
      childSyntax: caught instanceof child.SyntaxError,
    };
  }
  child.postMessage(
    { key: copy.key, again: copy.key },
    location.origin + "/authored/path?query=fixture",
  );
  const message = { delivery: await received, mismatchedGetter, invalidOrigin };
  frame.remove();
  const fake = structuredClone(Object.create(CryptoKey.prototype));
  const rejection = {
    function: error(() => structuredClone(() => {})),
    proxy: error(() => structuredClone(new Proxy({}, {}))),
    node: error(() => structuredClone(document.body)),
    crypto: error(() => structuredClone(crypto)),
    subtle: error(() => structuredClone(crypto.subtle)),
    event: error(() => structuredClone(new Event("authored"))),
    xpathEvaluator: error(() => structuredClone(new XPathEvaluator())),
    xpathExpression: error(() => structuredClone(document.createExpression("count(//*)"))),
    xpathResult: error(() =>
      structuredClone(document.evaluate("count(//*)", document, null, XPathResult.NUMBER_TYPE)),
    ),
    tokens: error(() => structuredClone(document.body.classList)),
    emptyList: error(() => structuredClone(document.querySelectorAll("authored-missing"))),
    url: error(() => structuredClone(new URL("https://fixture.test/"))),
    search: error(() => structuredClone(new URLSearchParams("a=1"))),
    fakePlain: Object.getPrototypeOf(fake) === Object.prototype && !(fake instanceof CryptoKey),
  };
  const bytes = new Uint8Array([1, 2, 3]),
    transferred = structuredClone({ bytes, buffer: bytes.buffer }, { transfer: [bytes.buffer] });
  const unused = new ArrayBuffer(2);
  structuredClone(null, { transfer: [unused] });
  const failed = new ArrayBuffer(2);
  const duplicate = error(() => structuredClone(null, { transfer: [failed, failed] }));
  const uncloneable = error(() => structuredClone({ failed, fn() {} }, { transfer: [failed] }));
  const detached = error(() => structuredClone(null, { transfer: [bytes.buffer] }));
  const invalid = error(() => structuredClone(null, { transfer: [{}] }));
  const resizable = new ArrayBuffer(2, { maxByteLength: 8 });
  new Uint8Array(resizable)[0] = 7;
  const resized = structuredClone(resizable, { transfer: [resizable] });
  resized.resize(4);
  const transfers = {
    length: bytes.length,
    sourceLength: bytes.buffer.byteLength,
    bytes: Array.from(transferred.bytes),
    alias: transferred.bytes.buffer === transferred.buffer,
    unusedLength: unused.byteLength,
    failureAttached: failed.byteLength === 2,
    duplicate,
    uncloneable,
    detached,
    invalid,
    resizable: resized.resizable,
    maximum: resized.maxByteLength,
    resized: resized.byteLength,
    first: new Uint8Array(resized)[0],
    dictionaries: [undefined, null, () => {}, 0, "", true, Symbol("authored")].map((options) =>
      error(() => structuredClone(1, options)),
    ),
    sequences: [null, 0, "", {}, [], [1], [{}]].map((transfer) =>
      error(() => structuredClone(1, { transfer })),
    ),
  };
  let transferReads = 0,
    iteratorReads = 0,
    nextCalls = 0,
    closes = 0;
  const sequence = {
    get [Symbol.iterator]() {
      iteratorReads++;
      return function () {
        return {
          next() {
            nextCalls++;
            return { done: false, value: 1 };
          },
          return() {
            closes++;
            return { done: true };
          },
        };
      };
    },
  };
  transfers.iterator = {
    error: error(() =>
      structuredClone(null, {
        get transfer() {
          transferReads++;
          return sequence;
        },
      }),
    ),
    transferReads,
    iteratorReads,
    nextCalls,
    closes,
  };
  const input = new Uint8Array([7]),
    list = [input.buffer],
    helpers = { from: Array.from, push: Array.prototype.push, includes: Array.prototype.includes };
  let helperHooks = 0,
    helperCopy,
    helperError;
  const intercepted = () => {
    helperHooks++;
    throw new Error("guest array helper");
  };
  Object.defineProperty(Array.prototype, "0", {
    configurable: true,
    set() {
      helperHooks++;
    },
  });
  Array.from = intercepted;
  Array.prototype.push = intercepted;
  Array.prototype.includes = intercepted;
  try {
    helperCopy = structuredClone(input, { transfer: list });
  } catch (error) {
    helperError = error.name;
  } finally {
    Array.from = helpers.from;
    Array.prototype.push = helpers.push;
    Array.prototype.includes = helpers.includes;
    delete Array.prototype[0];
  }
  transfers.helperHooks = {
    calls: helperHooks,
    error: helperError ?? null,
    source: input.length,
    result: helperCopy?.[0],
  };
  return JSON.stringify({
    graph,
    protection,
    platform,
    timing,
    realms,
    rejection,
    message,
    transfers,
  });
})();
