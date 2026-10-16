import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

test("HTTP cache control pins the authored source, collector and installed Chrome", () => {
  const fixture = JSON.parse(
    readFileSync(new URL("./fixtures/http-cache-chrome155.json", import.meta.url)),
  );
  const digest = (file) =>
    createHash("sha256")
      .update(readFileSync(new URL(file, import.meta.url)))
      .digest("hex");
  assert.equal(digest("./http-cache-probe.js"), fixture.probeSha256);
  assert.equal(digest("./authored-http-cache-chrome.mjs"), fixture.collectorSha256);
  assert.equal(fixture.browser, "Chrome/155.0.8059.79");
  assert.equal(fixture.v8, "15.5.35.23");
  assert.deepEqual(fixture.errors, []);
});
