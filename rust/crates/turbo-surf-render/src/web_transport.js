// WebTransport and WebTransportError (#144, network-transports-probe.js). WebTransport
// runs over HTTP/3 (QUIC), which this engine's network stack does not speak, so every
// session fails its opening handshake the way Chrome 156 reports an unreachable server:
// `ready` and `closed` reject with WebTransportError("Opening handshake failed.",
// source "session"). URL validation matches Chrome; the pinned gap is the transport.
(() => {
  "use strict";
  const apply = Reflect.apply,
    make = __G.makeNativeFunction,
    define = Object.defineProperty;
  const ErrorType = __exceptionTypeError,
    Exception = DOMException,
    LocationURL = globalThis.URL,
    PromiseType = Promise,
    then = Promise.prototype.then,
    StringValue = String;
  const hostTimeout = globalThis.setTimeout;
  const states = new WeakMap(),
    errors = new WeakMap(),
    stateGet = WeakMap.prototype.get,
    stateSet = WeakMap.prototype.set;
  const lookup = (map, receiver) => {
    const value =
      receiver !== null && typeof receiver === "object"
        ? apply(stateGet, map, [receiver])
        : undefined;
    if (!value) throw new ErrorType("Illegal invocation");
    return value;
  };
  const getters = (prototype, map, names) => {
    for (const [name, read] of names)
      define(prototype, name, {
        __proto__: null,
        get: make(
          function () {
            return read(lookup(map, this));
          },
          "get " + name,
          0,
        ),
        enumerable: true,
        configurable: true,
      });
  };
  const methods = (prototype, names) => {
    for (const [name, length, body] of names)
      define(prototype, name, {
        __proto__: null,
        value: make(body, name, length),
        writable: true,
        enumerable: true,
        configurable: true,
      });
  };

  const WebTransportError = __defineInterface(
    "WebTransportError",
    0,
    DOMException,
    function (message = "", options = undefined) {
      if (!new.target)
        throw new ErrorType(
          "Failed to construct 'WebTransportError': Please use the 'new' operator, this DOM object constructor cannot be called as a function.",
        );
      const source = options?.source === undefined ? "stream" : StringValue(options.source);
      if (source !== "stream" && source !== "session")
        throw new ErrorType(
          `Failed to construct 'WebTransportError': Failed to read the 'source' property from 'WebTransportErrorOptions': The provided value '${source}' is not a valid enum value of type WebTransportErrorSource.`,
        );
      const code = options?.streamErrorCode ?? null;
      const error = Reflect.construct(
        Exception,
        [StringValue(message), "WebTransportError"],
        new.target,
      );
      apply(stateSet, errors, [
        error,
        { source, streamErrorCode: code === null ? null : +code >>> 0 },
      ]);
      return error;
    },
  );
  getters(WebTransportError.prototype, errors, [
    ["source", (info) => info.source],
    ["streamErrorCode", (info) => info.streamErrorCode],
  ]);

  const handshakeFailure = () =>
    new WebTransportError("Opening handshake failed.", { source: "session" });
  const pending = () => {
    let reject;
    const promise = new PromiseType((_, fail) => (reject = fail));
    // Chrome reports these rejections only to their own handlers.
    apply(then, promise, [undefined, () => {}]);
    return [promise, reject];
  };
  const WebTransport = __defineInterface(
    "WebTransport",
    1,
    null,
    function (url, options = undefined) {
      const context = "Failed to construct 'WebTransport'";
      if (!new.target)
        throw new ErrorType(
          `${context}: Please use the 'new' operator, this DOM object constructor cannot be called as a function.`,
        );
      if (!arguments.length)
        throw new ErrorType(`${context}: 1 argument required, but only 0 present.`);
      url = StringValue(url);
      let parsed;
      try {
        parsed = new LocationURL(url);
      } catch {
        throw new Exception(`${context}: The URL '${url}' is invalid.`, "SyntaxError");
      }
      if (parsed.protocol !== "https:")
        throw new Exception(
          `${context}: The URL's scheme must be 'https'. '${parsed.protocol.slice(0, -1)}' is not allowed.`,
          "SyntaxError",
        );
      if (parsed.hash || url.includes("#"))
        throw new Exception(
          `${context}: The URL contains a fragment identifier ('${parsed.hash.slice(1)}'). Fragment identifiers are not allowed in WebTransport URLs.`,
          "SyntaxError",
        );
      const transport = Object.create(new.target.prototype);
      const [ready, rejectReady] = pending();
      const [closed, rejectClosed] = pending();
      const record = { __proto__: null, ready, closed, failed: false };
      apply(stateSet, states, [transport, record]);
      void options;
      apply(hostTimeout, globalThis, [
        () => {
          record.failed = true;
          rejectReady(handshakeFailure());
          rejectClosed(handshakeFailure());
        },
        0,
      ]);
      return transport;
    },
  );
  const P = WebTransport.prototype;
  getters(P, states, [
    ["ready", (record) => record.ready],
    ["closed", (record) => record.closed],
    ["protocol", () => ""],
    ["datagrams", () => null],
    ["incomingBidirectionalStreams", () => null],
    ["incomingUnidirectionalStreams", () => null],
  ]);
  const notConnected = (name) =>
    function () {
      lookup(states, this);
      return PromiseType.reject(
        new Exception(
          `Failed to execute '${name}' on 'WebTransport': No connection.`,
          "InvalidStateError",
        ),
      );
    };
  methods(P, [
    [
      "close",
      0,
      function () {
        lookup(states, this);
      },
    ],
    ["createBidirectionalStream", 0, notConnected("createBidirectionalStream")],
    ["createUnidirectionalStream", 0, notConnected("createUnidirectionalStream")],
  ]);
})();
