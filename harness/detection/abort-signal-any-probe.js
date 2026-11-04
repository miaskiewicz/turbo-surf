(async () => {
  // AbortSignal.any() sequence conversion (Blink's JS-array fast path versus the
  // iterator protocol) and AbortSignal.timeout() argument conversion, reason and
  // event timing.
  const out = {};
  const signal = new AbortController().signal;
  const attempt = (name, callback) => {
    try {
      out[name] = ["ok", callback()];
    } catch (error) {
      out[name] = [error.constructor.name, error.message];
    }
  };
  attempt("ownIterator", () => {
    let calls = 0;
    const input = [signal];
    input[Symbol.iterator] = function () {
      calls++;
      return [][Symbol.iterator].call([signal, signal]);
    };
    AbortSignal.any(input);
    return calls;
  });
  attempt("protoIterator", () => {
    let calls = 0;
    const original = Array.prototype[Symbol.iterator];
    Array.prototype[Symbol.iterator] = function () {
      calls++;
      return original.call(this);
    };
    try {
      AbortSignal.any([signal]);
    } finally {
      Array.prototype[Symbol.iterator] = original;
    }
    return calls;
  });
  attempt("arrayIteratorNext", () => {
    let calls = 0;
    const prototype = Object.getPrototypeOf([][Symbol.iterator]());
    const next = prototype.next;
    prototype.next = function () {
      calls++;
      return next.call(this);
    };
    try {
      AbortSignal.any([signal]);
    } finally {
      prototype.next = next;
    }
    return calls;
  });
  attempt("subclass", () => {
    let calls = 0;
    class Signals extends Array {
      [Symbol.iterator]() {
        calls++;
        return super[Symbol.iterator]();
      }
    }
    AbortSignal.any(Signals.of(signal));
    return calls;
  });
  attempt("holes", () => AbortSignal.any([signal, , signal]).aborted);
  attempt("inheritedIndex", () => {
    let calls = 0;
    Object.defineProperty(Array.prototype, 1, {
      get() {
        calls++;
        return signal;
      },
      configurable: true,
    });
    try {
      AbortSignal.any([signal, , signal]);
    } finally {
      delete Array.prototype[1];
    }
    return calls;
  });
  attempt("ownGetter", () => {
    let calls = 0;
    const input = [signal];
    Object.defineProperty(input, 0, {
      get() {
        calls++;
        return signal;
      },
    });
    AbortSignal.any(input);
    return calls;
  });
  attempt("proxy", () => {
    const keys = [];
    const proxy = new Proxy([signal], {
      get(target, key, receiver) {
        keys.push(typeof key === "symbol" ? key.description : key);
        return Reflect.get(target, key, receiver);
      },
    });
    AbortSignal.any(proxy);
    return keys;
  });
  attempt("set", () => AbortSignal.any(new Set([signal])).aborted);
  attempt("empty", () => AbortSignal.any([]).aborted);
  attempt("abortedInput", () => {
    const reason = AbortSignal.any([signal, AbortSignal.abort("why")]).reason;
    return reason;
  });
  attempt("notSignal", () => AbortSignal.any([1]));
  attempt("notSignalIterable", () => AbortSignal.any(new Set([1])));
  attempt("string", () => AbortSignal.any("ab"));
  attempt("noArgument", () => AbortSignal.any());
  attempt("null", () => AbortSignal.any(null));
  attempt("plainObject", () => AbortSignal.any({}));
  attempt("nonCallableIterator", () => AbortSignal.any({ [Symbol.iterator]: 1 }));
  attempt("iteratorNotObject", () => AbortSignal.any({ [Symbol.iterator]: () => 1 }));
  attempt("resultNotObject", () =>
    AbortSignal.any({ [Symbol.iterator]: () => ({ next: () => 1 }) }),
  );
  for (const [name, value] of [
    ["timeoutNegative", -1],
    ["timeoutNaN", Number.NaN],
    ["timeoutHuge", 2 ** 64],
    ["timeoutString", "5"],
    ["timeoutFraction", 1.9],
    ["timeoutInfinity", Number.POSITIVE_INFINITY],
    ["timeoutBigInt", 1n],
    ["timeoutSymbol", Symbol()],
  ])
    attempt(name, () => AbortSignal.timeout(value).aborted);
  attempt("timeoutNoArgument", () => AbortSignal.timeout().aborted);
  out.timeoutEvents = await new Promise((resolve) => {
    const log = [];
    const timed = AbortSignal.timeout(0);
    const combined = AbortSignal.any([timed]);
    timed.addEventListener("abort", (event) => {
      log.push(["timed", event.isTrusted, event.cancelable, timed.aborted, combined.aborted]);
      queueMicrotask(() => log.push(["microtask"]));
    });
    combined.onabort = () => log.push(["combined", combined.reason === timed.reason]);
    setTimeout(() => {
      const reason = timed.reason;
      log.push([
        "reason",
        Object.prototype.toString.call(reason),
        reason.name,
        reason.message,
        reason.code,
      ]);
      resolve(log);
    }, 50);
  });
  return JSON.stringify(out);
})();
