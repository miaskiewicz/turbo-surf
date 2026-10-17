import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const source = (name) => readFileSync(new URL(`./${name}`, import.meta.url));
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");

test("native Trusted Types controls pin independent case, probe and collector sources", () => {
  for (const [name, collector, count] of [
    ["trusted-types", "trusted-types-chrome", 15],
    ["trusted-types-sinks", "trusted-types-chrome", 4],
    ["trusted-types-code", "trusted-types-code-chrome", 11],
    ["trusted-types-lifecycle", "trusted-types-lifecycle-chrome", 1],
  ]) {
    const control = JSON.parse(source(`${name}-chrome155.json`));
    const cases = JSON.parse(source(`${name}-cases.json`));
    assert.equal(control.casesSha256, digest(source(`${name}-cases.json`)), name);
    assert.equal(control.probeSha256, digest(source(`${name}-probe.js`)), name);
    assert.equal(control.collectorSha256, digest(source(`${collector}.mjs`)), name);
    assert.equal(control.browser, "Chrome/155.0.8059.80");
    assert.equal(control.v8, "15.5.35.23");
    assert.deepEqual(control.errors, []);
    assert.equal(cases.length, count);
    assert.equal(control.result.length, count);
    assert.deepEqual(
      control.result.map((row) => row.id),
      cases.map((row) => row.id),
    );
  }
});

test("code collector keeps actual CSP enforcement and repeated timer lifecycle evidence", () => {
  assert.match(
    source("trusted-types-code-chrome.mjs").toString(),
    /allowUnsafeEvalBlockedByCSP:\s*false/,
  );
  const control = JSON.parse(source("trusted-types-code-chrome155.json"));
  for (const { result } of control.result) {
    assert.equal(result.repeating, 3);
    assert.equal(result.cancelled, false);
  }
});
