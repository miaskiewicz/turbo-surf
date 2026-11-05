import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

test("storage control pins all authored scripts and the actual Chrome collector", () => {
  const fixture = JSON.parse(
    readFileSync(new URL("./fixtures/storage-chrome155.json", import.meta.url)),
  );
  const digest = (name) =>
    createHash("sha256")
      .update(readFileSync(new URL(name, import.meta.url)))
      .digest("hex");
  assert.equal(digest("./storage-probe.js"), fixture.probeSha256);
  assert.equal(digest("./storage-widget-probe.js"), fixture.widgetSha256);
  assert.equal(digest("./authored-storage-chrome.mjs"), fixture.collectorSha256);
  // Re-recorded on the frozen Chrome for Testing headless shell after the #154 refactor.
  assert.equal(fixture.browser, "HeadlessChrome/156.0.8078.47");
  assert.equal(fixture.v8, "15.6.75.19");
  assert.deepEqual(fixture.errors, []);
});
