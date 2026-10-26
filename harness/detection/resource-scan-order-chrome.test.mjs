import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { extractResourceFixture, fixtureSource, sha256 } from "./resource-scan-order-chrome.mjs";

const read = (path) => readFileSync(new URL(`../../${path}`, import.meta.url));
test("resource-order Chrome control pins its authored fixture and collection tools", () => {
  const control = JSON.parse(read("harness/detection/fixtures/resource-scan-order-chrome156.json"));
  assert.equal(control.fixtureSource, fixtureSource);
  assert.equal(
    control.fixtureSha256,
    sha256(extractResourceFixture(read(fixtureSource).toString())),
  );
  assert.equal(control.collectorSha256, sha256(read("harness/detection/chrome-cdp-trace.mjs")));
  assert.equal(control.inputStateSha256, sha256(read("harness/detection/input-state.js")));
  assert.equal(control.observationSha256, sha256(read("harness/detection/observation-window.mjs")));
  assert.equal(control.navigationSha256, sha256(read("harness/detection/main-navigation.mjs")));
  assert.equal(control.lifecycleSha256, sha256(read("harness/detection/collector-lifecycle.mjs")));
  assert.equal(control.effectiveInputSha256, sha256(read("harness/detection/effective-input.mjs")));
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
