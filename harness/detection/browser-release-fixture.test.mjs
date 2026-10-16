import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { test } from "node:test";

test("current Chrome release identity retains independent probe and collector provenance", () => {
  const control = JSON.parse(
    readFileSync(new URL("fixtures/browser-release-chrome155.json", import.meta.url)),
  );
  const hash = (path) =>
    createHash("sha256")
      .update(readFileSync(new URL(path, import.meta.url)))
      .digest("hex");
  assert.equal(control.probeSha256, hash("browser-release-probe.js"));
  assert.equal(control.collectorSha256, hash("authored-dom-chrome.mjs"));
  assert.equal(control.browser, "Chrome/155.0.8059.80");
  assert.equal(control.v8, "15.5.35.23");
  assert.deepEqual(control.errors, []);
  assert.equal(control.result.high.uaFullVersion, "155.0.8059.80");
});
