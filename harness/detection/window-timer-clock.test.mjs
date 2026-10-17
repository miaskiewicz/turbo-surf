import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const control = JSON.parse(
  readFileSync(new URL("./fixtures/window-timer-clock-chrome155.json", import.meta.url)),
);
const digest = (file) =>
  createHash("sha256")
    .update(readFileSync(new URL(file, import.meta.url)))
    .digest("hex");

test("Window/native timer controls retain independently measured ordering and cancellation", () => {
  assert.equal(control.browser, "Chrome/155.0.8059.80");
  assert.equal(control.v8, "15.5.35.23");
  assert.deepEqual(control.errors, []);
  assert.equal(control.probeSha256, digest("./window-timer-clock-probe.js"));
  assert.equal(control.collectorSha256, digest("./window-timer-clock-chrome.mjs"));
  assert.equal(control.serverSha256, digest("./window-timer-clock-server.mjs"));
  assert.deepEqual(control.protocol, {
    bodyDelayMs: 20,
    bodyWindowMs: 100,
    abortWindowMs: 30,
    nativeTimeoutMs: 20,
    nativeTimeoutWindowMs: 30,
    gcRounds: 3,
  });
  assert.deepEqual(control.result, {
    bodyBeforeLaterTimer: true,
    windowDeadlineAbort: "AbortError",
    retainedNativeTimeout: true,
    heldNativeTimeout: true,
  });
  assert.deepEqual(control.serverReceipts, {
    delayedBody: { requests: 1, headersFlushed: true, bodyWritten: true },
    stalledBody: { requests: 1, headersFlushed: true, bodyWritten: false, socketClosed: true },
  });
});
