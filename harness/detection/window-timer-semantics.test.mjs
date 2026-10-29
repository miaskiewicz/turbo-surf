import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { test } from "node:test";

// The fixture is created only by a genuine frozen-Chrome capture; no synthetic fallback.
test("Window timer control pins actual task semantics and separates timing observations", () => {
  const control = JSON.parse(
    readFileSync(
      new URL("./fixtures/window-timer-semantics-chrome156.json", import.meta.url),
      "utf8",
    ),
  );
  assert.deepEqual(
    Object.keys(control.sourceHashes).sort(),
    [
      "window-timer-semantics-chrome.mjs",
      "window-timer-semantics-probe.js",
      "window-timer-semantics.html",
      "chrome-cdp-trace.mjs",
      "observation-window.mjs",
      "main-navigation.mjs",
      "collector-lifecycle.mjs",
    ].sort(),
  );
  for (const [name, expected] of Object.entries(control.sourceHashes))
    assert.equal(
      createHash("sha256")
        .update(readFileSync(new URL(name, import.meta.url)))
        .digest("hex"),
      expected,
      name,
    );
  assert.deepEqual(control.errors, []);
  assert.deepEqual(control.result.semantics, {
    initialCheckpoint: ["first", "microtask", "deep microtask", "observer", "second"],
    initialRealmOrder: ["frame", "parent"],
    cancellations: [false, false],
    intervalCancellation: 1,
    mathReplacement: { created: true, fired: true },
  });
  assert.deepEqual(control.result.protocol, {
    requestedDelayMs: 0,
    cycles: 12,
    initialParentDelayMs: 100,
    resetSource: "MessageChannel",
  });
  assert.deepEqual(
    Object.keys(control.result.timing).sort(),
    [
      "direct",
      "promise",
      "queueMicrotask",
      "crossRealm",
      "crossRealmPromise",
      "crossRealmQueueMicrotask",
      "interval",
    ].sort(),
  );
  for (const rows of Object.values(control.result.timing)) {
    assert.equal(rows.length, 12);
    assert.ok(rows.every((value) => Number.isFinite(value) && value >= 0));
  }
});
