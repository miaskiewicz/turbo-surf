import { X509Certificate, createHash, createPrivateKey } from "node:crypto";
import { readFileSync } from "node:fs";
import { createServer } from "node:https";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

export const dictionaryCases = [
  { name: "manual-exact-hash", manual: "matching" },
  { name: "manual-mismatched-hash", manual: "mismatch" },
  { name: "manual-malformed-hash", manual: "malformed" },
  {
    name: "manual-known-hash-outside-match",
    header: true,
    match: "target",
    resource: "/sub/target",
    manual: "matching",
  },
  {
    name: "manual-dictionary-id-alone",
    header: true,
    match: "target",
    resource: "/sub/target",
    manual: "id",
  },
  { name: "relative-root-target", header: true, match: "target", resource: "/target" },
  { name: "relative-nested-target", header: true, match: "target", resource: "/sub/target" },
  { name: "header", header: true },
  { name: "parser-link" },
  { name: "cache-no-store", cache: "no-store" },
  { name: "cache-reload", cache: "reload" },
  { name: "header-and-parser", header: true, both: true },
  { name: "dynamic-link", dynamic: true },
  { name: "foreign-svg-link", dynamic: true, foreign: true },
  { name: "shadow-open", dynamic: true, shadow: "open" },
  { name: "shadow-closed", dynamic: true, shadow: "closed" },
  { name: "removed-link", dynamic: true, remove: true },
  { name: "rel-changed-link", dynamic: true, relChange: true },
  { name: "href-changed-link", dynamic: true, hrefChange: true },
  { name: "connect-blocked", csp: "connect-src 'none'" },
  { name: "default-blocked", csp: "default-src 'none'; script-src 'unsafe-inline'" },
  {
    name: "connect-allowed",
    csp: "default-src 'none'; script-src 'unsafe-inline'; connect-src 'self'",
  },
  { name: "header-cross-cors", header: true, cross: true, cors: true },
  { name: "header-cross-no-cors", header: true, cross: true },
  { name: "link-cross-cors", cross: true, cors: true },
  { name: "link-cross-no-cors", cross: true },
  { name: "link-bad-integrity", integrity: "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=" },
  { name: "guest-priority-spoof", cross: true, cors: true, prioritySpoof: true },
  { name: "head-does-not-register", head: true },
  { name: "blocked-delayed-image", csp: "connect-src 'none'", lifecycleDelay: "image" },
  { name: "blocked-delayed-parser-script", csp: "connect-src 'none'", lifecycleDelay: "parser" },
  { name: "blocked-delayed-defer-script", csp: "connect-src 'none'", lifecycleDelay: "defer" },
];

export async function startDictionaryServers() {
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
  const wire = [],
    sockets = new Set(),
    timers = new Set();
  let primary, peer;
  const handler = (request, response) => {
    const url = new URL(request.url, "https://localhost"),
      id = Number(url.searchParams.get("case") || 0),
      item = dictionaryCases[id];
    if (url.pathname === "/wire") {
      response.setHeader("Content-Type", "application/json");
      response.end(JSON.stringify(wire));
      return;
    }
    const entry = {
      case: id,
      path: url.pathname,
      method: request.method,
      headers: request.headers,
      started: Date.now(),
      finished: null,
      aborted: false,
    };
    wire.push(entry);
    response.on("close", () => {
      entry.aborted = !response.writableFinished;
    });
    if (url.pathname === "/dictionary") {
      response.setHeader(
        "Use-As-Dictionary",
        `match=${JSON.stringify(item.match || "/resource-" + id)}, id="case-${id}"`,
      );
      response.setHeader("Cache-Control", "max-age=600");
      if (item.cors) response.setHeader("Access-Control-Allow-Origin", "*");
      const timer = setTimeout(() => {
        timers.delete(timer);
        entry.finished = Date.now();
        response.end("authored reusable compression dictionary case " + id);
      }, 600);
      timers.add(timer);
      return;
    }
    if (url.pathname === "/delayed-script" || url.pathname === "/delayed-image") {
      const image = url.pathname === "/delayed-image";
      response.setHeader("Content-Type", image ? "image/png" : "application/javascript");
      const timer = setTimeout(() => {
        timers.delete(timer);
        response.end(
          image
            ? Buffer.from(
                "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aF9sAAAAASUVORK5CYII=",
                "base64",
              )
            : "dictionaryEvents.push(['script',Date.now(),document.readyState]);",
        );
      }, 600);
      timers.add(timer);
      return;
    }
    if (url.pathname.startsWith("/resource-")) {
      response.setHeader("Cache-Control", "no-store");
      response.setHeader("Access-Control-Allow-Origin", "*");
      response.end("plain authored resource");
      return;
    }
    if (url.pathname !== "/page") {
      response.end("");
      return;
    }
    response.setHeader("Content-Type", "text/html");
    response.setHeader("Cache-Control", "no-store");
    response.setHeader(
      "Set-Cookie",
      "dictionary_page_cookie=present; Secure; SameSite=None; Path=/",
    );
    if (item.csp) response.setHeader("Content-Security-Policy", item.csp);
    const target = (item.cross ? peer : primary) + "/dictionary?case=" + id;
    if (item.header) response.setHeader("Link", `<${target}>; rel="compression-dictionary"`);
    const link = `<link rel="compression-dictionary" href="${target}"${item.integrity ? ` integrity="${item.integrity}"` : ""}>`;
    const dynamicScript = item.dynamic
      ? `const link=${item.foreign ? "document.createElementNS('http://www.w3.org/2000/svg','link')" : "document.createElement('link')"};link.setAttribute('rel','compression-dictionary');link.setAttribute('href',${JSON.stringify(target)});${item.shadow ? `const host=document.createElement('div');document.body.append(host);host.attachShadow({mode:${JSON.stringify(item.shadow)}}).append(link);link.addEventListener('load',()=>dictionaryEvents.push(['shadow:load',Date.now(),document.readyState]));` : "document.head.append(link);"}${item.remove ? "link.addEventListener('load',()=>dictionaryEvents.push(['detached:load',Date.now(),document.readyState]));setTimeout(()=>link.remove(),50);" : ""}${item.relChange ? "setTimeout(()=>link.rel='alternate',50);" : ""}${item.hrefChange ? `setTimeout(()=>link.href=${JSON.stringify(target + "&version=2")},50);` : ""}`
      : "";
    const delayed =
      item.lifecycleDelay === "image"
        ? `<img src="/delayed-image?case=${id}">`
        : item.lifecycleDelay
          ? `<script src="/delayed-script?case=${id}"${item.lifecycleDelay === "defer" ? " defer" : ""}></script>`
          : "";
    response.end(
      `<!doctype html><head><script>globalThis.dictionaryEvents=[];for(const type of ['DOMContentLoaded','load'])addEventListener(type,()=>dictionaryEvents.push([type,Date.now(),document.readyState]));for(const type of ['load','error'])document.addEventListener(type,e=>{if(e.target.localName==='link')dictionaryEvents.push(['link:'+type,Date.now(),document.readyState])},true);</script>${!item.head && (!item.header || item.both) && !item.dynamic ? link : ""}</head><body><script>${dynamicScript}${item.lifecycleDelay ? "setTimeout(()=>dictionaryEvents.push(['timer',Date.now(),document.readyState]),0);setTimeout(()=>dictionaryEvents.push(['future-timer',Date.now(),document.readyState]),100);" : ""}</script>${delayed}`,
    );
  };
  const servers = [
    createServer({ key, cert: cert.toString() }, handler),
    createServer({ key, cert: cert.toString() }, handler),
  ];
  for (const server of servers) {
    server.on("connection", (socket) => {
      sockets.add(socket);
      socket.on("close", () => sockets.delete(socket));
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  }
  primary = "https://localhost:" + servers[0].address().port;
  peer = "https://localhost:" + servers[1].address().port;
  return {
    primary,
    peer,
    wire,
    spki: createHash("sha256")
      .update(cert.publicKey.export({ type: "spki", format: "der" }))
      .digest("base64"),
    async close() {
      for (const timer of timers) clearTimeout(timer);
      for (const socket of sockets) socket.destroy();
      await Promise.all(servers.map((server) => new Promise((resolve) => server.close(resolve))));
    },
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const fixtures = await startDictionaryServers();
  process.stdout.write(JSON.stringify({ primary: fixtures.primary, peer: fixtures.peer }) + "\n");
  process.stdin.resume();
  process.stdin.on("end", () => {
    fixtures.close().finally(() => process.exit(0));
  });
}
