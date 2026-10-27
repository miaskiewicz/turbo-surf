import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

// chrome-trace.mjs passes Patchright's 3-argument `evaluate(fn, arg, isolatedContext)`;
// Patchright 1.62.3 and 1.64.2 reject it ("Too many arguments"). The sidecar pins the
// last release the recorded Chrome 156 fixtures were captured with, and commits the
// lockfile so a fresh `npm ci` cannot float to a newer major-compatible release.
const PATCHRIGHT = "1.59.4";
const sidecar = new URL("../../scripts/browser-sidecar/", import.meta.url);
const repo = fileURLToPath(new URL("../../", import.meta.url));

test(
  "browser sidecar pins the Patchright release chrome-trace.mjs is written against (turbo-surf#53)",
  { todo: "turbo-surf#53: fails until the fix" },
  () => {
    const manifest = JSON.parse(readFileSync(new URL("package.json", sidecar), "utf8"));
    assert.deepEqual(manifest.dependencies, { patchright: PATCHRIGHT });
    const lock = JSON.parse(readFileSync(new URL("package-lock.json", sidecar), "utf8"));
    assert.deepEqual(lock.packages[""].dependencies, { patchright: PATCHRIGHT });
    assert.equal(lock.packages["node_modules/patchright"].version, PATCHRIGHT);
    assert.deepEqual(lock.packages["node_modules/patchright"].dependencies, {
      "patchright-core": PATCHRIGHT,
    });
    assert.equal(lock.packages["node_modules/patchright-core"].version, PATCHRIGHT);
    assert.equal(
      execFileSync("git", ["ls-files", "scripts/browser-sidecar/package-lock.json"], {
        cwd: repo,
        encoding: "utf8",
      }).trim(),
      "scripts/browser-sidecar/package-lock.json",
      "the sidecar lockfile must be committed",
    );
  },
);
