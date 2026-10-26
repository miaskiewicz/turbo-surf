import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { bindingSource } from "./binding-files.mjs";
const read = (file) => readFileSync(new URL(file, import.meta.url));
const digest = (file) => createHash("sha256").update(read(file)).digest("hex");
const dom = bindingSource();
test("radio ownership controls retain their independently measured real-task GC policy", () => {
  const control = JSON.parse(read(`${dom}radio_dirty_adoption_gc_chrome155.json`));
  const review = JSON.parse(read(`${dom}radio_dirty_adoption_gc_review_chrome155.json`));
  assert.equal(control.collectorSha256, digest("./owned-gc-chrome.mjs"));
  assert.equal(control.probeSha256, digest(`${dom}radio_dirty_adoption_gc_probe.js`));
  assert.equal(control.casesSha256, digest(`${dom}radio_dirty_cases.json`));
  assert.deepEqual(control.gcPolicy, { rounds: 3, settleMs: 50, taskExpression: "0" });
  assert.deepEqual(control.errors, []);
  assert.equal(review.correctedSettledRepeats.length, 3);
  for (const repeated of review.correctedSettledRepeats) {
    for (const field of [
      "collectorSha256",
      "probeSha256",
      "casesSha256",
      "gcPolicy",
      "result",
      "errors",
    ])
      assert.deepEqual(repeated[field], control[field]);
  }
  assert.equal(
    review.originalOneoff.collectorSha256,
    digest("./element-internals-lifecycle-chrome.mjs"),
  );
  assert.deepEqual(review.nativeProof.first, control.result[0].result.first);
  assert.deepEqual(review.nativeProof.afterRelease, control.result[0].result.afterRelease);
});

test("native control labels use independently collected real-task GC controls", () => {
  const control = JSON.parse(read(`${dom}control_labels_gc_chrome155.json`));
  assert.equal(control.collectorSha256, digest("./owned-gc-chrome.mjs"));
  assert.equal(control.probeSha256, digest(`${dom}control_labels_gc_probe.js`));
  assert.equal(control.casesSha256, digest(`${dom}radio_dirty_cases.json`));
  assert.equal(control.browser, "Chrome/155.0.8059.80");
  assert.equal(control.v8, "15.5.35.23");
  assert.deepEqual(control.gcPolicy, { rounds: 3, settleMs: 50, taskExpression: "0" });
  assert.deepEqual(control.errors, []);
  assert.deepEqual(control.result[0].result.afterRelease.alive, [
    false,
    false,
    false,
    false,
    false,
  ]);
});
