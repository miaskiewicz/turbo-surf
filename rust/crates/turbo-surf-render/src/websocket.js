// WebSocket (#28, harness/detection/transports-probe.js; transport #144,
// network-transports-probe.js): Chrome's interface, URL and protocol validation, closing
// handshake rules, and a native RFC 6455 transport (network_transports.rs). Events come
// from an unref'd op pump, so an open socket never holds the page's event loop.
(() => {
  "use strict";
  const apply = Reflect.apply,
    make = __G.makeNativeFunction,
    define = Object.defineProperty;
  const ErrorType = __exceptionTypeError,
    Exception = DOMException,
    Close = globalThis.CloseEvent,
    PlainEvent = __hostEvent,
    Message = __hostMessageEvent,
    BlobType = globalThis.Blob;
  const LocationURL = globalThis.URL,
    StringValue = String,
    encoder = new TextEncoder(),
    encode = TextEncoder.prototype.encode;
  const hostTimeout = globalThis.setTimeout,
    exec = RegExp.prototype.exec,
    unref = __core.unrefOpPromise;
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
  // URL and subprotocol validation shared with WebSocketStream (websocket_stream.js).
  const parseSocketURL = (url, context) => {
    let parsed;
    try {
      parsed = new LocationURL(url, apply(__baseURI, __document, []));
    } catch {
      throw new Exception(`${context}: The URL '${url}' is invalid.`, "SyntaxError");
    }
    if (parsed.protocol === "http:") parsed.protocol = "ws:";
    else if (parsed.protocol === "https:") parsed.protocol = "wss:";
    if (parsed.protocol !== "ws:" && parsed.protocol !== "wss:")
      throw new Exception(
        `${context}: The URL's scheme must be either 'http', 'https', 'ws', or 'wss'. '${parsed.protocol.slice(0, -1)}' is not allowed.`,
        "SyntaxError",
      );
    if (parsed.hash || apply(exec, /#/, [parsed.href]))
      throw new Exception(
        `${context}: The URL contains a fragment identifier ('${parsed.hash.slice(1)}'). Fragment identifiers are not allowed in WebSocket URLs.`,
        "SyntaxError",
      );
    return parsed;
  };
  const checkProtocols = (list, context) => {
    for (let at = 0; at < list.length; at++) {
      if (!apply(exec, token, [list[at]]))
        throw new Exception(`${context}: The subprotocol '${list[at]}' is invalid.`, "SyntaxError");
      for (let other = 0; other < at; other++)
        if (list[other] === list[at])
          throw new Exception(
            `${context}: The subprotocol '${list[at]}' is duplicated.`,
            "SyntaxError",
          );
    }
    return list;
  };
  // The serialized origin of a ws(s) URL: MessageEvent.origin of its messages.
  const socketOrigin = (parsed) => `${parsed.protocol}//${parsed.host}`;
  // Reads transport events until the connection ends; `handle` returns false to stop.
  const pump = async (id, handle) => {
    for (;;) {
      const next = ops.op_ws_next(id);
      apply(unref, undefined, [next]);
      if (!handle(await next)) return;
    }
  };
  const terminal = (kind) => kind === "closed" || kind === "failed" || kind === "gone";
  __G.webSocketTransport = { parseSocketURL, checkProtocols, socketOrigin, pump, terminal, string };
  const WebSocket = __defineInterface(
    "WebSocket",
    1,
    EventTarget,
    function (url, protocols = undefined) {
      const context = "Failed to construct 'WebSocket'";
      if (!new.target)
        throw new ErrorType(
          `${context}: Please use the 'new' operator, this DOM object constructor cannot be called as a function.`,
        );
      if (!arguments.length)
        throw new ErrorType(`${context}: 1 argument required, but only 0 present.`);
      url = string(url);
      const parsed = parseSocketURL(url, context);
      const list = checkProtocols(
        protocols === undefined
          ? []
          : typeof protocols === "object" &&
              protocols !== null &&
              typeof protocols[Symbol.iterator] === "function"
            ? [...protocols].map(string)
            : [string(protocols)],
        context,
      );
      const target = new EventTarget();
      Object.setPrototypeOf(target, new.target.prototype);
      const record = {
        __proto__: null,
        url: parsed.href,
        origin: socketOrigin(parsed),
        readyState: 0,
        bufferedAmount: 0,
        binaryType: "blob",
        protocol: "",
        extensions: "",
        id: ops.op_connect_allowed(__document, parsed.href)
          ? ops.op_ws_open(__document, parsed.href, list)
          : 0,
      };
      apply(stateSet, states, [target, record]);
      if (record.id === 0) blocked(target, record);
      else pump(record.id, (event) => receive(target, record, event));
      return target;
    },
  );
  // A connect-src violation: CLOSED at once, then an error event and no close event.
  const blocked = (socket, record) => {
    record.readyState = 3;
    apply(hostTimeout, globalThis, [() => __dispatchTrusted(socket, new PlainEvent("error")), 0]);
  };
  const closeEvent = (socket, record, wasClean, code, reason) => {
    record.readyState = 3;
    __dispatchTrusted(socket, new Close("close", { wasClean, code, reason }));
  };
  const messageData = (record, event) => {
    if (event.kind === "text") return event.text;
    const bytes = event.bytes;
    return record.binaryType === "blob"
      ? new BlobType([bytes])
      : bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
  };
  const receivers = {
    __proto__: null,
    open(socket, record, event) {
      record.readyState = 1;
      record.protocol = event.protocol;
      record.extensions = event.extensions;
      __dispatchTrusted(socket, new PlainEvent("open"));
    },
    message(socket, record, event) {
      // Messages that arrive after close() was called are dropped.
      if (record.readyState !== 1) return;
      __dispatchTrusted(
        socket,
        new Message("message", { data: messageData(record, event), origin: record.origin }),
      );
    },
    sent(socket, record, event) {
      record.bufferedAmount = Math.max(0, record.bufferedAmount - event.sent);
    },
    closed(socket, record, event) {
      closeEvent(socket, record, event.clean, event.code, event.reason);
    },
    failed(socket, record) {
      record.readyState = 3;
      __dispatchTrusted(socket, new PlainEvent("error"));
      closeEvent(socket, record, false, 1006, "");
    },
  };
  const receive = (socket, record, event) => {
    const kind = event.kind === "text" || event.kind === "binary" ? "message" : event.kind;
    const handler = receivers[kind];
    if (handler && !(record.dropped && kind !== "sent")) handler(socket, record, event);
    return !terminal(event.kind);
  };
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
  getter("extensions", (record) => record.extensions);
  getter("protocol", (record) => record.protocol);
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
  // The bytes a send() queues, and the size bufferedAmount grows by.
  const payload = (data) => {
    const blob = __blobStates.get(data);
    if (blob) return [false, blob.bytes];
    if (data instanceof ArrayBuffer) return [false, new Uint8Array(data)];
    if (ArrayBuffer.isView(data))
      return [false, new Uint8Array(data.buffer, data.byteOffset, data.byteLength)];
    const text = string(data);
    return [true, text, apply(encode, encoder, [text]).length];
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
        const connecting = record.readyState === 0;
        record.readyState = 2;
        if (!connecting) {
          ops.op_ws_close(record.id, code === undefined ? -1 : code, reason ?? "");
          return;
        }
        // Not yet established: fail the connection, then report its abnormal closure.
        record.dropped = true;
        ops.op_ws_drop(record.id);
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
        const [text, bytes, size] = payload(data);
        record.bufferedAmount += text ? size : bytes.length;
        // Data sent after closing only grows bufferedAmount.
        if (record.readyState !== 1) return;
        if (text) ops.op_ws_send_text(record.id, bytes);
        else ops.op_ws_send_binary(record.id, bytes);
      },
      "send",
      1,
    ),
    writable: true,
    enumerable: true,
    configurable: true,
  });
})();
