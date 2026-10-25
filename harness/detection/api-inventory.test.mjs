import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { inventorySnapshot } from "./api-inventory.mjs";
import { compareInventories } from "./api-inventory-diff.mjs";

test("durable inventory separates absent interfaces and preserves every observed difference", () => {
  const measured = (missing) => ({ measured: true, missing, extra: [], changed: [] });
  const evidence = {
    binaryHash: "frozen executable",
    collectorHash: "frozen collector",
    chromeV8Version: "15.6.75.19",
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
  assert.equal(snapshot.chromeV8Version, "15.6.75.19");
});

const measured = (missing, extra = [], changed = []) => ({
  measured: true,
  missing,
  extra,
  changed,
});

test("inventory diff compares only shared interfaces and reports identity changes", () => {
  const previous = {
    chromeVersion: "Chrome/155.0.8059.85",
    build: { engine: { v8Version: "15.5.35.23-rusty" }, features: { gpuMetal: true } },
    collectorHash: "same",
    interfaces: {
      Document: measured(["all", "domain"], [], [{ path: "a.enumerable" }]),
      Range: measured(["expand"]),
      Gone: measured(["x"]),
      Absent: { measured: false, chrome: true, native: false },
    },
    differences: [
      { path: "constructors.Document.prototypeMembers.all" },
      { path: "constructors.Gone.prototypeMembers.x" },
      { path: "objects.navigator.ownKeys" },
    ],
  };
  const current = {
    chromeVersion: "Chrome/156.0.8078.47",
    build: { engine: { v8Version: "15.0.0-rusty" }, features: { gpuMetal: true } },
    collectorHash: "same",
    interfaces: {
      Document: measured(["domain", "timeline"], ["fake"]),
      Range: measured(["expand"]),
      Absent: measured([]),
      Added: measured(["y"]),
    },
    differences: [
      { path: "constructors.Document.prototypeMembers.timeline" },
      { path: "constructors.Added.prototypeMembers.y" },
      { path: "objects.navigator.ownKeys", chrome: ["a"], native: ["b"] },
    ],
  };
  const diff = compareInventories(previous, current);
  assert.deepEqual(diff.identityChanges, ["chromeVersion", "v8Version"]);
  assert.equal(diff.sameScope.interfaceCount, 2);
  assert.equal(diff.sameScope.previousMissing, 3);
  assert.equal(diff.sameScope.currentMissing, 3);
  assert.equal(diff.sameScope.resolved, 1);
  assert.equal(diff.sameScope.introduced, 1);
  assert.deepEqual(diff.interfaces.Document, {
    resolvedMissing: ["all"],
    introducedMissing: ["timeline"],
    resolvedExtra: [],
    introducedExtra: ["fake"],
    resolvedChanged: ["a.enumerable"],
    introducedChanged: [],
  });
  assert.equal(diff.interfaces.Range, undefined);
  assert.deepEqual(diff.onlyPrevious, ["Gone"]);
  assert.deepEqual(diff.onlyCurrent, ["Absent", "Added"]);
  // Gone/Added constructor paths are out of scope; the shared global path is kept.
  assert.equal(diff.sameScope.previousDifferencePaths, 2);
  assert.equal(diff.sameScope.resolvedDifferencePaths, 1);
  assert.equal(diff.sameScope.introducedDifferencePaths, 1);
  assert.equal(diff.sameScope.globalPathsCompared, true);
  assert.equal(diff.sameScope.valueChangedDifferencePaths, 1);
  const changedCollector = compareInventories(previous, { ...current, collectorHash: "new" });
  assert.equal(changedCollector.sameScope.previousDifferencePaths, 1);
  assert.equal(changedCollector.sameScope.globalPathsCompared, false);
});

test("expanded collector keeps the base scope and byte-identical descriptor helpers", () => {
  const read = (name) => readFileSync(new URL(name, import.meta.url), "utf8");
  const base = read("api-shape.js");
  const expanded = read("api-shape-interfaces.js");
  // Everything from `safe` through `members` serializes descriptors; it must not drift.
  const helpers = (source, next) =>
    source.slice(source.indexOf("  const safe"), source.indexOf(`  const ${next} =`));
  assert.ok(helpers(base, "owner").length > 1000);
  assert.equal(helpers(expanded, "data"), helpers(base, "owner"));
  const block = (source) => source.slice(source.indexOf("    data.constructors[name] = safe"));
  assert.equal(
    block(expanded).slice(0, block(expanded).indexOf("  }\n")),
    block(base).slice(0, block(base).indexOf("  }\n")),
  );
  const names = (source) => {
    const list = /for \(const name of \[\n([^\]]*)\]\) \{\n {4}data\.constructors/.exec(source)[1];
    return [...list.matchAll(/"([^"]+)"/g)].map((match) => match[1]);
  };
  assert.equal(names(base).length, 75);
  assert.deepEqual(names(expanded).slice(0, 75), names(base));
  assert.equal(new Set(names(expanded)).size, names(expanded).length);
  const report = JSON.parse(
    runInNewContext(`
      class EventTarget { addEventListener() {} }
      class Node extends EventTarget { get hostile() { throw new Error('accessor invoked'); } }
      Object.assign(globalThis, { EventTarget, Node });
      ${expanded}
    `),
  );
  assert.equal(report.constructors.Node.prototypeMembers.hostile.kind, "accessor");
  assert.deepEqual(report.constructors.EventTarget.prototypeOwnKeyOrder, [
    "constructor",
    "addEventListener",
  ]);
  assert.deepEqual(report.constructors.HTMLDialogElement, { error: "TypeError" });
});
