import assert from "node:assert/strict";
import test from "node:test";
import {
  expectedPromiseWork,
  measurePromiseWork,
} from "../../harness/hotpath/promise-work-perf.mjs";
let shim;
try {
  shim = await import("./index.mjs");
} catch (error) {
  if (
    !String(error.message).startsWith(
      "turbo-surf-native: no prebuilt binary found and no local cargo build",
    )
  )
    throw error;
}

for (const options of [
  { depth: 0, fanout: 2048, turns: 3 },
  { depth: 2048, fanout: 0, turns: 3 },
  { depth: 128, fanout: 128, turns: 5 },
]) {
  test(
    `MessageChannel preserves settled-parent reactions and task ordering ${JSON.stringify(options)}`,
    { skip: !shim && "native addon not built" },
    async () => {
      const page = shim.newPage();
      try {
        await page.setContent("<!doctype html><body><h1>Promise work</h1>");
        const result = await measurePromiseWork(page, options);
        assert.deepEqual(
          { completed: result.completed, checksum: result.checksum },
          expectedPromiseWork(options),
        );
        assert.equal(
          await page.evaluate(() => document.querySelector("h1").textContent),
          "Promise work",
        );
      } finally {
        await page.close();
      }
    },
  );
}
