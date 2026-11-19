import assert from "node:assert/strict";
import { test } from "@playwright/test";
let setups = 0;
test.beforeEach(() => setups++);
test.afterAll(() => assert.equal(setups, 1));
test("ordinary must not run", () => {
  throw Error("ordinary executed");
});
test.only("focused case", () => {});
