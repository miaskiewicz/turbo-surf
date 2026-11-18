"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { createHash } = require("node:crypto");
const digest = (value) => createHash("sha256").update(value).digest("hex");
const intentionalSkips = [
  { name: "skipped test does not run", reason: "", count: 1 },
  { name: "ordinary must not run", reason: "excluded by focused tests", count: 2 },
];
const testPattern = /(?:\.(?:test|spec)|\/test)\.[cm]?js$/;

const environmentKeys = [
  "NODE_OPTIONS",
  "NODE_V8_COVERAGE",
  "TURBO_SURF_NAPI_PATH",
  "TURBO_SURF_DETECTION_BINARY",
  "TURBO_SURF_CHROME_PATH",
  "TURBO_CHROME_PATH",
  "TURBO_SURF_CHROME_HEADLESS",
  "TURBO_SURF_BROWSER_CMD",
  "TURBO_SURF_BROWSER_FETCH_CMD",
  "TURBO_SURF_DETECTION_TEST_ARTIFACTS",
  "TURBO_SURF_CHROME_ARGS",
  "CARGO_HOME",
  "CARGO_TARGET_DIR",
  "RUSTY_V8_ARCHIVE",
  "PWD",
];

function executionInputs(root, env) {
  const environment = Object.fromEntries(
    environmentKeys.map((key) => [
      key,
      {
        present: Object.hasOwn(env, key),
        nonempty: Boolean(env[key]),
        sha256: Object.hasOwn(env, key) ? digest(env[key]) : null,
      },
    ]),
  );
  const sidecar = path.join(root, "scripts/browser-sidecar/node_modules/patchright/package.json");
  return {
    environment,
    sidecarPackageRealPath: fs.existsSync(sidecar) ? fs.realpathSync(sidecar) : null,
    sidecarPackageSha256: fs.existsSync(sidecar) ? digest(fs.readFileSync(sidecar)) : null,
  };
}

function testInventory(root) {
  const packagePath = path.join(root, "package.json");
  const packageBytes = fs.readFileSync(packagePath);
  const { scripts } = JSON.parse(packageBytes);
  const files = new Set(["scripts/local-addon.test.mjs"]);
  for (const name of ["test:shim", "test:e2e", "test:detection"]) {
    const patterns = scripts[name].split(/\s+/).filter((word) => testPattern.test(word));
    if (!patterns.length) throw new Error(`No registered test files in ${name}`);
    for (const pattern of patterns) {
      const matches = fs.globSync(pattern, { cwd: root });
      if (!matches.length) throw new Error(`Registered test pattern matched no files: ${pattern}`);
      for (const file of matches) files.add(file);
    }
  }
  return {
    packageSha256: digest(packageBytes),
    testSources: [...files]
      .sort()
      .map((file) => ({ path: file, sha256: digest(fs.readFileSync(path.join(root, file))) })),
  };
}

function testSummaries(text) {
  const summaries = [];
  for (const line of text.split(/\r?\n/)) {
    const match = /^# (tests|pass|fail|cancelled|skipped|todo) (\d+)$/.exec(line);
    if (!match) continue;
    if (match[1] === "tests") summaries.push({});
    const current = summaries.at(-1);
    if (!current || match[1] in current) throw new Error("Malformed TAP summary");
    current[match[1]] = Number(match[2]);
  }
  return summaries;
}

function skipRows(text) {
  return text.split(/\r?\n/).flatMap((line) => {
    const match = /^\s*ok \d+ - (.*?) # SKIP(?: (.*))?$/.exec(line);
    return match ? [{ name: match[1], reason: match[2] ?? "" }] : [];
  });
}

function checkSummaries(text, expected) {
  const summaries = testSummaries(text);
  if (summaries.length !== expected) throw new Error("Missing official Node TAP summaries");
  for (const row of summaries) {
    if (
      !(row.tests > 0) ||
      !(row.pass > 0) ||
      row.fail !== 0 ||
      row.cancelled !== 0 ||
      row.todo !== 0 ||
      !Number.isInteger(row.skipped) ||
      row.tests !== row.pass + row.skipped
    ) {
      throw new Error("Incomplete, failed, cancelled, empty or todo JS test run");
    }
  }
  const skipped = summaries.reduce((n, row) => n + row.skipped, 0);
  if (skipRows(text).length !== skipped) throw new Error("Skipped test accounting is incomplete");
  const fallback = /^\s*ok \d+ - .*\/(?:[^/]+\.(?:test|spec)\.[cm]?js|test\.mjs)\s*$/m;
  if (fallback.test(text)) throw new Error("Test file exited without registering test cases");
  return summaries;
}

function checkSkips(rows) {
  const remaining = intentionalSkips.map((row) => ({ ...row }));
  for (const row of rows) {
    const allowed = remaining.find((item) => item.name === row.name && item.reason === row.reason);
    if (!allowed || allowed.count === 0) throw new Error(`Unexpected skipped test: ${row.name}`);
    allowed.count--;
  }
  if (remaining.some((row) => row.count)) throw new Error("Intentional skip inventory changed");
}

function checkCompleteness(run, loaded) {
  const current = testInventory(run.root);
  if (
    JSON.stringify(current) !==
    JSON.stringify({ packageSha256: run.packageSha256, testSources: run.testSources })
  )
    throw new Error("Registered test sources/package changed");
  for (const file of current.testSources) {
    if (!loaded.has(path.join(run.root, file.path)))
      throw new Error(`Test file not loaded: ${file.path}`);
  }
  const allSkips = [];
  const results = {};
  for (const row of run.commands) {
    const bytes = fs.readFileSync(row.log);
    if (digest(bytes) !== row.logSha256) throw new Error("Test log changed");
    const text = bytes.toString("utf8");
    results[row.name] = checkSummaries(text, row.name === "playwright" ? 3 : 1);
    allSkips.push(...skipRows(text));
    if (
      row.name === "detection" &&
      (text.match(/^Ran [1-9]\d* tests? in /gm)?.length !== 2 || text.match(/^OK$/gm)?.length !== 2)
    )
      throw new Error("Offline Python helper runs missing");
  }
  checkSkips(allSkips);
  return {
    registeredFiles: current.testSources.length,
    summaries: results,
    intentionalSkips: allSkips,
  };
}

module.exports = {
  executionInputs,
  testInventory,
  testSummaries,
  skipRows,
  checkSummaries,
  checkSkips,
  checkCompleteness,
};
