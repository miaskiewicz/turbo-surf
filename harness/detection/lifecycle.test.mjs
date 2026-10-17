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
