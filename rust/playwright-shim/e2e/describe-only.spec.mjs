import assert from "node:assert/strict";
import { test } from "@playwright/test";
let setups = 0;
test.beforeEach(() => setups++);
test.afterAll(() => assert.equal(setups, 2));
test("ordinary must not run", () => {
  throw Error("ordinary executed");
});
test.describe
  .only("focused suite", () => {
    test("focused child", () => {});
    test.describe("nested focused suite", () => {
      let setup = false;
      test.beforeAll(() => {
        setup = true;
      });
      test("inherits focus into nested hooks", () => assert.ok(setup));
    });
  });
