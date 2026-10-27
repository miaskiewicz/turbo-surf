import { test } from "node:test";
import assert from "node:assert/strict";
import {
  appendFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:http";
import { createSecureServer } from "node:http2";
import { X509Certificate, createPrivateKey } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { summarizeResponseTransport } from "./chrome-network-metadata.mjs";
import { chromeSearchOutcome, observationSummary } from "./compare.mjs";
import { superviseTestChild } from "./child-supervision.mjs";

const chrome =
  process.env.TURBO_SURF_CHROME_PATH ||
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

async function collect(t, collector, configPath, output) {
  mkdirSync(output, { recursive: true, mode: 0o700 });
  for (const name of ["stdout", "stderr"])
    writeFileSync(join(output, `${name}.log`), "", { mode: 0o600 });
  const result = await superviseTestChild(
    t,
    process.execPath,
    [fileURLToPath(new URL(collector, import.meta.url)), configPath, output],
    {
      timeoutMs: 45000,
      onOutput: (name, bytes) => appendFileSync(join(output, `${name}.log`), bytes),
    },
  );
  assert.equal(result.timedOut, false, JSON.stringify(result));
  assert.equal(result.aborted, false, JSON.stringify(result));
  assert.equal(result.error, null, JSON.stringify(result));
  return result;
}

function disposeEvidence(t, directory, passed) {
  if (passed) rmSync(directory, { recursive: true, force: true });
  else t.diagnostic(`Failed Chrome fixture evidence retained at ${directory}`);
}
test(
  "Patchright status follows the current loader when a 429 document reuses an old 200 URL",
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
  async (t) => {
    const directory = mkdtempSync(join(tmpdir(), "turbo-patchright-loader-fixture-"));
    let passed = false;
    const server = createServer((request, response) => {
      const challenge = request.url === "/challenge";
      response.writeHead(challenge ? 429 : 200, { "content-type": "text/html" });
      response.end(
        challenge
          ? '<!doctype html><body data-ready><div id="search"><a href="/result"><h3>Challenge fixture marker</h3></a></div><script>history.replaceState(null,"","/search");</script></body>'
          : '<!doctype html><body><script>location.replace("/challenge");</script></body>',
      );
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
          chromePath: chrome,
          chromeBackend: "patchright",
          url: `http://127.0.0.1:${server.address().port}/search`,
          waitMs: 50,
          steps: [],
          interfaces: false,
          debugger: false,
          readySelector: "body[data-ready]",
        }),
      );
      const result = await collect(t, "./chrome-trace.mjs", configPath, directory);
      const code = result.code;
      const trace = JSON.parse(readFileSync(join(directory, "trace.json"), "utf8"));
      assert.equal(code, 0, JSON.stringify(trace.errors));
      assert.equal(trace.captureComplete, true);
      assert.deepEqual(trace.collectorErrors, []);
      const final = trace.pages.at(-1);
      assert.equal(new URL(final.url).pathname, "/search");
      assert.equal(new URL(final.statusDocumentUrl).pathname, "/challenge");
      assert.equal(final.results, 1);
      assert.equal(final.readyState, "complete");
      assert.equal(final.statusSource, "cdp-main-document-loader");
      assert.equal(final.status, 429);
      assert.equal(chromeSearchOutcome(trace).accepted, false);
      assert.equal(chromeSearchOutcome(trace).classification, "challenge");
      assert.ok(
        trace.responses.some(
          (response) => response.status === 200 && new URL(response.url).pathname === "/search",
        ),
      );
      passed = true;
    } finally {
      server.closeAllConnections();
      await new Promise((done) => server.close(done));
      disposeEvidence(t, directory, passed);
    }
  },
);

test("Chrome capture startup failures remain incomplete without launching a browser", async (t) => {
  const directory = mkdtempSync(join(tmpdir(), "turbo-cdp-startup-failure-"));
  let passed = false;
  try {
    const configPath = join(directory, "config.json");
    writeFileSync(
      configPath,
      JSON.stringify({ chromePath: join(directory, "missing-chrome"), interfaces: false }),
    );
    const result = await collect(t, "./chrome-cdp-trace.mjs", configPath, directory);
    const code = result.code;
    const trace = JSON.parse(readFileSync(join(directory, "trace.json"), "utf8"));
    assert.equal(code, 1);
    assert.equal(trace.captureComplete, false);
    assert.equal(trace.pages.length, 0);
    assert.equal(trace.collectorErrors.length, 1);
    assert.equal(trace.collectorErrors[0].phase, "capture");
    assert.equal(trace.collectorErrors[0].stage, "launch");
    assert.deepEqual(trace.errors, trace.collectorErrors);
    passed = true;
  } finally {
    disposeEvidence(t, directory, passed);
  }
});

test(
  "baseline Chrome distinguishes a result document, an HTTP 200 JS shell and an HTTP 429 challenge",
  { skip: !existsSync(chrome) || typeof WebSocket === "undefined", timeout: 60000 },
  async (t) => {
    const directory = mkdtempSync(join(tmpdir(), "turbo-cdp-verdict-fixture-"));
    let passed = false;
    const server = createServer((request, response) => {
      const url = new URL(request.url, "http://fixture.test");
      const challenge = url.pathname === "/sorry/index";
      response.writeHead(challenge ? 429 : 200, { "content-type": "text/html" });
      const fixture = url.searchParams.get("fixture");
      const result = ["results", "history"].includes(fixture);
      const redirect = fixture === "redirect-challenge";
      response.end(`<!doctype html><body data-ready data-case="${redirect ? "redirecting" : "terminal"}">
        <input type="hidden" name="q" value="fixture-hidden-before">
        ${challenge ? "" : '<textarea name="q">fixture query</textarea>'}
        <input type="hidden" name="q" value="fixture-hidden-after">
        ${result ? '<div id="rso"><a href="/result"><h3>Authored result</h3></a></div>' : ""}
        <script>
          ${fixture === "history" ? "history.replaceState(null,'','/search?fixture=history');" : ""}
          ${redirect ? "location.replace('/sorry/index');" : ""}
          ${result ? "throw new ReferenceError('completed-verdict-guest-fixture');" : "document.body.dataset.executed='true';"}
        </script>
      </body>`);
    });
    try {
      await new Promise((done, fail) => {
        server.once("error", fail);
        server.listen(0, "127.0.0.1", done);
      });
      const origin = `http://127.0.0.1:${server.address().port}`;
      for (const [name, path, accepted] of [
        ["results", "/search?fixture=results", true],
        ["history", "/bootstrap?fixture=history", true],
        ["shell", "/search?fixture=shell", false],
        ["challenge", "/sorry/index", false],
        ["redirected-challenge", "/bootstrap?fixture=redirect-challenge", false],
      ]) {
        const output = join(directory, name);
        const configPath = join(directory, `${name}.json`);
        writeFileSync(
          configPath,
          JSON.stringify({
            chromePath: chrome,
            url: origin + path,
            interfaces: false,
            debugger: false,
            disableExtensions: true,
            readySelector: 'body[data-case="terminal"]',
            steps: [],
            waitMs: 50,
          }),
        );
        const result = await collect(t, "./chrome-cdp-trace.mjs", configPath, output);
        const code = result.code;
        const trace = JSON.parse(readFileSync(join(output, "trace.json"), "utf8"));
        assert.equal(code, 0, JSON.stringify(trace.errors));
        assert.equal(trace.captureComplete, true);
        assert.deepEqual(trace.collectorErrors, []);
        const page = trace.pages.at(-1);
        const challenge = name.endsWith("challenge");
        assert.equal(page.status, challenge ? 429 : 200);
        assert.equal(page.statusSource, "cdp-main-document-loader");
        assert.equal(
          new URL(page.statusDocumentUrl).pathname,
          challenge ? "/sorry/index" : new URL(origin + path).pathname,
        );
        assert.equal(page.readyState, "complete");
        assert.equal(page.results, accepted ? 1 : 0);
        assert.equal(page.inputState.excludedHiddenQueryControls, 2);
        assert.equal(page.inputState.value, challenge ? null : "fixture query");
        const outcome = chromeSearchOutcome(trace);
        assert.equal(outcome.accepted, accepted, JSON.stringify(outcome));
        if (accepted) {
          assert.equal(
            trace.errors.some((error) => String(error).includes("completed-verdict-guest-fixture")),
            true,
          );
          assert.equal(outcome.classification, "serp");
        }
        if (challenge) assert.equal(outcome.classification, "challenge");
      }
      passed = true;
    } finally {
      server.closeAllConnections();
      await new Promise((done) => server.close(done));
      disposeEvidence(t, directory, passed);
    }
  },
);

test(
  "Chrome capture retains worker and out-of-process iframe requests and sources",
  {
    skip: !existsSync(chrome) || typeof WebSocket === "undefined",
    timeout: 60000,
  },
  async (t) => {
    const directory = mkdtempSync(join(tmpdir(), "turbo-cdp-fixture-"));
    let passed = false;
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
        response.end(`<!doctype html><body><div id="consent"><button id="dismiss" onclick="this.parentNode.remove();document.body.setAttribute('data-dismissed','true')">Dismiss</button></div><input type="hidden" name="q" value="fixture-hidden-before"><input name="q" id="q"><input type="hidden" name="q" value="fixture-hidden-after"><script>
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
          chromePath: chrome,
          url: `http://127.0.0.1:${port}/`,
          readySelector: "body[data-ready]",
          dismissSelector: "#dismiss",
          steps: [{ focus: "#q" }, { type: "a 1" }],
          successSelector: "body[data-ready]",
          interfaces: true,
          disableExtensions: true,
          // Keyboard-driven: headless keeps host keystrokes out of #q (turbo-surf#52).
          headless: true,
          debugger: true,
          waitMs: 100,
          interfaceScript: fileURLToPath(new URL("./interface-trace.js", import.meta.url)),
        }),
      );
      const result = await collect(t, "./chrome-cdp-trace.mjs", configPath, directory);
      const code = result.code;
      const trace = JSON.parse(readFileSync(join(directory, "trace.json"), "utf8"));
      assert.equal(code, 0, JSON.stringify({ errors: trace.errors, process: result }));
      assert.equal(trace.captureComplete, true);
      assert.deepEqual(trace.collectorErrors, []);
      assert.deepEqual(trace.effectiveExtensionFlags, [
        "--disable-extensions",
        "--disable-component-extensions-with-background-pages",
        "--headless=new",
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
      assert.equal(trace.pages.at(-1).inputState.controlType, "text");
      assert.equal(trace.pages.at(-1).inputState.excludedHiddenQueryControls, 2);
      assert.equal(trace.pages.at(-1).inputState.queryControlCount, 3);
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
      passed = true;
    } finally {
      server.closeAllConnections();
      await new Promise((done) => server.close(done));
      disposeEvidence(t, directory, passed);
    }
  },
);

test(
  "baseline Chrome retains actual HTTP/2 TLS and timing metadata through a redirect without JS tracing",
  { skip: !existsSync(chrome) || typeof WebSocket === "undefined", timeout: 45000 },
  async (t) => {
    const directory = mkdtempSync(join(tmpdir(), "turbo-cdp-tls-fixture-"));
    let passed = false;
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
        stream.end(
          `<!doctype html><body><input type="hidden" name="q" value="fixture-hidden-before"><textarea name="q">baseline query</textarea><input type="hidden" name="q" value="fixture-hidden-after"><script>throw new ReferenceError('completed-capture-guest-fixture');</script>baseline transport fixture</body>`,
        );
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
          chromePath: chrome,
          url: `https://127.0.0.1:${server.address().port}/start`,
          localCertificate: true,
          disableExtensions: true,
          waitMs: 0,
          steps: [],
          interfaces: false,
          debugger: false,
        }),
      );
      const result = await collect(t, "./chrome-cdp-trace.mjs", configPath, directory);
      const code = result.code;
      const trace = JSON.parse(readFileSync(join(directory, "trace.json"), "utf8"));
      assert.equal(code, 0, JSON.stringify({ errors: trace.errors, process: result }));
      assert.equal(trace.instrumented, false);
      assert.equal(trace.captureComplete, true);
      assert.deepEqual(trace.collectorErrors, []);
      assert.equal(
        trace.errors.some(
          (error) => typeof error === "string" && error.includes("completed-capture-guest-fixture"),
        ),
        true,
      );
      assert.equal(trace.pages.at(-1).inputState.value, "baseline query");
      assert.equal(trace.pages.at(-1).inputState.controlType, "textarea");
      assert.equal(trace.pages.at(-1).inputState.excludedHiddenQueryControls, 2);
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
      passed = true;
    } finally {
      for (const session of sessions) session.destroy();
      await new Promise((done) => server.close(done));
      disposeEvidence(t, directory, passed);
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
  async (t) => {
    const directory = mkdtempSync(join(tmpdir(), "turbo-patchright-network-fixture-"));
    let passed = false;
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
          chromePath: chrome,
          chromeBackend: "patchright",
          url: `http://127.0.0.1:${server.address().port}/start`,
          waitMs: 0,
          steps: [],
          interfaces: false,
          debugger: false,
          readySelector: "body[data-ready]",
        }),
      );
      const result = await collect(t, "./chrome-trace.mjs", configPath, directory);
      const code = result.code;
      const trace = JSON.parse(readFileSync(join(directory, "trace.json"), "utf8"));
      assert.equal(code, 0, JSON.stringify({ errors: trace.errors, process: result }));
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
      passed = true;
    } finally {
      server.closeAllConnections();
      await new Promise((done) => server.close(done));
      disposeEvidence(t, directory, passed);
    }
  },
);

for (const backend of ["cdp", "patchright"]) {
  test(
    `${backend} Chrome records a rootless post-Enter document without borrowing earlier results`,
    {
      skip:
        !existsSync(chrome) ||
        typeof WebSocket === "undefined" ||
        (backend === "patchright" &&
          !existsSync(
            new URL(
              "../../scripts/browser-sidecar/node_modules/patchright/package.json",
              import.meta.url,
            ),
          )),
      timeout: 45000,
      ...(backend === "patchright" ? { todo: "turbo-surf#52" } : {}),
    },
    async (t) => {
      const directory = mkdtempSync(join(tmpdir(), `turbo-${backend}-rootless-fixture-`));
      let passed = false;
      const server = createServer((request, response) => {
        const terminal = new URL(request.url, "http://fixture.test").pathname === "/search";
        response.writeHead(terminal ? 429 : 200, { "content-type": "text/html" });
        response.end(
          terminal
            ? '<!doctype html><html><body><script>addEventListener("DOMContentLoaded",()=>document.removeChild(document.documentElement));</script></body></html>'
            : '<!DOCTYPE html PUBLIC "fixture-public" "fixture-system"><html><body data-ready><div id="rso"><a><h3>Old result</h3></a></div><form action="/search"><input id="q" name="q"></form></body></html>',
        );
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
            chromePath: chrome,
            chromeBackend: backend,
            url: `http://127.0.0.1:${server.address().port}/`,
            readySelector: "body[data-ready]",
            interfaces: false,
            debugger: false,
            disableExtensions: true,
            // A headed Chrome takes the OS keyboard focus, and keys typed on the host
            // then corrupt the trusted query (turbo-surf#52).
            headless: true,
            observationMs: 100,
            steps: [{ focus: "#q" }, { type: "root fixture" }, { press: "Enter" }],
          }),
        );
        const result = await collect(
          t,
          backend === "cdp" ? "./chrome-cdp-trace.mjs" : "./chrome-trace.mjs",
          configPath,
          directory,
        );
        const trace = JSON.parse(readFileSync(join(directory, "trace.json"), "utf8"));
        assert.equal(result.code, 0, JSON.stringify(trace.collectorErrors));
        assert.equal(trace.captureComplete, true);
        assert.deepEqual(trace.collectorErrors, []);
        if (backend === "cdp")
          assert.deepEqual(trace.effectiveExtensionFlags, [
            "--disable-extensions",
            "--disable-component-extensions-with-background-pages",
            "--headless=new",
          ]);
        else assert.equal(trace.headless, true);
        assert.equal(trace.pages[0].results, 1);
        assert.equal(trace.pages[0].status, 200);
        assert.equal(trace.pages[0].documentElementPresent, true);
        assert.ok(
          readFileSync(join(directory, "home.html"), "utf8").startsWith(
            backend === "patchright"
              ? '<!DOCTYPE html PUBLIC "fixture-public" "fixture-system">'
              : "<!DOCTYPE html>",
          ),
        );
        const final = trace.pages.at(-1);
        assert.equal(final.documentElementPresent, false);
        assert.equal(final.results, 0);
        assert.equal(final.bytes, 0);
        assert.equal(readFileSync(join(directory, "search.html"), "utf8"), "");
        assert.equal(new URL(final.url).pathname, "/search");
        assert.equal(new URL(final.statusDocumentUrl).pathname, "/search");
        assert.equal(final.statusSource, "cdp-main-document-loader");
        assert.equal(final.status, 429);
        assert.equal(trace.observation.requestedMs, 100);
        assert.equal(trace.observation.documentElementPresent, false);
        assert.equal(trace.observation.completed, false);
        assert.equal(trace.observation.pendingNavigation, false);
        assert.equal(trace.observation.navigationFailed, false);
        assert.equal(observationSummary(trace).complete, false);
        assert.equal(chromeSearchOutcome(trace).accepted, false);
        passed = true;
      } finally {
        server.closeAllConnections();
        await new Promise((done) => server.close(done));
        disposeEvidence(t, directory, passed);
      }
    },
  );
}
