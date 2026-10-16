(() => {
  const controller = new AbortController(),
    front = controller.signal;
  const from = Array.from,
    iterator = Array.prototype[Symbol.iterator],
    zero = Object.getOwnPropertyDescriptor(Array.prototype, "0");
  const apply = Reflect.apply,
    define = Object.defineProperty,
    tag = Object.prototype.toString;
  const aborted = Object.getOwnPropertyDescriptor(AbortSignal.prototype, "aborted").get;
  const raw = (value) => {
    if (
      value === null ||
      typeof value !== "object" ||
      apply(tag, value, []) !== "[object AbortSignal]"
    )
      return false;
    try {
      apply(aborted, value, []);
      return false;
    } catch {
      return true;
    }
  };
  let exposed = false,
    fromCalls = 0;
  try {
    Array.from = function (input, mapper) {
      fromCalls++;
      if (typeof mapper === "function") exposed = raw(mapper(front)) || exposed;
      return apply(from, this, [input, mapper]);
    };
    AbortSignal.any([front]);
  } finally {
    Array.from = from;
  }
  const arrayFrom = { calls: fromCalls, rawExposed: exposed };
  exposed = false;
  let iterations = 0;
  try {
    Array.prototype[Symbol.iterator] = function () {
      iterations++;
      const source = apply(iterator, this, []);
      return {
        next() {
          const result = source.next();
          if (!result.done) exposed = raw(result.value) || exposed;
          return result;
        },
      };
    };
    AbortSignal.any([front]);
  } finally {
    Array.prototype[Symbol.iterator] = iterator;
  }
  const arrayIterator = { calls: iterations, rawExposed: exposed };
  exposed = false;
  try {
    define(Array.prototype, "0", {
      configurable: true,
      set(value) {
        exposed = raw(value) || exposed;
        define(this, "0", { value, writable: true, configurable: true, enumerable: true });
      },
    });
    AbortSignal.any([front]);
  } finally {
    if (zero) define(Array.prototype, "0", zero);
    else delete Array.prototype[0];
  }
  const numericSetter = { rawExposed: exposed };
  let descriptorExposed = false,
    abortSucceeded = false;
  const descriptorGet = Object.getOwnPropertyDescriptor(Object.prototype, "get");
  try {
    define(Object.prototype, "get", {
      configurable: true,
      get() {
        const value = this.value;
        if (value && typeof value === "object" && Object.hasOwn(value, "raw"))
          descriptorExposed = true;
        return undefined;
      },
    });
    abortSucceeded = AbortSignal.abort("reason").aborted;
  } finally {
    if (descriptorGet) define(Object.prototype, "get", descriptorGet);
    else delete Object.prototype.get;
  }
  const descriptorAttack = { rawExposed: descriptorExposed, abortSucceeded };
  const conversions = {};
  for (const [name, input] of [
    ["emptyString", ""],
    ["null", null],
    ["number", 1],
    ["emptyArray", []],
    ["invalidArray", [{}]],
  ]) {
    try {
      const result = AbortSignal.any(input);
      conversions[name] = result.aborted;
    } catch (error) {
      conversions[name] = error.name;
    }
  }
  const sequence = [];
  const bad = {
    [Symbol.iterator]() {
      sequence.push("iterator");
      return {
        next() {
          sequence.push("next");
          return { done: false, value: {} };
        },
        return() {
          sequence.push("return");
          return { done: true };
        },
      };
    },
  };
  try {
    AbortSignal.any(bad);
  } catch (error) {
    sequence.push(error.name);
  }
  const reason = {};
  const first = AbortSignal.abort(reason),
    second = AbortSignal.abort("second");
  const combined = AbortSignal.any([first, second]);
  const live = new AbortController(),
    events = [],
    dependent = AbortSignal.any([live.signal]);
  live.signal.addEventListener("abort", () => events.push("source"));
  dependent.addEventListener("abort", () => events.push("dependent"));
  live.abort(reason);
  return {
    arrayFrom,
    arrayIterator,
    numericSetter,
    descriptorAttack,
    conversions,
    sequence,
    firstReason: combined.reason === reason,
    propagation: { aborted: dependent.aborted, reason: dependent.reason === reason, events },
  };
})();
