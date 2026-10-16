import { X509Certificate, createHash, createPrivateKey } from "node:crypto";
import { readFileSync } from "node:fs";
import { createSecureServer } from "node:http2";
import { createServer as createHttpsServer } from "node:https";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

export async function startUploadServers() {
  const cert = new X509Certificate(
    readFileSync(
      new URL("../../rust/crates/turbo-surf-render/tests/csp_report_cert.der", import.meta.url),
    ),
  );
  const key = createPrivateKey({
    key: readFileSync(
      new URL("../../rust/crates/turbo-surf-render/tests/csp_report_key.der", import.meta.url),
    ),
    type: "pkcs8",
    format: "der",
  }).export({ type: "pkcs8", format: "pem" });
  const tls = { key, cert: cert.toString() };
  const records = new Map();
  const wire = [];
  const held = new Map();
  const sessions = new Set();
  const state = (id) => {
    if (!records.has(id)) records.set(id, { seen: false, bytes: 0, options: 0, aborted: false });
    return records.get(id);
  };
  const handler = (request, response) => {
    response.on("error", () => {});
    const url = new URL(request.url, "https://" + request.headers.host);
    const id = url.searchParams.get("id") || "";
    const record = state(id);
    const entry = {
      method: request.method,
      path: url.pathname,
      id,
      protocol: request.httpVersion,
      bytes: 0,
      bodyHex: "",
    };
    wire.push(entry);
    if (request.headers.origin) {
      response.setHeader("Access-Control-Allow-Origin", request.headers.origin);
      response.setHeader("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
      response.setHeader(
        "Access-Control-Allow-Headers",
        request.headers["access-control-request-headers"] || "content-type",
      );
    }
    if (request.method === "OPTIONS") {
      record.options++;
      response.writeHead(204);
      response.end();
      return;
    }
    response.setHeader("Content-Type", "application/json");
    if (url.pathname === "/state") {
      response.end(JSON.stringify(record));
      return;
    }
    if (url.pathname === "/release") {
      held.get(id)?.();
      response.end("{}");
      return;
    }
    if (!url.pathname.startsWith("/upload") && !url.pathname.startsWith("/redirect")) {
      response.setHeader("Content-Type", "text/html");
      response.end("<!doctype html><body>");
      return;
    }
    const chunks = [];
    let size = 0;
    request.on("aborted", () => {
      record.aborted = true;
    });
    request.on("error", () => {
      record.aborted = true;
    });
    const receive = () => {
      request.on("data", (chunk) => {
        record.seen = true;
        size += chunk.length;
        record.bytes = size;
        if (size > 64 * 1024 * 1024) {
          request.destroy(new Error("Upload fixture cap"));
          return;
        }
        if (size <= 4096) chunks.push(chunk);
      });
      request.on("end", () => {
        entry.bytes = size;
        entry.bodyHex = size <= 4096 ? Buffer.concat(chunks).toString("hex") : "<large>";
        if (url.pathname === "/redirect303" || url.pathname === "/redirect307") {
          const code = url.pathname === "/redirect303" ? 303 : 307;
          response.writeHead(code, { Location: "/redirected" + code + "?id=" + id });
          response.end();
          return;
        }
        if (url.pathname === "/upload-early") return;
        response.end(
          JSON.stringify({
            protocol: request.httpVersion,
            method: request.method,
            bytes: size,
            bodyHex: entry.bodyHex,
            options: record.options,
          }),
        );
      });
    };
    if (url.pathname === "/upload-hold") held.set(id, receive);
    else receive();
    if (url.pathname === "/upload-early") response.end(JSON.stringify({ early: true }));
  };
  const servers = [
    createSecureServer(tls, handler),
    createSecureServer(tls, handler),
    createHttpsServer(tls, handler),
  ];
  for (const server of servers) {
    server.on("session", (session) => {
      sessions.add(session);
      session.once("close", () => sessions.delete(session));
    });
    server.on("tlsClientError", () => {});
    server.on("sessionError", () => {});
    server.on("clientError", (_error, socket) => socket.destroy());
    await new Promise((done, fail) => {
      server.once("error", fail);
      server.listen(0, "127.0.0.1", done);
    });
  }
  const urls = servers.map((server) => "https://127.0.0.1:" + server.address().port);
  const spki = createHash("sha256")
    .update(cert.publicKey.export({ type: "spki", format: "der" }))
    .digest("base64");
  return {
    primary: urls[0],
    peer: urls[1],
    http1: urls[2],
    spki,
    wire,
    async close() {
      for (const session of sessions) session.destroy();
      for (const server of servers) {
        server.closeAllConnections?.();
        server.close();
      }
    },
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const fixtures = await startUploadServers();
  process.stdout.write(
    JSON.stringify({ primary: fixtures.primary, peer: fixtures.peer, http1: fixtures.http1 }) +
      "\n",
  );
  process.stdin.resume();
  process.stdin.on("end", () => {
    fixtures.close().finally(() => process.exit(0));
  });
}
