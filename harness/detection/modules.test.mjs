import { test } from "node:test";
import assert from "node:assert/strict";
import { moduleErrors, moduleSources, evaluationErrors, frameModuleErrors } from "./modules.mjs";
const event = (type) => [{ type, trusted: true, target: true, currentScript: true }];
const valid = {
  events: [
    "classic:loading",
    "deferred:eval",
    "deferred:load",
    "missing:error",
    "mime:error",
    "dcl",
    "async:eval",
    "async:load",
    "window:load",
  ],
  scriptEvents: {
    deferred: event("load"),
    async: event("load"),
    missing: event("error"),
    mime: event("error"),
  },
  dcl: { deferred: true, async: false, currentScript: true },
  deferred: {
    value: "linked",
    source: { url: "/dep.mjs", currentScript: true, stack: true },
    entry: "/entry.mjs",
    currentScript: true,
  },
  async: { currentScript: true },
  inline: true,
  inlineSources: [
    { url: true, stack: true, local: "independent" },
    { url: true, stack: true, local: "independent" },
  ],
};
test("module oracle validates native completion and allowed lifecycle ordering", () => {
  assert.deepEqual(moduleErrors(valid), []);
  assert.ok(
    moduleErrors({ ...valid, dcl: { ...valid.dcl, async: true } }).includes(
      "DOMContentLoaded module ordering",
    ),
  );
  assert.ok(
    moduleErrors({
      ...valid,
      scriptEvents: { ...valid.scriptEvents, deferred: [...event("load"), ...event("load")] },
    }).includes("deferred module completion"),
  );
  assert.ok(moduleErrors({ ...valid, inline: false }).includes("module currentScript"));
});
test("module graph sources are persisted and import a real gated dependency", () => {
  assert.match(moduleSources["/entry.mjs"], /\.\/dep\.mjs/);
  assert.match(moduleSources["/async.mjs"], /\.\/late-dep\.mjs/);
  assert.match(moduleSources["/dep.mjs"], /import\.meta\.url/);
  assert.match(moduleSources["/dep.mjs"], /new Error/);
});

test("module evaluation oracle rejects TLA lifecycle blocking and wrong error targets", () => {
  const value = {
    dcl: { started: true, done: false },
    load: { started: true, done: false },
    tla: { currentScript: true, readyState: "complete", url: "/tla.mjs" },
    repeat: 1,
    scriptEvents: Object.fromEntries(
      ["tla", "runtime", "parse", "repeat1", "repeat2"].map((id) => [id, event("load")]),
    ),
    errors: [
      ["RangeError", "/runtime-error.mjs"],
      ["SyntaxError", "/parse-error.mjs"],
    ].map(([name, filename]) => ({
      name,
      filename,
      trusted: true,
      target: true,
      currentScript: true,
      line: true,
      brand: "[object ErrorEvent]",
    })),
  };
  assert.deepEqual(evaluationErrors(value), []);
  assert.ok(
    evaluationErrors({ ...value, load: { started: true, done: true } }).includes(
      "top-level await blocks document lifecycle",
    ),
  );
  assert.ok(evaluationErrors({ ...value, errors: [] }).includes("RangeError module Window error"));
  assert.match(moduleSources["/tla.mjs"], /await fetch/);
  assert.throws(() => new Function(moduleSources["/parse-error.mjs"]), SyntaxError);
});

test("frame module oracle rejects shared caches, wrong realms and early lifecycle completion", () => {
  const frame = {
    title: "module child",
    ownArray: true,
    depRuns: 1,
    entryRuns: 1,
    result: { owned: "child", current: true, url: "/modules/frame-entry.mjs" },
    inline: { owned: "child", current: true, url: "/modules/frame-child.html" },
    events: ["module", "dcl", "load"],
  };
  const value = { parent: "parent", unpolluted: true, frames: [frame, structuredClone(frame)] };
  assert.deepEqual(frameModuleErrors(value), []);
  const broken = structuredClone(value);
  broken.frames[0].ownArray = false;
  broken.frames[1].depRuns = 2;
  broken.frames[1].events = ["dcl", "module", "load"];
  assert.deepEqual(frameModuleErrors(broken), [
    "native child realm",
    "independent module graph cache",
    "frame module lifecycle",
  ]);
  assert.ok(frameModuleErrors({ ...value, frames: [] }).includes("parent realm isolation"));
  assert.match(moduleSources["/modules/frame-entry.mjs"], /import.*frame-dep/);
  assert.match(moduleSources["/modules/frame-child.html"], /type=['"]module['"]/);
});
