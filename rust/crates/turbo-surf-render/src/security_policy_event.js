const __policyEventStates = __interfaceMap("SecurityPolicyViolationEvent"),
  __policyEventBase = globalThis.Event,
  __policyEventString = String,
  __policyEventTypeError = TypeError,
  __policyEventDefine = Object.defineProperty,
  __policyEventNumberFinite = Number.isFinite,
  __policyEventTrunc = Math.trunc;
const __policyEventStringValue = (value) => {
  if (typeof value === "symbol")
    throw new __policyEventTypeError("Cannot convert a Symbol to a string");
  return __policyEventString(value);
};
const __policyEventInteger = (value) => {
  const number = +value;
  return __policyEventNumberFinite(number) ? __policyEventTrunc(number) | 0 : 0;
};
const __policyEventMembers = [
  "documentURI",
  "referrer",
  "blockedURI",
  "violatedDirective",
  "effectiveDirective",
  "originalPolicy",
  "disposition",
  "sourceFile",
  "statusCode",
  "lineNumber",
  "columnNumber",
  "sample",
];
class SecurityPolicyViolationEvent extends __policyEventBase {
  constructor(type, init = undefined) {
    if (!arguments.length) throw new __policyEventTypeError("An event type is required");
    type = __policyEventStringValue(type);
    if (init != null && typeof init !== "object" && typeof init !== "function")
      throw new __policyEventTypeError("Event initialization must be a dictionary");
    init = init ?? { __proto__: null };
    const bubbles = !!init.bubbles,
      cancelable = !!init.cancelable,
      composed = !!init.composed;
    const state = { __proto__: null };
    const conversionMembers = [
      "blockedURI",
      "columnNumber",
      "disposition",
      "documentURI",
      "effectiveDirective",
      "lineNumber",
      "originalPolicy",
      "referrer",
      "sample",
      "sourceFile",
      "statusCode",
      "violatedDirective",
    ];
    for (let index = 0; index < conversionMembers.length; index++) {
      const name = conversionMembers[index],
        value = init[name];
      if (name === "lineNumber" || name === "columnNumber")
        state[name] = value === undefined ? 0 : __policyEventInteger(value);
      else if (name === "statusCode")
        state[name] = value === undefined ? 0 : __policyEventInteger(value) & 65535;
      else {
        state[name] =
          value === undefined
            ? name === "disposition"
              ? "enforce"
              : ""
            : __policyEventStringValue(value);
        if (name === "disposition" && state[name] !== "enforce" && state[name] !== "report")
          throw new __policyEventTypeError("Invalid SecurityPolicyViolationEventDisposition");
      }
    }
    void cancelable;
    super(type, { bubbles, cancelable: false, composed });
    __policyEventStates.set(this, state);
  }
}
delete SecurityPolicyViolationEvent.prototype.constructor;
for (const name of __policyEventMembers) {
  const getter = Object.getOwnPropertyDescriptor(
    {
      get value() {
        const state = __policyEventStates.get(this);
        if (!state) throw new __policyEventTypeError("Illegal invocation");
        return state[name];
      },
    },
    "value",
  ).get;
  __policyEventDefine(SecurityPolicyViolationEvent.prototype, name, {
    get: __documentFunction(getter, "get " + name, 0),
    enumerable: true,
    configurable: true,
  });
}
__policyEventDefine(SecurityPolicyViolationEvent.prototype, "constructor", {
  value: SecurityPolicyViolationEvent,
  writable: true,
  configurable: true,
});
__policyEventDefine(SecurityPolicyViolationEvent.prototype, Symbol.toStringTag, {
  value: "SecurityPolicyViolationEvent",
  configurable: true,
});
globalThis.SecurityPolicyViolationEvent = SecurityPolicyViolationEvent;

const __policyViolationDocument = __document,
  __policyViolationTimer = globalThis.setTimeout,
  __policyViolationCancelTimer = globalThis.clearTimeout,
  __policyViolationCtor = SecurityPolicyViolationEvent,
  __policyViolationApply = Reflect.apply,
  __policyViolationConnected = __G.domAccessor("isConnected", false, "Node"),
  __policyViolationTickets = new Map(),
  __policyViolationSet = Map.prototype.set,
  __policyViolationDelete = Map.prototype.delete,
  __policyViolationEach = Map.prototype.forEach,
  __policyViolationClear = Map.prototype.clear;
let __policyViolationPending = 0,
  __policyViolationBytes = 0;
const __policyViolationConsumer = (target, metadata) => {
  if (!ops.op_window_active(__policyViolationDocument)) return true;
  let bytes = 96;
  for (let index = 0; index < __policyEventMembers.length; index++) {
    const value = metadata[__policyEventMembers[index]];
    if (typeof value === "string") bytes += value.length * 2;
  }
  if (__policyViolationPending >= 4096 || bytes > 67108864 - __policyViolationBytes) return false;
  __policyViolationPending++;
  __policyViolationBytes += bytes;
  const ticket = __policyViolationTimer(() => {
    __policyViolationApply(__policyViolationDelete, __policyViolationTickets, [ticket]);
    __policyViolationPending--;
    __policyViolationBytes -= bytes;
    if (!ops.op_window_active(__policyViolationDocument)) return;
    const event = new __policyViolationCtor("securitypolicyviolation", {
      ...metadata,
      bubbles: true,
      composed: true,
    });
    ops.op_event_trust(event, -1);
    const recipient =
      target && __policyViolationApply(__policyViolationConnected, target, [])
        ? target
        : __policyViolationDocument;
    ops.op_dispatch_host_event(event, recipient);
  }, 0);
  __policyViolationApply(__policyViolationSet, __policyViolationTickets, [ticket, bytes]);
  return true;
};
const __retireCspViolationTasks = () => {
  __policyViolationApply(__policyViolationEach, __policyViolationTickets, [
    (_bytes, ticket) => __policyViolationCancelTimer(ticket),
  ]);
  __policyViolationApply(__policyViolationClear, __policyViolationTickets, []);
  __policyViolationPending = 0;
  __policyViolationBytes = 0;
  __documentRuntimeState.cspViolationConsumer = null;
};
__documentRuntimeState.cspViolationConsumer = __policyViolationConsumer;
ops.op_set_host_capabilities({ queueCspViolation: __policyViolationConsumer });
