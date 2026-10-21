import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import test from "node:test";

test("storage-eligibility control pins actual Chrome, source, collector and widget", () => {
  const control = JSON.parse(
    readFileSync(new URL("fixtures/storage-eligibility-chrome155.json", import.meta.url)),
  );
  const sha = (path) =>
    createHash("sha256")
      .update(readFileSync(new URL(path, import.meta.url)))
      .digest("hex");
  assert.equal(control.browser, "Chrome/155.0.8059.79");
  assert.equal(control.v8, "15.5.35.23");
  assert.equal(control.probeSha256, sha("storage-eligibility-probe.js"));
  assert.equal(control.collectorSha256, sha("authored-storage-eligibility-chrome.mjs"));
  assert.equal(control.widgetSha256, sha("storage-clear-widget.js"));
  assert.deepEqual(control.errors, []);
  for (const name of ["fetch", "classic", "module"]) {
    assert.deepEqual(control.result[name], {
      root: ["local", "session"],
      peer: [],
      local: null,
      session: null,
    });
  }
});
