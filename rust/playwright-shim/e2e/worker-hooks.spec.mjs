import assert from "node:assert/strict";
import { test } from "@playwright/test";

let workerBrowser;
test.beforeAll(({ browser }) => {
  assert.ok(browser, "beforeAll needs the worker browser");
  assert.equal(typeof browser.version(), "string");
  workerBrowser = browser;
});
test("shares worker browser with the test body", ({ browser }) => {
  assert.equal(browser, workerBrowser);
});
test.afterAll(({ browser }) => {
  assert.equal(browser, workerBrowser);
  assert.equal(typeof browser.version(), "string");
});
