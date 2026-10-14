import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

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
    } finally {
      server.closeAllConnections();
      await new Promise((done) => server.close(done));
      rmSync(directory, { recursive: true, force: true });
    }
  },
);
