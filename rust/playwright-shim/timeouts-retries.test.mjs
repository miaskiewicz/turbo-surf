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
  const r = suite(`test('cpu',({},info)=>{mark('entered');info.setTimeout(35);while(true){};});`);
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
    `test('extend',async()=>{test.setTimeout(35);test.setTimeout(300);await new Promise(r=>setTimeout(r,80));mark('extended');});test('disable',async({},info)=>{info.setTimeout(35);info.setTimeout(0);await new Promise(r=>setTimeout(r,80));mark('disabled');});test('slow',async({},info)=>{info.setTimeout(35);info.slow();await new Promise(r=>setTimeout(r,65));mark('slow');});`,
  );
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.equal(r.events, "extended\ndisabled\nslow\n");
});

test("worker dispatch scheduling does not consume the test execution budget", () => {
  const r = suite(
    `if(process.env.TURBO_PW_ATTEMPT_WORKER==='1')process.prependListener('message',m=>{if(m.kind==='run'){const end=Date.now()+90;while(Date.now()<end){}}});test('dispatch',()=>mark('ran'));`,
    { timeout: 35 },
  );
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.equal(r.events, "ran\n");
});

test("worker hook timeout override does not leak into the following test body", () => {
  const r = suite(
    `test.beforeAll(async()=>{test.setTimeout(1000);mark('hook');await new Promise(r=>setTimeout(r,150));});test('body',async({},info)=>{mark('body:'+info.timeout);await new Promise(r=>setTimeout(r,250));mark('late');});`,
    { timeout: 100 },
  );
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.equal(r.events, "hook\nbody:100\n");
  assert.match(r.stdout + r.stderr, /Timeout.*100ms/i);
});

test("time spent in an extended worker hook does not consume the test body budget", () => {
  const r = suite(
    `test.beforeAll(async()=>{test.setTimeout(1000);await new Promise(r=>setTimeout(r,150));mark('hook');});test('body',async({},info)=>{mark('body:'+info.timeout);await new Promise(r=>setTimeout(r,40));mark('done');});`,
    { timeout: 100 },
  );
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.equal(r.events, "hook\nbody:100\ndone\n");
});

test("separate worker hooks each retain their configured timeout budget", () => {
  const r = suite(
    `test.beforeAll(async()=>{await new Promise(r=>setTimeout(r,70));mark('first');});test.beforeAll(async()=>{await new Promise(r=>setTimeout(r,70));mark('second');});test('body',()=>mark('body'));`,
    { timeout: 100 },
  );
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.equal(r.events, "first\nsecond\nbody\n");
});

test("beforeEach and the body share one elapsed test budget", () => {
  const r = suite(
    `test.beforeEach(async()=>{await new Promise(r=>setTimeout(r,70));mark('before');});test('body',async()=>{mark('body');await new Promise(r=>setTimeout(r,70));mark('late');});`,
    { timeout: 100 },
  );
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.equal(r.events, "before\nbody\n");
  assert.match(r.stdout + r.stderr, /Timeout.*100ms/i);
});

test("beforeEach runtime timeout updates persist through the body", () => {
  const r = suite(
    `test.beforeEach(async()=>{test.setTimeout(500);await new Promise(r=>setTimeout(r,100));mark('before');});test('body',async({},info)=>{mark('body:'+info.timeout);await new Promise(r=>setTimeout(r,100));mark('done');});`,
    { timeout: 100 },
  );
  assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.equal(r.events, "before\nbody:500\ndone\n");
});
