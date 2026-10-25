// WebSocket (#28, harness/detection/transports-probe.js): Chrome's interface, URL and protocol
// validation and closing handshake rules. This engine has no WebSocket transport, so a
// connection is never established: the socket stays CONNECTING (as against a server that never
// answers) until close() fails it, which fires error and a 1006 close like Chrome.
(() => {
  "use strict";
  const apply = Reflect.apply,
    make = __G.makeNativeFunction,
    define = Object.defineProperty;
  const ErrorType = __exceptionTypeError,
    Exception = DOMException,
    Close = globalThis.CloseEvent,
    PlainEvent = __hostEvent;
  const LocationURL = globalThis.URL,
    StringValue = String,
    encoder = new TextEncoder(),
    encode = TextEncoder.prototype.encode;
  const hostTimeout = globalThis.setTimeout,
    exec = RegExp.prototype.exec;
  const states = new WeakMap(),
    stateGet = WeakMap.prototype.get,
    stateSet = WeakMap.prototype.set;
  const state = (receiver) => {
    const value =
      receiver !== null && typeof receiver === "object"
        ? apply(stateGet, states, [receiver])
        : undefined;
    if (!value) throw new ErrorType("Illegal invocation");
    return value;
  };
  const check = (receiver) => void state(receiver);
  const string = (value) => {
    if (typeof value === "symbol") throw new ErrorType("Cannot convert a Symbol to a string");
    return StringValue(value);
  };
  const token = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
  const WebSocket = __defineInterface(
    "WebSocket",
    1,
    EventTarget,
    function (url, protocols = undefined) {
      if (!new.target)
        throw new ErrorType(
          "Failed to construct 'WebSocket': Please use the 'new' operator, this DOM object constructor cannot be called as a function.",
        );
      if (!arguments.length)
        throw new ErrorType(
          "Failed to construct 'WebSocket': 1 argument required, but only 0 present.",
        );
      url = string(url);
      let parsed;
      try {
        parsed = new LocationURL(url, apply(__baseURI, __document, []));
      } catch {
        throw new Exception(
          `Failed to construct 'WebSocket': The URL '${url}' is invalid.`,
          "SyntaxError",
        );
      }
      if (parsed.protocol === "http:") parsed.protocol = "ws:";
      else if (parsed.protocol === "https:") parsed.protocol = "wss:";
      if (parsed.protocol !== "ws:" && parsed.protocol !== "wss:")
        throw new Exception(
          `Failed to construct 'WebSocket': The URL's scheme must be either 'http', 'https', 'ws', or 'wss'. '${parsed.protocol.slice(0, -1)}' is not allowed.`,
          "SyntaxError",
        );
      if (parsed.hash || apply(exec, /#/, [parsed.href]))
        throw new Exception(
          `Failed to construct 'WebSocket': The URL contains a fragment identifier ('${parsed.hash.slice(1)}'). Fragment identifiers are not allowed in WebSocket URLs.`,
          "SyntaxError",
        );
      const list =
        protocols === undefined
          ? []
          : typeof protocols === "object" &&
              protocols !== null &&
              typeof protocols[Symbol.iterator] === "function"
            ? [...protocols].map(string)
            : [string(protocols)];
      for (let at = 0; at < list.length; at++) {
        if (!apply(exec, token, [list[at]]))
          throw new Exception(
            `Failed to construct 'WebSocket': The subprotocol '${list[at]}' is invalid.`,
            "SyntaxError",
          );
        for (let other = 0; other < at; other++)
          if (list[other] === list[at])
            throw new Exception(
              `Failed to construct 'WebSocket': The subprotocol '${list[at]}' is duplicated.`,
              "SyntaxError",
            );
      }
      const target = new EventTarget();
      Object.setPrototypeOf(target, new.target.prototype);
      apply(stateSet, states, [
        target,
        { __proto__: null, url: parsed.href, readyState: 0, bufferedAmount: 0, binaryType: "blob" },
      ]);
      return target;
    },
  );
  const P = WebSocket.prototype;
  const getter = (name, read) =>
    define(P, name, {
      __proto__: null,
      get: make(
        function () {
          return read(state(this));
        },
        "get " + name,
        0,
      ),
      enumerable: true,
      configurable: true,
    });
  getter("url", (record) => record.url);
  getter("readyState", (record) => record.readyState);
  getter("bufferedAmount", (record) => record.bufferedAmount);
  for (const name of ["onopen", "onerror", "onclose"]) __defineEventHandler(P, name, check);
  getter("extensions", () => "");
  getter("protocol", () => "");
  __defineEventHandler(P, "onmessage", check);
  define(P, "binaryType", {
    __proto__: null,
    get: make(
      function () {
        return state(this).binaryType;
      },
      "get binaryType",
      0,
    ),
    set: make(
      function (value) {
        const record = state(this);
        value = string(value);
        if (value === "blob" || value === "arraybuffer") record.binaryType = value;
      },
      "set binaryType",
      1,
    ),
    enumerable: true,
    configurable: true,
  });
  __defineConstants(WebSocket, [
    ["CONNECTING", 0],
    ["OPEN", 1],
    ["CLOSING", 2],
    ["CLOSED", 3],
  ]);
  const size = (data) => {
    const blob = __blobStates.get(data);
    if (blob) return blob.bytes.length;
    if (data instanceof ArrayBuffer) return data.byteLength;
    if (ArrayBuffer.isView(data)) return data.byteLength;
    return apply(encode, encoder, [string(data)]).length;
  };
  define(P, "close", {
    __proto__: null,
    value: make(
      function (code = undefined, reason = undefined) {
        const record = state(this);
        if (code !== undefined) {
          code = +code & 65535;
          if (code !== 1000 && (code < 3000 || code > 4999))
            throw new Exception(
              `Failed to execute 'close' on 'WebSocket': The close code must be either 1000, or between 3000 and 4999. ${code} is neither.`,
              "InvalidAccessError",
            );
        }
        if (reason !== undefined) {
          reason = string(reason);
          if (apply(encode, encoder, [reason]).length > 123)
            throw new Exception(
              "Failed to execute 'close' on 'WebSocket': The close reason must not be greater than 123 UTF-8 bytes.",
              "SyntaxError",
            );
        }
        if (record.readyState >= 2) return;
        // Never established: fail the connection, then report its abnormal closure.
        record.readyState = 2;
        const socket = this;
        apply(hostTimeout, globalThis, [
          () => {
            record.readyState = 3;
            __dispatchTrusted(socket, new PlainEvent("error"));
            __dispatchTrusted(
              socket,
              new Close("close", { wasClean: false, code: 1006, reason: "" }),
            );
          },
          0,
        ]);
      },
      "close",
      0,
    ),
    writable: true,
    enumerable: true,
    configurable: true,
  });
  define(P, "send", {
    __proto__: null,
    value: make(
      function (data) {
        const record = state(this);
        if (!arguments.length)
          throw new ErrorType(
            "Failed to execute 'send' on 'WebSocket': 1 argument required, but only 0 present.",
          );
        if (record.readyState === 0)
          throw new Exception(
            "Failed to execute 'send' on 'WebSocket': Still in CONNECTING state.",
            "InvalidStateError",
          );
        // Data sent after closing only grows bufferedAmount.
        record.bufferedAmount += size(data);
      },
      "send",
      1,
    ),
    writable: true,
    enumerable: true,
    configurable: true,
  });
})();
