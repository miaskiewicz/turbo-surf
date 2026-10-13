// Observe actual Chrome sandbox origins, script gating and data-document access.
import { createServer } from "node:http";
import { createRequire } from "node:module";
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
const here = fileURLToPath(new URL(".", import.meta.url));
const out = resolve(process.argv[2] || join(here, "artifacts", `frame-policy-${Date.now()}`));
mkdirSync(out, { recursive: true, mode: 0o700 });
const { chromium } = createRequire(join(here, "../../scripts/browser-sidecar/package.json"))(
  "patchright",
);
const server = createServer((q, r) => {
  r.setHeader("Content-Type", "text/html");
  r.end("<body></body>");
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
let browser;
try {
  browser = await chromium.launch({ channel: "chrome", headless: true });
  const page = await browser.newPage();
  await page.goto(`http://127.0.0.1:${server.address().port}/parent`);
  const result = await page.evaluate(async () => {
    // This fixture measures the native document.cookie security boundary.
    // oxlint-disable-next-line unicorn/no-document-cookie
    document.cookie = "oracle_cookie=1; path=/";
    const output = [];
    for (const sandbox of [
      null,
      "",
      "allow-scripts",
      "allow-same-origin",
      "allow-scripts allow-same-origin",
    ]) {
      const iframe = document.createElement("iframe");
      if (sandbox !== null) iframe.setAttribute("sandbox", sandbox);
      const messages = [];
      const receive = (e) => {
        if (e.source === iframe.contentWindow) messages.push({ origin: e.origin, data: e.data });
      };
      addEventListener("message", receive);
      iframe.srcdoc = `<body><script>window.didRun=true;const capture=fn=>{try{return {value:fn()}}catch(e){return {error:e.name,code:e.code}}};parent.postMessage({origin:origin,locationOrigin:location.origin,url:document.URL,parent: capture(()=>parent.document.URL),cookie:capture(()=>document.cookie),storage:capture(()=>localStorage.length)},'*')</script></body>`;
      const loaded = new Promise((r) => iframe.addEventListener("load", r, { once: true }));
      document.body.append(iframe);
      const initial = { document: iframe.contentDocument !== null };
      try {
        initial.eval = iframe.contentWindow.eval("1+1");
      } catch (e) {
        initial.eval = e.name;
      }
      await loaded;
      await new Promise((r) => setTimeout(r, 40));
      const row = { sandbox, initial, document: iframe.contentDocument !== null, messages };
      if (iframe.contentDocument) row.didRun = iframe.contentWindow.didRun === true;
      output.push(row);
      iframe.remove();
      removeEventListener("message", receive);
    }
    const data = document.createElement("iframe");
    const messages = [];
    addEventListener("message", (e) => {
      if (e.source === data.contentWindow) messages.push({ origin: e.origin, data: e.data });
    });
    data.src =
      "data:text/html," +
      encodeURIComponent(
        `<script>parent.postMessage({origin:origin,locationOrigin:location.origin,url:document.URL},'*')</script>`,
      );
    const loaded = new Promise((r) => data.addEventListener("load", r, { once: true }));
    document.body.append(data);
    await loaded;
    await new Promise((r) => setTimeout(r, 40));
    return { sandbox: output, data: { document: data.contentDocument !== null, messages } };
  });
  writeFileSync(
    join(out, "chrome-frame-policy.json"),
    JSON.stringify({ version: await browser.version(), ...result }, null, 2),
    { mode: 0o600 },
  );
  console.log(JSON.stringify({ out, ...result }));
} finally {
  await browser?.close();
  server.close();
}
