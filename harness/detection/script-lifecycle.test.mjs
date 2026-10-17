import assert from "node:assert/strict";
import { test } from "node:test";
import { scriptLifecycleErrors } from "./script-lifecycle.mjs";
const valid = {
  order: [
    "inline:loading",
    "body:loading",
    "defer:interactive",
    "dcl:interactive",
    "async:interactive",
    "load:complete",
  ],
  asyncReady: true,
  events: [{ type: "load", trusted: true, event: true, target: true }],
};
test("async scripts delay load while DCL releases the server gate", () => {
  assert.deepEqual(scriptLifecycleErrors(valid), []);
  const blocked = { ...valid, order: [...valid.order] };
  [blocked.order[3], blocked.order[4]] = [blocked.order[4], blocked.order[3]];
  assert.deepEqual(scriptLifecycleErrors(blocked), ["script/DCL/load ordering"]);
  assert.deepEqual(
    scriptLifecycleErrors({ ...valid, events: [...valid.events, ...valid.events] }),
    ["native script load event"],
  );
});
