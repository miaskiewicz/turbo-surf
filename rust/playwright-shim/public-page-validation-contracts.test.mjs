import assert from "node:assert/strict";
import { test } from "node:test";
import { newPage } from "./index.mjs";

async function pageWith(t) {
  const page = newPage();
  t.after(() => page.close());
  await page.setContent('<input id="healthy" value="ready">');
  return page;
}

test("invalid public wait and viewport options leave the live document usable", async (t) => {
  const page = await pageWith(t);
  await assert.rejects(
    page.waitForLoadState("bogus", { timeout: 100 }),
    /Unsupported load state: bogus/,
  );
  await assert.rejects(
    page.waitForSelector("#healthy", { state: "bogus", timeout: 100 }),
    /Unsupported selector wait state: bogus/,
  );
  await assert.rejects(
    page.waitForEvent("bogus", { timeout: 100 }),
    /Unsupported Page event: bogus/,
  );
  const original = page.viewportSize();
  for (const size of [
    { width: 0, height: 400 },
    { width: 400, height: -1 },
    { width: 400.5, height: 400 },
  ]) {
    await assert.rejects(page.setViewportSize(size), /positive integers/);
    assert.deepEqual(page.viewportSize(), original);
    assert.equal(await page.locator("#healthy").inputValue(), "ready");
  }
  await page.setViewportSize({ width: 640, height: 480 });
  assert.deepEqual(page.viewportSize(), { width: 640, height: 480 });
  assert.deepEqual(await page.evaluate(() => ({ width: innerWidth, height: innerHeight })), {
    width: 640,
    height: 480,
  });
  await page.locator("#healthy").fill("recovered");
  assert.equal(await page.locator("#healthy").inputValue(), "recovered");
});

test("closed page operations reject without affecting an independently live page", async (t) => {
  const page = await pageWith(t);
  const peer = await pageWith(t);
  await page.close();
  assert.equal(page.isClosed(), true);
  await assert.rejects(
    page.addInitScript(() => {}),
    /Page is closed/,
  );
  await assert.rejects(page.setContent("<p>replacement</p>", { timeout: 100 }), /page is closed/);
  await assert.rejects(
    page.waitForURL("https://never-requested.test/", { timeout: 100 }),
    /Page closed while waiting for URL/,
  );
  await assert.rejects(page.waitForEvent("load", { timeout: 100 }), /Page is closed/);
  assert.equal(await peer.locator("#healthy").inputValue(), "ready");
  await peer.locator("#healthy").fill("peer survives");
  assert.equal(await peer.locator("#healthy").inputValue(), "peer survives");
});
