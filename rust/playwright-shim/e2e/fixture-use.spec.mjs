import assert from "node:assert/strict";
import { test as base } from "@playwright/test";

const test = base.extend({ label: "default" });
test.use({ label: "configured" });
test("root plain override", ({ label }) => assert.equal(label, "configured"));
test.describe("nested override", () => {
  test.use({ label: "nested" });
  test("nested plain override", ({ label }) => assert.equal(label, "nested"));
});
test.describe("sibling inherits root", () => {
  test("sibling plain override", ({ label }) => assert.equal(label, "configured"));
});
const child = test.extend({ suffix: "child" });
child("extended test inherits use", ({ label, suffix }) => {
  assert.equal(label, "configured");
  assert.equal(suffix, "child");
});
