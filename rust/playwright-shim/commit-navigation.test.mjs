import assert from "node:assert/strict";
import { createServer } from "node:http";
import test, { after } from "node:test";

let dataRequests = 0;
const server = createServer((request, response) => {
  if (request.url === "/rows") {
    dataRequests++;
    response.setHeader("content-type", "application/json");
    response.end(JSON.stringify({ count: 37 }));
    return;
  }
  const path = request.url;
  response.setHeader("content-type", "text/html");
  response.end(`<!doctype html><main id="ready"><div id="table"></div><button id="open">Open</button></main><script>
      window.bootPath=${JSON.stringify(path)};
      document.getElementById('open').onclick=()=>document.body.dataset.open='yes';
      fetch('/rows').then(response=>response.json()).then(data=>document.getElementById('table').textContent=bootPath+' of '+data.count);
    </script>`);
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const origin = `http://127.0.0.1:${server.address().port}`;
process.env.TURBO_SURF_ALLOWED_ORIGINS = JSON.stringify([origin]);
const { expect, newPage } = await import("./index.mjs");
after(async () => {
  server.closeAllConnections();
  await new Promise((resolve) => server.close(resolve));
});

async function fixture(run) {
  const before = dataRequests;
  const page = newPage();
  try {
    await run(page, origin, () => dataRequests - before);
  } finally {
    await page.close();
  }
}

test("commit stays raw with observers but later text assertions run asynchronous client data", async () => {
  await fixture(async (page, origin, requests) => {
    page.on("response", () => {});
    page.on("console", () => {});
    await page.goto(`${origin}/catalogue`, { waitUntil: "commit" });
    assert.equal(page._live, false);
    assert.equal(requests(), 0);
    assert.match(await page.content(), /id="ready"/);
    assert.equal(page._live, false, "raw content inspection needs no script execution");
    await expect(page.locator("#table")).toContainText("/catalogue of 37", { timeout: 1000 });
    assert.equal(requests(), 1);
    assert.equal(page._live, true);
  });
});

test("commit allows later evaluation and authored button actions", async () => {
  await fixture(async (page, origin) => {
    await page.goto(`${origin}/action`, { waitUntil: "commit" });
    await page.locator("#open").click();
    assert.equal(await page.evaluate(() => document.body.dataset.open), "yes");
    assert.equal(await page.evaluate(() => window.bootPath), "/action");
  });
});

test("commit reload hydrates the new document only when requested", async () => {
  await fixture(async (page, origin) => {
    await page.goto(`${origin}/reload`, { waitUntil: "networkidle" });
    await page.evaluate(() => {
      window.oldDocument = true;
    });
    await page.reload({ waitUntil: "commit" });
    assert.equal(page._live, false);
    assert.equal(await page.evaluate(() => typeof window.oldDocument), "undefined");
    await expect(page.locator("#table")).toHaveText("/reload of 37", { timeout: 1000 });
  });
});

test("commit history document replacements retain on-demand execution", async () => {
  await fixture(async (page, origin) => {
    await page.goto(`${origin}/first`, { waitUntil: "commit" });
    await page.goto(`${origin}/second`, { waitUntil: "commit" });
    await page.goBack({ waitUntil: "commit" });
    await expect(page.locator("#table")).toHaveText("/first of 37", { timeout: 1000 });
    await page.goForward({ waitUntil: "commit" });
    await expect(page.locator("#table")).toHaveText("/second of 37", { timeout: 1000 });
  });
});

test("commit preserves an independently disabled on-demand execution policy", async () => {
  await fixture(async (page, origin, requests) => {
    page._autoHydrate = false;
    await page.goto(origin, { waitUntil: "commit" });
    await assert.rejects(expect(page.locator("#table")).toContainText("of 37", { timeout: 20 }));
    assert.equal(page._live, false);
    assert.equal(requests(), 0);
    assert.match(await page.content(), /id="ready"/);
  });
});

test("closing a committed document cannot resurrect its execution session", async () => {
  await fixture(async (page, origin, requests) => {
    await page.goto(origin, { waitUntil: "commit" });
    await page.close();
    await assert.rejects(
      page.evaluate(() => 1),
      /closed/,
    );
    assert.equal(requests(), 0);
  });
});
