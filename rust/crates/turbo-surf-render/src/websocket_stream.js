// WebSocketStream and WebSocketError (#144, network-transports-probe.js): the promise and
// stream face of the same native transport as WebSocket (websocket.js). Messages are
// enqueued as strings (text) or ArrayBuffers (binary); `closed` settles with the server's
// close code, a failed connection rejects `opened` and `closed` with WebSocketError.
(() => {
  "use strict";
  const apply = Reflect.apply,
    make = __G.makeNativeFunction,
    define = Object.defineProperty;
  const ErrorType = __exceptionTypeError,
    Exception = DOMException;
  const transport = __G.webSocketTransport;
  const Readable = ReadableStream,
    Writable = WritableStream,
    PromiseType = Promise,
    then = Promise.prototype.then;
  const encoder = new TextEncoder(),
    encode = TextEncoder.prototype.encode;
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
  const validCode = (code) => code === 1000 || (code >= 3000 && code <= 4999);
  // Shared by WebSocketError's constructor and WebSocketStream.close().
  const closeInfo = (init, context) => {
    let code = init?.closeCode;
    const reason = init?.reason === undefined ? "" : transport.string(init.reason);
    if (code !== undefined) {
      code = +code & 65535;
      if (!validCode(code))
        throw new Exception(
          `${context}: The close code must be either 1000, or between 3000 and 4999. ${code} is neither.`,
          "InvalidAccessError",
        );
    } else if (reason) code = 1000;
    if (apply(encode, encoder, [reason]).length > 123)
      throw new Exception(
        `${context}: The close reason must not be greater than 123 UTF-8 bytes.`,
        "SyntaxError",
      );
    return { code: code ?? null, reason };
  };
  const WebSocketError = __defineInterface(
    "WebSocketError",
    0,
    DOMException,
    function (message = "", init = undefined) {
      if (!new.target)
        throw new ErrorType(
          "Failed to construct 'WebSocketError': Please use the 'new' operator, this DOM object constructor cannot be called as a function.",
        );
      const info = closeInfo(init, "Failed to construct 'WebSocketError'");
      const error = Reflect.construct(
        Exception,
        [transport.string(message), "WebSocketError"],
        new.target,
      );
      apply(stateSet, errors, [error, info]);
      return error;
    },
  );
  for (const [name, read] of [
    ["closeCode", (info) => info.code],
    ["reason", (info) => info.reason],
  ])
    define(WebSocketError.prototype, name, {
      __proto__: null,
      get: make(
        function () {
          return read(lookup(errors, this));
        },
        "get " + name,
        0,
      ),
      enumerable: true,
      configurable: true,
    });
  // Engine-made errors may carry 1006, which the public constructor refuses.
  const failure = (message, code) => {
    const error = new WebSocketError(message);
    apply(stateSet, errors, [error, { code: code ?? null, reason: "" }]);
    return error;
  };
  const settle = (record, event) => {
    record.done = true;
    if (event.kind === "closed" && event.clean) {
      record.readable?.close();
      record.resolveClosed({ closeCode: event.code, reason: event.reason });
      return;
    }
    const error = failure("WebSocket connection failed", 1006);
    if (record.opening) record.rejectOpened(failure("WebSocket opening handshake failed"));
    record.readable?.error(error);
    record.rejectClosed(error);
  };
  const receive = (record, event) => {
    if (event.kind === "open") {
      record.opening = false;
      record.resolveOpened(opened(record, event));
    } else if (event.kind === "text") record.readable?.enqueue(event.text);
    else if (event.kind === "binary") {
      const bytes = event.bytes;
      record.readable?.enqueue(
        bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength),
      );
    } else if (transport.terminal(event.kind)) settle(record, event);
    return !transport.terminal(event.kind);
  };
  const send = (record, chunk) => {
    if (record.done) throw failure("WebSocket is closed");
    if (typeof chunk === "string") ops.op_ws_send_text(record.id, chunk);
    else if (chunk instanceof ArrayBuffer) ops.op_ws_send_binary(record.id, new Uint8Array(chunk));
    else if (ArrayBuffer.isView(chunk))
      ops.op_ws_send_binary(
        record.id,
        new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength),
      );
    else throw new ErrorType("WebSocketStream only writes strings and BufferSources");
  };
  const closeSocket = (record, code, reason) => {
    if (record.done || record.closing) return;
    record.closing = true;
    if (record.opening) {
      ops.op_ws_drop(record.id);
      return;
    }
    ops.op_ws_close(record.id, code ?? -1, reason);
  };
  const opened = (record, event) => {
    const readable = new Readable({
      start(controller) {
        record.readable = controller;
      },
      cancel() {
        closeSocket(record, null, "");
      },
    });
    const writable = new Writable({
      write(chunk) {
        send(record, chunk);
      },
      close() {
        closeSocket(record, null, "");
      },
      abort() {
        closeSocket(record, null, "");
      },
    });
    return { readable, writable, extensions: event.extensions, protocol: event.protocol };
  };
  const quiet = (promise) => {
    apply(then, promise, [undefined, () => {}]);
    return promise;
  };
  const WebSocketStream = __defineInterface(
    "WebSocketStream",
    1,
    null,
    function (url, options = undefined) {
      const context = "Failed to construct 'WebSocketStream'";
      if (!new.target)
        throw new ErrorType(
          `${context}: Please use the 'new' operator, this DOM object constructor cannot be called as a function.`,
        );
      if (!arguments.length)
        throw new ErrorType(`${context}: 1 argument required, but only 0 present.`);
      const parsed = transport.parseSocketURL(transport.string(url), context);
      const protocols = transport.checkProtocols(
        [...(options?.protocols ?? [])].map(transport.string),
        context,
      );
      const stream = Object.create(new.target.prototype);
      const record = {
        __proto__: null,
        url: parsed.href,
        opening: true,
        done: false,
        closing: false,
        readable: null,
      };
      record.opened = quiet(
        new PromiseType((resolve, reject) => {
          record.resolveOpened = resolve;
          record.rejectOpened = reject;
        }),
      );
      record.closed = quiet(
        new PromiseType((resolve, reject) => {
          record.resolveClosed = resolve;
          record.rejectClosed = reject;
        }),
      );
      record.id = ops.op_connect_allowed(__document, parsed.href)
        ? ops.op_ws_open(__document, parsed.href, protocols)
        : 0;
      apply(stateSet, states, [stream, record]);
      if (record.id === 0) settle(record, { kind: "failed" });
      else transport.pump(record.id, (event) => receive(record, event));
      return stream;
    },
  );
  const P = WebSocketStream.prototype;
  for (const name of ["url", "opened", "closed"])
    define(P, name, {
      __proto__: null,
      get: make(
        function () {
          return lookup(states, this)[name];
        },
        "get " + name,
        0,
      ),
      enumerable: true,
      configurable: true,
    });
  define(P, "close", {
    __proto__: null,
    value: make(
      function (closeInfoInit = undefined) {
        const record = lookup(states, this);
        const info = closeInfo(closeInfoInit, "Failed to execute 'close' on 'WebSocketStream'");
        closeSocket(record, info.code, info.reason);
      },
      "close",
      0,
    ),
    writable: true,
    enumerable: true,
    configurable: true,
  });
})();
