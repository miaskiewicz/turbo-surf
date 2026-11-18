"use strict";

const fs = require("node:fs");
const { checkCompleteness } = require("./js-test-completeness.cjs");
const path = require("node:path");
const { pathToFileURL, fileURLToPath } = require("node:url");
const { Readable } = require("node:stream");
const { pipeline } = require("node:stream/promises");
const {
  scope,
  sha,
  verifySources,
  checkSummary,
  checkRun,
  checkLoaded,
} = require("./js-coverage.cjs");

function rawFiles(directory) {
  const names = fs
    .readdirSync(directory)
    .filter((f) => /^coverage-\d+-\d+-\d+\.json$/.test(f))
    .sort();
  if (!names.length) throw new Error("No raw V8 coverage reports");
  const loaded = new Set();
  const pins = names.map((name) => {
    const file = path.join(directory, name);
    const report = JSON.parse(fs.readFileSync(file, "utf8"));
    if (!Array.isArray(report.result)) throw new Error(`Invalid raw V8 report: ${file}`);
    for (const script of report.result) {
      if (typeof script.url === "string" && script.url.startsWith("file:"))
        loaded.add(fileURLToPath(script.url));
    }
    return { path: file, sha256: sha(file) };
  });
  return { pins, loaded };
}

function includeUnloaded(coverage, root) {
  // Node's native include-all zero record, restricted to the reviewed tracked list.
  const original = coverage.getCoverageFromDirectory.bind(coverage);
  coverage.getCoverageFromDirectory = () => {
    const merged = original();
    const seen = new Set(merged.map((row) => row.url));
    for (const file of scope.files) {
      const url = pathToFileURL(path.join(root, file)).href;
      if (seen.has(url)) continue;
      const lines = coverage.getLines(url);
      if (!lines?.length) throw new Error(`No source lines: ${file}`);
      const last = lines.at(-1);
      merged.push({
        url,
        functions: [
          {
            functionName: "",
            isBlockCoverage: false,
            ranges: [{ startOffset: 0, endOffset: last.startOffset + last.src.length, count: 0 }],
          },
        ],
      });
    }
    return merged;
  };
}

async function aggregate(directory) {
  if (process.version !== scope.nodeVersion)
    throw new Error(`Coverage requires Node ${scope.nodeVersion}`);
  if (process.env.NODE_V8_COVERAGE) throw new Error("Unset NODE_V8_COVERAGE during aggregation");
  const run = JSON.parse(fs.readFileSync(path.join(directory, "run.json"), "utf8"));
  verifySources(run.root, run.sources);
  const rawDirectory = path.join(directory, "raw");
  const { pins, loaded } = rawFiles(rawDirectory);
  const { TestCoverage } = require("internal/test_runner/coverage");
  const LcovReporter = require("internal/test_runner/reporter/lcov");
  const coverage = new TestCoverage(rawDirectory, undefined, {
    cwd: run.root,
    coverageIncludeAll: false,
    coverageIncludeGlobs: scope.files,
    coverageExcludeGlobs: [],
    sourceMaps: true,
  });
  includeUnloaded(coverage, run.root);
  const summary = coverage.summary();
  delete summary.thresholds;
  const gate = checkSummary(summary, run.root);
  const unloadedFiles = scope.files.filter((file) => !loaded.has(path.join(run.root, file)));
  let inventoryError = null;
  try {
    checkLoaded(loaded, run.root);
  } catch (error) {
    inventoryError = error.message;
  }
  let testError = null;
  try {
    checkRun(run);
  } catch (error) {
    testError = error.message;
  }
  let completeness = null;
  let completenessError = null;
  try {
    completeness = checkCompleteness(run, loaded);
  } catch (error) {
    completenessError = error.message;
  }
  const receipt = {
    nodeVersion: process.version,
    nodeBinarySha256: sha(process.execPath),
    scopeSha256: sha(path.join(__dirname, "js-coverage-scope.json")),
    aggregatorSha256: sha(__filename),
    checkerSha256: sha(path.join(__dirname, "js-coverage.cjs")),
    sourceRoot: run.root,
    commands: run.commands,
    executionInputs: run.executionInputs,
    scope: "product-launcher-shim-binding",
    raw: pins,
    sources: run.sources,
    metrics: gate.metrics,
    testsPassed: testError === null && completenessError === null,
    completeness,
    completenessError,
    testError,
    unloadedFiles,
    inventoryError,
    pass: gate.pass && testError === null && inventoryError === null && completenessError === null,
    limitation:
      "Node-host V8 coverage only. Unloaded files have zero-covered lines; nested function/branch counts are runtime-dependent, not statically inferred. No native or guest-isolate coverage claim.",
  };
  verifySources(run.root, run.sources);
  if (JSON.stringify(pins) !== JSON.stringify(rawFiles(rawDirectory).pins))
    throw new Error("Raw V8 coverage changed");
  fs.writeFileSync(path.join(directory, "coverage.json"), `${JSON.stringify(summary, null, 2)}\n`);
  await pipeline(
    Readable.from([{ type: "test:coverage", data: { summary } }]),
    new LcovReporter(),
    fs.createWriteStream(path.join(directory, "lcov.info")),
  );
  fs.writeFileSync(path.join(directory, "gate.json"), `${JSON.stringify(receipt, null, 2)}\n`);
  console.log(JSON.stringify(receipt.metrics, null, 2));
  if (!receipt.pass)
    throw new Error(
      testError ||
        inventoryError ||
        completenessError ||
        "JS coverage below 90% (lines, branches and functions)",
    );
}

aggregate(path.resolve(process.argv[2] || "coverage/js")).catch((error) => {
  console.error(error.stack);
  process.exitCode = 1;
});
