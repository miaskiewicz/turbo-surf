"use strict";

const assert = require("node:assert/strict");
const test = require("node:test");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const {
  scope,
  sha,
  checkSummary,
  checkRun,
  checkLoaded,
  verifySources,
} = require("./js-coverage.cjs");

function report(covered = 90, total = 100) {
  const counts = {};
  for (const metric of ["Line", "Branch", "Function"]) {
    counts[`total${metric}Count`] = total;
    counts[`covered${metric}Count`] = covered;
  }
  return { files: [{ path: "/fixture/a.js", ...counts }], totals: { ...counts } };
}
const check = (value) => checkSummary(value, "/fixture", ["a.js"]);

test("all three metrics must meet the exact unrounded 90% floor", () => {
  assert.equal(check(report()).pass, true);
  assert.equal(check(report(100)).pass, true);
  for (const metric of ["Line", "Branch", "Function"]) {
    const value = report(90000, 100000);
    value.files[0][`covered${metric}Count`] = 89999;
    value.totals[`covered${metric}Count`] = 89999;
    assert.equal(check(value).pass, false);
  }
});

test("empty totals, malformed or missing reports and missing files cannot pass", () => {
  for (const value of [null, {}, report(0, 0), { ...report(), files: [] }]) {
    assert.throws(() => check(value));
  }
  for (const count of [-1, NaN, Infinity, 101, 1.5]) {
    const value = report();
    value.files[0].coveredLineCount = count;
    assert.throws(() => check(value));
  }
  for (const metric of ["Line", "Branch", "Function"]) {
    const empty = report();
    empty.files[0][`total${metric}Count`] = 0;
    empty.files[0][`covered${metric}Count`] = 0;
    empty.totals[`total${metric}Count`] = 0;
    empty.totals[`covered${metric}Count`] = 0;
    assert.throws(() => check(empty), /Zero/);
  }
  const value = report();
  value.totals.coveredFunctionCount = 99;
  assert.throws(() => check(value), /Inconsistent/);
  assert.throws(
    () => check({ ...report(), files: [...report().files, ...report().files] }),
    /inventory/,
  );
});

test("each full test command must succeed; missing, skipped and failed commands fail closed", () => {
  const commands = [
    { name: "playwright", exit: 0 },
    { name: "detection", exit: 0 },
  ];
  assert.doesNotThrow(() => checkRun({ commands }));
  for (const invalid of [
    null,
    {},
    { commands: [] },
    { commands: commands.slice(1) },
    { commands: [commands[0], { name: "detection", exit: 1 }] },
    { commands: [commands[0], { name: "detection" }] },
  ]) {
    assert.throws(() => checkRun(invalid));
  }
});

function temporary(t) {
  const directory = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "surf-js-coverage-")));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  return directory;
}

function fixture(t) {
  const directory = temporary(t);
  const root = path.join(directory, "source");
  for (const file of scope.files) {
    const destination = path.join(root, file);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.writeFileSync(
      destination,
      "function example(x) { if (x) return 1; return 2; }\nexample(true);\n",
    );
  }
  const sources = scope.files.map((file) => ({ path: file, sha256: sha(path.join(root, file)) }));
  const commands = [
    { name: "playwright", exit: 0 },
    { name: "detection", exit: 0 },
  ];
  fs.writeFileSync(path.join(directory, "run.json"), JSON.stringify({ root, sources, commands }));
  return { directory, root, sources };
}

function aggregate(directory) {
  const env = { ...process.env };
  delete env.NODE_V8_COVERAGE;
  return spawnSync(
    process.execPath,
    ["--expose-internals", path.join(__dirname, "aggregate-js-coverage.cjs"), directory],
    { env, encoding: "utf8" },
  );
}

test("source hash or inventory changes are rejected", (t) => {
  const { root, sources } = fixture(t);
  assert.doesNotThrow(() => verifySources(root, sources));
  assert.throws(() => verifySources(root, sources.slice(1)), /inventory/);
  fs.appendFileSync(path.join(root, scope.files[0]), "// changed\n");
  assert.throws(() => verifySources(root, sources), /Source changed/);
});

test("aggregation rejects absent and malformed raw reports", (t) => {
  const { directory } = fixture(t);
  fs.mkdirSync(path.join(directory, "raw"));
  let result = aggregate(directory);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /No raw V8 coverage/);
  fs.writeFileSync(path.join(directory, "raw", "coverage-1-1-0.json"), "{}");
  result = aggregate(directory);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Invalid raw V8 report/);
});

test("real V8 merge retains all 43 files, including zero-hit unloaded sources", (t) => {
  const { directory, root } = fixture(t);
  const raw = path.join(directory, "raw");
  fs.mkdirSync(raw);
  const executed = spawnSync(process.execPath, [path.join(root, "cli.js")], {
    env: { ...process.env, NODE_V8_COVERAGE: raw },
    encoding: "utf8",
  });
  assert.equal(executed.status, 0, executed.stderr);
  const result = aggregate(directory);
  assert.notEqual(result.status, 0, "Unloaded product lines must keep the gate red");
  assert.match(result.stderr, /Unloaded product files/);
  const summary = JSON.parse(fs.readFileSync(path.join(directory, "coverage.json"), "utf8"));
  assert.equal(summary.files.length, 43);
  assert.equal(summary.files.filter((f) => f.coveredLineCount === 0).length, 42);
  const gate = JSON.parse(fs.readFileSync(path.join(directory, "gate.json"), "utf8"));
  assert.equal(gate.testsPassed, true);
  assert.equal(gate.unloadedFiles.length, 42);
  assert.equal(gate.pass, false);
  assert.ok(gate.metrics.lines.total > gate.metrics.lines.covered);
});

test("superficially passing aggregate cannot hide an unloaded product entrypoint", () => {
  assert.equal(check(report(99)).pass, true);
  assert.throws(() => checkLoaded(new Set(), "/fixture", ["a.js"]), /Unloaded product/);
  assert.doesNotThrow(() => checkLoaded(new Set(["/fixture/a.js"]), "/fixture", ["a.js"]));
});

test("real complete V8 inventory passes, but failed tests still reject high coverage", (t) => {
  const { directory, root } = fixture(t);
  const raw = path.join(directory, "raw");
  fs.mkdirSync(raw);
  for (const file of scope.files) {
    fs.appendFileSync(path.join(root, file), "example(false);\n");
  }
  const manifestPath = path.join(directory, "run.json");
  const run = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  run.sources = scope.files.map((file) => ({ path: file, sha256: sha(path.join(root, file)) }));
  fs.writeFileSync(manifestPath, JSON.stringify(run));
  const urls = scope.files.map(
    (file) => require("node:url").pathToFileURL(path.join(root, file)).href,
  );
  const executed = spawnSync(
    process.execPath,
    ["--input-type=module", "-e", `for (const url of ${JSON.stringify(urls)}) await import(url);`],
    { env: { ...process.env, NODE_V8_COVERAGE: raw }, encoding: "utf8" },
  );
  assert.equal(executed.status, 0, executed.stderr);
  let result = aggregate(directory);
  assert.equal(result.status, 0, result.stderr);
  let gate = JSON.parse(fs.readFileSync(path.join(directory, "gate.json"), "utf8"));
  assert.equal(gate.pass, true);
  assert.deepEqual(gate.unloadedFiles, []);
  run.commands[1].exit = 1;
  fs.writeFileSync(manifestPath, JSON.stringify(run));
  result = aggregate(directory);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /tests failed/);
  gate = JSON.parse(fs.readFileSync(path.join(directory, "gate.json"), "utf8"));
  assert.equal(gate.pass, false);
  assert.equal(gate.testsPassed, false);
  assert.equal(
    Object.values(gate.metrics).every((metric) => metric.pass),
    true,
  );
});
