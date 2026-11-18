"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");
const { spawnSync } = require("node:child_process");
const test = require("node:test");

function fixture(t) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "surf-reporter-contract-"));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  return directory;
}

function run(args, options = {}) {
  const env = { ...process.env };
  delete env.NODE_TEST_CONTEXT;
  delete env.NODE_OPTIONS;
  Object.assign(env, { TURBO_SURF_JS_COVERAGE_TAP: "1", ...options });
  return spawnSync(process.execPath, [path.join(__dirname, "run-node-tests.mjs"), ...args], {
    env,
    encoding: "utf8",
    timeout: 10000,
  });
}

test("outer TAP reporting preserves nested default-reporter AggregateError diagnostics", (t) => {
  const directory = fixture(t);
  const inner = path.join(directory, "inner.test.mjs");
  const outer = path.join(directory, "outer.test.mjs");
  fs.writeFileSync(
    inner,
    `import test from 'node:test';test('nested failure',()=>{throw new AggregateError([new Error('genuine nested cleanup reason')],'outer aggregate envelope')});`,
  );
  fs.writeFileSync(
    outer,
    `import test from 'node:test';import assert from 'node:assert/strict';import {spawnSync} from 'node:child_process';test('nested diagnostic contract',()=>{const env={...process.env};delete env.NODE_TEST_CONTEXT;const child=spawnSync(process.execPath,['--test',${JSON.stringify(inner)}],{env,encoding:'utf8'});assert.equal(child.status,1);assert.ok(child.stdout.includes('genuine nested cleanup reason'),'nested default-reporter diagnostic must remain present');assert.equal(process.env.TURBO_SURF_JS_COVERAGE_TAP,undefined);});`,
  );
  const result = run(["--test", outer]);
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.match(result.stdout, /^TAP version 13/m);
  assert.match(result.stdout, /# pass 1/);
});

test("ordinary test commands retain their reporter, arguments and inherited Node options", (t) => {
  const directory = fixture(t);
  const file = path.join(directory, "ordinary.test.mjs");
  fs.writeFileSync(
    file,
    `import test from 'node:test';import assert from 'node:assert/strict';test('ordinary command',()=>{assert.equal(process.env.NODE_OPTIONS,'--no-warnings');assert.equal(process.env.TURBO_SURF_JS_COVERAGE_TAP,undefined);});`,
  );
  const result = run(["--test", "--test-timeout=1234", file], {
    TURBO_SURF_JS_COVERAGE_TAP: "0",
    NODE_OPTIONS: "--no-warnings",
  });
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.doesNotMatch(result.stdout, /^TAP version 13/m);
  assert.match(result.stdout, /ordinary command/);
});
