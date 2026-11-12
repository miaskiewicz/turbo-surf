import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const shim = new URL("./index.mjs", import.meta.url).href;
const register = fileURLToPath(new URL("./register.mjs", import.meta.url));
const cli = fileURLToPath(new URL("./bin/turbo-surf-test.mjs", import.meta.url));
function suite(source, config) {
  const dir = mkdtempSync(join(tmpdir(), "surf318-"));
  const file = join(dir, "attempt.spec.mjs");
  const marker = join(dir, "events");
  writeFileSync(
    file,
    `import {test,expect} from ${JSON.stringify(shim)};\nimport {appendFileSync} from 'node:fs';\nconst mark=s=>appendFileSync(${JSON.stringify(marker)},s+'\\n');\n${source}`,
  );
  const env = { ...process.env };
  delete env.NODE_TEST_CONTEXT;
  delete env.NODE_OPTIONS;
  let args = ["--import", register, "--test", "--test-force-exit", file];
  if (config) {
    const cfg = join(dir, "playwright.config.mjs");
    writeFileSync(cfg, `export default ${JSON.stringify({ ...config, testDir: dir })}`);
    args = [cli, "--config", cfg];
  }
  const result = spawnSync(process.execPath, args, { env, encoding: "utf8", timeout: 15000 });
  let events = "";
  try {
    events = readFileSync(marker, "utf8");
  } catch {}
  return { ...result, events };
}

test("runtime timeout kills the old callback before a fresh retry starts", () => {
  const r = suite(
    `test.describe.configure({retries:1});test('deadline',async({},info)=>{mark('start:'+info.retry); info.setTimeout(info.retry===0?35:1000); if(info.retry===0){await new Promise(r=>setTimeout(r,180));mark('late');}else{await new Promise(r=>setTimeout(r,220));mark('retry-done');info.setTimeout(1000);}});`,
  );
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.equal(r.events, "start:0\nstart:1\nretry-done\n");
});
test("config timeout fails without allowing the timed out body to finish", () => {
  const r = suite(
    `test('deadline',async()=>{await new Promise(r=>setTimeout(r,140));mark('late');});`,
    { timeout: 30 },
  );
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.equal(r.events, "");
  assert.match(r.stdout + r.stderr, /Timeout.*30ms/i);
});
test("retries get fresh module and fixture state and attempt metadata", () => {
  const r = suite(
    `let state=0;const t=test.extend({value:async({},use,info)=>{mark('setup:'+info.retry);await use(++state);mark('cleanup:'+info.retry);}});t.describe.configure({retries:1});t('retry',async({value},info)=>{mark('body:'+info.retry+':'+value);expect(value).toBe(1);expect(info.retry).toBe(1);});`,
  );
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.equal(r.events, "setup:0\nbody:0:1\ncleanup:0\nsetup:1\nbody:1:1\ncleanup:1\n");
});
test("config retries honor final failure and successful tests retain worker state", () => {
  const r = suite(
    `let n=0;test.beforeAll(()=>{mark('before');});test.afterAll(()=>{mark('after');});test('one',()=>{n++;});test('two',()=>{expect(n).toBe(1);});test('failed',({},info)=>{mark('attempt:'+info.retry);throw new Error('still broken');});`,
    { retries: 1 },
  );
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.match(r.events, /attempt:0\n/);
  assert.match(r.events, /attempt:1\n/);
  assert.doesNotMatch(r.stdout + r.stderr, /expected.*1.*got.*0/i);
});

test("nested suite plans keep identity and overrides independent of registration scheduling", () => {
  const r = suite(
    `test.describe('nested',()=>{test.describe.configure({retries:1,timeout:1000});test('same',({},info)=>{mark('nested:'+info.retry);expect(info.retry).toBe(1);});});test('same',()=>mark('root'));`,
  );
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.events, /nested:0\nnested:1\n/);
  assert.match(r.events, /root\n/);
});

test("a synchronous runaway body is terminated at the configured deadline", () => {
  const r = suite(`test('cpu',()=>{mark('entered');while(true){};});`, { timeout: 35 });
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.equal(r.events, "entered\n");
  assert.match(r.stdout + r.stderr, /Timeout.*35ms/i);
});
test("worker cleanup failure fails the run and keeps nested diagnostics", () => {
  const r = suite(`test.afterAll(()=>{throw new Error('cleanup canary');});test('body',()=>{});`);
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.match(r.stdout + r.stderr, /cleanup canary/);
});

test("runtime timeout updates can extend or disable the current deadline", () => {
  const r = suite(
    `test.describe.configure({timeout:35});test('extend',async()=>{test.setTimeout(300);await new Promise(r=>setTimeout(r,80));mark('extended');});test('disable',async({},info)=>{info.setTimeout(0);await new Promise(r=>setTimeout(r,80));mark('disabled');});test('slow',async({},info)=>{info.slow();await new Promise(r=>setTimeout(r,65));mark('slow');});`,
  );
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.equal(r.events, "extended\ndisabled\nslow\n");
});
