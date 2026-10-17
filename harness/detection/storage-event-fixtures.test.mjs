import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import test from "node:test";

test("storage-event Chrome control is pinned to its actual source and collector", () => {
  const root = new URL("./", import.meta.url);
  const control = JSON.parse(readFileSync(new URL("fixtures/storage-event-chrome155.json", root)));
  const sha = (file) =>
    createHash("sha256")
      .update(readFileSync(new URL(file, root)))
      .digest("hex");
  assert.equal(control.probeSha256, sha("storage-event-probe.js"));
  assert.equal(control.collectorSha256, sha("authored-dom-chrome.mjs"));
  assert.equal(control.browser, "Chrome/155.0.8059.79");
  assert.equal(control.v8, "15.5.35.23");
  assert.deepEqual(control.errors, []);
  assert.equal(control.result.events.local.length, 5);
  assert.equal(control.result.events.session.length, 1);
  assert.deepEqual(control.result.shape.childSelf, []);
  assert.deepEqual(control.result.shape.borrowedShape, { same: true, prototype: true });
});
