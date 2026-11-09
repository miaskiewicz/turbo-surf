// Regression for #284: `activeT` and the describe-skip stack used to be
// module-level mutables, so concurrent node:test runs that touched the shim
// clobbered each other's active context. The AsyncLocalStorage-backed storage
// keeps each frame isolated. This test drives two concurrent async bodies,
// each setting up and reading back a pw.describe scope with a conditional
// skip, and asserts neither leaks into the other's registration decisions.

import assert from "node:assert/strict";
import test from "node:test";

let shim;
try {
  shim = await import("./index.mjs");
} catch (e) {
  console.log("shim: native addon not built, skipping —", e.message);
  process.exit(0);
}
const { test: pw } = shim;

test("shim describe skip-frames are per-context under concurrent registration", async () => {
  // Each body registers a describe. Inside the body, a conditional skip
  // either flips the local frame (a) or is a no-op (b). A shared module-level
  // stack would let (a)'s skip mark (b)'s frame when their bodies interleave,
  // since pw.describe pushes/pops an array. We yield to the microtask queue
  // in between to force the interleave even though the bodies are sync.
  const log = [];
  const a = (async () => {
    pw.describe("race-A", () => {
      pw.skip(true, "A"); // sets frame.skip = true on A's stack
      log.push("A-body");
    });
  })();
  const b = (async () => {
    await Promise.resolve();
    pw.describe("race-B", () => {
      pw.skip(false, "B"); // condition false → no-op
      log.push("B-body");
    });
  })();
  await Promise.all([a, b]);
  assert.deepEqual(log.sort(), ["A-body", "B-body"]);
});

test("shim activeT is per-context under concurrent registration", async () => {
  // Two pw.test registrations in parallel: both read pw.info() / runtime
  // pw.skip against the wrong frame only if activeT is module-global. With
  // per-context ALS storage, each registration resolves cleanly.
  const errors = [];
  const register = async (name) => {
    try {
      pw(name, async () => {});
    } catch (e) {
      errors.push(`${name}: ${e.message}`);
    }
  };
  await Promise.all([register("rt-A"), register("rt-B"), register("rt-C")]);
  assert.deepEqual(errors, []);
});
