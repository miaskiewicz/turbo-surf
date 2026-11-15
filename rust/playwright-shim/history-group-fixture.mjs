import assert from "node:assert/strict";
import { createServer } from "node:http";
import { after } from "node:test";

let browser;
const chrome = process.env.TURBO_SURF_HISTORY_CHROME === "1";
let createContext;
if (chrome) {
  const { chromium } = await import("playwright");
  browser = await chromium.launch({
    headless: true,
    ...(process.env.TURBO_SURF_HISTORY_CHROME_EXECUTABLE
      ? { executablePath: process.env.TURBO_SURF_HISTORY_CHROME_EXECUTABLE }
      : {}),
  });
  createContext = (options) => browser.newContext(options);
  after(() => browser.close());
} else {
  let BrowserContext;
  try {
    ({ BrowserContext } = await import(process.env.TURBO_SURF_TEST_SHIM ?? "./index.mjs"));
  } catch (error) {
    if (!String(error).includes("no prebuilt binary")) throw error;
    console.log("shim: native addon not built, skipping —", error.message);
    process.exit(0);
  }
  createContext = (options) => new BrowserContext(options);
}

export async function historyFixture(run, { isolated = false, contextOptions = {} } = {}) {
  const requests = [];
  const server = createServer((request, response) => {
    requests.push(request.url);
    response.setHeader("cache-control", "no-store");
    response.setHeader("content-type", "text/html");
    if (isolated) {
      response.setHeader("cross-origin-opener-policy", "same-origin");
      response.setHeader("cross-origin-embedder-policy", "require-corp");
    }
    response.end(
      '<body><script>window.marker={};window.popEvents=[];addEventListener("popstate",()=>popEvents.push(location.pathname));</script></body>',
    );
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const base = `http://127.0.0.1:${server.address().port}`;
  const context = await createContext(contextOptions);
  const root = await context.newPage();
  try {
    await root.goto(`${base}/host`, { waitUntil: "networkidle", timeout: 5000 });
    await run({ context, root, base, requests });
  } finally {
    await context.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

export async function popup(root, path = "/one", name = "history-case") {
  const opened = root.waitForEvent("popup", { timeout: 5000 });
  opened.catch(() => {});
  await root.evaluate(
    ({ path, name }) => {
      window[name] = window.open(path, name);
    },
    { path, name },
  );
  const page = await opened;
  await page.waitForLoadState("load", { timeout: 5000 });
  return page;
}

export async function assign(page, path) {
  const loaded = page.waitForEvent("load", { timeout: 5000 });
  loaded.catch(() => {});
  await Promise.all([loaded, page.evaluate((path) => location.assign(path), path)]);
}

export async function guestTraversal(page, method, delta) {
  const loaded = page.waitForEvent("load", { timeout: 5000 });
  loaded.catch(() => {});
  await Promise.all([
    loaded,
    page.evaluate(({ method, delta }) => history[method](delta), { method, delta }),
  ]);
}

export async function assertSameBack(page, path) {
  assert.equal(await page.goBack({ waitUntil: "load", timeout: 5000 }), null);
  assert.equal(new URL(page.url()).pathname, path);
}
