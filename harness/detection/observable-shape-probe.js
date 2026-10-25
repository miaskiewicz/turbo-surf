(async () => {
  // Web IDL shape, brand checks and argument conversion of Observable, Subscriber
  // and EventTarget.prototype.when, recorded without timing-dependent work.
  const out = {};
  const tag = (value) => Object.prototype.toString.call(value);
  const shape = (object) => {
    if (!object) return null;
    const result = {};
    for (const key of Reflect.ownKeys(object)) {
      const d = Object.getOwnPropertyDescriptor(object, key);
      const name = typeof key === "symbol" ? key.toString() : key;
      const fn = (f) =>
        f
          ? [
              f.name,
              f.length,
              Object.hasOwn(f, "prototype"),
              Function.prototype.toString.call(f).endsWith("{ [native code] }"),
            ]
          : null;
      result[name] =
        d.get || d.set
          ? {
              get: fn(d.get),
              set: fn(d.set),
              enumerable: d.enumerable,
              configurable: d.configurable,
            }
          : typeof d.value === "function"
            ? {
                value: fn(d.value),
                enumerable: d.enumerable,
                configurable: d.configurable,
                writable: d.writable,
              }
            : {
                value: typeof d.value === "object" ? tag(d.value) : d.value,
                enumerable: d.enumerable,
                configurable: d.configurable,
                writable: d.writable,
              };
    }
    return result;
  };
  // Promise-returning operators reject instead of throwing; their settlement is
  // read after one task, so a never-settling subscription reads as pending.
  const pending = [];
  const describe = (error) => [
    error?.constructor?.name ?? typeof error,
    String(error?.message ?? error),
  ];
  const attempt = (callback) => {
    try {
      const value = callback();
      if (value instanceof Promise) {
        const row = ["promise"];
        pending.push(
          Promise.race([
            value.then(
              (result) => ["fulfilled", result === undefined ? "undefined" : tag(result)],
              (error) => ["rejected", ...describe(error)],
            ),
            new Promise((done) => setTimeout(() => done(["pending"]), 0)),
          ]).then((state) => row.push(...state)),
        );
        return row;
      }
      return ["ok", value === undefined ? "undefined" : tag(value)];
    } catch (error) {
      return describe(error);
    }
  };
  out.window =
    shape(Object.fromEntries(["Observable", "Subscriber"].map((name) => [name, undefined]))) &&
    ["Observable", "Subscriber"].map((name) => {
      const d = Object.getOwnPropertyDescriptor(window, name);
      return [name, d.enumerable, d.configurable, d.writable];
    });
  out.Observable = shape(Observable);
  out.ObservablePrototype = shape(Observable.prototype);
  out.Subscriber = shape(Subscriber);
  out.SubscriberPrototype = shape(Subscriber.prototype);
  out.when =
    shape({ when: EventTarget.prototype.when }).when &&
    (() => {
      const d = Object.getOwnPropertyDescriptor(EventTarget.prototype, "when");
      return [
        d.value.name,
        d.value.length,
        d.enumerable,
        d.configurable,
        d.writable,
        Object.hasOwn(d.value, "prototype"),
      ];
    })();
  out.ownWhen = [
    Object.hasOwn(Node.prototype, "when"),
    Object.hasOwn(Window.prototype, "when"),
    Object.hasOwn(window, "when"),
    document.when === EventTarget.prototype.when,
    window.when === EventTarget.prototype.when,
  ];
  out.protoChain = [
    Object.getPrototypeOf(Observable.prototype) === Object.prototype,
    Object.getPrototypeOf(Subscriber.prototype) === Object.prototype,
    Object.getPrototypeOf(Observable) === Function.prototype,
  ];
  const noop = () => {};
  const source = new Observable(noop);
  out.instance = [
    tag(source),
    Object.getOwnPropertyNames(source).length,
    Object.getPrototypeOf(source) === Observable.prototype,
  ];
  let captured;
  new Observable((subscriber) => {
    captured = subscriber;
  }).subscribe();
  out.subscriber = [
    tag(captured),
    Object.getOwnPropertyNames(captured).length,
    captured.signal === captured.signal,
    tag(captured.signal),
    captured.active,
  ];
  const brands = {};
  for (const name of Object.getOwnPropertyNames(Observable.prototype)) {
    if (name === "constructor") continue;
    brands[name] = [
      attempt(() => Observable.prototype[name].call({}, noop)),
      attempt(() => Observable.prototype[name].call(captured, noop)),
    ];
  }
  for (const name of Object.getOwnPropertyNames(Subscriber.prototype)) {
    if (name === "constructor") continue;
    const d = Object.getOwnPropertyDescriptor(Subscriber.prototype, name);
    brands["Subscriber." + name] = attempt(() =>
      d.get ? d.get.call(source) : d.value.call(source, noop),
    );
  }
  brands.when = [
    attempt(() => EventTarget.prototype.when.call({}, "x")),
    attempt(() => EventTarget.prototype.when.call(undefined, "x")),
    attempt(() => EventTarget.prototype.when.call(new EventTarget(), "x")),
    attempt(() => EventTarget.prototype.when.call(new AbortController().signal, "x")),
    attempt(() => EventTarget.prototype.when.call(document.createElement("div"), "x")),
  ];
  out.brands = brands;
  class Sub extends Observable {}
  const sub = new Sub(noop);
  out.subclass = [
    sub instanceof Sub,
    sub instanceof Observable,
    tag(sub),
    Object.getPrototypeOf(sub.map((x) => x)) === Observable.prototype,
    Observable.from(sub) === sub,
  ];
  out.construct = [
    attempt(() => new Observable()),
    attempt(() => new Observable(undefined)),
    attempt(() => new Observable(null)),
    attempt(() => new Observable({})),
    attempt(() => new Observable(1)),
    attempt(() => Observable(noop)),
    attempt(() => Observable.call(source, noop)),
    attempt(() => new Subscriber()),
    attempt(() => Subscriber()),
    attempt(() => Reflect.construct(Observable, [noop], Object)),
    attempt(() => new Observable(class {})),
  ];
  const calls = [];
  const log = (label) => (value) => (calls.push(label), value);
  const getters = (label, object) =>
    new Proxy(object, {
      get(target, key, receiver) {
        calls.push(label + ":" + String(key));
        return Reflect.get(target, key, receiver);
      },
      has(target, key) {
        calls.push(label + ":has:" + String(key));
        return Reflect.has(target, key);
      },
    });
  const conversions = {};
  calls.length = 0;
  source.subscribe(
    getters("observer", { next: noop, error: noop, complete: noop }),
    getters("options", {}),
  );
  conversions.subscribeOrder = calls.slice();
  calls.length = 0;
  source.inspect(getters("inspector", { next: noop }));
  conversions.inspectOrder = calls.slice();
  calls.length = 0;
  document.when(
    { toString: log("type") },
    getters("options", { capture: { valueOf: log("capture") }, passive: 0 }),
  );
  conversions.whenOrder = calls.slice();
  calls.length = 0;
  conversions.takeConversion = attempt(() => source.take({ valueOf: log("valueOf") }));
  conversions.takeCalls = calls.slice();
  conversions.subscribe = [
    attempt(() => source.subscribe(1)),
    attempt(() => source.subscribe("x")),
    attempt(() => source.subscribe(null)),
    attempt(() => source.subscribe(undefined, undefined)),
    attempt(() => source.subscribe({ next: 1 })),
    attempt(() => source.subscribe({ error: "e" })),
    attempt(() => source.subscribe({ complete: {} })),
    attempt(() => source.subscribe({ next: null })),
    attempt(() => source.subscribe({}, 1)),
    attempt(() => source.subscribe({}, null)),
    attempt(() => source.subscribe({}, { signal: null })),
    attempt(() => source.subscribe({}, { signal: {} })),
    attempt(() => source.subscribe({}, { signal: undefined })),
    attempt(() => source.subscribe(1, { signal: {} })),
    attempt(() => source.subscribe({ next: 1 }, { signal: {} })),
  ];
  conversions.operators = {};
  for (const name of [
    "takeUntil",
    "map",
    "filter",
    "take",
    "drop",
    "flatMap",
    "switchMap",
    "inspect",
    "catch",
    "finally",
    "toArray",
    "forEach",
    "first",
    "last",
    "some",
    "every",
    "find",
    "reduce",
  ]) {
    conversions.operators[name] = [
      attempt(() => source[name]()),
      attempt(() => source[name](1)),
      attempt(() => source[name]({})),
      attempt(() => source[name](noop, 1)),
      attempt(() => source[name](noop, { signal: 1 })),
    ];
  }
  conversions.numbers = [-1, 1.9, NaN, Infinity, 2 ** 64, "3", null, true].map((value) =>
    attempt(() => source.take(value)),
  );
  conversions.from = [
    attempt(() => Observable.from()),
    attempt(() => Observable.from(undefined)),
    attempt(() => Observable.from(null)),
    attempt(() => Observable.from(1)),
    attempt(() => Observable.from("abc")),
    attempt(() => Observable.from({})),
    attempt(() => Observable.from([])),
    attempt(() => Observable.from(new Set())),
    attempt(() => Observable.from(Promise.resolve())),
    attempt(() => Observable.from({ [Symbol.iterator]: 1 })),
    attempt(() => Observable.from({ [Symbol.asyncIterator]: 1 })),
    attempt(() => Observable.from({ [Symbol.asyncIterator]: null, [Symbol.iterator]: noop })),
    // oxlint-disable-next-line unicorn/no-thenable -- a thenable is not a Promise
    attempt(() => Observable.from({ [Symbol.iterator]: null, then: noop })),
    // oxlint-disable-next-line unicorn/no-thenable -- a thenable is not a Promise
    attempt(() => Observable.from({ then: noop })),
    attempt(() => Observable.from((function* () {})())),
    attempt(() => Observable.from((async function* () {})())),
    attempt(() => Observable.from(Object.create(Observable.prototype))),
    attempt(() =>
      Observable.from({
        get [Symbol.asyncIterator]() {
          throw new RangeError("getter");
        },
      }),
    ),
    attempt(() => Observable.call(undefined)),
  ];
  calls.length = 0;
  Observable.from(getters("from", { [Symbol.iterator]: noop }));
  conversions.fromOrder = calls.slice();
  calls.length = 0;
  attempt(() => Observable.from(getters("from", {})));
  conversions.fromPlainOrder = calls.slice();
  conversions.when = [
    attempt(() => document.when()),
    attempt(() => document.when(Symbol())),
    attempt(() => document.when("x", 1)),
    attempt(() => document.when("x", null)),
    attempt(() => document.when(undefined)),
  ];
  out.conversions = conversions;
  await Promise.all(pending);
  return JSON.stringify(out);
})();
