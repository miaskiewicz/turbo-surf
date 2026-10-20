import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { validateSteps } from "./validate-steps.mjs";

const run = fileURLToPath(new URL("run.mjs", import.meta.url));
const invalid = [
  null,
  {},
  "type",
  [null],
  [[]],
  [{}],
  [{ type: "x", wait: 0 }],
  [{ unknown: "x" }],
  [{ type: 123 }],
  [{ type: false }],
  [{ type: null }],
  [{ type: {} }],
  [{ wait: -1 }],
  [{ wait: 0.5 }],
  [{ wait: 60001 }],
  [{ wait: "0" }],
  [{ move: null }],
  [{ move: "#q" }],
  [{ move: [] }],
  [{ move: {} }],
  [{ move: { selector: 1 } }],
  [{ move: { selector: "" } }],
  [{ move: { selector: "#q", extra: true } }],
  ...["click", "focus", "press"].flatMap((action) => [
    [{ [action]: null }],
    [{ [action]: 1 }],
    [{ [action]: "" }],
  ]),
  JSON.parse('[{"__proto__":"x"}]'),
];

test("step operand boundaries preserve empty typing and zero waits", () => {
  const steps = [
    { type: "" },
    { type: "abc" },
    { wait: 0 },
    { wait: 60000 },
    { move: { selector: "#q" } },
    { click: "#q" },
    { focus: "#q" },
    { press: "Enter" },
  ];
  const before = JSON.stringify(steps);
  assert.doesNotThrow(() => validateSteps(steps));
  assert.doesNotThrow(() => validateSteps([]));
  assert.equal(JSON.stringify(steps), before);
  for (const steps of invalid) assert.throws(() => validateSteps(steps), TypeError);
  assert.throws(
    () =>
      validateSteps([{ move: Object.assign(Object.create({ selector: "#q" }), { other: true }) }]),
    TypeError,
  );
  for (const wait of [NaN, Infinity, -Infinity, Number.MAX_SAFE_INTEGER + 1])
    assert.throws(() => validateSteps([{ wait }]), TypeError);
});

test("actual driver rejects bad operands before artifacts or backend launch", () => {
  const directory = mkdtempSync(join(tmpdir(), "detection-invalid-steps-"));
  try {
    for (const [index, steps] of invalid.entries()) {
      const path = join(directory, `steps-${index}.json`);
      const output = join(directory, `capture-${index}`);
      writeFileSync(path, JSON.stringify(steps));
      const result = spawnSync(
        process.execPath,
        [
          run,
          "--live",
          "--binary",
          process.execPath,
          "--steps",
          path,
          "--url",
          "http://127.0.0.1:1/",
          "--out",
          output,
          "--chrome-path",
          join(directory, "must-not-launch-chrome"),
        ],
        { encoding: "utf8", timeout: 10000 },
      );
      assert.equal(result.error, undefined, `case ${index}: ${result.error}`);
      assert.equal(result.status, 1, `case ${index}: ${result.stderr}`);
      assert.match(
        result.stderr,
        /TypeError: (Steps must be an array|Step [0-9]+ (must have one supported action|has an invalid .+ operand))/,
      );
      assert.equal(existsSync(output), false, `case ${index} created capture artifacts`);
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
