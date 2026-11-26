"use strict";

const assert = require("node:assert/strict");
const test = require("node:test");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const { testInventory } = require("./js-test-completeness.cjs");
const {
  scope,
  snapshot,
  sha,
  checkSummary,
  checkRun,
  checkLoaded,
  verifySources,
  thresholdBasisPoints,
} = require("./js-coverage.cjs");

function report(covered = 100, total = 100) {
  const counts = {};
  for (const metric of ["Line", "Branch", "Function"]) {
    counts[`total${metric}Count`] = total;
    counts[`covered${metric}Count`] = covered;
  }
  return { files: [{ path: "/fixture/a.js", ...counts }], totals: { ...counts } };
}
const check = (value) => checkSummary(value, "/fixture", ["a.js"]);

test("every tracked product runtime file participates in the coverage snapshot", () => {
  const sources = snapshot(path.resolve(__dirname, ".."));
  assert.equal(sources.length, scope.files.length);
  assert.ok(sources.every((source) => /^[a-f0-9]{64}$/.test(source.sha256)));
});

test("line floor is exactly 92.5% while branches and functions retain their 90% floors", () => {
  assert.deepEqual(scope.floors, { lines: 92.5, branches: 90, functions: 90 });
  assert.equal(check(report()).pass, true);
  for (const [metric, below] of [
    ["Line", 92499],
    ["Branch", 89999],
    ["Function", 89999],
  ]) {
    const value = report(100000, 100000);
    value.files[0][`covered${metric}Count`] = below;
    value.totals[`covered${metric}Count`] = below;
    assert.equal(check(value).pass, false);
  }
});

test("92.49% fails, 92.5% passes and 92.51% passes without rounding", () => {
  for (const [covered, pass] of [
    [9249, false],
    [9250, true],
    [9251, true],
  ]) {
    const value = report(9000, 10000);
    value.files[0].coveredLineCount = covered;
    value.totals.coveredLineCount = covered;
    assert.equal(check(value).pass, pass);
    assert.equal(check(value).metrics.branches.pass, true);
    assert.equal(check(value).metrics.functions.pass, true);
  }
});

test("thresholds use exact basis points and reject invalid or unsupported values", () => {
  for (const [value, points] of [
    [0, 0n],
    [0.01, 1n],
    [90, 9000n],
    [92.5, 9250n],
    [92.49, 9249n],
    [100, 10000n],
  ])
    assert.equal(thresholdBasisPoints(value), points);
  for (const value of [undefined, null, "92.5", true, {}, NaN, Infinity, -1, 100.01, 92.501]) {
    assert.throws(() => thresholdBasisPoints(value), /Coverage floor/);
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
  fs.mkdirSync(path.join(root, "scripts"), { recursive: true });
  fs.writeFileSync(
    path.join(root, "scripts/local-addon.test.mjs"),
    "export const loaded = true;\n",
  );
  fs.writeFileSync(
    path.join(root, "package.json"),
    JSON.stringify({
      scripts: {
        "test:shim": "node --test scripts/local-addon.test.mjs",
        "test:e2e": "node --test scripts/local-addon.test.mjs",
        "test:detection": "node --test scripts/local-addon.test.mjs",
      },
    }),
  );
  const summary = (extra = "", tests = 1, skipped = 0) =>
    `ok 1 - fixture behavior\n${extra}# tests ${tests}\n# pass 1\n# fail 0\n# cancelled 0\n# skipped ${skipped}\n# todo 0\n`;
  const logs = {
    playwright:
      summary() +
      summary("ok 2 - skipped test does not run # SKIP\n", 2, 1) +
      summary(
        "ok 2 - ordinary must not run # SKIP excluded by focused tests\nok 3 - ordinary must not run # SKIP excluded by focused tests\n",
        3,
        2,
      ),
    detection: summary() + "Ran 1 test in 0.1s\nOK\nRan 1 test in 0.1s\nOK\n",
  };
  for (const [name, text] of Object.entries(logs))
    fs.writeFileSync(path.join(directory, `${name}.log`), text);
  const sources = scope.files.map((file) => ({ path: file, sha256: sha(path.join(root, file)) }));
  const commands = ["playwright", "detection"].map((name) => ({
    name,
    exit: 0,
    log: path.join(directory, `${name}.log`),
    logSha256: sha(path.join(directory, `${name}.log`)),
  }));
  fs.writeFileSync(
    path.join(directory, "run.json"),
    JSON.stringify({ root, sources, commands, ...testInventory(root) }),
  );
  return { directory, root, sources };
}

test("an unregistered shim regression cannot disappear from the full test inventory", (t) => {
  const { root } = fixture(t);
  const file = "rust/playwright-shim/new-regression.test.mjs";
  fs.writeFileSync(path.join(root, file), "export const regression = true;\n");
  assert.throws(() => testInventory(root), /Unregistered shim test file/);
  const packagePath = path.join(root, "package.json");
  const pkg = JSON.parse(fs.readFileSync(packagePath, "utf8"));
  pkg.scripts["test:shim"] += ` ${file}`;
  fs.writeFileSync(packagePath, JSON.stringify(pkg));
  assert.ok(testInventory(root).testSources.some((source) => source.path === file));
});

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

test("real V8 merge retains the full inventory, including zero-hit unloaded sources", (t) => {
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
  assert.equal(summary.files.length, scope.files.length);
  assert.equal(
    summary.files.filter((f) => f.coveredLineCount === 0).length,
    scope.files.length - 1,
  );
  const gate = JSON.parse(fs.readFileSync(path.join(directory, "gate.json"), "utf8"));
  assert.equal(gate.testsPassed, false);
  assert.equal(gate.unloadedFiles.length, scope.files.length - 1);
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
  const urls = [...scope.files, "scripts/local-addon.test.mjs"].map(
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
