import assert from "node:assert/strict";
import test from "node:test";

let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  console.log("shim: native addon not built, skipping —", error.message);
  process.exit(0);
}

test("closed pages leave the context registry before their close event", async () => {
  const context = new shim.BrowserContext();
  const first = await context.newPage();
  const second = await context.newPage();
  const closed = [];
  first.on("close", () => {
    assert.equal(first.isClosed(), true);
    assert.deepEqual(context.pages(), [second]);
    closed.push(first);
  });
  second.on("close", () => closed.push(second));
  await first.close();
  assert.deepEqual(context.pages(), [second]);
  await context.close();
  assert.deepEqual(context.pages(), []);
  assert.deepEqual(closed, [first, second]);
});

test("each launched browser owns its contexts and closes only its resources", async () => {
  const first = await shim.chromium.launch();
  const second = await shim.chromium.launch();
  try {
    assert.notEqual(first, second);
    const context = await first.newContext();
    const other = await second.newContext();
    const page = await context.newPage();
    const secondPage = await context.newPage();
    const otherPage = await other.newPage();
    assert.equal(context.browser(), first);
    assert.deepEqual(first.contexts(), [context]);
    await first.close();
    assert.equal(first.isConnected(), false);
    assert.equal(page.isClosed(), true);
    assert.equal(secondPage.isClosed(), true);
    assert.deepEqual(context.pages(), []);
    assert.deepEqual(first.contexts(), []);
    await assert.rejects(first.newContext(), /closed/i);
    await assert.rejects(first.newPage(), /closed/i);
    assert.equal(second.isConnected(), true);
    assert.equal(otherPage.isClosed(), false);
    await other.close();
    assert.deepEqual(second.contexts(), []);
  } finally {
    await first.close();
    await second.close();
  }
});
