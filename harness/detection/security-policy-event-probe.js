(() => {
  const init = {
    documentURI: "doc",
    referrer: "ref",
    blockedURI: "blocked",
    effectiveDirective: "script-src-elem",
    violatedDirective: "script-src-elem",
    originalPolicy: "script-src 'none'",
    sourceFile: "source",
    disposition: "report",
    statusCode: 201,
    lineNumber: 42,
    columnNumber: 7,
    bubbles: true,
    cancelable: true,
    composed: true,
  };
  const event = new SecurityPolicyViolationEvent("securitypolicyviolation", init);
  const prototype = SecurityPolicyViolationEvent.prototype;
  const members = Object.getOwnPropertyNames(prototype).filter((name) => name !== "constructor");
  const rows = members.map((name) => {
    const descriptor = Object.getOwnPropertyDescriptor(prototype, name);
    const errors = [];
    for (const receiver of [{}, null, new Proxy(event, {})]) {
      try {
        descriptor.get.call(receiver);
        errors.push("accepted");
      } catch (error) {
        errors.push(error.name);
      }
    }
    return {
      name,
      value: event[name],
      enumerable: descriptor.enumerable,
      configurable: descriptor.configurable,
      get: descriptor.get.name,
      length: descriptor.get.length,
      set: !!descriptor.set,
      errors,
    };
  });
  const defaults = new SecurityPolicyViolationEvent("test");
  const reads = [];
  new SecurityPolicyViolationEvent(
    "x",
    new Proxy(
      {},
      {
        get(_target, key) {
          reads.push(String(key));
          return undefined;
        },
      },
    ),
  );
  const failures = [];
  for (const args of [
    [],
    [Symbol()],
    ["x", 1],
    ["x", { disposition: "bad" }],
    ["x", { blockedURI: Symbol() }],
  ]) {
    try {
      Reflect.construct(SecurityPolicyViolationEvent, args);
      failures.push("accepted");
    } catch (error) {
      failures.push(error.name);
    }
  }
  const numbers = new SecurityPolicyViolationEvent("x", {
    lineNumber: -1,
    columnNumber: 4294967297,
    statusCode: -1,
  });
  return {
    rows,
    defaults: members.map((name) => [name, defaults[name]]),
    reads,
    failures,
    numbers: [numbers.lineNumber, numbers.columnNumber, numbers.statusCode],
    event: {
      type: event.type,
      bubbles: event.bubbles,
      cancelable: event.cancelable,
      composed: event.composed,
      isTrusted: event.isTrusted,
      own: Reflect.ownKeys(event).map(String),
      brand: Object.prototype.toString.call(event),
      parent: Object.getPrototypeOf(prototype) === Event.prototype,
      constructorParent: Object.getPrototypeOf(SecurityPolicyViolationEvent) === Event,
    },
  };
})();
