import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { findConfig, loadConfig, normalizeConfig, matchAny } from "./config.mjs";
import { executionOptions, validateTimeout } from "./attempt-process.mjs";

const cli = fileURLToPath(new URL("./bin/turbo-surf-test.mjs", import.meta.url));
const loader = new URL("./loader.mjs", import.meta.url).href;
let nativeAvailable = false;
try {
  createRequire(import.meta.url)("../crates/turbo-surf-napi/index.js");
  nativeAvailable = true;
} catch {}

function fixture(callback) {
  const directory = mkdtempSync(join(tmpdir(), "surf-loader-compatibility-"));
  const file = (name, source) => {
    const filename = join(directory, name);
    mkdirSync(dirname(filename), { recursive: true });
    writeFileSync(filename, source);
    return filename;
  };
  const run = (args, env = {}) => {
    const environment = { ...process.env, NODE_OPTIONS: "", ...env };
    delete environment.NODE_TEST_CONTEXT;
    return spawnSync(process.execPath, args, {
      cwd: directory,
      env: environment,
      encoding: "utf8",
      timeout: 15000,
    });
  };
  try {
    return callback({ directory, file, run });
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

function successful(result) {
  assert.equal(result.error, undefined);
  assert.equal(result.status, 0, result.stderr);
  return result.stdout;
}

test(
  "Node module.register loads the actual compatibility loader and resolves real shim and typed imports",
  { skip: !nativeAvailable },
  () => {
    fixture((f) => {
      const typed = f.file(
        "value.ts",
        "export const value: number = 42; export const file=__filename; export const folder=__dirname;",
      );
      f.file(
        "element.tsx",
        "const React={createElement:(tag,props,...children)=>({tag,children})};export default <b>ok</b>;",
      );
      f.file("nested/index.ts", "export default 'directory';");
      const module = f.file(
        "value.mts",
        "export default {value: 'module',hasRequire:typeof require!=='undefined'};",
      );
      const common = f.file("value.cts", "const value: string='commonjs';module.exports={value};");
      const script = f.file(
        "entry.mjs",
        `
      import assert from 'node:assert/strict';
      import {register} from 'node:module';
      register(${JSON.stringify(loader)}, import.meta.url);
      const [test,playwright,core]=await Promise.all(['@playwright/test','playwright','playwright-core'].map(name=>import(name)));
      assert.equal(test,playwright);assert.equal(test,core);
      assert.equal(typeof test.test,'function');assert.equal(typeof test.expect,'function');
      assert.equal(await import('node:path'),await import('node:path'));
      const value=await import('./value.js');assert.equal(value.value,42);
      assert.equal(value.file,${JSON.stringify(realpathSync(typed))});assert.equal(value.folder,${JSON.stringify(realpathSync(f.directory))});
      assert.deepEqual((await import('./element.js')).default,{tag:'b',children:['ok']});
      assert.equal((await import('./nested')).default,'directory');
      assert.deepEqual((await import(${JSON.stringify(pathToFileURL(module).href)})).default,{value:'module',hasRequire:false});
      assert.deepEqual((await import(${JSON.stringify(pathToFileURL(common).href)})).default,{value:'commonjs'});
      assert.equal((await import('data:text/javascript,export default 7')).default,7);
      await assert.rejects(import('data:text/javascript,import "./missing"'));
      console.log('actual compatibility loader passed');
    `,
      );
      assert.match(successful(f.run([script])), /actual compatibility loader passed/);
    });
  },
);

test("CLI reports missing config and unknown project without starting suites", () => {
  fixture((f) => {
    const missing = f.run([cli]);
    assert.equal(missing.status, 2);
    assert.match(missing.stderr, /no playwright.config.* found/);
    const config = f.file("playwright.config.mjs", "export default {projects:[{name:'one'}]};");
    const unknown = f.run([cli, "--config", config, "--project", "missing"]);
    assert.equal(unknown.status, 2);
    assert.match(unknown.stderr, /no projects matched.*missing/);
    assert.doesNotMatch(unknown.stdout, /# tests/);
  });
});

test("CLI rejects malformed global hooks and still invokes teardown after setup fails", () => {
  fixture((f) => {
    const marker = f.file(
      "teardown.mjs",
      "import {writeFileSync} from 'node:fs';export default ()=>writeFileSync(new URL('./cleaned',import.meta.url),'yes');",
    );
    const config = f.file(
      "playwright.config.mjs",
      `export default {globalSetup:'./bad.mjs',globalTeardown:${JSON.stringify(marker)}};`,
    );
    f.file("bad.mjs", "export default 7;");
    const result = f.run([cli, "--config", config]);
    assert.equal(result.status, 1);
    assert.match(result.stderr, /Global setup\/teardown must export a function/);
    assert.equal(
      f.run(["-e", "process.exit(require('node:fs').readFileSync('cleaned','utf8')==='yes'?0:1)"])
        .status,
      0,
    );
  });
});

test("CLI skips dependent projects after failure and renders escaped HTML failure rows", () => {
  fixture((f) => {
    f.file(
      "failing/example.spec.mjs",
      "import test from 'node:test';import assert from 'node:assert/strict';test('failure <unsafe>&',()=>{assert.equal(process.env.TURBO_SHIM_BASE_URL,'http://127.0.0.1:1234');assert.equal(process.env.TURBO_SHIM_LOCALE,'pl-PL');assert.equal(process.env.TURBO_SHIM_USER_AGENT,'coverage-agent');assert.equal(process.env.TURBO_SHIM_TESTID_ATTR,'qa');assert.equal(process.env.TURBO_PW_PROJECT_NAME,'first');throw Error('expected fixture failure')});test.skip('skip <unsafe>',()=>{});",
    );
    f.file("dependent/example.spec.mjs", "throw Error('dependent suite must not execute');");
    const report = join(f.directory, "report.html");
    const config = f.file(
      "playwright.config.mjs",
      `export default {workers:1,use:{baseURL:'http://127.0.0.1:1234',locale:'pl-PL',userAgent:'coverage-agent',testIdAttribute:'qa'},reporter:[['html',{outputFile:${JSON.stringify(report)}}]],projects:[{name:'first',testDir:'./failing'},{name:'dependent',testDir:'./dependent',dependencies:['first']}]};`,
    );
    const result = f.run([cli, "--config", config]);
    assert.equal(result.status, 1);
    assert.match(result.stdout, /expected fixture failure/);
    assert.match(result.stdout, /dependent.*skipped because a dependency failed/);
    assert.doesNotMatch(result.stderr, /dependent suite must not execute/);
    const html = successful(
      f.run(["-e", "console.log(require('node:fs').readFileSync('report.html','utf8'))"]),
    );
    assert.match(html, /1 failed/);
    assert.match(html, /1 skipped/);
    assert.match(html, /failure &lt;unsafe&gt;&amp;/);
    assert.match(html, /skip &lt;unsafe&gt;/);
    assert.doesNotMatch(html, /<unsafe>/);
  });
});

test("config missing/default/named exports and reporter values retain their documented shapes", async () => {
  const directories = [];
  try {
    const directory = mkdtempSync(join(tmpdir(), "surf-config-shapes-"));
    directories.push(directory);
    assert.equal(findConfig(directory), null);
    const named = join(directory, "named.mjs");
    writeFileSync(named, "export const config={workers:1,reporter:[17,['tap']]};");
    assert.deepEqual((await loadConfig(named)).reporter, [["17"], ["tap"]]);
    const direct = join(directory, "direct.mjs");
    writeFileSync(direct, "export const workers=1;export const reporter=17;");
    assert.deepEqual((await loadConfig(direct)).reporter, [["17"]]);
    assert.equal(normalizeConfig(null, directory).projects[0].name, "default");
    const normalized = normalizeConfig(
      { workers: 1, projects: [{ testDir: "" }], webServer: [{ command: "unused" }] },
      directory,
    );
    assert.equal(normalized.projects[0].testDir, directory);
    assert.equal(normalized.projects[0].name, "default");
    assert.equal(normalized.webServer[0].timeout, 60000);
    assert.deepEqual(normalizeConfig({ reporter: 17 }, directory).reporter, [["17"]]);
    assert.equal(matchAny("x.spec.mjs", ["*.spec.mjs"]), true);
  } finally {
    for (const directory of directories) rmSync(directory, { recursive: true, force: true });
  }
});

test("attempt options reject invalid budgets before dispatching a worker", () => {
  for (const timeout of [-1, NaN, Infinity]) {
    assert.throws(() => validateTimeout(timeout), /non-negative finite number/);
    assert.throws(
      () => executionOptions([{ execution: { timeout } }]),
      /non-negative finite number/,
    );
  }
  for (const retries of [-1, 0.5]) {
    assert.throws(
      () => executionOptions([{ execution: { retries } }]),
      /Retries must be a non-negative integer/,
    );
  }
  assert.equal(validateTimeout(0), 0);
});

test("isolated execution without a file entry rejects before worker startup", () => {
  fixture((f) => {
    const source = new URL("./attempt-process.mjs", import.meta.url).href;
    const code = `import assert from 'node:assert/strict';import {executeIsolated,closeAttemptProcesses} from ${JSON.stringify(source)};await assert.rejects(executeIsolated({id:'missing',name:'missing'},{},{timeout:100,retries:0}),/require a test file entry point/);await closeAttemptProcesses();console.log('rejected without worker');`;
    assert.match(successful(f.run(["--input-type=module", "-e", code])), /rejected without worker/);
  });
});

test(
  "isolated workers report startup failure and abrupt test exit without hanging cleanup",
  { skip: !nativeAvailable },
  () => {
    fixture((f) => {
      const config = f.file(
        "playwright.config.mjs",
        "export default {workers:1,reporter:'tap',testDir:'specs'};",
      );
      f.file(
        "specs/startup.spec.mjs",
        "import {test} from '@playwright/test';if(process.env.TURBO_PW_ATTEMPT_WORKER==='1')process.exit(7);test('startup exit',()=>{});",
      );
      const startup = f.run([cli, "--config", config]);
      assert.equal(startup.status, 1);
      assert.match(startup.stdout, /Test worker exited during startup/);
      f.file(
        "specs/startup.spec.mjs",
        "import {test} from '@playwright/test';test('abrupt exit',()=>process.exit(9));",
      );
      const abrupt = f.run([cli, "--config", config]);
      assert.equal(abrupt.status, 1);
      assert.match(abrupt.stdout, /Test attempt worker exited unexpectedly/);
      assert.doesNotMatch(abrupt.stdout, /cleanup timed out/);
    });
  },
);
