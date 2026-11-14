// Chromium control for retained frame WindowProxy and retired document authority.
import { createServer } from "node:http";
import { writeFile } from "node:fs/promises";
import { chromium } from "playwright";
const servers = [];
async function server(body) {
  const s = createServer((_q, r) => {
    r.setHeader("content-type", "text/html");
    r.end(body);
  });
  await new Promise((r) => s.listen(0, "127.0.0.1", r));
  servers.push(s);
  return `http://127.0.0.1:${s.address().port}`;
}
const parent = await server("<body></body>");
const child = await server(
  "<body data-secret=remote><script>window.remoteSecret='remote';parent.postMessage({loaded:true},'*');</script></body>",
);
const browser = await chromium.launch({
  headless: true,
  ...(process.env.CHROME_EXECUTABLE ? { executablePath: process.env.CHROME_EXECUTABLE } : {}),
});
try {
  const page = await browser.newPage();
  await page.goto(parent);
  const observations = await page.evaluate(async (url) => {
    const f = document.createElement("iframe");
    document.body.appendChild(f);
    const oldWindow = f.contentWindow,
      oldDocument = oldWindow.document,
      oldFunction = oldWindow.Function;
    oldDocument.body.setAttribute("data-retained", "old");
    oldWindow.setTimeout(() => parent.document.body.setAttribute("data-old-timer", "ran"), 100000);
    await new Promise((resolve) => {
      addEventListener(
        "message",
        (e) => {
          if (e.data.loaded) resolve();
        },
        { once: true },
      );
      f.src = url;
    });
    const current = f.contentWindow,
      rows = {};
    const capture = (name, read) => {
      try {
        const value = read();
        rows[name] = { ok: true, value: value === undefined ? "undefined" : value };
      } catch (e) {
        rows[name] = { ok: false, name: e.name, message: e.message };
      }
    };
    capture("identity", () => oldWindow === current);
    capture("currentDocument", () => current.document.body !== null);
    capture("oldWindowDocument", () => oldWindow.document === oldDocument);
    capture("oldDocumentDefaultView", () => oldDocument.defaultView === oldWindow);
    capture("oldDocumentDefaultViewNull", () => oldDocument.defaultView === null);
    capture("retainedOldDocument", () => oldDocument.body.getAttribute("data-retained"));
    capture("retiredLiteral", () => oldFunction("return 73")());
    capture(
      "retiredChildDirect",
      () =>
        oldDocument.body.appendChild(oldDocument.createElement("iframe")).contentWindow === null,
    );
    capture("oldFunctionGlobalIdentity", () => oldFunction("return globalThis")() === oldWindow);
    capture("oldFunctionRemoteSecret", () => oldFunction("return globalThis.remoteSecret")());
    capture("oldFunctionGlobalDocument", () =>
      oldFunction('return globalThis.document.body.getAttribute("data-secret")')(),
    );
    capture("oldFunctionDirectDocument", () =>
      oldFunction('return document.body.getAttribute("data-secret")')(),
    );
    capture(
      "retiredChild",
      () =>
        oldFunction(
          "return document.body.appendChild(document.createElement('iframe')).contentWindow",
        )() === null,
    );
    oldDocument.body.setAttribute("data-mutated", "old-again");
    const script = oldDocument.createElement("script");
    script.textContent = "parent.document.body.setAttribute('data-old-script','ran')";
    oldDocument.body.appendChild(script);
    rows.oldTimerRan = document.body.hasAttribute("data-old-timer");
    rows.oldScriptRan = document.body.hasAttribute("data-old-script");
    return rows;
  }, child);
  const receipt = { version: browser.version(), parent, child, observations };
  await writeFile(
    new URL("../fixtures/frame-retired-window-chrome158.json", import.meta.url),
    JSON.stringify(receipt, null, 2),
  );
  console.log(JSON.stringify(receipt));
} finally {
  await browser.close();
  for (const s of servers) await new Promise((r) => s.close(r));
}
