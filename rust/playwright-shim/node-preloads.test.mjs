import assert from "node:assert/strict";
import { join } from "node:path";
import { mkdirSync, mkdtempSync, writeFileSync, symlinkSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import test from "node:test";
import { pathToFileURL } from "node:url";
import { nodePreloadArgs } from "./node-preloads.mjs";

test("explicit import and require preloads retain order and supported spellings", () => {
  assert.deepEqual(
    nodePreloadArgs(
      [
        "--import",
        "tsx",
        "--require=./bootstrap.cjs",
        "-r",
        "./second.cjs",
        "--import=./third.mjs",
      ],
      "",
    ),
    [
      "--import",
      "tsx",
      "--require",
      "./bootstrap.cjs",
      "--require",
      "./second.cjs",
      "--import",
      "./third.mjs",
    ],
  );
});
test("environment preloads precede argv and exact duplicates retain distinct mechanisms", () => {
  assert.deepEqual(
    nodePreloadArgs(
      ["--import", "tsx", "--require", "tsx", "--import=./last.mjs"],
      "--require=node:fs --import tsx",
    ),
    ["--require", "node:fs", "--import", "tsx", "--require", "tsx", "--import", "./last.mjs"],
  );
});
test("NODE_OPTIONS uses Node double quotes rather than shell evaluation", () => {
  const options =
    '--import="/tmp/loader with spaces.mjs" --require "/tmp/quote\\"loader.cjs" --import="$(not-a-shell-command)"';
  assert.deepEqual(nodePreloadArgs([], options), [
    "--import",
    "/tmp/loader with spaces.mjs",
    "--require",
    '/tmp/quote"loader.cjs',
    "--import",
    "$(not-a-shell-command)",
  ]);
});
test("ordinary backslashes and apostrophes outside double quotes remain literal", () => {
  assert.deepEqual(
    nodePreloadArgs(
      [],
      String.raw`--import=C:\loaders\entry.mjs --require=/tmp/it's.cjs --import="node:pa\th"`,
    ),
    [
      "--import",
      String.raw`C:\loaders\entry.mjs`,
      "--require",
      "/tmp/it's.cjs",
      "--import",
      "node:path",
    ],
  );
});
test("test runner flags and their values never become preloads", () => {
  assert.deepEqual(
    nodePreloadArgs(
      [
        "--test",
        "--test-name-pattern",
        "--require=not-a-preload",
        "--test-reporter",
        "--import=not-a-preload",
        "--test-reporter-destination",
        "output.txt",
        "--inspect=127.0.0.1:9229",
        "--eval",
        "--import=also-not-a-preload",
        "--import",
        "tsx",
      ],
      "",
    ),
    ["--import", "tsx"],
  );
});
test("positional CLI arguments and option terminators stop preload scanning", () => {
  assert.deepEqual(
    nodePreloadArgs(
      ["--import", "tsx", "cli.mjs", "--config", "config.mjs", "--import", "not-a-node-flag"],
      "",
    ),
    ["--import", "tsx"],
  );
  assert.deepEqual(
    nodePreloadArgs(["--require", "node:fs", "--", "--import", "not-a-node-flag"], ""),
    ["--require", "node:fs"],
  );
});
test("only canonical excluded import paths are removed", () => {
  const directory = mkdtempSync(join(tmpdir(), "surf-preload-paths-"));
  try {
    const own = join(directory, "surf registration");
    const elsewhere = join(directory, "other");
    mkdirSync(own);
    mkdirSync(elsewhere);
    const registration = join(own, "register.mjs");
    const other = join(elsewhere, "register.mjs");
    const linked = join(directory, "linked.mjs");
    writeFileSync(registration, "");
    writeFileSync(other, "");
    symlinkSync(registration, linked);
    assert.deepEqual(
      nodePreloadArgs(
        [
          "--import",
          registration,
          "--import",
          pathToFileURL(registration).href,
          "--import",
          linked,
          "--import",
          other,
          "--require",
          registration,
          "--import",
          "tsx",
        ],
        "",
        [registration],
      ),
      ["--import", other, "--require", registration, "--import", "tsx"],
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
test("malformed quoted environment options fail explicitly", () => {
  assert.throws(() => nodePreloadArgs([], '--import "unterminated'), /quot|unterminated/i);
});
test("empty preload selection does not forward unrelated inherited Node settings", () => {
  assert.deepEqual(
    nodePreloadArgs(
      ["--test", "--test-force-exit", "--experimental-strip-types", "--max-old-space-size=2048"],
      "--trace-warnings --conditions=testing",
    ),
    [],
  );
});

test("unrelated value-taking Node options do not hide later explicit preloads", () => {
  assert.deepEqual(
    nodePreloadArgs(
      ["--stack-size", "2048", "--import", "tsx"],
      "--max-old-space-size 1024 --require node:fs",
    ),
    ["--require", "node:fs", "--import", "tsx"],
  );
});
