// Localhost servers for the Chrome identity battery. One TLS listener per HTTPS
// origin captures each connection's exact ClientHello and taps the decrypted
// client→server HTTP/2 frames before Node's own HTTP/2 server sees them. Every
// request is logged with its raw header order and casing. Nothing leaves loopback.
import { execFileSync } from "node:child_process";
import { createHash, X509Certificate } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createServer as createHttpServer } from "node:http";
import { createServer as createH2Server } from "node:http2";
import { createServer as createNetServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Duplex } from "node:stream";
import { TLSSocket, createSecureContext } from "node:tls";
import { H2FrameTap, describeClientHello, headerList, summarizeH2 } from "./identity-wire.mjs";

// Replays the bytes consumed while capturing the ClientHello, then forwards the
// socket. A JS Duplex (not the net.Socket) keeps TLSSocket from reading the
// native handle directly and losing the replayed prefix.
class Replay extends Duplex {
  constructor(socket, prefix) {
    super();
    this.socket = socket;
    this.push(prefix);
    socket.on("data", (chunk) => {
      if (!this.push(chunk)) socket.pause();
    });
    socket.on("end", () => this.push(null));
    socket.on("error", (error) => this.destroy(error));
    socket.on("close", () => this.destroy());
  }
  _read() {
    this.socket.resume();
  }
  _write(chunk, encoding, done) {
    this.socket.write(chunk, encoding, done);
  }
  _final(done) {
    this.socket.end(done);
  }
  _destroy(error, done) {
    this.socket.destroy();
    done(error);
  }
}

// Tees decrypted inbound bytes into the frame tap and forwards them unchanged.
class Tee extends Replay {
  constructor(socket, onData) {
    super(socket, Buffer.alloc(0));
    socket.prependListener("data", onData);
    this.alpnProtocol = socket.alpnProtocol;
    this.encrypted = true;
    this.remoteAddress = socket.remoteAddress;
  }
}

/** Ephemeral loopback certificate (127.0.0.1, ::1, localhost) and Chrome's SPKI pin. */
export function loopbackCertificate() {
  const scratch = mkdtempSync(join(tmpdir(), "turbo-identity-cert-"));
  try {
    const key = join(scratch, "key.pem");
    const cert = join(scratch, "cert.pem");
    execFileSync(
      "openssl",
      [
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-days",
        "1",
        "-subj",
        "/CN=127.0.0.1",
        "-addext",
        "subjectAltName=IP:127.0.0.1,IP:::1,DNS:localhost",
        "-keyout",
        key,
        "-out",
        cert,
      ],
      { stdio: "ignore" },
    );
    const pem = readFileSync(cert, "utf8");
    const x509 = new X509Certificate(pem);
    return {
      key: readFileSync(key, "utf8"),
      cert: pem,
      der: x509.raw,
      spki: createHash("sha256")
        .update(x509.publicKey.export({ type: "spki", format: "der" }))
        .digest("base64"),
    };
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

const listen = (server, port, host) =>
  new Promise((done, fail) => {
    server.once("error", fail);
    server.listen(port, host, () => {
      server.off("error", fail);
      done(server.address().port);
    });
  });

// Binds 127.0.0.1 and ::1 on one port so `localhost` resolves to either family.
async function listenLoopback(makeServer) {
  for (let attempt = 0; attempt < 20; attempt++) {
    const v4 = makeServer();
    const port = await listen(v4, 0, "127.0.0.1");
    const v6 = makeServer();
    try {
      await listen(v6, port, "::1");
      return { port, servers: [v4, v6] };
    } catch {
      await new Promise((done) => v4.close(done));
    }
  }
  throw new Error("Could not bind one loopback port on both 127.0.0.1 and ::1");
}

/**
 * Start every battery origin. `route(request)` returns `{ status, headers, body }`;
 * the request object carries tier/kind/origin labels parsed from the path.
 */
export async function startIdentityServers(route) {
  const certificate = loopbackCertificate();
  const secureContext = createSecureContext({ key: certificate.key, cert: certificate.cert });
  const requests = [];
  const connections = [];
  const waiters = new Set();
  const sockets = new Set();
  const ports = {};
  const owner = new WeakMap();

  const record = (entry) => {
    entry.seq = requests.length;
    requests.push(entry);
    for (const waiter of waiters) waiter();
  };

  const handle = (protocol, label) => (request, response) => {
    let length = 0;
    request.on("data", (chunk) => {
      length += chunk.length;
    });
    request.on("end", () => {
      const connection = owner.get(protocol === "h2" ? request.stream.session : request.socket);
      const path = request.url;
      const match = /^\/t\/([a-z0-9-]+)\/k\/([a-z0-9-]+)/.exec(path);
      const entry = {
        tier: match?.[1] ?? null,
        kind: match?.[2] ?? null,
        method: request.method,
        path,
        origin: label(request),
        protocol,
        httpVersion: request.httpVersion,
        headers: headerList(request.rawHeaders),
        bodyBytes: length,
        connection: connection?.id ?? null,
        streamId: protocol === "h2" ? request.stream.id : null,
      };
      if (connection && protocol === "h2") {
        const pseudo = entry.headers.map(([name]) => name).filter((name) => name.startsWith(":"));
        connection.pseudoOrder ||= pseudo;
        entry.streamPriority =
          connection.tap.frames.find(
            (frame) => frame.type === "HEADERS" && frame.streamId === request.stream.id,
          )?.priority ?? null;
      }
      record(entry);
      const reply = route(entry, ports);
      response.writeHead(reply.status ?? 200, reply.headers ?? {});
      response.end(reply.body ?? "");
    });
  };

  const tlsOrigin = async (name, alpn) => {
    const h2 = createH2Server({ remoteCustomSettings: [8, 9] }, () => {});
    const h1 = createHttpServer({ keepAliveTimeout: 1000 });
    const label = (request) => {
      const host = (request.headers[":authority"] || request.headers.host || "").replace(
        /:\d+$/,
        "",
      );
      return host === "localhost" ? `${name}X` : name;
    };
    let opening = null;
    // Node wraps an emitted socket in a Proxy; the session event fires synchronously
    // inside emit("connection"), so it is the reliable join to the tapped connection.
    h2.on("session", (session) => owner.set(session, opening));
    h2.on("request", handle("h2", label));
    h1.on("request", handle("http/1.1", label));
    const accept = (socket) => {
      sockets.add(socket);
      socket.on("close", () => sockets.delete(socket));
      socket.on("error", () => {});
      let prefix = Buffer.alloc(0);
      const first = (chunk) => {
        prefix = Buffer.concat([prefix, chunk]);
        if (prefix.length < 5 || prefix.length < 5 + prefix.readUInt16BE(3)) return;
        socket.off("data", first);
        socket.pause();
        const raw = prefix.subarray(0, 5 + prefix.readUInt16BE(3));
        const connection = {
          id: connections.length,
          origin: name,
          family: socket.remoteFamily,
          raw: raw.toString("base64"),
          rawSha256: createHash("sha256").update(raw).digest("hex"),
          tap: new H2FrameTap(),
        };
        try {
          connection.hello = describeClientHello(raw);
        } catch (error) {
          connection.helloError = String(error);
        }
        connections.push(connection);
        const tls = new TLSSocket(new Replay(socket, prefix), {
          isServer: true,
          secureContext,
          ALPNProtocols: alpn,
        });
        tls.on("error", (error) => {
          connection.tlsError = String(error);
        });
        tls.once("secure", () => {
          connection.alpn = tls.alpnProtocol || null;
          if (tls.alpnProtocol === "h2") {
            const tee = new Tee(tls, (data) => connection.tap.push(data));
            opening = connection;
            h2.emit("connection", tee);
            opening = null;
          } else {
            owner.set(tls, connection);
            h1.emit("connection", tls);
          }
        });
      };
      socket.on("data", first);
    };
    const bound = await listenLoopback(() => createNetServer(accept));
    ports[name] = bound.port;
    return [...bound.servers, h1];
  };

  const plain = createHttpServer({ keepAliveTimeout: 1000 });
  plain.on(
    "request",
    handle("http/1.1", () => "D"),
  );
  plain.on("connection", (socket) => {
    sockets.add(socket);
    socket.on("close", () => sockets.delete(socket));
  });
  const servers = [
    ...(await tlsOrigin("A", ["h2", "http/1.1"])),
    ...(await tlsOrigin("B", ["http/1.1"])),
    ...(await tlsOrigin("C", ["h2", "http/1.1"])),
    plain,
  ];
  ports.D = await listen(plain, 0, "127.0.0.1");

  const origins = {
    A: `https://127.0.0.1:${ports.A}`,
    AX: `https://localhost:${ports.A}`,
    B: `https://127.0.0.1:${ports.B}`,
    C: `https://127.0.0.1:${ports.C}`,
    CX: `https://localhost:${ports.C}`,
    D: `http://127.0.0.1:${ports.D}`,
  };

  return {
    certificate,
    origins,
    ports,
    requests,
    connections,
    /** Resolve once `predicate(requests)` holds, or reject after `timeoutMs`. */
    waitFor(predicate, timeoutMs = 15000, what = "requests") {
      return new Promise((done, fail) => {
        const check = () => {
          if (!predicate(requests)) return;
          clearTimeout(timer);
          waiters.delete(check);
          done();
        };
        const timer = setTimeout(() => {
          waiters.delete(check);
          fail(new Error(`Timed out waiting for ${what}`));
        }, timeoutMs);
        waiters.add(check);
        check();
      });
    },
    /** Connection records with their H2 summaries; raw hello bytes stay base64. */
    connectionRecords() {
      return connections.map(({ tap, ...rest }) => ({
        ...rest,
        h2: rest.alpn === "h2" ? summarizeH2(tap.frames, rest.pseudoOrder) : null,
        h2Frames: rest.alpn === "h2" ? tap.frames.slice(0, 48) : null,
        h2Error: tap.error,
      }));
    },
    async close() {
      for (const socket of sockets) socket.destroy();
      await Promise.all(servers.map((server) => new Promise((done) => server.close(done))));
    },
  };
}
