import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { bindingSource } from "./binding-files.mjs";

const read = (file) => readFileSync(new URL(file, import.meta.url));
const digest = (file) => createHash("sha256").update(read(file)).digest("hex");
test("private abort boundary controls stay pinned to independently collected Chrome behavior", () => {
  const control = JSON.parse(read("./fixtures/abort-any-private-chrome155.json"));
  assert.equal(control.browser, "Chrome/155.0.8059.80");
  assert.equal(control.v8, "15.5.35.23");
  assert.deepEqual(control.errors, []);
  assert.equal(control.probeSha256, digest("./abort-any-private-probe.js"));
  assert.equal(control.collectorSha256, digest("./trusted-types-code-chrome.mjs"));
  assert.equal(control.casesSha256, digest(`${bindingSource()}radio_dirty_cases.json`));
  const result = control.result[0].result;
  assert.deepEqual(result.arrayFrom, { calls: 0, rawExposed: false });
  assert.deepEqual(result.arrayIterator, { calls: 0, rawExposed: false });
  assert.deepEqual(result.numericSetter, { rawExposed: false });
  assert.deepEqual(result.descriptorAttack, { rawExposed: false, abortSucceeded: true });
  assert.deepEqual(result.conversions, {
    emptyString: "TypeError",
    null: "TypeError",
    number: "TypeError",
    emptyArray: false,
    invalidArray: "TypeError",
  });
  assert.deepEqual(result.sequence, ["iterator", "next", "TypeError"]);
  assert.equal(result.firstReason, true);
  assert.deepEqual(result.propagation, {
    aborted: true,
    reason: true,
    events: ["source", "dependent"],
  });
});

test("stream-controller privacy control stays pinned to the independent hostile-hook probe", () => {
  const control = JSON.parse(read("./fixtures/stream-controller-private-chrome155.json"));
  assert.equal(control.browser, "Chrome/155.0.8059.80");
  assert.equal(control.v8, "15.5.35.23");
  assert.deepEqual(control.errors, []);
  assert.equal(control.probeSha256, digest("./stream-controller-private-probe.js"));
  assert.equal(control.collectorSha256, digest("./trusted-types-code-chrome.mjs"));
  assert.equal(control.casesSha256, digest(`${bindingSource()}radio_dirty_cases.json`));
  assert.deepEqual(control.result[0].result, {
    rawExposed: false,
    callback: true,
    enqueueWorks: true,
  });
});
