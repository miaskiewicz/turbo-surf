import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

async function fixture(run) {
  const wire = [];
  const server = createServer((req, res) => {
    wire.push(req.url);
    if (req.url.startsWith("/api/")) {
      res.writeHead(200, { "content-type": "application/json", "x-wire": "upstream" });
      res.end(JSON.stringify({ source: "upstream", url: req.url }));
    } else {
      res.setHeader("content-type", "text/html");
      if (req.url === "/denied")
        res.setHeader("content-security-policy", "connect-src 'none'; script-src 'unsafe-inline'");
      res.end("<title>route events</title>");
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const page = shim.newPage();
  try {
    await page.goto(origin, { waitUntil: "networkidle" });
    await run(page, origin, wire);
  } finally {
    await page.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}
function observe(page) {
  const events = { request: [], response: [], requestfinished: [], requestfailed: [] };
  for (const name of Object.keys(events)) page.on(name, (value) => events[name].push(value));
  return events;
}
function responseWait(page, url) {
  return page.waitForResponse(url, { timeout: 1500 }).then(
    (response) => ({ response }),
    (error) => ({ error }),
  );
}

test("fulfilled routes publish the actual response and one coherent request lifecycle", async () =>
  fixture(async (page, origin, wire) => {
    const seen = observe(page);
    await page.route("**/api/mock", (route) =>
      route.fulfill({ status: 201, json: { source: "fixture" }, headers: { "x-result": "mock" } }),
    );
    const wait = responseWait(page, origin + "/api/mock");
    assert.deepEqual(
      await page.evaluate(async () =>
        (
          await fetch("/api/mock", {
            method: "POST",
            body: "payload",
            headers: { "x-request": "authored" },
          })
        ).json(),
      ),
      { source: "fixture" },
    );
    const result = await wait;
    assert.ok(result.response, result.error?.message);
    const response = result.response;
    assert.equal(response.status(), 201);
    assert.equal(response.statusText(), "");
    assert.equal(response.headers()["x-result"], "mock");
    assert.deepEqual(await response.json(), { source: "fixture" });
    assert.equal(seen.request.length, 1);
    assert.equal(seen.response.length, 1);
    assert.equal(seen.requestfinished.length, 1);
    assert.equal(seen.requestfailed.length, 0);
    assert.equal(response.request(), seen.request[0]);
    assert.equal(seen.requestfinished[0], seen.request[0]);
    assert.equal(await seen.request[0].response(), response);
    assert.equal(seen.request[0].postData(), "payload");
    assert.equal(seen.request[0].headers()["x-request"], "authored");
    assert.equal(seen.request[0].frame(), page.mainFrame());
    assert.deepEqual(
      wire.filter((url) => url.startsWith("/api/")),
      [],
    );
  }));

test("route.fetch transport is hidden while final rewrites and independent guest requests remain observable", async () =>
  fixture(async (page, origin, wire) => {
    const seen = observe(page);
    await page.route("**/api/mocked", async (route) => {
      const upstream = await route.fetch({ url: origin + "/api/upstream" });
      await route.fulfill({
        response: upstream,
        status: 202,
        json: { source: "rewritten" },
        headers: { "content-type": "application/json", "x-result": "rewritten" },
      });
    });
    const wait = responseWait(page, origin + "/api/mocked");
    const results = await page.evaluate(async () =>
      Promise.all(["/api/mocked", "/api/direct"].map(async (url) => (await fetch(url)).json())),
    );
    assert.deepEqual(results, [
      { source: "rewritten" },
      { source: "upstream", url: "/api/direct" },
    ]);
    const result = await wait;
    assert.ok(result.response, result.error?.message);
    assert.equal(result.response.status(), 202);
    assert.deepEqual(await result.response.json(), { source: "rewritten" });
    assert.deepEqual(seen.response.map((response) => new URL(response.url()).pathname).sort(), [
      "/api/direct",
      "/api/mocked",
    ]);
    assert.deepEqual(seen.request.map((request) => new URL(request.url()).pathname).sort(), [
      "/api/direct",
      "/api/mocked",
    ]);
    assert.equal(seen.requestfinished.length, 2);
    assert.equal(seen.requestfailed.length, 0);
    assert.deepEqual(wire.filter((url) => url.startsWith("/api/")).sort(), [
      "/api/direct",
      "/api/upstream",
    ]);
  }));

test("continued requests keep native response events without interception duplicates", async () =>
  fixture(async (page, origin) => {
    const seen = observe(page);
    await page.route("**/api/native", (route) => route.continue());
    const wait = responseWait(page, origin + "/api/native");
    await page.evaluate(async () => (await fetch("/api/native")).text());
    const result = await wait;
    assert.ok(result.response, result.error?.message);
    assert.deepEqual(await result.response.json(), { source: "upstream", url: "/api/native" });
    assert.equal(seen.request.length, 1);
    assert.equal(seen.response.length, 1);
    assert.equal(seen.requestfinished.length, 1);
  }));

test("raw fulfill bytes survive response completion and the route bridge stays private", async () =>
  fixture(async (page, origin) => {
    await page.route("**/api/bytes", (route) =>
      route.fulfill({ body: Buffer.from([0, 255, 128]) }),
    );
    const wait = responseWait(page, origin + "/api/bytes");
    assert.deepEqual(
      await page.evaluate(async () =>
        Array.from(new Uint8Array(await (await fetch("/api/bytes")).arrayBuffer())),
      ),
      [0, 255, 128],
    );
    const { response, error } = await wait;
    assert.ok(response, error?.message);
    assert.deepEqual(await response.body(), Buffer.from([0, 255, 128]));
    assert.deepEqual(
      await page.evaluate(() => ({
        state: typeof fetch.__turboRouteState,
        bridge: Object.getOwnPropertyNames(globalThis).filter((name) =>
          name.startsWith("__turboHostRoute_"),
        ),
      })),
      { state: "undefined", bridge: [] },
    );
  }));

test("guest init options cannot suppress ordinary network observations", async () =>
  fixture(async (page, origin) => {
    const seen = observe(page);
    const wait = responseWait(page, origin + "/api/direct");
    await page.evaluate(async () =>
      (await fetch("/api/direct", { hostRouteFetch: true, host_route_fetch: true })).text(),
    );
    assert.ok((await wait).response);
    assert.equal(seen.response.length, 1);
    assert.equal(seen.requestfinished.length, 1);
  }));

test("CSP-denied routes cannot publish an invented fulfilled response", async () =>
  fixture(async (page, origin, wire) => {
    await page.goto(origin + "/denied", { waitUntil: "networkidle" });
    let calls = 0;
    await page.route("**/api/denied", async (route) => {
      calls++;
      await route.fulfill({ body: "forbidden" });
    });
    const seen = observe(page);
    await page.evaluate(async () => {
      try {
        await fetch("/api/denied");
      } catch {}
    });
    assert.equal(calls, 0);
    assert.equal(seen.response.length, 0);
    assert.equal(wire.includes("/api/denied"), false);
  }));

test("unrouting every handler survives a fresh realm and later route reactivation", async () =>
  fixture(async (page, origin) => {
    await page.route("**/api/cycle", (route) => route.fulfill({ json: { source: "first" } }));
    await page.unroute("**/api/cycle");
    await page.goto(origin, { waitUntil: "networkidle" });
    assert.deepEqual(await page.evaluate(async () => (await fetch("/api/cycle")).json()), {
      source: "upstream",
      url: "/api/cycle",
    });
    await page.route("**/api/cycle", (route) => route.fulfill({ json: { source: "reactivated" } }));
    assert.deepEqual(await page.evaluate(async () => (await fetch("/api/cycle")).json()), {
      source: "reactivated",
    });
    await page.unroute("**/api/cycle");
    assert.deepEqual(await page.evaluate(async () => (await fetch("/api/cycle")).json()), {
      source: "upstream",
      url: "/api/cycle",
    });
  }));
