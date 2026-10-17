import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

test("stream and fetch controls pin both authored probe and actual Chrome collector", () => {
  for (const [name, collector] of [
    ["stream-abort", "authored-dom-chrome"],
    ["fetch-stream", "authored-fetch-chrome"],
  ]) {
    const fixture = JSON.parse(
      readFileSync(new URL(`./fixtures/${name}-chrome155.json`, import.meta.url)),
    );
    const digest = (name) =>
      createHash("sha256")
        .update(readFileSync(new URL(name, import.meta.url)))
        .digest("hex");
    assert.equal(digest(`./${name}-probe.js`), fixture.probeSha256);
    assert.equal(digest(`./${collector}.mjs`), fixture.collectorSha256);
    assert.equal(fixture.browser, "Chrome/155.0.8059.79");
    assert.equal(fixture.v8, "15.5.35.23");
    assert.deepEqual(fixture.errors, []);
  }
});
