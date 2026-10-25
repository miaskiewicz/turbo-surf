// Endpoint ownership and queues live in the isolate's Rust registry.
let __portTransferSequence, __hostPortMessage;
{
  const realm = globalThis,
    Target = globalThis.EventTarget,
    Message = globalThis.MessageEvent,
    PortTypeError = TypeError;
  const apply = Reflect.apply,
    define = Object.defineProperty,
    setPrototype = Object.setPrototypeOf,
    freeze = Object.freeze;
  const records = __interfaceMap("MessagePort"),
    channels = __interfaceMap("MessageChannel");
  const schedule = globalThis.setTimeout,
    make = __G.makeNativeFunction;
  const listen = Target.prototype.addEventListener,
    unlisten = Target.prototype.removeEventListener;
  const preventDefault = Event.prototype.preventDefault,
    iteratorSymbol = Symbol.iterator;
  const ownIndex = (array, index, value) =>
    define(array, index, {
      __proto__: null,
      value,
      writable: true,
      configurable: true,
      enumerable: true,
    });
  const objectLike = (value) =>
    value !== null && (typeof value === "object" || typeof value === "function");
  const record = (receiver) => {
    const state = records.get(receiver);
    if (!state) throw new PortTypeError("Illegal invocation");
    return state;
  };
  const method = (target, name, fn, length) =>
    define(target, name, {
      value: make(fn, name, length),
      writable: true,
      configurable: true,
      enumerable: true,
    });
  __portTransferSequence = (value) => {
    if (!objectLike(value)) throw new PortTypeError("Transfer list must be a sequence");
    const iterator = value[iteratorSymbol];
    if (typeof iterator !== "function") throw new PortTypeError("Transfer list must be iterable");
    const source = apply(iterator, value, []);
    if (!objectLike(source)) throw new PortTypeError("Invalid transfer iterator");
    const next = source.next,
      result = [];
    if (typeof next !== "function") throw new PortTypeError("Invalid transfer iterator");
    while (true) {
      const step = apply(next, source, []);
      if (!objectLike(step)) throw new PortTypeError("Invalid transfer iterator result");
      if (step.done) return result;
      const entry = step.value;
      if (result.length === 4096 || !objectLike(entry)) {
        const error = new PortTypeError(
          result.length === 4096
            ? "Native transfer sequence budget exceeded"
            : "Transfer entries must be objects",
        );
        throw error;
      }
      ownIndex(result, result.length, entry);
    }
  };
  // Host arrays contain already-validated native ports. Avoid guest iteration,
  // species constructors and indexed setters while building the frozen view.
  // `activation` is the sender's UserActivation snapshot bits, or -1 without one.
  __hostPortMessage = (type, data, ports, source = null, origin = "", activation = -1) => {
    const userActivation =
      activation >= 0 ? __userActivationSnapshot(!!(activation & 1), !!(activation & 2)) : null;
    const event = new Message(type, { data, source, origin, userActivation });
    const list = [];
    for (let index = 0; index < ports.length; index++) ownIndex(list, index, ports[index]);
    __messageEventState.get(event).ports = freeze(list);
    return event;
  };
  const Port = make(
    class MessagePort extends Target {
      constructor() {
        throw new PortTypeError("Illegal constructor");
      }
    },
    "MessagePort",
    0,
    true,
    true,
  );
  setPrototype(Port.prototype, Target.prototype);
  setPrototype(Port, Target);
  const create = () => {
    const object = new Target();
    setPrototype(object, Port.prototype);
    records.set(object, {
      __proto__: null,
      onmessage: null,
      onmessageerror: null,
      onmessageListener: null,
      onmessageerrorListener: null,
    });
    return object;
  };
  for (const type of ["message", "messageerror"]) {
    const name = "on" + type,
      listenerName = name + "Listener";
    define(Port.prototype, name, {
      configurable: true,
      enumerable: true,
      get: make(
        function () {
          return record(this)[name];
        },
        "get " + name,
        0,
      ),
      set: make(
        function (value) {
          const state = record(this);
          state[name] = typeof value === "function" ? value : null;
          if (state[listenerName] && state[name] === null) {
            apply(unlisten, this, [type, state[listenerName]]);
            state[listenerName] = null;
          } else if (!state[listenerName] && state[name] !== null) {
            const callback = function (event) {
              const handler = state[name];
              if (handler && apply(handler, this, [event]) === false)
                apply(preventDefault, event, []);
            };
            state[listenerName] = callback;
            apply(listen, this, [type, callback]);
          }
          if (type === "message") ops.op_port_start(this);
        },
        "set " + name,
        1,
      ),
    });
  }
  method(
    Port.prototype,
    "postMessage",
    function (value, options) {
      record(this);
      if (!arguments.length) throw new PortTypeError("A message is required");
      let transfer,
        includeActivation = false;
      if (options !== undefined) {
        if (options !== null && !objectLike(options))
          throw new PortTypeError("Options must be a dictionary or sequence");
        if (
          options !== null &&
          (typeof options === "object" || typeof options === "function") &&
          typeof options[iteratorSymbol] === "function"
        )
          transfer = __portTransferSequence(options);
        else if (options !== null) {
          includeActivation = !!options.includeUserActivation;
          const list = options.transfer;
          if (list !== undefined) transfer = __portTransferSequence(list);
        }
      }
      ops.op_port_post_message(this, value, transfer, includeActivation);
    },
    1,
  );
  method(
    Port.prototype,
    "start",
    function () {
      record(this);
      ops.op_port_start(this);
    },
    0,
  );
  method(
    Port.prototype,
    "close",
    function () {
      record(this);
      ops.op_port_close(this);
    },
    0,
  );
  define(Port.prototype, Symbol.toStringTag, { value: "MessagePort", configurable: true });
  define(Port, "prototype", { writable: false });
  const Channel = make(
    class MessageChannel {
      constructor() {
        channels.set(this, ops.op_port_new_channel(realm));
      }
    },
    "MessageChannel",
    0,
    true,
    true,
  );
  for (const [name, index] of [
    ["port1", 0],
    ["port2", 1],
  ])
    define(Channel.prototype, name, {
      configurable: true,
      enumerable: true,
      get: make(
        function () {
          const ports = channels.get(this);
          if (!ports) throw new PortTypeError("Illegal invocation");
          return ports[index];
        },
        "get " + name,
        0,
      ),
    });
  define(Channel.prototype, Symbol.toStringTag, { value: "MessageChannel", configurable: true });
  define(Channel, "prototype", { writable: false });
  define(globalThis, "MessagePort", { value: Port, writable: true, configurable: true });
  define(globalThis, "MessageChannel", { value: Channel, writable: true, configurable: true });
  ops.op_port_install_adapter(
    create,
    (port, data, ports, failed, activation) =>
      ops.op_dispatch_host_event(
        __hostPortMessage(failed ? "messageerror" : "message", data, ports, null, "", activation),
        port,
      ),
    (port) => schedule(() => ops.op_port_deliver(port), 0),
  );
}
