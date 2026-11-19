import assert from "node:assert/strict";
import { test as base } from "@playwright/test";
const order = [];
const test = base.extend({
  marker: [
    async (_fixtures, use) => {
      order.push("setup");
      await use("ready");
      order.push("teardown");
    },
    { auto: true },
  ],
});
test("automatic tuple is active without consumption", () => {
  assert.deepEqual(order, ["setup"]);
  order.push("body1");
});
test("requested tuple supplies its resolved value", ({ marker }) => {
  assert.equal(marker, "ready");
  assert.deepEqual(order, ["setup", "body1", "teardown", "setup"]);
  order.push("body2");
});
test.afterAll(() =>
  assert.deepEqual(order, ["setup", "body1", "teardown", "setup", "body2", "teardown"]),
);
