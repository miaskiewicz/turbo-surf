import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";

test("raw CDP oracle skips cleanly when the Chrome dependency is absent", () => {
  const output = execFileSync(
    process.execPath,
    [
      fileURLToPath(new URL("./window-events-cdp.mjs", import.meta.url)),
      "--chrome=/nonexistent/turbo-surf-offline-oracle",
    ],
    { encoding: "utf8" },
  );
  const result = JSON.parse(output);
  assert.equal(result.skipped, true);
  assert.match(result.reason, /Chrome/);
});
