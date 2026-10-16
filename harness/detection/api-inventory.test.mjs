import { test } from "node:test";
import assert from "node:assert/strict";
import { inventorySnapshot } from "./api-inventory.mjs";

test("durable inventory separates absent interfaces and preserves every observed difference", () => {
  const measured = (missing) => ({ measured: true, missing, extra: [], changed: [] });
  const evidence = {
    binaryHash: "frozen executable",
    collectorHash: "frozen collector",
    members: {
      Document: measured(["domain"]),
      Blob: measured(["textStream"]),
      RadioNodeList: { measured: false, chrome: true, native: false },
    },
    chrome: {
      constructors: { RadioNodeList: { prototypeMembers: { value: {}, constructor: {} } } },
    },
    differences: [
      { path: "constructors.Blob.globalDescriptor.enumerable", chrome: true, native: false },
      { path: "constructors.Document.prototypeOwnKeyOrder", chrome: ["a"], native: ["b"] },
    ],
  };
  const snapshot = inventorySnapshot(evidence);
  assert.equal(snapshot.totals.originalFourMissing, 1);
  assert.equal(snapshot.totals.expandedMeasuredMissing, 2);
  assert.deepEqual(snapshot.totals.unmeasuredInterfaces, ["RadioNodeList"]);
  assert.deepEqual(snapshot.unmeasuredChromeMembers.RadioNodeList, ["value", "constructor"]);
  assert.deepEqual(snapshot.differences, evidence.differences);
  assert.equal(snapshot.binaryHash, evidence.binaryHash);
  assert.equal(snapshot.collectorHash, evidence.collectorHash);
});
