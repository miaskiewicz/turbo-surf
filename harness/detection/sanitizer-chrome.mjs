// Capture the HTML Sanitizer / declarative shadow DOM / DOMParser controls in an
// independently installed Chrome. Each case serves its own page (with optional
// response headers) on a loopback server and evaluates its probe after load.
// Usage: node harness/detection/sanitizer-chrome.mjs --out harness/detection/fixtures/sanitizer-chrome156.json [--chrome PATH]
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const here = dirname(fileURLToPath(import.meta.url));
export const cases = JSON.parse(readFileSync(join(here, "sanitizer-cases.json"), "utf8"));
const read = (name) => readFileSync(join(here, name), "utf8");

async function capture(executable, out) {
  const routes = new Map(
    cases.map((entry) => [
      `/${entry.name}`,
      {
        page: entry.page
          ? read(entry.page)
          : "<!doctype html><html><head></head><body></body></html>",
        headers: entry.headers || {},
      },
    ]),
  );
  const server = createServer((request, response) => {
    const route = routes.get(request.url);
    if (!route) {
      response.statusCode = 404;
      response.end();
      return;
    }
    response.setHeader("Content-Type", "text/html");
    for (const [name, value] of Object.entries(route.headers)) response.setHeader(name, value);
    response.end(route.page);
  });
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const profile = mkdtempSync(join(tmpdir(), "turbo-sanitizer-oracle-"));
  const chrome = spawn(
    executable,
    [
      "--headless=new",
      "--remote-debugging-port=0",
      `--user-data-dir=${profile}`,
      "--no-first-run",
      "--no-default-browser-check",
      "--disable-extensions",
      "about:blank",
    ],
    { stdio: ["ignore", "ignore", "pipe"] },
  );
  try {
    const endpoint = await new Promise((done, fail) => {
      let text = "";
      chrome.stderr.on("data", (chunk) => {
        text += chunk;
        const match = /DevTools listening on (ws:\/\/\S+)/.exec(text);
        if (match) done(match[1]);
      });
      chrome.once("exit", (code) => fail(Error(`Chrome exited ${code}`)));
    });
    const browser = new WebSocket(endpoint);
    await new Promise((done) => browser.addEventListener("open", done, { once: true }));
    let next = 0;
    const pending = new Map();
    browser.addEventListener("message", (event) => {
      const message = JSON.parse(event.data);
      if (message.id && pending.has(message.id)) {
        const { done, fail } = pending.get(message.id);
        pending.delete(message.id);
        if (message.error) fail(Error(JSON.stringify(message.error)));
        else done(message.result);
      }
    });
    const call = (method, params = {}, sessionId) =>
      new Promise((done, fail) => {
        const id = ++next;
        pending.set(id, { done, fail });
        browser.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
      });
    const version = await call("Browser.getVersion");
    const result = {};
    for (const entry of cases) {
      const { targetId } = await call("Target.createTarget", { url: "about:blank" });
      const { sessionId } = await call("Target.attachToTarget", { targetId, flatten: true });
      await call("Page.enable", {}, sessionId);
      await call(
        "Page.navigate",
        { url: `http://127.0.0.1:${server.address().port}/${entry.name}` },
        sessionId,
      );
      for (let attempt = 0; attempt < 200; attempt++) {
        const state = await call(
          "Runtime.evaluate",
          { expression: "document.readyState", returnByValue: true },
          sessionId,
        );
        if (state.result.value === "complete") break;
        await new Promise((done) => setTimeout(done, 25));
      }
      const evaluated = await call(
        "Runtime.evaluate",
        {
          expression: `(async () => JSON.stringify(await (${read(entry.probe).trim().replace(/;$/, "")})))()`,
          returnByValue: true,
          awaitPromise: true,
        },
        sessionId,
      );
      if (evaluated.exceptionDetails)
        throw Error(
          evaluated.exceptionDetails.exception?.description || evaluated.exceptionDetails.text,
        );
      result[entry.name] = JSON.parse(evaluated.result.value);
      await call("Target.closeTarget", { targetId });
    }
    const files = [
      ...new Set(cases.flatMap((entry) => [entry.probe, entry.page].filter(Boolean))),
    ].sort();
    const oracle = {
      browser: version.product,
      v8: version.jsVersion,
      recordedAt: new Date().toISOString(),
      flags: ["--headless=new", "--disable-extensions"],
      fixtures: Object.fromEntries(
        files.map((name) => [name, createHash("sha256").update(read(name)).digest("hex")]),
      ),
      result,
    };
    writeFileSync(resolve(out), `${JSON.stringify(oracle, null, 2)}\n`);
    console.log(JSON.stringify({ browser: oracle.browser, out: resolve(out) }));
    browser.close();
  } finally {
    const exited = new Promise((done) => chrome.once("exit", done));
    chrome.kill();
    await exited;
    server.closeAllConnections();
    await new Promise((done) => server.close(done));
    rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const { values } = parseArgs({
    options: { out: { type: "string" }, chrome: { type: "string" } },
  });
  if (!values.out) throw Error("Pass --out ORACLE.json");
  const executable = resolve(
    values.chrome ||
      process.env.TURBO_CHROME_PATH ||
      "/Applications/User Apps/Google Chrome.app/Contents/MacOS/Google Chrome",
  );
  if (!existsSync(executable)) throw Error(`Chrome not found at ${executable}`);
  await capture(executable, values.out);
}
