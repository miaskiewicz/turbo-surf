import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { extractResourceFixture, fixtureSource, sha256 } from "./resource-scan-order-chrome.mjs";

const read = (path) => readFileSync(new URL(`../../${path}`, import.meta.url));
test("resource-order Chrome control pins its authored fixture and collection tools", () => {
  const control = JSON.parse(read("harness/detection/fixtures/resource-scan-order-chrome155.json"));
  assert.equal(control.fixtureSource, fixtureSource);
  assert.equal(
    control.fixtureSha256,
    sha256(extractResourceFixture(read(fixtureSource).toString())),
  );
  assert.equal(control.collectorSha256, sha256(read("harness/detection/chrome-cdp-trace.mjs")));
  assert.equal(
    control.generatorSha256,
    sha256(read("harness/detection/resource-scan-order-chrome.mjs")),
  );
  assert.deepEqual(control.errors, []);
  assert.deepEqual(control.result, { cutoff: [1, true, 1], finished: true });
  assert.deepEqual(control.unexpectedStylesheetRequests, []);
  assert.deepEqual(control.scriptFetchesOutsideControl, []);
});
test("resource-order extraction rejects a missing fixture", () => {
  assert.throws(() => extractResourceFixture("let html = 42;"), /missing its HTML raw string/);
});
