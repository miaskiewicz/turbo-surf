import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
import { encodeStorageOrigins, decodeStorageOrigins } from "./storage-state.mjs";

let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

test("storage-state transport preserves lone UTF-16 units and large values", () => {
  const origins = [
    {
      origin: "https://example.test",
      localStorage: [{ name: "\ud800", value: "\udfff" + "x".repeat(20000) }],
    },
  ];
  assert.deepEqual(decodeStorageOrigins(encodeStorageOrigins(origins)), origins);
  assert.throws(
    () =>
      encodeStorageOrigins([
        { origin: "https://example.test", localStorage: [{ name: 1, value: "x" }] },
      ]),
    /must be strings/,
  );
});

test("storageState imports before scripts and exports native localStorage after page close", async () => {
  const server = createServer((_request, response) =>
    response.end('<script>window.bootValue=localStorage.getItem("probe")</script>'),
  );
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const initial = {
    cookies: [],
    origins: [{ origin, localStorage: [{ name: "probe", value: "preloaded" }] }],
  };
  const context = new shim.BrowserContext({ storageState: initial });
  let restored;
  try {
    const page = await context.newPage();
    await page.goto(origin, { waitUntil: "networkidle" });
    assert.equal(await page.evaluate(() => window.bootValue), "preloaded");
    await page.evaluate(() => {
      localStorage.setItem("written", "yes");
      localStorage.setItem("\ud800", "\udfff");
      sessionStorage.setItem("session-only", "excluded");
    });
    const peer = await context.newPage();
    await peer.goto(origin, { waitUntil: "networkidle" });
    assert.equal(await peer.evaluate(() => localStorage.getItem("written")), "yes");
    await page.close();
    await peer.close();
    const state = await context.storageState();
    assert.equal(state.origins.length, 1);
    assert.equal(state.origins[0].origin, origin);
    assert.deepEqual(state.origins[0].localStorage, [
      { name: "probe", value: "preloaded" },
      { name: "written", value: "yes" },
      { name: "\ud800", value: "\udfff" },
    ]);
    restored = new shim.BrowserContext({ storageState: state });
    const next = await restored.newPage();
    await next.goto(origin, { waitUntil: "networkidle" });
    assert.equal(await next.evaluate(() => localStorage.getItem("written")), "yes");
    assert.equal(await next.evaluate(() => localStorage.getItem("\ud800")), "\udfff");
    assert.equal(await next.evaluate(() => sessionStorage.getItem("session-only")), null);
  } finally {
    await context.close();
    await restored?.close();
    await new Promise((resolve) => server.close(resolve));
  }
});
