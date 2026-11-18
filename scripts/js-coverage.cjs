"use strict";

const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const { execFileSync } = require("node:child_process");
const scope = require("./js-coverage-scope.json");
const metrics = { lines: "Line", branches: "Branch", functions: "Function" };
const sha = (file) => crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");

function productCandidate(file) {
  if (["cli.js", "index.js", "rust/crates/turbo-surf-napi/index.js"].includes(file)) return true;
  return (
    file.startsWith("rust/playwright-shim/") &&
    /\.[cm]?js$/.test(file) &&
    !/(?:\.(test|spec)|\/test)\.[cm]?js$/.test(file) &&
    !scope.excludedHelpers.includes(file)
  );
}

function snapshot(root) {
  const tracked = execFileSync("git", ["ls-files", "-z"], { cwd: root, encoding: "utf8" });
  const candidates = tracked.split("\0").filter(productCandidate).sort();
  if (JSON.stringify(candidates) !== JSON.stringify([...scope.files].sort())) {
    throw new Error(
      "Product JS scope changed; review scripts/js-coverage-scope.json before measuring",
    );
  }
  return scope.files.map((file) => ({ path: file, sha256: sha(path.join(root, file)) }));
}

function verifySources(root, files) {
  if (
    !Array.isArray(files) ||
    JSON.stringify(files.map((f) => f.path)) !== JSON.stringify(scope.files)
  ) {
    throw new Error("Missing or changed product source inventory");
  }
  for (const file of files) {
    if (sha(path.join(root, file.path)) !== file.sha256)
      throw new Error(`Source changed: ${file.path}`);
  }
}

function validateCount(row, metric) {
  const kind = metrics[metric];
  const total = row[`total${kind}Count`];
  const covered = row[`covered${kind}Count`];
  if (
    !Number.isSafeInteger(total) ||
    !Number.isSafeInteger(covered) ||
    covered < 0 ||
    total < covered
  ) {
    throw new Error(`Invalid ${metric} counts`);
  }
  return { total, covered };
}

function thresholdBasisPoints(value) {
  if (
    typeof value !== "number" ||
    !Number.isFinite(value) ||
    value < 0 ||
    value > 100 ||
    !/^\d+(?:\.\d{1,2})?$/.test(String(value))
  ) {
    throw new Error(
      "Coverage floor must be a number from 0 to 100 with at most two decimal places",
    );
  }
  const [whole, fraction = ""] = String(value).split(".");
  return BigInt(whole) * 100n + BigInt(fraction.padEnd(2, "0"));
}

function checkSummary(summary, root, files = scope.files) {
  if (!summary || !Array.isArray(summary.files) || !summary.totals)
    throw new Error("Missing coverage report");
  const expected = files.map((f) => path.join(root, f)).sort();
  const actual = summary.files.map((f) => f.path).sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected))
    throw new Error("Coverage source inventory incomplete");
  const results = {};
  for (const metric of Object.keys(metrics)) {
    const counts = summary.files.map((f) => validateCount(f, metric));
    const total = counts.reduce((n, c) => n + c.total, 0);
    const covered = counts.reduce((n, c) => n + c.covered, 0);
    const reported = validateCount(summary.totals, metric);
    if (total !== reported.total || covered !== reported.covered)
      throw new Error(`Inconsistent ${metric} totals`);
    if (total === 0) throw new Error(`Zero ${metric} total cannot pass coverage`);
    results[metric] = {
      covered,
      total,
      percent: (covered * 100) / total,
      floor: scope.floors[metric],
      pass: BigInt(covered) * 10000n >= BigInt(total) * thresholdBasisPoints(scope.floors[metric]),
    };
  }
  return { metrics: results, pass: Object.values(results).every((r) => r.pass) };
}

function checkLoaded(loaded, root, files = scope.files) {
  const missing = files.filter((file) => !loaded.has(path.join(root, file)));
  if (missing.length)
    throw new Error(
      `Unloaded product files have unknown function/branch inventory: ${missing.join(", ")}`,
    );
}

function checkRun(run) {
  const expected = ["playwright", "detection"];
  if (
    !run ||
    !Array.isArray(run.commands) ||
    run.commands.length !== expected.length ||
    expected.some((name, i) => run.commands[i].name !== name || run.commands[i].exit !== 0)
  ) {
    throw new Error("Full JS command inventory missing or tests failed");
  }
}

module.exports = {
  scope,
  sha,
  snapshot,
  verifySources,
  checkSummary,
  checkRun,
  checkLoaded,
  thresholdBasisPoints,
};
