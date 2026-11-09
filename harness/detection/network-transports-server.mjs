// Localhost WebSocket + EventSource server for network-transports-probe.js. A hand-rolled
// RFC 6455 server (no dependency) so the script below is the whole contract; the native test
// (rust/crates/turbo-surf-render/tests/network_transports.rs) serves the same script.
// Handshake and EventSource request headers are logged in wire order with volatile values
// (key, user agent, port) masked, and read back by the probe from /log/ws and /log/sse.
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { deflateRawSync, inflateRawSync } from "node:zlib";

const GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const TAIL = Buffer.from([0, 0, 0xff, 0xff]);

/** Header list in wire order, with the per-run values masked. */
export function maskHeaders(raw, port) {
  const out = [];
  for (let at = 0; at < raw.length; at += 2) {
    const name = raw[at];
    let value = raw[at + 1];
    const lower = name.toLowerCase();
    if (lower === "sec-websocket-key")
      value =
        Buffer.from(value, "base64").length === 16 ? "<16-byte base64>" : `<invalid ${value}>`;
    else if (lower === "user-agent") value = "<user-agent>";
    else value = value.replaceAll(`:${port}`, ":<port>");
    out.push([name, value]);
  }
  return out;
}

function frame(opcode, payload, fin = true, rsv1 = false) {
  const length = payload.length;
  const head = length < 126 ? 2 : length < 65536 ? 4 : 10;
  const out = Buffer.alloc(head + length);
  out[0] = (fin ? 0x80 : 0) | (rsv1 ? 0x40 : 0) | opcode;
  if (length < 126) out[1] = length;
  else if (length < 65536) {
    out[1] = 126;
    out.writeUInt16BE(length, 2);
  } else {
    out[1] = 127;
    out.writeBigUInt64BE(BigInt(length), 2);
  }
  payload.copy(out, head);
  return out;
}

// Parses complete client frames off the front of `buffer`; returns [frames, rest].
function parse(buffer) {
  const frames = [];
  for (;;) {
    if (buffer.length < 2) break;
    let length = buffer[1] & 0x7f;
    let at = 2;
    if (length === 126) {
      if (buffer.length < 4) break;
      length = buffer.readUInt16BE(2);
      at = 4;
    } else if (length === 127) {
      if (buffer.length < 10) break;
      length = Number(buffer.readBigUInt64BE(2));
      at = 10;
    }
    const masked = (buffer[1] & 0x80) !== 0;
    const total = at + (masked ? 4 : 0) + length;
    if (buffer.length < total) break;
    const mask = masked ? buffer.subarray(at, at + 4) : null;
    const payload = Buffer.from(buffer.subarray(total - length, total));
    if (mask) for (let i = 0; i < payload.length; i++) payload[i] ^= mask[i & 3];
    frames.push({
      fin: (buffer[0] & 0x80) !== 0,
      rsv1: (buffer[0] & 0x40) !== 0,
      opcode: buffer[0] & 0x0f,
      masked,
      payload,
    });
    buffer = buffer.subarray(total);
  }
  return [frames, buffer];
}

function closePayload(code, reason) {
  const out = Buffer.alloc(2 + Buffer.byteLength(reason));
  out.writeUInt16BE(code, 0);
  out.write(reason, 2);
  return out;
}

// Server side of one accepted socket: echo plus the probe's text commands.
function serve(socket, deflate) {
  let buffer = Buffer.alloc(0);
  let message = null;
  let closeSent = false;
  const send = (opcode, payload) => {
    if (deflate && (opcode === 1 || opcode === 2)) {
      const packed = deflateRawSync(payload, { finishFlush: 2 });
      socket.write(frame(opcode, packed.subarray(0, packed.length - 4), true, true));
    } else socket.write(frame(opcode, payload));
  };
  const commands = {
    binary: () => send(2, Buffer.from([9, 8, 7])),
    frag: () => {
      socket.write(frame(1, Buffer.from("ab"), false));
      socket.write(frame(0, Buffer.from("cd"), true));
    },
    ping: () => socket.write(frame(9, Buffer.from("p1"))),
    big: () => send(1, Buffer.alloc(70000, "x")),
    drop: () => socket.destroy(),
    badutf8: () => socket.write(frame(1, Buffer.from([0xff, 0xfe]))),
  };
  const text = (payload) => {
    const value = payload.toString("utf8");
    if (value.startsWith("close:")) {
      const [, code, reason] = value.split(":");
      closeSent = true;
      socket.write(frame(8, closePayload(Number(code), reason)));
      return;
    }
    if (commands[value]) commands[value]();
    else send(1, payload);
  };
  const deliver = (opcode, payload, compressed) => {
    if (compressed) payload = inflateRawSync(Buffer.concat([payload, TAIL]), { finishFlush: 2 });
    if (opcode === 1) text(payload);
    else send(2, payload);
  };
  const control = (item) => {
    if (item.opcode === 8) {
      if (closeSent) socket.end();
      else socket.end(frame(8, item.payload));
      return;
    }
    if (item.opcode === 10) send(1, Buffer.concat([Buffer.from("pong:"), item.payload]));
  };
  socket.on("data", (chunk) => {
    let frames;
    [frames, buffer] = parse(Buffer.concat([buffer, chunk]));
    for (const item of frames) {
      if (!item.masked) return socket.end(frame(8, closePayload(1002, "unmasked")));
      if (item.opcode >= 8) {
        control(item);
        continue;
      }
      if (item.opcode !== 0) message = { opcode: item.opcode, rsv1: item.rsv1, parts: [] };
      message.parts.push(item.payload);
      if (item.fin) deliver(message.opcode, Buffer.concat(message.parts), message.rsv1);
    }
  });
  socket.on("error", () => {});
}

function upgrade(log, port) {
  return (request, socket) => {
    const url = new URL(request.url, "http://127.0.0.1");
    log.ws.push({
      path: url.pathname + url.search,
      headers: maskHeaders(request.rawHeaders, port()),
    });
    if (url.pathname === "/ws/reject") {
      socket.end("HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n");
      return;
    }
    const key = request.headers["sec-websocket-key"];
    const accept =
      url.pathname === "/ws/bad-accept"
        ? "AAAAAAAAAAAAAAAAAAAAAAAAAAA="
        : createHash("sha1")
            .update(key + GUID)
            .digest("base64");
    const lines = [
      "HTTP/1.1 101 Switching Protocols",
      "Upgrade: websocket",
      "Connection: Upgrade",
      `Sec-WebSocket-Accept: ${accept}`,
    ];
    const offered = (request.headers["sec-websocket-protocol"] || "")
      .split(",")
      .map((part) => part.trim());
    if (url.searchParams.get("proto") === "bogus") lines.push("Sec-WebSocket-Protocol: bogus");
    else if (offered.includes("chat")) lines.push("Sec-WebSocket-Protocol: chat");
    const deflate = url.searchParams.has("deflate");
    if (deflate)
      lines.push(
        "Sec-WebSocket-Extensions: permessage-deflate; server_no_context_takeover; client_no_context_takeover",
      );
    socket.write(`${lines.join("\r\n")}\r\n\r\n`);
    serve(socket, deflate);
  };
}

function eventStream(request, response, log, port) {
  const path = new URL(request.url, "http://127.0.0.1").pathname;
  log.sse.push({ path, headers: maskHeaders(request.rawHeaders, port()) });
  const attempt = log.sse.filter((entry) => entry.path === path).length;
  const stream = (body, end = true) => {
    response.writeHead(200, { "Content-Type": "text/event-stream", "Cache-Control": "no-cache" });
    if (end) response.end(body);
    else response.write(body);
  };
  if (path === "/sse/stream") {
    if (attempt === 1)
      stream(
        "﻿retry: 30\nid: 7\ndata: one\n\nevent: custom\ndata: two\ndata: three\n\n: comment\n\ndata: four\r\n\r\nid\ndata: five\n\nid: 9\ndata: six\n\ndata: unterminated",
      );
    else if (attempt === 2) stream("data: again\r\rid: 10\n\n");
    else {
      response.writeHead(204);
      response.end();
    }
    return;
  }
  if (path === "/sse/wrong-type") {
    response.writeHead(200, { "Content-Type": "text/plain" });
    response.end("data: no\n\n");
    return;
  }
  if (path === "/sse/forever") {
    stream("data: first\n\n", false);
    return;
  }
  response.writeHead(404, { "Content-Type": "text/plain" });
  response.end("missing");
}

/**
 * Starts the server on 127.0.0.1; `pages` maps paths to HTML documents. `reset()` clears the
 * request logs between probes and `stop()` closes every connection, upgraded ones included.
 */
export async function startTransportServer(pages) {
  const log = { ws: [], sse: [] };
  const server = createServer((request, response) => {
    const path = new URL(request.url, "http://127.0.0.1").pathname;
    if (path === "/log/ws" || path === "/log/sse") {
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify(log[path.slice(5)]));
      return;
    }
    if (path.startsWith("/sse/")) return eventStream(request, response, log, port);
    response.writeHead(200, { "Content-Type": "text/html" });
    response.end(pages[path] ?? pages["/"]);
  });
  const port = () => server.address().port;
  const sockets = new Set();
  server.on("connection", (socket) => {
    sockets.add(socket);
    socket.on("close", () => sockets.delete(socket));
  });
  server.on("upgrade", upgrade(log, port));
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const reset = () => {
    log.ws.length = 0;
    log.sse.length = 0;
  };
  const stop = () => {
    for (const socket of sockets) socket.destroy();
    return new Promise((done) => server.close(done));
  };
  return { server, port: port(), log, reset, stop };
}
