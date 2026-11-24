import assert from "node:assert/strict";
import { createServer } from "node:http";
import { createRequire } from "node:module";
import test from "node:test";
let shim, native;
try {
  shim = await import("./index.mjs");
  native = createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

async function fixture(run) {
  const server = createServer((request, response) => {
    if (request.headers.rsc) {
      response.end("real-flight-response");
      return;
    }
    response.setHeader("content-type", "text/html");
    response.end(`<body><button id="navigate">Navigate</button><button id="fetch">Fetch only</button><button id="prefetch">Prefetch</button><div id="result">pending</div><script>
      globalThis.documentMarker = {};
      const request = (prefetch = false) => fetch('/second?business=retained&_rsc=internal', { headers: {RSC:'1', ...(prefetch ? {'Next-Router-Prefetch':'1'} : {})} }).then(r=>r.text());
      document.querySelector('#navigate').onclick = async () => {
        await request(); history.pushState({owned:true}, '', '/second?business=retained');
        document.querySelector('#result').textContent = 'navigated';
      };
      document.querySelector('#prefetch').onclick = async () => {
        await request(true); document.querySelector('#result').textContent = 'prefetched';
      };
      document.querySelector('#fetch').onclick = async () => {
        await request(); document.querySelector('#result').textContent = 'fetched';
      };
    </script></body>`);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const page = shim.newPage();
  try {
    await page.goto(origin + "/start", { waitUntil: "load", timeout: 3000 });
    await run(page, origin);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

async function actualLocation(page) {
  return JSON.parse(
    await native.liveEvalEngine(
      page._session,
      "globalThis.__RESULT=JSON.stringify({href:location.href,documentURL:document.URL,base:document.baseURI});",
    ),
  );
}

test("RSC transport hint cannot overwrite actual native history URL", async () => {
  await fixture(async (page, origin) => {
    await page.locator("#navigate").click({ timeout: 3000 });
    const observed = page.url();
    const location = await actualLocation(page);
    const history = JSON.parse(native.liveTargetHistoryState(page._session));
    assert.equal(location.href, origin + "/second?business=retained");
    assert.equal(location.documentURL, location.href);
    assert.equal(history.entries[history.index].url, location.href);
    assert.equal(observed, location.href);
    assert.equal(page._historyCurrent.url, location.href);
  });
});

test("RSC fetch without authored navigation cannot change native Page URL", async () => {
  await fixture(async (page, origin) => {
    await page.locator("#fetch").click({ timeout: 3000 });
    const observed = page.url();
    const location = await actualLocation(page);
    assert.equal(location.href, origin + "/start");
    assert.equal(observed, location.href);
  });
});

test("RSC prefetch leaves its native document and current URL unchanged", async () => {
  await fixture(async (page, origin) => {
    await page.evaluate(() => true);
    const before = JSON.parse(native.liveTargetHistoryState(page._session));
    await page.locator("#prefetch").click({ timeout: 3000 });
    assert.equal(page.url(), origin + "/start");
    const after = JSON.parse(native.liveTargetHistoryState(page._session));
    assert.equal(after.document, before.document);
    assert.equal(after.entries[after.index].url, origin + "/start");
  });
});

test("legacy targets retain their existing RSC fallback", async () => {
  const context = new shim.BrowserContext({ baseURL: "https://example.test/" });
  try {
    const page = await context.newPage();
    assert.equal(
      await page._refreshRscNavigation(async () => "/second?business=retained", null),
      true,
    );
    assert.equal(page.url(), "https://example.test/second?business=retained");
  } finally {
    await context.close();
  }
});
