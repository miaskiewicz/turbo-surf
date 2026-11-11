// Regression for #284/#299: exercise the actual ALS slots without registering
// node:test suites inside an already-running test body.
import assert from "node:assert/strict";
import { setImmediate as nextTurn } from "node:timers/promises";
import test from "node:test";

let shim;
try {
  shim = await import("./index.mjs");
} catch (e) {
  console.log("shim: native addon not built, skipping —", e.message);
  process.exit(0);
}
const { test: pw, __internals } = shim;

test("shim describe skip-frames remain isolated across concurrent async contexts", async () => {
  const { skipStackStore, getSkipStack } = __internals;
  const first = [{ skip: false }];
  const second = [{ skip: false }];
  await Promise.all([
    skipStackStore.run(first, async () => {
      await nextTurn();
      pw.skip(true, "first only");
      await nextTurn();
      assert.equal(getSkipStack(), first);
      assert.equal(getSkipStack()[0].skip, true);
    }),
    skipStackStore.run(second, async () => {
      await nextTurn();
      pw.skip(false, "second stays enabled");
      await nextTurn();
      assert.equal(getSkipStack(), second);
      assert.equal(getSkipStack()[0].skip, false);
    }),
  ]);
  assert.deepEqual(getSkipStack(), []);
});

test("shim active test remains isolated across concurrent async contexts", async () => {
  const { activeTStore, getActiveT } = __internals;
  const skipped = [];
  await Promise.all(
    ["first", "second", "third"].map(async (name) => {
      const context = { skip: (reason) => skipped.push([name, reason]) };
      await activeTStore.run(context, async () => {
        await nextTurn();
        assert.equal(getActiveT(), context);
        assert.throws(() => pw.skip(true, name));
        await nextTurn();
        assert.equal(getActiveT(), context);
      });
    }),
  );
  assert.deepEqual(skipped.toSorted(), [
    ["first", "first"],
    ["second", "second"],
    ["third", "third"],
  ]);
  assert.equal(getActiveT(), null);
});
