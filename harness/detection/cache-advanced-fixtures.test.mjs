import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

for (const name of ["cache-advanced", "cache-unread"]) {
  test(`${name} control pins authored source and installed Chrome`, () => {
    const fixture = JSON.parse(
      readFileSync(new URL(`./fixtures/${name}-chrome155.json`, import.meta.url)),
    );
    const digest = (file) =>
      createHash("sha256")
        .update(readFileSync(new URL(file, import.meta.url)))
        .digest("hex");
    assert.equal(digest(`./${name}-probe.js`), fixture.probeSha256);
    assert.equal(digest(`./authored-${name}-chrome.mjs`), fixture.collectorSha256);
    // Re-recorded on the frozen Chrome for Testing headless shell after the #154 refactor.
    assert.equal(fixture.browser, "HeadlessChrome/156.0.8078.47");
    assert.equal(fixture.v8, "15.6.75.19");
    assert.deepEqual(fixture.errors, []);
  });
}
