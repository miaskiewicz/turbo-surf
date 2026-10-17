import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { test } from "node:test";

test("stream transfer controls pin every measured probe, collector and localhost fixture", () => {
  for (const name of [
    "transfer",
    "adversarial",
    "retirement",
    "cancel-context",
    "poison",
    "reentry",
    "order",
  ]) {
    const fixture = JSON.parse(
      readFileSync(new URL(`./fixtures/stream-${name}-chrome155.json`, import.meta.url), "utf8"),
    );
    for (const [path, expected] of [
      [`./stream-${name}-probe.js`, fixture.probeSha256],
      ["./authored-upload-chrome.mjs", fixture.collectorSha256],
      ["./upload-server.mjs", fixture.fixtureSha256],
    ]) {
      const source = readFileSync(new URL(path, import.meta.url));
      assert.equal(createHash("sha256").update(source).digest("hex"), expected, `${name}: ${path}`);
    }
    assert.match(fixture.browser, /^Chrome\/155\./);
    assert.match(fixture.v8, /^15\.5\./);
    assert.deepEqual(fixture.errors, []);
    assert.ok(fixture.result !== null && fixture.result !== undefined);
  }
});
