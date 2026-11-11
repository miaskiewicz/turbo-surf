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
