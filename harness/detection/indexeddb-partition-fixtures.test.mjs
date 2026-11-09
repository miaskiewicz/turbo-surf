import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

test("IndexedDB partition control pins its authored scripts and the Chrome collector", () => {
  const fixture = JSON.parse(
    readFileSync(new URL("./fixtures/indexeddb-partition-chrome156.json", import.meta.url)),
  );
  const digest = (name) =>
    createHash("sha256")
      .update(readFileSync(new URL(name, import.meta.url)))
      .digest("hex");
  assert.equal(digest("./indexeddb-partition-probe.js"), fixture.probeSha256);
  assert.equal(digest("./indexeddb-partition-widget.js"), fixture.widgetSha256);
  assert.equal(digest("./indexeddb-partition-chrome.mjs"), fixture.collectorSha256);
  assert.equal(fixture.browser, "Chrome/156.0.8078.80");
  assert.equal(fixture.v8, "15.6.75.27");
  assert.deepEqual(fixture.errors, []);
});
