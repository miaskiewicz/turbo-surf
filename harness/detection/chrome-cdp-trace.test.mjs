import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { createSecureServer } from "node:http2";
import { X509Certificate, createPrivateKey } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { summarizeResponseTransport } from "./chrome-network-metadata.mjs";

const chrome = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
test(
  "Chrome capture retains worker and out-of-process iframe requests and sources",
  {
    skip: !existsSync(chrome) || typeof WebSocket === "undefined",
    timeout: 60000,
  },
  async () => {
    const directory = mkdtempSync(join(tmpdir(), "turbo-cdp-fixture-"));
    let port;
    const server = createServer((request, response) => {
      const path = request.url;
      response.setHeader(
        "Content-Type",
        path === "/worker.js" ? "application/javascript" : "text/html",
      );
      if (path === "/worker.js")
        response.end(
          "fetch('/worker-request').then(()=>postMessage('done'));setInterval(()=>{},1000);",
        );
      else if (path === "/frame")
        response.end("<script>fetch('/iframe-request');</script><body>frame</body>");
      else if (path.endsWith("-request")) response.end("fixture response");
      else
        response.end(`<!doctype html><body><div id="consent"><button id="dismiss" onclick="this.parentNode.remove();document.body.setAttribute('data-dismissed','true')">Dismiss</button></div><input name="q" id="q"><script>
      globalThis.__browserFixture={keys:[]};document.querySelector('#q').onkeydown=event=>__browserFixture.keys.push([event.key,event.code]);
      const frame=document.createElement('iframe');frame.src='http://localhost:${port}/frame';
      const frameDone=new Promise(done=>frame.onload=done);document.body.append(frame);
      const worker=new Worker('/worker.js');const workerDone=new Promise(done=>worker.onmessage=done);
      Promise.all([frameDone,workerDone]).then(()=>document.body.setAttribute('data-ready','true'));
    </script></body>`);
    });
    try {
      await new Promise((done, fail) => {
        server.once("error", fail);
        server.listen(0, "127.0.0.1", done);
      });
      port = server.address().port;
      const configPath = join(directory, "config.json");
      writeFileSync(
        configPath,
        JSON.stringify({
          url: `http://127.0.0.1:${port}/`,
          readySelector: "body[data-ready]",
          dismissSelector: "#dismiss",
          steps: [{ focus: "#q" }, { type: "a 1" }],
          successSelector: "body[data-ready]",
          interfaces: true,
          disableExtensions: true,
          debugger: true,
          waitMs: 100,
          interfaceScript: fileURLToPath(new URL("./interface-trace.js", import.meta.url)),
        }),
      );
      const logs = [];
      const code = await new Promise((done, fail) => {
        const child = spawn(
          process.execPath,
          [
            fileURLToPath(new URL("./chrome-cdp-trace.mjs", import.meta.url)),
            configPath,
            directory,
          ],
          { stdio: ["ignore", "pipe", "pipe"] },
        );
        child.stdout.on("data", (chunk) => logs.push(String(chunk)));
        child.stderr.on("data", (chunk) => logs.push(String(chunk)));
        child.on("error", fail);
        child.on("close", done);
      });
      const trace = JSON.parse(readFileSync(join(directory, "trace.json"), "utf8"));
      assert.equal(code, 0, JSON.stringify({ errors: trace.errors, logs }));
      assert.deepEqual(trace.effectiveExtensionFlags, [
        "--disable-extensions",
        "--disable-component-extensions-with-background-pages",
      ]);
      assert.equal(
        trace.pages[0].environment.webdriver,
        false,
        "ordinary headed Chrome must retain its native automation value",
      );
      assert.ok(trace.pages.some((page) => page.phase === "dismiss"));
      assert.match(readFileSync(join(directory, "dismiss.html"), "utf8"), /data-dismissed="true"/);
      assert.doesNotMatch(readFileSync(join(directory, "dismiss.html"), "utf8"), /id="consent"/);
      assert.deepEqual(trace.pages.at(-1).browserFixture.keys, [
        ["a", "KeyA"],
        [" ", "Space"],
        ["1", "Digit1"],
      ]);
      assert.equal(trace.pages.at(-1).inputState.value, "a 1");
      const worker = trace.targets.find((row) => row.type === "worker");
      const frame = trace.targets.find((row) => row.type === "iframe");
      assert.ok(worker, JSON.stringify(trace.targets));
      assert.ok(frame, JSON.stringify(trace.targets));
      assert.ok(
        trace.requests.some(
          (row) => row.url.endsWith("/worker-request") && row.sessionId === worker.sessionId,
        ),
      );
      assert.ok(
        trace.requests.some(
          (row) => row.url.endsWith("/iframe-request") && row.sessionId === frame.sessionId,
        ),
      );
      assert.ok(
        trace.scripts.some(
          (row) => row.url.endsWith("/worker.js") && row.sessionId === worker.sessionId,
        ),
      );
      assert.ok(
        trace.pages
          .at(-1)
          .workers.some((row) =>
            row.interfaces?.accesses.some((access) => access.name.endsWith(".fetch")),
          ),
      );
      assert.equal(
        trace.errors.some((row) => row.phase === "target setup"),
        false,
        JSON.stringify(trace.errors),
      );
      for (const target of [worker, frame]) {
        const request = trace.requests.find(
          (row) => row.sessionId === target.sessionId && row.url.endsWith("-request"),
        );
        const extraInfo = trace.protocolNetwork.filter(
          (row) =>
            row.event === "requestWillBeSentExtraInfo" &&
            row.sessionId === target.sessionId &&
            row.params.requestId === request.protocolRequestId,
        );
        assert.equal(
          request.headerScope,
          extraInfo.length === 1 ? "cdp-extra-info" : "cdp-request-event",
        );
        const response = trace.responses.find(
          (row) => row.sessionId === target.sessionId && row.url.endsWith("-request"),
        );
        assert.equal(response.transport.protocol, "http/1.1");
        assert.equal(response.transport.securityDetails, null);
        assert.equal(typeof response.transport.timing.requestTime, "number");
        assert.ok(
          trace.protocolNetwork.some(
            (row) => row.event === "responseReceived" && row.sessionId === target.sessionId,
          ),
        );
      }
    } finally {
      server.closeAllConnections();
      await new Promise((done) => server.close(done));
      rmSync(directory, { recursive: true, force: true });
    }
  },
);

test(
  "baseline Chrome retains actual HTTP/2 TLS and timing metadata through a redirect without JS tracing",
  { skip: !existsSync(chrome) || typeof WebSocket === "undefined", timeout: 45000 },
  async () => {
    const directory = mkdtempSync(join(tmpdir(), "turbo-cdp-tls-fixture-"));
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
    const sessions = new Set();
    const server = createSecureServer({ key, cert: cert.toString() });
    server.on("session", (session) => {
      sessions.add(session);
      session.on("close", () => sessions.delete(session));
    });
    server.on("stream", (stream, headers) => {
      if (headers[":path"] === "/start") {
        stream.respond({ ":status": 302, location: "/page" });
        stream.end();
      } else {
        stream.respond({ ":status": 200, "content-type": "text/html" });
        stream.end("<!doctype html><body>baseline transport fixture</body>");
      }
    });
    try {
      await new Promise((done, fail) => {
        server.once("error", fail);
        server.listen(0, "127.0.0.1", done);
      });
      const configPath = join(directory, "config.json");
      writeFileSync(
        configPath,
        JSON.stringify({
          url: `https://127.0.0.1:${server.address().port}/start`,
          localCertificate: true,
          disableExtensions: true,
          waitMs: 0,
          steps: [],
          interfaces: false,
          debugger: false,
        }),
      );
      const logs = [];
      const code = await new Promise((done, fail) => {
        const child = spawn(
          process.execPath,
          [
            fileURLToPath(new URL("./chrome-cdp-trace.mjs", import.meta.url)),
            configPath,
            directory,
          ],
          { stdio: ["ignore", "pipe", "pipe"] },
        );
        child.stdout.on("data", (chunk) => logs.push(String(chunk)));
        child.stderr.on("data", (chunk) => logs.push(String(chunk)));
        child.on("error", fail);
        child.on("close", done);
      });
      const trace = JSON.parse(readFileSync(join(directory, "trace.json"), "utf8"));
      assert.equal(code, 0, JSON.stringify({ errors: trace.errors, logs }));
      assert.equal(trace.instrumented, false);
      assert.equal(trace.scripts.length, 0);
      assert.equal(
        trace.pages.every((page) => page.interfaces === null),
        true,
      );
      assert.equal(trace.networkCoverage.enabledInBaseline, true);
      assert.equal(trace.networkCoverage.packetCapture, false);
      const redirect = trace.responses.find((row) => row.redirect && row.url.endsWith("/start"));
      const response = trace.responses.find((row) => row.url.endsWith("/page"));
      for (const path of ["/start", "/page"])
        assert.equal(
          trace.requests.find((row) => row.url.endsWith(path)).headerScope,
          "cdp-extra-info",
        );
      assert.equal(redirect.transport.protocol, "h2");
      assert.equal(response.transport.protocol, "h2");
      assert.match(response.transport.securityDetails.protocol, /^TLS 1\.[23]$/);
      assert.equal(typeof response.transport.securityDetails.cipher, "string");
      assert.equal(typeof response.transport.timing.requestTime, "number");
      assert.ok(response.transport.timing.receiveHeadersEnd >= response.transport.timing.sendStart);
      assert.equal(response.transport.remoteIPAddress, "127.0.0.1");
      assert.equal(response.transport.remotePort, server.address().port);
      assert.ok(
        trace.protocolNetwork.some(
          (row) => row.event === "responseReceived" && row.params.response.protocol === "h2",
        ),
      );
      assert.ok(trace.protocolNetwork.some((row) => row.event === "loadingFinished"));
      const summary = summarizeResponseTransport(trace.responses);
      assert.ok(summary.protocols.h2 >= 2);
      assert.equal(summary.unknown, 0);
      assert.ok(summary.securityDetails.present >= 2);
    } finally {
      for (const session of sessions) session.destroy();
      await new Promise((done) => server.close(done));
      rmSync(directory, { recursive: true, force: true });
    }
  },
);

test(
  "Patchright baseline collects main-session response metadata without interface or debugger tracing",
  {
    skip:
      !existsSync(chrome) ||
      !existsSync(
        new URL(
          "../../scripts/browser-sidecar/node_modules/patchright/package.json",
          import.meta.url,
        ),
      ),
    timeout: 45000,
  },
  async () => {
    const directory = mkdtempSync(join(tmpdir(), "turbo-patchright-network-fixture-"));
    const server = createServer((request, response) => {
      if (request.url === "/start") {
        response.writeHead(302, { location: "/page" });
        response.end();
      } else if (request.url === "/worker.js") {
        response.setHeader("Content-Type", "application/javascript");
        response.end("fetch('/shared').then(()=>postMessage('done'));setInterval(()=>{},1000)");
      } else if (request.url === "/frame") {
        response.setHeader("Content-Type", "text/html");
        response.end(
          `<body><script>fetch('http://127.0.0.1:${server.address().port}/shared').then(()=>parent.postMessage('done','*'))</script>`,
        );
      } else if (request.url === "/shared") {
        response.setHeader("Access-Control-Allow-Origin", "*");
        response.end("fixture shared response");
      } else {
        response.setHeader("Content-Type", "text/html");
        response.end(`<!doctype html><body><script>
          const worker = new Worker('/worker.js');
          const workerDone = new Promise(done=>worker.onmessage=done);
          const frameDone = new Promise(done=>addEventListener('message',done,{once:true}));
          const frame = document.createElement('iframe');
          frame.src='http://localhost:${server.address().port}/frame';document.body.append(frame);
          Promise.all([fetch('/shared'),workerDone,frameDone]).then(()=>document.body.dataset.ready='true');
        </script>Patchright baseline fixture</body>`);
      }
    });
    try {
      await new Promise((done, fail) => {
        server.once("error", fail);
        server.listen(0, "127.0.0.1", done);
      });
      const configPath = join(directory, "config.json");
      writeFileSync(
        configPath,
        JSON.stringify({
          chromeBackend: "patchright",
          url: `http://127.0.0.1:${server.address().port}/start`,
          waitMs: 0,
          steps: [],
          interfaces: false,
          debugger: false,
          readySelector: "body[data-ready]",
        }),
      );
      const logs = [];
      const code = await new Promise((done, fail) => {
        const child = spawn(
          process.execPath,
          [fileURLToPath(new URL("./chrome-trace.mjs", import.meta.url)), configPath, directory],
          { stdio: ["ignore", "pipe", "pipe"] },
        );
        child.stdout.on("data", (chunk) => logs.push(String(chunk)));
        child.stderr.on("data", (chunk) => logs.push(String(chunk)));
        child.on("error", fail);
        child.on("close", done);
      });
      const trace = JSON.parse(readFileSync(join(directory, "trace.json"), "utf8"));
      assert.equal(code, 0, JSON.stringify({ errors: trace.errors, logs }));
      assert.equal(trace.instrumented, false);
      assert.equal(trace.scripts.length, 0);
      assert.equal(trace.networkCoverage.scope, "main-page-session");
      assert.equal(
        trace.pages.every((page) => page.interfaces === null),
        true,
      );
      for (const path of ["/start", "/page"]) {
        const response = trace.responses.find((row) => row.url.endsWith(path));
        assert.equal(
          trace.requests.find((row) => row.id === response.requestId).headerScope,
          "cdp-extra-info",
        );
        assert.equal(
          response.transport.protocol,
          "http/1.1",
          JSON.stringify({
            path,
            transport: response.transport,
            requestScope: trace.requests.find((row) => row.id === response.requestId)
              ?.cdpTargetScope,
          }),
        );
        assert.equal(response.transport.securityDetails, null);
        assert.equal(response.transport.association, "main-frame-navigation-unique-url-method");
      }
      assert.ok(trace.protocolNetwork.some((row) => row.event === "responseReceived"));
      const sharedRequests = trace.requests.filter((row) => row.url.endsWith("/shared"));
      assert.equal(sharedRequests.length, 3);
      assert.equal(
        sharedRequests.filter((row) => row.cdpTargetScope === "main-frame-navigation").length,
        0,
      );
      for (const request of sharedRequests) {
        assert.equal(request.headerScope, "automation-api");
        const response = trace.responses.find((row) => row.requestId === request.id);
        assert.equal(response.transport.source, "unavailable");
      }
      assert.ok(
        trace.protocolNetwork.some(
          (row) => row.event === "responseReceived" && row.params.response.url.endsWith("/shared"),
        ),
      );
    } finally {
      server.closeAllConnections();
      await new Promise((done) => server.close(done));
      rmSync(directory, { recursive: true, force: true });
    }
  },
);
