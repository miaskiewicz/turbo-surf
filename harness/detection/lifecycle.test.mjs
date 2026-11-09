import assert from "node:assert/strict";
import { test } from "node:test";
import { lifecycleErrors } from "./lifecycle.mjs";

test("lifecycle oracle flags image blockers and partial resource success", () => {
  const errors = lifecycleErrors({
    events: [],
    imageAtDcl: true,
    image: { complete: true, width: 0, height: 0 },
    broken: { complete: false, width: 3 },
    linked: { rules: [] },
    dynamic: {},
    control: "",
  });
  assert.deepEqual(errors, [
    "lifecycle ordering",
    "image delayed DOMContentLoaded",
    "decoded image dimensions",
    "invalid image load semantics",
    "stylesheet import graph",
    "dynamic resources",
    "parsed hidden control",
    "host event dispatch state",
  ]);
});

test("lifecycle oracle checks host event dispatch state per phase", () => {
  const valid = {
    events: [
      "classic:loading",
      "body:loading",
      "state:interactive",
      "defer:interactive",
      "dcl:interactive",
      "image:load",
      "image:error",
      "state:complete",
      "load:complete",
      "pageshow",
    ],
    imageAtDcl: false,
    image: { complete: true, width: 3, height: 2 },
    broken: { complete: true, width: 0 },
    linked: { imported: "em", rules: ["import", "sentinel"] },
    dynamic: { imageWidth: 3, styleRule: "dynamic-sentinel" },
    control: "parsed-token",
    hostEvents: Array(5).fill({ trusted: true, phase: 2, timestampValid: true }),
    completedEventStates: [
      ...Array(3).fill({ trusted: true, currentTarget: null, phase: 0 }),
      ...Array(2).fill({ trusted: true, currentTarget: "window", phase: 2, pathLength: 1 }),
    ],
    redispatchStates: Array(2).fill({ error: "InvalidStateError", trusted: true }),
  };
  assert.deepEqual(lifecycleErrors(valid), []);
  const swapped = [...valid.completedEventStates].reverse();
  assert.deepEqual(lifecycleErrors({ ...valid, completedEventStates: swapped }), [
    "host event dispatch state",
  ]);
  assert.deepEqual(
    lifecycleErrors({ ...valid, redispatchStates: [{ error: "TypeError", trusted: true }, {}] }),
    ["host event dispatch state"],
  );
});
