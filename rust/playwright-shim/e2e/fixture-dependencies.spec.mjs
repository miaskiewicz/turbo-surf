import assert from "node:assert/strict";
import { test as base } from "@playwright/test";

const test = base.extend({
  dependent: async ({ source }, use) => use(`${source}-dependent`),
  source: async (_fixtures, use) => use("source"),
});
test("resolves dependencies before their earlier-declared consumer", ({ dependent }) => {
  assert.equal(dependent, "source-dependent");
});
const lazyTest = base.extend({
  unused: async (_fixtures, _use) => {
    throw Error("unused fixture ran");
  },
});
lazyTest("does not execute unrequested factories", () => {});
const aliasTest = base.extend({ value: "alias value" });
aliasTest("resolves requested aliases", ({ value: renamed }) =>
  assert.equal(renamed, "alias value"),
);
