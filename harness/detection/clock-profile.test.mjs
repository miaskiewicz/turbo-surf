import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

test("fixed-epoch timezone control preserves UTC instants and measures both DST states", () => {
  const source = readFileSync(new URL("clock-profile-probe.js", import.meta.url), "utf8");
  const run = (timezone) =>
    JSON.parse(
      execFileSync(
        process.execPath,
        [
          "--input-type=module",
          "-e",
          `console.log(JSON.stringify(${source.trim().replace(/;$/, "")}))`,
        ],
        { encoding: "utf8", env: { ...process.env, TZ: timezone } },
      ),
    );
  const pacific = run("America/Los_Angeles"),
    utc = run("UTC");
  assert.equal(pacific.timezone, "America/Los_Angeles");
  assert.deepEqual(
    pacific.dates.map((date) => date.offset),
    [480, 420],
  );
  assert.deepEqual(
    pacific.dates.map((date) => date.local),
    [
      [2026, 0, 1, 4, 0],
      [2026, 6, 1, 5, 0],
    ],
  );
  assert.deepEqual(
    pacific.dates.map((date) => date.epoch),
    utc.dates.map((date) => date.epoch),
  );
  assert.deepEqual(
    pacific.dates.map((date) => date.iso),
    utc.dates.map((date) => date.iso),
  );
  assert.deepEqual(
    utc.dates.map((date) => date.offset),
    [0, 0],
  );
  assert.deepEqual(
    pacific.dates.map((date, index) => date.parsedLocal - utc.dates[index].parsedLocal),
    [480 * 60000, 420 * 60000],
  );
});
