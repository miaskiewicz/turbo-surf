// Measure genuine Chrome WindowProxy operations using localhost origins.
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { writeFileSync, mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, resolve } from "node:path";
const here = fileURLToPath(new URL(".", import.meta.url));
const out = resolve(process.argv[2] || join(here, "artifacts", `window-operations-${Date.now()}`));
mkdirSync(out, { recursive: true, mode: 0o700 });
const require = createRequire(join(here, "../../scripts/browser-sidecar/package.json"));
const { chromium } = require("patchright");
const server = createServer((q, r) =>
  r.end(
    q.url.startsWith("/child")
      ? '<script>parent.postMessage("loaded","*")</script>'
      : "<body></body>",
  ),
);
const other = createServer((q, r) => r.end('<script>parent.postMessage("loaded","*")</script>'));
await Promise.all([
  new Promise((r) => server.listen(0, "127.0.0.1", r)),
  new Promise((r) => other.listen(0, "127.0.0.1", r)),
]);
let browser;
try {
  browser = await chromium.launch({ channel: "chrome", headless: true });
  const page = await browser.newPage();
  const results = [];
  for (const cross of [false, true]) {
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    results.push(
      await page.evaluate(
        ({ port, cross }) =>
          new Promise((resolve) => {
            const frame = document.createElement("iframe");
            document.body.appendChild(frame);
            const proxy = frame.contentWindow,
              doc = proxy.document,
              ctor = proxy.Function,
              code = ctor("return document"),
              globalCode = ctor("return globalThis.document");
            onmessage = () => {
              const output = { cross, sameProxy: proxy === frame.contentWindow };
              const operations = {
                compile: () => typeof ctor("return document"),
                runDocument: () => ctor("return document")() === doc,
                runGlobalDocument: () => ctor("return globalThis.document")() === doc,
                runCompiledDocument: () => code() === doc,
                runCompiledGlobalDocument: () => globalCode() === doc,
                oldDocument: () => doc.body !== null,
                prototype: () => Object.getPrototypeOf(proxy) === null,
                prototypeKind: () => Object.prototype.toString.call(Object.getPrototypeOf(proxy)),
                setPrototype: () => Object.setPrototypeOf(proxy, {}) === proxy,
                descriptor: () => !!Object.getOwnPropertyDescriptor(proxy, "document"),
                ownKeys: () => (cross ? Reflect.ownKeys(proxy).map((k) => String(k)) : null),
                hasDocument: () => "document" in proxy,
                deleteDocument: () => delete proxy.document,
              };
              for (const [key, op] of Object.entries(operations)) {
                try {
                  output[key] = op();
                } catch (e) {
                  output[key] = { error: e.name, message: e.message };
                }
              }
              resolve(output);
            };
            frame.src = `http://127.0.0.1:${port}/child`;
          }),
        { port: (cross ? other : server).address().port, cross },
      ),
    );
  }
  writeFileSync(
    join(out, "chrome-window-operations.json"),
    JSON.stringify({ version: await browser.version(), results }, null, 2),
    { mode: 0o600 },
  );
  console.log(JSON.stringify({ out, results }));
} finally {
  await browser?.close();
  server.close();
  other.close();
}
