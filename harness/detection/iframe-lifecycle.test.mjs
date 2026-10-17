import assert from "node:assert/strict";
import { test } from "node:test";
import { iframeLifecycleErrors } from "./iframe-lifecycle.mjs";

test("iframe oracle rejects eager detached loads and missing removal checks", () => {
  const errors = iframeLifecycleErrors({ nativeNode: true, detached: false, removed: false });
  assert.ok(errors.includes("detached"));
  assert.ok(errors.includes("removed"));
  assert.ok(errors.includes("finishedEvent"));
  assert.ok(!errors.includes("nativeNode"));
});
