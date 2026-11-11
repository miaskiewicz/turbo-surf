import assert from "node:assert/strict";
import { test as base } from "@playwright/test";
const order = [];
let created = 0;
const test = base.extend({
  state: async (_fixtures, use) => {
    created++;
    order.push("setup");
    await use({ value: 0 });
    await new Promise((resolve) => setTimeout(resolve, 10));
    order.push("teardown");
  },
});
test.beforeEach(({ state }) => {
  state.value = 1;
  order.push("before");
});
test.afterEach(({ state }) => {
  assert.equal(state.value, 2);
  order.push("after");
});
test("hooks and body share one graph", ({ state }) => {
  assert.equal(state.value, 1);
  assert.equal(created, 1);
  state.value = 2;
  order.push("body");
});
test.afterAll(() => assert.deepEqual(order, ["setup", "before", "body", "after", "teardown"]));
