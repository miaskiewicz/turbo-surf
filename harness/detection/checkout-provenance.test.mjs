import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { checkoutProvenance } from "./checkout-provenance.mjs";

test("provenance hashes complete large diffs and untracked source files", async () => {
  const root = mkdtempSync(join(tmpdir(), "surf-provenance-"));
  const git = (...args) => execFileSync("git", args, { cwd: root, maxBuffer: 4 * 1024 * 1024 });
  try {
    git("init", "--quiet");
    writeFileSync(join(root, "source.js"), "export const original = true;\n");
    git("add", "source.js");
    git(
      "-c",
      "user.name=Trace Fixture",
      "-c",
      "user.email=trace@example.test",
      "-c",
      "commit.gpgsign=false",
      "commit",
      "--quiet",
      "-m",
      "fixture",
    );
    writeFileSync(join(root, "source.js"), "const line = 'changed';\n".repeat(60_000));
    writeFileSync(join(root, "new-source.js"), "new implementation\n");
    const result = await checkoutProvenance(root);
    const diff = git("diff", "--no-ext-diff", "HEAD");
    assert.ok(diff.length > 1024 * 1024);
    assert.equal(result.dirtyDiffHash, createHash("sha256").update(diff).digest("hex"));
    assert.deepEqual(result.untrackedHashes, {
      "new-source.js": createHash("sha256").update("new implementation\n").digest("hex"),
    });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
