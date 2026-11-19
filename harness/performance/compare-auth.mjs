import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { compareAuthTrials } from "./auth-comparison.mjs";

const manifestPath = resolve(process.argv[2]);
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
assert.equal(manifest.schemaVersion, 1, "Unsupported auth comparison manifest");
const artifacts = new Set();
const readArtifact = (record) => {
  const path = resolve(dirname(manifestPath), record.path);
  assert.ok(!artifacts.has(path), "A repeated trial cannot reuse a report artifact");
  artifacts.add(path);
  const bytes = readFileSync(path);
  assert.equal(
    createHash("sha256").update(bytes).digest("hex"),
    record.sha256,
    "Report hash mismatch",
  );
  return bytes.toString("utf8");
};
const pairs = manifest.pairs.map(({ surf, chrome }) => ({
  surf: { ...surf, tap: readArtifact(surf.artifact) },
  chrome: { ...chrome, report: JSON.parse(readArtifact(chrome.artifact)) },
}));
console.log(JSON.stringify(compareAuthTrials(manifest.expectedCases, pairs), null, 2));
