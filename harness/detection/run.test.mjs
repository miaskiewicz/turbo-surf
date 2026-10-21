import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { superviseTestChild } from "./child-supervision.mjs";
import { searchSelector } from "./search-plan.mjs";

const runner = fileURLToPath(new URL("run.mjs", import.meta.url));
// Exercise the real runner's configuration writes without launching collectors.
// Only local git reads and the fixture server are real; unexpected execution fails.
const preload = `
import childProcess from 'node:child_process';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { appendFileSync } from 'node:fs';
import { basename } from 'node:path';
import { syncBuiltinESMExports } from 'node:module';
const originalExec = childProcess.execFileSync;
const originalSpawn = childProcess.spawn;
childProcess.execFileSync = (command, args, options) => {
  if (basename(command) === 'native-executable' && args.length === 1 && args[0] === '--build-info')
    return JSON.stringify({ features: { impersonate: true, trustAnchors: true, gpuMetal: true, coretext: true } });
  if (command === 'git') return originalExec(command, args, options);
  throw Error('Unexpected executable in configuration control');
};
childProcess.spawn = (command, args, options) => {
  if (command === 'git') return originalSpawn(command, args, options);
  const native = basename(command) === 'native-executable';
  const script = command === process.execPath ? basename(args[0]) : '';
  if (!native && !['chrome-trace.mjs', 'compare.mjs'].includes(script))
    throw Error('Unexpected collector in configuration control');
  appendFileSync(process.env.DETECTION_CONFIG_CALLS, JSON.stringify({ native, script }) + '\\n');
  const child = new EventEmitter();
  child.stdout = new PassThrough();
  child.stderr = new PassThrough();
  setImmediate(() => { child.stdout.end(); child.stderr.end(); child.emit('close', 0, null); });
  return child;
};
syncBuiltinESMExports();
`;

for (const readySelector of [undefined, "#fixture-ready"]) {
  test(
    `blank probe fixture clears ${readySelector ? "explicit" : "default search"} readiness only`,
    { timeout: 20000 },
    async (t) => {
      const directory = mkdtempSync(join(tmpdir(), "detection-run-probe-config-"));
      let complete = false;
      t.after(() => {
        if (complete) rmSync(directory, { recursive: true, force: true });
        else t.diagnostic(`Retained failed runner configuration control: ${directory}`);
      });
      const binary = join(directory, "placeholder-binary");
      const probe = join(directory, "probe.js");
      const calls = join(directory, "collector-calls.jsonl");
      const output = join(directory, "capture");
      writeFileSync(binary, "Configuration fixture, not a native executable.");
      writeFileSync(probe, "JSON.stringify({fixture: true})");
      const env = { ...process.env, DETECTION_CONFIG_CALLS: calls };
      delete env.NODE_OPTIONS;
      delete env.NODE_PATH;
      const result = await superviseTestChild(
        t,
        process.execPath,
        [
          "--import",
          `data:text/javascript;base64,${Buffer.from(preload).toString("base64")}`,
          runner,
          "--live",
          "--binary",
          binary,
          "--url",
          "http://127.0.0.1:1/",
          "--probe",
          probe,
          "--out",
          output,
          ...(readySelector ? ["--ready-selector", readySelector] : []),
        ],
        { timeoutMs: 15000, spawnOptions: { env } },
      );
      assert.equal(result.code, 0, result.stderr);
      assert.equal(result.signal, null);
      assert.equal(result.error, null);
      assert.equal(result.timedOut, false);
      assert.equal(result.aborted, false);
      const read = (name) => JSON.parse(readFileSync(join(output, name), "utf8"));
      const baseline = read("baseline-config.json");
      const instrumented = read("instrumented-config.json");
      const isolated = read("probe-config.json");
      assert.equal(baseline.readySelector, readySelector ?? searchSelector);
      assert.equal(instrumented.readySelector, baseline.readySelector);
      assert.ok(baseline.steps.length > 0);
      assert.deepEqual(instrumented.steps, baseline.steps);
      assert.equal(isolated.probeOnly, true);
      assert.equal(isolated.fixtureHtml, "<!doctype html><html><head></head><body></body></html>");
      assert.equal(isolated.readySelector, "");
      assert.equal(isolated.dismissSelector, "");
      assert.deepEqual(isolated.steps, []);
      assert.deepEqual(isolated.probes, baseline.probes);
      for (const key of ["pageBudgetMs", "waitMs", "observationMs", "inputCadenceMs"])
        assert.equal(isolated[key], baseline[key], `${key} must remain unchanged`);
      const observed = readFileSync(calls, "utf8")
        .trim()
        .split("\n")
        .map((line) => JSON.parse(line));
      assert.equal(observed.length, 7);
      assert.equal(observed.filter((row) => row.native).length, 3);
      assert.equal(observed.filter((row) => row.script === "chrome-trace.mjs").length, 3);
      assert.equal(read("manifest.json").runs.length, 7);
      complete = true;
    },
  );
}
