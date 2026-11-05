(async () => {
  "use strict";
  // WebSocket, WebSocketStream, EventSource and WebTransport against the localhost transport
  // server (network-transports-server.mjs; the native test serves the same script). Every wait
  // is event-driven: no timers, so a virtual-time engine cannot time out ahead of the network.
  const host = location.host;
  const out = {};
  const run = (f) => {
    try {
      return ["ok", f()];
    } catch (error) {
      return ["throw", error.name, error.message];
    }
  };
  const bytes = (view) =>
    Array.from(new Uint8Array(view.buffer || view, view.byteOffset || 0, view.byteLength));
  const describe = async (data) => {
    if (typeof data === "string")
      return data.length > 64 ? ["text", data.length, data.slice(0, 4)] : ["text", data];
    if (data instanceof ArrayBuffer) return ["arraybuffer", bytes(data)];
    if (data instanceof Blob)
      return ["blob", data.size, data.type, bytes(new Uint8Array(await data.arrayBuffer()))];
    if (ArrayBuffer.isView(data)) return [Object.prototype.toString.call(data), bytes(data)];
    return [typeof data];
  };
  const keys = (o) => Object.getOwnPropertyNames(o).sort();
  const once = (target, type) =>
    new Promise((done) => target.addEventListener(type, done, { once: true }));

  // ---- WebSocket ----
  // Each socket logs [type, readyState, ...details]; message data is described after the run.
  const socket = (path, protocols, setup) => {
    const ws =
      protocols === undefined
        ? new WebSocket(`ws://${host}${path}`)
        : new WebSocket(`ws://${host}${path}`, protocols);
    const log = [];
    const pending = [];
    for (const type of ["open", "message", "error", "close"])
      ws.addEventListener(type, (event) => {
        const row = [type, ws.readyState, event.isTrusted, Object.prototype.toString.call(event)];
        if (type === "message") {
          row.push(event.origin, event.lastEventId);
          const at = log.length;
          pending.push(describe(event.data).then((value) => (log[at] = row.concat([value]))));
        }
        if (type === "close") row.push(event.wasClean, event.code, event.reason);
        if (type === "open") row.push(ws.protocol, ws.extensions);
        log.push(row);
      });
    if (setup) setup(ws);
    const closed = once(ws, "close")
      .then(() => Promise.all(pending))
      .then(() => log);
    return { ws, log, closed };
  };
  const messages = (ws, count) =>
    new Promise((done) => {
      let seen = 0;
      ws.addEventListener("message", function listener() {
        if (++seen === count) {
          ws.removeEventListener("message", listener);
          done();
        }
      });
    });
  const ws = {};
  {
    // eslint-disable-next-line unicorn/no-document-cookie -- handshakes must carry a document cookie.
    document.cookie = "wsc=1; path=/";
    const s = socket("/ws/echo", ["chat", "superchat"]);
    ws.connecting = [s.ws.url, s.ws.readyState, s.ws.protocol, s.ws.extensions, s.ws.binaryType];
    await once(s.ws, "open");
    ws.opened = [s.ws.readyState, s.ws.protocol, s.ws.extensions, s.ws.bufferedAmount];
    s.ws.binaryType = "arraybuffer";
    s.ws.send("hello");
    ws.buffered = s.ws.bufferedAmount;
    s.ws.send(new Uint8Array([1, 2, 3]));
    s.ws.send(new Uint8Array([4, 5, 6, 7]).buffer);
    s.ws.send(new Blob(["blob!"]));
    s.ws.send("binary");
    s.ws.send("frag");
    s.ws.send("big");
    s.ws.send("été");
    await messages(s.ws, 8);
    // The server's ping is answered by the transport; its pong report is the next message.
    s.ws.send("ping");
    await messages(s.ws, 1);
    s.ws.close(4000, "done");
    ws.closing = [s.ws.readyState, run(() => s.ws.send("late"))[0], s.ws.bufferedAmount];
    ws.echo = await s.closed;
  }
  {
    const s = socket("/ws/echo");
    await once(s.ws, "open");
    s.ws.send("binary");
    await messages(s.ws, 1);
    s.ws.send("close:4321:server bye");
    ws.serverClose = await s.closed;
  }
  {
    const s = socket("/ws/echo");
    await once(s.ws, "open");
    s.ws.close();
    ws.noCode = await s.closed;
  }
  {
    const s = socket("/ws/echo");
    await once(s.ws, "open");
    s.ws.send("drop");
    ws.drop = await s.closed;
  }
  {
    const s = socket("/ws/echo");
    await once(s.ws, "open");
    s.ws.send("badutf8");
    ws.badUtf8 = await s.closed;
  }
  ws.reject = await socket("/ws/reject").closed;
  ws.badAccept = await socket("/ws/bad-accept").closed;
  ws.badProtocol = await socket("/ws/echo?proto=bogus", ["chat"]).closed;
  ws.unrequestedProtocol = await socket("/ws/echo?proto=bogus").closed;
  {
    const s = socket("/ws/echo?deflate", undefined, (ws) => (ws.binaryType = "arraybuffer"));
    await once(s.ws, "open");
    s.ws.send("squeeze squeeze squeeze");
    s.ws.send("again again again");
    s.ws.send("binary");
    await messages(s.ws, 3);
    s.ws.close(1000);
    ws.deflate = await s.closed;
  }
  ws.refused = await new Promise((done) => {
    const socket = new WebSocket("ws://127.0.0.1:1/");
    const log = [];
    socket.onerror = () => log.push(["error", socket.readyState]);
    socket.onclose = (event) =>
      done(log.concat([["close", event.wasClean, event.code, event.reason]]));
  });
  ws.handshakes = await (await fetch("/log/ws")).json();
  out.webSocket = ws;

  // ---- WebSocketStream ----
  const wss = { globals: [typeof WebSocketStream, typeof WebSocketError] };
  if (typeof WebSocketStream === "function") {
    wss.proto = keys(WebSocketStream.prototype);
    wss.errorProto = keys(WebSocketError.prototype);
    wss.construct = [
      run(() => WebSocketStream(`ws://${host}/`)).slice(0, 2),
      run(() => new WebSocketStream("ftp://x/")).slice(0, 2),
      run(() => new WebSocketStream(`ws://${host}/#x`)).slice(0, 2),
    ];
    const error = new WebSocketError("why", { closeCode: 4002, reason: "r" });
    wss.error = [
      error.name,
      error.message,
      error.closeCode,
      error.reason,
      error instanceof DOMException,
    ];
    wss.errorChecks = [
      run(() => new WebSocketError("x", { closeCode: 1001 })).slice(0, 2),
      run(() => new WebSocketError("x", { reason: "r" })).slice(0, 2),
    ];
    const stream = new WebSocketStream(`ws://${host}/ws/echo`, { protocols: ["chat"] });
    wss.url = stream.url;
    const opened = await stream.opened;
    wss.opened = [
      keys(opened),
      opened.protocol,
      opened.extensions,
      Object.prototype.toString.call(opened.readable),
      Object.prototype.toString.call(opened.writable),
    ];
    const writer = opened.writable.getWriter();
    const reader = opened.readable.getReader();
    await writer.write("hi");
    await writer.write(new Uint8Array([7, 8]));
    wss.reads = [
      await describe((await reader.read()).value),
      await describe((await reader.read()).value),
    ];
    stream.close({ closeCode: 4001, reason: "x" });
    const closed = await stream.closed;
    wss.closed = [keys(closed), closed.closeCode, closed.reason];
    wss.afterClose = await reader.read().then(
      (part) => ["read", part.done],
      (error) => ["reject", error.name],
    );
    const rejected = new WebSocketStream(`ws://${host}/ws/reject`);
    wss.rejected = await Promise.allSettled([rejected.opened, rejected.closed]).then((parts) =>
      parts.map((part) =>
        part.status === "rejected"
          ? [part.reason.name, part.reason.closeCode, part.reason.reason]
          : ["fulfilled"],
      ),
    );
    const server = new WebSocketStream(`ws://${host}/ws/echo`);
    const serverOpened = await server.opened;
    const serverWriter = serverOpened.writable.getWriter();
    await serverWriter.write("close:4400:stream bye");
    const serverClosed = await server.closed;
    wss.serverClose = [serverClosed.closeCode, serverClosed.reason];
  }
  out.webSocketStream = wss;

  // ---- EventSource ----
  const es = { global: typeof EventSource };
  if (typeof EventSource === "function") {
    es.constants = [EventSource.CONNECTING, EventSource.OPEN, EventSource.CLOSED];
    es.proto = keys(EventSource.prototype);
    es.construct = [
      run(() => EventSource("/sse")).slice(0, 2),
      run(() => new EventSource("http://[")).slice(0, 2),
    ];
    const stream = (path, init, stop) =>
      new Promise((done) => {
        const source = new EventSource(path, init);
        const log = [[source.url, source.readyState, source.withCredentials]];
        const record = (event) => {
          const row = [event.type, source.readyState, Object.prototype.toString.call(event)];
          if ("data" in event) row.push(event.data, event.lastEventId, event.origin);
          log.push(row);
          if (stop(event, source)) {
            source.close();
            log.push(["closed", source.readyState]);
            done(log);
          }
        };
        for (const type of ["open", "message", "error", "custom"])
          source.addEventListener(type, record);
      });
    es.stream = await stream(
      "/sse/stream",
      undefined,
      (event, source) => event.type === "error" && source.readyState === 2,
    );
    es.wrongType = await stream("/sse/wrong-type", undefined, (event) => event.type === "error");
    es.missing = await stream(
      "/sse/missing",
      { withCredentials: true },
      (event) => event.type === "error",
    );
    es.closeEarly = await stream("/sse/forever", undefined, (event) => event.type === "message");
    es.requests = await (await fetch("/log/sse")).json();
  }
  out.eventSource = es;

  // ---- WebTransport ----
  const wt = { global: typeof WebTransport };
  if (typeof WebTransport === "function") {
    wt.proto = keys(WebTransport.prototype);
    wt.construct = [
      run(() => new WebTransport("http://127.0.0.1:1/")).slice(0, 2),
      run(() => new WebTransport("https://127.0.0.1:1/#x")).slice(0, 2),
    ];
    const transport = new WebTransport("https://127.0.0.1:1/");
    const settled = await Promise.allSettled([transport.ready, transport.closed]);
    wt.failure = settled.map((part) =>
      part.status === "rejected"
        ? [
            part.reason.name,
            part.reason.message,
            part.reason.source,
            part.reason.streamErrorCode,
            part.reason instanceof DOMException,
          ]
        : ["fulfilled"],
    );
  }
  out.webTransport = wt;
  // The server port differs per run.
  return JSON.parse(JSON.stringify(out).replaceAll(`:${location.port}`, ":<port>"));
})();
